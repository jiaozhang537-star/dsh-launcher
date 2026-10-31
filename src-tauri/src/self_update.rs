//! 启动器自更新（FR-13）：经 GitHub Releases 检查新版并获取安装包。
//!
//! 无服务端红线：GitHub 公开接口。更新源 = config.json 的 `update_repo`
//! 显式配置优先，未配置回落内置默认 `imccyu/dsh-launcher`（P7 W1；仓库尚无
//! Release 时如实报 404 可读文案）。安装包 = Release 资产中首个 `.exe`（缺失
//! 回落 `.msi`）；下载进统一任务队列（任务键 `update`，支持暂停 / 继续 / 取消
//! 与断点续传）。启动器只负责「下载 + 打开安装包」，安装流程交给系统安装器。
//!
//! 安全口径（对抗审查 R1 2026-10-05 P1-01）：
//! - 检查与下载**恒为 GitHub 官方直连**，不经统一网关的镜像改写——安装包是
//!   可执行代码，取用通道绝不交给可配置的第三方前缀（`gateway::resolve` 的
//!   custom 改写只服务非执行内容）；用户代理设置仍生效（base_agent）。
//! - 资产名白名单消毒后才能拼 URL / 落盘：远端 JSON 的 name 不可信，含路径
//!   分隔符等字符的名字会让 `temp_dir().join(name)` 写穿临时目录。
//! - **SHA256 强校验**（与 Node 发行包同口径）：Release 必须附 `SHA256SUMS.txt`
//!   资产，缺失、无条目或不匹配一律拒绝交付安装包——发布侧要求已入 P7 计划。

use serde::Serialize;
use std::io::Read;
use std::path::PathBuf;

use crate::gateway;
use crate::node_runtime;
use crate::task_queue;

/// 最新 Release 的规范化视图（IPC camelCase）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// 是否已配置更新源（未配置时其余可空字段为空）。
    pub configured: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub release_notes: Option<String>,
    pub asset_name: Option<String>,
    /// 最新版是否比当前新（三段数字比较）。
    pub has_update: bool,
}

/// `vX.Y.Z[-pre]` → 三段数字；解析失败返回 None（预发布段不参与比较）。
fn parse_triple(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn newer(latest: &str, current: &str) -> bool {
    match (parse_triple(latest), parse_triple(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// 安装包资产的后缀优先级（前者优先）。平台相关——其他平台的安装器形态
/// 在移植打包层时一并定义。
fn installer_suffixes() -> &'static [&'static str] {
    if cfg!(windows) {
        &[".exe", ".msi"]
    } else {
        &[".deb", ".AppImage"]
    }
}

/// 校验文件资产名（与安装包同 Release 发布）。
const CHECKSUM_ASSET: &str = "SHA256SUMS.txt";

/// 内置默认更新源（P7 W1）：config.json 的 `update_repo` 未配置时回落——
/// 用户无需手工配置即可获得检查更新能力；仓库还没有 Release 时如实报
/// 「404 通常 = 更新源还没有任何正式发布」。config.json 显式配置恒优先。
const DEFAULT_UPDATE_REPO: &str = "imccyu/dsh-launcher";

/// 生效更新源：config.json 显式配置优先，缺省回落 [`DEFAULT_UPDATE_REPO`]。
pub(crate) fn effective_update_repo() -> Option<String> {
    node_runtime::load_config()
        .ok()
        .and_then(|c| c.update_repo)
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .or_else(|| Some(DEFAULT_UPDATE_REPO.to_string()))
}

/// 资产名消毒：只放行 `[A-Za-z0-9._-]` 且不含 `..`、长度合理。远端 JSON 里的
/// 名字在拼下载 URL / 就位落盘前必须过这道闸。
fn sanitize_asset_name(name: &str) -> Option<String> {
    if name.is_empty() || name.len() > 128 {
        return None;
    }
    if name.contains("..") {
        return None;
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return None;
    }
    Some(name.to_string())
}

/// 从 Release JSON 里挑选（消毒后的）安装包资产与校验文件资产。
/// 安装包按 [`installer_suffixes`] 的优先级取第一个命中的资产。
fn pick_assets(value: &serde_json::Value) -> (Option<(String, String)>, Option<(String, String)>) {
    let Some(assets) = value.get("assets").and_then(|v| v.as_array()) else {
        return (None, None);
    };
    let mut installer = None;
    'outer: for suffix in installer_suffixes() {
        for a in assets {
            let Some(name) = a.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(suffix) {
                continue;
            }
            let Some(url) = a.get("browser_download_url").and_then(|v| v.as_str()) else {
                continue;
            };
            if let Some(safe) = sanitize_asset_name(name) {
                installer = Some((safe, url.to_string()));
                break 'outer;
            }
        }
    }
    let checksum = assets.iter().find_map(|a| {
        let name = a.get("name")?.as_str()?;
        if !name.eq_ignore_ascii_case(CHECKSUM_ASSET) {
            return None;
        }
        let url = a.get("browser_download_url")?.as_str()?;
        Some((name.to_string(), url.to_string()))
    });
    (installer, checksum)
}

/// Release 查询的内部视图（check 与 download 共用，避免双解析漂移）。
struct ReleaseInfo {
    latest_version: Option<String>,
    release_notes: Option<String>,
    /// （消毒后的资产名, 下载 URL）。
    installer: Option<(String, String)>,
    /// （资产名, 下载 URL）。
    checksum: Option<(String, String)>,
    has_update: bool,
}

/// 查询更新源的最新 Release（阻塞网络调用——command 侧 spawn_blocking）。
/// 恒为 GitHub 官方直连（见模块文档安全口径），代理设置经 base_agent 生效。
fn fetch_release(current_version: &str) -> Result<ReleaseInfo, String> {
    let repo = effective_update_repo().ok_or("未配置更新源")?;
    let repo = repo.trim_start_matches("https://github.com/").to_string();
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let agent = gateway::base_agent()
        .timeout_connect(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let response = agent
        .get(&url)
        .set("User-Agent", "dsh-launcher")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("检查更新失败：{e}（404 通常 = 更新源还没有任何正式发布）"))?;
    let mut body = String::new();
    response
        .into_reader()
        .take(8 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| format!("读取更新信息失败：{e}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("解析更新信息失败：{e}"))?;
    if let Some(msg) = value.get("message").and_then(|v| v.as_str()) {
        return Err(format!("检查更新失败：GitHub 返回「{msg}」"));
    }
    let latest_version = value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches('v').to_string());
    let release_notes = value
        .get("body")
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(2000).collect::<String>());
    let (installer, checksum) = pick_assets(&value);
    let has_update = latest_version
        .as_deref()
        .map(|l| newer(l, current_version))
        .unwrap_or(false);
    Ok(ReleaseInfo {
        latest_version,
        release_notes,
        installer,
        checksum,
        has_update,
    })
}

/// 查最新 Release（阻塞网络调用——command 侧 spawn_blocking）。
/// 内置默认源生效后（P7 W1）不再有「未配置更新源」态：仓库还没有 Release
/// 时如实报 404 可读文案。`configured` 字段保留作 IPC 协议稳定，恒为 true。
pub fn check(current_version: &str) -> Result<UpdateStatus, String> {
    let info = fetch_release(current_version)?;
    Ok(UpdateStatus {
        configured: true,
        current_version: current_version.into(),
        latest_version: info.latest_version,
        release_notes: info.release_notes,
        asset_name: info.installer.as_ref().map(|(n, _)| n.clone()),
        has_update: info.has_update,
    })
}

/// 下载校验文件内容（小文件，直连 + base_agent 代理）。
fn fetch_checksum_text(url: &str) -> Result<String, String> {
    let agent = gateway::base_agent()
        .timeout_connect(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let response = agent
        .get(url)
        .set("User-Agent", "dsh-launcher")
        .call()
        .map_err(|e| format!("下载校验文件失败：{e}"))?;
    let mut text = String::new();
    response
        .into_reader()
        .take(4 * 1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|e| format!("读取校验文件失败：{e}"))?;
    Ok(text)
}

/// 下载最新安装包（阻塞；任务键 `update`，统一队列 + Range 断点续传）。
/// 返回本地安装包路径（调用方经 update_install 打开触发系统安装器）。
/// SHA256 强校验：Release 缺少 SHA256SUMS.txt、清单无条目或哈希不匹配，
/// 一律拒绝就位——安装包是可执行内容，这是最后一道内容防线。
pub fn download(current_version: &str, control: &task_queue::TaskControl) -> Result<PathBuf, String> {
    let info = fetch_release(current_version)?;
    if !info.has_update {
        return Err("当前已是最新版本，无需下载".into());
    }
    let (asset_name, asset_url) = info
        .installer
        .ok_or_else(|| "更新源发布里没有可安装的安装包资产（.exe / .msi）".to_string())?;
    let (sums_name, sums_url) = info
        .checksum
        .ok_or_else(|| format!(
            "更新源发布缺少 {CHECKSUM_ASSET} 校验文件——为防安装包被篡改已拒绝下载；\
             请在发布 Release 时附上与安装包同源的校验清单"
        ))?;
    let sums_text = fetch_checksum_text(&sums_url)?;
    let expected = node_runtime::shasums_hash_for(&sums_text, &asset_name).ok_or_else(|| {
        format!(
            "{sums_name} 里没有「{asset_name}」的校验条目——校验清单与安装包不同源，已拒绝下载"
        )
    })?;

    let part = task_queue::part_path("update");
    node_runtime::try_download(&asset_url, &part, Some(control), &mut |_received, _total| {})
        .map(|_| ())
        .map_err(|e| if e.starts_with(task_queue::CANCELLED_PREFIX) { e } else { format!("下载安装包失败：{e}") })?;

    let actual = node_runtime::file_sha256(&part)?;
    if actual != expected {
        let _ = std::fs::remove_file(&part);
        return Err(format!(
            "安装包哈希校验失败（期望 {expected}，实际 {actual}）——已丢弃下载产物，请重试或检查网络环境"
        ));
    }
    // 资产名已过消毒白名单（无路径分隔符），join 到 temp 目录是安全的。
    let final_path = std::env::temp_dir().join(&asset_name);
    std::fs::rename(&part, &final_path)
        .map_err(|e| format!("就位安装包失败：{e}"))?;
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 内置默认更新源（P7 W1）：config.json 显式配置优先，缺省回落默认。
    #[test]
    fn effective_update_repo_prefers_config_then_default() {
        let (_lock, tmp) = crate::instances::tests::sandbox("self-update-repo");
        // 无 config.json → 回落内置默认。
        assert_eq!(effective_update_repo().as_deref(), Some(DEFAULT_UPDATE_REPO));
        // 显式配置恒优先。
        crate::node_runtime::save_config(&crate::node_runtime::LauncherConfig {
            update_repo: Some("someone/else-repo".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(effective_update_repo().as_deref(), Some("someone/else-repo"));
        // 空串视同未配置。
        crate::node_runtime::save_config(&crate::node_runtime::LauncherConfig {
            update_repo: Some("   ".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(effective_update_repo().as_deref(), Some(DEFAULT_UPDATE_REPO));
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 版本比较：三段数字、容忍 v 前缀与预发布段。
    #[test]
    fn version_compare_ignores_v_prefix_and_prerelease() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("0.1.1", "v0.1.0"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0-rc.1", "0.1.0"));
        assert!(newer("0.2.0-rc.1", "0.1.9"));
        assert!(newer("1.0.0", "0.9.9"));
        assert!(!newer("banana", "0.1.0"), "解析失败一律不报新版");
    }

    /// 资产名消毒（R1 P1-01）：常规名放行；路径分隔符、`..`、空白、
    /// Unicode 与超长一律拒绝——temp 落点用 join，名字不干净 = 任意路径写。
    #[test]
    fn sanitize_asset_name_rejects_path_and_dirty_characters() {
        assert_eq!(
            sanitize_asset_name("dsh-launcher-setup-0.2.0.exe").as_deref(),
            Some("dsh-launcher-setup-0.2.0.exe")
        );
        assert_eq!(sanitize_asset_name("dsh-launcher_0.2.msi").as_deref(), Some("dsh-launcher_0.2.msi"));
        assert!(sanitize_asset_name("../evil.exe").is_none(), "相对路径段必须拒绝");
        assert!(sanitize_asset_name("a/b.exe").is_none(), "正斜杠必须拒绝");
        assert!(sanitize_asset_name("a\\b.exe").is_none(), "反斜杠必须拒绝");
        assert!(sanitize_asset_name("..").is_none());
        assert!(sanitize_asset_name("a b.exe").is_none(), "空白必须拒绝");
        assert!(sanitize_asset_name("安装包.exe").is_none(), "非 ASCII 必须拒绝");
        assert!(sanitize_asset_name("").is_none());
        assert!(sanitize_asset_name(&"a".repeat(129)).is_none(), "超长必须拒绝");
    }

    /// 资产挑选：按后缀优先级取第一个；名字不过消毒的资产跳过（而不是整体失败）；
    /// SHA256SUMS.txt 独立挑出（大小写不敏感）。
    #[test]
    fn pick_assets_prefers_exe_and_skips_unsanitized_names() {
        let value = json!({
            "assets": [
                { "name": "source.zip", "browser_download_url": "https://github.com/a/b/releases/download/v1/source.zip" },
                { "name": "../evil.exe", "browser_download_url": "https://github.com/a/b/releases/download/v1/x.exe" },
                { "name": "dsh-launcher-setup-0.2.0.msi", "browser_download_url": "https://github.com/a/b/releases/download/v1/dsh.msi" },
                { "name": "dsh-launcher-setup-0.2.0.exe", "browser_download_url": "https://github.com/a/b/releases/download/v1/dsh.exe" },
                { "name": "sha256sums.txt", "browser_download_url": "https://github.com/a/b/releases/download/v1/SHA256SUMS.txt" }
            ]
        });
        let (installer, checksum) = pick_assets(&value);
        // .exe 优先于 .msi；../evil.exe 未过消毒，不挡后面的合法资产。
        assert_eq!(
            installer.unwrap().0,
            "dsh-launcher-setup-0.2.0.exe",
            "消毒失败的资产应被跳过，后继合法资产命中"
        );
        assert_eq!(checksum.unwrap().0, "sha256sums.txt");
    }

    /// 缺安装包 / 缺校验文件的形态各自如实返回 None（download 侧据此 fail loud）。
    #[test]
    fn pick_assets_reports_missing_pieces() {
        let no_installer = json!({ "assets": [ { "name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/a/b/SHA256SUMS.txt" } ] });
        let (installer, checksum) = pick_assets(&no_installer);
        assert!(installer.is_none());
        assert!(checksum.is_some());

        let no_checksum = json!({ "assets": [ { "name": "setup.exe", "browser_download_url": "https://github.com/a/b/setup.exe" } ] });
        let (installer, checksum) = pick_assets(&no_checksum);
        assert!(installer.is_some());
        assert!(checksum.is_none(), "缺校验文件必须显性可见");

        let empty = json!({ "assets": [] });
        assert!(pick_assets(&empty).0.is_none());
    }
}
