//! Node.js 运行时代管：系统检测、版本范围校验、便携版下载/解压/注册。
//!
//! 范围规则来自 PROJECT FR-05：`^22.19 || >=24`（即 22.19 ≤ v < 23，或 ≥ 24；
//! 23.x 不合格）。托管便携版落在 `<managed root>/nodes/<semver>/node.exe`，
//! 多版本共存；全局默认版本记录在 `<managed root>/config.json`。

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};

use crate::paths;
use std::sync::atomic::{AtomicBool, Ordering};

/// 进度事件名（前端 listen 用）。
pub const PROGRESS_EVENT: &str = "node-download-progress";

/// Node 下载取消标志（全局）：手动「下载并托管」与安装链 AutoNode 共用。
/// 边界：两条链并发下载时取消会同时中止两者——前端只在手动下载处暴露
/// 取消按钮，内测期可接受；标志在每个下载入口统一重置。
static DOWNLOAD_CANCEL: AtomicBool = AtomicBool::new(false);

/// 请求中止进行中的 Node 下载（至多在下一次读取返回时生效）。
pub fn cancel_download() {
    DOWNLOAD_CANCEL.store(true, Ordering::Relaxed);
}

fn download_cancelled() -> bool {
    DOWNLOAD_CANCEL.load(Ordering::Relaxed)
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

/// 定位 PATH 上的 node.exe（Windows）；找不到返回 None。
fn which_node() -> Option<String> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in path_var.split(';') {
        if dir.trim().is_empty() {
            continue;
        }
        let candidate = Path::new(dir).join("node.exe");
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
        if parse_node_version(&name).is_some() && entry.path().join("node.exe").is_file() {
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
    let exe = paths::nodes_dir().ok()?.join(version).join("node.exe");
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
pub fn ensure_default_node(event: &mut dyn FnMut(EnsureEvent)) -> Result<PathBuf, String> {
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
    let dir = download_and_install(candidate, &dist_bases, &mut |received, total| {
        event(EnsureEvent::Bytes { received, total });
    })?;
    set_default_version(candidate)?;
    drop(guard);
    Ok(dir.join("node.exe"))
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
fn shasums_hash_for(text: &str, archive_name: &str) -> Option<String> {
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
        let agent = ureq::AgentBuilder::new()
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

/// 本地文件 sha256（hex 小写）。
fn file_sha256(path: &Path) -> Result<String, String> {
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
/// `progress` 每 ≥256KB 或状态变化回调一次 (received, total)；total 为 0 表示未知长度。
/// 成功返回安装目录；所有候选都不可达时返回最后一个错误。
/// P2-03：下载前先取官方 SHASUMS256 校验和（取不到即拒绝），落盘 zip 必须
/// 哈希一致才解压。
pub fn download_and_install(
    version: &str,
    dist_bases: &[&str],
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let (ma, mi, pa) =
        parse_node_version(version).ok_or_else(|| format!("非法版本号：{version:?}"))?;
    let archive_name = format!("node-v{ma}.{mi}.{pa}-win-x64.zip");
    if dist_bases.is_empty() {
        return Err("下载网关未提供任何 Node 发行包通道".into());
    }
    let expected = expected_sha256(&format!("{ma}.{mi}.{pa}"), &archive_name, dist_bases)?;

    let mut last_err = String::from("未尝试任何通道");
    DOWNLOAD_CANCEL.store(false, Ordering::Relaxed);
    for base in dist_bases {
        if download_cancelled() {
            return Err("下载已取消".into());
        }
        let url = format!("{base}/v{ma}.{mi}.{pa}/{archive_name}");
        match try_download(&url, &archive_name, progress) {
            Ok(zip_path) => {
                let actual = file_sha256(&zip_path).unwrap_or_default();
                if actual != expected {
                    let _ = std::fs::remove_file(&zip_path);
                    last_err = format!(
                        "[{base}] 哈希校验失败（期望 {expected}，实际 {actual}）——已丢弃下载产物"
                    );
                    continue;
                }
                match extract_zip(&zip_path, version) {
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

/// 单镜像下载到临时文件；成功返回 zip 本地路径。crate 内复用（Node 便携包、
/// DSH 源码归档共用同一套停滞看门狗与取消标志）。
///
/// 超时策略（真机走查实证：官方直连会「连接成功但数据流停滞」）：
/// - `timeout_connect` 20s：连接建立失败快速换通道；
/// - `timeout_read` 60s：**停滞看门狗**——单次读 60s 无数据即报错，由上层
///   循环自动切换下一通道，「永远下载中」不可能再出现；
/// - `timeout` 30min：整体兜底，持续有慢速数据也不允许无限占用。
pub(crate) fn try_download(
    url: &str,
    archive_name: &str,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(60))
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("下载 {url} 失败：{e}"))?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let tmp_dir = std::env::temp_dir();
    let zip_path = tmp_dir.join(archive_name);
    let mut file = std::fs::File::create(&zip_path)
        .map_err(|e| format!("创建临时文件 {} 失败：{e}", zip_path.display()))?;

    let mut reader = response.into_reader();
    let mut received: u64 = 0;
    let mut next_report: u64 = 256 * 1024;
    let mut buf = [0u8; 64 * 1024];
    loop {
        if download_cancelled() {
            return Err(format!(
                "下载已取消（已接收 {}）",
                fmt_size(received)
            ));
        }
        let n = reader.read(&mut buf).map_err(|e| {
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
    // 长度为已知且不匹配视为损坏，避免解压半包。
    if total != 0 && received != total {
        return Err(format!(
            "下载不完整：预期 {total} 字节，实际 {received} 字节"
        ));
    }
    Ok(zip_path)
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

    if !target.join("node.exe").is_file() {
        return Err(format!(
            "解压完成但 {} 不存在，产物异常",
            target.join("node.exe").display()
        ));
    }
    verify_installed_version(&target, version)?;
    Ok(target)
}

/// 安装后复验：运行解压出的 node --version 必须与目标版本一致。
fn verify_installed_version(dir: &Path, version: &str) -> Result<(), String> {
    let out = std::process::Command::new(dir.join("node.exe"))
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

    /// 取消标志：置位后 try 循环应中止（不经网络验证，只验标志语义与重置）。
    #[test]
    fn cancel_flag_sets_and_resets() {
        cancel_download();
        assert!(download_cancelled());
        // download_and_install 入口统一重置；直接模拟一次入口行为。
        DOWNLOAD_CANCEL.store(false, Ordering::Relaxed);
        assert!(!download_cancelled());
    }

    /// 验收「一键下载成功 + 进度反馈 + ≥2 版本共存 + 全局默认生效」的真实下载验证。
    /// 慢速（两版 zip 共 ~60MB）+ 需网络，默认 `#[ignore]`，显式 `cargo test -- --ignored`。
    #[test]
    #[ignore]
    fn real_download_installs_two_versions_with_progress() {
        let (_lock, _tmp) = sandbox("download-real");
        let bases = crate::gateway::node_dist_bases();
        let mut progress_events: usize = 0;
        for version in ["22.20.0", "24.12.0"] {
            let dir = download_and_install(version, &bases, &mut |received, total| {
                progress_events += 1;
                println!("[{version}] 进度回调 #{progress_events}: {received}/{total} 字节");
                assert!(total == 0 || received <= total, "进度越界");
            })
            .unwrap_or_else(|e| panic!("下载 {version} 失败：{e}"));
            assert!(dir.join("node.exe").is_file(), "{version} 安装目录缺 node.exe");
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
