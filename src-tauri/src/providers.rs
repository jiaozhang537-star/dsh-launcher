//! 模型提供方目录（FR-20，前置侦察 T-7 定稿）：内置 11 类提供方静态快照
//! + 自定义提供方 + 拖拽排序持久化。
//!
//! T-7 裁决：models.dev（MIT）字段不统一且缺 GLM/Kimi/Qwen/Ollama、
//! Cherry Studio 注册表硬编码于 GPL 应用内——故以 models.dev 与各官方文档
//! 交叉核对的结果落**仓库内静态快照**（`providers-catalog.json`，含出处与
//! 许可注记），不引入运行时依赖；图标用 simple-icons（CC0）slug，缺省前端
//! 字母块兜底。禁止从零硬编码的禁令以「交叉核对 + 注明出处」满足。
//!
//! 排序持久化（`<managed root>/providers.json`）：`order` 记录完整卡片顺序
//! （内置 id + 自定义 id，顺序即启动页下拉顺序）；`custom` 存自定义提供方
//! 定义（与内置同权，其 Key/模型/地址照旧存 keyvault，vendor id =
//! `custom-<slug>`）。

use serde::{Deserialize, Serialize};

use crate::paths;

/// 静态目录（编译期嵌入；出处与许可见文件内 attribution）。
const CATALOG_JSON: &str = include_str!("providers-catalog.json");

/// 模型列表接口形态（provider.rs 按 style 分派请求头/解析）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiStyle {
    /// OpenAI 兼容 `GET {base}/models`（Bearer）。
    Openai,
    /// OpenAI Responses API（模型列表与 OpenAI 兼容层相同：`GET {base}/models`
    /// + Bearer；`/responses` 对话形态由 DSH 侧消费，壳只负责拉模型清单）。
    Responses,
    /// Anthropic `GET {base}/models`（x-api-key + anthropic-version）。
    Anthropic,
    /// Gemini `GET {base}/models`（x-goog-api-key，名字带 `models/` 前缀）。
    Gemini,
    /// Ollama `GET {base}/api/tags`（无鉴权；Key 可空）。
    Ollama,
}

/// 内置提供方目录条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub label: String,
    /// 默认 API 地址（卡片可改，支持自建反代/中转）。
    pub base: String,
    pub style: ApiStyle,
    /// simple-icons slug；None = 前端字母块。
    pub icon: Option<String>,
    /// 目录分组：`curated` = 启动器精选（T-7 定稿 11 类），`dsh` = DSH 本体
    /// llm 层（pi-ai）原生实装的提供方（前端下拉按此分组展示）。
    #[serde(default = "default_group")]
    pub group: String,
}

fn default_group() -> String {
    "curated".to_string()
}

/// 自定义提供方定义（持久化部分；Key/模型在 keyvault）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProvider {
    /// 稳定 id：`custom-<slug>`（slug 由名称生成，撞名加序号）。
    pub id: String,
    pub name: String,
    pub base: String,
    pub style: ApiStyle,
}

/// 排序与自定义提供方持久化根。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersLayout {
    #[serde(default)]
    pub order: Vec<String>,
    #[serde(default)]
    pub custom: Vec<CustomProvider>,
}

/// 给前端的目录视图：目录条目 + 自定义条目 + 排序（前端按 order 渲染）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersView {
    pub catalog: Vec<CatalogEntry>,
    pub custom: Vec<CustomProvider>,
    pub order: Vec<String>,
}

/// 解析静态目录；结构损坏是编译期产物损坏，fail loud。
pub fn catalog() -> Vec<CatalogEntry> {
    let value: serde_json::Value = serde_json::from_str(CATALOG_JSON)
        .expect("providers-catalog.json 必须是合法 JSON（编译期内嵌产物）");
    serde_json::from_value(value.get("providers").cloned().unwrap_or_default())
        .expect("providers-catalog.json 缺 providers 数组")
}

/// 按 id 查目录条目。
pub fn catalog_entry(id: &str) -> Option<CatalogEntry> {
    catalog().into_iter().find(|e| e.id == id)
}

/// 自定义提供方 id 的合法性（keyvault 白名单同口径）。
pub fn is_custom_id(vendor: &str) -> bool {
    vendor.starts_with("custom-")
        && vendor.len() > "custom-".len()
        && vendor["custom-".len()..]
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// 持久化文件：`<managed root>/providers.json`。
fn layout_file() -> Result<std::path::PathBuf, String> {
    Ok(paths::managed_root()?.join("providers.json"))
}

/// 读布局；缺失/损坏按默认空（损坏时清掉坏文件，与市场缓存同语义）。
pub fn load_layout() -> Result<ProvidersLayout, String> {
    let path = layout_file()?;
    if !path.exists() {
        return Ok(ProvidersLayout::default());
    }
    let text = crate::paths::read_text(&path).map_err(|e| format!("读取提供方布局失败：{e}"))?;
    match serde_json::from_str(&text) {
        Ok(layout) => Ok(layout),
        Err(_) => {
            std::fs::remove_file(&path).ok();
            Ok(ProvidersLayout::default())
        }
    }
}

/// 保存布局（临时文件 + 改名原子替换）。
pub fn save_layout(layout: &ProvidersLayout) -> Result<(), String> {
    let path = layout_file()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
    }
    let text = serde_json::to_string(layout).map_err(|e| format!("序列化失败：{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写入失败：{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换失败：{e}"))
}

/// 目录视图（排序 = 已存 order ∪ 目录缺省顺序 ∪ 自定义追加）。
pub fn view() -> Result<ProvidersView, String> {
    let layout = load_layout()?;
    Ok(ProvidersView {
        catalog: catalog(),
        custom: layout.custom.clone(),
        order: layout.order.clone(),
    })
}

/// 从名称生成自定义 id：小写化、非字母数字折叠为 `-`、裁短；与现有 id 撞名
/// 时加 `-2/-3` 后缀（与实例重名同语义）。
pub fn custom_id_for(name: &str, existing: &[String]) -> String {
    let base = format!(
        "custom-{}",
        name.trim()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_string()
    );
    let base = if base == "custom-" { "custom-provider".to_string() } else { base };
    let mut candidate = base.clone();
    let mut n = 2;
    while existing.iter().any(|e| e == &candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

/// 新增自定义提供方（id 自动生成；重名拒绝）。
pub fn add_custom(name: &str, base: &str, style: ApiStyle) -> Result<CustomProvider, String> {
    let name = name.trim();
    let base = base.trim().trim_end_matches('/');
    if name.is_empty() {
        return Err("自定义提供方名称不能为空".into());
    }
    if base.is_empty() || !base.starts_with("http://") && !base.starts_with("https://") {
        return Err("自定义提供方地址必须是 http(s) URL".into());
    }
    let mut layout = load_layout()?;
    if layout.custom.iter().any(|c| c.name == name) {
        return Err(format!("自定义提供方 {name:?} 已存在"));
    }
    let ids: Vec<String> = layout
        .custom
        .iter()
        .map(|c| c.id.clone())
        .chain(layout.order.iter().cloned())
        .collect();
    let provider = CustomProvider {
        id: custom_id_for(name, &ids),
        name: name.to_string(),
        base: base.to_string(),
        style,
    };
    layout.custom.push(provider.clone());
    layout.order.push(provider.id.clone());
    save_layout(&layout)?;
    Ok(provider)
}

/// 删除自定义提供方（内置目录条目拒绝）。连带清除该厂商在保险库的记录
/// （Key/模型/启用态）——先清保险库再删布局：前者失败则整体未动，后者
/// 失败时重试幂等。
pub fn remove_custom(id: &str) -> Result<(), String> {
    if !is_custom_id(id) {
        return Err(format!("{id:?} 不是自定义提供方（内置目录不可删）"));
    }
    {
        let mut layout = load_layout()?;
        let before = layout.custom.len();
        layout.custom.retain(|c| c.id != id);
        layout.order.retain(|o| o != id);
        if layout.custom.len() == before {
            return Err(format!("自定义提供方 {id:?} 不存在"));
        }
        save_layout(&layout)?;
    }
    crate::keyvault::forget_vendor(id)
}

/// 保存拖拽排序：必须恰好覆盖全部已知 id（内置 ∪ 自定义），多/少/重复都拒绝
/// ——半份排序会让卡片渲染口径漂移。
pub fn set_order(order: &[String]) -> Result<(), String> {
    let layout = load_layout()?;
    let mut known: Vec<String> = catalog().into_iter().map(|e| e.id).collect();
    known.extend(layout.custom.iter().map(|c| c.id.clone()));
    let mut seen = std::collections::BTreeSet::new();
    for id in order {
        if !known.contains(id) {
            return Err(format!("未知提供方 id：{id:?}"));
        }
        if !seen.insert(id.clone()) {
            return Err(format!("排序里有重复 id：{id:?}"));
        }
    }
    if seen.len() != known.len() {
        return Err(format!(
            "排序必须覆盖全部 {} 个提供方，实际 {} 个",
            known.len(),
            seen.len()
        ));
    }
    save_layout(&ProvidersLayout {
        order: order.to_vec(),
        custom: layout.custom.clone(),
    })
}

/// 查提供方的 (默认地址, 接口形态)：内置目录优先，其次自定义定义。
pub fn resolve_style_and_base(vendor: &str) -> Result<(ApiStyle, String), String> {
    if let Some(entry) = catalog_entry(vendor) {
        return Ok((entry.style, entry.base));
    }
    let layout = load_layout()?;
    if let Some(custom) = layout.custom.iter().find(|c| c.id == vendor) {
        return Ok((custom.style, custom.base.clone()));
    }
    Err(format!("未知提供方：{vendor:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        crate::instances::tests::sandbox(&format!("providers-{tag}"))
    }

    #[test]
    fn catalog_has_11_curated_plus_dsh_native_entries_with_unique_ids() {
        let catalog = catalog();
        assert_eq!(
            catalog.iter().filter(|e| e.group == "curated").count(),
            11,
            "FR-20：11 精选内置"
        );
        assert!(
            catalog.iter().filter(|e| e.group == "dsh").count() >= 17,
            "DSH 内置（pi-ai 原生实装）预设必须成组在列"
        );
        let mut ids: Vec<&str> = catalog.iter().map(|e| e.id.as_str()).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "id 不得重复");
        for required in [
            "deepseek", "glm", "kimi", "qwen", "openai", "anthropic", "xai", "gemini",
            "siliconflow", "openrouter", "ollama",
        ] {
            assert!(catalog.iter().any(|e| e.id == required), "缺 {required}");
        }
        // DSH 内置组抽样：base/env 全部对齐 pi-ai 实装（R5b 扩编）。
        for (id, base) in [
            ("groq", "https://api.groq.com/openai/v1"),
            ("minimax", "https://api.minimax.io/anthropic"),
            ("kimi-coding", "https://api.kimi.com/coding"),
            ("zai", "https://api.z.ai/api/coding/paas/v4"),
        ] {
            let entry = catalog_entry(id).unwrap_or_else(|| panic!("缺 DSH 内置 {id}"));
            assert_eq!(entry.group, "dsh");
            assert_eq!(entry.base, base);
        }
        let ollama = catalog_entry("ollama").unwrap();
        assert_eq!(ollama.style, ApiStyle::Ollama);
        assert_eq!(ollama.base, "http://127.0.0.1:11434");
        let anthropic = catalog_entry("anthropic").unwrap();
        assert_eq!(anthropic.style, ApiStyle::Anthropic);
        let gemini = catalog_entry("gemini").unwrap();
        assert_eq!(gemini.style, ApiStyle::Gemini);
    }

    #[test]
    fn custom_id_slug_and_collision_suffix() {
        assert_eq!(custom_id_for("My Relay", &[]), "custom-my-relay");
        assert_eq!(custom_id_for("中转", &[]), "custom-provider", "全中文折叠为空 → 兜底名");
        assert_eq!(custom_id_for("relay", &["custom-relay".into()]), "custom-relay-2");
    }

    #[test]
    fn custom_id_validation() {
        assert!(is_custom_id("custom-my-relay"));
        assert!(is_custom_id("custom-a1"));
        assert!(!is_custom_id("custom-"));
        assert!(!is_custom_id("custom-UPPER"));
        assert!(!is_custom_id("deepseek"));
        assert!(!is_custom_id("custom-x/y"));
    }

    #[test]
    fn add_remove_custom_roundtrip_with_order() {
        let (_lock, tmp) = sandbox("custom");
        let added = add_custom("My Relay", "https://relay.example.com/v1/", ApiStyle::Openai).unwrap();
        assert_eq!(added.id, "custom-my-relay");
        assert!(added.base.ends_with("/v1"), "尾斜杠裁掉：{}", added.base);

        let v = view().unwrap();
        assert_eq!(v.custom.len(), 1);
        assert_eq!(v.order, vec!["custom-my-relay"]);

        // 撞名拒绝；地址协议校验。
        assert!(add_custom("My Relay", "https://x.example.com", ApiStyle::Openai).is_err());
        assert!(add_custom("Another", "ftp://x", ApiStyle::Openai).is_err());

        // 内置 id 拒绝删除；自定义可删。
        assert!(remove_custom("deepseek").is_err());
        remove_custom(&added.id).unwrap();
        assert!(view().unwrap().custom.is_empty());
        assert!(view().unwrap().order.is_empty());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn set_order_requires_full_cover_no_dup() {
        let (_lock, tmp) = sandbox("order");
        add_custom("R1", "https://r1.example.com", ApiStyle::Openai).unwrap();
        let all: Vec<String> = catalog()
            .into_iter()
            .map(|e| e.id)
            .chain(std::iter::once("custom-r1".to_string()))
            .collect();
        // 缺一个 → 拒绝。
        assert!(set_order(&all[1..]).is_err());
        // 重复 → 拒绝。
        let mut dup = all.clone();
        dup.push(all[0].clone());
        assert!(set_order(&dup).is_err());
        // 未知 id → 拒绝。
        let mut unknown = all.clone();
        unknown.push("custom-ghost".into());
        assert!(set_order(&unknown).is_err());
        // 全量合法 → 保存。
        let mut reordered = all.clone();
        reordered.reverse();
        set_order(&reordered).unwrap();
        assert_eq!(load_layout().unwrap().order, reordered);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_style_covers_builtin_and_custom() {
        let (_lock, tmp) = sandbox("resolve");
        let (style, base) = resolve_style_and_base("deepseek").unwrap();
        assert_eq!(style, ApiStyle::Openai);
        assert_eq!(base, "https://api.deepseek.com");
        let added = add_custom("Relay", "https://relay.example.com", ApiStyle::Gemini).unwrap();
        let (style, base) = resolve_style_and_base(&added.id).unwrap();
        assert_eq!(style, ApiStyle::Gemini);
        assert_eq!(base, "https://relay.example.com");
        assert!(resolve_style_and_base("custom-ghost").is_err());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
