//! 启动器自更新（FR-13）：经 GitHub Releases 检查新版并获取安装包。
//!
//! 无服务端红线：GitHub 公开接口，经统一下载网关（镜像前缀 / 代理 / 统一
//! agent 生效）。更新源 = config.json 的 `update_repo`（`owner/repo`）——
//! 未配置时如实报告「未配置更新源」（启动器尚未发布时的合法状态，P7 发布
//! 时写入）。安装包 = Release 资产中首个 `.exe`（缺失回落 `.msi`）；
//! 下载进统一任务队列（任务键 `update`，支持暂停 / 继续 / 取消与断点续传）。
//! 启动器只负责「下载 + 打开安装包」，安装流程交给系统安装器。

use serde::Serialize;
use std::io::Read;
use std::path::PathBuf;

use crate::gateway;
use crate::mirror_config;
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

fn not_configured(current_version: &str) -> UpdateStatus {
    UpdateStatus {
        configured: false,
        current_version: current_version.into(),
        latest_version: None,
        release_notes: None,
        asset_name: None,
        has_update: false,
    }
}

/// 查最新 Release（阻塞网络调用——command 侧 spawn_blocking）。
pub fn check(current_version: &str) -> Result<UpdateStatus, String> {
    let repo = node_runtime::load_config()?
        .update_repo
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());
    let Some(repo) = repo else {
        return Ok(not_configured(current_version));
    };
    let repo = repo.trim_start_matches("https://github.com/").to_string();
    let settings = mirror_config::load()?;
    let url = gateway::resolve(
        &format!("https://api.github.com/repos/{repo}/releases/latest"),
        &settings,
    )?;
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
    // Windows 安装包：优先 .exe，其次 .msi。
    let pick_asset = |suffix: &str| -> Option<(String, String)> {
        value.get("assets")?.as_array()?.iter().find_map(|a| {
            let name = a.get("name")?.as_str()?.to_string();
            let url = a.get("browser_download_url")?.as_str()?.to_string();
            (name.to_ascii_lowercase().ends_with(suffix)).then_some((name, url))
        })
    };
    let asset = pick_asset(".exe").or_else(|| pick_asset(".msi"));
    let has_update = latest_version
        .as_deref()
        .map(|l| newer(l, current_version))
        .unwrap_or(false);
    Ok(UpdateStatus {
        configured: true,
        current_version: current_version.into(),
        latest_version,
        release_notes,
        asset_name: asset.as_ref().map(|(n, _)| n.clone()),
        has_update,
    })
}

/// 下载最新安装包（阻塞；任务键 `update`，统一队列 + Range 断点续传）。
/// 返回本地安装包路径（调用方经 update_install 打开触发系统安装器）。
pub fn download(current_version: &str, control: &task_queue::TaskControl) -> Result<PathBuf, String> {
    let status = check(current_version)?;
    if !status.has_update {
        return Err("当前已是最新版本，无需下载".into());
    }
    let asset_name = status.asset_name.clone().unwrap_or_else(|| "dsh-launcher-setup.exe".into());
    let asset_url = {
        let repo = node_runtime::load_config()?
            .update_repo
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty())
            .ok_or("未配置更新源")?;
        let repo = repo.trim_start_matches("https://github.com/").to_string();
        let settings = mirror_config::load()?;
        let tag = format!("v{}", status.latest_version.unwrap_or_default());
        gateway::resolve(
            &format!("https://github.com/{repo}/releases/download/{tag}/{asset_name}"),
            &settings,
        )?
    };
    let file_name = asset_name;
    let part = task_queue::part_path("update");
    node_runtime::try_download(&asset_url, &part, Some(control), &mut |_received, _total| {})
        .map(|_| ())
        .map_err(|e| if e.starts_with(task_queue::CANCELLED_PREFIX) { e } else { format!("下载安装包失败：{e}") })?;
    // 断点文件即最终产物；改名成正经安装包名便于用户辨认。
    let final_path = std::env::temp_dir().join(&file_name);
    std::fs::rename(&part, &final_path)
        .map_err(|e| format!("就位安装包失败：{e}"))?;
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
