//! 实例进程生命周期（FR-15 / FR-08）：端口隔离分配、拉起、实时输出事件、停止。
//!
//! 端口策略：DSH web 服务的监听端口经 CLI `--port <N>` 指定，组合层默认 3080——
//! 启动器**刻意不从 3080 起分配**（那是用户自己跑 `dsh web` 的惯用端口，见
//! `instances::PORT_BASE` 注释）。**端口是实例身份的一部分**（决策记录见
//! phase-03 计划文件）：创建实例时即从 PORT_BASE 起分配并钉定（随名唯一、删号
//! 即灭），launch 永远复用固定端口；仅当被外部程序抢占时自愈式顺延并写回。
//! 端口函数的权威实现与测试在 instances.rs。
//!
//! 哨兵挂钩：控制台每行先送 log_sentinel 匹配，命中的诊断随行发独立事件。
//!
//! 运行表与孤儿识别（P1 批次）：内存运行表只覆盖本会话，而 DSH 实例进程**不随
//! 启动器退出而终止**——没有持久记录时，重启启动器后旧进程就是看不见的孤儿
//! （端口被占、停用插件后点启动也杀不掉它，这就是「停用后仍报错」的病灶）。
//! 因此每次拉起都往 `<managed root>/running.json` 记一行（pid / 端口 / 拉起
//! 时刻 / 插件清单戳），进程退出时删除；下一会话据「pid 存活 + 钉定端口仍被
//! 占用」采用它，并在插件清单戳变化时重启（`stop_for_restart`）。
//!
//! 平台边界：Windows 用 `taskkill /T /F` 杀整棵进程树；非 Windows 用 `kill -9`
//! 直杀主进程（std 无进程组语义；树杀留待打包层移植时补齐，接口签名已可移植）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use crate::instances::{self, resolve_port};
use crate::keyvault;
use crate::log_sentinel::SentinelSession;
use crate::node_runtime::{self, CreationFlagsExt};

/// 运行中实例的登记项。
struct RunningEntry {
    instance_id: String,
    pid: u32,
    port: u16,
    /// 拉起那一刻插件清单（profile package.json）的修改时间戳——判定「装配是否
    /// 变过、需不需要重启」的基线（与持久记录同源，取内存值避免多读一次盘）。
    manifest_stamp: Option<u64>,
    /// waiter 线程的所有权载体（`wait()` 需要持有它）；停止走 taskkill/kill(pid)。
    #[allow(dead_code)]
    child: Arc<Mutex<Child>>,
}

fn running() -> &'static Mutex<HashMap<String, RunningEntry>> {
    static TABLE: OnceLock<Mutex<HashMap<String, RunningEntry>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sentinels() -> &'static Mutex<HashMap<String, SentinelSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, SentinelSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 重启时被**主动停掉**的实例：waiter 见到它就不再发 `instance-exit`。
/// 否则前端会把旧进程的退出当成整条启动流程的结束，在重启中途收掉加载动画
/// （新进程还在拉起，界面却已经回到空闲态）。
fn suppressed_exits() -> &'static Mutex<std::collections::HashSet<String>> {
    static SUPPRESSED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    SUPPRESSED.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// 落盘的一行运行记录（`<managed root>/running.json`）：跨启动器会话识别
/// 「本启动器拉起且仍在运行」的实例进程。只用于识别与停止，不参与启动决策。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunRecord {
    instance_id: String,
    pid: u32,
    port: u16,
    /// 拉起时刻（Unix epoch 毫秒）。
    launched_at_ms: u64,
    /// 拉起那一刻插件清单的修改时间戳（None = 当时尚无清单）。
    manifest_stamp: Option<u64>,
    /// `dsh web` 在就绪时打印的那个 URL（带进程 token；旧版本不打印则为 None）。
    /// 落盘是为了跨启动器会话仍能「点启动只开网页」——token 只在本进程生命周期
    /// 有效，进程死了这条记录也会随之被清理。
    #[serde(default)]
    web_url: Option<String>,
}

/// `dsh web` 就绪时打印的 URL：`dsh web: http://127.0.0.1:<port>/?<token>`，
/// 可能带 ` (LAN: http://<lan-ip>:<port>/?<token>)` 尾巴。
///
/// **为什么必须用这一行而不是自己拼端口 URL**：新版 DSH 的 web 面板带鉴权栅栏
/// （`@deepseek-ai/dsh-client-connection` 的 `authenticatedUrl`：根查询参数带
/// 进程 token，用它换 cookie），裸 `http://127.0.0.1:<port>/` 一律 401
/// （`dsh web authentication required; reopen the URL printed by dsh web.`）。
/// 2026-09-10 真机事故：启动器传了 `--no-open` 又自己拼 URL，于是新版本实例
/// 点「启动」后浏览器只看到 401 错误页。旧版本不打印这行 → 返回 None，调用方
/// 回落裸端口 URL（旧版本没有栅栏，裸 URL 可用）。
fn parse_web_url(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("dsh web:")?.trim_start();
    let url = rest.split_whitespace().next()?;
    if url.starts_with("http://") || url.starts_with("https://") {
        Some(url.to_string())
    } else {
        None
    }
}

/// 本会话已知的「dsh web 打印的 URL」（instance id → URL）。
fn web_urls() -> &'static Mutex<HashMap<String, String>> {
    static URLS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    URLS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 记下某实例的网页 URL（stdout 线程调用；同时写回运行记录）。
fn publish_web_url(instance_id: &str, url: &str) {
    web_urls()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(instance_id.to_string(), url.to_string());
    if let Ok(mut records) = load_records() {
        if let Some(record) = records.get_mut(instance_id) {
            record.web_url = Some(url.to_string());
            let _ = save_records(&records);
        }
    }
}

/// 取某实例的网页 URL：先看本会话内存，再回落落盘记录（跨会话打开运行中实例）。
fn known_web_url(instance_id: &str) -> Option<String> {
    if let Some(url) = web_urls()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(instance_id)
    {
        return Some(url.clone());
    }
    load_records()
        .ok()
        .and_then(|records| records.get(instance_id).and_then(|r| r.web_url.clone()))
}

fn load_records() -> Result<HashMap<String, RunRecord>, String> {
    let file = crate::paths::running_file()?;
    if !file.exists() {
        return Ok(HashMap::new());
    }
    // 容忍开头 BOM（见 paths::read_text）：运行记录被外部工具碰过不该让查询失败。
    let body = crate::paths::read_text(&file)?;
    let list: Vec<RunRecord> = serde_json::from_str(&body)
        .map_err(|e| format!("解析 {} 失败：{e}", file.display()))?;
    Ok(list.into_iter().map(|r| (r.instance_id.clone(), r)).collect())
}

fn save_records(records: &HashMap<String, RunRecord>) -> Result<(), String> {
    let file = crate::paths::running_file()?;
    let mut list: Vec<&RunRecord> = records.values().collect();
    list.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    let body = serde_json::to_string_pretty(&list)
        .map_err(|e| format!("序列化运行记录失败：{e}"))?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("替换 {} 失败：{e}", file.display()))
}

/// 记下一条运行项（覆盖同实例的旧记录）。
fn remember(record: RunRecord) -> Result<(), String> {
    let mut records = load_records()?;
    records.insert(record.instance_id.clone(), record);
    save_records(&records)
}

/// 抹掉某实例的运行记录（进程已退出/已被停掉）。
fn forget(instance_id: &str) -> Result<(), String> {
    let mut records = load_records()?;
    if records.remove(instance_id).is_some() {
        save_records(&records)?;
    }
    Ok(())
}

/// 进程是否存活且确实是 DSH 实例进程（node.exe）。
///
/// Windows 走进程句柄查询（OpenProcess + GetExitCodeProcess），不走 tasklist：
/// 子进程 + 管道在受限环境下可能被拒（实测沙箱里 tasklist 直接 Access denied），
/// 而且每次点击都拉一个外部进程也白费时间。镜像名必须核对——pid 会被系统复用，
/// 只看 pid 存在就可能杀掉无关进程。
#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// `STILL_ACTIVE`（winnt.h）：进程仍在运行时 GetExitCodeProcess 的返回值。
    const STILL_ACTIVE_CODE: u32 = 259;

    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let alive = GetExitCodeProcess(handle, &mut code).is_ok() && code == STILL_ACTIVE_CODE;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let is_node = alive
            && QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok()
            && String::from_utf16_lossy(&buf[..len as usize])
                .rsplit(['\\', '/'])
                .next()
                .map(|name| name.eq_ignore_ascii_case("node.exe"))
                .unwrap_or(false);
        let _ = CloseHandle(handle);
        is_node
    }
}

#[cfg(not(windows))]
fn pid_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// 杀一棵进程树（Windows taskkill /T /F；其他平台 kill -9）。
/// 进程已不在时按幂等成功处理（孤儿记录可能滞后于实际退出）。
fn kill_pid_tree(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    let out = {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
        cmd.creation_flags_cv();
        cmd.output().map_err(|e| format!("执行 taskkill 失败：{e}"))?
    };
    #[cfg(not(windows))]
    let out = {
        let mut cmd = Command::new("kill");
        cmd.args(["-9", &pid.to_string()]);
        cmd.output().map_err(|e| format!("执行 kill 失败：{e}"))?
    };
    if out.status.success() || !pid_alive(pid) {
        return Ok(());
    }
    Err(format!(
        "停止进程 {pid} 失败：{}",
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

/// 等端口不再被监听。重启前的必要等待：旧监听还在就拉起新进程，DSH 会绑不上
/// 钉定端口，`resolve_port` 随即重锚定——用户的入口地址就漂了。
///
/// 判据是「不再监听」而不是「可 bind」：刚被停掉的进程留下的 TIME_WAIT 连接会
/// 让 bind 探测失败，但监听确已释放（DSH 侧 libuv 带 SO_REUSEADDR，绑得上）。
fn wait_port_released(port: u16, budget: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + budget;
    loop {
        if !port_listening(port) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// 该端口是否有人在监听（连一次即可判定；无监听者时本机连接被立刻拒绝）。
fn port_listening(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// 插件装配是否自进程拉起以来变过：清单戳不同即变过（None 参与比较——
/// 「当时没有清单、现在有了」同样是变化）。
fn restart_needed(recorded: Option<u64>, current: Option<u64>) -> bool {
    recorded != current
}

/// 实例进程状态（前端「启动」按钮的决策输入）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessState {
    pub instance_id: String,
    /// 进程仍在运行：本会话拉起的，或本启动器此前拉起且仍存活的。
    pub running: bool,
    /// true = 运行中但不在本会话内存表里（上一会话遗留的孤儿，已按记录采用）。
    pub adopted: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    /// 插件装配自该进程拉起以来变过——不重启进程就不会生效。
    pub restart_needed: bool,
    /// `dsh web` 打印的带 token 网页 URL（None = 旧版本没打印，前端回落裸端口）。
    pub web_url: Option<String>,
}

/// 查询实例进程状态：内存表 → 落盘记录（pid 存活 + 端口仍被占用才算采用）。
/// 记录已失效（进程没了或端口易主）时顺手清理，下次查询不再重复探测。
pub fn process_state(instance_id: &str) -> Result<ProcessState, String> {
    let current = instances::profile_manifest_stamp(instance_id)?;
    let session = running()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(instance_id)
        .map(|e| (e.pid, e.port, e.manifest_stamp));
    if let Some((pid, port, stamp)) = session {
        return Ok(ProcessState {
            instance_id: instance_id.to_string(),
            running: true,
            adopted: false,
            pid: Some(pid),
            port: Some(port),
            restart_needed: restart_needed(stamp, current),
            web_url: known_web_url(instance_id),
        });
    }

    let records = load_records()?;
    let Some(record) = records.get(instance_id) else {
        // 没有记录，但钉定端口有人在监听：极可能是**本版本之前**的启动器留下的
        // 实例进程（那时还没有运行记录）。端口就是实例身份，按「运行中（身份未
        // 识别）」处理——打开它正是用户想要的，也避免静默拉起第二个实例把端口
        // 挤走。代价：没有 pid 就无法重启（`stop_for_restart` 会如实报错）。
        let pinned = instances::get_instance(instance_id)?.port;
        if let Some(port) = pinned {
            if port_listening(port) {
                return Ok(ProcessState {
                    instance_id: instance_id.to_string(),
                    running: true,
                    adopted: true,
                    pid: None,
                    port: Some(port),
                    restart_needed: false,
                    web_url: known_web_url(instance_id),
                });
            }
        }
        return Ok(idle_state(instance_id, current));
    };
    if pid_alive(record.pid) && port_listening(record.port) {
        return Ok(ProcessState {
            instance_id: instance_id.to_string(),
            running: true,
            adopted: true,
            pid: Some(record.pid),
            port: Some(record.port),
            restart_needed: restart_needed(record.manifest_stamp, current),
            web_url: record.web_url.clone(),
        });
    }
    forget(instance_id)?;
    Ok(idle_state(instance_id, current))
}

fn idle_state(instance_id: &str, current: Option<u64>) -> ProcessState {
    ProcessState {
        instance_id: instance_id.to_string(),
        running: false,
        adopted: false,
        pid: None,
        port: None,
        restart_needed: restart_needed(None, current),
        web_url: None,
    }
}

/// 重启前停掉该实例的进程（含上一会话遗留的孤儿），并等到钉定端口释放。
///
/// 顺序保证：先确认端口真回到空闲才返回；若进程没杀干净（端口仍被占），
/// 直接报错中断——否则下一次拉起会重锚定到别的端口，用户地址心智就漂了。
pub fn stop_for_restart(instance_id: &str) -> Result<(), String> {
    let state = process_state(instance_id)?;
    if !state.running {
        return Ok(());
    }
    let (Some(pid), Some(port)) = (state.pid, state.port) else {
        return Err("该实例的进程无法识别（没有运行记录），请手动结束它后重试".to_string());
    };
    if state.adopted {
        kill_pid_tree(pid)?;
        forget(instance_id)?;
    } else {
        // 主动停旧进程：抑制它那一条 instance-exit（新进程随后就拉起，界面
        // 不该在重启中途回到空闲态）。停失败要撤销抑制，别让真退出被吞掉。
        suppressed_exits()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(instance_id.to_string());
        if let Err(e) = stop(instance_id) {
            suppressed_exits()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(instance_id);
            return Err(e);
        }
    }
    if !wait_port_released(port, std::time::Duration::from_secs(10)) && pid_alive(pid) {
        return Err(format!(
            "旧实例进程 {pid} 未能停止（端口 {port} 仍被占用）——请手动结束该进程后重试"
        ));
    }
    Ok(())
}

/// 用户取消启动时停掉已经拉起的进程：与 `stop_for_restart` 同款，抑制它那条
/// `instance-exit`。前端是自己点的取消，收尾依据是取消命令的返回值（以及启动链
/// 的 Err），不需要再收一条「异常退出」；抑制同时消掉一个真实竞态——取消后立刻
/// 再点启动时，上一次的退出事件会落在新一次启动的中途，把加载动画误收掉。
///
/// 与 `stop_for_restart` 一样等到钉定端口释放才返回：取消之后用户马上再点启动，
/// 必须先确认端口真的空出来（否则新进程会重锚定到别的端口）。
pub fn stop_silently(instance_id: &str) -> Result<(), String> {
    let state = process_state(instance_id)?;
    if !state.running {
        return Ok(());
    }
    suppressed_exits()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(instance_id.to_string());
    if let Err(e) = stop(instance_id) {
        // 停失败要撤销抑制，别让一次真退出被吞掉。
        suppressed_exits()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(instance_id);
        return Err(e);
    }
    if let (Some(pid), Some(port)) = (state.pid, state.port) {
        if !wait_port_released(port, std::time::Duration::from_secs(10)) && pid_alive(pid) {
            return Err(format!(
                "已取消启动，但进程 {pid} 未能停止（端口 {port} 仍被占用）——请手动结束该进程"
            ));
        }
    }
    Ok(())
}

/// 运行状态的前端视图。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub instance_id: String,
    pub port: u16,
    pub pid: u32,
}

/// 枚举当前运行中的实例（仅本启动器会话内有效）。
pub fn list_running() -> Vec<RunStatus> {
    running()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
        .map(|e| RunStatus {
            instance_id: e.instance_id.clone(),
            port: e.port,
            pid: e.pid,
        })
        .collect()
}

/// 实例工作区目录：`<实例目录>/workspace`（首次启动创建）。
fn workspace_dir(instance_id: &str) -> Result<PathBuf, String> {
    let dir = instances::instance_dir(instance_id)?.join("workspace");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("创建工作区 {} 失败：{e}", dir.display()))?;
    Ok(dir)
}

/// 拉起参数与结果的公开形态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOutcome {
    pub instance_id: String,
    pub port: u16,
    pub pid: u32,
}

/// 启动实例（阻塞到进程拉起成功；输出经事件流推送）。
///
/// 校验链由 command 层先跑（`precheck::run`），这里只做拉起侧的硬前置：
/// 已安装 DSH、托管 Node 在位。`app` 传 None 时不发事件（集成测试注入点）。
/// `keep_pinned_port = true`（重启路径）：调用方刚停掉占用钉定端口的旧进程，
/// 端口可能仍处 TIME_WAIT——强行复用原端口，不让入口地址漂移。
pub fn launch(
    app: Option<&tauri::AppHandle>,
    instance_id: &str,
    keep_pinned_port: bool,
) -> Result<LaunchOutcome, String> {
    use tauri::Emitter;

    if running()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .contains_key(instance_id)
    {
        return Err("该实例已在运行".into());
    }

    let instance = instances::get_instance(instance_id)?;
    if instance.dsh_version.is_none() {
        return Err("尚未安装 DSH 本体——请先到「下载」页安装".into());
    }
    let bin_js = crate::dsh_install::installed_bin_js(instance_id)?
        .ok_or_else(|| "DSH 安装区不完整（bin.js 缺失）——请到「下载」页重装".to_string())?;

    // Node 解析与 precheck 相同规则：实例覆盖 > 全局默认 > 系统 PATH。
    let node_exe = match instance
        .node_version
        .clone()
        .or_else(|| node_runtime::load_config().ok()?.default_node_version)
    {
        Some(v) => node_runtime::managed_node_exe(&v),
        None => node_runtime::detect_system_node().map(|sys| PathBuf::from(sys.path)),
    }
    .ok_or_else(|| "没有可用 Node——请到「设置」页代管便携版".to_string())?;

    // 端口决策：固定端口优先（不漂移）；占用排除 = 运行表 ∪ 其他实例已钉定的
    // 端口——给谁分配过就永久是谁的，新分配绝不偷别人的地址。
    let mut taken: Vec<u16> = running()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .values()
        .map(|e| e.port)
        .collect();
    for other in instances::list_instances()? {
        if other.id != instance_id {
            if let Some(p) = other.port {
                taken.push(p);
            }
        }
    }
    let (port, reanchored) = resolve_port(instance.port, &taken, keep_pinned_port)?;

    let workspace = workspace_dir(instance_id)?;

    // 插件清单基线：拉起那一刻的戳，之后任何启停/安装都会让现值偏离它。
    let manifest_stamp = instances::profile_manifest_stamp(instance_id)?;

    let mut cmd = Command::new(&node_exe);
    cmd.arg(&bin_js)
        .args(["--profile", &instance.profile, "--port", &port.to_string(), "--no-open"])
        .current_dir(&workspace)
        .env("DSH_HOME", instances::instance_dir(instance_id)?);
    // Key 注入通道（R-2.3 裁决）：逐「启用且有 Key」厂商注入其环境变量；
    // 明文只在内存中传递，绝不落盘。多厂商并存注入，实例用哪家模型由
    // 实例注入的默认模型决定。变量名与路由 apiKeyEnv 共用 keyvault::env_name_of
    // （自定义提供方 `custom-<slug>` → `CUSTOM_<SLUG>_API_KEY`），两端不会漂移。
    for (vendor, key) in keyvault::reveal_enabled_keys()? {
        if let Some(name) = keyvault::env_name_of(&vendor) {
            cmd.env(name, key);
        }
    }
    // deepseek 的地址覆盖走 DEEPSEEK_BASE_URL（llm-deepseek 的基址覆盖变量，
    // 官方默认 https://api.deepseek.com）。其余厂商的地址覆盖在路由桥接时
    // 写进各自路由的 baseURL（llm_bridge）。
    if let Ok(Some(base)) = keyvault::address_of("deepseek") {
        let base = base.trim();
        if !base.is_empty() {
            cmd.env("DEEPSEEK_BASE_URL", base);
        }
    }
    if let Some(node_dir) = node_exe.parent() {
        let prepend =
            format!("{};{}", node_dir.display(), std::env::var("PATH").unwrap_or_default());
        cmd.env("PATH", prepend);
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags_cv();

    let mut child = cmd.spawn().map_err(|e| format!("拉起 DSH 失败：{e}"))?;
    // 自愈告知：钉定端口被外部抢占而重锚定时，在控制台里说明缘由（对用户可见）。
    if reanchored {
        if let Some(app) = app {
            let _ = app.emit(
                "instance-console",
                serde_json::json!({
                    "instanceId": instance_id,
                    "stream": "stderr",
                    "line": format!("── 固定端口被外部程序占用，本次起改用端口 {port}（已写回实例） ──"),
                }),
            );
        }
    }
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // 输出转发：每流一线程，逐行发事件；哨兵命中追加诊断事件。
    // stdout/stderr 类型不同，按 trait 对象统一处理。
    let streams: Vec<(&str, Box<dyn std::io::Read + Send>)> = vec![
        (
            "stdout",
            Box::new(stdout.ok_or("DSH stdout 缺失")?),
        ),
        (
            "stderr",
            Box::new(stderr.ok_or("DSH stderr 缺失")?),
        ),
    ];
    for (stream_name, stream) in streams {
        let emitter = app.cloned();
        let id = instance_id.to_string();
        thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(std::result::Result::ok) {
                // `dsh web` 就绪时打印带 token 的 URL：这是打开网页的唯一正确入口
                // （裸端口 URL 在带鉴权栅栏的新版本上必然 401，见 parse_web_url）。
                if stream_name == "stdout" {
                    if let Some(url) = parse_web_url(&line) {
                        publish_web_url(&id, &url);
                    }
                }
                let diagnostic = sentinels()
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .entry(id.clone())
                    .or_insert_with(SentinelSession::new)
                    .feed(&line);
                if let Some(emitter) = &emitter {
                    let _ = emitter.emit(
                        "instance-console",
                        serde_json::json!({ "instanceId": id, "stream": stream_name, "line": line }),
                    );
                    if let Some(card) = diagnostic {
                        let _ = emitter.emit(
                            "instance-diagnostic",
                            serde_json::json!({ "instanceId": id, "diagnostic": card }),
                        );
                    }
                }
            }
        });
    }

    // 运行登记 + 退出监听：waiter 持有 Child 句柄，退出时清表并发事件。
    let child_arc = Arc::new(Mutex::new(child));
    let entry = RunningEntry {
        instance_id: instance_id.to_string(),
        pid,
        port,
        manifest_stamp,
        child: Arc::clone(&child_arc),
    };
    running()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(instance_id.to_string(), entry);

    // 持久化运行记录：进程不随启动器退出而终止，下一会话要靠它认出这个孤儿。
    // 落盘失败不阻断启动（内存表仍能管好本会话），但必须在控制台说明——静默
    // 失败会让用户在下一次启动器会话里遇到「点启动又拉一个进程」的怪象。
    let launched_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let record = RunRecord {
        instance_id: instance_id.to_string(),
        pid,
        port,
        launched_at_ms,
        manifest_stamp,
        // 拉起时还没有 URL——`dsh web` 就绪后由 stdout 线程补写（publish_web_url）。
        web_url: None,
    };
    // 上一次拉起留下的 URL 不能复用：token 随进程失效，且会让就绪判定抢跑。
    web_urls()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(instance_id);
    if let Err(e) = remember(record) {
        if let Some(app) = app {
            let _ = app.emit(
                "instance-console",
                serde_json::json!({
                    "instanceId": instance_id,
                    "stream": "stderr",
                    "line": format!("── 运行记录落盘失败（不影响本次启动）：{e} ──"),
                }),
            );
        }
    }

    let waiter_app = app.cloned();
    let waiter_id = instance_id.to_string();
    thread::spawn(move || {
        let status = {
            let mut guard = child_arc.lock().unwrap_or_else(|p| p.into_inner());
            guard.wait()
        };
        running()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&waiter_id);
        sentinels()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&waiter_id);
        if let Err(e) = forget(&waiter_id) {
            if let Some(app) = &waiter_app {
                let _ = app.emit(
                    "instance-console",
                    serde_json::json!({
                        "instanceId": waiter_id,
                        "stream": "stderr",
                        "line": format!("── 清理运行记录失败：{e} ──"),
                    }),
                );
            }
        }
        // URL 随进程失效：不清掉会让下一次拉起的就绪判定抢跑（见 known_web_url）。
        web_urls()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&waiter_id);
        let code = status.map(|s| s.code()).unwrap_or(None).unwrap_or(-1);
        let suppressed = suppressed_exits()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&waiter_id);
        if let Some(app) = &waiter_app {
            if !suppressed {
                let _ = app.emit(
                    "instance-exit",
                    serde_json::json!({ "instanceId": waiter_id, "code": code }),
                );
            }
        }
    });

    // 网页就绪监视（R6）：进程拉起 ≠ 网页可访问——首启要拉依赖、常规也要
    // 数秒。就绪信号优先取 `dsh web` 打印的 URL（那是 DSH 装载完插件层、服务
    // 真正可服务时才打印的，且带鉴权 token）；旧版本不打印这行 → 端口可连即
    // 就绪，回落裸端口 URL（旧版本没有鉴权栅栏）。180s 超时发
    // instance-web-timeout（前端提示可稍后手动打开；进程若提前退出，exit
    // 事件已另行告知，本监视到点自然收尾）。
    // 期间按 500ms 步进推 launch-progress（70 → 96 缓升，就绪时 100）——
    // 加载动画的百分比由真实进度驱动，不靠前端自转。
    if let Some(app) = app {
        let emitter = app.clone();
        let ready_id = instance_id.to_string();
        let ready_port = port;
        thread::spawn(move || {
            use tauri::Emitter;
            let start = std::time::Instant::now();
            let deadline = start + std::time::Duration::from_secs(180);
            // 端口已可连之后，再等多久去抓 URL 打印行（防「端口先通、URL 行后到」
            // 的毫秒级竞争把带 token 的 URL 漏掉）。旧版本永不打印，等满即回落。
            const URL_GRACE: std::time::Duration = std::time::Duration::from_millis(2500);
            let mut port_open_at: Option<std::time::Instant> = None;
            let announce = |url: Option<String>| {
                let _ = emitter.emit(
                    "instance-web-ready",
                    serde_json::json!({ "instanceId": ready_id, "port": ready_port, "url": url }),
                );
                let _ = emitter.emit(
                    crate::LAUNCH_PROGRESS_EVENT,
                    serde_json::json!({
                        "instanceId": ready_id,
                        "percent": 100,
                        "stage": "DSH 网页已就绪，正在打开浏览器…",
                    }),
                );
            };
            loop {
                // 本次启动被取消（用户点了取消）：立刻收尾，既不发就绪也不发超时
                // ——否则取消后仍会蹦出「网页已就绪」并自动开浏览器。
                if crate::launch_cancel::is_cancelled(&ready_id) {
                    return;
                }
                if let Some(url) = known_web_url(&ready_id) {
                    announce(Some(url));
                    return;
                }
                let now = std::time::Instant::now();
                if std::net::TcpStream::connect(("127.0.0.1", ready_port)).is_ok() {
                    let opened = *port_open_at.get_or_insert(now);
                    if now.duration_since(opened) >= URL_GRACE {
                        // 旧版本：没有 URL 行可用，裸端口 URL 就是它的正确入口。
                        announce(None);
                        return;
                    }
                }
                if now >= deadline {
                    let _ = emitter.emit(
                        "instance-web-timeout",
                        serde_json::json!({ "instanceId": ready_id, "port": ready_port }),
                    );
                    return;
                }
                let elapsed = start.elapsed().as_secs() as u8;
                let percent = 70u8.saturating_add(elapsed.saturating_mul(2)).min(96);
                let _ = emitter.emit(
                    crate::LAUNCH_PROGRESS_EVENT,
                    serde_json::json!({
                        "instanceId": ready_id,
                        "percent": percent,
                        "stage": "等待 DSH 装载插件层并开放网页…",
                    }),
                );
                thread::sleep(std::time::Duration::from_millis(500));
            }
        });
    }

    // 最近使用排序依据 + 端口重锚定落盘（首次分配/自愈时把结果钉回注册表）。
    instances::update_instance(instance_id, |i| {
        i.last_launched_at = Some(instances::now_rfc3339());
        if i.port != Some(port) {
            i.port = Some(port);
        }
    })?;

    Ok(LaunchOutcome { instance_id: instance_id.to_string(), port, pid })
}

/// 停止实例：Windows 杀整棵进程树；其他平台直杀主进程。
///
/// 内存表命中 → 杀该 pid 并清表（waiter 会另行收尾）；未命中 → 按落盘记录找
/// 上一会话遗留的孤儿进程。进程不存在或已退出一律幂等成功。
pub fn stop(instance_id: &str) -> Result<(), String> {
    let entry = running()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(instance_id);
    let Some(entry) = entry else {
        return stop_recorded(instance_id);
    };

    let result = kill_pid_tree(entry.pid);
    let forgotten = forget(instance_id);
    result.and_then(|()| forgotten)
}

/// 停止「不在本会话内存表里、但落盘记录说它还在跑」的实例进程（幂等）。
fn stop_recorded(instance_id: &str) -> Result<(), String> {
    let records = load_records()?;
    let Some(record) = records.get(instance_id) else {
        return Ok(());
    };
    let (pid, port) = (record.pid, record.port);
    if !pid_alive(pid) {
        forget(instance_id)?;
        return Ok(());
    }
    kill_pid_tree(pid)?;
    wait_port_released(port, std::time::Duration::from_secs(10));
    forget(instance_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_missing_instance_is_idempotent_success() {
        assert_eq!(stop("i-never-started"), Ok(()));
        assert!(list_running().iter().all(|r| r.instance_id != "i-never-started"));
    }

    #[test]
    fn run_status_shape_serializes_camel_case() {
        let status = RunStatus { instance_id: "abc".into(), port: 3080, pid: 42 };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"instanceId\":\"abc\""));
        assert!(json.contains("\"port\":3080"));
    }

    /// 重启判定的唯一规则：清单戳不同即变过。None（当时没有清单）与有时间戳
    /// 是两种不同事实，「先无后有」同样是变化——不能当相等放过。
    #[test]
    fn restart_needed_compares_stamps_including_absence() {
        assert!(!restart_needed(None, None));
        assert!(!restart_needed(Some(1_700_000_000_000), Some(1_700_000_000_000)));
        assert!(restart_needed(Some(1_700_000_000_000), Some(1_700_000_000_001)));
        assert!(restart_needed(None, Some(1_700_000_000_000)));
        assert!(restart_needed(Some(1_700_000_000_000), None));
    }

    /// 运行记录的持久化键必须是 camelCase（前端 ProcessState 按 restartNeeded
    /// 读取；记录本身仅供本模块解析，但键名一旦漂移会让旧文件读成空表）。
    /// 旧记录没有 `webUrl` 键（本字段是后加的）也必须能读回来。
    #[test]
    fn run_record_round_trips() {
        let record = RunRecord {
            instance_id: "i-x".into(),
            pid: 4242,
            port: 3080,
            launched_at_ms: 1_700_000_000_000,
            manifest_stamp: Some(1_699_999_000_000),
            web_url: Some("http://127.0.0.1:3090/?token=t".into()),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("\"instanceId\":\"i-x\""));
        assert!(json.contains("\"manifestStamp\":1699999000000"));
        assert!(json.contains("\"webUrl\":\"http://127.0.0.1:3090/?token=t\""));
        let back: RunRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pid, 4242);
        assert_eq!(back.manifest_stamp, Some(1_699_999_000_000));

        // 旧文件（无 webUrl 键）按 None 读入，不因缺键解析失败。
        let legacy = r#"[{"instanceId":"i-y","pid":1,"port":3091,"launchedAtMs":1,"manifestStamp":null}]"#;
        let parsed: Vec<RunRecord> = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed[0].web_url, None);
    }

    #[test]
    fn process_state_shape_serializes_camel_case() {
        let state = ProcessState {
            instance_id: "i-x".into(),
            running: true,
            adopted: true,
            pid: Some(7),
            port: Some(3081),
            restart_needed: true,
            web_url: Some("http://127.0.0.1:3081/?token=abc".into()),
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["instanceId"], "i-x");
        assert_eq!(json["restartNeeded"], true);
        assert_eq!(json["adopted"], true);
        assert_eq!(json["pid"], 7);
        assert_eq!(json["webUrl"], "http://127.0.0.1:3081/?token=abc");
    }

    /// `dsh web` 打印的 URL 解析：带 token / 带 LAN 尾巴 / 旧版无 token / 无关行。
    #[test]
    fn parses_dsh_web_url_line() {
        assert_eq!(
            parse_web_url("dsh web: http://127.0.0.1:3091/?token=deadbeef"),
            Some("http://127.0.0.1:3091/?token=deadbeef".to_string())
        );
        assert_eq!(
            parse_web_url(
                "dsh web: http://127.0.0.1:3091/?token=deadbeef (LAN: http://192.168.1.5:3091/?token=deadbeef)"
            ),
            Some("http://127.0.0.1:3091/?token=deadbeef".to_string()),
            "LAN 尾巴只作展示，本地入口取第一个 URL"
        );
        assert_eq!(
            parse_web_url("dsh web: http://127.0.0.1:3082"),
            Some("http://127.0.0.1:3082".to_string()),
            "旧版本不带 token 也照样解析"
        );
        for unrelated in [
            "dsh web: opening the default browser; pass --no-open to disable",
            "正在启动…",
            "",
            "dsh web:",
        ] {
            assert_eq!(parse_web_url(unrelated), None, "不该把无关行当 URL：{unrelated:?}");
        }
    }

    /// 未拉起的实例：状态为「未运行」，且不得把「无记录」当成需要重启
    /// （否则每次点启动都会走重启分支，白白多一次 kill 探测）。
    #[test]
    fn process_state_of_never_launched_instance_is_idle() {
        let (_guard, _dir) = instances::tests::sandbox("proc-idle");
        // 必须用确定空闲的端口：写死的端口号在本机可能真被别的实例或用户的
        // GUI（3080）占着，那会命中「无记录但端口被监听 → 视为运行中」的兜底分支。
        save_test_registry("i-idle", free_port());
        let state = process_state("i-idle").unwrap();
        assert!(!state.running);
        assert!(!state.adopted);
        assert_eq!(state.pid, None);
        assert_eq!(state.port, None);
        assert!(!state.restart_needed);
    }

    /// 孤儿采用 + 重启判定（真机路径）：真实拉起一个 node 监听进程，写一条运行
    /// 记录，断言采用为「运行中」、清单未变时无需重启、清单变过后要求重启。
    /// 环境没有 node 时如实跳过并打印原因。
    #[test]
    fn adopts_recorded_orphan_and_reports_restart_needed() {
        let (_guard, _dir) = instances::tests::sandbox("proc-orphan");
        let Some((keeper, pid, port)) = spawn_recorded_orphan() else {
            return;
        };

        let state = process_state("i-orphan").unwrap();
        assert!(state.running, "记录了 pid 与端口，应识别为运行中");
        assert!(state.adopted, "不在本会话内存表 → 采用自记录");
        assert_eq!(state.pid, Some(pid));
        assert_eq!(state.port, Some(port));
        assert!(!state.restart_needed, "清单当时不存在、现在也不存在 → 无需重启");

        // 插件清单出现（等价于用户启停/安装了插件）→ 必须重启才生效。
        let manifest = instances::profile_manifest_path("i-orphan").unwrap();
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(&manifest, "{}").unwrap();
        assert!(process_state("i-orphan").unwrap().restart_needed);

        let _ = keeper.0.lock().unwrap().kill();
    }

    /// 重启停进程：`stop_for_restart` 必须真的杀掉孤儿进程、释放监听并清掉记录。
    /// 忽略项——受限环境会拒绝 taskkill（实测 Access denied），需在非受限环境跑：
    /// `cargo test --lib stop_for_restart_kills_recorded_orphan -- --ignored`
    #[test]
    #[ignore = "需要非受限环境：受限环境拒绝 taskkill"]
    fn stop_for_restart_kills_recorded_orphan() {
        let (_guard, _dir) = instances::tests::sandbox("proc-kill");
        let Some((_keeper, pid, port)) = spawn_recorded_orphan() else {
            return;
        };
        stop_for_restart("i-orphan").unwrap();
        assert!(!pid_alive(pid), "重启必须先杀掉旧进程");
        assert!(!port_listening(port), "旧监听必须已释放（否则端口会漂移）");
        let after = process_state("i-orphan").unwrap();
        assert!(!after.running);
        assert_eq!(after.pid, None);
        assert!(load_records().unwrap().get("i-orphan").is_none(), "记录应被清除");
    }

    /// 无运行记录但钉定端口被监听（旧版启动器遗留的实例进程）：按「运行中（身份
    /// 未识别）」处理，点启动只开网页——绝不静默拉起第二个实例。
    #[test]
    fn unrecorded_listener_on_pinned_port_counts_as_running() {
        let (_guard, _dir) = instances::tests::sandbox("proc-unrecorded");
        let port = free_port();
        save_test_registry("i-orphan", port);
        let Some((keeper, _pid)) = spawn_listening_node(port) else {
            return;
        };

        let state = process_state("i-orphan").unwrap();
        assert!(state.running, "钉定端口有人在监听 → 视为运行中");
        assert!(state.adopted);
        assert_eq!(state.pid, None, "无记录时无法识别 pid");
        assert_eq!(state.port, Some(port));
        assert!(!state.restart_needed, "识别不出 pid 就不能声称需要重启");

        // 端口空闲（无人监听）→ 回到未运行。
        drop(keeper);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while port_listening(port) && std::time::Instant::now() < deadline {
            thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!process_state("i-orphan").unwrap().running);
    }

    /// 真机脚手架：拉起一个真实 node 监听进程并写入一条运行记录（模拟上一会话
    /// 遗留的孤儿）。环境没有 node 时返回 None（调用方如实跳过）。调用方须先建沙箱。
    fn spawn_recorded_orphan() -> Option<(KillOnDrop, u32, u16)> {
        let port = free_port();
        save_test_registry("i-orphan", port);
        let (keeper, pid) = spawn_listening_node(port)?;
        remember(RunRecord {
            instance_id: "i-orphan".into(),
            pid,
            port,
            launched_at_ms: 1_700_000_000_000,
            manifest_stamp: None,
            web_url: None,
        })
        .unwrap();
        Some((keeper, pid, port))
    }

    /// 确定空闲的端口：绑 0 号口取系统分配（临时端口段，与实例端口池 309x 不相交），
    /// 释放后即可用——真机测试不能写死端口号，本机可能真被别人的实例占着。
    fn free_port() -> u16 {
        let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        port
    }

    /// 拉起一个只监听端口的 node 进程；环境无 node 时打印原因并返回 None。
    fn spawn_listening_node(port: u16) -> Option<(KillOnDrop, u32)> {
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("跳过：环境无 node");
            return None;
        }
        let child = Command::new("node")
            .args([
                "-e",
                &format!(
                    "require('net').createServer(function(c){{c.on('error',function(){{}})}}).listen({port},'127.0.0.1')"
                ),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags_cv()
            .spawn()
            .expect("拉起 node 监听进程失败");
        let pid = child.id();
        let keeper = KillOnDrop(std::sync::Mutex::new(child));
        let listening_deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !port_listening(port) {
            if std::time::Instant::now() >= listening_deadline {
                panic!("node 未在 10s 内监听端口 {port}");
            }
            thread::sleep(std::time::Duration::from_millis(100));
        }
        Some((keeper, pid))
    }

    /// 断言失败会 panic（不执行后续清理），用守卫保证监听进程一定被收走。
    struct KillOnDrop(std::sync::Mutex<std::process::Child>);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            if let Ok(child) = self.0.get_mut() {
                let _ = child.kill();
            }
        }
    }

    /// 测试用最小注册表：一个钉定端口的实例（profile = web）。
    fn save_test_registry(id: &str, port: u16) {
        let registry = instances::InstanceRegistry {
            instances: vec![instances::Instance {
                id: id.into(),
                name: format!("dsh-{id}"),
                profile: "web".into(),
                node_version: None,
                dsh_version: None,
                last_launched_at: None,
                port: Some(port),
                created_at: "2026-09-08T00:00:00Z".into(),
            }],
        };
        instances::save_registry(&registry).unwrap();
    }
}
