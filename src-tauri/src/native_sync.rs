//! 原生配置双向同步（FR-25，phase-08）——M1 模型域先行切片。
//!
//! DSH 0.1.7+ 的网页端改动**不落 settings.yaml**：settings.mutate 写进
//! `profiles/<profile>/cordis.patch.yml` 的 patch 行；启动器投递的 settings.yaml
//! 每次启动被 DSH `importLegacyDocument` 改名 merge 进 patch（对象递归合并、
//! 数组整体替换）后从磁盘消失。由此两条铁律：
//! - **读侧必须双面合并**（settings.yaml ∪ patch 行）——只读 settings.yaml 对
//!   0.1.7+ 实例结构性失明（0.2.0 实例该文件已被改名走）；
//! - **删除只能就地编辑 patch**——投递箱 merge 语义删不掉 patch 里的键
//!   （phase-08 §6：cordis.patch.yml 是 DSH 原生配置文件，编辑属红线合法通道，
//!   与网页端同一写入面）。
//!
//! M1 范围（2026-11-01）：已管厂商路由下模型清单的读侧收编（keyvault 并集）与
//! 毒路由清理；M2 在本模块扩展提供商整体收编 / MCP / 钩子 / Key 迁移。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_yaml::Value;

/// 实例的用户层 patch 文件路径：`<实例>/profiles/<profile>/cordis.patch.yml`。
/// 关隔离时 profiles 是指向 shared 的联接，路径拼法不变、读写天然穿透。
fn user_patch_path(instance_id: &str) -> Result<PathBuf, String> {
    let instance = crate::instances::get_instance(instance_id)?;
    Ok(crate::instances::instance_dir(instance_id)?
        .join("profiles")
        .join(&instance.profile)
        .join(crate::plugin_manage::USER_PATCH_FILENAME))
}

/// patch 文档里 `id: llm-pi-ai` 条目的 `config.providers.<路由>.models` 的
/// 模型 id 清单（0.1.7+ 网页端写入面）。文件缺失/损坏/异形 = 空，fail-soft。
fn patch_provider_models(instance_id: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let path = user_patch_path(instance_id)?;
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = crate::paths::read_text(&path)?;
    let Ok(doc) = serde_yaml::from_str::<Value>(&text) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in crate::plugin_manage::patch_top_entries(&doc) {
        if entry.get("id").and_then(Value::as_str) != Some("llm-pi-ai") {
            continue;
        }
        let Some(providers) = entry
            .get("config")
            .and_then(|config| config.get("providers"))
            .and_then(Value::as_mapping)
        else {
            continue;
        };
        for (key, profile) in providers {
            let Some(route) = key.as_str() else {
                continue;
            };
            let Some(models) = profile.get("models").and_then(Value::as_sequence) else {
                continue;
            };
            let ids: Vec<String> = models.iter().filter_map(yaml_model_id).collect();
            if !ids.is_empty() {
                out.push((route.to_string(), ids));
            }
        }
    }
    Ok(out)
}

/// 模型条目里的 id（serde_yaml 形态：`"<id>"` 与 `{ id: "<id>" }` 两表）。
fn yaml_model_id(entry: &Value) -> Option<String> {
    match entry {
        Value::String(id) if !id.is_empty() => Some(id.clone()),
        Value::Mapping(map) => map
            .get(Value::from("id"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

/// 实例侧 llm-pi-ai 路由的模型 id 清单（双面合并：settings.yaml 面 + patch 面，
/// 同路由拼接后保序去重）。
pub fn instance_side_models(instance_id: &str) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // 面 1：settings.yaml（0.1.5 活文档 / 0.1.7+ 启动器上次投递的残本）。
    for (route, profile) in crate::llm_bridge::existing_providers(instance_id)? {
        let Some(models) = profile.get("models").and_then(serde_json::Value::as_array) else {
            continue;
        };
        let ids: Vec<String> = models
            .iter()
            .filter_map(crate::llm_bridge::model_id_of)
            .collect();
        if !ids.is_empty() {
            out.entry(route).or_default().extend(ids);
        }
    }
    // 面 2：profile patch 行（0.1.7+ 网页端真实落点）。
    for (route, ids) in patch_provider_models(instance_id)? {
        out.entry(route).or_default().extend(ids);
    }
    for list in out.values_mut() {
        *list = dedup_keep_order(list);
    }
    Ok(out)
}

/// 读侧收编（M1 切片，需求方 bug 4）：实例侧（settings.yaml ∪ patch 双面）
/// 启动器**已管厂商**路由下的模型清单 → keyvault 并集（现值在前、新值在后）。
/// 外来路由不收编（整体收编成提供商卡是 M2）；deepseek 的模型清单在
/// llm-deepseek 命名空间，不在 llm-pi-ai providers 里，天然不参与。返回发生
/// 收编的厂商 id（排序去重）。幂等：实例侧模型都已在 keyvault 时零写入。
pub fn adopt_instance_models(instance_id: &str) -> Result<Vec<String>, String> {
    let side = instance_side_models(instance_id)?;
    if side.is_empty() {
        return Ok(Vec::new());
    }
    let status = crate::keyvault::status()?;
    let mut adopted = Vec::new();
    for (vendor, route) in crate::llm_bridge::managed_vendor_routes()? {
        let Some(ids) = side.get(&route) else {
            continue;
        };
        let known = status
            .vendors
            .iter()
            .find(|v| v.vendor == vendor)
            .map(|v| &v.models);
        let new_ids: Vec<String> = ids
            .iter()
            .filter(|id| known.is_none_or(|list| !list.contains(id)))
            .cloned()
            .collect();
        if new_ids.is_empty() {
            continue;
        }
        crate::keyvault::merge_models(&vendor, &new_ids)?;
        adopted.push(vendor);
    }
    adopted.sort();
    Ok(adopted)
}

/// patch 侧毒路由清理（M1，需求方 bug 2「网页选模型没有分组」）：从 `id:
/// llm-pi-ai` 条目的 `config.providers` 里删除「无 models 且不在 pi-ai 目录内」
/// 的路由——它们会让 DSH 节级拒绝整个 llm-pi-ai 命名空间（含完全合法的其他
/// 路由），而投递箱 merge 删不掉这些键，只能就地编辑。返回清除的路由键。
/// fail-soft：文件缺失/损坏/顶层异形一律不动（手写高级补丁自行维护，与
/// plugin_manage 同口径）；无可清路由时零写入。
pub fn clean_patch_poison_routes(instance_id: &str) -> Result<Vec<String>, String> {
    let path = user_patch_path(instance_id)?;
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = crate::paths::read_text(&path)?;
    let mut doc: Value = match serde_yaml::from_str(&text) {
        Ok(doc) => doc,
        Err(_) => return Ok(Vec::new()),
    };
    let Value::Sequence(items) = &mut doc else {
        return Ok(Vec::new());
    };
    let mut removed = Vec::new();
    for entry in items.iter_mut() {
        if entry.get("id").and_then(Value::as_str) != Some("llm-pi-ai") {
            continue;
        }
        let Some(providers) = entry
            .get_mut("config")
            .and_then(|config| config.get_mut("providers"))
            .and_then(Value::as_mapping_mut)
        else {
            continue;
        };
        let poison: Vec<Value> = providers
            .iter()
            .filter_map(|(key, profile)| {
                let route = key.as_str()?;
                let json = serde_json::to_value(profile).ok()?;
                crate::llm_bridge::is_poison_route(route, &json).then(|| key.clone())
            })
            .collect();
        for key in poison {
            if let Some(route) = key.as_str() {
                removed.push(route.to_string());
            }
            providers.remove(&key);
        }
    }
    if removed.is_empty() {
        return Ok(removed);
    }
    let body = serde_yaml::to_string(&doc).map_err(|e| format!("patch 序列化失败：{e}"))?;
    // 原子写回（tmp + rename，与 inject/keyvault 同款）。
    let tmp = path.with_extension("yml.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 {} 失败：{e}", path.display()))?;
    removed.sort();
    Ok(removed)
}

/// 保序去重。
fn dedup_keep_order(list: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    list.iter()
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances;

    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        instances::tests::sandbox(&format!("native-sync-{tag}"))
    }

    fn profile_dir(instance_id: &str) -> std::path::PathBuf {
        let instance = instances::get_instance(instance_id).unwrap();
        instances::instance_dir(instance_id)
            .unwrap()
            .join("profiles")
            .join(&instance.profile)
    }

    fn vault_models(vendor: &str) -> Vec<String> {
        crate::keyvault::status()
            .unwrap()
            .vendors
            .into_iter()
            .find(|v| v.vendor == vendor)
            .map(|v| v.models)
            .unwrap_or_default()
    }

    /// patch 读面：`id: llm-pi-ai` 条目的 providers.models 抽成路由 → id 清单；
    /// 字符串与对象两种条目形态都认。
    #[test]
    fn patch_provider_models_reads_both_entry_shapes() {
        let (_lock, tmp) = sandbox("patch-read");
        let instance = instances::create_instance("patch 读", "web").unwrap();
        let dir = profile_dir(&instance.id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(crate::plugin_manage::USER_PATCH_FILENAME),
            "- id: llm-pi-ai\n  config:\n    providers:\n      custom-x:\n        api: openai-completions\n        models:\n          - relay-a\n          - { id: relay-b, name: Relay B }\n- id: agent-default-model\n  config:\n    provider: deepseek-official\n",
        )
        .unwrap();

        let models = patch_provider_models(&instance.id).unwrap();
        assert_eq!(models.len(), 1, "agent-default-model 条目不参与：{models:?}");
        assert_eq!(models[0].0, "custom-x");
        assert_eq!(models[0].1, vec!["relay-a", "relay-b"]);
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 双面合并：settings.yaml 面 + patch 面的同路由清单拼接去重。
    #[test]
    fn instance_side_models_merges_both_faces() {
        let (_lock, tmp) = sandbox("dual-face");
        let instance = instances::create_instance("双面", "web").unwrap();
        let dir = instances::instance_dir(&instance.id).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.yaml"),
            "llm-pi-ai:\n  providers:\n    glm:\n      api: openai-completions\n      models:\n        - glm-4.6\n",
        )
        .unwrap();
        let pdir = profile_dir(&instance.id);
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join(crate::plugin_manage::USER_PATCH_FILENAME),
            "- id: llm-pi-ai\n  config:\n    providers:\n      glm:\n        models:\n          - glm-4.6\n          - glm-4.5-air\n",
        )
        .unwrap();

        let side = instance_side_models(&instance.id).unwrap();
        assert_eq!(
            side.get("glm").unwrap(),
            &vec!["glm-4.6".to_string(), "glm-4.5-air".to_string()]
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 读侧收编：patch 里已管厂商（custom-*/kimi 的目录路由）的模型并入
    /// keyvault；外来路由（无厂商对应）不收编；幂等（重复收编零新增）。
    #[test]
    fn adopt_merges_managed_vendor_models_only() {
        let (_lock, tmp) = sandbox("adopt");
        let instance = instances::create_instance("收编", "web").unwrap();
        let custom = crate::providers::add_custom(
            "My Relay",
            "https://relay.example.com/v1",
            crate::providers::ApiStyle::Openai,
        )
        .unwrap();
        crate::keyvault::set_models("kimi", &["kimi-k2.5".into()]).unwrap();
        let pdir = profile_dir(&instance.id);
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join(crate::plugin_manage::USER_PATCH_FILENAME),
            format!(
                "- id: llm-pi-ai\n  config:\n    providers:\n      moonshotai-cn:\n        models:\n          - kimi-k2.5\n          - kimi-k3\n      {id}:\n        models:\n          - stranger-model\n      nobody-relay:\n        models:\n          - foreign-model\n",
                id = custom.id
            ),
        )
        .unwrap();

        let adopted = adopt_instance_models(&instance.id).unwrap();
        assert_eq!(
            adopted,
            vec![custom.id.clone(), "kimi".to_string()],
            "已管厂商收编（排序去重）；nobody-relay 是外来路由不收编"
        );
        assert_eq!(vault_models("kimi"), vec!["kimi-k2.5", "kimi-k3"], "并集：现值在前、新值在后");
        assert_eq!(vault_models(&custom.id), vec!["stranger-model"]);

        let again = adopt_instance_models(&instance.id).unwrap();
        assert!(again.is_empty(), "幂等：都已收编则零动作");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// patch 毒路由清理三态：毒路由（无 models、非目录）删；健康外来（有
    /// models）与目录路由（无 models 但在目录）留；其他条目（agent-default-
    /// model）与文件结构原样。无残可清时零写入。
    #[test]
    fn clean_patch_removes_poison_keeps_healthy() {
        let (_lock, tmp) = sandbox("clean");
        let instance = instances::create_instance("清理", "web").unwrap();
        let path = profile_dir(&instance.id).join(crate::plugin_manage::USER_PATCH_FILENAME);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "- id: llm-pi-ai\n  config:\n    providers:\n      glm:\n        api: openai-completions\n      moonshotai-cn:\n        apiKeyEnv: MOONSHOT_API_KEY\n      custom-y:\n        api: openai-completions\n        models:\n          - keep-me\n- id: agent-default-model\n  config:\n    provider: deepseek-official\n    model: deepseek-v4-flash\n",
        )
        .unwrap();

        let removed = clean_patch_poison_routes(&instance.id).unwrap();
        assert_eq!(removed, vec!["glm"], "glm 无 models 且非目录 = 毒；moonshotai-cn 在目录 = 留");

        let doc: Value =
            serde_yaml::from_str(&crate::paths::read_text(&path).unwrap()).unwrap();
        let entries = doc.as_sequence().unwrap();
        assert_eq!(entries.len(), 2, "条目数不变");
        let llm = entries
            .iter()
            .find(|e| e.get("id").and_then(Value::as_str) == Some("llm-pi-ai"))
            .unwrap();
        let providers = llm.get("config").unwrap().get("providers").unwrap();
        assert!(providers.get("glm").is_none(), "毒路由已删");
        assert!(providers.get("moonshotai-cn").is_some(), "目录路由保留");
        assert_eq!(
            providers.get("custom-y").unwrap().get("models").unwrap()[0].as_str(),
            Some("keep-me"),
            "健康外来路由原样"
        );
        let adm = entries
            .iter()
            .find(|e| e.get("id").and_then(Value::as_str) == Some("agent-default-model"))
            .unwrap();
        assert_eq!(
            adm.get("config").unwrap().get("provider").and_then(Value::as_str),
            Some("deepseek-official"),
            "其他条目不碰"
        );

        // 无残可清：不再改写（返回空，文件内容原样）。
        let before = crate::paths::read_text(&path).unwrap();
        assert!(clean_patch_poison_routes(&instance.id).unwrap().is_empty());
        assert_eq!(crate::paths::read_text(&path).unwrap(), before, "零写入");

        // 损坏/异形 fail-soft：不报错、不动文件。
        std::fs::write(&path, "::: 不是 YAML :::").unwrap();
        assert!(clean_patch_poison_routes(&instance.id).unwrap().is_empty());
        std::fs::write(&path, "insert:\n  - { id: x, name: x }\n").unwrap();
        assert!(clean_patch_poison_routes(&instance.id).unwrap().is_empty());
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 端到端：sync_instance = 先收编（patch 模型进 vault）→ 再写 settings.yaml
    /// （路由带收编后的清单）→ 最后清 patch 毒路由。
    #[test]
    fn sync_adopts_then_writes_then_cleans() {
        let (_lock, tmp) = sandbox("sync-e2e");
        let instance = instances::create_instance("端到端", "web").unwrap();
        let custom = crate::providers::add_custom(
            "My Relay",
            "https://relay.example.com/v1",
            crate::providers::ApiStyle::Openai,
        )
        .unwrap();
        crate::keyvault::set_enabled(&custom.id, true).unwrap();
        let pdir = profile_dir(&instance.id);
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join(crate::plugin_manage::USER_PATCH_FILENAME),
            "- id: llm-pi-ai\n  config:\n    providers:\n      glm:\n        api: openai-completions\n",
        )
        .unwrap();

        let outcome = crate::llm_bridge::sync_instance(&instance.id).unwrap();
        assert!(outcome.adopted.is_empty(), "patch 里没有带 models 的已管路由");
        assert_eq!(outcome.cleaned, vec!["glm"], "毒 glm 从 patch 清除");

        let settings: serde_json::Value = serde_yaml::from_str(
            &crate::paths::read_text(
                &instances::instance_dir(&instance.id).unwrap().join("settings.yaml"),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            settings.get("llm-pi-ai").is_none(),
            "毒路由不进 settings.yaml，也没有其他可写路由 → 整节缺席：{settings}"
        );

        // 网页侧补模型 → 再 sync：先收编进 vault，路由清单带上它（不被数组替换抹掉）。
        let path = pdir.join(crate::plugin_manage::USER_PATCH_FILENAME);
        std::fs::write(
            &path,
            format!(
                "- id: llm-pi-ai\n  config:\n    providers:\n      {id}:\n        api: openai-completions\n        models:\n          - web-added-model\n",
                id = custom.id
            ),
        )
        .unwrap();

        let outcome = crate::llm_bridge::sync_instance(&instance.id).unwrap();
        assert_eq!(outcome.adopted, vec![custom.id.clone()], "网页侧模型收编进 vault");
        assert_eq!(vault_models(&custom.id), vec!["web-added-model"]);
        let settings: serde_json::Value = serde_yaml::from_str(
            &crate::paths::read_text(
                &instances::instance_dir(&instance.id).unwrap().join("settings.yaml"),
            )
            .unwrap(),
        )
        .unwrap();
        let route = &settings["llm-pi-ai"]["providers"][&custom.id];
        assert_eq!(
            route["models"][0]["id"], "web-added-model",
            "路由清单带收编后的模型（投递箱数组替换也不丢）"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }
}
