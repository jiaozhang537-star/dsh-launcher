//! 配置注入引擎：把启动器侧设置写成 DSH 原生配置文件。
//!
//! R-1 侦察结论（见 phase-01 日志）：DSH 从 `$DSH_HOME/settings.yaml`
//! 读用户设置（按插件命名空间分节、热重载）；`.env` 仅承载普通环境变量，
//! 且 PATH/HOME/USERPROFILE/SHELL/NODE_OPTIONS/NODE_PATH 与 `DSH_*`/`XDG_*`/
//! `DYLD_*`/`BASH_FUNC_*` 前缀是 bootstrap-only，出现在 .env 里会让 DSH
//! fail loud 拒绝启动——本引擎写盘前先做同样校验，把错误留在写盘这一步。
//!
//! 数据模型：全局默认模板（`<managed root>/inject-default.json`）与实例覆盖
//! （`<instance dir>/inject.json`）都是 `{ settings: <任意 JSON 对象>,
//! env: {名: 值} }`；生效值 = 默认与覆盖的深层合并（覆盖优先）。产物只写
//! settings.yaml 与 .env 两个文件，均为「临时文件 + 改名」原子落盘，任何一步
//! 失败都不产生半成品。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use crate::instances;
use crate::paths;

/// inject.json / inject-default.json 读-改-写的进程内互斥（对抗审查 R2 P2-03）：
/// `llm_bridge::sync_instance` 的「读模板 → 改路由 → 写回」与设置页保存并发时，
/// 无锁会让后写者拿旧快照覆盖新保存（丢更新）。与 instances 的 REGISTRY_LOCK
/// 无嵌套（inject 侧对实例注册表只做无锁读 get_instance），无死锁面。
/// **非重入**——持锁期间必须调 `_locked` 内核，不得再进持锁公开壳。
static INJECT_IO_LOCK: Mutex<()> = Mutex::new(());

/// 取 inject 文件 IO 锁。crate 内复用（llm_bridge 的读-改-写链）。
pub(crate) fn inject_io_lock() -> MutexGuard<'static, ()> {
    INJECT_IO_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 一份可注入配置（默认模板与实例覆盖共用此结构）。
/// IPC 键为 camelCase（前端 services/instances.ts 同名视图）——2026-09-20
/// 实机走查实证：缺 rename 时前端发的 `agentsMd` 被静默丢弃，版本级覆盖
/// 永远存不进去（单测走 Rust 直调暴露不了 IPC 键型）。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectConfig {
    /// settings.yaml 的内容：顶层键 = DSH 插件命名空间（如 `llm-deepseek`）。
    #[serde(default)]
    pub settings: Map<String, Value>,
    /// 写入实例 `.env` 的普通环境变量。
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 实例级 AGENTS.md 覆盖（版本设置页输入框落点）。None = 继承全局默认。
    /// alias 保留旧蛇形键的读取兼容（手工编辑过的存量文件）。
    #[serde(default, alias = "agents_md", skip_serializing_if = "Option::is_none")]
    pub agents_md: Option<String>,
}

/// DSH bootstrap-only 变量精确名单（镜像 @deepseek-ai/dsh-app-boot）。
const BOOTSTRAP_NAMES: &[&str] = &["PATH", "HOME", "USERPROFILE", "SHELL", "NODE_OPTIONS", "NODE_PATH"];

/// DSH bootstrap-only 变量前缀（同上，大小写不敏感）。
const BOOTSTRAP_PREFIXES: &[&str] = &["DSH_", "XDG_", "DYLD_", "BASH_FUNC_"];

/// 环境变量名是否为 DSH 禁止经 .env 设置的 bootstrap-only 名。
pub fn is_bootstrap_only(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    BOOTSTRAP_NAMES.contains(&upper.as_str())
        || BOOTSTRAP_PREFIXES.iter().any(|p| upper.starts_with(p))
}

/// 校验 env 名单：空名或 bootstrap-only 名返回 Err；值含换行同样拒绝
/// （对抗审查 R2 P2-06）：.env 是 KEY=VALUE 逐行格式，值里的换行会让
/// 「\nEVIL=1」成为独立变量行——若夹带 bootstrap-only 名，DSH fail loud
/// 拒绝启动，而 UI 报错不会指向真正的肇因（一个换行）。
pub fn validate_env(env: &BTreeMap<String, String>) -> Result<(), String> {
    for (name, value) in env {
        if name.trim().is_empty() {
            return Err("环境变量名不能为空白".into());
        }
        if is_bootstrap_only(name) {
            return Err(format!(
                "{name} 是 DSH 的 bootstrap-only 变量，只能由启动器进程环境注入，不能写入 .env"
            ));
        }
        if value.contains('\n') || value.contains('\r') {
            return Err(format!(
                "环境变量 {name} 的值包含换行——.env 按行解析，多行值会破坏格式；请去掉换行或改用进程环境注入"
            ));
        }
    }
    Ok(())
}

/// 深层合并 b 到 a（b 优先）：对象递归合并，其余类型整体替换。
pub fn deep_merge(a: &mut Value, b: Value) {
    match (a, b) {
        (Value::Object(a_map), Value::Object(b_map)) => {
            for (key, b_val) in b_map {
                match a_map.get_mut(&key) {
                    Some(a_val) => deep_merge(a_val, b_val),
                    None => {
                        a_map.insert(key, b_val);
                    }
                }
            }
        }
        (a_slot, b_val) => *a_slot = b_val,
    }
}

fn inject_default_file() -> Result<PathBuf, String> {
    Ok(paths::managed_root()?.join("inject-default.json"))
}

fn instance_inject_file(id: &str) -> Result<PathBuf, String> {
    Ok(instances::instance_dir(id)?.join("inject.json"))
}

/// 读一份 InjectConfig；文件缺失视为空配置。
/// 读路径单点做遗留路由提供方重映射（撞名合并 2026-10-03）：旧厂商
/// zai/moonshotai 的 `agent-default-model.provider` 改指合并后的卡——
/// precheck 直读本函数（get_override）、物化走 effective_config，单点重映射
/// 才能让两边视图一致；幂等、不回写磁盘。
fn read_inject_file(path: PathBuf) -> Result<InjectConfig, String> {
    if !path.exists() {
        return Ok(InjectConfig::default());
    }
    let raw =
        std::fs::read_to_string(&path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    let mut config: InjectConfig =
        serde_json::from_str(&raw).map_err(|e| format!("解析 {} 失败：{e}", path.display()))?;
    crate::llm_bridge::remap_legacy_route_provider(&mut config.settings);
    Ok(config)
}

/// 原子写一份 InjectConfig（临时文件 + 改名）。
fn write_inject_file(path: PathBuf, config: &InjectConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建 {} 失败：{e}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(config).map_err(|e| format!("序列化失败：{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body + "\n").map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 {} 失败：{e}", path.display()))?;
    Ok(())
}

/// 读全局默认模板。
pub fn get_default_template() -> Result<InjectConfig, String> {
    read_inject_file(inject_default_file()?)
}

/// 写全局默认模板（先校验 env 名单）。
pub fn set_default_template(config: &InjectConfig) -> Result<(), String> {
    let _guard = inject_io_lock();
    set_default_template_locked(config)
}

/// [`set_default_template`] 的无锁内核：供已持 [`INJECT_IO_LOCK`] 的
/// 读-改-写链（llm_bridge::sync_instance）复用，避免重入死锁。
pub(crate) fn set_default_template_locked(config: &InjectConfig) -> Result<(), String> {
    validate_env(&config.env)?;
    write_inject_file(inject_default_file()?, config)
}

/// 读实例覆盖。
pub fn get_override(id: &str) -> Result<InjectConfig, String> {
    instances::get_instance(id)?;
    read_inject_file(instance_inject_file(id)?)
}

/// 写实例覆盖（先校验 env 名单；实例必须已存在）。保存后**立即物化**
/// AGENTS.md（DSH 每轮对话重读该文件，运行中实例下一条消息即生效）。
pub fn set_override(id: &str, config: &InjectConfig) -> Result<(), String> {
    let _guard = inject_io_lock();
    set_override_locked(id, config)
}

/// [`set_override`] 的无锁内核：同 [`set_default_template_locked`]。
pub(crate) fn set_override_locked(id: &str, config: &InjectConfig) -> Result<(), String> {
    instances::get_instance(id)?;
    validate_env(&config.env)?;
    write_inject_file(instance_inject_file(id)?, config)?;
    materialize_agents_md(id)?;
    Ok(())
}

/// 计算生效配置 = 全局默认 ⊕ 实例覆盖（覆盖优先）。
/// FR-17 共享模式（isolation=false）：配置回退全局单源——实例覆盖
///（settings / env / AGENTS.md）不参与，覆盖文件原样保留、重新隔离后自动恢复。
pub fn effective_config(id: &str) -> Result<InjectConfig, String> {
    let instance = instances::get_instance(id)?;
    let mut merged = get_default_template()?;
    if !instance.isolation {
        return Ok(merged);
    }
    let over = get_override(id)?;
    let mut settings = Value::Object(merged.settings.clone());
    deep_merge(&mut settings, Value::Object(over.settings));
    merged.settings = match settings {
        Value::Object(map) => map,
        _ => Map::new(), // 深合并以对象为根，此处不可达；防御式兜底
    };
    for (name, value) in over.env {
        merged.env.insert(name, value);
    }
    // AGENTS.md 不走深合并：覆盖要么整体自设、要么整体继承（None = 继承全局）。
    merged.agents_md = over.agents_md;
    Ok(merged)
}

/// 全局 AGENTS.md 文件（FILE-LAYOUT：全局输入框的落点即存储本体）。
pub fn global_agents_md_path() -> Result<PathBuf, String> {
    paths::global_agents_md_file()
}

/// 读全局 AGENTS.md 内容；文件缺失 = 未设置（None）。
pub fn get_global_agents_md() -> Result<Option<String>, String> {
    let path = global_agents_md_path()?;
    if !path.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|e| format!("读取 {} 失败：{e}", path.display()))
}

/// 写全局 AGENTS.md；None = 删除（未设置）。原子落盘。
/// 保存后对所有「未设版本级覆盖」的实例**立即物化**——DSH 每轮对话重读
/// `$DSH_HOME/AGENTS.md`（侦察实证：无文件监听但按轮询版本，改文件下一条
/// 消息即生效），因此全局改动即时可见，无需重启。单实例物化失败不阻塞保存
/// （best-effort，该实例下次启动会再物化）。
pub fn set_global_agents_md(content: Option<&str>) -> Result<(), String> {
    let path = global_agents_md_path()?;
    match content {
        None => {
            if path.exists() {
                std::fs::remove_file(&path)
                    .map_err(|e| format!("删除 {} 失败：{e}", path.display()))?;
            }
        }
        Some(text) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录 {} 失败：{e}", parent.display()))?;
            }
            let tmp = path.with_extension("md.tmp-writing");
            std::fs::write(&tmp, text).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
            std::fs::rename(&tmp, &path)
                .map_err(|e| format!("替换 {} 失败：{e}", path.display()))?;
        }
    }
    for instance in instances::list_instances()? {
        if get_override(&instance.id)?.agents_md.is_none() {
            let _ = materialize_agents_md(&instance.id);
        }
    }
    Ok(())
}

/// 实例生效的 AGENTS.md：实例覆盖优先，否则继承全局默认。
pub fn effective_agents_md(id: &str) -> Result<Option<String>, String> {
    instances::get_instance(id)?;
    if let Some(text) = get_override(id)?.agents_md {
        return Ok(Some(text));
    }
    get_global_agents_md()
}

/// 把生效的 AGENTS.md 物化到 `<instance_dir>/AGENTS.md`：有内容 → 原子重写；
/// 生效为空 → 删除存量（启动器拥有该文件生命周期：残留旧指令会被 DSH 继续
/// 读取，"清空=没有指令" 必须落实为删文件）。返回写入路径（删除/无内容 = None）。
fn materialize_agents_md(id: &str) -> Result<Option<PathBuf>, String> {
    let dir = instances::instance_dir(id)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 {} 失败：{e}", dir.display()))?;
    let target = dir.join("AGENTS.md");
    match effective_agents_md(id)? {
        Some(text) => {
            let tmp = target.with_extension("md.tmp-writing");
            std::fs::write(&tmp, &text)
                .map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
            std::fs::rename(&tmp, &target)
                .map_err(|e| format!("替换 {} 失败：{e}", target.display()))?;
            Ok(Some(target))
        }
        None => {
            if target.exists() {
                std::fs::remove_file(&target)
                    .map_err(|e| format!("删除 {} 失败：{e}", target.display()))?;
            }
            Ok(None)
        }
    }
}

/// 渲染 settings.yaml 文本（JSON → YAML；空对象产出空映射文档）。
pub fn render_settings_yaml(settings: &Map<String, Value>) -> Result<String, String> {
    let yaml =
        serde_yaml::to_string(&Value::Object(settings.clone())).map_err(|e| format!("渲染 YAML 失败：{e}"))?;
    // 回读校验：产物必须能无损解析回等价 JSON，否则视为引擎缺陷拒绝落盘。
    let roundtrip: Value =
        serde_yaml::from_str(&yaml).map_err(|e| format!("YAML 回读失败：{e}"))?;
    if roundtrip != Value::Object(settings.clone()) {
        return Err("YAML 回读与源数据不等价，拒绝落盘".into());
    }
    Ok(yaml)
}

/// 渲染 .env 文本（KEY=VALUE 行，按名称排序保证确定性输出）。
pub fn render_env(env: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for (name, value) in env {
        out.push_str(&format!("{name}={value}\n"));
    }
    out
}

/// 把生效配置写入实例目录：settings.yaml + .env + AGENTS.md（若生效内容非空）。
/// 原子性口径：**单文件**临时写 + 改名（写盘中断不留坏文件）；三文件组并非
/// 原子——后一步失败时前面的文件已替换（如 AGENTS.md 物化失败而
/// settings/.env 已更新），内容都是合法产物，下次物化/启动自愈。
pub fn write_instance_files(id: &str) -> Result<Vec<PathBuf>, String> {
    let config = effective_config(id)?;
    validate_env(&config.env)?;
    let dir = instances::instance_dir(id)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 {} 失败：{e}", dir.display()))?;

    let yaml = render_settings_yaml(&config.settings)?;
    let env_text = render_env(&config.env);

    let targets = [
        (dir.join("settings.yaml"), yaml),
        (dir.join(".env"), env_text),
    ];
    let mut written = Vec::with_capacity(targets.len() + 1);
    for (target, body) in &targets {
        let tmp = target.with_extension("tmp-writing");
        std::fs::write(&tmp, body).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
        std::fs::rename(&tmp, target).map_err(|e| format!("替换 {} 失败：{e}", target.display()))?;
        written.push(target.clone());
    }

    // AGENTS.md 物化（FILE-LAYOUT）：实例覆盖优先、否则全局默认；生效为空时
    // 删除存量文件（残留旧指令会被 DSH 继续读取——"清空=没有指令"）。
    if materialize_agents_md(id)?.is_some() {
        written.push(dir.join("AGENTS.md"));
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        crate::instances::tests::sandbox(&format!("inject-{tag}"))
    }

    #[test]
    fn merge_is_deep_and_override_wins() {
        let mut base = json!({
            "llm-deepseek": { "thinking": "enabled", "nested": { "a": 1, "b": 2 } },
            "other": true
        });
        let over = json!({
            "llm-deepseek": { "thinking": "disabled", "nested": { "b": 9 } }
        });
        deep_merge(&mut base, over);
        assert_eq!(
            base,
            json!({
                "llm-deepseek": { "thinking": "disabled", "nested": { "a": 1, "b": 9 } },
                "other": true
            })
        );
    }

    #[test]
    fn bootstrap_names_rejected_case_insensitive() {
        let mut env = BTreeMap::new();
        env.insert("DSH_HOME".to_string(), "x".to_string());
        assert!(validate_env(&env).is_err());

        let mut env = BTreeMap::new();
        env.insert("dsh_permission_mode".to_string(), "ask".to_string());
        assert!(validate_env(&env).is_err(), "前缀校验必须大小写不敏感");

        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), "/x".to_string());
        assert!(validate_env(&env).is_err());

        let mut env = BTreeMap::new();
        env.insert("MY_APP_FLAG".to_string(), "1".to_string());
        assert!(validate_env(&env).is_ok());
    }

    /// 值含换行拒绝（对抗审查 R2 P2-06）：换行会让夹带的 KEY=VALUE 成为独立
    /// 变量行，破坏 .env 结构；报错必须点名肇因变量。
    #[test]
    fn env_values_with_newline_are_rejected_by_name() {
        let mut env = BTreeMap::new();
        env.insert("MY_VAR".to_string(), "line1\nEVIL=1".to_string());
        let err = validate_env(&env).unwrap_err();
        assert!(err.contains("MY_VAR"), "报错必须点名变量：{err}");

        let mut env = BTreeMap::new();
        env.insert("MY_VAR".to_string(), "line1\r\nline2".to_string());
        assert!(validate_env(&env).is_err(), "\\r\\n 同样拒绝");

        let mut env = BTreeMap::new();
        env.insert("OK_VAR".to_string(), "plain value with spaces=#not-comment".to_string());
        assert!(validate_env(&env).is_ok(), "无换行的普通值放行");
    }

    #[test]
    fn settings_yaml_roundtrip_exact() {
        let mut settings = Map::new();
        settings.insert(
            "llm-deepseek".into(),
            json!({ "baseURL": "https://api.deepseek.com/v1", "thinking": "disabled" }),
        );
        settings.insert(
            "agent-default-model".into(),
            json!({ "provider": "deepseek-official", "model": "deepseek-v4-pro" }),
        );
        let yaml = render_settings_yaml(&settings).unwrap();
        assert!(yaml.contains("llm-deepseek"));
        assert!(yaml.contains("deepseek-v4-pro"));
    }

    #[test]
    fn env_render_sorted_and_deterministic() {
        let mut env = BTreeMap::new();
        env.insert("Z_LAST".into(), "1".into());
        env.insert("A_FIRST".into(), "2".into());
        assert_eq!(render_env(&env), "A_FIRST=2\nZ_LAST=1\n");
    }

    #[test]
    fn full_pipeline_merge_and_files_on_disk() {
        let (_lock, _tmp) = sandbox("pipe");
        let instance = instances::create_instance("注入管线", "headless").unwrap();

        set_default_template(&InjectConfig {
            settings: serde_json::from_value(json!({
                "llm-deepseek": { "thinking": "enabled" },
                "agent-default-model": { "model": "deepseek-v4-flash" }
            }))
            .unwrap(),
            env: BTreeMap::new(),
            agents_md: None,
        })
        .unwrap();

        set_override(
            &instance.id,
            &InjectConfig {
                settings: serde_json::from_value(json!({
                    "llm-deepseek": { "thinking": "disabled" }
                }))
                .unwrap(),
                env: BTreeMap::from([("MY_FLAG".to_string(), "on".to_string())]),
                agents_md: None,
            },
        )
        .unwrap();

        let written = write_instance_files(&instance.id).unwrap();
        assert_eq!(written.len(), 2);

        let dir = instances::instance_dir(&instance.id).unwrap();
        let yaml = std::fs::read_to_string(dir.join("settings.yaml")).unwrap();
        assert!(yaml.contains("deepseek-v4-flash")); // 来自默认模板
        assert!(yaml.contains("disabled")); // 覆盖胜出

        let env_text = std::fs::read_to_string(dir.join(".env")).unwrap();
        assert_eq!(env_text, "MY_FLAG=on\n");

        // bootstrap-only 名在写盘前被拒，磁盘上不应出现半成品 .env。
        set_override(
            &instance.id,
            &InjectConfig {
                settings: Map::new(),
                env: BTreeMap::from([("DSH_HOME".to_string(), "evil".to_string())]),
                agents_md: None,
            },
        )
        .unwrap_err();
        assert_eq!(std::fs::read_to_string(dir.join(".env")).unwrap(), "MY_FLAG=on\n");

        instances::delete_instance(&instance.id).unwrap();
    }

    #[test]
    fn agents_md_inherits_global_and_override_wins() {
        let (_lock, _tmp) = sandbox("agentsmd");
        let instance = instances::create_instance("AGENTS 物化", "headless").unwrap();
        let dir = instances::instance_dir(&instance.id).unwrap();

        // 全局与实例都未设置：不写 AGENTS.md，生效值 None。
        assert_eq!(effective_agents_md(&instance.id).unwrap(), None);
        let written = write_instance_files(&instance.id).unwrap();
        assert_eq!(written.len(), 2, "未设置 AGENTS.md 时只写 settings/.env");
        assert!(!dir.join("AGENTS.md").exists());

        // 只设全局：物化继承内容。
        set_global_agents_md(Some("全局规则\n")).unwrap();
        assert_eq!(
            effective_agents_md(&instance.id).unwrap().as_deref(),
            Some("全局规则\n")
        );
        write_instance_files(&instance.id).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("AGENTS.md")).unwrap(),
            "全局规则\n"
        );

        // 实例覆盖：整体自设，胜出全局。
        set_override(
            &instance.id,
            &InjectConfig {
                settings: Map::new(),
                env: BTreeMap::new(),
                agents_md: Some("实例规则\n".into()),
            },
        )
        .unwrap();
        write_instance_files(&instance.id).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("AGENTS.md")).unwrap(),
            "实例规则\n"
        );

        // 全局清空（删除文件）不影响实例覆盖与已物化文件。
        set_global_agents_md(None).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("AGENTS.md")).unwrap(),
            "实例规则\n"
        );
        assert_eq!(
            effective_agents_md(&instance.id).unwrap().as_deref(),
            Some("实例规则\n")
        );

        // 清除实例覆盖后：全局也未设置 → 生效 None；**存量文件被删除**
        // （清空=没有指令；残留旧内容会被 DSH 继续读取——侦察指出的坑）。
        set_override(
            &instance.id,
            &InjectConfig {
                settings: Map::new(),
                env: BTreeMap::new(),
                agents_md: None,
            },
        )
        .unwrap();
        assert_eq!(effective_agents_md(&instance.id).unwrap(), None);
        assert!(
            !dir.join("AGENTS.md").exists(),
            "生效为空时应删除存量 AGENTS.md"
        );

        instances::delete_instance(&instance.id).unwrap();
    }

    #[test]
    fn ipc_camel_case_agents_md_roundtrips() {
        // IPC 键型回归（2026-09-20 实机走查 P0）：前端发 `agentsMd`（camelCase）
        // 必须落进 agents_md，序列化回读也用 camelCase——缺 rename 时版本级
        // AGENTS.md 覆盖被静默丢弃。settings/env 单词键两种命名同形，一并钉住。
        let value: InjectConfig = serde_json::from_str(
            r#"{ "settings": {}, "env": {}, "agentsMd": "局部规则" }"#,
        )
        .unwrap();
        assert_eq!(value.agents_md.as_deref(), Some("局部规则"));
        let json = serde_json::to_string(&value).unwrap();
        assert!(json.contains("\"agentsMd\""), "IPC 序列化必须是 camelCase：{json}");

        // 兼容：旧键 agents_md（若有手工文件）也能读出。
        let legacy: InjectConfig =
            serde_json::from_str(r#"{ "agents_md": "旧键内容" }"#).unwrap();
        assert_eq!(legacy.agents_md.as_deref(), Some("旧键内容"));
    }

    #[test]
    fn override_requires_existing_instance() {
        let (_lock, _tmp) = sandbox("noinst");
        assert!(get_override("i-missing").is_err());
        assert!(set_override("i-missing", &InjectConfig::default()).is_err());
        assert!(effective_config("i-missing").is_err());
    }
}
