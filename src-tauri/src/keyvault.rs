//! API Key 保险库（FR-06）：多厂商启用制 + Windows DPAPI 加密落盘。
//!
//! 数据模型：`Vault { vendors: [VaultVendor] }`，每个厂商一条记录——
//! 启用开关、可选密文（DPAPI 后 base64）、已添加模型 id 列表。启用与
//! 是否已填 Key 解耦：无 Key 也可启用（启动前由校验链按「任一启用厂商
//! 可得 Key」裁决），启动页补填写回保险库。
//!
//! 明文 Key 只存在于两处：调用方内存与拉起实例时的进程环境变量
//! （R-2.3 裁决：credentials-local 以进程环境为最高优先级通道），
//! 绝不写入任何文件。密文绑定当前 Windows 用户（DPAPI 用户作用域）。
//!
//! 安全边界：Tauri command 只暴露 status/set_enabled/set_key/set_models/
//! clear_key/forget_vendor——没有任何命令会把明文或密文读回前端；解密入口
//! 仅供启动器内部的进程拉起使用。

use serde::{Deserialize, Serialize};

use crate::paths;

/// 保险库文件名（位于托管根下）。
const KEYVAULT_FILE: &str = "keyvault.json";

/// 已知厂商目录（内置 11 类）以 providers-catalog 为唯一事实来源；本模块
/// 只保留两个历史常量与 GitHub Token 通道常量。
pub const VENDOR_DEEPSEEK: &str = "deepseek";
pub const VENDOR_OPENAI: &str = "openai";

/// GitHub 个人访问令牌（市场/Skill 聚合 API 提额用，FR-03）。复用保险库
/// DPAPI 存储与读写链，但**不是模型厂商**：status 视图与启动注入一律跳过，
/// 前端经由 VaultStatus 的 githubToken 字段感知其存在。
pub const VENDOR_GITHUB: &str = "github";

/// DeepSeek 默认模型目录（镜像 dsh-llm-deepseek 的 DEFAULT_MODELS id）；
/// 仅作设置页默认勾选项，用户可添加任意非空模型 id。
pub const DEEPSEEK_KNOWN_MODELS: &[&str] = &["deepseek-v4-flash", "deepseek-v4-pro"];

/// 厂商 → 注入环境变量名映射（进程环境是 credentials-local 的最高优先
/// 通道）。取值 = pi-ai 上游 provider 目录的规范名（best-effort：DSH 侧
/// 路由可经 apiKeyEnv 自定其它 ref，届时还有 `$DSH_HOME/.credentials.yaml`
/// 原生凭据通道兜底）；ollama 为本地网关免 Key，不设环境变量。
pub fn vendor_env_name(vendor: &str) -> Option<&'static str> {
    match vendor {
        VENDOR_DEEPSEEK => Some("DEEPSEEK_API_KEY"),
        VENDOR_OPENAI => Some("OPENAI_API_KEY"),
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "gemini" => Some("GEMINI_API_KEY"),
        "glm" => Some("ZAI_API_KEY"),
        "kimi" | "moonshotai" => Some("MOONSHOT_API_KEY"),
        "qwen" => Some("DASHSCOPE_API_KEY"),
        "qwen-token-plan" => Some("QWEN_TOKEN_PLAN_API_KEY"),
        "qwen-token-plan-cn" => Some("QWEN_TOKEN_PLAN_CN_API_KEY"),
        "xai" => Some("XAI_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "siliconflow" => Some("SILICONFLOW_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "cerebras" => Some("CEREBRAS_API_KEY"),
        "together" => Some("TOGETHER_API_KEY"),
        "nvidia" => Some("NVIDIA_API_KEY"),
        "huggingface" => Some("HF_TOKEN"),
        "ant-ling" => Some("ANT_LING_API_KEY"),
        "zai" => Some("ZAI_API_KEY"),
        "zai-coding-cn" => Some("ZAI_CODING_CN_API_KEY"),
        "xiaomi" => Some("XIAOMI_API_KEY"),
        "minimax" => Some("MINIMAX_API_KEY"),
        "minimax-cn" => Some("MINIMAX_CN_API_KEY"),
        "kimi-coding" => Some("KIMI_API_KEY"),
        "fireworks" => Some("FIREWORKS_API_KEY"),
        "vercel-ai-gateway" => Some("AI_GATEWAY_API_KEY"),
        VENDOR_GITHUB => Some("GH_TOKEN"),
        _ => None,
    }
}

/// 自定义提供方（id = `custom-<slug>`）的注入环境变量名：`CUSTOM_<SLUG>_API_KEY`。
/// 非字母数字折叠成 `_`。名字由本函数单点决定：进程注入（process.rs）与路由
/// `apiKeyEnv`（llm_bridge）必须取同一个名字，否则 Key 到不了 DSH。
pub fn custom_env_name(vendor: &str) -> Option<String> {
    let slug = vendor.strip_prefix("custom-")?;
    if slug.is_empty() {
        return None;
    }
    let mut name = String::from("CUSTOM_");
    for ch in slug.chars() {
        if ch.is_ascii_alphanumeric() {
            name.push(ch.to_ascii_uppercase());
        } else {
            name.push('_');
        }
    }
    name.push_str("_API_KEY");
    Some(name)
}

/// 厂商的注入环境变量名：目录厂商用 pi-ai 规范名，自定义提供方按 id 派生。
/// None = 该厂商不需要 Key（ollama 本地网关）——路由照写，只是不带 apiKeyEnv。
pub fn env_name_of(vendor: &str) -> Option<String> {
    if let Some(name) = vendor_env_name(vendor) {
        return Some(name.to_string());
    }
    if vendor == "ollama" {
        return None;
    }
    custom_env_name(vendor)
}

/// 单厂商保险库条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultVendor {
    /// 厂商 id（目录内）。
    pub vendor: String,
    /// 设置页勾选启用；启用与是否有 Key 解耦。
    #[serde(default)]
    pub enabled: bool,
    /// DPAPI 密文（base64）。None = 尚未填写 Key。
    pub ciphertext_b64: Option<String>,
    /// 已添加的模型 id 列表（任意非空去重字符串）。
    #[serde(default)]
    pub models: Vec<String>,
    /// 提供方 API 地址覆盖；None = 使用该厂商默认地址。
    #[serde(default)]
    pub address: Option<String>,
}

/// 保险库持久化根结构。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Vault {
    #[serde(default)]
    pub vendors: Vec<VaultVendor>,
}

/// 给前端的单厂商只读视图（不含任何密钥材料；IPC 键为 camelCase，与
/// 前端 `VendorKeyStatus.hasKey/envAuto` 对齐。持久化的 VaultVendor 不受影响）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VendorStatus {
    pub vendor: String,
    pub enabled: bool,
    /// 保险库里已有 Key。
    pub has_key: bool,
    /// 环境变量里已有 Key（自动配置通道）。
    pub env_auto: bool,
    pub models: Vec<String>,
    /// API 地址覆盖；None = 默认地址。
    pub address: Option<String>,
}

/// 保险库状态（给前端的只读视图）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub vendors: Vec<VendorStatus>,
    /// 保险库里已存 GitHub Token（市场聚合提额用）。
    pub github_token_configured: bool,
    /// 环境变量 GH_TOKEN / GITHUB_TOKEN 里已有 Token（自动配置通道）。
    pub github_token_env_auto: bool,
}

#[cfg(windows)]
mod dpapi {
    //! CryptProtectData/CryptUnprotectData 的最小封装。
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    /// DPAPI 加密（用户作用域）；失败返回错误摘要。
    pub fn protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
        if plaintext.is_empty() {
            return Err("明文为空，拒绝加密空串".into());
        }
        let mut blob_in = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_ptr() as *mut u8,
        };
        let mut blob_out = CRYPT_INTEGER_BLOB::default();
        // SAFETY：blob_in 指向调用方持有的有效缓冲区；输出缓冲区由系统分配，
        // 成功后立即拷贝并 LocalFree。UI_FORBIDDEN 保证无窗口弹窗。
        let hr = unsafe {
            CryptProtectData(
                &mut blob_in,
                PWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut blob_out,
            )
        };
        if let Err(e) = hr {
            return Err(format!("CryptProtectData 失败：{e}"));
        }
        let slice =
            unsafe { std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize) };
        let out = slice.to_vec();
        unsafe { LocalFree(HLOCAL(blob_out.pbData.cast())) };
        Ok(out)
    }

    /// DPAPI 解密；当前用户之外加密的数据会失败（预期行为）。
    pub fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        let mut blob_in = CRYPT_INTEGER_BLOB {
            cbData: ciphertext.len() as u32,
            pbData: ciphertext.as_ptr() as *mut u8,
        };
        let mut blob_out = CRYPT_INTEGER_BLOB::default();
        // SAFETY：同 protect；输出缓冲区由系统分配，拷贝后 LocalFree。
        let hr = unsafe {
            CryptUnprotectData(
                &mut blob_in,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut blob_out,
            )
        };
        if let Err(e) = hr {
            return Err(format!("CryptUnprotectData 失败（{e}）：密文可能由其他用户账户加密"));
        }
        let slice =
            unsafe { std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize) };
        let out = slice.to_vec();
        unsafe { LocalFree(HLOCAL(blob_out.pbData.cast())) };
        Ok(out)
    }
}

/// 非 Windows 平台的编译兜底：一期只交付 Windows 包，接口保持可移植签名。
#[cfg(not(windows))]
mod dpapi {
    pub fn protect(_plaintext: &[u8]) -> Result<Vec<u8>, String> {
        Err("DPAPI 仅在 Windows 可用".into())
    }
    pub fn unprotect(_ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        Err("DPAPI 仅在 Windows 可用".into())
    }
}

fn vault_file() -> Result<std::path::PathBuf, String> {
    Ok(paths::managed_root()?.join(KEYVAULT_FILE))
}

/// 旧版单条格式（Phase 2 早批）迁移为新 `Vault`：整文件只有一条厂商记录
/// 视为旧格式，启用状态置真。
fn migrate(raw: &str) -> Result<Vault, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("解析保险库失败：{e}"))?;
    if value.get("vendors").is_some() {
        let vault: Vault =
            serde_json::from_value(value).map_err(|e| format!("解析保险库失败：{e}"))?;
        return Ok(vault);
    }
    if let Ok(entry) = serde_json::from_value::<VaultVendor>(value) {
        return Ok(Vault {
            vendors: vec![VaultVendor { enabled: true, ..entry }],
        });
    }
    Err("保险库文件格式无法识别".into())
}

/// 读保险库；文件不存在返回默认视图（deepseek 启用 + 内置模型 + 无 Key，
/// 纯视图不落盘——零配置开箱）。
fn load_vault() -> Result<Vault, String> {
    let file = vault_file()?;
    if !file.exists() {
        return Ok(default_vault());
    }
    let raw = paths::read_text(&file)?;
    migrate(&raw)
}

/// 原子写保险库。
fn save_vault(vault: &Vault) -> Result<(), String> {
    let file = vault_file()?;
    paths::ensure_managed_dirs()?;
    let body = serde_json::to_string_pretty(vault).map_err(|e| format!("序列化失败：{e}"))?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, body + "\n").map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("替换 {} 失败：{e}", file.display()))?;
    Ok(())
}

/// 默认视图：deepseek 启用 + 内置模型 + 无 Key。
fn default_vault() -> Vault {
    Vault {
        vendors: vec![VaultVendor {
            vendor: VENDOR_DEEPSEEK.into(),
            enabled: true,
            ciphertext_b64: None,
            models: DEEPSEEK_KNOWN_MODELS.iter().map(|s| s.to_string()).collect(),
            address: None,
        }],
    }
}

/// 厂商合法性：GitHub Token 通道 ∪ 自定义提供方 ∪ 内置目录全部 id——
/// 与 providers-catalog 同口径（R5 修复：此前只放行 deepseek/openai，
/// 目录里其余厂商的启用/存 Key/模型/地址全被误拒，前端勾选必然回弹）。
fn valid_vendor(vendor: &str) -> bool {
    vendor == VENDOR_GITHUB
        || crate::providers::is_custom_id(vendor)
        || crate::providers::catalog_entry(vendor).is_some()
}

/// 取厂商条目，不存在则按默认建档（deepseek 预填内置模型，其余空）。
fn entry_or_default<'a>(vault: &'a mut Vault, vendor: &str) -> &'a mut VaultVendor {
    if let Some(idx) = vault.vendors.iter().position(|v| v.vendor == vendor) {
        return &mut vault.vendors[idx];
    }
    vault.vendors.push(VaultVendor {
        vendor: vendor.to_string(),
        enabled: false,
        ciphertext_b64: None,
        models: if vendor == VENDOR_DEEPSEEK {
            DEEPSEEK_KNOWN_MODELS.iter().map(|s| s.to_string()).collect()
        } else {
            Vec::new()
        },
        address: None,
    });
    vault.vendors.last_mut().expect("刚 push 的条目必然存在")
}

/// 归一化模型 id 列表：trim、去空、按首次出现顺序去重。
fn normalize_models(models: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for m in models {
        let trimmed = m.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// 勾选/取消启用厂商（与 Key 无关；档位即时影响启动页候选与校验链）。
pub fn set_enabled(vendor: &str, enabled: bool) -> Result<(), String> {
    if !valid_vendor(vendor) {
        return Err(format!("不支持的模型厂商：{vendor:?}"));
    }
    let mut vault = load_vault()?;
    entry_or_default(&mut vault, vendor).enabled = enabled;
    save_vault(&vault)
}

/// 保存 API Key：加密后按厂商落盘（保留该厂商的启用状态与模型列表）。
/// 明文参数只在本函数栈上存在。
pub fn set_key(vendor: &str, key: &str) -> Result<(), String> {
    if !valid_vendor(vendor) {
        return Err(format!("不支持的模型厂商：{vendor:?}"));
    }
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err("API Key 不能为空白".into());
    }
    let ciphertext = dpapi::protect(trimmed.as_bytes())?;
    let mut vault = load_vault()?;
    entry_or_default(&mut vault, vendor).ciphertext_b64 = Some(base64_encode(&ciphertext));
    save_vault(&vault)
}

/// 覆盖厂商的已添加模型列表（任意非空去重 id；写入不改 Key 与启用状态）。
pub fn set_models(vendor: &str, models: &[String]) -> Result<(), String> {
    if !valid_vendor(vendor) {
        return Err(format!("不支持的模型厂商：{vendor:?}"));
    }
    let mut vault = load_vault()?;
    entry_or_default(&mut vault, vendor).models = normalize_models(models);
    save_vault(&vault)
}

/// 保存提供方 API 地址覆盖；空串清除覆盖（回落默认地址）。
pub fn set_address(vendor: &str, address: &str) -> Result<(), String> {
    if !valid_vendor(vendor) {
        return Err(format!("不支持的模型厂商：{vendor:?}"));
    }
    let trimmed = address.trim();
    let mut vault = load_vault()?;
    entry_or_default(&mut vault, vendor).address =
        (!trimmed.is_empty()).then(|| trimmed.to_string());
    save_vault(&vault)
}

/// 清除厂商已存 Key（保留启用状态与模型列表；无 Key 时幂等成功）。
pub fn clear_key(vendor: &str) -> Result<(), String> {
    if !valid_vendor(vendor) {
        return Err(format!("不支持的模型厂商：{vendor:?}"));
    }
    let mut vault = load_vault()?;
    if let Some(entry) = vault.vendors.iter_mut().find(|v| v.vendor == vendor) {
        if entry.ciphertext_b64.take().is_none() {
            return Ok(());
        }
    } else {
        return Ok(());
    }
    save_vault(&vault)
}

/// 把厂商从保险库整条移除（启用态/Key/模型列表/地址一并清除）——
/// 「按需添加」列表的移除入口（R5）；幂等（无记录即成功）。
/// GitHub Token 不属于模型厂商列表，拒绝。
pub fn forget_vendor(vendor: &str) -> Result<(), String> {
    if vendor == VENDOR_GITHUB {
        return Err("GitHub Token 不在模型厂商列表内".into());
    }
    if !valid_vendor(vendor) {
        return Err(format!("不支持的模型厂商：{vendor:?}"));
    }
    let mut vault = load_vault()?;
    let before = vault.vendors.len();
    vault.vendors.retain(|v| v.vendor != vendor);
    if vault.vendors.len() == before {
        return Ok(());
    }
    save_vault(&vault)
}

/// GitHub Token 的环境变量自动配置探测（GH_TOKEN / GITHUB_TOKEN 任一非空）。
fn github_token_env_auto() -> bool {
    ["GH_TOKEN", "GITHUB_TOKEN"].iter().any(|name| {
        std::env::var(name).map(|v| !v.trim().is_empty()).unwrap_or(false)
    })
}

/// 只读状态视图（无密钥材料）；环境变量探测在本函数完成，前端不做二套判断。
/// GitHub Token 不是模型厂商——不出现在 vendors 里，只以两个布尔位呈现。
pub fn status() -> Result<VaultStatus, String> {
    let vault = load_vault()?;
    let mut github_token_configured = false;
    let vendors = vault
        .vendors
        .iter()
        .filter(|entry| {
            if entry.vendor == VENDOR_GITHUB {
                github_token_configured |= entry.ciphertext_b64.is_some();
                return false;
            }
            true
        })
        .map(|entry| VendorStatus {
            has_key: entry.ciphertext_b64.is_some(),
            env_auto: vendor_env_name(&entry.vendor)
                .map(|name| std::env::var(name).map(|v| !v.trim().is_empty()).unwrap_or(false))
                .unwrap_or(false),
            vendor: entry.vendor.clone(),
            enabled: entry.enabled,
            models: entry.models.clone(),
            address: entry.address.clone(),
        })
        .collect();
    Ok(VaultStatus {
        vendors,
        github_token_configured,
        github_token_env_auto: github_token_env_auto(),
    })
}

/// 枚举「已启用且已有 Key」厂商的 (厂商, 明文 Key)——拉起实例时逐条注入
/// 各自的环境变量。明文只在返回值中存在，调用方不得落盘。
pub fn reveal_enabled_keys() -> Result<Vec<(String, String)>, String> {
    let vault = load_vault()?;
    let mut out = Vec::new();
    for entry in &vault.vendors {
        if !entry.enabled || entry.vendor == VENDOR_GITHUB {
            continue;
        }
        let Some(ciphertext) = entry.ciphertext_b64.as_deref() else {
            continue;
        };
        let plaintext = base64_decode(ciphertext)
            .ok_or_else(|| "保险库密文不是合法 base64".to_string())?;
        let key = dpapi::unprotect(&plaintext).and_then(|bytes| {
            String::from_utf8(bytes).map_err(|_| "解密后的 Key 不是合法 UTF-8".into())
        })?;
        out.push((entry.vendor.clone(), key));
    }
    Ok(out)
}

/// 厂商的 API 地址覆盖（None = 使用默认地址）。launch 注入与路由桥接消费。
pub fn address_of(vendor: &str) -> Result<Option<String>, String> {
    let vault = load_vault()?;
    Ok(vault
        .vendors
        .iter()
        .find(|v| v.vendor == vendor)
        .and_then(|v| v.address.clone()))
}

/// 解密取出指定厂商的 Key——供模型列表拉取等需要主动调用的壳侧功能使用，
/// 明文只在返回值中存在。
pub fn reveal_key_for(vendor: &str) -> Result<String, String> {
    let vault = load_vault()?;
    let entry = vault
        .vendors
        .iter()
        .find(|v| v.vendor == vendor)
        .ok_or_else(|| format!("保险库中没有厂商 {vendor:?} 的记录"))?;
    let ciphertext = entry
        .ciphertext_b64
        .as_deref()
        .ok_or_else(|| format!("厂商 {vendor:?} 尚未配置 API Key"))?;
    let plaintext = base64_decode(ciphertext)
        .ok_or_else(|| "保险库密文不是合法 base64".to_string())?;
    dpapi::unprotect(&plaintext).and_then(|bytes| {
        String::from_utf8(bytes).map_err(|_| "解密后的 Key 不是合法 UTF-8".into())
    })
}

/// 标准 base64 编码（无依赖实现，字母表 A–Z a–z 0–9 + /，`=` 补位）。
pub fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18 & 63) as usize] as char);
        out.push(TABLE[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// 标准 base64 解码；非法输入返回 None（空串是合法输入，产出空向量）。
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for group in bytes.chunks(4) {
        let pad = group.iter().filter(|&&c| c == b'=').count();
        if pad > 2 {
            return None;
        }
        if group[..4 - pad].iter().any(|&c| value(c).is_none()) {
            return None;
        }
        if pad > 0 && group[4 - pad..].iter().any(|&c| c != b'=') {
            return None; // = 只允许出现在尾部
        }
        let n = (value(group[0])? << 18)
            | (value(group[1])? << 12)
            | (if pad < 2 { value(group[2])? } else { 0 } << 6)
            | if pad < 1 { value(group[3])? } else { 0 };
        out.push((n >> 16) as u8);
        // 每个尾部 = 少还原一个字节：pad=2 → 1 字节，pad=1 → 2 字节。
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances;

    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        instances::tests::sandbox(&format!("vault-{tag}"))
    }

    fn vault_path(tmp: &std::path::Path) -> std::path::PathBuf {
        tmp.join("dsh-launcher").join("keyvault.json")
    }

    #[test]
    fn base64_roundtrip_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        for sample in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            let encoded = base64_encode(sample.as_bytes());
            assert_eq!(base64_decode(&encoded).as_deref(), Some(sample.as_bytes()));
        }
        assert_eq!(base64_decode("Zm9v"), Some(b"foo".to_vec()));
        assert_eq!(base64_decode("A"), None, "长度非 4 的倍数");
        assert_eq!(base64_decode("Zn=v"), None, "非法字符");
        assert_eq!(base64_decode("Zm9=v"), None, "= 后还有数据");
    }

    #[test]
    fn status_view_defaults_to_enabled_deepseek_without_disk() {
        let (_lock, tmp) = sandbox("default-view");
        let st = status().unwrap();
        assert_eq!(st.vendors.len(), 1);
        let ds = &st.vendors[0];
        assert_eq!(ds.vendor, VENDOR_DEEPSEEK);
        assert!(ds.enabled);
        assert!(!ds.has_key);
        assert_eq!(ds.models, DEEPSEEK_KNOWN_MODELS);
        assert!(!vault_path(&tmp).exists(), "默认视图不得落盘");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// IPC 键名必须是 camelCase：前端按 hasKey/envAuto 读状态，曾因漏
    /// rename 让「保险库有 Key」被渲染成「无 Key（可稍后补）」。
    #[test]
    fn vendor_status_serializes_camel_case_keys() {
        let view = VendorStatus {
            vendor: VENDOR_DEEPSEEK.into(),
            enabled: true,
            has_key: true,
            env_auto: false,
            models: vec!["deepseek-v4-flash".into()],
            address: None,
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["hasKey"], true);
        assert_eq!(json["envAuto"], false);
        let raw = serde_json::to_string(&view).unwrap();
        assert!(!raw.contains("has_key"), "禁止泄漏 snake_case 键：{raw}");
    }

    #[test]
    fn enable_without_key_is_persisted_and_visible() {
        let (_lock, tmp) = sandbox("enable-nokey");
        set_enabled(VENDOR_DEEPSEEK, true).unwrap();
        let st = status().unwrap();
        let ds = st.vendors.iter().find(|v| v.vendor == VENDOR_DEEPSEEK).unwrap();
        assert!(ds.enabled);
        assert!(!ds.has_key, "启用不要求 Key");
        assert!(vault_path(&tmp).exists());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn key_models_and_clear_are_independent_per_vendor() {
        let (_lock, tmp) = sandbox("multi");
        set_enabled(VENDOR_DEEPSEEK, true).unwrap();
        set_key(VENDOR_DEEPSEEK, " sk-test-1234567890abcdefSECRET ").unwrap();
        set_models(VENDOR_DEEPSEEK, &[
            "deepseek-v4-pro".into(),
            " custom-model-x ".into(),
            "deepseek-v4-pro".into(),
            "".into(),
        ])
        .unwrap();

        // 磁盘字节绝不含明文。
        let disk = std::fs::read_to_string(vault_path(&tmp)).unwrap();
        assert!(!disk.contains("sk-test-1234567890abcdefSECRET"), "明文泄漏到磁盘");

        let st = status().unwrap();
        let ds = st.vendors.iter().find(|v| v.vendor == VENDOR_DEEPSEEK).unwrap();
        assert!(ds.has_key);
        // trim + 去重 + 去空：内置顺序保留，自定义追加。
        assert_eq!(ds.models, vec!["deepseek-v4-pro".to_string(), "custom-model-x".to_string()]);

        assert_eq!(
            reveal_enabled_keys()
                .unwrap()
                .into_iter()
                .find(|(v, _)| v == VENDOR_DEEPSEEK)
                .map(|(_, key)| key)
                .unwrap(),
            "sk-test-1234567890abcdefSECRET"
        );

        // 清 Key 只清密文，启用与模型保留。
        clear_key(VENDOR_DEEPSEEK).unwrap();
        let st = status().unwrap();
        let ds = st.vendors.iter().find(|v| v.vendor == VENDOR_DEEPSEEK).unwrap();
        assert!(!ds.has_key);
        assert!(ds.enabled);
        assert_eq!(ds.models.len(), 2);
        assert!(
            reveal_enabled_keys().unwrap().iter().all(|(v, _)| v != VENDOR_DEEPSEEK),
            "清 Key 后不应再可取出"
        );
        clear_key(VENDOR_DEEPSEEK).unwrap(); // 幂等

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn legacy_single_entry_file_migrates_on_read() {
        let (_lock, tmp) = sandbox("migrate");
        let legacy = serde_json::json!({
            "vendor": "deepseek",
            "ciphertext_b64": base64_encode(b"irrelevant-but-valid"),
            "models": ["deepseek-v4-flash"],
        });
        std::fs::create_dir_all(vault_path(&tmp).parent().unwrap()).unwrap();
        std::fs::write(vault_path(&tmp), legacy.to_string()).unwrap();

        let st = status().unwrap();
        let ds = st.vendors.iter().find(|v| v.vendor == VENDOR_DEEPSEEK).unwrap();
        assert!(ds.enabled, "旧单条格式迁移后视为已启用");
        assert!(ds.has_key);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn invalid_inputs_fail_loud_before_touching_disk() {
        let (_lock, tmp) = sandbox("invalid");
        assert!(set_key("unknown-vendor", "sk-x").is_err(), "未知厂商拒绝");
        assert!(set_key(VENDOR_DEEPSEEK, "   ").is_err(), "空白 Key 拒绝");
        assert!(!vault_path(&tmp).exists(), "失败路径不得产生文件");

        // 模型归一化：trim/去空/按首次出现去重，空列表合法。
        set_models(VENDOR_DEEPSEEK, &["  ".into(), "a".into(), "a".into()]).unwrap();
        let st = status().unwrap();
        assert_eq!(st.vendors[0].models, vec!["a".to_string()]);

        set_key(VENDOR_DEEPSEEK, "sk-first").unwrap();
        assert!(set_key(VENDOR_DEEPSEEK, "").is_err());
        assert_eq!(
            reveal_enabled_keys()
                .unwrap()
                .into_iter()
                .find(|(v, _)| v == VENDOR_DEEPSEEK)
                .map(|(_, key)| key)
                .unwrap(),
            "sk-first",
            "失败保存不得破坏既有密文"
        );

        // 环境变量名映射：已知厂商有名字，未知厂商没有。
        assert_eq!(vendor_env_name(VENDOR_DEEPSEEK), Some("DEEPSEEK_API_KEY"));
        assert_eq!(vendor_env_name(VENDOR_OPENAI), Some("OPENAI_API_KEY"));
        assert_eq!(vendor_env_name("anthropic"), Some("ANTHROPIC_API_KEY"));
        assert_eq!(vendor_env_name("kimi"), Some("MOONSHOT_API_KEY"));
        assert_eq!(vendor_env_name("ollama"), None, "本地网关免 Key");
        assert_eq!(vendor_env_name("nope"), None);
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// R5 修复回归：内置目录全部 id 的启用/存 Key 必须放行（此前白名单只有
    /// deepseek/openai，勾选 glm 等必然报错回弹）。
    #[test]
    fn every_catalog_vendor_is_writable() {
        let (_lock, tmp) = sandbox("catalog-writable");
        for entry in crate::providers::catalog() {
            set_enabled(&entry.id, true).unwrap_or_else(|e| {
                panic!("内置厂商 {} 启用被拒：{e}", entry.id)
            });
        }
        let st = status().unwrap();
        for entry in crate::providers::catalog() {
            let row = st
                .vendors
                .iter()
                .find(|v| v.vendor == entry.id)
                .unwrap_or_else(|| panic!("启用后 status 缺厂商 {}", entry.id));
            assert!(row.enabled, "{} 应为启用态", entry.id);
        }
        // 任选一个非 deepseek 厂商走存 Key 链（DPAPI 往返）。
        set_key("glm", "sk-glm-test-1234567890abcdef").unwrap();
        let st = status().unwrap();
        let glm = st.vendors.iter().find(|v| v.vendor == "glm").unwrap();
        assert!(glm.has_key);
        assert_eq!(
            reveal_key_for("glm").unwrap(),
            "sk-glm-test-1234567890abcdef"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// R5：forget_vendor 整条移除（含 Key/启用态），幂等，github 拒绝。
    #[test]
    fn forget_vendor_removes_record_idempotent() {
        let (_lock, tmp) = sandbox("forget");
        set_enabled("kimi", true).unwrap();
        set_key("kimi", "sk-kimi-test-1234567890abcdef").unwrap();
        let st = status().unwrap();
        assert!(st.vendors.iter().any(|v| v.vendor == "kimi"));

        forget_vendor("kimi").unwrap();
        let st = status().unwrap();
        assert!(!st.vendors.iter().any(|v| v.vendor == "kimi"), "移除后记录消失");
        assert!(reveal_key_for("kimi").is_err(), "Key 随记录一并清除");

        forget_vendor("kimi").unwrap(); // 幂等
        assert!(forget_vendor(VENDOR_GITHUB).is_err(), "github 不在模型厂商列表");
        assert!(forget_vendor("unknown-vendor").is_err(), "未知厂商拒绝");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// GitHub Token 复用保险库存储但不污染模型厂商视图：不出现在 vendors、
    /// 不参与启动注入，仅以 githubToken* 布尔位可见（P3 市场 Token 提额）。
    #[cfg(windows)]
    #[test]
    fn github_token_is_isolated_from_model_vendors() {
        let (_lock, tmp) = sandbox("github-token");
        set_key(VENDOR_GITHUB, "ghp_test1234567890abcdef").unwrap();
        set_key(VENDOR_DEEPSEEK, "sk-test-1234567890abcdefSECRET").unwrap();

        let st = status().unwrap();
        assert!(
            st.vendors.iter().all(|v| v.vendor != VENDOR_GITHUB),
            "github 不得作为厂商卡出现"
        );
        assert!(st.github_token_configured);
        assert_eq!(st.vendors.len(), 1, "模型厂商视图不受影响");
        assert_eq!(st.vendors[0].vendor, VENDOR_DEEPSEEK);

        // 启动注入只取模型厂商：github 条目即使误启用也不进环境注入。
        set_enabled(VENDOR_GITHUB, true).unwrap();
        assert!(
            reveal_enabled_keys().unwrap().iter().all(|(v, _)| v != VENDOR_GITHUB),
            "github token 不得注入实例进程环境"
        );
        // reveal_key_for 面向壳侧功能（github 客户端提额）仍可解密。
        assert_eq!(reveal_key_for(VENDOR_GITHUB).unwrap(), "ghp_test1234567890abcdef");

        // 磁盘无明文。
        let disk = std::fs::read_to_string(vault_path(&tmp)).unwrap();
        assert!(!disk.contains("ghp_test1234567890abcdef"));

        clear_key(VENDOR_GITHUB).unwrap();
        let st = status().unwrap();
        assert!(!st.github_token_configured);
        assert_eq!(reveal_key_for(VENDOR_GITHUB).unwrap_err(), "厂商 \"github\" 尚未配置 API Key");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// DPAPI 真实加解密往返（纯本机 API，无网络无 UI）。
    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip_real() {
        let secret = b"dsh-launcher-dpapi-selftest";
        let cipher = dpapi::protect(secret).unwrap();
        assert_ne!(cipher, secret, "密文不应等于明文");
        assert_eq!(dpapi::unprotect(&cipher).unwrap(), secret.to_vec());

        // 篡改密文应失败而不是产出垃圾明文。
        let mut corrupted = cipher.clone();
        corrupted[cipher.len() / 2] ^= 0xFF;
        let repaired = dpapi::unprotect(&corrupted);
        if let Ok(text) = repaired {
            assert_ne!(text, secret.to_vec(), "篡改后不应还原出原文");
        }
    }
}
