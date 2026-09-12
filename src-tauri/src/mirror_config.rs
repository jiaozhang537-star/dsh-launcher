//! 镜像源设置存储（FR-18）：版本下载通道与社区资源通道的激活源选择、自定义前缀模板。
//!
//! 持久化于 `<managed root>/mirrors.json`。两类通道各自独立选择：
//! - 版本下载通道（`active`）：`official`（官方源：npm 优先走 npmmirror、GitHub 默认直连）
//!   与 `custom`（仅对 GitHub 类 URL 应用用户前缀模板）。npmmirror 不再是可选择的
//!   激活源——它内化为官方源模式下 npm/Node 类资源的优先内部通道（FR-18 修订，
//!   需求方 2026-09-01 拍板），读入遗留 `npmmirror` 时静默迁移为 `official`。
//! - 社区资源通道（`community_active`）：`official`（官方 GitHub 聚合）/ `gitee` /
//!   `dshhub`（dsh-hub.cc）。Phase 3 插件市场 / Skill 中心按此取数；在此之前
//!   不影响任何下载。
//!
//! 校验在保存时执行：未知 id 或非法前缀 fail loud，绝不落盘半配置。

use serde::{Deserialize, Serialize};

use crate::paths;

/// 官方源 id（版本通道：npm 优先 + GitHub 默认；社区通道：官方 GitHub 聚合）。
pub const OFFICIAL_ID: &str = "official";
/// 内置 npmmirror 加速镜像 id（npm registry + Node 发行包双端点，Phase 1 实测可达）。
/// 仅作为官方源模式下 npm/Node 类资源的内部优先通道，不再是可选择激活源。
pub const NPMMIRROR_ID: &str = "npmmirror";
/// 自定义前缀模板源 id（作用范围 = GitHub 类 URL）。
pub const CUSTOM_ID: &str = "custom";
/// 社区资源镜像：Gitee（Phase 3 按「GitHub→Gitee 仓库映射表」接入）。
pub const GITEE_ID: &str = "gitee";
/// 社区资源镜像：dsh-hub.cc 社区插件目录（Phase 3 作为聚合数据源备选接入）。
pub const DSHHUB_ID: &str = "dshhub";

/// 版本下载通道的合法激活 id 集合。
const VERSION_SOURCE_IDS: &[&str] = &[OFFICIAL_ID, CUSTOM_ID];
/// 社区资源通道的合法激活 id 集合。
const COMMUNITY_SOURCE_IDS: &[&str] = &[OFFICIAL_ID, GITEE_ID, DSHHUB_ID];

/// 镜像源设置（持久化于 `<managed root>/mirrors.json`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorSettings {
    /// 版本下载通道当前激活的源 id。
    #[serde(default)]
    pub active: String,
    /// 自定义前缀模板；仅 `active == "custom"` 时参与改写。
    #[serde(default)]
    pub custom_prefix: String,
    /// 社区资源（插件 / Skill）通道当前激活的源 id；Phase 3 起被消费。
    #[serde(default = "default_community")]
    pub community_active: String,
}

fn default_community() -> String {
    OFFICIAL_ID.to_string()
}

impl Default for MirrorSettings {
    /// 默认 = 官方源 + 官方社区源；serde 缺字段时同样回落到此值。
    fn default() -> Self {
        Self::official()
    }
}

impl MirrorSettings {
    /// 官方源设置。
    pub fn official() -> Self {
        Self {
            active: OFFICIAL_ID.to_string(),
            custom_prefix: String::new(),
            community_active: OFFICIAL_ID.to_string(),
        }
    }
}

/// 前缀是否为合法模板值：http(s) 绝对地址、无空白与 `?`/`#`、无查询串；
/// 允许多级路径但去除尾部 `/`，保证 `{prefix}/https://github.com/...` 拼接结果唯一。
pub fn valid_prefix(prefix: &str) -> bool {
    let Some(rest) = prefix.strip_prefix("https://").or_else(|| prefix.strip_prefix("http://")) else {
        return false;
    };
    let host_part = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host_part.is_empty() || rest.starts_with('/') {
        return false;
    }
    !prefix.contains(['?', '#', ' ', '\t', '\r', '\n'])
}

/// 归一化前缀：去尾部斜杠。
pub fn normalize_prefix(prefix: &str) -> String {
    prefix.trim_end_matches('/').to_string()
}

/// 校验设置整体：两个激活 id 必须各自已知；custom 时前缀必须合法。
pub fn validate(settings: &MirrorSettings) -> Result<(), String> {
    if !VERSION_SOURCE_IDS.contains(&settings.active.as_str()) {
        return Err(format!("未知的镜像源 id：{:?}（合法：official / custom）", settings.active));
    }
    if !COMMUNITY_SOURCE_IDS.contains(&settings.community_active.as_str()) {
        return Err(format!(
            "未知的社区资源镜像 id：{:?}（合法：official / gitee / dshhub）",
            settings.community_active
        ));
    }
    if settings.active == CUSTOM_ID && !valid_prefix(&settings.custom_prefix) {
        return Err("自定义镜像前缀必须是 http(s) 开头的完整地址（如 https://example.com），且不含 ? 或 #".into());
    }
    Ok(())
}

/// 迁移历史遗留配置：npmmirror 曾是可选择激活源，内化后该值回落 official
/// （npm 优先本就是官方源模式的内置行为，用户无感知损失）。
fn migrate_legacy(raw: &mut MirrorSettings) {
    if raw.active == NPMMIRROR_ID {
        raw.active = OFFICIAL_ID.to_string();
    }
}

fn mirrors_file() -> Result<std::path::PathBuf, String> {
    Ok(paths::managed_root()?.join("mirrors.json"))
}

/// 读镜像源设置；文件不存在视为官方源默认，遗留 npmmirror 激活值静默迁移。
pub fn load() -> Result<MirrorSettings, String> {
    let file = mirrors_file()?;
    if !file.exists() {
        return Ok(MirrorSettings::official());
    }
    let raw = crate::paths::read_text(&file)?;
    let mut settings: MirrorSettings =
        serde_json::from_str(&raw).map_err(|e| format!("解析 {} 失败：{e}", file.display()))?;
    migrate_legacy(&mut settings);
    Ok(settings)
}

/// 保存镜像源设置（先校验，再原子落盘）。
pub fn save(settings: &MirrorSettings) -> Result<(), String> {
    validate(settings)?;
    let mut normalized = settings.clone();
    if normalized.active == CUSTOM_ID {
        normalized.custom_prefix = normalize_prefix(&normalized.custom_prefix);
    }
    let file = mirrors_file()?;
    paths::ensure_managed_dirs()?;
    let body = serde_json::to_string_pretty(&normalized)
        .map_err(|e| format!("序列化镜像设置失败：{e}"))?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, &body).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("替换 {} 失败：{e}", file.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances;

    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        instances::tests::sandbox(&format!("mirror-{tag}"))
    }

    #[test]
    fn default_is_official_and_roundtrips() {
        assert_eq!(MirrorSettings::default().active, OFFICIAL_ID);
        assert_eq!(MirrorSettings::default().community_active, OFFICIAL_ID);
        assert_eq!(MirrorSettings::official().active, OFFICIAL_ID);

        let (_lock, tmp) = sandbox("roundtrip");
        let loaded = load().unwrap();
        assert_eq!(loaded.active, OFFICIAL_ID);
        assert_eq!(loaded.community_active, OFFICIAL_ID);

        save(&MirrorSettings {
            active: CUSTOM_ID.into(),
            custom_prefix: "https://gh.example.top/".into(),
            community_active: GITEE_ID.into(),
        })
        .unwrap();
        let loaded = load().unwrap();
        assert_eq!(loaded.active, CUSTOM_ID);
        assert_eq!(loaded.custom_prefix, "https://gh.example.top", "尾部斜杠被归一化");
        assert_eq!(loaded.community_active, GITEE_ID);

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 遗留 npmmirror 激活值在读取时静默迁移为 official，且缺省的
    /// community_active 字段回落 official（旧版 mirrors.json 无该字段）。
    #[test]
    fn legacy_npmmirror_active_migrates_to_official() {
        let (_lock, tmp) = sandbox("legacy");
        let root = tmp.join("dsh-launcher");
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("mirrors.json");
        std::fs::write(
            &file,
            r#"{ "active": "npmmirror", "custom_prefix": "" }"#,
        )
        .unwrap();
        let loaded = load().unwrap();
        assert_eq!(loaded.active, OFFICIAL_ID, "遗留 npmmirror 迁移为 official");
        assert_eq!(loaded.community_active, OFFICIAL_ID, "缺省 community 回落 official");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn validation_rejects_unknown_id_and_bad_prefix() {
        // npmmirror 不再是合法激活源（已内化）。
        assert!(validate(&MirrorSettings {
            active: NPMMIRROR_ID.into(),
            custom_prefix: String::new(),
            community_active: OFFICIAL_ID.into(),
        })
        .is_err());
        assert!(validate(&MirrorSettings {
            active: "nope".into(),
            custom_prefix: String::new(),
            community_active: OFFICIAL_ID.into(),
        })
        .is_err());
        assert!(validate(&MirrorSettings {
            active: OFFICIAL_ID.into(),
            custom_prefix: String::new(),
            community_active: "nope".into(),
        })
        .is_err());

        for bad in [
            "",
            "ftp://x.com",
            "https://",
            "https://a.com/?x=1",
            "https://a.com/#frag",
            "has space/a",
            "/relative",
        ] {
            let s = MirrorSettings {
                active: CUSTOM_ID.into(),
                custom_prefix: bad.into(),
                community_active: OFFICIAL_ID.into(),
            };
            assert!(validate(&s).is_err(), "前缀 {bad:?} 应被拒绝");
        }

        for good in [
            "https://ghfast.top",
            "http://127.0.0.1:8443",
            "https://mirror.example.com/prefix",
        ] {
            let s = MirrorSettings {
                active: CUSTOM_ID.into(),
                custom_prefix: good.into(),
                community_active: OFFICIAL_ID.into(),
            };
            assert!(validate(&s).is_ok(), "前缀 {good:?} 应通过");
        }

        // 非 custom 激活时前缀不参与校验（保留用户输入但不生效）。
        assert!(validate(&MirrorSettings {
            active: OFFICIAL_ID.into(),
            custom_prefix: String::new(),
            community_active: DSHHUB_ID.into(),
        })
        .is_ok());
    }

    #[test]
    fn normalize_strips_trailing_slashes_only() {
        assert_eq!(normalize_prefix("https://a.com/"), "https://a.com");
        assert_eq!(normalize_prefix("https://a.com"), "https://a.com");
        assert_eq!(normalize_prefix("https://a.com/x//"), "https://a.com/x");
    }
}
