//! 模型路由桥接（R6）：把启动器「勾选启用」的厂商写进实例 settings.yaml 的
//! `llm-pi-ai.providers.<路由>` 段。
//!
//! 侦察结论（phase-03 日志 R6）：DSH 的模型路由**没有静态表**——bundle 层的
//! llm-pi-ai adapter 以「零路由休眠态」挂载，只有 settings.yaml 的
//! `llm-pi-ai.providers.<路由>` profile 出现时才注册路由；不写 = DSH
//! 「设置>模型」看不见该厂商、输入框也没有它的模型，注入的 Key 环境变量
//! 本身不产生路由。本模块就是「启动器勾选启用 → DSH 可见可调用」的桥。
//!
//! 写入位置是 inject.json 的 settings 映射（启动器对 settings.yaml 的唯一
//! 事实来源），再经 inject::write_instance_files 再生 settings.yaml——直接改
//! settings.yaml 会被下一次再生抹掉。外来路由（用户在 DSH 模型页自行添加、
//! 不属于启动器厂商的路由键）在桥接时从现有 settings.yaml 读出并保留。
//!
//! 路由映射（定稿依据：pi-ai 目录与 dist 源码逐个核对，见 R5b/R6 日志）：
//! - deepseek 不写路由：bundle 层自带 deepseek-official adapter，Key 吃注入的
//!   DEEPSEEK_API_KEY，地址覆盖经 DEEPSEEK_BASE_URL 注入（process.rs）；
//! - 与 pi-ai 目录同名的厂商用目录路由（协议/基址/模型目录全部继承目录）：
//!   openai/anthropic/groq/cerebras/together/nvidia/huggingface/ant-ling/zai/
//!   zai-coding-cn/moonshotai/xiaomi/qwen-token-plan(-cn)/minimax(-cn)/
//!   kimi-coding/fireworks/vercel-ai-gateway/openrouter；kimi→moonshotai-cn
//!   （基址与目录完全一致）、gemini→google；
//! - 无目录路由的厂商（glm/qwen/siliconflow/ollama/自定义）写自描述路由：
//!   api = 按接口形态映射（openai→openai-completions、responses→
//!   openai-responses、anthropic→anthropic-messages、ollama→openai-completions
//!   且基址补 /v1；gemini 形态在配置协议中无对应者），baseURL = 地址覆盖 ??
//!   目录默认，models = 凭据库模型清单（缺省元数据按 llm-pi-ai config.ts 的
//!   DEFAULT_CONTEXT_WINDOW/MAX_TOKENS 兜底）∪ 实例现有清单。
//!
//! 为什么自描述路由必须带 models（R7-2 真机裁决）：DSH 只对**目录内**的
//! route key 兜底模型目录；自描述路由的模型只能来自配置。给一条没有 models 的
//! 自描述路由，DSH 里该提供方会以
//! `llm-pi-ai: provider "glm" resolves no models; the installed catalog does not
//! describe this route, so its models must be listed in configuration` 报错出现
//! 且一个模型都没有（启动不失败，但那条路由废掉）。所以：
//! - 自描述路由在「凭据库清单 ∪ 实例现有清单」都为空时**不写**，改为上报
//!   `SkippedRoute`（precheck 警告 + 设置页提示），不伪造模型、也不留红字；
//! - 接口形态在配置协议里无对应者（gemini 形态的自定义提供方）同样不上报
//!   路由，只上报跳过原因；
//! - 实例里已有的 models（用户在 DSH 模型页补的）原样沿用，不被桥接抹掉。

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

/// 非目录模型缺失元数据时的兜底（与 llm-pi-ai config.ts 的默认值一致）。
const DEFAULT_MODEL_CONTEXT_WINDOW: u64 = 262_144;
const DEFAULT_MODEL_MAX_TOKENS: u64 = 32_768;

/// 跳过原因码：自描述路由的模型清单为空（凭据库与实例都没有）。
pub const SKIP_NO_MODELS: &str = "no-models";
/// 跳过原因码：接口形态在配置协议里没有对应者（gemini 形态的自定义提供方）。
pub const SKIP_NO_PROTOCOL: &str = "no-protocol";

/// 一条「勾了启用、但没能进实例」的路由。给前端的如实报告：绝不静默丢弃。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedRoute {
    /// 启动器厂商 id。
    pub vendor: String,
    /// 厂商显示名（报告文案直接用）。
    pub label: String,
    /// 原因码：`no-models` | `no-protocol`（见上面的常量）。
    pub reason: String,
}

/// 一次桥接计算的产物：期望路由表 + 管理中路由键 + 被跳过的路由。
struct RouteDesire {
    routes: Map<String, Value>,
    managed: Vec<String>,
    skipped: Vec<SkippedRoute>,
}

/// 目录路由映射：启动器厂商 id → pi-ai 目录 route key。None = 非目录路由。
/// deepseek 不在表内：bundle 层内建 adapter 拥有它，绝不写 pi-ai 路由。
///
/// 表内每一项都必须真实存在于安装版 pi-ai 的 `providers/` 目录（漂移守卫见
/// `catalog_routes_exist_in_installed_pi_ai`）：写错或写多会让桥接落一条模型
/// 清单为空的自描述路由，DSH 侧报 `resolves no models`。
const CATALOG_ROUTES: &[(&str, &str)] = &[
    ("openai", "openai"),
    ("anthropic", "anthropic"),
    ("groq", "groq"),
    ("cerebras", "cerebras"),
    ("together", "together"),
    ("nvidia", "nvidia"),
    ("huggingface", "huggingface"),
    ("ant-ling", "ant-ling"),
    ("zai", "zai"),
    ("zai-coding-cn", "zai-coding-cn"),
    ("moonshotai", "moonshotai"),
    ("xiaomi", "xiaomi"),
    ("qwen-token-plan", "qwen-token-plan"),
    ("qwen-token-plan-cn", "qwen-token-plan-cn"),
    ("minimax", "minimax"),
    ("minimax-cn", "minimax-cn"),
    ("kimi-coding", "kimi-coding"),
    ("fireworks", "fireworks"),
    ("vercel-ai-gateway", "vercel-ai-gateway"),
    ("openrouter", "openrouter"),
    ("xai", "xai"),
    ("kimi", "moonshotai-cn"),
    ("gemini", "google"),
];

fn catalog_route(vendor: &str) -> Option<&'static str> {
    CATALOG_ROUTES
        .iter()
        .find(|(id, _)| *id == vendor)
        .map(|(_, route)| *route)
}

/// 接口形态 → 配置协议名（pi-ai supportedProtocols 三值）；None = 该形态无法
/// 自描述（gemini 目录路由走 catalog_route，不经此处）。
fn protocol_of(style: &str) -> Option<&'static str> {
    match style {
        "openai" => Some("openai-completions"),
        "responses" => Some("openai-responses"),
        "anthropic" => Some("anthropic-messages"),
        "ollama" => Some("openai-completions"),
        _ => None,
    }
}

/// ollama 的 OpenAI 兼容端点挂在 /v1 下（原生 /api/tags 与 /api/chat 不在
/// 配置协议的路径约定内）。
fn ollama_openai_base(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

/// 凭据库模型 id 清单 → 路由 models 数组（元数据按 llm-pi-ai 默认容量兜底）。
fn models_profiles(models: &[String]) -> Value {
    Value::Array(
        models
            .iter()
            .map(|id| {
                json!({
                    "id": id,
                    "name": id,
                    "contextWindow": DEFAULT_MODEL_CONTEXT_WINDOW,
                    "maxTokens": DEFAULT_MODEL_MAX_TOKENS,
                    "input": ["text"],
                })
            })
            .collect(),
    )
}

/// 生成「启动器管理的路由」期望表 + 全部管理中路由键（含停用者——停用者的
/// 旧路由键要从 settings.yaml 删除，且不得误判为外来路由）+ 被跳过的路由。
///
/// `existing` = 实例 settings.yaml 现有的 `llm-pi-ai.providers` 映射：用于
/// 沿用网页侧已写入的 models（见模块头）。
fn desired_routes(existing: &Map<String, Value>) -> Result<RouteDesire, String> {
    let status = crate::keyvault::status()?;
    let view = crate::providers::view()?;

    // 全部管理中路由键：每个非 deepseek 厂商（启用与否）+ 每个自定义提供方。
    let mut managed: Vec<String> = Vec::new();
    for entry in &view.catalog {
        if entry.id == "deepseek" {
            continue;
        }
        managed.push(catalog_route(&entry.id).unwrap_or(&entry.id).to_string());
    }
    for custom in &view.custom {
        managed.push(custom.id.clone());
    }

    let mut routes = Map::new();
    let mut skipped = Vec::new();
    for vendor in &status.vendors {
        if !vendor.enabled || vendor.vendor == "deepseek" || vendor.vendor == crate::keyvault::VENDOR_GITHUB
        {
            continue;
        }
        // 参与路由的资格 = 在启动器目录/自定义里，而不是「有没有 Key 变量名」：
        // 自定义提供方的变量名按 id 派生（keyvault::env_name_of），ollama 免 Key
        // （路由不带 apiKeyEnv）——按老门禁（vendor_env_name）这两类会被静默丢弃。
        let env = crate::keyvault::env_name_of(&vendor.vendor);
        // 目录路由：同名厂商，协议/基址/模型目录继承 pi-ai；用户改过地址才
        // 写 baseURL 覆盖。
        if let Some(route) = catalog_route(&vendor.vendor) {
            let mut profile = Map::new();
            if let Some(env) = &env {
                profile.insert("apiKeyEnv".into(), json!(env));
            }
            profile.insert("displayName".into(), json!(label_in(&view, &vendor.vendor)));
            if let Some(base) = &vendor.address {
                profile.insert("baseURL".into(), json!(base));
            }
            // 目录路由的模型目录归 pi-ai；实例里已有的 models 原样沿用，
            // 免得把网页侧补的条目抹掉。
            if let Some(models) = existing_models(existing, route) {
                profile.insert("models".into(), models);
            }
            routes.insert(route.to_string(), Value::Object(profile));
            continue;
        }
        // 非目录路由：api 按形态映射，baseURL = 地址覆盖 ?? 目录默认，模型来自
        // 凭据库清单 ∪ 实例现有清单。
        let (style, base, label) = if let Some(custom) =
            view.custom.iter().find(|c| c.id == vendor.vendor)
        {
            (
                style_name(custom.style),
                custom.base.clone(),
                custom.name.clone(),
            )
        } else if let Some(entry) = view.catalog.iter().find(|e| e.id == vendor.vendor) {
            (
                style_name(entry.style),
                entry.base.clone(),
                entry.label.clone(),
            )
        } else {
            continue;
        };
        let Some(protocol) = protocol_of(&style) else {
            // 该形态在配置协议里没有对应者：写不出可用路由，只上报。
            skipped.push(SkippedRoute {
                vendor: vendor.vendor.clone(),
                label,
                reason: SKIP_NO_PROTOCOL.to_string(),
            });
            continue;
        };
        let models = merged_model_entries(&vendor.models, existing_models(existing, &vendor.vendor));
        if models.as_array().is_none_or(|list| list.is_empty()) {
            // 自描述路由没有模型 = DSH 里必然 resolves no models：宁可不写，
            // 也不留一条红字废路由；由 precheck 与设置页告诉用户怎么补。
            skipped.push(SkippedRoute {
                vendor: vendor.vendor.clone(),
                label,
                reason: SKIP_NO_MODELS.to_string(),
            });
            continue;
        }
        let mut base = vendor.address.clone().unwrap_or(base);
        if style == "ollama" {
            base = ollama_openai_base(&base);
        }
        let mut profile = Map::new();
        profile.insert("api".into(), json!(protocol));
        profile.insert("baseURL".into(), json!(base));
        if let Some(env) = &env {
            profile.insert("apiKeyEnv".into(), json!(env));
        }
        profile.insert("displayName".into(), json!(label));
        profile.insert("models".into(), models);
        routes.insert(vendor.vendor.clone(), Value::Object(profile));
    }
    Ok(RouteDesire {
        routes,
        managed,
        skipped,
    })
}

/// 该厂商在实例里的**提供方 id**（写进 `agent-default-model.provider` 的值）：
/// deepseek 走内建 adapter（`deepseek-official`），目录厂商走目录 route key，
/// 非目录厂商就是它自己的 id。None = 本启动器给它落不了路由。
pub fn route_key_of(vendor: &str) -> Result<Option<String>, String> {
    let view = crate::providers::view()?;
    if vendor == "deepseek" {
        return Ok(Some("deepseek-official".to_string()));
    }
    if let Some(route) = catalog_route(vendor) {
        return Ok(Some(route.to_string()));
    }
    let Some(style) = style_in(&view, vendor) else {
        return Ok(None);
    };
    Ok(protocol_of(&style).map(|_| vendor.to_string()))
}

/// 厂商在启动器目录/自定义里的接口形态；两处都没有 = 不在启动器管理范围。
fn style_in(view: &crate::providers::ProvidersView, vendor: &str) -> Option<String> {
    if let Some(custom) = view.custom.iter().find(|c| c.id == vendor) {
        return Some(style_name(custom.style));
    }
    view.catalog
        .iter()
        .find(|e| e.id == vendor)
        .map(|entry| style_name(entry.style))
}

/// 厂商显示名（报告文案用）。
fn label_in(view: &crate::providers::ProvidersView, vendor: &str) -> String {
    if let Some(custom) = view.custom.iter().find(|c| c.id == vendor) {
        return custom.name.clone();
    }
    view.catalog
        .iter()
        .find(|e| e.id == vendor)
        .map(|entry| entry.label.clone())
        .unwrap_or_else(|| vendor.to_string())
}

/// 接口形态枚举 → 配置协议映射用的字符串（serde 小写名）。
fn style_name(style: crate::providers::ApiStyle) -> String {
    serde_json::to_value(style)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// 实例 settings.yaml 里某路由现有的 models 数组（网页侧写入）；空数组按"没有"
/// 处理——空清单与没有清单对 DSH 是同一件事。
fn existing_models(existing: &Map<String, Value>, route: &str) -> Option<Value> {
    let models = existing.get(route)?.get("models")?;
    models
        .as_array()
        .filter(|list| !list.is_empty())
        .map(|_| models.clone())
}

/// 自描述路由的 models = 启动器清单（默认元数据兜底）+ 实例现有清单里启动器
/// 没有的条目（原样沿用，保住网页侧补的元数据）。无 id 的畸形条目不沿用——
/// 写进实例会让 DSH 拒绝整节。
fn merged_model_entries(vendor_models: &[String], existing: Option<Value>) -> Value {
    let mut entries: Vec<Value> = models_profiles(vendor_models)
        .as_array()
        .cloned()
        .unwrap_or_default();
    let Some(Value::Array(list)) = existing else {
        return Value::Array(entries);
    };
    for entry in list {
        let Some(id) = model_id_of(&entry) else {
            continue;
        };
        let known = entries
            .iter()
            .any(|known| model_id_of(known).as_deref() == Some(id.as_str()));
        if !known {
            entries.push(entry);
        }
    }
    Value::Array(entries)
}

/// 模型条目里的 id（`"<id>"` 与 `{ id: "<id>" }` 两种形态）。
fn model_id_of(entry: &Value) -> Option<String> {
    match entry {
        Value::String(id) if !id.is_empty() => Some(id.clone()),
        Value::Object(map) => map
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

/// 从现有 settings.yaml 里挑出**外来路由**：既不在启动器管理键集合里、也不是
/// 本次期望写入的键——用户在 DSH 模型页/网页侧自行添加的提供商，桥接必须原样
/// 保留（`sync_instance` 写回）与如实对账（`external_route_keys`）。判定只有这
/// 一处，避免两条路径的规则漂移。
fn foreign_providers(
    existing: &Map<String, Value>,
    managed: &[String],
    desired: &Map<String, Value>,
) -> Map<String, Value> {
    let mut foreign = Map::new();
    for (key, value) in existing {
        if !managed.contains(key) && !desired.contains_key(key) {
            foreign.insert(key.clone(), value.clone());
        }
    }
    foreign
}

/// 实例 settings.yaml 的 `llm-pi-ai.providers` 映射（缺失/不可解析都按空处理：
/// 路由段不存在与"没有外来路由"对桥接是同一件事）。
fn existing_providers(instance_id: &str) -> Result<Map<String, Value>, String> {
    let settings_path = crate::instances::instance_dir(instance_id)?.join("settings.yaml");
    if !settings_path.is_file() {
        return Ok(Map::new());
    }
    let text = crate::paths::read_text(&settings_path)?;
    let root: Value = serde_yaml::from_str(&text).unwrap_or(Value::Null);
    Ok(root
        .get("llm-pi-ai")
        .and_then(|ns| ns.get("providers"))
        .and_then(|p| p.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 该实例里**非启动器管理**的 llm 路由键（用户在网页侧自己加的提供商）。
/// 启动对账用：只读键名，不读值、不碰内容。
pub fn external_route_keys(instance_id: &str) -> Result<BTreeSet<String>, String> {
    let existing = existing_providers(instance_id)?;
    let desire = desired_routes(&existing)?;
    Ok(foreign_providers(&existing, &desire.managed, &desire.routes)
        .keys()
        .cloned()
        .collect())
}

/// 路由对账报告（R7）：本次启动会被跳过的路由（勾了启用但落不了地）+
/// 实例里启动器不管的外来路由键。纯读，不改任何文件。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteReport {
    /// 勾了启用但没写进实例的路由及原因。
    pub skipped: Vec<SkippedRoute>,
    /// 实例里非启动器管理、桥接原样保留的外来路由键。
    pub external: Vec<String>,
}

/// 计算某实例的路由对账报告（precheck 警告 / 设置页提示共用同一份判定）。
pub fn route_report(instance_id: &str) -> Result<RouteReport, String> {
    let existing = existing_providers(instance_id)?;
    let desire = desired_routes(&existing)?;
    let mut external: Vec<String> = foreign_providers(&existing, &desire.managed, &desire.routes)
        .keys()
        .cloned()
        .collect();
    external.sort();
    Ok(RouteReport {
        skipped: desire.skipped,
        external,
    })
}

/// 把单个实例的 settings.yaml 路由段同步成期望状态（经 inject.json——
/// settings.yaml 由 inject 引擎再生，直改会被下一次再生抹掉）。外来路由
/// （非启动器管理键）原样保留。幂等：内容不变时也安全重写。
pub fn sync_instance(instance_id: &str) -> Result<(), String> {
    let existing = existing_providers(instance_id)?;
    let desire = desired_routes(&existing)?;

    let mut providers = Map::new();
    for (key, value) in foreign_providers(&existing, &desire.managed, &desire.routes) {
        providers.insert(key, value);
    }
    for (key, value) in desire.routes {
        providers.insert(key, value);
    }

    let mut config = crate::inject::get_override(instance_id)?;
    if providers.is_empty() {
        config.settings.remove("llm-pi-ai");
    } else {
        let mut namespace = config
            .settings
            .get("llm-pi-ai")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        namespace.insert("providers".into(), Value::Object(providers));
        config
            .settings
            .insert("llm-pi-ai".into(), Value::Object(namespace));
    }
    crate::inject::set_override(instance_id, &config)?;
    crate::inject::write_instance_files(instance_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances;

    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        instances::tests::sandbox(&format!("bridge-{tag}"))
    }

    fn settings_yaml(instance_id: &str) -> Value {
        let dir = instances::instance_dir(instance_id).unwrap();
        let text = std::fs::read_to_string(dir.join("settings.yaml")).unwrap();
        serde_yaml::from_str(&text).unwrap()
    }

    fn providers_of(root: &Value) -> Map<String, Value> {
        root.get("llm-pi-ai")
            .and_then(|ns| ns.get("providers"))
            .and_then(|p| p.as_object())
            .cloned()
            .unwrap_or_default()
    }

    /// 桥接主链：目录厂商（kimi→moonshotai-cn）与非目录厂商（glm，带地址
    /// 覆盖与模型清单）各写各的 profile；deepseek 恒不出现在路由里。
    #[test]
    fn bridge_writes_catalog_and_selfdescribed_routes() {
        let (_lock, tmp) = sandbox("routes");
        let instance = instances::create_instance("桥接测试", "web").unwrap();
        crate::keyvault::set_enabled("kimi", true).unwrap();
        crate::keyvault::set_enabled("glm", true).unwrap();
        crate::keyvault::set_models("glm", &["glm-4.6".into(), "glm-4.5-air".into()]).unwrap();
        crate::keyvault::set_address("glm", "https://my-relay.example.com/v4").unwrap();
        crate::keyvault::set_enabled("deepseek", true).unwrap();

        sync_instance(&instance.id).unwrap();

        let providers = providers_of(&settings_yaml(&instance.id));
        let kimi = providers.get("moonshotai-cn").expect("kimi 应桥到 moonshotai-cn");
        assert_eq!(kimi["apiKeyEnv"], "MOONSHOT_API_KEY");
        assert!(kimi.get("models").is_none(), "目录路由不写 models");
        assert!(kimi.get("baseURL").is_none(), "未改地址不写 baseURL 覆盖");

        let glm = providers.get("glm").expect("非目录厂商应写自描述路由");
        assert_eq!(glm["api"], "openai-completions");
        assert_eq!(glm["baseURL"], "https://my-relay.example.com/v4", "地址覆盖进路由");
        assert_eq!(glm["apiKeyEnv"], "ZAI_API_KEY");
        let models = glm["models"].as_array().unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["id"], "glm-4.6");
        assert_eq!(models[0]["contextWindow"], 262_144, "缺省元数据按官方默认兜底");

        assert!(
            !providers.contains_key("deepseek"),
            "deepseek 走内建 adapter，绝不写 pi-ai 路由"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 停用 = 路由键删除；外来路由（用户在 DSH 侧自加）保留不被抹。
    #[test]
    fn disable_removes_route_and_foreign_routes_survive() {
        let (_lock, tmp) = sandbox("foreign");
        let instance = instances::create_instance("外来测试", "web").unwrap();
        crate::keyvault::set_enabled("kimi", true).unwrap();
        // 预置一份含「外来路由」的 settings.yaml（用户在 DSH 模型页自加）。
        let dir = instances::instance_dir(&instance.id).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.yaml"),
            "llm-pi-ai:\n  providers:\n    my-own-relay:\n      api: openai-completions\n      baseURL: https://x.example.com\n",
        )
        .unwrap();

        sync_instance(&instance.id).unwrap();
        let providers = providers_of(&settings_yaml(&instance.id));
        assert!(providers.contains_key("moonshotai-cn"));
        assert!(
            providers.contains_key("my-own-relay"),
            "外来路由必须保留：{:?}",
            providers.keys().collect::<Vec<_>>()
        );

        crate::keyvault::set_enabled("kimi", false).unwrap();
        sync_instance(&instance.id).unwrap();
        let providers = providers_of(&settings_yaml(&instance.id));
        assert!(!providers.contains_key("moonshotai-cn"), "停用即撤路由");
        assert!(
            providers.contains_key("my-own-relay"),
            "外来路由在多次桥接后仍在"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 地址覆盖回默认（清空地址）→ baseURL 覆盖随之消失，回落目录基址。
    #[test]
    fn clearing_address_drops_base_url_override() {
        let (_lock, tmp) = sandbox("addr");
        let instance = instances::create_instance("地址测试", "web").unwrap();
        crate::keyvault::set_enabled("glm", true).unwrap();
        // glm 是自描述路由：没有模型清单就不写路由（见
        // skip_selfdescribed_route_without_models），观察地址覆盖前必须先给模型。
        crate::keyvault::set_models("glm", &["glm-4.6".into()]).unwrap();
        crate::keyvault::set_address("glm", "https://changed.example.com").unwrap();
        sync_instance(&instance.id).unwrap();
        assert_eq!(
            providers_of(&settings_yaml(&instance.id))["glm"]["baseURL"],
            "https://changed.example.com"
        );
        crate::keyvault::set_address("glm", "").unwrap();
        sync_instance(&instance.id).unwrap();
        assert_eq!(
            providers_of(&settings_yaml(&instance.id))["glm"]["baseURL"],
            "https://open.bigmodel.cn/api/paas/v4",
            "清空覆盖 → 回落启动器目录默认地址"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// R7-2：自描述路由没有模型 → 不写进实例，改为上报跳过原因。
    /// 真机依据：DSH 对该形态报 `resolves no models` 且提供方零模型（.recon T1）。
    #[test]
    fn skip_selfdescribed_route_without_models() {
        let (_lock, tmp) = sandbox("skip-nomodels");
        let instance = instances::create_instance("跳过测试", "web").unwrap();
        crate::keyvault::set_enabled("glm", true).unwrap(); // 无模型

        sync_instance(&instance.id).unwrap();

        let providers = providers_of(&settings_yaml(&instance.id));
        assert!(
            !providers.contains_key("glm"),
            "无模型的自描述路由不写：{:?}",
            providers.keys().collect::<Vec<_>>()
        );
        let report = route_report(&instance.id).unwrap();
        assert_eq!(report.skipped.len(), 1, "{:?}", report.skipped);
        assert_eq!(report.skipped[0].vendor, "glm");
        assert_eq!(report.skipped[0].label, "GLM（智谱）");
        assert_eq!(report.skipped[0].reason, SKIP_NO_MODELS);
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// R7-2：接口形态在配置协议里没有对应者（gemini 形态的自定义提供方）→
    /// 不写路由，上报 `no-protocol`。
    #[test]
    fn skip_custom_provider_without_config_protocol() {
        let (_lock, tmp) = sandbox("skip-noprotocol");
        let instance = instances::create_instance("协议测试", "web").unwrap();
        let custom = crate::providers::add_custom(
            "Gemini 中转",
            "https://relay.example.com/v1beta",
            crate::providers::ApiStyle::Gemini,
        )
        .unwrap();
        crate::keyvault::set_models(&custom.id, &["gemini-2.5-pro".into()]).unwrap();
        crate::keyvault::set_enabled(&custom.id, true).unwrap();

        sync_instance(&instance.id).unwrap();

        let providers = providers_of(&settings_yaml(&instance.id));
        assert!(!providers.contains_key(&custom.id), "写不出可用路由");
        let report = route_report(&instance.id).unwrap();
        assert_eq!(report.skipped.len(), 1, "{:?}", report.skipped);
        assert_eq!(report.skipped[0].reason, SKIP_NO_PROTOCOL);
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// R7-2：实例里已有 models（网页侧补的）→ 沿用而不是抹掉，并据此写出路由；
    /// 启动器清单与实例清单取并集，启动器条目的默认元数据优先。
    #[test]
    fn selfdescribed_route_adopts_existing_models() {
        let (_lock, tmp) = sandbox("adopt");
        let instance = instances::create_instance("沿用测试", "web").unwrap();
        crate::keyvault::set_enabled("glm", true).unwrap(); // 启动器侧无模型
        let dir = instances::instance_dir(&instance.id).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("settings.yaml"),
            "llm-pi-ai:\n  providers:\n    glm:\n      api: openai-completions\n      baseURL: https://open.bigmodel.cn/api/paas/v4\n      models:\n        - id: glm-4.6\n          name: glm-4.6\n          contextWindow: 131072\n",
        )
        .unwrap();

        sync_instance(&instance.id).unwrap();

        let providers = providers_of(&settings_yaml(&instance.id));
        let models = providers["glm"]["models"].as_array().unwrap();
        assert_eq!(models.len(), 1, "网页侧补的模型必须沿用：{models:?}");
        assert_eq!(models[0]["id"], "glm-4.6");
        assert_eq!(
            models[0]["contextWindow"], 131_072,
            "沿用条目保留原元数据，不被启动器默认覆盖"
        );
        assert!(
            route_report(&instance.id).unwrap().skipped.is_empty(),
            "有模型就不该再报跳过"
        );

        // 启动器侧也加了模型 → 并集（启动器条目在前、元数据按启动器默认）。
        crate::keyvault::set_models("glm", &["glm-4.5-air".into()]).unwrap();
        sync_instance(&instance.id).unwrap();
        let models = providers_of(&settings_yaml(&instance.id))["glm"]["models"]
            .as_array()
            .cloned()
            .unwrap();
        let ids: Vec<&str> = models.iter().filter_map(|m| m["id"].as_str()).collect();
        assert_eq!(ids, vec!["glm-4.5-air", "glm-4.6"]);
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// R7-1：提供方 id 查询——deepseek 走内建 adapter，目录厂商走目录 route key，
    /// 自描述厂商就是自己，gemini 形态的自定义提供方没有落地 id。
    #[test]
    fn route_key_of_maps_launcher_vendors() {
        let (_lock, tmp) = sandbox("route-key");
        assert_eq!(route_key_of("deepseek").unwrap().as_deref(), Some("deepseek-official"));
        assert_eq!(route_key_of("kimi").unwrap().as_deref(), Some("moonshotai-cn"));
        assert_eq!(route_key_of("gemini").unwrap().as_deref(), Some("google"));
        assert_eq!(route_key_of("xai").unwrap().as_deref(), Some("xai"));
        assert_eq!(route_key_of("glm").unwrap().as_deref(), Some("glm"));
        let custom = crate::providers::add_custom(
            "Gemini 中转",
            "https://relay.example.com/v1beta",
            crate::providers::ApiStyle::Gemini,
        )
        .unwrap();
        assert_eq!(route_key_of(&custom.id).unwrap(), None, "形态无协议 → 无落地 id");
        assert_eq!(route_key_of("nobody").unwrap(), None);
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// R7-2：xai 在安装版 pi-ai 目录里（真机 xai 路由零 models 仍解析出 3 个模型）
    /// → 必须按目录路由写，不写自描述 api/baseURL。
    #[test]
    fn catalog_route_needs_no_models_and_no_self_description() {
        let (_lock, tmp) = sandbox("xai-catalog");
        let instance = instances::create_instance("目录路由测试", "web").unwrap();
        crate::keyvault::set_enabled("xai", true).unwrap();

        sync_instance(&instance.id).unwrap();

        let providers = providers_of(&settings_yaml(&instance.id));
        let xai = providers.get("xai").expect("xai 应有路由");
        assert_eq!(xai["apiKeyEnv"], "XAI_API_KEY");
        assert!(xai.get("api").is_none(), "目录路由不写自描述 api");
        assert!(xai.get("baseURL").is_none(), "目录路由继承目录基址");
        assert!(xai.get("models").is_none(), "目录路由的模型目录归 pi-ai");
        assert!(route_report(&instance.id).unwrap().skipped.is_empty());
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// R7-2（Q7 前置）：启动器「添加自定义提供方」加进来的 `custom-*` 厂商必须
    /// 真的落地——路由写进实例，且 apiKeyEnv 与启动注入用的变量名是同一个
    /// （keyvault::env_name_of）。老门禁按 vendor_env_name 过滤，自定义厂商被
    /// 静默丢弃：路由、Key 两条链一起断。
    #[test]
    fn custom_provider_route_lands_with_matching_env_name() {
        let (_lock, tmp) = sandbox("custom-route");
        let instance = instances::create_instance("自定义路由测试", "web").unwrap();
        let custom = crate::providers::add_custom(
            "My Relay",
            "https://relay.example.com/v1",
            crate::providers::ApiStyle::Openai,
        )
        .unwrap();
        crate::keyvault::set_models(&custom.id, &["relay-large".into()]).unwrap();
        crate::keyvault::set_enabled(&custom.id, true).unwrap();

        sync_instance(&instance.id).unwrap();

        let providers = providers_of(&settings_yaml(&instance.id));
        let route = providers
            .get(&custom.id)
            .unwrap_or_else(|| panic!("自定义提供方应有路由：{:?}", providers.keys().collect::<Vec<_>>()));
        assert_eq!(route["api"], "openai-completions");
        assert_eq!(route["baseURL"], "https://relay.example.com/v1");
        assert_eq!(route["displayName"], "My Relay");
        assert_eq!(
            route["apiKeyEnv"],
            crate::keyvault::env_name_of(&custom.id).unwrap(),
            "路由 apiKeyEnv 必须等于启动注入的变量名"
        );
        assert_eq!(route["apiKeyEnv"], "CUSTOM_MY_RELAY_API_KEY");
        assert_eq!(route["models"][0]["id"], "relay-large");
        assert!(route_report(&instance.id).unwrap().skipped.is_empty());
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// R7-2：ollama 免 Key（不在 vendor_env_name 表里）——路由照写但不带
    /// apiKeyEnv，模型来自启动器清单，基址补 /v1。
    #[test]
    fn keyless_ollama_route_is_written_without_api_key_env() {
        let (_lock, tmp) = sandbox("ollama-route");
        let instance = instances::create_instance("Ollama 路由测试", "web").unwrap();
        crate::keyvault::set_models("ollama", &["qwen3:8b".into()]).unwrap();
        crate::keyvault::set_enabled("ollama", true).unwrap();

        sync_instance(&instance.id).unwrap();

        let providers = providers_of(&settings_yaml(&instance.id));
        let ollama = providers.get("ollama").expect("ollama 应有路由");
        assert!(ollama.get("apiKeyEnv").is_none(), "本地网关免 Key");
        assert_eq!(ollama["api"], "openai-completions");
        assert_eq!(ollama["baseURL"], "http://127.0.0.1:11434/v1");
        assert_eq!(ollama["models"][0]["id"], "qwen3:8b");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 目录漂移守卫（R7-2，真实安装版）：本机任一已装 DSH 的实例里，pi-ai 的
    /// `providers/` 目录必须覆盖 CATALOG_ROUTES 的每个 route key。缺一个就说明
    /// 目录变了而启动器还在按目录路由写——那会落一条零模型的自描述路由，DSH
    /// 侧 `resolves no models`（xai 曾经就是这个形状）。
    #[test]
    #[ignore = "真实安装版：需本机已有装好 DSH 的实例"]
    fn catalog_routes_exist_in_installed_pi_ai() {
        let dir = installed_pi_ai_providers().expect("本机需有装好 DSH 的实例（含 @earendil-works/pi-ai）");
        let files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        for (vendor, route) in CATALOG_ROUTES {
            assert!(
                files.iter().any(|f| f == &format!("{route}.js")),
                "启动器把「{vendor}」当目录路由（{route}），但安装版 pi-ai 里没有 {route}.js——\
                 目录漂移，应改成自描述路由（带 models）或更正 route key。目录：{}",
                dir.display()
            );
        }
    }

    /// 找本机任一实例内安装版 pi-ai 的 providers 目录（找不到返回 None）。
    fn installed_pi_ai_providers() -> Option<std::path::PathBuf> {
        for instance in instances::list_instances().ok()? {
            let app = instances::instance_dir(&instance.id).ok()?.join("app").join("node_modules");
            for rel in ["@earendil-works/pi-ai", "@deepseek-ai/pi-ai"] {
                let dir = app.join(rel).join("dist").join("providers");
                if dir.is_dir() {
                    return Some(dir);
                }
            }
        }
        None
    }
}
