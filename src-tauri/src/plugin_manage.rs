//! 插件管理页后端（FR-03 配套，激活 P2 表面批次 UI）：已装插件三分组、
//! 删除、检查更新。分组口径（R-2/T-6 确立 + R-3 实证）：
//! - **dsh 内置**：在 `dsh.profile.bundles` 层但不在 `dependencies`（模板
//!   bundle，如 dsh-base / dsh-headless）——不可删；
//! - **第三方**：普通 registry 依赖声明；
//! - **自建**：`file:` / `link:` 本地依赖。
//! 删除走官方 `dsh plugin remove` 链（market_install），删除后 precheck
//! 可复核（清单与 node_modules 同步消失）。「检查更新」只对第三方 registry
//! 依赖，经网关对比 npm `dist-tags.latest`（与市场数据层共用 packument
//! 通道回落语义）。

use serde::Serialize;

use crate::instances;
use crate::market_install;
use crate::registry_meta;

/// 插件分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginGroup {
    /// dsh 内置层（在 bundles 不在 dependencies）——不可删。
    Builtin,
    /// 普通 registry 依赖。
    Thirdparty,
    /// `file:` / `link:` 本地依赖。
    Selfmade,
}

/// 已装插件一行。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRow {
    pub name: String,
    /// 已安装版本（解析不到为空串）。
    pub version: String,
    /// 清单里的依赖声明原文（内置组为空串）。
    pub spec: String,
    pub group: PluginGroup,
    /// 是否在 `dsh.profile.bundles` 层。
    pub in_bundles: bool,
    /// 已装 manifest 是否声明 `dsh.bundle.patch`——false = 纯依赖包（不是
    /// 可加载的插件层，无「启用」概念；前端渲染禁用灰框，走查实证修复）。
    pub declares_patch: bool,
    pub removable: bool,
}

/// 已装插件清单视图。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginList {
    #[serde(rename = "instanceId")]
    pub instance_id: String,
    pub profile: String,
    pub plugins: Vec<PluginRow>,
}

/// 一个第三方插件的更新对比。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdate {
    pub name: String,
    pub current: String,
    pub latest: String,
    pub updatable: bool,
}

/// 分组是纯函数：输入（依赖序对、bundles、版本与插件层声明的解析器）→
/// 输出行列表。解析器注入以便测试（真实实现查 node_modules 三级根）。
pub fn group_plugins(
    deps: &[(String, String)],
    bundles: &[String],
    version_of: &dyn Fn(&str) -> String,
    patch_of: &dyn Fn(&str) -> bool,
) -> Vec<PluginRow> {
    let mut rows = Vec::new();
    for (name, spec) in deps {
        let group = if spec.starts_with("file:") || spec.starts_with("link:") {
            PluginGroup::Selfmade
        } else {
            PluginGroup::Thirdparty
        };
        rows.push(PluginRow {
            name: name.clone(),
            version: version_of(name),
            spec: spec.clone(),
            group,
            in_bundles: bundles.iter().any(|b| b == name),
            declares_patch: patch_of(name),
            removable: true,
        });
    }
    for name in bundles {
        if !deps.iter().any(|(d, _)| d == name) {
            rows.push(PluginRow {
                name: name.clone(),
                version: version_of(name),
                spec: String::new(),
                group: PluginGroup::Builtin,
                in_bundles: true,
                declares_patch: true,
                removable: false,
            });
        }
    }
    rows
}

/// 枚举实例已装插件（三分组）。实例不存在 fail loud。
pub fn list(instance_id: &str) -> Result<PluginList, String> {
    let instance = instances::get_instance(instance_id)?;
    let profile_dir = instances::instance_dir(instance_id)?
        .join("profiles")
        .join(&instance.profile);
    let manifest = profile_dir.join("package.json");
    let (deps, bundles) = if manifest.is_file() {
        let text =
            std::fs::read_to_string(&manifest).map_err(|e| format!("读清单失败：{e}"))?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("清单不是合法 JSON：{e}"))?;
        let deps: Vec<(String, String)> = value
            .get("dependencies")
            .and_then(|v| v.as_object())
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| {
                        let spec = v.as_str()?.to_string();
                        Some((k.clone(), spec))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let bundles: Vec<String> = value
            .pointer("/dsh/profile/bundles")
            .and_then(|v| v.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|b| b.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        (deps, bundles)
    } else {
        (Vec::new(), Vec::new())
    };
    let version_of = |name: &str| {
        market_install::resolve_installed_version(instance_id, &instance.profile, name)
            .unwrap_or_default()
    };
    let patch_of = |name: &str| {
        market_install::resolve_installed_manifest(instance_id, &instance.profile, name)
            .ok()
            .flatten()
            .map(|m| m.pointer("/dsh/bundle/patch").is_some())
            .unwrap_or(false)
    };
    Ok(PluginList {
        instance_id: instance_id.to_string(),
        profile: instance.profile.clone(),
        plugins: group_plugins(&deps, &bundles, &version_of, &patch_of),
    })
}

/// 删除插件（内置组拒绝；其余走官方 remove 链）。阻塞调用。
pub fn remove(instance_id: &str, package: &str, progress: &mut dyn FnMut(String)) -> Result<(), String> {
    let list = list(instance_id)?;
    let Some(row) = list.plugins.iter().find(|p| p.name == package) else {
        return Err(format!("该 profile 未安装 {package}"));
    };
    if row.group == PluginGroup::Builtin {
        return Err(format!("{package} 是 dsh 内置层，不可删除"));
    }
    market_install::plugin_remove(instance_id, package, progress)
}

/// 切换插件启用状态（需求方走查定稿，T-2 提前落位）：enabled=false = 从
/// `dsh.profile.bundles` 移出（**依赖保留**）；true = 加回（要求是
/// dependencies 成员且已装 manifest 声明 `dsh.bundle.patch`）。直接读写
/// DSH 原生 profile 清单（与官方 CLI 同一写入面），下次启动实例生效。
/// 已知边界（前端如实告知）：官方 `dsh plugin add/remove` 的 reconcile 会把
/// 停用项复活（依赖仍在即重加层）。内置层恒启用，不可切换。
pub fn set_enabled(instance_id: &str, package: &str, enabled: bool) -> Result<(), String> {
    let instance = instances::get_instance(instance_id)?;
    let manifest_path = instances::instance_dir(instance_id)?
        .join("profiles")
        .join(&instance.profile)
        .join("package.json");
    if !manifest_path.is_file() {
        return Err("该实例的 profile 尚未初始化（先启动一次实例）".into());
    }
    let text = std::fs::read_to_string(&manifest_path).map_err(|e| format!("读清单失败：{e}"))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("清单不是合法 JSON：{e}"))?;
    let is_dependency = value
        .get("dependencies")
        .and_then(|v| v.as_object())
        .map(|m| m.contains_key(package))
        .unwrap_or(false);
    if !is_dependency {
        return Err(format!("{package} 不是依赖声明（内置层随本体分发，恒启用不可切换）"));
    }
    let bundles = value
        .pointer_mut("/dsh/profile/bundles")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| "清单缺少 dsh.profile.bundles".to_string())?;
    let present = bundles.iter().any(|b| b.as_str() == Some(package));
    if enabled {
        if present {
            return Ok(()); // 幂等
        }
        let declares_patch = market_install::resolve_installed_manifest(instance_id, &instance.profile, package)?
            .map(|m| m.pointer("/dsh/bundle/patch").is_some())
            .unwrap_or(false);
        if !declares_patch {
            return Err(format!("{package} 未声明 dsh.bundle.patch——它是纯依赖包（如 UI 库），不作为插件层加载，无需启用"));
        }
        bundles.push(serde_json::Value::String(package.to_string()));
    } else {
        if !present {
            return Ok(()); // 幂等
        }
        bundles.retain(|b| b.as_str() != Some(package));
    }
    let out = serde_json::to_string_pretty(&value).map_err(|e| format!("序列化失败：{e}"))? + "\n";
    std::fs::write(&manifest_path, out).map_err(|e| format!("写清单失败：{e}"))
}

/// 检查第三方插件更新（显式触发；阻塞网络调用）。file:/link: 与内置组跳过；
/// 单包 packument 拉取失败不中断整体（该包 latest 记为空串、不可更新）。
pub fn check_updates(instance_id: &str) -> Result<Vec<PluginUpdate>, String> {
    let list = list(instance_id)?;
    let mut out = Vec::new();
    for row in list.plugins.iter().filter(|p| p.group == PluginGroup::Thirdparty) {
        let latest = registry_meta::fetch_packument(&row.name)
            .ok()
            .and_then(|packument| {
                packument
                    .get("dist-tags")
                    .and_then(|t| t.get("latest"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let updatable = !latest.is_empty() && latest != row.version;
        out.push(PluginUpdate {
            name: row.name.clone(),
            current: row.version.clone(),
            latest,
            updatable,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        crate::instances::tests::sandbox(&format!("plugin-manage-{tag}"))
    }

    fn v(name: &str) -> String {
        match name {
            "@deepseek-ai/dsh-base" => "0.1.1-rc.2".into(),
            _ => "1.0.0".into(),
        }
    }

    #[test]
    fn three_groups_with_builtin_not_removable() {
        let deps = vec![
            ("dsh-cost-meter".to_string(), "^1.7.5".to_string()),
            ("local-panel".to_string(), "file:D:/x/plugin".to_string()),
        ];
        let bundles = vec![
            "@deepseek-ai/dsh-base".to_string(),
            "@deepseek-ai/dsh-headless".to_string(),
            "dsh-cost-meter".to_string(),
        ];
        let rows = group_plugins(&deps, &bundles, &v, &|_| true);
        let builtin: Vec<&PluginRow> = rows.iter().filter(|r| r.group == PluginGroup::Builtin).collect();
        assert_eq!(builtin.len(), 2, "bundles 里不在 dependencies 的两个内置层");
        assert!(builtin.iter().all(|r| !r.removable));
        let cost = rows.iter().find(|r| r.name == "dsh-cost-meter").unwrap();
        assert_eq!(cost.group, PluginGroup::Thirdparty);
        assert!(cost.in_bundles);
        assert!(cost.removable);
        let local = rows.iter().find(|r| r.name == "local-panel").unwrap();
        assert_eq!(local.group, PluginGroup::Selfmade);
        assert!(!local.in_bundles);
    }

    #[test]
    fn missing_dependencies_key_yields_builtin_only() {
        // R-3 实证：pnpm remove 后 dependencies 键整个消失。
        let rows = group_plugins(&[], &["@deepseek-ai/dsh-base".to_string()], &v, &|_| true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].group, PluginGroup::Builtin);
        assert_eq!(rows[0].version, "0.1.1-rc.2");
    }

    #[test]
    fn empty_profile_yields_empty_list() {
        let rows = group_plugins(&[], &[], &v, &|_| true);
        assert!(rows.is_empty());
    }

    /// Q4 的后端证据：清单是唯一枚举来源，且**每次 list 都重读**——DSH 在实例内
    /// 执行 `dsh plugin add`（AI 在网页会话里代跑）写进 profile 清单后，下一次
    /// list 就能看到它，不需要重启实例，也不需要启动器侧缓存失效。
    #[test]
    fn list_reflects_profile_manifest_changes_without_restart() {
        let (_lock, tmp) = sandbox("list-refresh");
        let instance = crate::instances::create_instance("q4web", "web").unwrap();
        let profile_dir = crate::instances::instance_dir(&instance.id)
            .unwrap()
            .join("profiles")
            .join("web");
        let manifest = profile_dir.join("package.json");

        std::fs::create_dir_all(profile_dir.join("node_modules")).unwrap();
        std::fs::write(
            &manifest,
            r#"{ "dependencies": { "dsh-cost-meter": "^1.0.0" },
                 "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "dsh-cost-meter"] } } }"#,
        )
        .unwrap();
        let before = list(&instance.id).unwrap();
        assert!(before.plugins.iter().any(|p| p.name == "dsh-cost-meter"));
        assert!(!before.plugins.iter().any(|p| p.name == "dsh-later"));

        // 模拟 DSH 侧 add：清单多一条依赖 + bundles 多一层。没有重启、没有缓存失效。
        std::fs::write(
            &manifest,
            r#"{ "dependencies": { "dsh-cost-meter": "^1.0.0", "dsh-later": "^2.0.0" },
                 "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "dsh-cost-meter", "dsh-later"] } } }"#,
        )
        .unwrap();
        let after = list(&instance.id).unwrap();
        let later = after
            .plugins
            .iter()
            .find(|p| p.name == "dsh-later")
            .expect("清单变更后应立即出现在列表里");
        assert!(later.in_bundles, "bundles 里有它 → 勾选框应为已启用");
        assert!(later.removable, "非内置组可删");

        // 边界：手放进 node_modules、但清单没记录的包不在列表里（枚举来源 = 清单）。
        std::fs::create_dir_all(profile_dir.join("node_modules").join("dropped-in")).unwrap();
        let after2 = list(&instance.id).unwrap();
        assert!(!after2.plugins.iter().any(|p| p.name == "dropped-in"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 启停切换（走查定稿）：bundles 成员切换、依赖保留、幂等、纯依赖拒绝、
    /// 内置层拒绝。
    #[test]
    fn set_enabled_toggles_bundles_membership_with_guards() {
        let (_lock, tmp) = sandbox("set-enabled");
        let instance = crate::instances::create_instance("pweb", "web").unwrap();
        let profile_dir = crate::instances::instance_dir(&instance.id)
            .unwrap()
            .join("profiles")
            .join("web");
        std::fs::create_dir_all(profile_dir.join("node_modules").join("dsh-cost-meter")).unwrap();
        std::fs::create_dir_all(profile_dir.join("node_modules").join("plain-lib")).unwrap();
        std::fs::write(
            profile_dir.join("package.json"),
            r#"{
  "dependencies": { "dsh-cost-meter": "^1.0.0", "plain-lib": "^1.0.0" },
  "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] } }
}"#,
        )
        .unwrap();
        std::fs::write(
            profile_dir
                .join("node_modules")
                .join("dsh-cost-meter")
                .join("package.json"),
            r#"{ "name": "dsh-cost-meter", "version": "1.0.0", "dsh": { "bundle": { "patch": "./cordis.patch.yml" } } }"#,
        )
        .unwrap();
        std::fs::write(
            profile_dir
                .join("node_modules")
                .join("plain-lib")
                .join("package.json"),
            r#"{ "name": "plain-lib", "version": "1.0.0" }"#,
        )
        .unwrap();

        // 启用：bundles 追加（清单保持合法、依赖不动）。
        set_enabled(&instance.id, "dsh-cost-meter", true).unwrap();
        let row = list(&instance.id)
            .unwrap()
            .plugins
            .into_iter()
            .find(|p| p.name == "dsh-cost-meter")
            .unwrap();
        assert!(row.in_bundles);
        assert_eq!(row.group, PluginGroup::Thirdparty);

        // 幂等：重复启用不报错、不重复追加。
        set_enabled(&instance.id, "dsh-cost-meter", true).unwrap();
        let bundles_count = |id: &str| {
            list(id)
                .unwrap()
                .plugins
                .iter()
                .filter(|p| p.name == "dsh-cost-meter")
                .count()
        };
        assert_eq!(bundles_count(&instance.id), 1);

        // 停用：bundles 移出、依赖保留（仍是 thirdparty 组）。
        set_enabled(&instance.id, "dsh-cost-meter", false).unwrap();
        let row = list(&instance.id)
            .unwrap()
            .plugins
            .into_iter()
            .find(|p| p.name == "dsh-cost-meter")
            .unwrap();
        assert!(!row.in_bundles);
        assert_eq!(row.group, PluginGroup::Thirdparty);
        set_enabled(&instance.id, "dsh-cost-meter", false).unwrap(); // 幂等停用

        // 纯依赖行如实标注 declares_patch=false（走查：UI 据此渲染禁用灰框，
        // 不再提供点了必报错的假勾选框）。
        let plain_row = list(&instance.id)
            .unwrap()
            .plugins
            .into_iter()
            .find(|p| p.name == "plain-lib")
            .unwrap();
        assert!(!plain_row.declares_patch);
        let bundled_row = list(&instance.id)
            .unwrap()
            .plugins
            .into_iter()
            .find(|p| p.name == "dsh-cost-meter")
            .unwrap();
        assert!(bundled_row.declares_patch);

        // 纯依赖包（未声明 patch）启用被拒。
        let err = set_enabled(&instance.id, "plain-lib", true).unwrap_err();
        assert!(err.contains("未声明"), "实际：{err}");

        // 内置层（模板 bundle，非依赖）拒绝切换。
        let err = set_enabled(&instance.id, "@deepseek-ai/dsh-base", false).unwrap_err();
        assert!(err.contains("恒启用"), "实际：{err}");

        std::fs::remove_dir_all(&tmp).ok();
    }
}
