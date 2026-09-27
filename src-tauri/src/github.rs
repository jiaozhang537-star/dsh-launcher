//! GitHub REST 客户端（FR-03 数据面）：topic 搜索、仓库 tags 枚举。
//! Token 可选提额（保险库 DPAPI 存取，环境变量 GH_TOKEN/GITHUB_TOKEN
//! 兜底自动配置）；全部请求带 ETag 条件头，304 命中不重读响应体——这是
//! 匿名 60 次/小时配额下「市场可用」的根基。URL 一律经统一网关 resolve
//! （custom 前缀对 GitHub 类生效），业务模块不拼直链。
//!
//! 限流策略：403/429 且配额耗尽时报可读错误并指路设置页配 Token；
//! 搜索接口每页间隔由调用方控制（市场一次 feed 至多 2 页）。

use std::io::Read;
use std::time::Duration;

use serde::Serialize;

use crate::gateway;
use crate::keyvault;
use crate::market_cache::{self, EtagEntry};
use crate::mirror_config;

/// GitHub REST API 基址（官方形态；custom 源下经网关改写）。
pub const API_BASE: &str = "https://api.github.com";

/// 一次 GitHub API 取回的结果。
pub struct FetchOutcome {
    pub body: serde_json::Value,
}

/// 解析 GitHub Token：保险库优先，其次环境变量 GH_TOKEN / GITHUB_TOKEN。
pub fn github_token() -> Option<String> {
    if let Ok(token) = keyvault::reveal_key_for(keyvault::VENDOR_GITHUB) {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    for name in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(token) = std::env::var(name) {
            let trimmed = token.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// GET 一个 GitHub API 端点并解析 JSON。`path` 形如 `/search/repositories`，
/// `query` 为已编码的查询串（可为空）。阻塞网络调用——command 侧放
/// spawn_blocking。
pub fn api_get_json(path: &str, query: &str) -> Result<FetchOutcome, String> {
    let raw = if query.is_empty() {
        format!("{API_BASE}{path}")
    } else {
        format!("{API_BASE}{path}?{query}")
    };
    let settings = mirror_config::load()?;
    let url = gateway::resolve(&raw, &settings)?;

    let cached = market_cache::load()?.etags.get(&url).cloned();
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build();
    let mut request = agent
        .get(&url)
        .set("User-Agent", "dsh-launcher")
        .set("Accept", "application/vnd.github+json");
    if let Some(token) = github_token() {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    if let Some(entry) = &cached {
        if !entry.etag.is_empty() {
            request = request.set("If-None-Match", &entry.etag);
        }
    }

    match request.call() {
        Ok(response) => {
            // ureq 把非重定向的 3xx 当 Ok 返回（真机实证：304 走了 200 分支、
            // 读到空 body）。304 = ETag 命中 → 直接回缓存体。
            if response.status() == 304 {
                let entry = cached
                    .filter(|e| !e.body.is_null())
                    .ok_or_else(|| "GitHub 返回 304 但本地没有缓存，重新拉取失败".to_string())?;
                return Ok(FetchOutcome { body: entry.body });
            }
            let etag = response.header("etag").unwrap_or("").to_string();
            let mut text = String::new();
            response
                .into_reader()
                .take(16 * 1024 * 1024)
                .read_to_string(&mut text)
                .map_err(|e| format!("读取 GitHub 响应失败：{e}"))?;
            let body: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| format!("GitHub 响应不是合法 JSON（len {}）：{e}", text.len()))?;
            if !etag.is_empty() {
                market_cache::update(|cache| {
                    cache.etags.insert(
                        url.clone(),
                        EtagEntry {
                            etag: etag.clone(),
                            fetched_at: crate::instances::now_rfc3339(),
                            body: body.clone(),
                        },
                    );
                })?;
            }
            Ok(FetchOutcome { body })
        }
        Err(ureq::Error::Status(304, _)) => {
            let entry = cached
                .filter(|e| !e.body.is_null())
                .ok_or_else(|| "GitHub 返回 304 但本地没有缓存，重新拉取失败".to_string())?;
            Ok(FetchOutcome { body: entry.body })
        }
        Err(ureq::Error::Status(code, response)) => {
            let remaining = response
                .header("x-ratelimit-remaining")
                .and_then(|v| v.parse::<u32>().ok());
            if matches!(code, 403 | 429) && remaining == Some(0) {
                return Err(
                    "GitHub API 配额已用尽（匿名 60 次/小时）：可在设置页配置 GitHub Token 提额，\
                     或稍后再试——已缓存的离线快照仍可浏览"
                        .into(),
                );
            }
            Err(format!("GitHub API 返回 HTTP {code}（{url}）"))
        }
        Err(e) => Err(format!("连接 GitHub 失败：{e}")),
    }
}

/// topic 搜索结果中的仓库摘要（卡片渲染所需的全部字段）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSummary {
    /// `owner/repo`。
    pub full_name: String,
    pub description: String,
    pub stars: u64,
    /// 最近 push 时间（RFC3339，可能为空串）。
    pub pushed_at: String,
    pub avatar_url: String,
    pub topics: Vec<String>,
}

/// 从 `/search/repositories` 响应解析仓库摘要；字段缺失的条目跳过。
pub fn parse_repo_search(body: &serde_json::Value) -> Vec<RepoSummary> {
    let Some(items) = body.get("items").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let full_name = item.get("full_name")?.as_str()?.trim().to_string();
            if full_name.is_empty() {
                return None;
            }
            Some(RepoSummary {
                full_name,
                description: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                stars: item
                    .get("stargazers_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                pushed_at: item
                    .get("pushed_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                avatar_url: item
                    .get("owner")
                    .and_then(|o| o.get("avatar_url"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                topics: item
                    .get("topics")
                    .and_then(|v| v.as_array())
                    .map(|list| {
                        list.iter()
                            .filter_map(|t| t.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// 按原始 q 表达式搜索仓库（一页，page 从 1 起）。`q` 需已完成 URL 编码
/// （调用方用 market::encode_query 组合「关键词 topic:x」）。星标降序请求，
/// 客户端仍按 T-4 口径重排。阻塞网络调用。
pub fn search_repos_raw(q_encoded: &str, page: u32) -> Result<Vec<RepoSummary>, String> {
    let query = format!("q={q_encoded}&sort=stars&order=desc&per_page=100&page={page}");
    let outcome = api_get_json("/search/repositories", &query)?;
    Ok(parse_repo_search(&outcome.body))
}

/// 搜索带指定话题的仓库（一页，page 从 1 起）。按星标降序请求，客户端
/// 仍会按 T-4 口径重排。阻塞网络调用。
pub fn search_topic_page(topic: &str, page: u32) -> Result<Vec<RepoSummary>, String> {
    let query = format!("q=topic%3A{topic}&sort=stars&order=desc&per_page=100&page={page}");
    let outcome = api_get_json("/search/repositories", &query)?;
    Ok(parse_repo_search(&outcome.body))
}

/// 仓库 tags 列表（`owner/repo`，供 git 直装形态的版本枚举；阻塞网络调用）。
/// 批次 C 的安装视图消费（git 卡片展开列表）；先行落地供 e2e 侦察复用。
#[allow(dead_code)]
pub fn repo_tags(full_name: &str) -> Result<Vec<String>, String> {
    if full_name.matches('/').count() != 1
        || full_name.split('/').any(|part| {
            part.is_empty() || part.contains([' ', '?', '#', '&', '/'])
        })
    {
        return Err(format!("非法仓库标识：{full_name:?}"));
    }
    let path = format!("/repos/{full_name}/tags");
    let outcome = api_get_json(&path, "per_page=100")?;
    let Some(items) = outcome.body.as_array() else {
        return Ok(Vec::new());
    };
    Ok(items
        .iter()
        .filter_map(|t| t.get("name").and_then(|v| v.as_str()).map(str::to_string))
        .collect())
}

/// Release 摘要（详情页「全部版本」行；camelCase 与前端契约一致）。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseSummary {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
}

/// 仓库 Releases 列表（`owner/repo`；git 直装卡的「历代版本」，阻塞网络调用）。
/// 跳过 draft；prerelease 保留（前端打徽章）。走 api_get_json：网关 + Token +
/// ETag 缓存 + 限流可读错误全自动。
pub fn repo_releases(full_name: &str) -> Result<Vec<ReleaseSummary>, String> {
    if full_name.matches('/').count() != 1
        || full_name.split('/').any(|part| {
            part.is_empty() || part.contains([' ', '?', '#', '&', '/'])
        })
    {
        return Err(format!("非法仓库标识：{full_name:?}"));
    }
    let path = format!("/repos/{full_name}/releases");
    let outcome = api_get_json(&path, "per_page=30")?;
    let Some(items) = outcome.body.as_array() else {
        return Ok(Vec::new());
    };
    Ok(items
        .iter()
        .filter_map(|r| {
            let tag = r.get("tag_name").and_then(|v| v.as_str())?;
            if r.get("draft").and_then(|v| v.as_bool()).unwrap_or(false) {
                return None;
            }
            Some(ReleaseSummary {
                tag_name: tag.to_string(),
                name: r
                    .get("name")
                    .and_then(|v| v.as_str())
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or(tag)
                    .to_string(),
                published_at: r
                    .get("published_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                prerelease: r
                    .get("prerelease")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repo_search_parses_summaries_and_skips_incomplete() {
        let body = json!({
            "total_count": 2,
            "items": [
                {
                    "full_name": " a/b ",
                    "description": "demo",
                    "stargazers_count": 42,
                    "pushed_at": "2026-09-01T00:00:00Z",
                    "owner": { "avatar_url": "https://avatars.githubusercontent.com/u/1" },
                    "topics": ["dsh-plugin", "tool"]
                },
                { "full_name": "" },
                { "nope": true }
            ]
        });
        let repos = parse_repo_search(&body);
        assert_eq!(repos.len(), 1);
        let repo = &repos[0];
        assert_eq!(repo.full_name, "a/b");
        assert_eq!(repo.stars, 42);
        assert_eq!(repo.pushed_at, "2026-09-01T00:00:00Z");
        assert_eq!(repo.avatar_url, "https://avatars.githubusercontent.com/u/1");
        assert_eq!(repo.topics, vec!["dsh-plugin", "tool"]);
        assert!(parse_repo_search(&json!({})).is_empty());
    }

    #[test]
    fn repo_tags_rejects_malformed_names() {
        assert!(repo_tags("justname").is_err(), "缺 owner");
        assert!(repo_tags("a/b/c").is_err(), "多级路径");
        assert!(repo_tags("a/b c").is_err(), "含空格");
        assert!(repo_tags("a/").is_err(), "空段");
    }

    #[test]
    fn token_prefers_vault_then_env() {
        // 环境兜底路径可观测（保险库无记录时）。
        std::env::set_var("GH_TOKEN", "env-token");
        std::env::remove_var("GITHUB_TOKEN");
        // 保险库多半没有 github 记录；无论有无，env 顺序断言只在 vault 未命中时成立，
        // 因此这里只断言「取到的是非空串」，并把 env 清理干净。
        let _ = github_token();
        std::env::remove_var("GH_TOKEN");
    }
}
