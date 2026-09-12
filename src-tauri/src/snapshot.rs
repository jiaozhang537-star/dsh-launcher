//! 实例配置快照与启动时对账。
//!
//! 启动契约：每次点「启动」都是一次独立的完整启动（实例在跑就先停掉再拉起），
//! 本次启动把**自上次启动以来实例配置的变更**一次性对账出来——插件清单、
//! Skill 目录、以及实例 `settings.yaml` 里由用户自己（在 DSH 网页侧）添加的
//! 外部模型提供商。对账结果进加载清单，让"网页里装的东西"在启动器里可见。
//!
//! 三个集合都只比**名称有无**，不比内容：内容差异会在本次启动时由启动链重新
//! 写入并生效（`llm_bridge::sync_instance` 负责写实例配置）。对账是诊断，
//! 不是启动前置——采集或落盘失败只记在结论里，绝不阻断启动。

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 上次启动时记下的实例实况。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceSnapshot {
    #[serde(default)]
    pub plugins: BTreeSet<String>,
    #[serde(default)]
    pub skills: BTreeSet<String>,
    #[serde(default)]
    pub external_routes: BTreeSet<String>,
}

/// 对账结果（纯数据，供加载清单逐条呈现）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reconcile {
    /// false = 首次启动（没有基线）：只报"当前构成"，不报"新增/移除"。
    pub baseline: bool,
    pub plugins_total: usize,
    pub skills_total: usize,
    pub external_routes_total: usize,
    pub plugins_added: Vec<String>,
    pub plugins_removed: Vec<String>,
    pub skills_added: Vec<String>,
    pub skills_removed: Vec<String>,
    pub external_routes_added: Vec<String>,
    pub external_routes_removed: Vec<String>,
    /// 对账自身的问题（采集失败 / 快照落盘失败）——如实呈现，不阻断启动。
    pub note: Option<String>,
}

/// 比对基线（`None` = 首次）与当前实况。纯函数：不碰文件系统。
pub fn diff(prev: Option<&InstanceSnapshot>, cur: &InstanceSnapshot) -> Reconcile {
    let (plugins_added, plugins_removed) = set_diff(prev.map(|p| &p.plugins), &cur.plugins);
    let (skills_added, skills_removed) = set_diff(prev.map(|p| &p.skills), &cur.skills);
    let (external_routes_added, external_routes_removed) =
        set_diff(prev.map(|p| &p.external_routes), &cur.external_routes);
    Reconcile {
        baseline: prev.is_some(),
        plugins_total: cur.plugins.len(),
        skills_total: cur.skills.len(),
        external_routes_total: cur.external_routes.len(),
        plugins_added,
        plugins_removed,
        skills_added,
        skills_removed,
        external_routes_added,
        external_routes_removed,
        note: None,
    }
}

fn set_diff(prev: Option<&BTreeSet<String>>, cur: &BTreeSet<String>) -> (Vec<String>, Vec<String>) {
    let Some(prev) = prev else {
        return (Vec::new(), Vec::new());
    };
    (
        cur.difference(prev).cloned().collect(),
        prev.difference(cur).cloned().collect(),
    )
}

/// 采集实例当前实况。任一步失败即返回 Err（调用方转成 `Reconcile::note`）。
pub fn collect(instance_id: &str) -> Result<InstanceSnapshot, String> {
    let instance_dir = crate::instances::instance_dir(instance_id)?;
    let plugins = crate::plugin_manage::list(instance_id)?
        .plugins
        .into_iter()
        .map(|row| row.name)
        .collect();
    let skills = crate::skills::discover_skill_dirs(&instance_dir.join(crate::skills::SKILLS_DIR_NAME))
        .into_iter()
        .map(|(_, name)| name)
        .collect();
    Ok(InstanceSnapshot {
        plugins,
        skills,
        external_routes: crate::llm_bridge::external_route_keys(instance_id)?,
    })
}

fn snapshot_file(instance_id: &str) -> Result<PathBuf, String> {
    let dir = crate::paths::managed_root()?.join("instance-snapshot");
    Ok(dir.join(format!("{instance_id}.json")))
}

/// 读上次基线。文件缺失或损坏都按"首次"处理（返回 None）：对账不该因为一个
/// 诊断用的历史文件而失败，也不要静默当成"什么都没变"。
pub fn load(instance_id: &str) -> Option<InstanceSnapshot> {
    let path = snapshot_file(instance_id).ok()?;
    if !path.is_file() {
        return None;
    }
    let text = crate::paths::read_text(&path).ok()?;
    serde_json::from_str(&text).ok()
}

/// 覆盖写入基线（临时文件 + 改名，避免半成品被下次启动读成基线）。
pub fn save(instance_id: &str, snapshot: &InstanceSnapshot) -> Result<(), String> {
    let path = snapshot_file(instance_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建 {} 失败：{e}", parent.display()))?;
    }
    let body =
        serde_json::to_string_pretty(snapshot).map_err(|e| format!("序列化实例快照失败：{e}"))?;
    let tmp = path.with_extension("json-writing");
    std::fs::write(&tmp, body).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 {} 失败：{e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(plugins: &[&str], skills: &[&str], routes: &[&str]) -> InstanceSnapshot {
        InstanceSnapshot {
            plugins: plugins.iter().map(|s| s.to_string()).collect(),
            skills: skills.iter().map(|s| s.to_string()).collect(),
            external_routes: routes.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn first_run_reports_composition_without_additions() {
        let cur = snap(&["a", "b"], &["s1"], &["x"]);
        let out = diff(None, &cur);
        assert!(!out.baseline, "没有基线必须标成首次");
        assert_eq!((out.plugins_total, out.skills_total, out.external_routes_total), (2, 1, 1));
        assert!(out.plugins_added.is_empty() && out.plugins_removed.is_empty());
        assert!(out.skills_added.is_empty() && out.external_routes_added.is_empty());
    }

    #[test]
    fn diff_reports_added_and_removed_per_category() {
        let prev = snap(&["a", "b"], &["s1", "s2"], &["x"]);
        let cur = snap(&["b", "c"], &["s2", "s3"], &["x", "y"]);
        let out = diff(Some(&prev), &cur);
        assert!(out.baseline);
        assert_eq!(out.plugins_added, vec!["c"]);
        assert_eq!(out.plugins_removed, vec!["a"]);
        assert_eq!(out.skills_added, vec!["s3"]);
        assert_eq!(out.skills_removed, vec!["s1"]);
        assert_eq!(out.external_routes_added, vec!["y"]);
        assert!(out.external_routes_removed.is_empty());
    }

    #[test]
    fn identical_snapshots_produce_empty_diff() {
        let one = snap(&["a"], &["s"], &["x"]);
        let out = diff(Some(&one), &one);
        assert_eq!(out, Reconcile { baseline: true, plugins_total: 1, skills_total: 1, external_routes_total: 1, note: None, ..Default::default() });
    }

    #[test]
    fn snapshot_round_trips_and_missing_baseline_is_none() {
        let (_guard, _root) = crate::instances::tests::sandbox("snapshot");
        assert!(load("i-none").is_none(), "没有基线文件必须按首次处理");
        let one = snap(&["a"], &["s"], &["x"]);
        save("i-one", &one).unwrap();
        assert_eq!(load("i-one").as_ref(), Some(&one));
        // 损坏文件按首次处理，不 panic、不返回空快照冒充基线。
        let path = snapshot_file("i-one").unwrap();
        std::fs::write(&path, b"\xff\xfe not utf8 \x00").unwrap();
        assert!(load("i-one").is_none());
    }
}
