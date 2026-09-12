//! 启动前校验链（FR-08 前半 + FR-16 静态部分）：
//! Node → Key → 实例完整性 → 插件体检（静态），按序产出分级报告。
//!
//! 分级：错误（Error，默认拦截启动，可被用户显式忽略）/ 警告（Warning，
//! 可忽略并指路）。纯读侧：只读实例目录与插件清单，绝不改写。
//!
//! 插件体检能力边界（PROJECT FR-16）：结构与声明类冲突（重复 bundles 行、
//! 清单与依赖不一致、有兼容声明的才核对）可静态检出；功能性冲突只能靠
//! 日志哨兵事后发现——本模块不假装能查。

use serde::Serialize;
use std::collections::BTreeMap;

use crate::instances::{self, Instance};
use crate::keyvault;
use crate::node_runtime;

/// 检查级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// 默认拦截启动。
    Error,
    /// 展示后可放行。
    Warning,
}

/// 报告里的一项检查结论；`action` 是给前端的路由提示（如 "settings"/"downloads"）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    pub id: String,
    pub level: Level,
    pub message: String,
    pub action: Option<String>,
}

/// 完整报告。`blocked` = 存在 Error 级条目；`plugins` 供就绪度卡片展示。
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub items: Vec<CheckItem>,
    pub blocked: bool,
    pub plugins: PluginStats,
}

impl Report {
    fn from_items(items: Vec<CheckItem>, plugins: PluginStats) -> Self {
        let blocked = items.iter().any(|i| i.level == Level::Error);
        Self { items, blocked, plugins }
    }
}

/// 校验链入口（阻塞调用，全部本地 IO）。实例不存在返回 Err（fail loud）。
pub fn run(instance_id: &str) -> Result<Report, String> {
    let instance = instances::get_instance(instance_id)?;
    let mut items = Vec::new();

    check_node(&mut items);
    check_key(&mut items);
    check_routes(instance_id, &mut items);
    check_default_model(instance_id, &mut items);
    check_integrity(&instance, &mut items);
    let plugins = check_plugins(&instance, &mut items);

    Ok(Report::from_items(items, plugins))
}

/// 路由落地体检（R7-2）：勾了启用却写不进实例的路由必须**先**告诉用户——
/// 否则用户只会看到「勾了 GLM，DSH 里没有它」。判定与桥接同源
/// （llm_bridge::route_report），前端不做二套。
fn check_routes(instance_id: &str, items: &mut Vec<CheckItem>) {
    let report = match crate::llm_bridge::route_report(instance_id) {
        Ok(report) => report,
        Err(e) => {
            items.push(CheckItem {
                id: "routes".into(),
                level: Level::Warning,
                message: format!("路由对账失败：{e}"),
                action: Some("settings".into()),
            });
            return;
        }
    };
    for skipped in report.skipped {
        let message = match skipped.reason.as_str() {
            crate::llm_bridge::SKIP_NO_MODELS => format!(
                "「{}」没有可用模型，本次启动不会写入实例（DSH 里看不到它）——到「设置 · 模型厂商」点「拉模型」或手填模型后再启动",
                skipped.label
            ),
            crate::llm_bridge::SKIP_NO_PROTOCOL => format!(
                "「{}」的接口形态（gemini）在 DSH 配置协议里没有对应者，无法写入实例路由——请改用 OpenAI 兼容地址",
                skipped.label
            ),
            reason => format!("「{}」未写入实例路由（{reason}）", skipped.label),
        };
        items.push(CheckItem {
            id: "routes".into(),
            level: Level::Warning,
            message,
            action: Some("settings".into()),
        });
    }
}

/// 默认模型体检（R7-1）：`agent-default-model` 只写了 model、没写 provider 时，
/// DSH 会用 DeepSeek 官方通道发这个模型 id（真机证据见
/// docs/reviews/2026-09-12-p0-p3-qa.md）。老实例就是这样写下的，这里如实警告。
fn check_default_model(instance_id: &str, items: &mut Vec<CheckItem>) {
    let Ok(config) = crate::inject::get_override(instance_id) else {
        return;
    };
    let Some(section) = config
        .settings
        .get("agent-default-model")
        .and_then(|v| v.as_object())
    else {
        return;
    };
    let model = section
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if model.is_empty() {
        return;
    }
    let provider = section
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if provider.is_empty() {
        items.push(CheckItem {
            id: "default-model".into(),
            level: Level::Warning,
            message: format!(
                "实例默认模型「{model}」没记提供方：DSH 会按 DeepSeek 官方通道发送，多半报模型不存在——到「启动」页重点一次该模型即可修好"
            ),
            action: Some("launch".into()),
        });
        return;
    }
    if provider == "deepseek-official" {
        return;
    }
    // 提供方是启动器管理的厂商时，核对模型是否在该厂商的清单里（只对能查到的
    // 厂商核对；查不到就不猜）。
    let Ok(status) = keyvault::status() else {
        return;
    };
    let Some(vendor) = status
        .vendors
        .iter()
        .find(|v| crate::llm_bridge::route_key_of(&v.vendor).ok().flatten().as_deref() == Some(provider.as_str()))
    else {
        return;
    };
    if !vendor.models.is_empty() && !vendor.models.iter().any(|m| m == &model) {
        items.push(CheckItem {
            id: "default-model".into(),
            level: Level::Warning,
            message: format!(
                "默认模型「{model}」不在「{}」的模型清单里（{}）——DSH 侧可能报模型不存在",
                vendor.vendor,
                vendor.models.join("、")
            ),
            action: Some("launch".into()),
        });
    }
}

fn check_node(items: &mut Vec<CheckItem>) {
    let default_version = node_runtime::load_config().ok().and_then(|c| c.default_node_version);
    match default_version {
        Some(v) if node_runtime::managed_node_exe(&v).is_some() => {
            items.push(CheckItem {
                id: "node".into(),
                level: Level::Warning,
                message: format!("托管 Node v{v} 就绪"),
                action: None,
            });
        }
        Some(v) => {
            items.push(CheckItem {
                id: "node".into(),
                level: Level::Error,
                message: format!("托管 Node {v} 的 node.exe 缺失，请到「设置」页重新下载"),
                action: Some("settings".into()),
            });
        }
        None => {
            // 无全局默认时再看系统 Node 是否满足范围（可直接用于安装/运行判断的宽松项）。
            match node_runtime::detect_system_node() {
                Some(sys) if sys.meets_range => items.push(CheckItem {
                    id: "node".into(),
                    level: Level::Warning,
                    message: format!(
                        "未设默认托管 Node；系统 {} 可用，但建议代管以保证版本受控",
                        sys.version
                    ),
                    action: Some("settings".into()),
                }),
                Some(sys) => items.push(CheckItem {
                    id: "node".into(),
                    level: Level::Error,
                    message: format!(
                        "系统 Node {} 不满足 ^22.19 || >=24，自动代管也没成功——到「设置」页下载一个托管版本（可填 22.20.0）",
                        sys.version
                    ),
                    action: Some("settings".into()),
                }),
                None => items.push(CheckItem {
                    id: "node".into(),
                    level: Level::Error,
                    message: "没有可用 Node：既无托管版本，PATH 上也没有系统 Node，自动代管也没成功——到「设置」页下载一个托管版本（可填 22.20.0）".into(),
                    action: Some("settings".into()),
                }),
            }
        }
    }
}

/// 启用厂商的 Key 可得性状态（校验链输入）。
pub(crate) struct VendorKeyState {
    pub vendor: String,
    pub has_vault_key: bool,
    pub has_env_key: bool,
}

/// Key 决策纯函数：任一启用厂商可得 Key（保险库或环境变量自动配置）即
/// 放行；启用集合为空或全部无 Key 才拦截。判断全在后端，前端不做二套。
pub(crate) fn key_check_item(states: &[VendorKeyState]) -> CheckItem {
    if states.is_empty() {
        return CheckItem {
            id: "key".into(),
            level: Level::Error,
            message: "未启用任何模型厂商：请到「设置」页模型厂商区勾选启用".into(),
            action: Some("settings".into()),
        };
    }
    let ready: Vec<&str> = states
        .iter()
        .filter(|s| s.has_vault_key || s.has_env_key)
        .map(|s| s.vendor.as_str())
        .collect();
    if !ready.is_empty() {
        return CheckItem {
            id: "key".into(),
            level: Level::Warning,
            message: format!("API Key 就绪（{}）", ready.join("、")),
            action: None,
        };
    }
    let names: Vec<&str> = states.iter().map(|s| s.vendor.as_str()).collect();
    CheckItem {
        id: "key".into(),
        level: Level::Error,
        message: format!(
            "启用的模型厂商均未配置 Key（{}）：既未在「设置」页保存，环境变量里也没有对应变量",
            names.join("、")
        ),
        action: Some("settings".into()),
    }
}

fn check_key(items: &mut Vec<CheckItem>) {
    match keyvault::status() {
        Ok(status) => {
            let states: Vec<VendorKeyState> = status
                .vendors
                .iter()
                .filter(|v| v.enabled)
                .map(|v| VendorKeyState {
                    vendor: v.vendor.clone(),
                    has_vault_key: v.has_key,
                    has_env_key: v.env_auto,
                })
                .collect();
            items.push(key_check_item(&states));
        }
        Err(e) => items.push(CheckItem {
            id: "key".into(),
            level: Level::Error,
            message: format!("读取凭据状态失败：{e}"),
            action: Some("settings".into()),
        }),
    }
}

fn check_integrity(instance: &Instance, items: &mut Vec<CheckItem>) {
    let dir = match instances::instance_dir(&instance.id) {
        Ok(d) => d,
        Err(e) => {
            items.push(CheckItem {
                id: "integrity".into(),
                level: Level::Error,
                message: e,
                action: None,
            });
            return;
        }
    };
    if !dir.join("instance.json").is_file() {
        items.push(CheckItem {
            id: "integrity".into(),
            level: Level::Error,
            message: format!("实例元数据缺失（{}）", dir.display()),
            action: Some("downloads".into()),
        });
    }
    match instance.dsh_version.as_deref() {
        None => items.push(CheckItem {
            id: "integrity".into(),
            level: Level::Error,
            message: "该实例还没有安装 DSH 本体，请到「下载」页选择版本安装".into(),
            action: Some("downloads".into()),
        }),
        Some(version) => match crate::dsh_install::installed_bin_js(&instance.id) {
            Ok(Some(_)) => items.push(CheckItem {
                id: "integrity".into(),
                level: Level::Warning,
                message: format!("DSH {version} 安装区完整"),
                action: None,
            }),
            Ok(None) => items.push(CheckItem {
                id: "integrity".into(),
                level: Level::Error,
                message: format!(
                    "记录显示已装 DSH {version}，但 bin.js 不在位——请到「下载」页重装"
                ),
                action: Some("downloads".into()),
            }),
            Err(e) => items.push(CheckItem {
                id: "integrity".into(),
                level: Level::Error,
                message: e,
                action: Some("downloads".into()),
            }),
        },
    }
}

/// 插件体检统计（供就绪度卡片展示）。
#[derive(Debug, Clone, Serialize)]
pub struct PluginStats {
    /// 已识别为插件（声明 dsh.bundle）的依赖数。
    pub installed: usize,
    /// 错误数。
    pub errors: usize,
    /// 警告数。
    pub warnings: usize,
}

/// 静态插件体检：读取 profile 目录的 package.json 与各插件包清单。
/// profile 目录不存在（从未启动过）不算错误——首次启动由 DSH 自己生成。
fn check_plugins(instance: &Instance, items: &mut Vec<CheckItem>) -> PluginStats {
    let mut stats = PluginStats { installed: 0, errors: 0, warnings: 0 };
    let app_node_modules;
    let profile_dir = match instances::instance_dir(&instance.id) {
        Ok(dir) => {
            // npm 依赖树装在 `<实例>/app`（安装器 --prefix=app），in-box 全家桶
            // 也物理落在这里；profile 本地 node_modules 只承载玩家手放/链接的包。
            app_node_modules = dir.join("app").join("node_modules");
            dir.join("profiles").join(&instance.profile)
        }
        Err(_) => return stats,
    };
    let manifest_path = profile_dir.join("package.json");
    if !manifest_path.is_file() {
        stats.warnings += 1;
        items.push(CheckItem {
            id: "plugins".into(),
            level: Level::Warning,
            message: format!(
                "profile「{}」尚未初始化（首次启动时自动创建）；当前无可体检的插件",
                instance.profile
            ),
            action: None,
        });
        return stats;
    }

    let raw = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(e) => {
            stats.errors += 1;
            items.push(CheckItem {
                id: "plugins".into(),
                level: Level::Error,
                message: format!("读取 profile 清单失败：{e}"),
                action: None,
            });
            return stats;
        }
    };
    let manifest: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            stats.errors += 1;
            items.push(CheckItem {
                id: "plugins".into(),
                level: Level::Error,
                message: format!("profile 清单不是合法 JSON：{e}"),
                action: None,
            });
            return stats;
        }
    };

    let dependencies: BTreeMap<String, String> = manifest
        .get("dependencies")
        .and_then(|v| v.as_object())
        .map(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();
    let bundles: Vec<String> = manifest
        .get("dsh")
        .and_then(|d| d.get("profile"))
        .and_then(|p| p.get("bundles"))
        .and_then(|b| b.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // ① 重复 bundles 行（同包出现两次 = 组合层叠混乱）；后续体检按去重清单遍历，
    // 否则同一插件被重复计数、重复深检。
    let mut seen = std::collections::BTreeSet::new();
    for name in &bundles {
        if !seen.insert(name.clone()) {
            stats.errors += 1;
            items.push(CheckItem {
                id: "plugins".into(),
                level: Level::Error,
                message: format!("重复安装：{name} 在 bundles 层列表中出现多次"),
                action: None,
            });
        }
    }

    for name in &seen {
        // ② bundles 与 dependencies 一致性（官方模板自带的 in-box 包除外：
        // 它们不在 dependencies 里但一定存在于 node_modules）。
        // 安装落点有两处：npm 依赖树在 `<实例>/app/node_modules`（rc.2 实测
        // dsh-base 等全家桶都在这里、且不在任何 dependencies——它们是元包的
        // 传递依赖），玩家/DSH 自己放进 profile 的包才在 profile 本地
        // node_modules。两处都找，找见即算「已安装」。
        let rel = name.replace('/', "\\");
        let pkg_dir = [profile_dir.join("node_modules"), app_node_modules.clone()]
            .iter()
            .map(|base| base.join(&rel))
            .find(|dir| dir.join("package.json").is_file());
        let is_dependency = dependencies.contains_key(name);
        let pkg_manifest = pkg_dir.as_ref().map(|dir| dir.join("package.json"));
        if !is_dependency && pkg_manifest.is_none() {
            stats.errors += 1;
            items.push(CheckItem {
                id: "plugins".into(),
                level: Level::Error,
                message: format!("{name} 在层列表中但既非依赖也未安装"),
                action: None,
            });
            continue;
        }
        let Some(pkg_manifest) = pkg_manifest else {
            continue; // in-box 官方包：存在性已由安装器保证，跳过深检
        };
        stats.installed += 1;

        // ③ 兼容声明核对：有 engines.dsh 声明才核（缺声明标未知不误报）。
        if let Ok(pkg_raw) = std::fs::read_to_string(&pkg_manifest) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&pkg_raw) {
                if let Some(required) = pkg
                    .get("engines")
                    .and_then(|e| e.get("dsh"))
                    .and_then(|v| v.as_str())
                {
                    let current = instance.dsh_version.clone().unwrap_or_default();
                    if current.is_empty() {
                        // 本体未装：无从比较硬核，但声明信息不能丢——降级为警告。
                        stats.warnings += 1;
                        items.push(CheckItem {
                            id: "plugins".into(),
                            level: Level::Warning,
                            message: format!(
                                "{name} 要求 DSH {required}——实例本体尚未安装，安装后需复检"
                            ),
                            action: Some("downloads".into()),
                        });
                    } else if !compat_decl_satisfied(required, &current) {
                        stats.errors += 1;
                        items.push(CheckItem {
                            id: "plugins".into(),
                            level: Level::Error,
                            message: format!("{name} 要求 DSH {required}，当前实例是 {current}"),
                            action: Some("downloads".into()),
                        });
                    }
                }
            }
        }
    }

    // ④ 依赖了却没进层列表的包：只是普通库，不报警（官方语义如此）。

    if stats.errors == 0 && stats.installed > 0 {
        items.push(CheckItem {
            id: "plugins".into(),
            level: Level::Warning,
            message: format!("插件体检通过：{installed} 个插件无静态冲突", installed = stats.installed),
            action: None,
        });
    }
    stats
}

/// 兼容声明满足判定：`^x.y.z` 前缀匹配同主次版本段、`~` 同主版本段、
/// 精确串要求完全一致；无法解析的声明按满足处理（宁可漏报不误报）。
pub fn compat_decl_satisfied(required: &str, current: &str) -> bool {
    let required = required.trim();
    let current_core = current.split('-').next().unwrap_or(current);
    let parse =
        |s: &str| -> Option<(u64, u64, u64)> {
            let mut it = s.trim_start_matches(['^', '~', 'v']).split('.');
            Some((
                it.next()?.parse().ok()?,
                it.next()?.parse().ok()?,
                it.next().and_then(|p| p.parse().ok()).unwrap_or(0),
            ))
        };
    let (Some(req), Some(cur)) = (parse(required), parse(current_core)) else {
        return true; // 无法解析 → 不核（有声明才核的兜底）
    };
    if required.starts_with('^') {
        req.0 == cur.0 && (req.1, req.2) <= (cur.1, cur.2)
    } else if required.starts_with('~') {
        (req.0, req.1) == (cur.0, cur.1) && req.2 <= cur.2
    } else {
        req == cur
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::tests::sandbox;

    /// Key 决策：任一启用厂商可得 Key 即放行；启用集合为空或全无才拦截。
    #[test]
    fn key_check_allows_inherited_environment_channel() {
        let ready = key_check_item(&[VendorKeyState {
            vendor: keyvault::VENDOR_DEEPSEEK.to_string(),
            has_vault_key: false,
            has_env_key: true,
        }]);
        assert_eq!(ready.level, Level::Warning, "环境变量通道不得拦截");
        assert!(ready.action.is_none());

        let vault_ready = key_check_item(&[VendorKeyState {
            vendor: keyvault::VENDOR_DEEPSEEK.to_string(),
            has_vault_key: true,
            has_env_key: false,
        }]);
        assert_eq!(vault_ready.level, Level::Warning);

        let none = key_check_item(&[VendorKeyState {
            vendor: keyvault::VENDOR_DEEPSEEK.to_string(),
            has_vault_key: false,
            has_env_key: false,
        }]);
        assert_eq!(none.level, Level::Error);
        assert_eq!(none.action.as_deref(), Some("settings"));

        let empty = key_check_item(&[]);
        assert_eq!(empty.level, Level::Error, "未启用任何厂商必须拦截");
        assert!(empty.message.contains("未启用"));

        // 多厂商：只有一个就绪也算放行，消息点名就绪者。
        let mixed = key_check_item(&[
            VendorKeyState { vendor: "a".into(), has_vault_key: false, has_env_key: false },
            VendorKeyState { vendor: "b".into(), has_vault_key: true, has_env_key: false },
        ]);
        assert_eq!(mixed.level, Level::Warning);
        assert!(mixed.message.contains('b'));
    }

    /// R7-2：无模型的自描述厂商在启动前就被点名，不闷着让用户去 DSH 找。
    #[test]
    fn routes_warning_names_vendor_without_models() {
        let (_lock, tmp) = sandbox("precheck-routes");
        let instance = instances::create_instance("路由体检", "web").unwrap();
        keyvault::set_enabled("glm", true).unwrap(); // 无模型

        let mut items = Vec::new();
        check_routes(&instance.id, &mut items);

        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].level, Level::Warning);
        assert!(items[0].message.contains("GLM（智谱）"), "{}", items[0].message);
        assert!(items[0].message.contains("拉模型"), "文案要指路：{}", items[0].message);
        assert_eq!(items[0].action.as_deref(), Some("settings"));
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// R7-1：默认模型只写了 model（老实例的写法）→ 启动前警告并指路「启动」页。
    #[test]
    fn default_model_without_provider_warns() {
        let (_lock, tmp) = sandbox("precheck-model");
        let instance = instances::create_instance("默认模型体检", "web").unwrap();
        let mut config = crate::inject::get_override(&instance.id).unwrap();
        config.settings.insert(
            "agent-default-model".into(),
            serde_json::json!({ "model": "glm-4.6" }),
        );
        crate::inject::set_override(&instance.id, &config).unwrap();

        let mut items = Vec::new();
        check_default_model(&instance.id, &mut items);

        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].level, Level::Warning);
        assert!(items[0].message.contains("glm-4.6"), "{}", items[0].message);
        assert_eq!(items[0].action.as_deref(), Some("launch"));

        // 写全 provider+model 后不再警告。
        config.settings.insert(
            "agent-default-model".into(),
            serde_json::json!({ "provider": "glm", "model": "glm-4.6" }),
        );
        crate::inject::set_override(&instance.id, &config).unwrap();
        let mut ok = Vec::new();
        check_default_model(&instance.id, &mut ok);
        assert!(ok.is_empty(), "{ok:?}");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn compat_ranges_match_semver_expectations() {
        assert!(compat_decl_satisfied("^0.1.0", "0.1.1"));
        assert!(compat_decl_satisfied("^0.1.0", "0.1.1-rc.2"), "prerelease 尾巴不影响核心比较");
        assert!(!compat_decl_satisfied("^0.2.0", "0.1.9"));
        assert!(compat_decl_satisfied("~1.2.0", "1.2.5"));
        assert!(!compat_decl_satisfied("~1.2.0", "1.3.0"));
        assert!(compat_decl_satisfied("1.0.0", "1.0.0"));
        assert!(!compat_decl_satisfied("1.0.0", "1.0.1"));
        assert!(compat_decl_satisfied("*", "anything"), "通配声明不拦");
        assert!(compat_decl_satisfied("", ""), "空对空视为满足");
    }

    #[test]
    fn missing_instance_fails_loud() {
        let (_lock, tmp) = sandbox("precheck-noinst");
        assert!(run("i-missing").is_err());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn uninstalled_instance_reports_errors_in_order() {
        let (_lock, tmp) = sandbox("precheck-uninstalled");
        let instance = instances::create_instance("未装本体", "web").unwrap();
        let report = run(&instance.id).unwrap();
        assert!(report.blocked, "无 Node 配置 + 无 Key + 未装 DSH 必然拦截");
        let ids: Vec<&str> = report.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids.first(), Some(&"node"), "校验链顺序：Node 最先");
        assert!(ids.contains(&"key"));
        assert!(ids.contains(&"integrity"));
        assert!(
            report.items.iter().any(|i| i.message.contains("还没有安装 DSH")),
            "完整性错误应指明未安装：{:?}",
            report.items
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 验收（FR-16 前半）：人为构造的重复 bundles + 兼容声明冲突场景，
    /// 校验链给出分级报告（Error 拦截），且报告可序列化给前端展示/放行。
    #[test]
    fn fabricated_plugin_conflicts_produce_graded_report() {
        let (_lock, tmp) = sandbox("precheck-plugins");
        let instance = instances::create_instance("插件体检", "web").unwrap();

        // 构造 profile 清单：同一插件在层列表出现两次 + 一个声明了 engines.dsh 的包。
        let profile = instances::instance_dir(&instance.id)
            .unwrap()
            .join("profiles")
            .join("web");
        std::fs::create_dir_all(profile.join("node_modules").join("fake-plugin")).unwrap();
        std::fs::write(
            profile.join("package.json"),
            serde_json::json!({
                "name": "fabricated-profile",
                "dependencies": { "fake-plugin": "^1.0.0" },
                "dsh": { "profile": { "bundles": ["fake-plugin", "fake-plugin"] } }
            })
            .to_string(),
        )
        .unwrap();
        // fake-plugin 声明要求 DSH ^9.9.9——与实例（未装）必然冲突。
        std::fs::write(
            profile.join("node_modules").join("fake-plugin").join("package.json"),
            serde_json::json!({ "name": "fake-plugin", "version": "1.0.0", "engines": { "dsh": "^9.9.9" } })
                .to_string(),
        )
        .unwrap();

        let report = run(&instance.id).unwrap();
        assert!(report.blocked, "构造冲突必须拦截：{:?}", report.items);
        assert_eq!(report.plugins.installed, 1, "应识别出 1 个已装插件");
        let dupes = report
            .items
            .iter()
            .filter(|i| i.message.contains("重复安装") && i.level == Level::Error)
            .count();
        assert_eq!(dupes, 1, "重复 bundles 行应有恰好一条错误");
        assert!(
            report
                .items
                .iter()
                .any(|i| i.level == Level::Warning && i.message.contains("fake-plugin 要求 DSH")),
            "本体未装时兼容声明应降级为警告并点名插件：{:?}",
            report.items
        );
        // 报告可 JSON 化传给前端（command 层把它当 Err 抛出的前提）。
        assert!(serde_json::to_string(&report).is_ok());
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 回归（真机 rc.2 实测误报）：in-box 全家桶物理装在 `<实例>/app/node_modules`
    /// 且不在任何 dependencies（它们是 npm 元包的传递依赖），曾因体检只扫
    /// profile 本地 node_modules 被误判「既非依赖也未安装」→ 每次启动都被拦截。
    #[test]
    fn inbox_bundles_in_app_node_modules_are_not_flagged() {
        let (_lock, tmp) = sandbox("precheck-inbox-app");
        let instance = instances::create_instance("rc2 布局", "web").unwrap();
        let dir = instances::instance_dir(&instance.id).unwrap();

        // npm 安装区：app/package.json 只有元包依赖，node_modules 有被提升的 in-box 包。
        let app = dir.join("app");
        for pkg in ["dsh-base", "dsh-web-app"] {
            let pkg_dir = app.join("node_modules").join("@deepseek-ai").join(pkg);
            std::fs::create_dir_all(&pkg_dir).unwrap();
            std::fs::write(
                pkg_dir.join("package.json"),
                serde_json::json!({ "name": format!("@deepseek-ai/{pkg}"), "version": "0.1.1-rc.2" })
                    .to_string(),
            )
            .unwrap();
        }
        std::fs::write(
            app.join("package.json"),
            serde_json::json!({ "name": "instance-app", "dependencies": { "@deepseek-ai/dsh": "0.1.1-rc.2" } })
                .to_string(),
        )
        .unwrap();

        // profile 清单：层列表声明 in-box 包，无 dependencies（DSH reconcile 的真实形态）。
        let profile = dir.join("profiles").join("web");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            profile.join("package.json"),
            serde_json::json!({
                "name": "web-profile",
                "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] } }
            })
            .to_string(),
        )
        .unwrap();

        let report = run(&instance.id).unwrap();
        assert!(
            !report.items.iter().any(|i| i.message.contains("既非依赖也未安装")),
            "app/node_modules 里的 in-box 包不得误报：{:?}",
            report.items
        );
        assert_eq!(report.plugins.installed, 2, "两个 in-box 包都应计为已装");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
