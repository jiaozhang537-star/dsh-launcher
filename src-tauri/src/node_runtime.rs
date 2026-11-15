//! Node.js 运行时代管：系统检测、版本范围校验、便携版下载/解压/注册。
//!
//! 范围规则来自 PROJECT FR-05：`^22.19 || >=24`（即 22.19 ≤ v < 23，或 ≥ 24；
//! 23.x 不合格）。托管便携版落在 `<managed root>/nodes/<semver>/<node 可执行
//! 文件>`，多版本共存；全局默认版本记录在 `<managed root>/config.json`。
//!
//! 平台边界（对抗审查 R1 P1-06）：发行包文件名与可执行文件名收敛为
//! [`node_archive_name`] / [`node_exe_name`] 两个单点，两侧实现均有单测
//! （参数化 os/arch，Windows 上即可覆盖两个分支）；解压按包格式分派
//! （Windows zip / 其他平台 tar.gz）。

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};

use crate::paths;

/// 进度事件名（前端 listen 用）。
pub const PROGRESS_EVENT: &str = "node-download-progress";

/// Node 可执行文件名（单点）：Windows `node.exe`，其他平台 `node`。
pub fn node_exe_name() -> &'static str {
    if cfg!(windows) {
        "node.exe"
    } else {
        "node"
    }
}

/// Node 发行包文件名（单点，参数化 os/arch 使两个分支都能在任一平台被单测）。
/// Windows 取 zip，其他平台取 tar.gz（nodejs.org 双格式同源发布）。
/// 未覆盖的平台 fail loud——宁可明确不支持，不可默默下错包。
pub fn node_archive_name(os: &str, arch: &str, version: (u32, u32, u32)) -> Result<String, String> {
    let (ma, mi, pa) = version;
    let (tag, ext) = match (os, arch) {
        ("windows", "x86_64") => ("win-x64", "zip"),
        ("linux", "x86_64") => ("linux-x64", "tar.gz"),
        (os, arch) => {
            return Err(format!("暂不支持的 Node 发行包平台：{os}/{arch}"));
        }
    };
    Ok(format!("node-v{ma}.{mi}.{pa}-{tag}.{ext}"))
}

/// 系统 Node 检测结果（IPC 视图；键为 camelCase，与前端 `meetsRange` 对齐）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemNode {
    /// `vX.Y.Z` 原样输出去换行。
    pub version: String,
    /// node 可执行文件绝对路径。
    pub path: String,
    /// 是否满足 `^22.19 || >=24`。
    pub meets_range: bool,
}

/// 启动器配置（持久化于 `<managed root>/config.json`）。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    /// 全局默认托管 Node 版本；None = 尚未代管任何版本。
    #[serde(default)]
    pub default_node_version: Option<String>,
    /// 下载任务并发上限（FR-09 队列 / FR-11 设置项；None = 默认 3，收敛 1~10）。
    #[serde(default)]
    pub max_downloads: Option<u32>,
    /// HTTP(S) 代理（FR-11，形如 `http://127.0.0.1:7890`）；None/空 = 直连。
    /// 作用于全部 ureq 取数（统一经 gateway::base_agent）与 npm 子进程链。
    #[serde(default)]
    pub proxy: Option<String>,
    /// 自更新源（FR-13，`owner/repo`）；None/空 = 未配置（检查更新如实报告）。
    #[serde(default)]
    pub update_repo: Option<String>,
    /// 默认版本隔离（Phase 8.5，FR-17 扩展；PCL LaunchArgumentIndieV2 同语义）：
    /// 新装实例的隔离默认；None = 完全隔离（true）。**仅作用于新建实例**——
    /// 已有实例各走版本设置页的隔离开关，不受本项影响。
    #[serde(default)]
    pub default_isolation: Option<bool>,
    /// 启动器可见性（Phase 8.5，FR-28；PCL LaunchArgumentVisible 原值系，1 弃用）：
    /// 0 = 网页就绪后立即关闭；2 = 隐藏，最后实例退出后自动关闭；
    /// 3 = 隐藏，最后实例退出后重新打开；4 = 最小化；5/None = 保持不变。
    #[serde(default)]
    pub launcher_visibility: Option<u8>,
}

impl LauncherConfig {
    /// 新装实例的隔离默认（None 回落完全隔离——安全默认，配置损坏不扩大共享面）。
    pub fn effective_default_isolation(&self) -> bool {
        self.default_isolation.unwrap_or(true)
    }

    /// 启动器可见性档位（None 回落保持不变 = PCL 出厂默认）。
    pub fn effective_launcher_visibility(&self) -> u8 {
        self.launcher_visibility.unwrap_or(5)
    }
}

/// 解析 `vX.Y.Z` 形式的版本输出为 (major, minor, patch)。
pub fn parse_node_version(output: &str) -> Option<(u32, u32, u32)> {
    let trimmed = output.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let patch = parts.next()?.parse::<u32>().ok()?;
    Some((major, minor, patch))
}

/// FR-05 的固定范围：`^22.19 || >=24`。
pub fn meets_node_range((major, minor, _patch): (u32, u32, u32)) -> bool {
    (major == 22 && minor >= 19) || major >= 24
}

/// 在 PATH 上探测系统 Node；未安装返回 None。
pub fn detect_system_node() -> Option<SystemNode> {
    // run_version_command 已返回 v 前缀串（v24.12.0），原样透传——
    // 此处曾再拼一次 "v{}" 导致界面显示 vv24.12.0。
    let output = run_version_command("node")?;
    let (ma, mi, pa) = parse_node_version(&output)?;
    Some(SystemNode {
        version: output,
        path: which_node()?,
        meets_range: meets_node_range((ma, mi, pa)),
    })
}

/// 运行 `node --version` 并返回去 `v` 的纯版本串；命令不存在或失败返回 None。
fn run_version_command(_program: &str) -> Option<String> {
    let exe = which_node()?;
    let out = std::process::Command::new(exe)
        .arg("--version")
        .creation_flags_cv()
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (ma, mi, pa) = parse_node_version(&text)?;
    Some(format!("v{ma}.{mi}.{pa}"))
}

/// 定位 PATH 上的 node 可执行文件；找不到返回 None。
fn which_node() -> Option<String> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(node_exe_name());
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

/// Windows 下隐藏控制台窗口的 creation_flags（非 Windows 编译为空操作）。
/// 供本 crate 内所有拉起 node/npm 子进程的模块复用。
pub(crate) trait CreationFlagsExt {
    fn creation_flags_cv(&mut self) -> &mut Self;
}

impl CreationFlagsExt for std::process::Command {
    #[cfg(windows)]
    fn creation_flags_cv(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    fn creation_flags_cv(&mut self) -> &mut Self {
        self
    }
}

/// 枚举已托管的 Node 版本（目录名满足 x.y.z 且含 node.exe），升序返回。
pub fn managed_versions() -> Result<Vec<String>, String> {
    let nodes = paths::nodes_dir()?;
    if !nodes.exists() {
        return Ok(Vec::new());
    }
    let mut versions = Vec::new();
    let entries = std::fs::read_dir(&nodes).map_err(|e| format!("读取 {} 失败：{e}", nodes.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if parse_node_version(&name).is_some() && entry.path().join(node_exe_name()).is_file() {
            versions.push(name);
        }
    }
    versions.sort_by_key(|v| {
        let p = parse_node_version(v).unwrap_or((0, 0, 0));
        (p.0, p.1, p.2)
    });
    Ok(versions)
}

/// 托管版本的可执行文件路径；未托管返回 None。
pub fn managed_node_exe(version: &str) -> Option<PathBuf> {
    if parse_node_version(version).is_none() {
        return None;
    }
    let exe = paths::nodes_dir().ok()?.join(version).join(node_exe_name());
    exe.is_file().then_some(exe)
}

/// 读启动器配置；文件不存在视为默认配置。
pub fn load_config() -> Result<LauncherConfig, String> {
    let file = paths::managed_root()?.join("config.json");
    if !file.exists() {
        return Ok(LauncherConfig::default());
    }
    let raw = paths::read_text(&file)?;
    serde_json::from_str(&raw).map_err(|e| format!("解析 {} 失败：{e}", file.display()))
}

/// 代理设置（FR-11）：空串归一为 None；ureq（gateway::base_agent）与
/// npm 子进程链共用同一来源。
pub fn launcher_proxy() -> Option<String> {
    load_config()
        .ok()?
        .proxy
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
}

/// 原子写启动器配置。
pub fn save_config(config: &LauncherConfig) -> Result<(), String> {
    let file = paths::managed_root()?.join("config.json");
    paths::ensure_managed_dirs()?;
    let body = serde_json::to_string_pretty(config).map_err(|e| format!("序列化配置失败：{e}"))?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, &body).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("替换 {} 失败：{e}", file.display()))?;
    Ok(())
}

/// 设置全局默认托管版本；版本必须已托管。
pub fn set_default_version(version: &str) -> Result<(), String> {
    if managed_node_exe(version).is_none() {
        return Err(format!("版本 {version} 未托管，无法设为默认"));
    }
    let mut config = load_config()?;
    config.default_node_version = Some(version.to_string());
    save_config(&config)
}

/// 自动代管候选版本（按序尝试第一个尚未托管的）：两次真实下载验收通过的
/// 已知良好版本且满足 FR-05 范围。这是产品钉定值而非部署可调参数。
pub const AUTO_NODE_CANDIDATES: [&str; 2] = ["22.20.0", "24.12.0"];

/// 从候选表挑出第一个未托管的版本；全部已托管返回 None（调用方 fail loud）。
pub fn next_auto_candidate(managed: &[String]) -> Option<&'static str> {
    AUTO_NODE_CANDIDATES
        .iter()
        .copied()
        .find(|cand| !managed.iter().any(|m| m == cand))
}

/// 当前是否已有可用于安装/启动的 Node：托管默认就位，或系统 Node 满足版本范围。
///
/// 启动链在预检之前读它——为假就先用 [`ensure_default_node`] 自动代管一个已知
/// 良好版本（与安装链同一套兜底），而不是把新手丢到设置页手打版本号。
pub fn node_available() -> bool {
    ready_default_exe().is_some()
        || detect_system_node().map(|sys| sys.meets_range).unwrap_or(false)
}

/// `ensure_default_node` 的事件回调载荷。
#[derive(Debug, Clone)]
pub enum EnsureEvent {
    /// 已决定要自动下载的候选版本（发起下载前回调一次）。
    Downloading { version: String },
    /// 下载字节进度；total 为 0 表示长度未知。
    Bytes { received: u64, total: u64 },
}

/// 并发安装任务的互斥锁：多个并行任务同时发现无 Node 可用时，
/// 只有持锁者执行下载，等待者在拿锁后复查（双检查），避免两个线程
/// 同时解压同一个 `nodes/<version>` 目录造成产物损坏。
static ENSURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 全局默认托管 Node 的可用性（已设置默认且 node.exe 在位）。
fn ready_default_exe() -> Option<PathBuf> {
    load_config()
        .ok()?
        .default_node_version
        .and_then(|v| managed_node_exe(&v))
}

/// 确保存在可用于安装/运行的托管 Node：已就绪直接返回；否则从候选表
/// 自动下载一个已知良好版本、写为全局默认后返回其可执行文件路径。
/// 下载经统一网关的 Node 发行包通道（激活镜像优先 + 官方兜底）。
/// 阻塞调用——command/installer 侧放 spawn_blocking/worker 线程。
pub fn ensure_default_node(
    event: &mut dyn FnMut(EnsureEvent),
    control: Option<&crate::task_queue::TaskControl>,
) -> Result<PathBuf, String> {
    // 快路径：无需加锁即可确认已就绪。
    if let Some(exe) = ready_default_exe() {
        return Ok(exe);
    }
    let guard = ENSURE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // 双检查：等待期间别的任务可能已完成下载并写入默认值。
    if let Some(exe) = ready_default_exe() {
        drop(guard);
        return Ok(exe);
    }

    let managed = managed_versions()?;
    let candidate = next_auto_candidate(&managed).ok_or_else(|| {
        format!(
            "候选版本 {} 均已托管但都缺少 node.exe，请到「设置」页修复托管目录",
            AUTO_NODE_CANDIDATES.join(" / ")
        )
    })?;
    event(EnsureEvent::Downloading {
        version: candidate.to_string(),
    });
    // FR-18 修订：发行包通道恒为 npmmirror 优先 + 官方兜底，与设置无关。
    let dist_bases = crate::gateway::node_dist_bases();
    let part = crate::task_queue::part_path(&format!("auto-node-{candidate}"));
    let dir = download_and_install(candidate, &dist_bases, control, &part, &mut |received, total| {
        event(EnsureEvent::Bytes { received, total });
    })?;
    set_default_version(candidate)?;
    drop(guard);
    Ok(dir.join(node_exe_name()))
}

/// 字节数的紧凑展示（进度文案用）：0 B / 512 KB / 12.3 MB / 1.2 GB。
pub(crate) fn fmt_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let n = bytes as f64;
    if n < KB {
        format!("{bytes} B")
    } else if n < KB * KB {
        format!("{:.0} KB", n / KB)
    } else if n < KB * KB * KB {
        format!("{:.1} MB", n / (KB * KB))
    } else {
        format!("{:.1} GB", n / (KB * KB * KB))
    }
}

/// 从 SHASUMS256.txt 内容里取目标文件的哈希（行格式 `<sha256>  <文件名>`，
/// 二进制形态为 `<sha256> *<文件名>`）；找不到返回 None。
/// crate 内复用（Node 发行包、自更新安装包共用同一校验口径——R1 P1-01）。
pub(crate) fn shasums_hash_for(text: &str, archive_name: &str) -> Option<String> {
    for line in text.lines() {
        let mut parts = line.splitn(2, ' ');
        let hash = parts.next().unwrap_or("").trim();
        let file = parts.next().unwrap_or("").trim_start_matches('*').trim();
        let valid = !hash.is_empty()
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit());
        if valid && file == archive_name {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

/// 取发行包的官方 SHA256 校验和：SHASUMS256.txt 与 zip 同源发布，逐通道
/// 尝试（顺序与下载通道一致）。全部通道失败 → Err（P2-03：拒绝安装无法
/// 校验的 Node 发行包——镜像通道被劫持时这是最后一道内容防线）。
fn expected_sha256(version_triple: &str, archive_name: &str, dist_bases: &[&str]) -> Result<String, String> {
    let mut attempts: Vec<String> = Vec::new();
    for base in dist_bases {
        let url = format!("{base}/v{version_triple}/SHASUMS256.txt");
        let agent = crate::gateway::base_agent()
            .timeout_connect(std::time::Duration::from_secs(20))
            .timeout(std::time::Duration::from_secs(30))
            .build();
        match agent.get(&url).call() {
            Ok(response) => {
                let mut text = String::new();
                match response
                    .into_reader()
                    .take(4 * 1024 * 1024)
                    .read_to_string(&mut text)
                {
                    Ok(_) => match shasums_hash_for(&text, archive_name) {
                        Some(hash) => return Ok(hash),
                        None => attempts.push(format!("[{base}] SHASUMS 中无 {archive_name} 条目")),
                    },
                    Err(e) => attempts.push(format!("[{base}] SHASUMS 读取失败：{e}")),
                }
            }
            Err(e) => attempts.push(format!("[{base}] {e}")),
        }
    }
    Err(format!(
        "无法获取 Node v{version_triple} 的官方 SHA256 校验和（SHASUMS256.txt）：\n{}\n\
         拒绝安装未校验的发行包——请检查网络或镜像源设置后重试",
        attempts.join("\n")
    ))
}

/// 本地文件 sha256（hex 小写）。crate 内复用（Node 发行包、自更新安装包）。
pub(crate) fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("打开 {} 失败：{e}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// 下载并安装一个便携版 Node。阻塞调用——Tauri command 侧放 spawn_blocking。
///
/// `dist_bases` 是经统一网关排序的发行包基址候选（激活源在前，其余兜底），
/// 由调用方从 gateway::node_dist_bases 取得；本模块不感知镜像设置。
/// `control` = 任务控制柄（暂停 / 继续 / 取消；None = 不可控，如测试）。
/// `part_path` = 任务键控的断点续传临时文件（取消 / 换通道后自动 Range 续传）。
/// `progress` 每 ≥256KB 或状态变化回调一次 (received, total)；total 为 0 表示未知长度。
/// 成功返回安装目录；所有候选都不可达时返回最后一个错误。
/// P2-03：下载前先取官方 SHASUMS256 校验和（取不到即拒绝），落盘 zip 必须
/// 哈希一致才解压。
pub fn download_and_install(
    version: &str,
    dist_bases: &[&str],
    control: Option<&crate::task_queue::TaskControl>,
    part_path: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let (ma, mi, pa) =
        parse_node_version(version).ok_or_else(|| format!("非法版本号：{version:?}"))?;
    let archive_name =
        node_archive_name(std::env::consts::OS, std::env::consts::ARCH, (ma, mi, pa))?;
    if dist_bases.is_empty() {
        return Err("下载网关未提供任何 Node 发行包通道".into());
    }
    let expected = expected_sha256(&format!("{ma}.{mi}.{pa}"), &archive_name, dist_bases)?;

    let mut last_err = String::from("未尝试任何通道");
    for base in dist_bases {
        if let Some(control) = control {
            control.checkpoint()?;
        }
        let url = format!("{base}/v{ma}.{mi}.{pa}/{archive_name}");
        match try_download(&url, part_path, control, progress) {
            Ok(zip_path) => {
                let actual = file_sha256(&zip_path).unwrap_or_default();
                if actual != expected {
                    let _ = std::fs::remove_file(&zip_path);
                    last_err = format!(
                        "[{base}] 哈希校验失败（期望 {expected}，实际 {actual}）——已丢弃下载产物"
                    );
                    continue;
                }
                match extract_archive(&zip_path, version, &archive_name) {
                    Ok(dir) => {
                        let _ = std::fs::remove_file(&zip_path);
                        return Ok(dir);
                    }
                    Err(e) => {
                        last_err = format!("[{base}] 解压失败：{e}");
                    }
                }
            }
            Err(e) => {
                last_err = format!("[{base}] {e}");
            }
        }
    }
    Err(last_err)
}

/// 下载到任务键控的临时文件；成功返回本地路径。crate 内复用（Node 便携包、
/// DSH 源码归档、技能包共用同一套停滞看门狗、检查点与断点续传）。
///
/// 断点续传（FR-09 W1）：`part_path` 已有 N 字节时先带 `Range: bytes=N-` 重连，
/// 206 → 追加续传（received 从 N 起算，total 取 Content-Range 绝对总长）；
/// 服务端不支持（200 / 请求失败）→ 丢弃断点整段重来。断点在取消后**保留**
///（重试自动续传）、读取失败时保留（换通道后 Range 重连仍有效）、成功后由
/// 调用方随解压清理。
///
/// 超时策略（真机走查实证：官方直连会「连接成功但数据流停滞」）：
/// - `timeout_connect` 20s：连接建立失败快速换通道；
/// - `timeout_read` 60s：**停滞看门狗**——单次读 60s 无数据即报错，由上层
///   循环自动切换下一通道，「永远下载中」不可能再出现；
/// - `timeout` 30min：整体兜底，持续有慢速数据也不允许无限占用。
///   暂停期间不发起读取、不触发读超时；暂停过久被服务端掐断连接时，恢复
///   后的下一次读报错，上层换通道重连，Range 断点续传兜底。
pub(crate) fn try_download(
    url: &str,
    part_path: &Path,
    control: Option<&crate::task_queue::TaskControl>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let agent = crate::gateway::base_agent()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(60))
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build();
    // 检查点包装：取消错误统一带上已接收字节数（前端展示与重试口径一致）。
    let checkpoint = |received: u64| -> Result<(), String> {
        match control {
            Some(c) => c.checkpoint().map_err(|e| {
                if e.starts_with(crate::task_queue::CANCELLED_PREFIX) {
                    crate::task_queue::cancelled_error(Some(received))
                } else {
                    e
                }
            }),
            None => Ok(()),
        }
    };

    let mut received: u64 = 0;
    let (mut file, total, response) = loop {
        checkpoint(received)?;
        let resumed_bytes = if part_path.exists() {
            part_path.metadata().map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        let mut request = agent.get(url);
        if resumed_bytes > 0 {
            request = request.set("Range", &format!("bytes={resumed_bytes}-"));
        }
        let response = match request.call() {
            Ok(response) => response,
            Err(e) => {
                if resumed_bytes > 0 {
                    // 断点请求失败（416 / 代理不支持 Range / 服务端异常）：
                    // 丢断点从零再来；再失败即返回错误（循环至多两轮）。
                    let _ = std::fs::remove_file(part_path);
                    continue;
                }
                return Err(format!("下载 {url} 失败：{e}"));
            }
        };
        let status = response.status();
        let append = resumed_bytes > 0 && status == 206;
        if resumed_bytes > 0 && status != 206 {
            // 服务器无视 Range 返回整段 200：丢断点整段重来。
            let _ = std::fs::remove_file(part_path);
        }
        let start = if append { resumed_bytes } else { 0 };
        let total = if status == 206 {
            // Content-Range: bytes N-T/L——取绝对总长；缺失时退回 N + 剩余长度。
            response
                .header("Content-Range")
                .and_then(|v| v.rsplit('/').next().and_then(|t| t.parse::<u64>().ok()))
                .filter(|t| *t > 0)
                .unwrap_or_else(|| {
                    start
                        + response
                            .header("Content-Length")
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(0)
                })
        } else {
            response
                .header("Content-Length")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
        };
        let opened = if append {
            std::fs::OpenOptions::new().append(true).open(part_path)
        } else {
            std::fs::File::create(part_path)
        }
        .map_err(|e| format!("打开临时文件 {} 失败：{e}", part_path.display()))?;
        received = start;
        break (opened, total, response);
    };

    let mut reader = response.into_reader();
    let mut next_report: u64 = received + 256 * 1024;
    let mut buf = [0u8; 64 * 1024];
    loop {
        checkpoint(received)?;
        let n = reader.read(&mut buf).map_err(|e| {
            // 断点保留：上层换通道重连后 Range 续传（文案保留既有指引）。
            format!(
                "连接停滞或读取超时，已中止（已接收 {}）：{e} —— 可切换镜像源后重试",
                fmt_size(received)
            )
        })?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])
            .map_err(|e| format!("写临时文件失败：{e}"))?;
        received += n as u64;
        if received >= next_report {
            progress(received, total);
            next_report = received + 256 * 1024;
        }
    }
    file.sync_all().ok();
    drop(file);
    progress(received, total);
    // 长度为已知且不匹配视为损坏，避免解压半包（断点保留供续传补齐）。
    if total != 0 && received != total {
        return Err(format!(
            "下载不完整：预期 {total} 字节，实际 {received} 字节"
        ));
    }
    Ok(part_path.to_path_buf())
}

/// 解压分派（R1 P1-06）：按发行包格式走对应实现；两端都剥压缩包首层目录
/// （node-vX-*/）并落到 nodes/<version>/，产物校验共用。
fn extract_archive(archive_path: &Path, version: &str, archive_name: &str) -> Result<PathBuf, String> {
    if archive_name.ends_with(".zip") {
        extract_zip(archive_path, version)
    } else {
        extract_tar_gz(archive_path, version)
    }
}

/// 解压 zip 到 nodes/<version>/，剥掉压缩包首层目录（node-vX-win-x64/）。
fn extract_zip(zip_path: &Path, version: &str) -> Result<PathBuf, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开 zip 失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| format!("读取 zip 结构失败：{e}"))?;

    let target = paths::nodes_dir()?.join(version);
    std::fs::create_dir_all(&target).map_err(|e| format!("创建 {} 失败：{e}", target.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("遍历 zip 条目失败：{e}"))?;
        let raw_name = entry.name().to_string();
        let Some(relative) = raw_name.split_once('/').map(|(_, rest)| rest.to_string()) else {
            continue; // 首层目录项本身跳过
        };
        if relative.is_empty() {
            continue;
        }
        // 剥层后的 relative 才是落在 target 下的部分；helper 在其上再做
        // zip-slip 防护（错误信息携带的即条目内的相对路径）。
        let out_path = crate::paths::safe_entry_path(&target, &relative)?;
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("创建 {} 失败：{e}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压 {relative} 失败：{e}"))?;
    }

    if !target.join(node_exe_name()).is_file() {
        return Err(format!(
            "解压完成但 {} 不存在，产物异常",
            target.join(node_exe_name()).display()
        ));
    }
    verify_installed_version(&target, version)?;
    Ok(target)
}

/// 解压 tar.gz 到 nodes/<version>/（非 Windows 发行包形态），剥掉压缩包首层
/// 目录（node-vX-linux-x64/）。穿越防护与 zip 路径同走 `safe_entry_path`；
/// 符号链接条目跳过不建（启动器只用 node 本体，npm 等链接由 corepack 供给）。
/// 跨平台实现（tar/flate2 与平台无关），Windows 上保留可编译以便单测分派逻辑。
fn extract_tar_gz(archive_path: &Path, version: &str) -> Result<PathBuf, String> {
    let file = std::fs::File::open(archive_path).map_err(|e| format!("打开压缩包失败：{e}"))?;
    let gz = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
    let mut archive = tar::Archive::new(gz);

    let target = paths::nodes_dir()?.join(version);
    std::fs::create_dir_all(&target).map_err(|e| format!("创建 {} 失败：{e}", target.display()))?;

    for entry in archive
        .entries()
        .map_err(|e| format!("遍历压缩包条目失败：{e}"))?
    {
        let mut entry = entry.map_err(|e| format!("读取压缩包条目失败：{e}"))?;
        let raw_name = entry
            .path()
            .map_err(|e| format!("解析条目路径失败：{e}"))?
            .to_string_lossy()
            .into_owned();
        let Some(relative) = raw_name.split_once('/').map(|(_, rest)| rest.to_string()) else {
            continue; // 首层目录项本身跳过
        };
        if relative.is_empty() {
            continue;
        }
        let out_path = crate::paths::safe_entry_path(&target, &relative)?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if entry_type.is_symlink() {
            continue; // 见函数注释：链接条目不落地
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("创建 {} 失败：{e}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压 {relative} 失败：{e}"))?;
    }

    if !target.join(node_exe_name()).is_file() {
        return Err(format!(
            "解压完成但 {} 不存在，产物异常",
            target.join(node_exe_name()).display()
        ));
    }
    verify_installed_version(&target, version)?;
    Ok(target)
}

/// 安装后复验：运行解压出的 node --version 必须与目标版本一致。
fn verify_installed_version(dir: &Path, version: &str) -> Result<(), String> {
    let out = std::process::Command::new(dir.join(node_exe_name()))
        .arg("--version")
        .creation_flags_cv()
        .output()
        .map_err(|e| format!("运行解压后的 node 失败：{e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text != format!("v{version}") {
        return Err(format!("装后复验失败：期望 v{version}，实际 {text:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 复用 instances 测试模块的跨模块互斥沙箱：所有测试共享进程级 LOCALAPPDATA。
    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        super::super::instances::tests::sandbox(&format!("node-{tag}"))
    }

    /// Phase 8.5：启动选项两字段（默认隔离 / 启动器可见性）serde 回环 + None
    /// 回落语义（完全隔离 / 保持不变 = PCL 出厂默认）。
    #[test]
    fn launcher_config_launch_options_roundtrip_and_fallback() {
        let (_lock, tmp) = sandbox("launch-options");
        let cfg = load_config().unwrap();
        assert!(cfg.effective_default_isolation(), "未配置回落完全隔离");
        assert_eq!(cfg.effective_launcher_visibility(), 5, "未配置回落保持不变");
        save_config(&LauncherConfig {
            default_isolation: Some(false),
            launcher_visibility: Some(2),
            ..LauncherConfig::default()
        })
        .unwrap();
        let cfg = load_config().unwrap();
        assert!(!cfg.effective_default_isolation());
        assert_eq!(cfg.effective_launcher_visibility(), 2);
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 启动链的运行环境门槛：托管默认必须真的有个 node.exe 才算数；系统 Node 不
    /// 合格时门槛为假（启动链于是先自动代管再预检）。
    #[test]
    fn node_available_requires_a_real_managed_exe() {
        let (_lock, tmp) = sandbox("available");
        // 配置指向一个尚未托管的版本 → 托管侧不算数。
        save_config(&LauncherConfig {
            default_node_version: Some("22.20.0".to_string()),
            ..LauncherConfig::default()
        })
        .unwrap();
        assert!(managed_node_exe("22.20.0").is_none());
        let system_ok = detect_system_node().map(|sys| sys.meets_range).unwrap_or(false);
        if system_ok {
            // 本机系统 Node 合格 → 门槛本就为真，这条断言无信息量，跳过。
            std::fs::remove_dir_all(&tmp).ok();
            return;
        }
        assert!(!node_available(), "配置指向缺失的托管版本且系统 Node 不合格");

        let dir = paths::nodes_dir().unwrap().join("22.20.0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("node.exe"), b"stub").unwrap();
        assert!(node_available(), "托管 node.exe 到位后门槛为真");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn parses_v_prefix_and_rejects_garbage() {
        assert_eq!(parse_node_version("v24.12.0\n"), Some((24, 12, 0)));
        assert_eq!(parse_node_version("22.19.0"), Some((22, 19, 0)));
        assert_eq!(parse_node_version(""), None);
        assert_eq!(parse_node_version("vabc"), None);
        assert_eq!(parse_node_version("v24"), None);
    }

    #[test]
    fn range_rule_matches_fr05_including_23_exclusion() {
        // ^22.19 || >=24：22.18 与整个 23.x 都不合格。
        assert!(!meets_node_range((22, 18, 9)));
        assert!(meets_node_range((22, 19, 0)));
        assert!(meets_node_range((22, 20, 5)));
        assert!(!meets_node_range((23, 0, 0)));
        assert!(!meets_node_range((23, 11, 4)));
        assert!(meets_node_range((24, 0, 0)));
        assert!(meets_node_range((25, 3, 1)));
    }

    #[test]
    fn managed_versions_scan_sorted_and_validated() {
        let (_lock, tmp) = sandbox("scan");
        let nodes = tmp.join("dsh-launcher").join("nodes");
        for name in ["9.9.9", "24.12.0"] {
            std::fs::create_dir_all(nodes.join(name)).unwrap();
            std::fs::write(nodes.join(name).join("node.exe"), b"x").unwrap();
        }
        // 缺 node.exe 与非法名不应出现。
        std::fs::create_dir_all(nodes.join("25.0.0-empty")).unwrap();
        std::fs::create_dir_all(nodes.join("not-a-version")).unwrap();
        std::fs::write(nodes.join("not-a-version").join("keep"), b"x").unwrap();

        let versions = managed_versions().unwrap();
        assert_eq!(versions, vec!["9.9.9".to_string(), "24.12.0".to_string()]);
        assert!(managed_node_exe("24.12.0").is_some());
        assert!(managed_node_exe("25.0.0-empty").is_none());
        assert!(managed_node_exe("../evil").is_none());

        set_default_version("24.12.0").unwrap();
        let cfg = load_config().unwrap();
        assert_eq!(cfg.default_node_version.as_deref(), Some("24.12.0"));
        assert!(set_default_version("9.9.9-not-installed").is_err());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn default_config_when_file_absent() {
        let (_lock, tmp) = sandbox("cfg");
        let cfg = load_config().unwrap();
        assert!(cfg.default_node_version.is_none());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn auto_candidate_skips_managed_and_exhausts_loudly() {
        assert_eq!(next_auto_candidate(&[]), Some("22.20.0"));
        assert_eq!(
            next_auto_candidate(&["22.20.0".to_string()]),
            Some("24.12.0")
        );
        assert_eq!(
            next_auto_candidate(&["9.9.9".to_string(), "22.20.0".to_string()]),
            Some("24.12.0")
        );
        assert!(next_auto_candidate(&[
            "22.20.0".to_string(),
            "24.12.0".to_string()
        ])
        .is_none());
    }

    #[test]
    fn fmt_size_uses_readable_units() {
        assert_eq!(fmt_size(0), "0 B");
        assert_eq!(fmt_size(512), "512 B");
        assert_eq!(fmt_size(512 * 1024), "512 KB");
        assert_eq!(fmt_size(12 * 1024 * 1024 + 300 * 1024), "12.3 MB");
        assert_eq!(fmt_size(2u64 * 1024 * 1024 * 1024), "2.0 GB");
    }

    /// 发行包命名单点（R1 P1-06）：os/arch 参数化，两个平台的分支在任一平台
    /// 都可断言；未支持平台 fail loud。
    #[test]
    fn archive_name_is_platform_parameterized_and_fails_loud() {
        assert_eq!(
            node_archive_name("windows", "x86_64", (22, 20, 0)).unwrap(),
            "node-v22.20.0-win-x64.zip"
        );
        assert_eq!(
            node_archive_name("linux", "x86_64", (24, 12, 0)).unwrap(),
            "node-v24.12.0-linux-x64.tar.gz"
        );
        let err = node_archive_name("macos", "aarch64", (24, 0, 0)).unwrap_err();
        assert!(err.contains("暂不支持"), "实际报错：{err}");
        // 当前宿主平台必须有实现（防新增平台参数后忘配）。
        assert!(node_archive_name(std::env::consts::OS, std::env::consts::ARCH, (24, 0, 0)).is_ok());
    }

    /// 可执行文件名单点（R1 P1-06）：与宿主平台一致（行为锚定，防手滑改坏）。
    #[test]
    fn node_exe_name_matches_host_os() {
        if cfg!(windows) {
            assert_eq!(node_exe_name(), "node.exe");
        } else {
            assert_eq!(node_exe_name(), "node");
        }
    }

    /// P2-03：SHASUMS256.txt 行解析——官方/二进制形态、坏行跳过、大小写归一。
    #[test]
    fn shasums_parsing_picks_target_line_and_skips_garbage() {
        let text = "\
b5f4a1cb06f3e0d2013e5d0a3a8aa2ee15b6f3d2a0c1d9e8f7a6b5c4d3e2f100  node-v22.20.0-win-x64.zip
02060b29bb1f2e6ea6b93ea6b9c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6071820 *node-v22.20.0-win-x64.msi
short  node-v22.20.0-headers.tar.gz
zzzz-not-a-hash  node-v22.20.0-win-x86.zip
";
        let got = shasums_hash_for(text, "node-v22.20.0-win-x64.zip").unwrap();
        assert_eq!(got, "b5f4a1cb06f3e0d2013e5d0a3a8aa2ee15b6f3d2a0c1d9e8f7a6b5c4d3e2f100");
        // 二进制（*）形态同样命中。
        assert!(shasums_hash_for(text, "node-v22.20.0-win-x64.msi").is_some());
        // 坏行（非哈希/超长）不误配。
        assert!(shasums_hash_for(text, "node-v22.20.0-headers.tar.gz").is_none());
        assert!(shasums_hash_for(text, "node-v22.20.0-win-x86.zip").is_none());
        assert!(shasums_hash_for("", "node-v22.20.0-win-x64.zip").is_none());
    }

    /// P2-03：本地文件 sha256 与标准测试向量一致。
    #[test]
    fn file_sha256_matches_known_vector() {
        let base = std::env::temp_dir().join(format!("dsh-sha-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("abc.txt");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            file_sha256(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn system_node_detected_or_absent_without_panic() {
        // 本机装了 Node 则应识别出合法结构；没有也不 panic——两种都是合法环境。
        if let Some(node) = detect_system_node() {
            assert!(node.version.starts_with('v'));
            assert!(!node.version.starts_with("vv"), "不得双 v 前缀：{}", node.version);
            assert!(Path::new(&node.path).is_file());
        }
    }

    /// IPC 键名必须是 camelCase：前端按 meetsRange 决定「满足/不满足版本要求」
    /// 徽章，曾因漏 rename 把合格的 v24 显示成「不满足」。
    #[test]
    fn system_node_serializes_camel_case_keys() {
        let node = SystemNode {
            version: "v24.12.0".into(),
            path: "C:\\node\\node.exe".into(),
            meets_range: true,
        };
        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["meetsRange"], true);
        assert_eq!(json["version"], "v24.12.0");
        let raw = serde_json::to_string(&node).unwrap();
        assert!(!raw.contains("meets_range"), "禁止泄漏 snake_case 键：{raw}");
    }

    /// 取消标志语义已随 FR-09 W1 迁移至 task_queue::TaskControl
    ///（见 task_queue 测试）；此处不再保留全局标志用例。

    /// 验收「一键下载成功 + 进度反馈 + ≥2 版本共存 + 全局默认生效」的真实下载验证。
    /// 慢速（两版 zip 共 ~60MB）+ 需网络，默认 `#[ignore]`，显式 `cargo test -- --ignored`。
    #[test]
    #[ignore]
    fn real_download_installs_two_versions_with_progress() {
        let (_lock, _tmp) = sandbox("download-real");
        let bases = crate::gateway::node_dist_bases();
        let mut progress_events: usize = 0;
        for version in ["22.20.0", "24.12.0"] {
            let part = crate::task_queue::part_path(&format!("test-node-{version}"));
            let _ = std::fs::remove_file(&part);
            let dir = download_and_install(version, &bases, None, &part, &mut |received, total| {
                progress_events += 1;
                println!("[{version}] 进度回调 #{progress_events}: {received}/{total} 字节");
                assert!(total == 0 || received <= total, "进度越界");
            })
            .unwrap_or_else(|e| panic!("下载 {version} 失败：{e}"));
            assert!(dir.join("node.exe").is_file(), "{version} 安装目录缺 node.exe");
            let _ = std::fs::remove_file(&part);
        }
        assert!(progress_events > 0, "全程未收到任何进度回调");

        // 多版本共存 + 升序枚举
        let versions = managed_versions().unwrap();
        assert!(
            versions.contains(&"22.20.0".to_string()) && versions.contains(&"24.12.0".to_string()),
            "托管列表缺版本：{versions:?}"
        );
        // 全局默认落地
        set_default_version("24.12.0").unwrap();
        assert_eq!(
            load_config().unwrap().default_node_version.as_deref(),
            Some("24.12.0")
        );
    }
}
