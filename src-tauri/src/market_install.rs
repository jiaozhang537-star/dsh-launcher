//! 插件安装管线（FR-03）：走 DSH 官方 `dsh plugin add/remove` 命令链
//! （R-3 e2e 定案）——DSH 自己完成 pnpm 安装与 `dsh.profile.bundles`
//! reconcile，壳零复制本体逻辑，符合「只做壳」红线。
//!
//! pnpm 供给（R-2 遗留问题的解法）：托管 Node 自带 corepack，用
//! `corepack enable pnpm --install-directory <managed>/bin` 生成 pnpm shim，
//! 调用官方链时把该目录前置到 PATH。镜像语义：`COREPACK_NPM_REGISTRY`
//! 与 `npm_config_registry` 恒取网关候选链首位（npmmirror 优先），官方链
//! 安装失败自动整链重试下一通道（优先≠独占）。
//!
//! 幂等：重复安装同版本 = pnpm 覆盖重装（官方链天然幂等）；装后读取
//! profile 清单差分确定实际包名与版本，并核对 bundles 生效。

use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::dsh_install;
use crate::gateway;
use crate::instances;
use crate::node_runtime::CreationFlagsExt;
use crate::paths;

/// pnpm shim 目录：`<managed root>/bin`。
pub fn shim_dir() -> Result<PathBuf, String> {
    Ok(paths::managed_root()?.join("bin"))
}

/// pnpm 事件进度通道（Tauri 事件 `plugin-install-progress`）。
pub const PROGRESS_EVENT: &str = "plugin-install-progress";

/// 确保 pnpm shim 就位，返回 shim 目录。
/// 已存在则跳过（幂等）；首次生成时 corepack 从镜像拉取 pnpm（数百 KB 级）。
pub fn ensure_pnpm_shim(node_exe: &Path) -> Result<PathBuf, String> {
    let dir = shim_dir()?;
    let pnpm_cmd = dir.join("pnpm.CMD");
    if pnpm_cmd.is_file() {
        return Ok(dir);
    }
    let node_dir = node_exe
        .parent()
        .ok_or_else(|| "node.exe 路径异常".to_string())?;
    let corepack = node_dir.join("corepack.cmd");
    if !corepack.is_file() {
        return Err(format!(
            "该 Node 缺自带 corepack（{} 不存在），无法供给 pnpm——请到「设置」页下载并代管官方便携版 Node",
            corepack.display()
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 shim 目录失败：{e}"))?;
    let out = Command::new(&corepack)
        .args(["enable", "pnpm", "--install-directory"])
        .arg(&dir)
        .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
        .env("COREPACK_NPM_REGISTRY", gateway::NPM_NPMMIRROR_REGISTRY)
        .creation_flags_cv()
        .output()
        .map_err(|e| format!("运行 corepack enable 失败：{e}"))?;
    if !out.status.success() || !pnpm_cmd.is_file() {
        return Err(format!(
            "corepack 生成 pnpm shim 失败（exit {:?}）：\n{}\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(dir)
}

/// 组装官方链子进程：`node <bin.js> plugin --profile <p> <args…>`，
/// PATH 前置 [node 目录, pnpm shim 目录]，corepack/pnpm 走 npmmirror。
/// **DSH_HOME 必须钉到实例目录**——官方链靠它解析 profile 落点（真机教训：
/// 漏设时安装会静默落进用户全局 `~/.dsh`，污染真实环境）。
fn plugin_command(
    node_exe: &Path,
    bin_js: &Path,
    shim: &Path,
    dsh_home: &Path,
    profile: &str,
    args: &[&str],
    registry_base: &str,
) -> Command {
    let mut cmd = Command::new(node_exe);
    cmd.arg(bin_js)
        .args(["plugin", "--profile", profile])
        .args(args)
        .env("DSH_HOME", dsh_home)
        .env(
            "PATH",
            format!(
                "{};{};{}",
                node_exe.parent().unwrap_or(Path::new(".")).display(),
                shim.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("COREPACK_NPM_REGISTRY", registry_base)
        .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
        .env("npm_config_registry", registry_base)
        .env("npm_config_update_notifier", "false");
    cmd
}

/// 运行一次官方链命令并 drain 双输出流（静默看门狗 600s 纯兜底）。
fn run_plugin_command(mut cmd: Command, action: &str, progress: &mut dyn FnMut(String)) -> Result<(), String> {
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags_cv();
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("拉起官方插件命令失败：{e}"))?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let mut readers = Vec::new();
    let streams: Vec<Box<dyn std::io::Read + Send>> = vec![
        Box::new(child.stdout.take().ok_or("stdout 缺失")?),
        Box::new(child.stderr.take().ok_or("stderr 缺失")?),
    ];
    for stream in streams {
        let tx = tx.clone();
        readers.push(thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);

    let quiet_limit = Duration::from_secs(600);
    let mut tail = dsh_install::OutputTail::new(30);
    let mut last_output = std::time::Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(line) => {
                last_output = std::time::Instant::now();
                tail.push(line.clone());
                if tail.total % 5 == 0 {
                    let detail = line.trim().chars().take(160).collect::<String>();
                    progress(if detail.is_empty() { "…".to_string() } else { detail });
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if last_output.elapsed() >= quiet_limit {
                    let _ = child.kill();
                    for handle in readers {
                        let _ = handle.join();
                    }
                    let _ = child.wait();
                    return Err(format!(
                        "官方插件命令超过 10 分钟无任何输出，已中止。\n{}",
                        tail.join()
                    ));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    for handle in readers {
        handle.join().map_err(|_| "输出读取线程崩溃".to_string())?;
    }
    let status = child.wait().map_err(|e| format!("等待退出失败：{e}"))?;
    if !status.success() {
        return Err(format!("{action} 失败（exit {}）：\n{}", status.code().unwrap_or(-1), tail.join()));
    }
    Ok(())
}

/// 实例 profile 目录：`<实例>/profiles/<profile>`。
fn profile_dir(instance_id: &str, profile: &str) -> Result<PathBuf, String> {
    Ok(instances::instance_dir(instance_id)?.join("profiles").join(profile))
}

/// 读 profile 清单的 dependencies 映射；文件/字段缺失按空表（R-3 实证：
/// pnpm remove 后 `dependencies` 键会整个消失）。
fn read_dependencies(profile: &Path) -> Result<Vec<(String, String)>, String> {
    let manifest = profile.join("package.json");
    if !manifest.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("读 {} 失败：{e}", manifest.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("清单不是合法 JSON：{e}"))?;
    let Some(deps) = value.get("dependencies").and_then(|v| v.as_object()) else {
        return Ok(Vec::new());
    };
    Ok(deps
        .iter()
        .filter_map(|(k, v)| {
            let spec = v.as_str()?.to_string();
            Some((k.clone(), spec))
        })
        .collect())
}

/// 读 profile 清单的 `dsh.profile.bundles`。
fn read_bundles(profile: &Path) -> Result<Vec<String>, String> {
    let manifest = profile.join("package.json");
    if !manifest.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("读 {} 失败：{e}", manifest.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("清单不是合法 JSON：{e}"))?;
    Ok(value
        .pointer("/dsh/profile/bundles")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|b| b.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// 插件安装结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallOutcome {
    #[serde(rename = "instanceId")]
    pub instance_id: String,
    /// 装后确定的实际包名（依赖差分，spec 形态无关）。
    pub package_name: String,
    /// 装后实际版本（node_modules 内 manifest；读不到为空串）。
    pub version: String,
    /// 是否已进入 `dsh.profile.bundles` 层（非 bundle 纯依赖为 false）。
    pub bundle_activated: bool,
}

/// 把一个插件装进实例 profile（官方 `dsh plugin add` 链）。spec 支持 npm
/// `name@ver` / `github:owner/repo` / `file:`/`link:` 本地路径（pnpm 语义）。
/// 阻塞调用——command 侧放 spawn_blocking。重复安装幂等（覆盖重装）。
pub fn plugin_add(
    instance_id: &str,
    spec: &str,
    progress: &mut dyn FnMut(String),
) -> Result<PluginInstallOutcome, String> {
    let trimmed = spec.trim();
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return Err(format!("非法安装标识：{trimmed:?}"));
    }
    // git 直装需要本机 git（pnpm 的 git 依赖走系统 git 拉取构建）；缺失早
    // 失败并给可读指引，不进官方链吃 pnpm 的 ENOENT 模糊报错。
    if trimmed.starts_with("github:") || trimmed.starts_with("git+") || trimmed.starts_with("git:") {
        match Command::new("git").arg("--version").creation_flags_cv().output() {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                return Err(format!(
                    "git 直装需要本机安装 Git：git --version 退出码 {:?}——请安装 Git 后重试",
                    out.status.code()
                ));
            }
            Err(e) => {
                return Err(format!(
                    "git 直装需要本机安装 Git（PATH 上找不到 git：{e}）——请安装后重试"
                ));
            }
        }
    }
    let instance = instances::get_instance(instance_id)?;
    let bin_js = dsh_install::installed_bin_js(instance_id)?
        .ok_or_else(|| "该实例尚未安装 DSH 本体——请先到下载页安装，再装插件".to_string())?;
    progress("校验 Node 运行时与 pnpm 供给…".into());
    let node_exe = dsh_install::resolve_install_node(&instance, &mut |_stage| {})?;
    let shim = ensure_pnpm_shim(&node_exe)?;

    let profile = profile_dir(instance_id, &instance.profile)?;
    let dsh_home = instances::instance_dir(instance_id)?;
    let before = read_dependencies(&profile)?;

    let mut attempts = Vec::new();
    for registry_base in gateway::npm_registry_bases() {
        progress(format!("正在安装 {trimmed}（通道 {registry_base}）…"));
        let cmd = plugin_command(
            &node_exe,
            &bin_js,
            &shim,
            &dsh_home,
            &instance.profile,
            &["add", trimmed],
            registry_base,
        );
        match run_plugin_command(cmd, &format!("dsh plugin add {trimmed}"), progress) {
            Ok(()) => break,
            Err(e) => attempts.push(format!("[通道 {registry_base}] {e}")),
        }
    }
    if attempts.len() == gateway::npm_registry_bases().len() {
        return Err(format!(
            "插件安装 {trimmed} 在全部通道上失败：\n{}",
            attempts.join("\n---\n")
        ));
    }

    // 依赖差分确定实际包名（git/别名 spec 的清单键与 spec 不同）。
    let after = read_dependencies(&profile)?;
    let package_name = after
        .iter()
        .find(|(name, _)| !before.iter().any(|(old, _)| old == name))
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| {
            // 无新增依赖 = 重复安装既有包（幂等重装）：取 spec 的包名部分。
            spec_package_name(trimmed)
        });
    let version = resolve_installed_version(instance_id, &instance.profile, &package_name)?;
    let bundles = read_bundles(&profile)?;
    let bundle_activated = bundles.iter().any(|b| b == &package_name);
    progress(format!(
        "安装完成：{package_name} {version}{}",
        if bundle_activated { "（已进入 bundles 层）" } else { "（纯依赖，无 bundles 层）" }
    ));
    Ok(PluginInstallOutcome {
        instance_id: instance_id.to_string(),
        package_name,
        version,
        bundle_activated,
    })
}

/// 从安装 spec 提取包名：npm 形态 `name[@ver]`（含 scoped）；其余形态尽力
/// 取尾段。仅作「重复安装」场景的兜底——正常路径以依赖差分为准。
fn spec_package_name(spec: &str) -> String {
    if let Some(rest) = spec.strip_prefix("github:") {
        let without_fragment = rest.split('#').next().unwrap_or(rest);
        return without_fragment
            .rsplit('/')
            .next()
            .unwrap_or(without_fragment)
            .to_string();
    }
    let core = spec.rsplit("::").next().unwrap_or(spec);
    match core.rfind('@') {
        // `@scope/name` 的首个 @ 不算版本分隔；scoped 包找 name 段内的 @。
        Some(idx) if idx > 0 && !core[..idx].contains('/') => core[..idx].to_string(),
        Some(idx) if idx > 0 => core[..idx].to_string(),
        _ => core.to_string(),
    }
}

/// 已装包的模块查找根：profile 本地 node_modules → profiles 扁平回退区
/// （DSH heal 的 junction 目录）→ app 安装区（in-box bundle）。
pub fn module_roots(instance_id: &str, profile: &str) -> Result<Vec<PathBuf>, String> {
    let base = instances::instance_dir(instance_id)?;
    Ok(vec![
        base.join("profiles").join(profile).join("node_modules"),
        base.join("profiles").join("node_modules"),
        base.join(dsh_install::APP_DIR_NAME).join("node_modules"),
    ])
}

/// 在实例的模块根中解析已装包版本：profile 本地 node_modules → profiles 扁平
/// 回退区（DSH heal 的 junction 目录）→ app 安装区（in-box bundle）。
pub fn resolve_installed_version(
    instance_id: &str,
    profile: &str,
    package: &str,
) -> Result<String, String> {
    Ok(resolve_installed_manifest(instance_id, profile, package)?
        .and_then(|m| m.get("version").and_then(|v| v.as_str()).map(str::to_string))
        .unwrap_or_default())
}

/// 读已装包的 package.json（三级根扫描；找不到返回 None）。
pub fn resolve_installed_manifest(
    instance_id: &str,
    profile: &str,
    package: &str,
) -> Result<Option<serde_json::Value>, String> {
    for root in module_roots(instance_id, profile)? {
        let manifest = root.join(package).join("package.json");
        if manifest.is_file() {
            let text = std::fs::read_to_string(&manifest)
                .map_err(|e| format!("读 {} 失败：{e}", manifest.display()))?;
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| format!("{} 不是合法 JSON：{e}", manifest.display()))?;
            return Ok(Some(value));
        }
    }
    Ok(None)
}

/// 从实例 profile 移除一个插件（官方 `dsh plugin remove` 链，reconcile 自动
/// 同步 bundles）。阻塞调用——command 侧放 spawn_blocking。
pub fn plugin_remove(instance_id: &str, package: &str, progress: &mut dyn FnMut(String)) -> Result<(), String> {
    let instance = instances::get_instance(instance_id)?;
    let bin_js = dsh_install::installed_bin_js(instance_id)?
        .ok_or_else(|| "该实例尚未安装 DSH 本体".to_string())?;
    let node_exe = dsh_install::resolve_install_node(&instance, &mut |_stage| {})?;
    let shim = ensure_pnpm_shim(&node_exe)?;
    let profile = profile_dir(instance_id, &instance.profile)?;
    let dsh_home = instances::instance_dir(instance_id)?;
    let before = read_dependencies(&profile)?;
    if !before.iter().any(|(name, _)| name == package) {
        return Err(format!(
            "{package} 不是该 profile 的依赖（内置层不可经此移除）"
        ));
    }

    let mut attempts = Vec::new();
    for registry_base in gateway::npm_registry_bases() {
        progress(format!("正在移除 {package}（通道 {registry_base}）…"));
        let cmd = plugin_command(
            &node_exe,
            &bin_js,
            &shim,
            &dsh_home,
            &instance.profile,
            &["remove", package],
            registry_base,
        );
        match run_plugin_command(cmd, &format!("dsh plugin remove {package}"), progress) {
            Ok(()) => break,
            Err(e) => attempts.push(format!("[通道 {registry_base}] {e}")),
        }
    }
    if attempts.len() == gateway::npm_registry_bases().len() {
        return Err(format!(
            "插件移除 {package} 失败：\n{}",
            attempts.join("\n---\n")
        ));
    }

    let after = read_dependencies(&profile)?;
    if after.iter().any(|(name, _)| name == package) {
        return Err(format!("移除命令成功但清单仍含 {package}——状态异常"));
    }
    let bundles = read_bundles(&profile)?;
    if bundles.iter().any(|b| b == package) {
        return Err(format!("移除后 bundles 仍含 {package}——reconcile 未生效"));
    }
    progress(format!("已移除 {package}（bundles 已同步）"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_package_name_handles_npm_scoped_and_git_forms() {
        assert_eq!(spec_package_name("dsh-cost-meter@1.2.3"), "dsh-cost-meter");
        assert_eq!(spec_package_name("@nanmicoder/dsh-agent-teams@0.1.15"), "@nanmicoder/dsh-agent-teams");
        assert_eq!(spec_package_name("dsh-cost-meter"), "dsh-cost-meter");
        assert_eq!(spec_package_name("github:owner/dsh-tool"), "dsh-tool");
        assert_eq!(spec_package_name("github:owner/dsh-tool#v1.0.0"), "dsh-tool");
    }

    #[test]
    fn blank_or_spaced_spec_rejected() {
        let mut sink = |_d: String| {};
        assert!(plugin_add("i-x", "", &mut sink).is_err());
        assert!(plugin_add("i-x", "a b", &mut sink).is_err());
    }

    #[test]
    fn missing_instance_fails_loud() {
        let (_lock, tmp) = crate::instances::tests::sandbox("minstall-noinst");
        let mut sink = |_d: String| {};
        assert!(plugin_add("i-missing", "dsh-cost-meter", &mut sink).is_err());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn dependencies_reader_tolerates_missing_manifest_and_key() {
        let (_lock, tmp) = crate::instances::tests::sandbox("minstall-deps");
        let profile = tmp.join("profiles").join("web");
        std::fs::create_dir_all(&profile).unwrap();
        assert!(read_dependencies(&profile).unwrap().is_empty(), "无清单 → 空表");
        std::fs::write(profile.join("package.json"), r#"{ "name": "p", "dsh": { "profile": { "bundles": ["a"] } } }"#).unwrap();
        assert!(read_dependencies(&profile).unwrap().is_empty(), "缺 dependencies 键 → 空表");
        assert_eq!(read_bundles(&profile).unwrap(), vec!["a"]);
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 真实 e2e（显式 `cargo test real_plugin -- --ignored --nocapture`）：
    /// 在真实托管根的既有实例上走「官方链安装 → 三分组清单 → 更新检查 →
    /// 官方链移除」全环。要求本机存在已安装 DSH 的实例；结束时恢复原状
    /// （目标插件被移除），失败时可能残留 dsh-cost-meter，重跑即清。
    #[test]
    #[ignore = "真实网络 + 真实实例：安装/移除 dsh-cost-meter 全环"]
    fn real_plugin_add_list_remove_roundtrip() {
        let all = crate::instances::list_instances().expect("实例注册表可读");
        let target = all
            .iter()
            .find(|i| i.dsh_version.is_some())
            .expect("本机需要一个已安装 DSH 的实例（先在下载页安装）")
            .clone();
        println!("e2e 目标实例：{}（profile {}）", target.id, target.profile);
        let mut sink = |detail: String| println!("  [进度] {detail}");

        let _ = plugin_remove(&target.id, "dsh-cost-meter", &mut sink); // 干净起点（幂等）
        let outcome = plugin_add(&target.id, "dsh-cost-meter", &mut sink).expect("官方链安装失败");
        assert_eq!(outcome.package_name, "dsh-cost-meter");
        assert!(outcome.bundle_activated, "dsh-cost-meter 应进入 bundles 层");
        assert!(!outcome.version.is_empty(), "应能解析已装版本");
        println!("安装完成：{} @ {}", outcome.package_name, outcome.version);

        let list = crate::plugin_manage::list(&target.id).expect("清单可读");
        let row = list
            .plugins
            .iter()
            .find(|p| p.name == "dsh-cost-meter")
            .expect("三分组清单应含新插件");
        assert_eq!(row.group, crate::plugin_manage::PluginGroup::Thirdparty);
        assert!(row.removable);
        assert!(row.in_bundles);

        let updates = crate::plugin_manage::check_updates(&target.id).expect("更新检查可跑");
        let update = updates.iter().find(|u| u.name == "dsh-cost-meter").expect("更新检查应含该包");
        println!("更新检查：current={} latest={}", update.current, update.latest);

        plugin_remove(&target.id, "dsh-cost-meter", &mut sink).expect("官方链移除失败");
        let after = crate::plugin_manage::list(&target.id).expect("清单可读");
        assert!(
            !after.plugins.iter().any(|p| p.name == "dsh-cost-meter"),
            "移除后清单不应再含该插件"
        );
        println!("全环通过：安装 → 清单 → 更新检查 → 移除");
    }

    /// 真实 e2e（验收锚点，显式触发）：安装一个**非最新版本**插件 → bundles
    /// 生效 → 官方链拉起实例 → web 就绪（profile 补丁层组合成功 = 插件被
    /// DSH 识别加载；组合失败会直接起不来）→ 进程存活 → 停止回收 → 清场。
    /// 结束后移除该插件；失败可能残留 dsh-find-plugin，重跑即清。
    #[test]
    #[ignore = "真实网络 + 真实启动：装非最新版 dsh-find-plugin 并启动验证"]
    fn real_plugin_recognized_by_dsh_launch() {
        let all = crate::instances::list_instances().expect("实例注册表可读");
        let target = all
            .iter()
            .find(|i| i.dsh_version.is_some())
            .expect("本机需要一个已安装 DSH 的实例")
            .clone();
        println!("e2e 目标实例：{}（profile {}）", target.id, target.profile);
        let mut sink = |detail: String| println!("  [进度] {detail}");

        let _ = plugin_remove(&target.id, "dsh-find-plugin", &mut sink); // 干净起点
        let versions = crate::market::fetch_versions("dsh-find-plugin").expect("版本枚举失败");
        let pick = versions
            .versions
            .iter()
            .find(|v| !v.latest)
            .expect("需要一个非最新版本");
        println!("选择非最新版本：dsh-find-plugin@{}", pick.version);

        let outcome = plugin_add(&target.id, &format!("dsh-find-plugin@{}", pick.version), &mut sink)
            .expect("安装失败");
        assert!(outcome.bundle_activated, "应进入 bundles 层");
        assert_eq!(outcome.version, pick.version, "装后版本应与所选一致");

        let out = crate::process::launch(None, &target.id, false).expect("实例启动失败");
        println!("实例已拉起：port {} pid {}", out.port, out.pid);
        wait_http_ready(out.port, 120);
        println!("web 就绪 ✓（profile 补丁层组合成功，插件被 DSH 识别加载）");
        std::thread::sleep(std::time::Duration::from_secs(10));
        assert!(
            crate::process::list_running().iter().any(|r| r.instance_id == target.id),
            "启动 10s 后进程不应退出（插件不应拖垮启动）"
        );
        crate::process::stop(&target.id).expect("停止失败");
        {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while !crate::instances::port_free(out.port) {
                assert!(std::time::Instant::now() < deadline, "端口 {} 未释放", out.port);
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
        println!("停止回收 ✓");

        plugin_remove(&target.id, "dsh-find-plugin", &mut sink).expect("清场移除失败");
        println!("全环通过：非最新版安装 → 启动加载 → 停止 → 清场");
    }

    /// 真实 e2e（走查反馈回归，显式触发）：github: spec 的 git 直装全链。
    /// 该包同时发布 npm，但 spec 走 github: 即走 git 拉取构建链。成功则断言
    /// bundles 生效并清场；被 pnpm ≥10 构建脚本 allowBuilds 拦截属官方已知
    /// 行为——报错必须可读（含官方指引），不允许模糊 ENOENT。
    #[test]
    #[ignore = "真实网络 + git 拉取：github: 直装链实测"]
    fn real_plugin_git_install_roundtrip() {
        let all = crate::instances::list_instances().expect("实例注册表可读");
        let target = all
            .iter()
            .find(|i| i.dsh_version.is_some())
            .expect("本机需要一个已安装 DSH 的实例")
            .clone();
        println!("e2e 目标实例：{}", target.id);
        let mut sink = |detail: String| println!("  [进度] {detail}");

        let result = plugin_add(&target.id, "github:NanmiCoder/dsh-agent-teams", &mut sink);
        match result {
            Ok(outcome) => {
                println!("git 直装成功：{} @ {}", outcome.package_name, outcome.version);
                assert!(!outcome.version.is_empty());
                plugin_remove(&target.id, &outcome.package_name, &mut sink).expect("清场移除失败");
                println!("清场完成 ✓");
            }
            Err(e) => {
                println!("git 直装报错（如实记录）：{e}");
                assert!(
                    e.contains("pnpm") || e.contains("Git") || e.contains("allowBuilds") || e.contains("构建"),
                    "git 直装失败信息应可读（含官方指引）：{e}"
                );
            }
        }
    }

    /// 轮询等待端口可连通（DSH web 启动需数十秒）。
    fn wait_http_ready(port: u16, max_secs: u64) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(max_secs);
        loop {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "端口 {port} 在 {max_secs}s 内未就绪"
            );
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}
