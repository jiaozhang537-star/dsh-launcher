//! 整合包清单格式与校验（FR-04 · T-3 定稿 2026-09-12）。
//!
//! 两类包共用一个 tgz 容器（`manifest.json` + `packages/*.tgz` 插件 npm 源码
//! tarball），manifest 的 `type` 分辨类型且**可混合**（插件合集 / 设置包不互斥，
//! 对标 PCL：mod 包、整包、带启动器包共存）。本模块是纯数据层：结构、解析、
//! 校验；打包/解包见同文件导出与导入管线，安装走 `market_install` 官方链。
//!
//! 安全与隐私边界（FILE-LAYOUT 入包口径）：Key 密文、环境变量（env）、
//! 对话列表（sessions）**没有对应的 manifest 字段**——结构上就装不进去。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Map;

/// manifest 格式版本（不兼容变更时递增；导入端只认自己认识的版本）。
pub const FORMAT_VERSION: u32 = 1;

/// 包内清单文件名（tgz 根下）。
pub const MANIFEST_NAME: &str = "manifest.json";

/// 包内插件 tarball 目录名。
pub const PACKAGES_DIR: &str = "packages";

/// 包类型：`plugins` = 插件合集；`settings` = 设置包；`mixed` = 两者都有。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackType {
    Plugins,
    Settings,
    Mixed,
}

impl PackType {
    /// 由内容推导类型（导出侧用；导入侧以清单声明为准并做一致性校验）。
    pub fn derive(has_plugins: bool, has_settings: bool) -> PackType {
        match (has_plugins, has_settings) {
            (true, true) => PackType::Mixed,
            (true, false) => PackType::Plugins,
            (false, true) => PackType::Settings,
            (false, false) => PackType::Plugins, // 空包无意义，校验层拦截
        }
    }
}

/// 清单里一个插件条目（名称 + 版本 + 来源 + 包内文件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry {
    /// npm 包名（含 scope，如 `@deepseek-ai/dsh-base`）。
    pub name: String,
    /// 导出时的已装版本（semver 键；git/自建来源允许空串，导入端回落 latest）。
    #[serde(default)]
    pub version: String,
    /// 来源：`npm`（默认）/ `github` / `file`。仅描述性字段，安装走包内 tarball。
    #[serde(default)]
    pub source: String,
    /// 包内 tarball 相对路径（`packages/<名>-<版本>.tgz`）。空 = 纯清单条目。
    #[serde(default)]
    pub file: String,
}

/// 设置包·模型提供商条目（keyvault 的无密钥视图：enabled/models/address）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VendorEntry {
    pub vendor: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub models: Vec<String>,
    /// API 地址覆盖；None = 使用目录默认地址。
    #[serde(default)]
    pub address: Option<String>,
    /// 自定义提供方的显示名（仅 custom-* 条目携带，导入重建用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 自定义提供方的接口形态（openai/responses/anthropic/gemini/ollama）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<crate::providers::ApiStyle>,
}

/// 设置包·AGENTS.md 内容（全局与版本级各自独立，均可缺省）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsMd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

/// 设置包内容节（FILE-LAYOUT 入包口径：无 Key/密文、无 env、无会话）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSection {
    #[serde(default)]
    pub vendors: Vec<VendorEntry>,
    /// 热键覆盖表（导入恒写启动器全局，无实例维度）。
    #[serde(default)]
    pub hotkeys: BTreeMap<String, String>,
    /// 注入 settings（`InjectConfig.settings` 的 JSON 形态；env 不在此结构内）。
    #[serde(default)]
    pub inject_settings: Map<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents_md: Option<AgentsMd>,
}

impl SettingsSection {
    /// 是否携带任何设置内容（决定 `type` 推导与混合校验）。
    pub fn is_present(&self) -> bool {
        !self.vendors.is_empty()
            || !self.hotkeys.is_empty()
            || !self.inject_settings.is_empty()
            || self.agents_md.as_ref().map_or(false, |a| {
                a.global.is_some() || a.instance.is_some()
            })
    }
}

/// 整合包清单（manifest v1）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format_version: u32,
    #[serde(rename = "type")]
    pub pack_type: PackType,
    /// 包名（导入确认弹窗展示用，必填非空）。
    pub name: String,
    /// 生成方标识（如 `dsh-launcher/0.1.0`），仅描述性。
    #[serde(default)]
    pub generator: String,
    /// RFC3339 生成时间，仅描述性。
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub plugins: Vec<PluginEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<SettingsSection>,
}

/// 包内插件 tarball 的确定性文件名：scope 去掉 `@`、`/` 换 `-`（npm pack 同款
/// 命名习惯），如 `@a/b@1.2.3` → `packages/a-b-1.2.3.tgz`。
pub fn plugin_tarball_rel_path(name: &str, version: &str) -> String {
    let base = name.replace('@', "").replace('/', "-");
    format!("{PACKAGES_DIR}/{base}-{version}.tgz")
}

/// 解析 manifest JSON 文本 → 结构（字段级错误带路径，fail loud）。
pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    serde_json::from_str(text).map_err(|e| format!("清单解析失败：{e}"))
}

/// 全量校验：结构、类型一致性、插件名/版本/去重、设置节内容。返回可读错误。
pub fn validate(m: &Manifest) -> Result<(), String> {
    if m.format_version != FORMAT_VERSION {
        return Err(format!(
            "不支持的清单格式版本：{}（本启动器支持 v{FORMAT_VERSION}）",
            m.format_version
        ));
    }
    let name = m.name.trim();
    if name.is_empty() {
        return Err("清单缺少包名（name）".into());
    }
    if name.chars().count() > 100 {
        return Err("包名过长（上限 100 字符）".into());
    }

    let has_plugins = !m.plugins.is_empty();
    let has_settings = m.settings.as_ref().map_or(false, SettingsSection::is_present);
    // 空类型节 = 空包，无论 type 怎么写都拒绝（防止「什么都没有却叫整合包」）。
    if !has_plugins && !has_settings {
        return Err("清单为空：既没有插件也没有设置内容".into());
    }
    let expected = PackType::derive(has_plugins, has_settings);
    if m.pack_type != expected {
        return Err(format!(
            "清单类型不一致：声明 {:?}，实际内容对应 {:?}",
            m.pack_type, expected
        ));
    }

    let mut seen = std::collections::BTreeSet::new();
    for p in &m.plugins {
        let trimmed = p.name.trim();
        if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
            return Err(format!("非法插件名：{:?}", p.name));
        }
        if !seen.insert(trimmed.to_string()) {
            return Err(format!("插件重复列出：{trimmed}"));
        }
        if !p.version.is_empty() && crate::registry_meta::parse_semver_key(&p.version).is_none() {
            return Err(format!("插件 {trimmed} 的版本号非法：{:?}", p.version));
        }
        match p.source.as_str() {
            "" | "npm" | "github" | "file" => {}
            other => return Err(format!("插件 {trimmed} 的来源未知：{other:?}")),
        }
        if !p.file.is_empty() {
            if !p.file.starts_with(&format!("{PACKAGES_DIR}/")) {
                return Err(format!("插件 {trimmed} 的包内文件路径非法：{:?}", p.file));
            }
            if p.file.contains('\\') || p.file.contains("..") {
                return Err(format!("插件 {trimmed} 的包内文件路径非法：{:?}", p.file));
            }
        }
    }

    if let Some(settings) = &m.settings {
        let mut vendors = std::collections::BTreeSet::new();
        for v in &settings.vendors {
            let id = v.vendor.trim();
            if id.is_empty() {
                return Err("设置包存在空厂商 id 的提供商条目".into());
            }
            if !vendors.insert(id.to_string()) {
                return Err(format!("提供商重复列出：{id}"));
            }
            for model in &v.models {
                if model.trim().is_empty() {
                    return Err(format!("提供商 {id} 存在空白模型 id"));
                }
            }
        }
        for action in settings.hotkeys.keys() {
            if action.trim().is_empty() {
                return Err("设置包的热键表存在空动作 id".into());
            }
        }
    }
    Ok(())
}

/// 解析 + 校验一步到位（导入管线入口）。
pub fn parse_and_validate(text: &str) -> Result<Manifest, String> {
    let m = parse_manifest(text)?;
    validate(&m)?;
    Ok(m)
}

// ============================================================================
// 导出管线（T-3 定稿：自包含离线 tgz）
// ============================================================================

/// 导出请求（IPC camelCase；前端勾选结果）。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub instance_id: String,
    /// 包名（清单 name；文件名由保存对话框的 dest 决定）。
    pub name: String,
    /// 勾选导出的第三方插件名（`plugin_manage::list` thirdparty 名单的子集）。
    #[serde(default)]
    pub plugins: Vec<String>,
    #[serde(default)]
    pub include_vendors: bool,
    #[serde(default)]
    pub include_hotkeys: bool,
    #[serde(default)]
    pub include_inject: bool,
    #[serde(default)]
    pub include_agents_md: bool,
}

/// 收集设置节（按勾选；全部来自启动器自身持久化，天然无 Key/密文/env/会话）。
fn collect_settings(opts: &ExportOptions) -> Result<SettingsSection, String> {
    let mut section = SettingsSection::default();
    if opts.include_vendors {
        // 保险库 = 已添加的提供商全集（按需添加式）；自定义提供方补 name/style。
        let vault = crate::keyvault::status()?;
        let view = crate::providers::view()?;
        for v in &vault.vendors {
            let mut entry = VendorEntry {
                vendor: v.vendor.clone(),
                enabled: v.enabled,
                models: v.models.clone(),
                address: v.address.clone(),
                name: None,
                style: None,
            };
            if let Some(custom) = view.custom.iter().find(|c| c.id == v.vendor) {
                entry.name = Some(custom.name.clone());
                entry.style = Some(custom.style);
                if entry.address.is_none() {
                    entry.address = Some(custom.base.clone());
                }
            }
            section.vendors.push(entry);
        }
    }
    if opts.include_hotkeys {
        section.hotkeys = crate::hotkey_store::load()?;
    }
    if opts.include_inject {
        section.inject_settings = crate::inject::effective_config(&opts.instance_id)?.settings;
    }
    if opts.include_agents_md {
        section.agents_md = Some(AgentsMd {
            global: crate::inject::get_global_agents_md()?,
            instance: crate::inject::get_override(&opts.instance_id)?.agents_md,
        });
    }
    Ok(section)
}

/// 暂存目录里唯一的 `.tgz` 即 `npm pack` 的产物（产物名不可从 stdout 可靠
/// 解析）；0 个或 ≥2 个都视为该通道产物异常。
fn single_produced_tgz(dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let produced: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("读取暂存目录失败：{e}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |ext| ext == "tgz"))
        .collect();
    match produced.len() {
        1 => Ok(produced.into_iter().next().expect("len == 1 已判定")),
        n => Err(format!("产物异常：期望 1 个 tgz，实际 {n} 个")),
    }
}

/// `npm pack <spec>` 拉插件源码 tarball 到 `dest_dir`（npmmirror 优先、失败回落
/// 官方——与插件安装同一通道链，经 `npm_config_registry` 注入）。成功返回
/// 下载产物 tgz 的绝对路径。
fn npm_pack(
    node_exe: &std::path::Path,
    npm_cli: &std::path::Path,
    spec: &str,
    dest_dir: &std::path::Path,
    progress: &mut dyn FnMut(String),
) -> Result<std::path::PathBuf, String> {
    use crate::node_runtime::CreationFlagsExt;
    let mut attempts: Vec<String> = Vec::new();
    for registry_base in crate::gateway::npm_registry_bases() {
        progress(format!("正在拉取 {spec}（通道 {registry_base}）…"));
        let mut cmd = std::process::Command::new(node_exe);
        cmd.arg(npm_cli)
            .args(["pack", spec, "--pack-destination"])
            .arg(dest_dir)
            .env("npm_config_registry", registry_base)
            .env("COREPACK_NPM_REGISTRY", registry_base)
            .creation_flags_cv();
        match cmd.output() {
            Ok(out) if out.status.success() => {
                // npm pack 不清空 --pack-destination：目录必须由调用方保证为本
                // 次调用独占（见 export_pack_inner 的每插件子目录），否则前一
                // 个插件的残留会污染「恰 1 个 tgz」的产物判定。
                match single_produced_tgz(dest_dir) {
                    Ok(path) => return Ok(path),
                    Err(e) => attempts.push(format!("[通道 {registry_base}] {e}")),
                }
            }
            Ok(out) => {
                let tail = String::from_utf8_lossy(&out.stderr);
                let tail = tail.lines().rev().take(4).collect::<Vec<_>>().join(" | ");
                attempts.push(format!("[通道 {registry_base}] {tail}"));
            }
            Err(e) => attempts.push(format!("[通道 {registry_base}] 拉起 npm 失败：{e}")),
        }
    }
    Err(format!("npm pack {spec} 在全部通道上失败：\n{}", attempts.join("\n---\n")))
}

/// 组装 tgz：`manifest.json` + `packages/*.tgz`（临时文件 + 改名语义由
/// 「写到 dest.tmp 再改名」实现，避免半成品被当成完整包）。
fn write_tgz(
    manifest: &Manifest,
    staged: &[(std::path::PathBuf, String)],
    dest: &std::path::Path,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建 {} 失败：{e}", parent.display()))?;
    }
    let tmp = dest.with_extension("tgz.tmp-writing");
    let file = std::fs::File::create(&tmp).map_err(|e| format!("创建 {} 失败：{e}", tmp.display()))?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);

    let json = serde_json::to_string_pretty(manifest).map_err(|e| format!("序列化清单失败：{e}"))?;
    let mut header = tar::Header::new_gnu();
    header.set_size(json.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, MANIFEST_NAME, json.as_bytes())
        .map_err(|e| format!("写入清单失败：{e}"))?;

    for (src, rel) in staged {
        builder
            .append_path_with_name(src, rel)
            .map_err(|e| format!("打包 {rel} 失败：{e}"))?;
    }

    let encoder = builder
        .into_inner()
        .map_err(|e| format!("收尾 tar 失败：{e}"))?;
    encoder
        .finish()
        .map_err(|e| format!("收尾 gzip 失败：{e}"))?;
    std::fs::rename(&tmp, dest).map_err(|e| format!("替换 {} 失败：{e}", dest.display()))?;
    Ok(())
}

/// 导出整合包（阻塞调用——command 侧放 spawn_blocking）。流程：勾选校验 →
/// 逐插件 `npm pack` 暂存 → 收集设置节 → 组 manifest → 打 tgz。返回清单
/// （前端据此展示摘要与落点提示）。
pub fn export_pack(
    opts: &ExportOptions,
    dest: &std::path::Path,
    progress: &mut dyn FnMut(String),
) -> Result<Manifest, String> {
    let temp = std::env::temp_dir().join(format!(
        "dsh-modpack-export-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&temp);
    let result = export_pack_inner(opts, dest, &temp, progress);
    let _ = std::fs::remove_dir_all(&temp);
    result
}

fn export_pack_inner(
    opts: &ExportOptions,
    dest: &std::path::Path,
    temp: &std::path::Path,
    progress: &mut dyn FnMut(String),
) -> Result<Manifest, String> {
    use crate::plugin_manage::PluginGroup;

    let list = crate::plugin_manage::list(&opts.instance_id)?;
    let mut entries: Vec<PluginEntry> = Vec::new();
    let mut staged: Vec<(std::path::PathBuf, String)> = Vec::new();

    if !opts.plugins.is_empty() {
        let available: std::collections::BTreeMap<&str, &crate::plugin_manage::PluginRow> = list
            .plugins
            .iter()
            .filter(|p| matches!(p.group, PluginGroup::Thirdparty))
            .map(|p| (p.name.as_str(), p))
            .collect();
        let pack_root = temp.join("pack");
        std::fs::create_dir_all(&pack_root).map_err(|e| format!("创建暂存目录失败：{e}"))?;
        let instance = crate::instances::get_instance(&opts.instance_id)?;
        let node_exe = crate::dsh_install::resolve_install_node(&instance, &mut |_| {})?;
        let npm_cli = crate::dsh_install::npm_cli_of(&node_exe)?;
        for (idx, name) in opts.plugins.iter().enumerate() {
            let row = available
                .get(name.as_str())
                .ok_or_else(|| format!("插件 {name} 不在该实例的第三方已装清单中"))?;
            if row.version.is_empty() {
                return Err(format!(
                    "无法确定 {name} 的已装版本，无法打包——可到插件市场重装后再导出"
                ));
            }
            // 每个插件独立暂存子目录：npm pack 不清空 --pack-destination，共用
            // 同一目录时前一插件的产物会让「恰 1 个 tgz」判定从第二个插件起
            // 必然失败（2026-09-20 审查 P1-01，真机复现 .recon/r4-lab）。
            let pack_dir = pack_root.join(idx.to_string());
            std::fs::create_dir_all(&pack_dir).map_err(|e| format!("创建暂存目录失败：{e}"))?;
            let tgz = npm_pack(&node_exe, &npm_cli, &format!("{name}@{}", row.version), &pack_dir, progress)?;
            let rel = plugin_tarball_rel_path(name, &row.version);
            staged.push((tgz, rel.clone()));
            entries.push(PluginEntry {
                name: name.clone(),
                version: row.version.clone(),
                source: "npm".into(),
                file: rel,
            });
        }
    }

    let wants_settings =
        opts.include_vendors || opts.include_hotkeys || opts.include_inject || opts.include_agents_md;
    let settings = if wants_settings {
        progress("正在收集设置内容…".into());
        Some(collect_settings(opts)?)
    } else {
        None
    };

    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        pack_type: PackType::derive(
            !entries.is_empty(),
            settings.as_ref().map_or(false, SettingsSection::is_present),
        ),
        name: opts.name.trim().to_string(),
        generator: format!("dsh-launcher/{}", env!("CARGO_PKG_VERSION")),
        created_at: crate::instances::now_rfc3339(),
        plugins: entries,
        settings,
    };
    validate(&manifest)?;

    progress(format!("正在打包 {}…", dest.display()));
    write_tgz(&manifest, &staged, dest)?;
    progress("打包完成".into());
    Ok(manifest)
}

// ============================================================================
// 导入管线（file: 离线安装 + 失败回滚 + 设置落点写入）
// ============================================================================

/// 流式读取包内 manifest（不解包；供导入前预览）。
pub fn peek_manifest(tgz: &std::path::Path) -> Result<Manifest, String> {
    let file = std::fs::File::open(tgz).map_err(|e| format!("打开 {} 失败：{e}", tgz.display()))?;
    let decoder = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
    let mut archive = tar::Archive::new(decoder);
    for entry in archive
        .entries()
        .map_err(|e| format!("读取 tar 结构失败：{e}"))?
    {
        let mut entry = entry.map_err(|e| format!("遍历 tar 条目失败：{e}"))?;
        let name = entry
            .path()
            .map_err(|e| format!("读取条目名失败：{e}"))?
            .to_string_lossy()
            .to_string();
        if name == MANIFEST_NAME {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut entry, &mut text)
                .map_err(|e| format!("读取清单失败：{e}"))?;
            return parse_and_validate(&text);
        }
    }
    Err(format!("包内缺少 {MANIFEST_NAME}——不是本启动器的整合包"))
}

/// 解包 tgz 到 dest（条目名经 `safe_entry_path` 防穿越；剥不剥层不重要——
/// 包是自己写的扁平结构，但防护对未知来源的包必须一视同仁）。
fn extract_tgz_to(tgz: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::open(tgz).map_err(|e| format!("打开 {} 失败：{e}", tgz.display()))?;
    let decoder = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
    let mut archive = tar::Archive::new(decoder);
    for entry in archive
        .entries()
        .map_err(|e| format!("读取 tar 结构失败：{e}"))?
    {
        let mut entry = entry.map_err(|e| format!("遍历 tar 条目失败：{e}"))?;
        let name = entry
            .path()
            .map_err(|e| format!("读取条目名失败：{e}"))?
            .to_string_lossy()
            .to_string();
        let out = crate::paths::safe_entry_path(dest, &name)?;
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("创建 {} 失败：{e}", out.display()))?;
            continue;
        }
        // 远端内容 fail-closed：目录条目之外仅放行普通文件——符号链接/硬链接
        // 条目会被 unpack 原样落链（目标可为任意路径），一律拒绝解压。
        if !entry.header().entry_type().is_file() {
            return Err(format!(
                "包内包含非法条目类型（符号链接/硬链接/特殊文件）：{name}——拒绝解压"
            ));
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建 {} 失败：{e}", parent.display()))?;
        }
        entry
            .unpack(&out)
            .map_err(|e| format!("解压 {name} 失败：{e}"))?;
    }
    Ok(())
}

/// 设置落点：全局默认注入模板 / 指定实例（IPC 传 "global"/"instance"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingsTarget {
    Global,
    Instance,
}

/// 导入报告（逐条可读；失败时已装项在 Err 文案里逐条列出）。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub manifest_name: String,
    pub installed: Vec<crate::market_install::PluginInstallOutcome>,
    /// 已成功应用的设置节描述（"模型提供商 ×3" 等）。
    pub applied: Vec<String>,
    /// 非致命警告（如某设置节写入失败但不回滚插件）。
    pub warnings: Vec<String>,
}

/// 导入整合包（阻塞调用——command 侧放 spawn_blocking）。
///
/// `instance_id` = 插件安装目标实例（纯设置包时不使用但仍须合法）；
/// `settings_target` = 设置节落点（None = 包内无设置节）。
///
/// 失败语义：插件环节任一步失败 → 反向移除已装项（尽力而为）并整体报错，
/// 设置节不写入——无半装状态；设置节写入失败 → 插件保留，逐节报错进
/// warnings（设置可重导，插件回滚成本高）。
pub fn import_pack(
    tgz: &std::path::Path,
    instance_id: &str,
    settings_target: Option<SettingsTarget>,
    progress: &mut dyn FnMut(String),
) -> Result<ImportReport, String> {
    let manifest = peek_manifest(tgz)?;
    // 实例存在性校验按需进行：纯设置包走全局落点时不需要实例
    // （全新启动器也能先导设置）。
    let needs_instance = !manifest.plugins.is_empty() || settings_target == Some(SettingsTarget::Instance);
    if needs_instance {
        crate::instances::get_instance(instance_id)?;
    }

    let temp = std::env::temp_dir().join(format!(
        "dsh-modpack-import-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&temp);
    let result = import_pack_inner(tgz, instance_id, settings_target, &manifest, &temp, progress);
    let _ = std::fs::remove_dir_all(&temp);
    result
}

fn import_pack_inner(
    tgz: &std::path::Path,
    instance_id: &str,
    settings_target: Option<SettingsTarget>,
    manifest: &Manifest,
    temp: &std::path::Path,
    progress: &mut dyn FnMut(String),
) -> Result<ImportReport, String> {
    let mut report = ImportReport {
        manifest_name: manifest.name.clone(),
        ..Default::default()
    };
    progress(format!("正在解包「{}」…", manifest.name));
    extract_tgz_to(tgz, temp)?;

    // ── 插件环节：逐个 file: 安装，失败反向回滚 ──
    if !manifest.plugins.is_empty() {
        // 实例必须存在（fail loud 可读）；此前用于 spec 归一化的 profile 已随
        // 归一化撤销而不需要。
        crate::instances::get_instance(instance_id)?;
        let mut installed: Vec<crate::market_install::PluginInstallOutcome> = Vec::new();
        for entry in &manifest.plugins {
            if entry.file.is_empty() {
                report.warnings.push(format!(
                    "插件 {} 缺包内文件（纯清单条目）——已按原版本走在线安装链",
                    entry.name
                ));
                let spec = if entry.version.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{}@{}", entry.name, entry.version)
                };
                match crate::market_install::plugin_add(instance_id, &spec, progress) {
                    Ok(outcome) => installed.push(outcome),
                    Err(e) => {
                        return Err(rollback_plugins(&installed, &e, progress));
                    }
                }
                continue;
            }
            let tarball = temp.join(&entry.file);
            if !tarball.is_file() {
                let err = format!("包内缺少 {} 的 tarball（{}）", entry.name, entry.file);
                return Err(rollback_plugins(&installed, &err, progress));
            }
            // tarball 落实例 `imported/` 留档（FILE-LAYOUT §3）：file: spec 必须
            // 指向导入结束后仍然存在的路径——pnpm 的 add/remove 都会重链 file:
            // 依赖，指向已删除的导入暂存区会让后续一切插件操作 ENOENT
            // （双包 e2e 实证 2026-09-20）。
            let imported_dir = crate::instances::instance_dir(instance_id)?.join("imported");
            std::fs::create_dir_all(&imported_dir)
                .map_err(|e| format!("创建导入留档目录失败：{e}"))?;
            let file_name = entry
                .file
                .rsplit('/')
                .next()
                .unwrap_or("plugin.tgz")
                .to_string();
            let persistent = imported_dir.join(file_name);
            std::fs::copy(&tarball, &persistent)
                .map_err(|e| format!("留存 {} 失败：{e}", persistent.display()))?;
            progress(format!("正在离线安装 {} {}…", entry.name, entry.version));
            // file: tarball 直装（pnpm/npm spec 语义）；个别 pnpm 版本不收
            // tarball 时回落到解开 package/ 目录再 file:（罕见路径，真实环境
            // 未观测到触发；其 spec 指向导入暂存区，导入后即失效）。
            let spec = format!("file:{}", persistent.display());
            let outcome = match crate::market_install::plugin_add(instance_id, &spec, progress) {
                Ok(outcome) => Ok(outcome),
                Err(first_err) => {
                    progress("file: 直装 tarball 失败，回落为解包目录安装…".into());
                    let dir = extract_package_dir(&tarball, temp)?;
                    match crate::market_install::plugin_add(
                        instance_id,
                        &format!("file:{}", dir.display()),
                        progress,
                    ) {
                        Ok(outcome) => Ok(outcome),
                        Err(_) => Err(first_err),
                    }
                }
            };
            match outcome {
                Ok(o) => {
                    // spec 保持 file: 原样、不归一为 ^版本：pnpm 每次安装都会重
                    // 解析清单里 spec 与 lockfile 不一致的依赖——归一后的未发布
                    // 包会被回 registry 拉 → 404 → 多插件导入从第二个起必挂
                    // （双包 e2e 实证 2026-09-20，撤销走查反馈批次②的归一化；
                    // 「不误入自建组」由 group_plugins 的 *.tgz 兜底保证）。
                    installed.push(o);
                }
                Err(e) => return Err(rollback_plugins(&installed, &e, progress)),
            }
        }
        report.installed = installed;
    }

    // ── 设置环节：插件已齐，设置失败不回滚插件（逐节报告） ──
    if let Some(settings) = &manifest.settings {
        if let Some(target) = settings_target {
            apply_settings(settings, target, instance_id, &mut report, progress);
        }
    }
    progress(format!(
        "导入完成：{}",
        if report.installed.is_empty() {
            "无插件".to_string()
        } else {
            format!("装齐 {} 个插件", report.installed.len())
        }
    ));
    Ok(report)
}

/// npm tarball → `package/` 目录（pnpm 不收 tarball 时的 file: 目录回落）。
fn extract_package_dir(
    tarball: &std::path::Path,
    temp: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let dir = temp.join(format!(
        "unpacked-{}",
        tarball.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    ));
    extract_tgz_to(tarball, &dir)?;
    let package = dir.join("package");
    if !package.join("package.json").is_file() {
        return Err(format!("tarball 结构异常：{} 缺 package/package.json", tarball.display()));
    }
    Ok(package)
}

/// 回滚：反向移除已装项（尽力而为，逐条记录失败），返回携带全部信息的错误串。
fn rollback_plugins(
    installed: &[crate::market_install::PluginInstallOutcome],
    cause: &str,
    progress: &mut dyn FnMut(String),
) -> String {
    let mut lines = vec![format!("导入失败：{cause}")];
    if installed.is_empty() {
        lines.push("未安装任何插件，无需回滚。".into());
    } else {
        lines.push(format!("正在回滚已安装的 {} 个插件…", installed.len()));
        // 真反向：后装的先拆（含互相依赖的包时先摘依赖方，残留面最小）。
        for outcome in installed.iter().rev() {
            progress(format!("正在回滚 {}…", outcome.package_name));
            match crate::plugin_manage::remove(&outcome.instance_id, &outcome.package_name, &mut |_| {}) {
                Ok(()) => lines.push(format!("已回滚 {}", outcome.package_name)),
                Err(e) => lines.push(format!(
                    "回滚 {} 失败：{e}（可在插件管理页手动删除）",
                    outcome.package_name
                )),
            }
        }
    }
    lines.join("\n")
}

/// 应用设置节到落点（逐节 best-effort；失败进 warnings 不阻断）。
fn apply_settings(
    settings: &SettingsSection,
    target: SettingsTarget,
    instance_id: &str,
    report: &mut ImportReport,
    progress: &mut dyn FnMut(String),
) {
    // 1) 模型提供商：内置 = keyvault 增改；custom-* = 重建自定义提供方再配 vault。
    if !settings.vendors.is_empty() {
        progress("正在写入模型提供商…".into());
        let mut ok = 0usize;
        let mut errs: Vec<String> = Vec::new();
        for v in &settings.vendors {
            let mut vendor_id = v.vendor.clone();
            let is_custom = crate::providers::is_custom_id(&v.vendor);
            if is_custom {
                let name = v.name.clone().unwrap_or_else(|| v.vendor.clone());
                let style = v.style.unwrap_or(crate::providers::ApiStyle::Openai);
                let base = v.address.clone().unwrap_or_default();
                match crate::providers::add_custom(&name, &base, style) {
                    Ok(created) => vendor_id = created.id,
                    Err(e) => {
                        errs.push(format!("重建自定义提供方 {name} 失败：{e}"));
                        continue;
                    }
                }
            }
            let mut vendor_ok = true;
            if crate::keyvault::set_enabled(&vendor_id, v.enabled).is_err() {
                vendor_ok = false;
            }
            if !v.models.is_empty()
                && crate::keyvault::set_models(&vendor_id, &v.models).is_err()
            {
                vendor_ok = false;
            }
            if let Some(addr) = &v.address {
                if !is_custom && crate::keyvault::set_address(&vendor_id, addr).is_err() {
                    vendor_ok = false;
                }
            }
            if vendor_ok {
                ok += 1;
            } else {
                errs.push(format!("写入提供商 {} 的凭据记录失败", v.vendor));
            }
        }
        if ok > 0 {
            report.applied.push(format!("模型提供商 ×{ok}"));
        }
        report.warnings.extend(errs);
    }

    // 2) 热键：合并写启动器全局（无实例维度）。
    if !settings.hotkeys.is_empty() {
        progress("正在合并热键配置…".into());
        match crate::hotkey_store::load() {
            Ok(mut current) => {
                for (action, combo) in &settings.hotkeys {
                    current.insert(action.clone(), combo.clone());
                }
                match crate::hotkey_store::save(&current) {
                    Ok(()) => report.applied.push(format!("热键 ×{}", settings.hotkeys.len())),
                    Err(e) => report.warnings.push(format!("热键写入失败：{e}")),
                }
            }
            Err(e) => report.warnings.push(format!("热键写入失败：{e}")),
        }
    }

    // 3) 注入 settings + AGENTS.md：按落点写全局默认模板或实例覆盖，随后物化。
    let inject_changed = !settings.inject_settings.is_empty();
    let agents = settings.agents_md.as_ref();
    if !inject_changed && agents.is_none() {
        return;
    }
    progress("正在写入注入配置…".into());
    let write_result: Result<(), String> = (|| {
        match target {
            SettingsTarget::Global => {
                let mut template = crate::inject::get_default_template()?;
                if inject_changed {
                    let mut merged = serde_json::Value::Object(template.settings.clone());
                    crate::inject::deep_merge(
                        &mut merged,
                        serde_json::Value::Object(settings.inject_settings.clone()),
                    );
                    template.settings = serde_json::from_value(merged).unwrap_or_default();
                }
                if let Some(a) = agents {
                    if a.global.is_some() {
                        crate::inject::set_global_agents_md(a.global.as_deref())?;
                    }
                }
                crate::inject::set_default_template(&template)
            }
            SettingsTarget::Instance => {
                let mut over = crate::inject::get_override(instance_id)?;
                if inject_changed {
                    let mut merged = serde_json::Value::Object(over.settings.clone());
                    crate::inject::deep_merge(
                        &mut merged,
                        serde_json::Value::Object(settings.inject_settings.clone()),
                    );
                    over.settings = serde_json::from_value(merged).unwrap_or_default();
                }
                if let Some(a) = agents {
                    if a.instance.is_some() {
                        over.agents_md = a.instance.clone();
                    }
                }
                crate::inject::set_override(instance_id, &over)
            }
        }
    })();
    match write_result {
        Ok(()) => {
            if inject_changed {
                report.applied.push(match target {
                    SettingsTarget::Global => "注入配置（全局默认）",
                    SettingsTarget::Instance => "注入配置（实例覆盖）",
                }
                .to_string());
            }
            // 报告如实（2026-09-20 二轮实机发现）：AGENTS.md 按落点分半写入——
            // 只有对应那一半真的写了才计入 applied；另一半为空时改为可读说明，
            // 不再出现「报告已应用、文件未写」的误导。
            match agents.map(|a| match target {
                SettingsTarget::Global => a.global.is_some(),
                SettingsTarget::Instance => a.instance.is_some(),
            }) {
                Some(true) => report.applied.push("AGENTS.md".into()),
                Some(false) => {
                    let (present, landing, hint) = match target {
                        SettingsTarget::Global => ("版本级", "全局", "只写入下方所选版本"),
                        SettingsTarget::Instance => ("全局", "版本级", "写入全局默认"),
                    };
                    report.warnings.push(format!(
                        "包内 AGENTS.md 仅含{present}内容、当前落点为{landing}——未写入\
                         （如需应用，导入时改选「{hint}」）"
                    ));
                }
                None => {}
            }
            // 物化（settings.yaml/.env/AGENTS.md）：实例落点立即写盘；全局落点
            // 由各实例下次启动时统一物化，这里不逐实例改文件。
            if target == SettingsTarget::Instance {
                if let Err(e) = crate::inject::write_instance_files(instance_id) {
                    report.warnings.push(format!("实例配置物化失败：{e}"));
                }
            }
        }
        Err(e) => report.warnings.push(format!("注入配置写入失败：{e}")),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plugins_manifest() -> String {
        serde_json::to_string(&json!({
            "formatVersion": 1,
            "type": "plugins",
            "name": "我的全家桶",
            "generator": "dsh-launcher/test",
            "createdAt": "2026-09-12T00:00:00Z",
            "plugins": [
                { "name": "@deepseek-ai/dsh-base", "version": "1.0.0", "source": "npm",
                  "file": "packages/deepseek-ai-dsh-base-1.0.0.tgz" },
                { "name": "dsh-tui", "version": "0.2.1" }
            ]
        }))
        .unwrap()
    }

    #[test]
    fn valid_plugins_manifest_parses_and_validates() {
        let m = parse_and_validate(&plugins_manifest()).unwrap();
        assert_eq!(m.pack_type, PackType::Plugins);
        assert_eq!(m.plugins.len(), 2);
        assert_eq!(m.plugins[0].file, "packages/deepseek-ai-dsh-base-1.0.0.tgz");
    }

    #[test]
    fn settings_manifest_with_all_sections() {
        let text = serde_json::to_string(&json!({
            "formatVersion": 1,
            "type": "settings",
            "name": "我的配置",
            "settings": {
                "vendors": [
                    { "vendor": "deepseek", "enabled": true, "models": ["deepseek-v4-pro"] },
                    { "vendor": "custom-x", "enabled": false, "models": [], "address": "https://relay.example/v1" }
                ],
                "hotkeys": { "nav-launch": "Alt+9" },
                "injectSettings": { "llm-deepseek": { "thinking": "disabled" } },
                "agentsMd": { "global": "全局规则", "instance": "实例规则" }
            }
        }))
        .unwrap();
        let m = parse_and_validate(&text).unwrap();
        assert_eq!(m.pack_type, PackType::Settings);
        let s = m.settings.as_ref().unwrap();
        assert_eq!(s.vendors.len(), 2);
        assert_eq!(s.vendors[0].models, vec!["deepseek-v4-pro"]);
        assert_eq!(s.agents_md.as_ref().unwrap().global.as_deref(), Some("全局规则"));
    }

    #[test]
    fn mixed_type_accepted_when_both_present() {
        let text = serde_json::to_string(&json!({
            "formatVersion": 1,
            "type": "mixed",
            "name": "全家桶+配置",
            "plugins": [{ "name": "dsh-tui", "version": "0.2.1" }],
            "settings": { "hotkeys": { "nav-more": "Alt+4" } }
        }))
        .unwrap();
        let m = parse_and_validate(&text).unwrap();
        assert_eq!(m.pack_type, PackType::Mixed);
    }

    #[test]
    fn wrong_format_version_rejected() {
        let text = plugins_manifest().replace("\"formatVersion\":1", "\"formatVersion\":2");
        let err = parse_and_validate(&text).unwrap_err();
        assert!(err.contains("格式版本"), "实际：{err}");
    }

    #[test]
    fn type_mismatch_rejected() {
        // 只有插件却声明 settings（json! 序列化是紧凑形态，无空格）。
        let text = plugins_manifest().replace("\"type\":\"plugins\"", "\"type\":\"settings\"");
        let err = parse_and_validate(&text).unwrap_err();
        assert!(err.contains("类型不一致"), "实际：{err}");
    }

    #[test]
    fn empty_manifest_rejected() {
        let text = serde_json::to_string(&json!({
            "formatVersion": 1, "type": "plugins", "name": "空包"
        }))
        .unwrap();
        let err = parse_and_validate(&text).unwrap_err();
        assert!(err.contains("清单为空"), "实际：{err}");
    }

    #[test]
    fn duplicate_plugins_and_bad_semver_rejected() {
        let dup = serde_json::to_string(&json!({
            "formatVersion": 1, "type": "plugins", "name": "重复",
            "plugins": [
                { "name": "dsh-tui", "version": "0.2.1" },
                { "name": "dsh-tui", "version": "0.2.2" }
            ]
        }))
        .unwrap();
        assert!(parse_and_validate(&dup).unwrap_err().contains("重复列出"));

        let badver = serde_json::to_string(&json!({
            "formatVersion": 1, "type": "plugins", "name": "坏版本",
            "plugins": [{ "name": "dsh-tui", "version": "not.a.version" }]
        }))
        .unwrap();
        let err = parse_and_validate(&badver).unwrap_err();
        assert!(err.contains("版本号非法"), "实际：{err}");
    }

    #[test]
    fn tarball_path_traversal_rejected() {
        let text = serde_json::to_string(&json!({
            "formatVersion": 1, "type": "plugins", "name": "穿越",
            "plugins": [{ "name": "dsh-tui", "version": "0.2.1", "file": "packages/../evil.tgz" }]
        }))
        .unwrap();
        let err = parse_and_validate(&text).unwrap_err();
        assert!(err.contains("路径非法"), "实际：{err}");
    }

    #[test]
    fn tarball_rel_path_matches_npm_pack_convention() {
        assert_eq!(
            plugin_tarball_rel_path("@deepseek-ai/dsh-base", "1.0.0"),
            "packages/deepseek-ai-dsh-base-1.0.0.tgz"
        );
        assert_eq!(plugin_tarball_rel_path("dsh-tui", "0.2.1"), "packages/dsh-tui-0.2.1.tgz");
    }

    #[test]
    fn derive_covers_all_shapes() {
        assert_eq!(PackType::derive(true, false), PackType::Plugins);
        assert_eq!(PackType::derive(false, true), PackType::Settings);
        assert_eq!(PackType::derive(true, true), PackType::Mixed);
    }

    // ===== 导出/导入管线（离线：tgz 用 write_tgz 手工组装，npm pack 走 ignored e2e）=====

    use std::path::PathBuf;

    /// 手工组装一个最小整合包（假 tarball = 任意文件内容，离线测容器与清单链）。
    fn build_pack(dir: &std::path::Path, manifest: &Manifest, files: &[(&str, &[u8])]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let staged: Vec<(PathBuf, String)> = files
            .iter()
            .map(|(rel, data)| {
                let p = dir.join(format!("staged-{}", rel.replace('/', "-")));
                std::fs::write(&p, data).unwrap();
                (p, rel.to_string())
            })
            .collect();
        let dest = dir.join("pack.tgz");
        write_tgz(manifest, &staged, &dest).unwrap();
        dest
    }

    #[test]
    fn tgz_roundtrip_and_peek() {
        let base = std::env::temp_dir().join(format!("dsh-modpack-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let manifest: Manifest =
            serde_json::from_str(&plugins_manifest()).unwrap();
        let dest = build_pack(
            &base,
            &manifest,
            &[("packages/dsh-tui-0.2.1.tgz", b"fake-tarball".as_slice())],
        );
        assert!(dest.is_file());

        let peeked = peek_manifest(&dest).unwrap();
        assert_eq!(peeked.name, manifest.name);
        assert_eq!(peeked.plugins.len(), manifest.plugins.len());

        // 解包后条目落在声明路径上（含 packages/ 子目录）。
        let out = base.join("out");
        extract_tgz_to(&dest, &out).unwrap();
        assert!(out.join("manifest.json").is_file());
        assert!(out.join("packages").join("dsh-tui-0.2.1.tgz").is_file());

        // 清单被篡改（换空包名）→ peek fail loud。
        let mut evil = manifest.clone();
        evil.name = " ".into();
        let evil_dest = build_pack(&base.join("evil"), &evil, &[]);
        let err = peek_manifest(&evil_dest).unwrap_err();
        assert!(err.contains("包名") || err.contains("清单"), "实际：{err}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn settings_only_import_applies_by_target() {
        let (_lock, tmp) = crate::instances::tests::sandbox("modpack-import");
        let instance = crate::instances::create_instance("设置包导入", "headless").unwrap();

        let manifest: Manifest = serde_json::from_value(json!({
            "formatVersion": 1, "type": "settings", "name": "纯设置",
            "settings": {
                "hotkeys": { "nav-launch": "Alt+9" },
                "injectSettings": { "llm-deepseek": { "thinking": "disabled" } },
                "agentsMd": { "global": "全局规则", "instance": "实例规则" }
            }
        }))
        .unwrap();
        let base = tmp.join("packsrc");
        let dest = build_pack(&base, &manifest, &[]);

        // 全局落点：注入进默认模板、全局 AGENTS.md 落根文件、热键合并。
        let mut progress_sink = |_d: String| {};
        let report = import_pack(&dest, &instance.id, Some(SettingsTarget::Global), &mut progress_sink)
            .unwrap();
        assert!(report.applied.contains(&"注入配置（全局默认）".to_string()));
        assert!(report.applied.contains(&"AGENTS.md".to_string()));
        assert!(report.applied.iter().any(|a| a.contains("热键")));
        let template = crate::inject::get_default_template().unwrap();
        assert!(template.settings.contains_key("llm-deepseek"));
        assert_eq!(
            crate::inject::get_global_agents_md().unwrap().as_deref(),
            Some("全局规则")
        );
        assert_eq!(
            crate::hotkey_store::load().unwrap().get("nav-launch").map(String::as_str),
            Some("Alt+9")
        );

        // 实例落点：覆盖合并 + 立即物化（settings.yaml 与实例 AGENTS.md）。
        let report = import_pack(&dest, &instance.id, Some(SettingsTarget::Instance), &mut progress_sink)
            .unwrap();
        let dir = crate::instances::instance_dir(&instance.id).unwrap();
        let yaml = std::fs::read_to_string(dir.join("settings.yaml")).unwrap();
        assert!(yaml.contains("disabled"), "实例 settings.yaml 应含合并结果");
        assert_eq!(
            std::fs::read_to_string(dir.join("AGENTS.md")).unwrap(),
            "实例规则"
        );
        assert!(report.warnings.is_empty());
        crate::instances::delete_instance(&instance.id).unwrap();
    }

    #[test]
    fn single_produced_tgz_requires_exactly_one() {
        let base = std::env::temp_dir().join(format!("dsh-modpack-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        // 0 个：暂存目录为空 → 通道产物异常。
        assert!(single_produced_tgz(&base).is_err());

        // 1 个：唯一 tgz 即产物（非 tgz 文件不参与计数）。
        std::fs::write(base.join("pkg-1.0.0.tgz"), b"x").unwrap();
        std::fs::write(base.join("npm-output.log"), b"stdout noise").unwrap();
        let got = single_produced_tgz(&base).unwrap();
        assert_eq!(got.file_name().unwrap().to_string_lossy(), "pkg-1.0.0.tgz");

        // 2 个：前一插件的残留即污染判定（多插件共用暂存目录的旧缺陷形态，
        // 审查 P1-01）——判定层必须报异常，由调用方的每插件子目录规避。
        std::fs::write(base.join("other-2.0.0.tgz"), b"y").unwrap();
        let err = single_produced_tgz(&base).unwrap_err();
        assert!(err.contains("实际 2 个"), "实际：{err}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn extract_tgz_rejects_symlink_entries() {
        let base = std::env::temp_dir().join(format!("dsh-modpack-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let tgz = base.join("evil.tgz");
        let file = std::fs::File::create(&tgz).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        // 符号链接条目指向包外：unpack 语义会原样落链，必须在条目类型层拒绝。
        // （append_link 不代设类型，须显式 Symlink；`..` 只允许出现在链接
        // 目标里——归档路径本身被 tar-rs 拒绝含 `..`。）
        let mut link_header = tar::Header::new_gnu();
        link_header.set_entry_type(tar::EntryType::Symlink);
        link_header.set_size(0);
        link_header.set_mode(0o777);
        link_header.set_cksum();
        builder
            .append_link(
                &mut link_header,
                "packages/evil.tgz",
                "../../../outside.txt",
            )
            .unwrap();
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();

        let out = base.join("out");
        let err = extract_tgz_to(&tgz, &out).unwrap_err();
        assert!(err.contains("非法条目类型"), "实际：{err}");
        assert!(
            !out.join("packages").join("evil.tgz").exists(),
            "符号链接条目不得落盘"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn settings_only_import_needs_no_instance_for_global_target() {
        let (_lock, tmp) = crate::instances::tests::sandbox("modpack-noinstance");
        // 全新启动器：一个实例都没有（审查 P1-02 回归——纯设置包 + 全局落点
        // 是「分享配置给还没装启动器的人」的核心路径）。
        let manifest: Manifest = serde_json::from_value(json!({
            "formatVersion": 1, "type": "settings", "name": "新机器配置",
            "settings": {
                "hotkeys": { "nav-launch": "Alt+9" },
                "agentsMd": { "global": "全局规则" }
            }
        }))
        .unwrap();
        let dest = build_pack(&tmp.join("packsrc"), &manifest, &[]);
        let mut sink = |_d: String| {};

        let report = import_pack(&dest, "", Some(SettingsTarget::Global), &mut sink).unwrap();
        assert!(report.applied.contains(&"AGENTS.md".to_string()));
        assert!(report.applied.iter().any(|a| a.contains("热键")));
        assert_eq!(
            crate::inject::get_global_agents_md().unwrap().as_deref(),
            Some("全局规则")
        );
        assert_eq!(
            crate::hotkey_store::load().unwrap().get("nav-launch").map(String::as_str),
            Some("Alt+9")
        );

        // Instance 落点仍要求实例存在，fail loud 可读。
        let err = import_pack(&dest, "", Some(SettingsTarget::Instance), &mut sink).unwrap_err();
        assert!(err.contains("实例不存在"), "实际：{err}");
        crate::inject::set_global_agents_md(None).unwrap();
    }

    /// 报告如实（2026-09-20 二轮实机发现）：仅含全局内容的设置包走实例落点时，
    /// 不得申报「AGENTS.md」已应用（实际未写），而应给出可读说明；同一包走
    /// 全局落点则如实申报并写入。
    #[test]
    fn agents_md_report_matches_actual_landing() {
        let (_lock, tmp) = crate::instances::tests::sandbox("modpack-agents-report");
        let instance = crate::instances::create_instance("落点报告", "headless").unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "formatVersion": 1, "type": "settings", "name": "仅全局",
            "settings": { "agentsMd": { "global": "仅全局指令" } }
        }))
        .unwrap();
        let dest = build_pack(&tmp.join("packsrc"), &manifest, &[]);
        let mut sink = |_d: String| {};

        // 实例落点：不申报、给说明、文件确未写。
        let report =
            import_pack(&dest, &instance.id, Some(SettingsTarget::Instance), &mut sink).unwrap();
        assert!(
            !report.applied.iter().any(|a| a == "AGENTS.md"),
            "实际未写不得申报：{:?}",
            report.applied
        );
        assert!(
            report.warnings.iter().any(|w| w.contains("未写入") && w.contains("全局")),
            "应给可读说明：{:?}",
            report.warnings
        );
        let dir = crate::instances::instance_dir(&instance.id).unwrap();
        assert!(!dir.join("AGENTS.md").exists(), "实例 AGENTS.md 不得被写");
        assert!(crate::inject::get_override(&instance.id).unwrap().agents_md.is_none());

        // 全局落点：如实申报，且根文件与实例物化到位。
        let report =
            import_pack(&dest, &instance.id, Some(SettingsTarget::Global), &mut sink).unwrap();
        assert!(report.applied.iter().any(|a| a == "AGENTS.md"), "{:?}", report.applied);
        assert!(!report.warnings.iter().any(|w| w.contains("未写入")));
        assert_eq!(
            crate::inject::get_global_agents_md().unwrap().as_deref(),
            Some("仅全局指令")
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("AGENTS.md")).unwrap(),
            "仅全局指令"
        );
        crate::inject::set_global_agents_md(None).unwrap();
        crate::instances::delete_instance(&instance.id).unwrap();
    }

    #[test]
    fn import_missing_tarball_rolls_back_with_readable_error() {
        let (_lock, tmp) = crate::instances::tests::sandbox("modpack-rollback");
        let instance = crate::instances::create_instance("回滚", "headless").unwrap();

        // 声明了 tarball 但包里没带（坏包 fixture）——安装未开始即失败，
        // 报错必须可读且说明无需回滚。
        let manifest: Manifest = serde_json::from_value(json!({
            "formatVersion": 1, "type": "plugins", "name": "坏包",
            "plugins": [{ "name": "dsh-tui", "version": "0.2.1",
                          "file": "packages/dsh-tui-0.2.1.tgz" }]
        }))
        .unwrap();
        let base = tmp.join("packsrc");
        let dest = build_pack(&base, &manifest, &[]);
        let mut progress_sink = |_d: String| {};
        let err = import_pack(&dest, &instance.id, None, &mut progress_sink).unwrap_err();
        assert!(err.contains("导入失败"), "实际：{err}");
        assert!(err.contains("缺少"), "实际：{err}");
        assert!(err.contains("无需回滚"), "实际：{err}");
        crate::instances::delete_instance(&instance.id).unwrap();
    }

    #[test]
    fn export_settings_only_offline_roundtrip() {
        let (_lock, tmp) = crate::instances::tests::sandbox("modpack-export");
        let instance = crate::instances::create_instance("设置导出", "headless").unwrap();
        crate::hotkey_store::save(&{
            let mut m = std::collections::BTreeMap::new();
            m.insert("nav-more".to_string(), "Alt+4".to_string());
            m
        })
        .unwrap();
        crate::inject::set_global_agents_md(Some("全局指令".into())).unwrap();

        let opts = ExportOptions {
            instance_id: instance.id.clone(),
            name: "我的配置包".into(),
            plugins: vec![],
            include_vendors: false,
            include_hotkeys: true,
            include_inject: false,
            include_agents_md: true,
        };
        let dest = tmp.join("out").join("我的配置包.tgz");
        let manifest = export_pack(&opts, &dest, &mut |_d| {}).unwrap();
        assert_eq!(manifest.pack_type, PackType::Settings);
        assert!(dest.is_file(), "导出产物应存在：{}", dest.display());
        let peeked = peek_manifest(&dest).unwrap();
        assert_eq!(peeked.settings.as_ref().unwrap().hotkeys.len(), 1);
        assert_eq!(
            peeked.settings.as_ref().unwrap().agents_md.as_ref().unwrap().global.as_deref(),
            Some("全局指令")
        );

        // 勾选了不存在于第三方清单的插件 → fail loud。
        let bad = ExportOptions { plugins: vec!["no-such-pkg".into()], ..opts };
        let err = export_pack(&bad, &tmp.join("bad.tgz"), &mut |_d| {}).unwrap_err();
        assert!(err.contains("第三方已装清单"), "实际：{err}");

        crate::inject::set_global_agents_md(None).unwrap();
        crate::instances::delete_instance(&instance.id).unwrap();
    }

    /// 真实链路 e2e（显式触发，与 market_install 真链测试同约定：本机需有一个
    /// 已装 DSH 的实例）：两个本地目录离线 `npm pack` 出真 tarball → 组双插件包
    /// → 导入安装（file: 链）→ bundles 生效比对 → 官方链清场。双包是审查
    /// P1-01 的回归形态（单包覆盖不了多插件路径）。默认 `#[ignore]`。
    #[test]
    #[ignore = "真实实例：双插件 file: 离线安装 + 导入全环"]
    fn real_export_import_with_local_npm_pack() {
        let all = crate::instances::list_instances().expect("实例注册表可读");
        let target = all
            .iter()
            .find(|i| i.dsh_version.is_some())
            .expect("本机需要一个已安装 DSH 的实例（先在下载页安装）")
            .clone();
        println!("e2e 目标实例：{}（profile {}）", target.id, target.profile);
        // 与应用启动同路径：旧 instances/ 目录若还在，这里完成一次性迁移，
        // 否则 instance_dir() 指向还不存在的 versions/<id>。
        crate::paths::ensure_managed_dirs().expect("托管根初始化/迁移");
        let tmp = std::env::temp_dir().join(format!("dsh-modpack-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // 1) 两个本地假插件包（无依赖，离线可装；声明 dsh.bundle.patch 使其进 bundles）。
        //    每包独立暂存子目录，与 export_pack_inner 的每插件暂存语义一致。
        let node_exe = which_node().expect("本机需有 node 在 PATH 上");
        let npm_cli = crate::dsh_install::npm_cli_of(&node_exe).unwrap();
        let mut staged: Vec<(std::path::PathBuf, String)> = Vec::new();
        let mut entries: Vec<PluginEntry> = Vec::new();
        for (name, version) in [("local-dsh-noop", "0.0.1"), ("local-dsh-noop-2", "0.0.2")] {
            let pkg = tmp.join(name);
            std::fs::create_dir_all(&pkg).unwrap();
            std::fs::write(
                pkg.join("package.json"),
                format!(
                    r#"{{
  "name": "{name}",
  "version": "{version}",
  "dsh": {{ "bundle": {{ "patch": "./cordis.patch.yml" }} }}
}}"#
                ),
            )
            .unwrap();
            std::fs::write(pkg.join("cordis.patch.yml"), "services: {}\n").unwrap();

            // npm pack（离线：本地目录参数）。
            let pack_dir = tmp.join(format!("pack-out-{name}"));
            std::fs::create_dir_all(&pack_dir).unwrap();
            let tgz = npm_pack(
                &node_exe,
                &npm_cli,
                pkg.to_string_lossy().as_ref(),
                &pack_dir,
                &mut |_d| {},
            )
            .unwrap();
            let rel = plugin_tarball_rel_path(name, version);
            staged.push((tgz, rel.clone()));
            entries.push(PluginEntry {
                name: name.into(),
                version: version.into(),
                source: "npm".into(),
                file: rel,
            });
        }

        // 2) 组包（导出的 npm registry 链另有走查覆盖；这里直接以本地 tarball 为 staged 源）。
        let manifest = Manifest {
            format_version: FORMAT_VERSION,
            pack_type: PackType::Plugins,
            name: "本地离线包".into(),
            generator: "test".into(),
            created_at: crate::instances::now_rfc3339(),
            plugins: entries,
            settings: None,
        };
        let dest = tmp.join("local.tgz");
        write_tgz(&manifest, &staged, &dest).unwrap();

        // 3) 导入：双插件 file: 安装链 + bundles 生效比对。
        let mut sink = |d: String| println!("  [进度] {d}");
        let report = import_pack(&dest, &target.id, None, &mut sink).unwrap();
        assert_eq!(report.installed.len(), 2, "双插件应全部装齐");
        assert!(
            report
                .installed
                .iter()
                .all(|o| o.package_name.starts_with("local-dsh-noop") && o.bundle_activated),
            "两个插件都应进入 bundles 层：{:?}",
            report.installed
        );
        println!("导入全环通过：双包 file: 离线安装 → bundles 生效");

        // 3.5) imported/ 留档与 spec 持久化（审查 P2-07 修复实证）。
        let instance_dir = crate::instances::instance_dir(&target.id).unwrap();
        let imported_dir = instance_dir.join("imported");
        assert!(
            imported_dir.join("local-dsh-noop-0.0.1.tgz").is_file()
                && imported_dir.join("local-dsh-noop-2-0.0.2.tgz").is_file(),
            "两个 tarball 都应留档 imported/"
        );
        let deps_text = std::fs::read_to_string(
            instance_dir.join("profiles").join(&target.profile).join("package.json"),
        )
        .unwrap();
        assert!(
            deps_text.starts_with('{')
                && deps_text.contains("\"file:")
                && deps_text.contains("imported"),
            "依赖 spec 应以 file: 指向 imported/ 留档：{deps_text}"
        );

        // 4) 清场：官方链逐个移除（remove 会重链 file: 依赖——留档缺失在此即炸，
        // 这正是 2026-09-20 首跑暴露的缺陷形态，现由 imported/ 兜住）。
        for name in ["local-dsh-noop", "local-dsh-noop-2"] {
            crate::plugin_manage::remove(&target.id, name, &mut sink).expect("清场：官方链移除");
        }
        let _ = std::fs::remove_dir_all(&imported_dir);
    }

    /// PATH 上找 node.exe（测试辅助；找不到返回 None）。
    fn which_node() -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("node.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
}
