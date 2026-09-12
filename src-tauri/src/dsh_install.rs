//! DSH 实例安装器（FR-01）：把官方元包装进实例安装区。
//!
//! R-1 结论的落地：发行物 = npm 元包 `@deepseek-ai/dsh`，无 GitHub Release 便携包。
//! 安装 = 用托管 Node 自带的 npm 执行
//! `npm install --prefix=<实例>/app @deepseek-ai/dsh@<版本> --ignore-scripts`，
//! 避免任意生命周期脚本在用户机器上执行（更安全也更可控）；
//! koffi 平台预编译包经 optionalDependencies 正常落盘（--ignore-scripts 不影响下载），
//! 装后校验其存在，再跑 `bin.js --version` 复验产物可用。
//!
//! 进度口径（计划文件已记录的决策）：npm 无字节级进度，采用阶段级事件
//! Preparing → Installing(带输出尾部) → Verifying → Done。

use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::gateway;
use crate::instances;
use crate::mirror_config;
use crate::node_runtime::{self, CreationFlagsExt};
use crate::registry_meta::DSH_PACKAGE;

/// 实例安装区目录名（位于实例目录下）。
pub const APP_DIR_NAME: &str = "app";

/// 安装阶段事件载荷（Tauri 事件 `instance-install-progress`）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallStage {
    /// 校验前置条件（实例/Node/npm 就位）。
    Preparing,
    /// 无可用 Node 运行时，正在按候选版本自动下载便携版（FR-05 兜底）。
    /// `detail` 为文案；`received/total` 为字节进度（total=0 表示长度未知），
    /// 供前端计算百分比与速度。
    AutoNode {
        detail: String,
        received: u64,
        total: u64,
    },
    /// npm 执行中；detail 为最近一行输出（首个事件携带安装通道）。
    Installing { detail: String },
    /// 产物校验：bin.js / koffi 平台包 / --version 复验。
    Verifying,
    /// 全部完成。
    Done,
}

/// 安装结果。
#[derive(Debug, Clone, Serialize)]
pub struct InstallOutcome {
    #[serde(rename = "instanceId")]
    pub instance_id: String,
    pub version: String,
    /// 可执行入口 `…/@deepseek-ai/dsh/lib/bin.js` 绝对路径。
    pub bin_js: String,
}

/// 实例安装区目录：`<实例目录>/app`。
pub fn app_dir(instance_id: &str) -> Result<PathBuf, String> {
    Ok(instances::instance_dir(instance_id)?.join(APP_DIR_NAME))
}

/// 已安装 DSH 的 CLI 入口；不存在返回 None。
pub fn installed_bin_js(instance_id: &str) -> Result<Option<PathBuf>, String> {
    let candidate = app_dir(instance_id)?
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("lib")
        .join("bin.js");
    Ok(candidate.is_file().then_some(candidate))
}

/// 解析安装用的 Node 运行时，依次尝试：
/// ① 实例覆盖 / 全局默认的托管便携版（配置指向的版本缺失时报错而非静默跳过）；
/// ② 系统 PATH 上满足 FR-05 范围的 Node（npm-cli 由随后的 `npm_cli_of` 统一校验，
///    与 precheck/launch 的系统 Node 口径一致）；
/// ③ 自动下载便携版代管并写为全局默认（FR-05 一键兜底，进度以 AutoNode 阶段外发）。
/// 插件安装链（market_install）复用同一口径。
pub(crate) fn resolve_install_node(
    instance: &instances::Instance,
    progress: &mut dyn FnMut(InstallStage),
) -> Result<PathBuf, String> {
    if let Some(configured) = instance
        .node_version
        .clone()
        .or_else(|| node_runtime::load_config().ok()?.default_node_version)
    {
        return node_runtime::managed_node_exe(&configured).ok_or_else(|| {
            format!("配置指向的托管 Node {configured} 缺 node.exe，请到「设置」页重新下载该版本")
        });
    }

    if let Some(sys) = node_runtime::detect_system_node() {
        if sys.meets_range {
            return Ok(PathBuf::from(sys.path));
        }
        // 版本不合格：不中断，继续走自动代管兜底（错误信息随下载失败一并可见）。
    }

    let exe = node_runtime::ensure_default_node(&mut |event| {
        let (detail, received, total) = match event {
            node_runtime::EnsureEvent::Downloading { version } => (
                format!("未检测到可用的 Node 运行时，开始自动下载便携版 Node v{version}…"),
                0,
                0,
            ),
            node_runtime::EnsureEvent::Bytes { received, total } => (
                format!(
                    "正在自动下载 Node 运行时（{} / {}）",
                    node_runtime::fmt_size(received),
                    node_runtime::fmt_size(total)
                ),
                received,
                total,
            ),
        };
        progress(InstallStage::AutoNode { detail, received, total });
    })?;
    Ok(exe)
}

/// 从 `<node 目录>/node_modules/npm/bin/npm-cli.js` 定位 Node 自带 npm
///（托管便携版与系统安装均遵循此布局；缺失时 fail loud 并指路设置页）。
pub(crate) fn npm_cli_of(node_exe: &Path) -> Result<PathBuf, String> {
    let node_dir = node_exe.parent().ok_or_else(|| "node.exe 路径异常".to_string())?;
    let cli = node_dir
        .join("node_modules")
        .join("npm")
        .join("bin")
        .join("npm-cli.js");
    if cli.is_file() {
        Ok(cli)
    } else {
        Err(format!(
            "该 Node 缺自带 npm（{} 不存在），请到「设置」页下载并代管便携版",
            cli.display()
        ))
    }
}

/// 保留输出尾部用于错误报告与进度展示（插件安装链复用）。
pub(crate) struct OutputTail {
    lines: Vec<String>,
    cap: usize,
    /// 累计收到的输出行数（进度节流的依据）。
    pub total: usize,
}

impl OutputTail {
    pub fn new(cap: usize) -> Self {
        Self {
            lines: Vec::new(),
            cap,
            total: 0,
        }
    }
    pub fn push(&mut self, line: String) {
        self.total += 1;
        if self.lines.len() == self.cap {
            self.lines.remove(0);
        }
        self.lines.push(line);
    }
    pub fn join(&self) -> String {
        self.lines.join("\n")
    }
}

/// 安装官方 DSH 到指定实例。阻塞调用——command 侧放 spawn_blocking。
///
/// DSH 版本号白名单：semver（core 三段数字 + 可选 `-pre` 段，字符集限字母
/// 数字与 `.-`）。npm spec 支持 `file:`/`git+`/URL 形态——不收紧白名单等于
/// 允许「装任意来源的包」，同族 node/source 的版本校验必须在此对齐。
pub fn valid_dsh_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 32
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        && crate::registry_meta::parse_semver_key(version).is_some()
}

/// 幂等性：重复安装同版本 = npm 覆盖重装，成功后覆盖 `instance.dsh_version`。
pub fn install(
    instance_id: &str,
    version: &str,
    progress: &mut dyn FnMut(InstallStage),
) -> Result<InstallOutcome, String> {
    if !valid_dsh_version(version) {
        return Err(format!(
            "非法版本号：{version:?}——需要形如 0.1.1-rc.2 的 semver 版本"
        ));
    }
    progress(InstallStage::Preparing);

    let instance = instances::get_instance(instance_id)?;
    let node_exe = resolve_install_node(&instance, progress)?;
    let npm_cli = npm_cli_of(&node_exe)?;
    let app = app_dir(instance_id)?;
    std::fs::create_dir_all(&app).map_err(|e| format!("创建安装区 {} 失败：{e}", app.display()))?;

    // registry 经统一网关取候选链（FR-18 修订：npmmirror 优先、官方兜底，
    // 优先≠独占——单通道失败自动回落下一通道，业务模块不拼镜像直链）。
    mirror_config::load()?;
    let spec = format!("{DSH_PACKAGE}@{version}");

    let mut attempts: Vec<String> = Vec::new();
    for registry_base in gateway::npm_registry_bases() {
        match run_npm_install(&node_exe, &npm_cli, &app, &spec, registry_base, progress) {
            Ok(()) => break,
            Err(e) => attempts.push(format!("[通道 {registry_base}] {e}")),
        }
    }
    if attempts.len() == gateway::npm_registry_bases().len() {
        return Err(format!(
            "npm install {spec} 在全部通道上失败：\n{}",
            attempts.join("\n---\n")
        ));
    }

    progress(InstallStage::Verifying);
    let bin_js = installed_bin_js(instance_id)?
        .ok_or_else(|| format!("安装完成但未找到 {DSH_PACKAGE}/lib/bin.js——产物异常"))?;

    // koffi 平台预编译包补位校验（R-1：缺它 FFI 层无法加载）。
    let koffi_platform = app
        .join("node_modules")
        .join("@koromix")
        .join("koffi-win32-x64");
    if !koffi_platform.is_dir() {
        return Err(format!(
            "koffi 平台包缺失（{}）——本机平台可选依赖未落盘，请重试安装",
            koffi_platform.display()
        ));
    }

    // 装后复验：bin.js --version 输出必须等于请求版本。
    let out = Command::new(&node_exe)
        .arg(&bin_js)
        .arg("--version")
        .creation_flags_cv()
        .output()
        .map_err(|e| format!("运行 bin.js --version 失败：{e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || !text.contains(version) {
        return Err(format!(
            "装后复验失败：期望包含 {version}，实际输出 {:?}（exit {:?}）\nstderr: {}",
            text,
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    instances::update_instance(instance_id, |i| {
        i.dsh_version = Some(version.to_string());
    })?;

    progress(InstallStage::Done);
    Ok(InstallOutcome {
        instance_id: instance_id.to_string(),
        version: version.to_string(),
        bin_js: bin_js.display().to_string(),
    })
}

/// 在单一 registry 通道上执行一次 npm 安装（阻塞；成功 Ok，失败 Err 带输出尾部）。
///
/// 进度事件：首个事件点名通道（「正在从 {registry} 安装 …」，前端存为常驻行），
/// 之后每 10 行输出推一次最近动作。看门狗 900s 纯兜底（慢而活的安装不误杀）。
fn run_npm_install(
    node_exe: &Path,
    npm_cli: &Path,
    app: &Path,
    spec: &str,
    registry_base: &str,
    progress: &mut dyn FnMut(InstallStage),
) -> Result<(), String> {
    let mut cmd = Command::new(node_exe);
    cmd.arg(npm_cli)
        .args([
            "install",
            &format!("--prefix={}", app.display()),
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            // http 级日志逐条打印每个请求（URL + 耗时）：慢速网络下任务卡
            // 才有「正在干嘛」的依据，静默看门狗也不会误杀慢而活的安装。
            "--loglevel=http",
            spec,
        ])
        .env("npm_config_registry", registry_base)
        .env("npm_config_update_notifier", "false");
    if let Some(node_dir) = node_exe.parent() {
        // npm 内部会再拉起 node；把它放到 PATH 首位保证同版本。
        let prepend = format!("{};{}", node_dir.display(), std::env::var("PATH").unwrap_or_default());
        cmd.env("PATH", prepend);
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags_cv();

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("拉起 npm 失败：{e}"))?;
    // 两条流各自 drain（防管道写满死锁），行经通道汇入主循环。
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let mut readers = Vec::new();
    // stdout/stderr 类型不同，按 trait 对象统一 drain（防管道写满死锁）；
    // 每个流移交给独立线程，因此需要 Send 约束。
    let streams: Vec<Box<dyn std::io::Read + Send>> = vec![
        Box::new(child.stdout.take().ok_or("npm stdout 缺失")?),
        Box::new(child.stderr.take().ok_or("npm stderr 缺失")?),
    ];
    for stream in streams {
        let tx = tx.clone();
        readers.push(thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);

    // 通道可见：首个进度事件点名 npm 正在对话的 registry（镜像挂死时用户
    // 一眼看到该切哪个通道）。
    progress(InstallStage::Installing {
        detail: format!("正在从 {registry_base} 安装 {spec}"),
    });

    // npm 无字节级进度（R-1 决策），但 http 级日志让每个请求都成为输出；
    // 慢网络（国内直连 npmjs）单次安装可合法运行 20-30 分钟且中途有 >10 分钟
    // 的静默段（e2e 实测），npm 自身对死网络有超时+重试会自行报错退出——
    // 因此看门狗只作 900s 纯兜底：连续 900s 无任何输出才杀进程。
    let quiet_limit = Duration::from_secs(900);
    let mut tail = OutputTail::new(30);
    let mut last_output = std::time::Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(line) => {
                last_output = std::time::Instant::now();
                tail.push(line.clone());
                // 每 10 行推一次阶段事件，前端可见任务仍在推进。
                if tail.total % 10 == 0 {
                    progress(InstallStage::Installing {
                        detail: last_meaningful_line(&line),
                    });
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if last_output.elapsed() >= quiet_limit {
                    let _ = child.kill();
                    for handle in readers {
                        let _ = handle.join();
                    }
                    let _ = child.wait();
                    return Err(format!(
                        "npm 超过 15 分钟无任何网络活动，已中止。\
                         常见原因：镜像源不可达、代理或安全软件拦截。\
                         \n{}",
                        tail.join()
                    ));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    for handle in readers {
        handle.join().map_err(|_| "npm 输出读取线程崩溃".to_string())?;
    }
    let status = child.wait().map_err(|e| format!("等待 npm 退出失败：{e}"))?;
    if !status.success() {
        return Err(format!(
            "npm install {spec} 失败（exit {}）：\n{}",
            status.code().unwrap_or(-1),
            tail.join()
        ));
    }
    Ok(())
}

/// 进度事件里的 detail 取最后一条非空行（npm 的空行很多）。
fn last_meaningful_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        "…".to_string()
    } else {
        trimmed.chars().take(160).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_tail_keeps_capacity_and_counts_total() {
        let mut tail = OutputTail::new(3);
        for i in 0..5 {
            tail.push(format!("line-{i}"));
        }
        assert_eq!(tail.total, 5);
        assert_eq!(tail.join(), "line-2\nline-3\nline-4");
    }

    #[test]
    fn meaningful_line_trims_and_caps_length() {
        assert_eq!(last_meaningful_line("   "), "…");
        let long = "x".repeat(300);
        assert_eq!(last_meaningful_line(&long).chars().count(), 160);
    }

    #[test]
    fn missing_instance_and_node_fail_loud_with_guidance() {
        // 未创建实例 → get_instance 报错（不触碰磁盘外的东西）。
        let (_lock, tmp) = crate::instances::tests::sandbox("install-noinst");
        let mut sink = |_stage: InstallStage| {};
        assert!(install("i-missing", "0.1.1-rc.2", &mut sink).is_err());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn dsh_version_whitelist_blocks_npm_spec_shapes() {
        for good in ["0.1.1-rc.2", "0.1.2-alpha.3", "1.2.3"] {
            assert!(valid_dsh_version(good), "{good:?} 应放行");
        }
        for bad in [
            "",
            "banana",
            "1.2",
            "1.2.3.4",
            "1.2.3+meta",
            "../../evil",
            "file:/C:/evil.tgz",
            "git+https://evil/x",
            "https://evil/x.tgz",
            "1.2.3 ../x",
            "1.2.3;rm",
            "latest",
        ] {
            assert!(!valid_dsh_version(bad), "{bad:?} 应被拒绝");
        }
    }
}
