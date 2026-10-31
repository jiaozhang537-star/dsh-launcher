//! 启动编排内核（P7.5 S2 / FR-26）：GUI 命令与无头 CLI 共用的完整启动链。
//!
//! 抽取自 lib.rs `instance_launch`（2026-10-24）：事件百分比、检查点取消、先校验
//! 后停进程的顺序全部原样保留；差异只在两个出口——
//! - 进度：GUI 走 `launch-progress` 事件（回调里 emit），CLI 走 stderr 行；
//! - 拉起 IO：GUI = 管道 + 控制台事件；CLI = 实例 `logs/console.log` 落盘。
//!
//! 「实例在跑就先停再按钉定端口拉起」的重启语义内建于此——CLI 的 `restart`
//! 与 `launch` 走的是同一条链，端口不漂移。

use serde::Serialize;

use crate::{instances, launch_cancel, llm_bridge, node_runtime, precheck, process, snapshot};

/// 无头模式的日志落点（就绪轮询从 `start_offset` 起步）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadlessLog {
    pub path: String,
    pub start_offset: u64,
}

/// 启动链产物：进程信息 + 校验链报告 + 配置对账。GUI 前端契约 = 旧 `LaunchReply`
/// 的 camelCase 形状（`log` 仅无头模式有值且跳过 None 序列化，GUI JSON 逐字段不变）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub instance_id: String,
    pub port: u16,
    pub pid: u32,
    pub report: precheck::Report,
    pub reconcile: snapshot::Reconcile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<HeadlessLog>,
}

/// 取消本次启动的返回文案（前端不据此判断，只用于日志与兜底展示）。
pub(crate) const CANCELLED_MESSAGE: &str = "已取消启动";

/// 链上检查点：本代启动已取消就中断本次启动。
fn ensure_not_cancelled(instance_id: &str, gen: u64) -> Result<(), String> {
    if launch_cancel::is_cancelled(instance_id, gen) {
        Err(CANCELLED_MESSAGE.to_string())
    } else {
        Ok(())
    }
}

/// 完整启动链（阻塞执行）。
///
/// - `app = Some`：GUI 模式（进度同时走 `progress` 回调；拉起走管道）；
/// - `app = None`：无头模式（拉起走日志文件落盘，返回 `log` 位置）；
/// - `force = true`：用户看过校验报告后显式放行（错误级条目不再拦截）；
/// - 校验被拦截时的 Err 是**报告的 JSON 串**（与 GUI 契约一致，由前端/CLI 解析）。
pub fn launch_pipeline(
    app: Option<&tauri::AppHandle>,
    instance_id: &str,
    force: bool,
    progress: &mut dyn FnMut(u8, &str),
) -> Result<LaunchResult, String> {
    // 本次启动开始：代际自增（对抗审查 R1 P1-03）。旧代标记刻意保留——
    // 「取消后立刻再启动」时旧链按旧代查询仍命中取消，本链用新代互不干扰。
    let launch_gen = launch_cancel::begin(instance_id);
    // 运行环境自愈：没有可用 Node 就先自动代管一个已知良好版本（与安装链
    // 同一套兜底），预检的 Node 项于是成为"结论"而不是新手要自己解决的任务。
    if !node_runtime::node_available() {
        progress(5, "正在准备 Node 运行环境…");
        node_runtime::ensure_default_node(
            &mut |event| {
                let detail = match event {
                    node_runtime::EnsureEvent::Downloading { version } => {
                        format!("正在自动代管 Node v{version}…")
                    }
                    node_runtime::EnsureEvent::Bytes { received, total } => format!(
                        "正在自动代管 Node（{} / {}）",
                        node_runtime::fmt_size(received),
                        node_runtime::fmt_size(total)
                    ),
                };
                progress(6, &detail);
            },
            None,
        )?;
        ensure_not_cancelled(instance_id, launch_gen)?;
    }
    progress(8, "正在校验运行环境…");
    let report = precheck::run(instance_id)?;
    ensure_not_cancelled(instance_id, launch_gen)?;
    if report.blocked && !force {
        return Err(serde_json::to_string(&report)
            .unwrap_or_else(|_| r#"{"items":[],"blocked":true}"#.into()));
    }
    progress(45, "正在对账实例配置…");
    let state = process::process_state(instance_id)?;
    let baseline = snapshot::load(instance_id);
    let reconcile = match snapshot::collect(instance_id) {
        Ok(current) => {
            let mut reconcile = snapshot::diff(baseline.as_ref(), &current);
            // 基线落盘失败不阻断启动，但要如实说明：下次启动会退化成"首次"
            // （只报构成、不报新增），用户能看出对账没生效。
            if let Err(err) = snapshot::save(instance_id, &current) {
                reconcile.note = Some(format!("实例快照未落盘：{err}"));
            }
            reconcile
        }
        Err(err) => snapshot::Reconcile {
            note: Some(format!("实例配置对账失败：{err}")),
            ..Default::default()
        },
    };
    ensure_not_cancelled(instance_id, launch_gen)?;
    progress(55, "正在挂载全局技能目录…");
    instances::ensure_skills_link(instance_id)?;
    ensure_not_cancelled(instance_id, launch_gen)?;
    progress(55, "正在同步模型配置…");
    if state.running {
        progress(64, "实例正在运行，正在重启实例…");
        process::stop_for_restart(instance_id)?;
    }
    // 配置同步是「不改实例就跑不起来」的必要一步，取消不半途跳过它：取消在
    // 这之后落地时，改动已写入——它是幂等的，下次启动会照常再做一遍。
    llm_bridge::sync_instance(instance_id)?;
    ensure_not_cancelled(instance_id, launch_gen)?;
    progress(70, "正在拉起实例进程…");
    let keep_pinned_port = state.running;
    // 拉起那一步把 id 移进阻塞调用；取消兜底还要问一次令牌，留一份副本。
    let cancel_id = instance_id.to_string();
    let (outcome, log) = match app {
        Some(app) => (
            process::launch(Some(app), instance_id, keep_pinned_port)?,
            None,
        ),
        None => {
            let headless = process::launch_headless(instance_id, keep_pinned_port)?;
            let log = HeadlessLog {
                path: headless.log_path.display().to_string(),
                start_offset: headless.log_start_offset,
            };
            (headless.outcome, Some(log))
        }
    };
    // spawn 后兜底：取消可能就落在「拉起中」这一小段里——进程已经起来了，就地停掉。
    if launch_cancel::is_cancelled(&cancel_id, launch_gen) {
        process::stop_silently(&cancel_id)?;
        return Err(CANCELLED_MESSAGE.to_string());
    }
    Ok(LaunchResult {
        instance_id: outcome.instance_id,
        port: outcome.port,
        pid: outcome.pid,
        report,
        reconcile,
        log,
    })
}
