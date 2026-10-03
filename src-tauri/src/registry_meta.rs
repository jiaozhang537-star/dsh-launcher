//! npm registry 元数据（R-2.4）：拉取 `@deepseek-ai/dsh` 的 packument，
//! 解析出可选安装的版本列表。网络经统一下载网关取 registry 基址。
//!
//! 解析与排序是纯函数（cargo 可测）；网络拉取是薄封装。

use serde::Serialize;
use serde_json::Value;
use std::io::Read;

use crate::gateway;
use crate::mirror_config;

/// 包名：DSH CLI 元包。
pub const DSH_PACKAGE: &str = "@deepseek-ai/dsh";

/// 一个可安装版本的前端视图。
#[derive(Debug, Clone, Serialize)]
pub struct DshVersion {
    /// semver 版本串（如 `0.1.1-rc.2`）。
    pub version: String,
    /// 是否 dist-tags.latest 指向的最新版。
    pub latest: bool,
    /// 发布时间（RFC3339 原样透传，供「最新」排序展示）。
    #[serde(rename = "publishedAt")]
    pub published_at: String,
}

/// prerelease 标识的排序单元：数字标识恒小于字母数字标识（semver 规范第 11 条），
/// 同类按数值 / ASCII 比较；Vec<PreId> 的字典序自动满足「前缀相等时字段多者大」。
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PreId {
    Num(u64),
    Alpha(String),
}

/// packument 解析失败的具体原因（fail loud，不静默造空表）。
fn parse_error(context: &str) -> String {
    format!("解析 {DSH_PACKAGE} 的 registry 元数据失败：{context}")
}

/// 从 packument JSON 提取版本列表，按 semver 降序（prerelease 参与比较）。
/// 结构不符合预期时返回 Err；个别版本缺失时间允许为空串。
pub fn parse_packument(packument: &Value) -> Result<Vec<DshVersion>, String> {
    let versions = packument
        .get("versions")
        .and_then(|v| v.as_object())
        .ok_or_else(|| parse_error("缺少 versions 对象"))?;
    let times = packument.get("time").and_then(|v| v.as_object());
    let latest_tag = packument
        .get("dist-tags")
        .and_then(|v| v.get("latest"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    for name in versions.keys() {
        parse_semver_key(name)
            .ok_or_else(|| parse_error(&format!("非法版本号 {name:?}")))?;
    }

    let mut out: Vec<DshVersion> = versions
        .keys()
        .map(|name| DshVersion {
            version: name.clone(),
            latest: name == latest_tag,
            published_at: times
                .and_then(|t| t.get(name))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
        .collect();
    out.sort_by(|a, b| semver_sort_key(&b.version).cmp(&semver_sort_key(&a.version)));
    Ok(out)
}

/// semver 排序键 `(major, minor, patch, 是正式版?, prerelease 标识序列)`：
/// 正式版 > 同核 prerelease；prerelease 逐段比较。
pub fn semver_sort_key(version: &str) -> (u64, u64, u64, bool, Vec<PreId>) {
    let (core, pre) = match version.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (version, None),
    };
    let mut nums = core.split('.');
    let major = nums.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let minor = nums.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let patch = nums.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);

    match pre {
        None => (major, minor, patch, true, Vec::new()),
        Some(pre) => (
            major,
            minor,
            patch,
            false,
            pre.split('.')
                .map(|id| match id.parse::<u64>() {
                    Ok(num) => PreId::Num(num),
                    Err(_) => PreId::Alpha(id.to_string()),
                })
                .collect(),
        ),
    }
}

/// 版本键合法性：必须是 `X.Y.Z[-pre]` 形态且 X/Y/Z 为非空数字段；pre 段
/// 存在时必须为点分标识符、每段非空且仅含字母/数字/连字符（semver 规范 §9）。
/// P3-07：pre 段会被原样拼进源码 tag 与下载 URL（source_archive），不接受
/// 任意字符——安装场景的同类白名单见 dsh_install::valid_dsh_version。
pub fn parse_semver_key(version: &str) -> Option<()> {
    let (core, pre) = match version.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (version, None),
    };
    let mut parts = core.split('.');
    for _ in 0..3 {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    parts.next().is_none().then_some(())?;
    if let Some(pre) = pre {
        for id in pre.split('.') {
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return None;
            }
        }
    }
    Some(())
}

/// 按网关候选链拉取任意 npm 包的 packument 原文（阻塞网络调用——command
/// 侧放 spawn_blocking）。通道回落语义同 fetch_versions：基址一律取自网关
/// 常量，单通道网络/解析失败自动尝试下一通道，全部失败时逐通道报告错误。
pub fn fetch_packument(package: &str) -> Result<Value, String> {
    let name = package.trim();
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@' | b'/'))
    {
        return Err(format!("非法 npm 包名：{package:?}"));
    }
    mirror_config::load()?;
    let agent = crate::gateway::base_agent()
        .timeout_connect(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(60))
        .build();

    let mut attempts: Vec<String> = Vec::new();
    for base in gateway::npm_registry_bases() {
        let url = format!("{base}/{name}");
        let result = agent
            .get(&url)
            .call()
            .map_err(|e| format!("拉取 {url} 失败：{e}"))
            .and_then(|response| {
                let mut text = String::new();
                response
                    .into_reader()
                    .take(32 * 1024 * 1024)
                    .read_to_string(&mut text)
                    .map_err(|e| format!("读取响应失败：{e}"))?;
                serde_json::from_str(&text).map_err(|e| parse_error(&format!("JSON 无效：{e}")))
            });
        match result {
            Ok(packument) => return Ok(packument),
            Err(e) => attempts.push(format!("[{base}] {e}")),
        }
    }
    Err(attempts.join("\n"))
}

/// 按网关候选链拉取版本列表（阻塞网络调用——command 侧放 spawn_blocking）。
pub fn fetch_versions() -> Result<Vec<DshVersion>, String> {
    let packument = fetch_packument(DSH_PACKAGE)?;
    parse_packument(&packument)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn semver_keys_order_prerelease_below_release_and_numeric_ids_numerically() {
        // 升序断言：每个键必须小于下一个。
        let ascending = [
            "0.0.1-rc.1",
            "0.0.1-rc.2",
            "0.0.1-rc.9",
            "0.0.1-rc.10", // 数字标识按数值比：rc.10 > rc.9
            "0.0.1",       // 正式版 > 同核任何 prerelease
            "0.1.0-rc.7",
            "0.1.0",
            "0.1.1-rc.1",
            "0.1.1-rc.2",
            "1.0.0",
        ];
        for pair in ascending.windows(2) {
            assert_eq!(
                semver_sort_key(pair[0]).cmp(&semver_sort_key(pair[1])),
                std::cmp::Ordering::Less,
                "{} 应小于 {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn numeric_identifier_ranks_below_alphanumeric_per_spec() {
        assert_eq!(
            semver_sort_key("1.0.0-1").cmp(&semver_sort_key("1.0.0-alpha")),
            std::cmp::Ordering::Less,
            "semver 第 11 条：数字标识优先级低于字母数字"
        );
        assert_eq!(
            semver_sort_key("1.0.0-alpha").cmp(&semver_sort_key("1.0.0-alpha.1")),
            std::cmp::Ordering::Less,
            "前缀相等时字段多者大"
        );
    }

    #[test]
    fn packument_parses_sorted_with_latest_tagged() {
        let packument = json!({
            "dist-tags": { "latest": "0.2.0" },
            "versions": {
                "0.1.0": {},
                "0.2.0": {},
                "0.2.0-rc.1": {}
            },
            "time": {
                "0.1.0": "2026-08-13T09:48:26.232Z",
                "0.2.0": "2026-08-21T12:42:19.422Z",
                "0.2.0-rc.1": "2026-08-20T00:00:00.000Z"
            }
        });
        let versions = parse_packument(&packument).unwrap();
        let names: Vec<&str> = versions.iter().map(|v| v.version.as_str()).collect();
        assert_eq!(names, vec!["0.2.0", "0.2.0-rc.1", "0.1.0"]);
        assert!(versions[0].latest);
        assert!(!versions[1].latest);
        assert_eq!(versions[2].published_at, "2026-08-13T09:48:26.232Z");
    }

    #[test]
    fn packument_structure_violations_fail_loud() {
        assert!(parse_packument(&json!({})).is_err(), "缺 versions");
        assert!(parse_packument(&json!({ "versions": { "banana": {} } })).is_err(), "非法版本键");
        // 缺 time/dist-tags 允许：字段留空，latest 不标。
        let ok = parse_packument(&json!({ "versions": { "1.2.3": {} } })).unwrap();
        assert_eq!(ok.len(), 1);
        assert!(!ok[0].latest);
        assert_eq!(ok[0].published_at, "");
    }

    #[test]
    fn version_key_validation_rejects_garbage() {
        assert!(parse_semver_key("1.2.3").is_some());
        assert!(parse_semver_key("1.2.3-rc.4").is_some());
        assert!(parse_semver_key("").is_none());
        assert!(parse_semver_key("1.2").is_none());
        assert!(parse_semver_key("a.b.c").is_none());
        assert!(parse_semver_key("1.2.3.4").is_none());
    }

    /// P3-07：pre 段会拼进下载 URL，必须过点分标识符白名单。
    #[test]
    fn version_key_validates_prerelease_segment_charset() {
        assert!(parse_semver_key("1.2.3-rc.4").is_some());
        assert!(parse_semver_key("1.2.3-alpha-1").is_some());
        assert!(parse_semver_key("1.2.3-0").is_some());
        assert!(parse_semver_key("1.2.3-0.1.2-3").is_some());
        assert!(parse_semver_key("1.2.3-").is_none(), "空 pre 段");
        assert!(parse_semver_key("1.2.3-rc..4").is_none(), "空标识符段");
        assert!(parse_semver_key("1.2.3-..").is_none());
        assert!(parse_semver_key("1.2.3-rc/../../evil").is_none(), "穿越字符");
        assert!(parse_semver_key("1.2.3-rc.4 x").is_none(), "空格");
        assert!(parse_semver_key("1.2.3-rc_4").is_none(), "下划线不在 semver 字符集");
        assert!(parse_semver_key("1.2.3-rc:4").is_none(), "冒号");
    }
}
