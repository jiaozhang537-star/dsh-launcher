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
use std::path::Path;

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
    /// 组合包子插件（2026-09-12 追加批次）：仅组合包口径命中的行携带，
    /// None = 非组合包。识别口径见 `modpack_children_of`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modpack_children: Option<Vec<ModpackChild>>,
    pub removable: bool,
}

/// 组合包的子插件一行 = 父包 cordis.patch.yml 的一条 insert 行。
/// 启停键是 `row_id`（用户层覆盖按 id 定位），不是包名——DSH 对组合包子
/// 插件的挂载通道是父包 insert 行，bundles 摘名对它无效（侦察实证）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackChild {
    /// 子插件包名（insert 行 `name`）。
    pub name: String,
    /// insert 行 `id`——用户层 `{id, disabled}` 覆盖的定位键。
    pub row_id: String,
    /// 子插件已装版本（解析不到为空串）。
    pub version: String,
    /// 当前停用 = insert 行自带 disabled || 用户层已有覆盖行。
    pub disabled: bool,
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
        // file:/link: → 自建（本地目录开发）；但以 .tgz 结尾的是整合包离线
        // 导入产物（tarball 安装），归第三方（2026-09-12 走查反馈：导入的
        // 组合包插件不应出现在自建组）。
        let group = if (spec.starts_with("file:") || spec.starts_with("link:"))
            && !spec.ends_with(".tgz")
        {
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
            modpack_children: None,
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
                modpack_children: None,
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
    let mut rows = group_plugins(&deps, &bundles, &version_of, &patch_of);

    // 组合包识别（2026-09-12 追加批次）：对声明 patch 的**第三方/自建**行解析
    // 包内 patch 的 insert 行；识别失败的行按「非组合包」降级，不影响清单整体。
    // 内置组（dsh-base/dsh-web-app 等本体底座层）绝不识别——它们的 patch 插着
    // 全部官方子插件（dsh-base 78 行），误判会凭空冒出几十个"子插件"（真机
    // 事故：84 个插件卡，2026-09-12）。
    let user_disabled = user_disabled_ids(&profile_dir);
    for row in rows.iter_mut() {
        if !row.declares_patch || row.group == PluginGroup::Builtin {
            continue;
        }
        row.modpack_children = modpack_children_of(
            instance_id,
            &instance.profile,
            &row.name,
            &user_disabled,
            &version_of,
            &patch_of,
        )
        .unwrap_or(None);
    }

    Ok(PluginList {
        instance_id: instance_id.to_string(),
        profile: instance.profile.clone(),
        plugins: rows,
    })
}

/// 用户层 patch 文件名（DSH 约定：profile 根的 `cordis.patch.yml`，所有
/// bundle 层之后应用、可覆盖前层插入的行——组合包子插件启停的落点）。
const USER_PATCH_FILENAME: &str = "cordis.patch.yml";

/// patch 文档的顶层条目序列：序列文档取自身；`insert:` 顶层键的映射文档取
/// 该键下的数组（dsh-base 为前者、部分第三方包为后者，两种都支持）。
fn patch_top_entries(doc: &serde_yaml::Value) -> Vec<&serde_yaml::Value> {
    match doc {
        serde_yaml::Value::Sequence(items) => items.iter().collect(),
        serde_yaml::Value::Mapping(map) => match map.get(serde_yaml::Value::from("insert")) {
            Some(serde_yaml::Value::Sequence(items)) => items.iter().collect(),
            _ => vec![doc],
        },
        _ => Vec::new(),
    }
}

/// 抽取 patch 文档中的 insert 行（row_id, name, 自带 disabled）。两种形态：
/// 序列文档的条目内含 `insert:` 数组（dsh-base 型），或映射文档顶层 `insert:`
/// 直接是行数组（部分第三方包型）。
fn insert_rows(doc: &serde_yaml::Value) -> Vec<(String, String, bool)> {
    fn collect(row: &serde_yaml::Value, rows: &mut Vec<(String, String, bool)>) {
        let id = row.get("id").and_then(|v| v.as_str()).map(str::to_string);
        let name = row.get("name").and_then(|v| v.as_str()).map(str::to_string);
        if let (Some(id), Some(name)) = (id, name) {
            let disabled = row
                .get("disabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            rows.push((id, name, disabled));
        }
    }
    let mut rows = Vec::new();
    match doc {
        serde_yaml::Value::Sequence(items) => {
            for entry in items {
                if let Some(ins) = entry.get("insert").and_then(|v| v.as_sequence()) {
                    for row in ins {
                        collect(row, &mut rows);
                    }
                }
            }
        }
        serde_yaml::Value::Mapping(_) => {
            if let Some(ins) = doc.get("insert").and_then(|v| v.as_sequence()) {
                for row in ins {
                    collect(row, &mut rows);
                }
            }
        }
        _ => {}
    }
    rows
}

/// 读用户层 patch 中被停用的行 id 集（文件缺失/损坏 = 空集，不阻塞清单）。
fn user_disabled_ids(profile_dir: &Path) -> std::collections::BTreeSet<String> {
    // 与其余状态文件同口径走 paths::read_text（剥 BOM；编辑器/PowerShell 产物，
    // 见 paths.rs 的 2026-09-10 事故注）。libyaml 实测容忍流首 BOM，此为
    // 预防性统一而非故障修复；缺失/损坏仍按空集降级，不阻塞清单。
    let path = profile_dir.join(USER_PATCH_FILENAME);
    if !path.exists() {
        return Default::default();
    }
    let Ok(text) = crate::paths::read_text(&path) else {
        return Default::default();
    };
    let Ok(doc) = serde_yaml::from_str::<serde_yaml::Value>(&text) else {
        return Default::default();
    };
    let mut ids = std::collections::BTreeSet::new();
    for entry in patch_top_entries(&doc) {
        let disabled = entry.get("disabled").and_then(|v| v.as_bool());
        if disabled == Some(true) {
            if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    ids
}

/// 组合包识别与子清单（侦察定稿口径）：声明 `dsh.bundle.patch` 且满足其一——
/// ①insert 行去重（按行 id、排除指向自身）后 ≥2；②dependencies 中存在同样
/// 声明 patch 的包。子清单 = insert 行（挂载通道），空清单不算组合包。
#[allow(clippy::too_many_arguments)]
fn modpack_children_of(
    instance_id: &str,
    profile: &str,
    package: &str,
    user_disabled: &std::collections::BTreeSet<String>,
    version_of: &dyn Fn(&str) -> String,
    patch_of: &dyn Fn(&str) -> bool,
) -> Result<Option<Vec<ModpackChild>>, String> {
    let Some(dir) = market_install::resolve_installed_package_dir(instance_id, profile, package)?
    else {
        return Ok(None);
    };
    // 本体家族（@deepseek-ai/*）永不识别为组合包——与市场聚合口径一致
    // （market::is_modpack_card 同款排除），双保险挡住底座层误判。
    if package.starts_with("@deepseek-ai/") {
        return Ok(None);
    }
    let manifest_text = std::fs::read_to_string(dir.join("package.json"))
        .map_err(|e| format!("读 {} 失败：{e}", dir.join("package.json").display()))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .map_err(|e| format!("{} 不是合法 JSON：{e}", package))?;
    let Some(patch_rel) = manifest
        .pointer("/dsh/bundle/patch")
        .and_then(|v| v.as_str())
    else {
        return Ok(None);
    };
    let patch_text = std::fs::read_to_string(dir.join(patch_rel)).map_err(|e| {
        format!(
            "读组合包 {package} 的补丁文件失败（{}）：{e}",
            dir.join(patch_rel).display()
        )
    })?;
    let doc: serde_yaml::Value =
        serde_yaml::from_str(&patch_text).map_err(|e| format!("{package} 的补丁不是合法 YAML：{e}"))?;

    let mut kids: Vec<ModpackChild> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (row_id, name, disabled) in insert_rows(&doc) {
        if name == package || !seen.insert(row_id.clone()) {
            continue;
        }
        let version = version_of(&name);
        let is_disabled = disabled || user_disabled.contains(&row_id);
        kids.push(ModpackChild {
            name,
            row_id,
            version,
            disabled: is_disabled,
        });
    }
    if kids.is_empty() {
        return Ok(None);
    }
    let deps_declare = manifest
        .get("dependencies")
        .and_then(|d| d.as_object())
        .map(|map| map.keys().any(|k| patch_of(k)))
        .unwrap_or(false);
    if kids.len() >= 2 || deps_declare {
        Ok(Some(kids))
    } else {
        Ok(None)
    }
}

/// 组合包子插件启停（2026-09-12 追加批次）：写版本的用户层
/// `cordis.patch.yml`（DSH 热重载，运行中实例即时生效；未运行则下次启动生效）。
/// 停用 = 追加/置位 `{id, disabled: true}`；启用 = 移除该 id 的覆盖行
/// （恢复父包 patch 默认）。幂等；原子落盘。
pub fn set_child_enabled(instance_id: &str, row_id: &str, enabled: bool) -> Result<(), String> {
    let instance = instances::get_instance(instance_id)?;
    let rid = row_id.trim();
    if rid.is_empty()
        || !rid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@/_-.: ".contains(c))
    {
        return Err(format!("非法的子插件行 id：{row_id:?}"));
    }
    let profile_dir = instances::instance_dir(instance_id)?
        .join("profiles")
        .join(&instance.profile);
    std::fs::create_dir_all(&profile_dir)
        .map_err(|e| format!("创建 profile 目录失败：{e}"))?;
    let path = profile_dir.join(USER_PATCH_FILENAME);

    let mut doc: serde_yaml::Value = if path.is_file() {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("读 {} 失败：{e}", path.display()))?;
        serde_yaml::from_str(&text)
            .map_err(|e| format!("{} 不是合法 YAML，拒绝改写（请手工检查）：{e}", path.display()))?
    } else {
        serde_yaml::Value::Sequence(Vec::new())
    };
    if !matches!(doc, serde_yaml::Value::Sequence(_)) {
        return Err(format!(
            "{} 顶层不是列表，拒绝改写（用户手写的高级补丁请自行维护）",
            path.display()
        ));
    }

    if enabled {
        // 移除该 id 的覆盖行 = 恢复父包 patch 默认（原样保留用户其余条目）。
        if let serde_yaml::Value::Sequence(items) = &mut doc {
            items.retain(|entry| entry.get("id").and_then(|v| v.as_str()) != Some(rid));
        }
    } else {
        let seq = match &mut doc {
            serde_yaml::Value::Sequence(items) => items,
            _ => unreachable!(),
        };
        let mut found = false;
        for entry in seq.iter_mut() {
            if entry.get("id").and_then(|v| v.as_str()) == Some(rid) {
                if let serde_yaml::Value::Mapping(map) = entry {
                    map.insert(
                        serde_yaml::Value::from("disabled"),
                        serde_yaml::Value::from(true),
                    );
                    found = true;
                }
            }
        }
        if !found {
            let mut map = serde_yaml::Mapping::new();
            map.insert(
                serde_yaml::Value::from("id"),
                serde_yaml::Value::from(rid),
            );
            map.insert(
                serde_yaml::Value::from("disabled"),
                serde_yaml::Value::from(true),
            );
            seq.push(serde_yaml::Value::Mapping(map));
        }
    }

    let body = serde_yaml::to_string(&doc).map_err(|e| format!("序列化用户层补丁失败：{e}"))?;
    let tmp = path.with_extension("yml.tmp-writing");
    std::fs::write(&tmp, body).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 {} 失败：{e}", path.display()))?;
    Ok(())
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
    market_install::plugin_remove(instance_id, package, progress)?;
    // 官方链删除成功后清理 imported/ 孤儿留档（P4 挂账收口，2026-10-05 R2）。
    // 留档无害（只占磁盘），清理失败不推翻已成功的删除——只如实提示。
    match prune_orphan_imported(instance_id) {
        Ok(removed) if !removed.is_empty() => {
            progress(format!("已清理离线安装留档：{}", removed.join("、")));
        }
        Ok(_) => {}
        Err(e) => progress(format!("离线安装留档清理失败（不影响删除）：{e}")),
    }
    Ok(())
}

/// 清理 `imported/` 里的孤儿 tarball（P4 挂账「删除插件不清理 imported/ 残留」）。
///
/// imported/ 留档是 file: 依赖的持久锚点（modpack 导入时落地，删了会让残余
/// 依赖的重装链 ENOENT——2026-09-20 双包 e2e 实证），因此**只删 profile 清单
/// dependencies 中已无任何 `file:` spec 指向的留档**：删除插件后它自己的
/// file: 依赖已从清单移除，其留档即成孤儿；仍被引用的一律保留。
/// file: spec 是绝对路径（modpack.rs 落档时以 `file:{persistent.display()}`
/// 写入），比对按路径末端文件名进行。返回被清理的文件名列表。
fn prune_orphan_imported(instance_id: &str) -> Result<Vec<String>, String> {
    let instance = instances::get_instance(instance_id)?;
    let manifest_path = instances::instance_dir(instance_id)?
        .join("profiles")
        .join(&instance.profile)
        .join("package.json");
    let referenced: Vec<String> = if manifest_path.is_file() {
        let text = std::fs::read_to_string(&manifest_path).map_err(|e| format!("读清单失败：{e}"))?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("清单不是合法 JSON：{e}"))?;
        value
            .get("dependencies")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.values()
                    .filter_map(|spec| spec.as_str())
                    .filter(|s| s.starts_with("file:"))
                    .map(|s| s.trim_start_matches("file:").to_string())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let imported_dir = instances::instance_dir(instance_id)?.join("imported");
    if !imported_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut removed = Vec::new();
    let entries =
        std::fs::read_dir(&imported_dir).map_err(|e| format!("读取 {} 失败：{e}", imported_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("tgz") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let still_referenced = referenced
            .iter()
            .any(|r| r.rsplit(['/', '\\']).next() == Some(name.as_str()));
        if still_referenced {
            continue;
        }
        std::fs::remove_file(&path)
            .map_err(|e| format!("清理留档 {} 失败：{e}", path.display()))?;
        removed.push(name);
    }
    Ok(removed)
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
    // profile 权威清单与其他写入器同口径：tmp + rename 原子替换，写盘中断不留坏 JSON。
    let tmp = manifest_path.with_extension("json.tmp-writing");
    std::fs::write(&tmp, &out).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &manifest_path)
        .map_err(|e| format!("替换 {} 失败：{e}", manifest_path.display()))
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
            (
                "imported-pkg".to_string(),
                "file:C:\\temp\\packages\\imported-pkg-1.2.3.tgz".to_string(),
            ),
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
        // 整合包离线导入产物（file: tarball）归第三方，不进自建组。
        let imported = rows.iter().find(|r| r.name == "imported-pkg").unwrap();
        assert_eq!(imported.group, PluginGroup::Thirdparty, "file: *.tgz 是离线导入不是自建");
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

    /// 组合包夹具：写 profile 清单 + 父包/子插件/纯依赖包的 manifest 与 patch。
    /// 返回 profile 目录。
    fn modpack_fixture(instance_id: &str, profile: &str) -> std::path::PathBuf {
        let profile_dir = crate::instances::instance_dir(instance_id)
            .unwrap()
            .join("profiles")
            .join(profile);
        let nm = profile_dir.join("node_modules");
        // 父组合包：insert 两行指向两个子插件（dsh-base 型：子插件无自带 patch）。
        let parent = nm.join("mod-pack");
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::write(
            parent.join("package.json"),
            r#"{ "name": "mod-pack", "version": "2.0.0",
                 "dsh": { "bundle": { "patch": "./cordis.patch.yml" } } }"#,
        )
        .unwrap();
        std::fs::write(
            parent.join("cordis.patch.yml"),
            "insert:\n  - { id: mod-pack-child-a, name: child-a }\n  - { id: mod-pack-child-b, name: child-b, disabled: true }\n",
        )
        .unwrap();
        // 单体插件：insert 只指向自己（不得误判为组合包）。
        let single = nm.join("dsh-cost-meter");
        std::fs::create_dir_all(&single).unwrap();
        std::fs::write(
            single.join("package.json"),
            r#"{ "name": "dsh-cost-meter", "version": "1.0.0",
                 "dsh": { "bundle": { "patch": "./cordis.patch.yml" } } }"#,
        )
        .unwrap();
        std::fs::write(
            single.join("cordis.patch.yml"),
            "- insert:\n    - { id: dsh-tasks, name: dsh-cost-meter }\n",
        )
        .unwrap();
        // 子插件本体（无 patch，传递依赖形态）。
        for child in ["child-a", "child-b"] {
            let dir = nm.join(child);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("package.json"),
                format!(r#"{{ "name": "{child}", "version": "0.3.0" }}"#),
            )
            .unwrap();
        }
        std::fs::write(
            profile_dir.join("package.json"),
            r#"{ "dependencies": { "mod-pack": "^2.0.0", "dsh-cost-meter": "^1.0.0" },
                 "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "mod-pack", "dsh-cost-meter"] } } }"#,
        )
        .unwrap();
        // 本体底座层 dsh-base（app 安装区，patch 插 N 行官方子插件）：
        // 内置组永不识别为组合包（真机事故：84 个"子插件"，2026-09-12）。
        let base = crate::instances::instance_dir(instance_id)
            .unwrap()
            .join("app")
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh-base");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("package.json"),
            r#"{ "name": "@deepseek-ai/dsh-base", "version": "0.1.1-rc.2",
                 "dsh": { "bundle": { "patch": "./cordis.patch.yml" } } }"#,
        )
        .unwrap();
        std::fs::write(
            base.join("cordis.patch.yml"),
            "insert:\n  - { id: dsh-llm, name: dsh-llm }\n  - { id: dsh-session, name: dsh-session }\n  - { id: dsh-agent, name: dsh-agent }\n",
        )
        .unwrap();
        profile_dir
    }

    #[test]
    fn modpack_detection_and_children_list() {
        let (_lock, tmp) = sandbox("modpack-detect");
        let instance = crate::instances::create_instance("组合包", "web").unwrap();
        modpack_fixture(&instance.id, "web");

        let list = list(&instance.id).unwrap();
        let parent = list
            .plugins
            .iter()
            .find(|p| p.name == "mod-pack")
            .expect("父包在清单里");
        let kids = parent.modpack_children.as_ref().expect("应识别为组合包");
        assert_eq!(kids.len(), 2, "insert 两行（dsh-base 型识别）");
        let a = kids.iter().find(|k| k.name == "child-a").unwrap();
        assert_eq!(a.row_id, "mod-pack-child-a");
        assert!(!a.disabled);
        assert_eq!(a.version, "0.3.0");
        let b = kids.iter().find(|k| k.name == "child-b").unwrap();
        assert!(b.disabled, "insert 行自带 disabled → 初始即停用");
        assert_eq!(b.row_id, "mod-pack-child-b");

        // 单体插件（insert 只指向自己）不得误判。
        let single = list
            .plugins
            .iter()
            .find(|p| p.name == "dsh-cost-meter")
            .unwrap();
        assert!(single.modpack_children.is_none(), "单体插件不是组合包");

        // 内置底座层（dsh-base，patch 插满官方子插件）绝不识别为组合包。
        let base = list
            .plugins
            .iter()
            .find(|p| p.name == "@deepseek-ai/dsh-base")
            .expect("dsh-base 在 bundles 且不在 dependencies → 内置组");
        assert_eq!(base.group, PluginGroup::Builtin);
        assert!(
            base.modpack_children.is_none(),
            "内置底座层不得出现组合包卡（真机事故回归）"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn set_child_enabled_writes_user_layer_and_is_idempotent() {
        let (_lock, tmp) = sandbox("modpack-toggle");
        let instance = crate::instances::create_instance("子插件启停", "web").unwrap();
        let profile_dir = modpack_fixture(&instance.id, "web");
        let user_patch = profile_dir.join("cordis.patch.yml");

        // 初始：child-a 启用。停用 → 用户层出现覆盖行，list 如实反映。
        set_child_enabled(&instance.id, "mod-pack-child-a", false).unwrap();
        let text = std::fs::read_to_string(&user_patch).unwrap();
        assert!(text.contains("mod-pack-child-a") && text.contains("disabled: true"));
        let kids = list(&instance.id)
            .unwrap()
            .plugins
            .into_iter()
            .find(|p| p.name == "mod-pack")
            .unwrap()
            .modpack_children
            .unwrap();
        assert!(kids.iter().find(|k| k.name == "child-a").unwrap().disabled);
        assert!(kids.iter().find(|k| k.name == "child-b").unwrap().disabled);

        // 幂等停用：不产生重复行。
        set_child_enabled(&instance.id, "mod-pack-child-a", false).unwrap();
        let text = std::fs::read_to_string(&user_patch).unwrap();
        assert_eq!(
            text.matches("mod-pack-child-a").count(),
            1,
            "重复停用不得叠加覆盖行"
        );

        // 启用 = 移除覆盖行（恢复父包默认）；child-b 恢复为「自带停用」。
        set_child_enabled(&instance.id, "mod-pack-child-a", true).unwrap();
        let text = std::fs::read_to_string(&user_patch).unwrap();
        assert!(
            !text.contains("mod-pack-child-a"),
            "启用后覆盖行应被移除"
        );
        let kids = list(&instance.id)
            .unwrap()
            .plugins
            .into_iter()
            .find(|p| p.name == "mod-pack")
            .unwrap()
            .modpack_children
            .unwrap();
        assert!(!kids.iter().find(|k| k.name == "child-a").unwrap().disabled);
        assert!(kids.iter().find(|k| k.name == "child-b").unwrap().disabled);

        // 非法行 id 拒绝（防 YAML 结构注入）。
        assert!(set_child_enabled(&instance.id, "a\n- injected", false).is_err());

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 用户层 patch 带 UTF-8 BOM 时（编辑器/PowerShell 常态产物），停用行
    /// 仍必须被识别（2026-09-20 审查 P2-02：状态文件读取的 BOM 容错口径回归）。
    #[test]
    fn user_disabled_ids_tolerate_bom_in_user_patch() {
        let (_lock, tmp) = sandbox("modpack-bom");
        let instance = crate::instances::create_instance("BOM停用", "web").unwrap();
        let profile_dir = modpack_fixture(&instance.id, "web");
        std::fs::write(
            profile_dir.join("cordis.patch.yml"),
            "\u{feff}- { id: mod-pack-child-a, disabled: true }\n",
        )
        .unwrap();

        let kids = list(&instance.id)
            .unwrap()
            .plugins
            .into_iter()
            .find(|p| p.name == "mod-pack")
            .unwrap()
            .modpack_children
            .unwrap();
        assert!(
            kids.iter().find(|k| k.name == "child-a").unwrap().disabled,
            "带 BOM 的用户层覆盖行必须仍被识别为停用"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// imported/ 孤儿留档清理（R2，P4 挂账）：删除插件后其 file: 依赖已出清单，
    /// 留档成孤儿 → 清；仍被引用（相对/绝对路径形态都算）的一律保留。
    /// 布局口径：imported/ 在实例目录下（modpack.rs 落档位），清单在 profile 下。
    #[test]
    fn prune_orphan_imported_keeps_referenced_and_removes_orphans() {
        let (_lock, tmp) = sandbox("prune-imported");
        let instance = crate::instances::create_instance("留档清理", "web").unwrap();
        let instance_root = crate::instances::instance_dir(&instance.id).unwrap();
        let imported = instance_root.join("imported");
        std::fs::create_dir_all(&imported).unwrap();
        for name in ["pkg-a-1.0.0.tgz", "pkg-b-2.0.0.tgz", "pkg-orphan-3.0.0.tgz"] {
            std::fs::write(imported.join(name), b"stub").unwrap();
        }
        // 清单：pkg-a 绝对路径（导入落档的原生形态）、pkg-b 相对路径（手工场景）；
        // pkg-orphan 无任何 file: 引用。
        let profile_dir = instance_root.join("profiles").join("web");
        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            profile_dir.join("package.json"),
            r#"{
  "dependencies": {
    "pkg-a": "file:D:\\fake\\imported\\pkg-a-1.0.0.tgz",
    "pkg-b": "file:imported/pkg-b-2.0.0.tgz"
  }
}"#,
        )
        .unwrap();

        let removed = prune_orphan_imported(&instance.id).unwrap();
        assert_eq!(removed, vec!["pkg-orphan-3.0.0.tgz".to_string()]);
        assert!(imported.join("pkg-a-1.0.0.tgz").is_file(), "绝对路径引用的留档必须保留");
        assert!(imported.join("pkg-b-2.0.0.tgz").is_file(), "相对路径引用的留档必须保留");
        assert!(!imported.join("pkg-orphan-3.0.0.tgz").exists(), "孤儿留档必须被清理");

        // 清单缺失（profile 未初始化）= 全部按孤儿处理，不 panic。
        std::fs::remove_file(profile_dir.join("package.json")).unwrap();
        let removed = prune_orphan_imported(&instance.id).unwrap();
        assert_eq!(removed.len(), 2, "无清单时引用集为空，剩余留档全部清理");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
