// Phase 2 · 启动闭环：模块挂载与 Tauri command 注册。
mod dsh_install;
mod gateway;
mod github;
mod inject;
mod instances;
mod keyvault;
mod launch_cancel;
mod llm_bridge;
mod log_sentinel;
mod market;
mod market_cache;
mod market_install;
mod mirror_config;
mod modpack;
mod node_runtime;
mod paths;
mod plugin_manage;
mod precheck;
mod process;
mod provider;
mod providers;
mod registry_meta;
mod skills;
mod snapshot;
mod source_archive;
#[cfg(test)]
mod verify_dsh;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            instance_list,
            instance_create,
            instance_register_external,
            instance_open_subdir,
            instance_delete,
            instance_get,
            instance_update,
            managed_root_info,
            node_system_info,
            node_managed_versions,
            node_download,
            node_download_cancel,
            node_default_get,
            node_default_set,
            inject_get_override,
            inject_set_override,
            inject_effective,
            inject_write_instance_files,
            inject_default_get,
            inject_default_set,
            inject_agents_global_get,
            inject_agents_global_set,
            mirror_get,
            mirror_set,
            mirror_test,
            source_download,
            source_download_cancel,
            source_open_dir,
            source_delete,
            source_status,
            keyvault_status,
            keyvault_set_enabled,
            keyvault_set_key,
            keyvault_set_models,
            keyvault_set_address,
            keyvault_clear_key,
            keyvault_forget_vendor,
            provider_fetch_models,
            providers_view,
            providers_set_order,
            providers_add_custom,
            providers_remove_custom,
            dsh_versions,
            instance_install_dsh,
            instance_launch,
            instance_launch_cancel,
            instance_stop,
            instance_process_state,
            instance_running_list,
            precheck_run,
            provider_route_key,
            instance_route_report,
            market_feed,
            market_versions,
            market_github_releases,
            plugin_list,
            plugin_install,
            plugin_remove,
            plugin_check_updates,
            plugin_set_enabled,
            plugin_set_child_enabled,
    skill_feed,
    skill_install,
    modpack_feed,
    modpack_export,
    modpack_peek,
    modpack_import,
    modpack_stage_drop,
])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 暴露托管根目录与就绪状态给前端（首次运行时触发目录初始化）。
#[tauri::command]
fn managed_root_info() -> Result<serde_json::Value, String> {
    let root = paths::ensure_managed_dirs()?;
    Ok(serde_json::json!({
        "root": root.display().to_string(),
        "nodesDir": paths::nodes_dir()?.display().to_string(),
        "instancesDir": paths::versions_dir()?.display().to_string(),
        "sourcesDir": paths::sources_dir()?.display().to_string(),
    }))
}

/// 系统 Node 检测结果（None = PATH 上没有 node）。
#[tauri::command]
fn node_system_info() -> Result<Option<node_runtime::SystemNode>, String> {
    Ok(node_runtime::detect_system_node())
}

/// 已托管的 Node 版本列表（升序）。
#[tauri::command]
fn node_managed_versions() -> Result<Vec<String>, String> {
    node_runtime::managed_versions()
}

/// 下载并安装便携版 Node（阻塞至完成；进度经事件 `node-download-progress` 推送）。
/// 发行包通道顺序由统一网关按镜像源设置决定。
#[tauri::command]
async fn node_download(
    app: tauri::AppHandle,
    version: String,
) -> Result<String, String> {
    use tauri::Emitter;
    // 版本合法性先行校验，避免把任意字符串拼进下载 URL。
    if node_runtime::parse_node_version(&version).is_none() {
        return Err(format!("非法版本号：{version:?}"));
    }
    let dist_bases = gateway::node_dist_bases();
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        node_runtime::download_and_install(&version, &dist_bases, &mut |received, total| {
            let _ = emitter.emit(
                node_runtime::PROGRESS_EVENT,
                serde_json::json!({ "received": received, "total": total }),
            );
        })
    })
    .await
    .map_err(|e| format!("下载任务崩溃：{e:?}"))?;
    match result {
        Ok(dir) => Ok(dir.display().to_string()),
        Err(e) => Err(e),
    }
}

/// 请求中止进行中的 Node 下载（停滞看门狗至多 60s 后生效）。
#[tauri::command]
fn node_download_cancel() -> Result<(), String> {
    node_runtime::cancel_download();
    Ok(())
}

/// 读全局默认托管 Node 版本。
#[tauri::command]
fn node_default_get() -> Result<Option<String>, String> {
    Ok(node_runtime::load_config()?.default_node_version)
}

/// 设置全局默认托管 Node 版本（必须已托管）。
#[tauri::command]
fn node_default_set(version: String) -> Result<(), String> {
    node_runtime::set_default_version(&version)
}

/// 枚举全部实例（camelCase 视图；持久化格式不受影响）。
#[tauri::command]
fn instance_list() -> Result<Vec<instances::InstanceView>, String> {
    Ok(instances::list_instances()?
        .iter()
        .map(instances::InstanceView::from)
        .collect())
}

/// 创建实例：名称 + 初始 profile。创建后立即桥接当前启用的模型路由
/// （llm_bridge::sync——新实例的 settings.yaml 必须带上路由段）。
#[tauri::command]
fn instance_create(name: String, profile: String) -> Result<instances::InstanceView, String> {
    let instance =
        instances::create_instance(&name, &profile).map(|i| instances::InstanceView::from(&i))?;
    llm_bridge::sync_instance(&instance.id)?;
    instances::ensure_skills_link(&instance.id)?;
    Ok(instance)
}

/// 「添加已有文件夹」：把既有 DSH 实例目录原地注册为实例（数据留在原地，
/// 删除 = 仅解除关联）。注册后立即桥接当前启用的模型路由。
#[tauri::command]
fn instance_register_external(folder: String) -> Result<instances::InstanceView, String> {
    let instance = instances::register_external(&folder)
        .map(|i| instances::InstanceView::from(&i))?;
    llm_bridge::sync_instance(&instance.id)?;
    // 外部实例不共享 skills（2026-09-20 需求方裁决方案 A）：不动用户文件夹的
    // 技能数据、不建联接；共享挂载仅托管实例。
    if instance.dir.is_none() {
        instances::ensure_skills_link(&instance.id)?;
    }
    Ok(instance)
}

/// 在文件管理器中打开实例子目录（skills / sessions / sources）。
#[tauri::command]
fn instance_open_subdir(
    app: tauri::AppHandle,
    id: String,
    which: String,
) -> Result<String, String> {
    instances::open_subdir(&app, &id, &which)
        .map(|dir| dir.display().to_string())
}

/// 删除实例（整目录移除，零残留）。
#[tauri::command]
fn instance_delete(id: String) -> Result<(), String> {
    instances::delete_instance(&id)
}

/// 读取单个实例。
#[tauri::command]
fn instance_get(id: String) -> Result<instances::InstanceView, String> {
    instances::get_instance(&id).map(|i| instances::InstanceView::from(&i))
}

/// 更新实例可变字段；None 表示不修改该字段。
#[tauri::command]
fn instance_update(
    id: String,
    name: Option<String>,
    profile: Option<String>,
    node_version: Option<Option<String>>,
) -> Result<instances::InstanceView, String> {
    if let Some(v) = &profile {
        if !instances::valid_profile(v) {
            return Err(format!("非法 profile：{v:?}（限小写字母/数字/连字符/下划线）"));
        }
    }
    instances::update_instance(&id, |i| {
        if let Some(v) = name {
            i.name = v;
        }
        if let Some(v) = profile {
            i.profile = v;
        }
        if let Some(v) = node_version {
            i.node_version = v;
        }
    })
    .map(|i| instances::InstanceView::from(&i))
}

/// 读实例的注入覆盖配置。
#[tauri::command]
fn inject_get_override(id: String) -> Result<inject::InjectConfig, String> {
    inject::get_override(&id)
}

/// 写实例的注入覆盖配置（校验 bootstrap-only 名单）。
#[tauri::command]
fn inject_set_override(id: String, config: inject::InjectConfig) -> Result<(), String> {
    inject::set_override(&id, &config)
}

/// 计算实例的生效注入配置（全局默认 ⊕ 覆盖）。
#[tauri::command]
fn inject_effective(id: String) -> Result<inject::InjectConfig, String> {
    inject::effective_config(&id)
}

/// 把生效配置写进实例目录（settings.yaml + .env），返回写入文件列表。
#[tauri::command]
fn inject_write_instance_files(id: String) -> Result<Vec<String>, String> {
    Ok(inject::write_instance_files(&id)?
        .into_iter()
        .map(|p| p.display().to_string())
        .collect())
}

/// 读全局默认注入模板。
#[tauri::command]
fn inject_default_get() -> Result<inject::InjectConfig, String> {
    inject::get_default_template()
}

/// 写全局默认注入模板。
#[tauri::command]
fn inject_default_set(config: inject::InjectConfig) -> Result<(), String> {
    inject::set_default_template(&config)
}

/// 读全局 AGENTS.md（None = 未设置；存储本体即根 AGENTS.md 文件，FILE-LAYOUT）。
#[tauri::command]
fn inject_agents_global_get() -> Result<Option<String>, String> {
    inject::get_global_agents_md()
}

/// 写全局 AGENTS.md（None = 清除；实例在下次配置物化时回落）。
#[tauri::command]
fn inject_agents_global_set(content: Option<String>) -> Result<(), String> {
    inject::set_global_agents_md(content.as_deref())
}

/// 读镜像源设置与内置源清单（版本通道 + 社区资源通道，FR-18 修订后结构）。
#[tauri::command]
fn mirror_get() -> Result<serde_json::Value, String> {
    let settings = mirror_config::load()?;
    Ok(serde_json::json!({
        "active": settings.active,
        "customPrefix": settings.custom_prefix,
        "communityActive": settings.community_active,
        "sources": [
            { "id": mirror_config::OFFICIAL_ID, "label": "官方源：npm 优先，GitHub 默认" },
            { "id": mirror_config::CUSTOM_ID, "label": "自定义前缀（GitHub 类）" },
        ],
        "communitySources": [
            { "id": mirror_config::OFFICIAL_ID, "label": "官方 GitHub（默认）" },
            { "id": mirror_config::GITEE_ID, "label": "Gitee" },
            { "id": mirror_config::DSHHUB_ID, "label": "dsh-hub.cc" },
        ],
    }))
}

/// 保存镜像源设置：三个字段均可选传——只更新提供的字段（load 内含遗留
/// npmmirror 迁移，保存前整体校验）。
#[tauri::command]
fn mirror_set(
    active: Option<String>,
    prefix: Option<String>,
    community_active: Option<String>,
) -> Result<(), String> {
    let mut settings = mirror_config::load()?;
    if let Some(v) = active {
        settings.active = v;
    }
    if let Some(v) = prefix {
        settings.custom_prefix = v;
    }
    if let Some(v) = community_active {
        settings.community_active = v;
    }
    mirror_config::save(&settings)
}

/// 测试指定源连通性（阻塞网络调用，spawn_blocking 执行）。
/// `source` 缺省测当前版本通道激活源；custom 需带 prefix。
/// 社区卡测官方时传 `github`（单探测点），版本卡传 `official`（双探测点）。
#[tauri::command]
async fn mirror_test(source: Option<String>, prefix: Option<String>) -> Result<gateway::ProbeResult, String> {
    let settings = mirror_config::load()?;
    let source_id = source.unwrap_or_else(|| settings.active.clone());
    let custom_prefix = prefix.unwrap_or_else(|| settings.custom_prefix.clone());
    tauri::async_runtime::spawn_blocking(move || gateway::test_source(&source_id, &custom_prefix))
        .await
        .map_err(|e| format!("连通性测试任务崩溃：{e:?}"))
}

/// 下载指定版本的 DSH 源码归档（FR-22；阻塞至完成，进度经事件
/// `source-download-progress` 推送 {version, instanceId, received, total}）。
/// `instance_id` = Some 时落 `versions/<id>/sources/`，None 落全局
/// `sources/<tag>/`（重名加副本后缀）。返回解压目录。
#[tauri::command]
async fn source_download(
    app: tauri::AppHandle,
    version: String,
    instance_id: Option<String>,
) -> Result<String, String> {
    use tauri::Emitter;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        source_archive::download(&version, instance_id.as_deref(), &mut |received, total| {
            let _ = emitter.emit(
                source_archive::PROGRESS_EVENT,
                serde_json::json!({
                    "version": version,
                    "instanceId": instance_id,
                    "received": received,
                    "total": total,
                }),
            );
        })
    })
    .await
    .map_err(|e| format!("源码下载任务崩溃：{e:?}"))?;
    result.map(|dir| dir.display().to_string())
}

/// 请求中止进行中的源码归档下载（与 Node 下载共用取消标志，边界见
/// node_runtime::DOWNLOAD_CANCEL 注释；停滞看门狗至多 60s 后生效）。
#[tauri::command]
fn source_download_cancel() -> Result<(), String> {
    node_runtime::cancel_download();
    Ok(())
}

/// 在系统文件管理器中打开指定版本的已下载源码目录（instance_id = Some 开
/// 版本内 sources/，None 开全局 tag 目录）。
#[tauri::command]
fn source_open_dir(
    app: tauri::AppHandle,
    version: String,
    instance_id: Option<String>,
) -> Result<(), String> {
    source_archive::open_dir(&app, &version, instance_id.as_deref())
}

/// 删除版本内的源码归档目录（不存在时幂等成功）。
#[tauri::command]
fn source_delete(instance_id: String) -> Result<(), String> {
    source_archive::delete_instance_sources(&instance_id)
}

/// 版本内源码归档是否已下载（卡片状态回显）。
#[tauri::command]
fn source_status(instance_id: String) -> Result<bool, String> {
    source_archive::instance_sources_downloaded(&instance_id)
}

/// 保险库状态（只读视图；无密钥材料）。多厂商形态：每厂商一条
/// {vendor, enabled, hasKey, envAuto, models}。
#[tauri::command]
fn keyvault_status() -> Result<keyvault::VaultStatus, String> {
    keyvault::status()
}

/// 勾选/取消启用厂商（与 Key 解耦；档位即时影响启动页候选与校验链）。
/// 启用集变化只落启动器状态：DSH 侧路由在**下一次启动**统一写入实例
/// （启动器侧配置改动一律不在运行中改实例文件）。
#[tauri::command]
fn keyvault_set_enabled(vendor: String, enabled: bool) -> Result<(), String> {
    keyvault::set_enabled(&vendor, enabled)
}

/// 保存 API Key（DPAPI 加密落盘；明文不回读、不留盘）。
#[tauri::command]
fn keyvault_set_key(vendor: String, key: String) -> Result<(), String> {
    keyvault::set_key(&vendor, &key)
}

/// 覆盖厂商的已添加模型列表（任意非空去重 id）。模型清单进路由——同启用集，
/// 下一次启动写入实例。
#[tauri::command]
fn keyvault_set_models(vendor: String, models: Vec<String>) -> Result<(), String> {
    keyvault::set_models(&vendor, &models)
}

/// 清除厂商已存 Key（保留启用状态与模型列表；无 Key 时幂等成功）。
#[tauri::command]
fn keyvault_clear_key(vendor: String) -> Result<(), String> {
    keyvault::clear_key(&vendor)
}

/// 把厂商从模型厂商列表整条移除（R5 按需添加式；Key/模型/启用态一并清除）。
#[tauri::command]
fn keyvault_forget_vendor(vendor: String) -> Result<(), String> {
    keyvault::forget_vendor(&vendor)
}

/// 保存提供方 API 地址覆盖；空串清除覆盖（回落默认地址）。地址进路由——
/// 下一次启动写入实例。
#[tauri::command]
fn keyvault_set_address(vendor: String, address: String) -> Result<(), String> {
    keyvault::set_address(&vendor, &address)
}

/// 从提供方 API 拉取可用模型列表（按目录接口形态分派：OpenAI 兼容 /
/// Responses / Anthropic / Gemini / Ollama `api/tags`；Key 取自保险库，
/// 地址空 = 目录默认地址；Ollama 允许未配 Key）。
#[tauri::command]
async fn provider_fetch_models(
    vendor: String,
    address: Option<String>,
) -> Result<Vec<String>, String> {
    let (style, default_base) = providers::resolve_style_and_base(&vendor)?;
    let base = match address.filter(|a| !a.trim().is_empty()) {
        Some(a) => a,
        None => default_base,
    };
    let key = if style == providers::ApiStyle::Ollama {
        keyvault::reveal_key_for(&vendor).unwrap_or_default()
    } else {
        keyvault::reveal_key_for(&vendor)?
    };
    tauri::async_runtime::spawn_blocking(move || provider::fetch_models_style(&base, &key, style))
        .await
        .map_err(|e| format!("拉取任务崩溃：{e:?}"))?
}

/// 提供方目录视图：内置 11 类 + 自定义 + 拖拽排序（FR-20）。
#[tauri::command]
fn providers_view() -> Result<providers::ProvidersView, String> {
    providers::view()
}

/// 保存拖拽排序（必须恰好覆盖全部内置 ∪ 自定义 id）。
#[tauri::command]
fn providers_set_order(order: Vec<String>) -> Result<(), String> {
    providers::set_order(&order)
}

/// 新增自定义提供方（与内置同权；id 自动生成）。
#[tauri::command]
fn providers_add_custom(
    name: String,
    base: String,
    style: String,
) -> Result<providers::CustomProvider, String> {
    let style = match style.to_lowercase().as_str() {
        "openai" => providers::ApiStyle::Openai,
        "responses" => providers::ApiStyle::Responses,
        "anthropic" => providers::ApiStyle::Anthropic,
        "gemini" => providers::ApiStyle::Gemini,
        "ollama" => providers::ApiStyle::Ollama,
        other => return Err(format!("未知接口形态：{other:?}")),
    };
    let added = providers::add_custom(&name, &base, style)?;
    Ok(added)
}

/// 删除自定义提供方（内置目录拒绝；连带清凭据记录）。
#[tauri::command]
fn providers_remove_custom(id: String) -> Result<(), String> {
    providers::remove_custom(&id)
}

/// 拉取可安装的 DSH 版本列表（经网关当前源；spawn_blocking 执行）。
#[tauri::command]
async fn dsh_versions() -> Result<Vec<registry_meta::DshVersion>, String> {
    tauri::async_runtime::spawn_blocking(registry_meta::fetch_versions)
        .await
        .map_err(|e| format!("版本列表任务崩溃：{e:?}"))?
}

/// 把官方 DSH 安装进指定实例（阻塞安装，进度走 `instance-install-progress` 事件）。
/// 多个实例的安装可并发调用——每个任务在独立 spawn_blocking 线程执行。
#[tauri::command]
async fn instance_install_dsh(
    app: tauri::AppHandle,
    id: String,
    version: String,
) -> Result<dsh_install::InstallOutcome, String> {
    use tauri::Emitter;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        let instance_id = id.clone();
        let stage_sink = move |stage: dsh_install::InstallStage| {
            let _ = emitter.emit(
                "instance-install-progress",
                serde_json::json!({ "instanceId": instance_id, "stage": stage }),
            );
        };
        // InstallStage 不是 Copy，闭包捕获需要再包一层引用转发。
        dsh_install::install(&id, &version, &mut |stage| stage_sink(stage))
    })
    .await
    .map_err(|e| format!("安装任务崩溃：{e:?}"))?;
    result
}

/// 运行启动前校验链（Node→Key→完整性→插件体检），返回分级报告。
#[tauri::command]
fn precheck_run(id: String) -> Result<precheck::Report, String> {
    precheck::run(&id)
}

/// 厂商在实例里的**提供方 id**（写进 `agent-default-model.provider` 的值）：
/// `deepseek` → `deepseek-official`，目录厂商走目录 route key，自描述厂商是自己
/// 的 id；None = 本启动器给它落不了路由（形态无对应协议）。前端据此把默认模型
/// 的 provider 与 model 一起写，避免 DSH 用 DeepSeek 通道发别家的模型 id。
#[tauri::command]
fn provider_route_key(vendor: String) -> Result<Option<String>, String> {
    llm_bridge::route_key_of(&vendor)
}

/// 实例路由对账报告（只读）：勾了启用却落不了地的路由（带原因）+ 实例里启动器
/// 不管的外来路由键。设置页「模型提供商」与启动前校验共用同一份判定。
#[tauri::command]
fn instance_route_report(id: String) -> Result<llm_bridge::RouteReport, String> {
    llm_bridge::route_report(&id)
}

/// 启动进度事件通道：前端加载动画按它推进百分比与步骤清单。
pub const LAUNCH_PROGRESS_EVENT: &str = "launch-progress";

/// 推一条启动进度（percent 0~100 + 人类可读阶段文案）。
fn emit_launch_progress(app: &tauri::AppHandle, instance_id: &str, percent: u8, stage: &str) {
    use tauri::Emitter;
    let _ = app.emit(
        LAUNCH_PROGRESS_EVENT,
        serde_json::json!({ "instanceId": instance_id, "percent": percent, "stage": stage }),
    );
}

/// 启动命令的完整答复：进程信息 + 校验链报告（前端加载动画据此渲染
/// 「体检通过 / 告警」清单——报告不再只在被拦截时才有出口）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchReply {
    pub instance_id: String,
    pub port: u16,
    pub pid: u32,
    pub report: precheck::Report,
    /// 本次启动的实例配置对账结果（插件/Skill/外部提供商相对上次启动的变更）。
    pub reconcile: snapshot::Reconcile,
}

/// 启动实例：先跑校验链，存在错误级条目时拒绝并返回报告；
/// `force = true` 表示用户看过报告后显式放行。
///
/// **每次调用都是一次独立的完整启动**：实例在跑就先停掉（含上一会话遗留的
/// 孤儿），再按原钉定端口拉起新进程——运行期注入的环境变量（厂商 Key、基址
/// 覆盖）因此每次都刷新。顺序是先校验后停进程：校验被拦截时不该把用户正在用
/// 的实例杀掉。
///
/// 启动链里统一写实例配置（`llm_bridge::sync_instance` 再生 settings.yaml +
/// `.env`）：启动器侧的配置改动一律只在下一次启动生效，运行中不改实例文件。
///
/// 对账（`snapshot`）只是诊断：采集/落盘失败记进结论，不阻断启动。
///
/// 取消（R3）：链上每个检查点都问一次 `launch_cancel::is_cancelled`，命中就地收尾
/// 并返回 `Err("已取消启动…")`；已经拉起的进程由 `stop_silently` 停掉（取消命令
/// 与这里的兜底各停一次，幂等）。取消命令早于 spawn 到达时，进程会被拉起一瞬间
/// 再被这里的兜底停掉——前端按「自己的取消标记」处置，不按错误文案猜。
#[tauri::command]
async fn instance_launch(
    app: tauri::AppHandle,
    id: String,
    force: Option<bool>,
) -> Result<LaunchReply, String> {
    // 本次启动开始：清掉上一次留下的取消标记（否则会误伤这一次）。
    launch_cancel::clear(&id);
    // 运行环境自愈：没有可用 Node 就先自动代管一个已知良好版本（与安装链
    // 同一套兜底），预检的 Node 项于是成为"结论"而不是新手要自己解决的任务。
    if !node_runtime::node_available() {
        emit_launch_progress(&app, &id, 5, "正在准备 Node 运行环境…");
        let node_app = app.clone();
        let node_id = id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            node_runtime::ensure_default_node(&mut |event| {
                let detail = match event {
                    node_runtime::EnsureEvent::Downloading { version } => {
                        format!("正在自动代管 Node v{version}…")
                    }
                    node_runtime::EnsureEvent::Bytes { received, total } => format!(
                        "正在自动代管 Node（{} / {}）",
                        node_runtime::fmt_size(received),
                        node_runtime::fmt_size(total)
                    ),
                };
                emit_launch_progress(&node_app, &node_id, 6, &detail);
            })
        })
        .await
        .map_err(|e| format!("运行环境准备任务崩溃：{e:?}"))??;
        ensure_not_cancelled(&id)?;
    }
    emit_launch_progress(&app, &id, 8, "正在校验运行环境…");
    let report = precheck::run(&id)?;
    ensure_not_cancelled(&id)?;
    if report.blocked && force != Some(true) {
        return Err(serde_json::to_string(&report)
            .unwrap_or_else(|_| r#"{"items":[],"blocked":true}"#.into()));
    }
    emit_launch_progress(&app, &id, 45, "正在对账实例配置…");
    let state = process::process_state(&id)?;
    let baseline = snapshot::load(&id);
    let reconcile = match snapshot::collect(&id) {
        Ok(current) => {
            let mut reconcile = snapshot::diff(baseline.as_ref(), &current);
            // 基线落盘失败不阻断启动，但要如实说明：下次启动会退化成"首次"
            // （只报构成、不报新增），用户能看出对账没生效。
            if let Err(err) = snapshot::save(&id, &current) {
                reconcile.note = Some(format!("实例快照未落盘：{err}"));
            }
            reconcile
        }
        Err(err) => snapshot::Reconcile {
            note: Some(format!("实例配置对账失败：{err}")),
            ..Default::default()
        },
    };
    ensure_not_cancelled(&id)?;
    emit_launch_progress(&app, &id, 55, "正在挂载全局技能目录…");
    instances::ensure_skills_link(&id)?;
    ensure_not_cancelled(&id)?;
    emit_launch_progress(&app, &id, 55, "正在同步模型配置…");
    if state.running {
        emit_launch_progress(&app, &id, 64, "实例正在运行，正在重启实例…");
        process::stop_for_restart(&id)?;
    }
    // 配置同步是「不改实例就跑不起来」的必要一步，取消不半途跳过它：取消在
    // 这之后落地时，改动已写入——它是幂等的，下次启动会照常再做一遍。
    llm_bridge::sync_instance(&id)?;
    ensure_not_cancelled(&id)?;
    emit_launch_progress(&app, &id, 70, "正在拉起实例进程…");
    let keep_pinned_port = state.running;
    // 拉起那一步把 id 移进阻塞任务；取消兜底还要问一次令牌，留一份副本。
    let cancel_id = id.clone();
    let outcome =
        tauri::async_runtime::spawn_blocking(move || process::launch(Some(&app), &id, keep_pinned_port))
            .await
            .map_err(|e| format!("启动任务崩溃：{e:?}"))??;
    // spawn 后兜底：取消可能就落在「拉起中」这一小段里——进程已经起来了，就地停掉。
    if launch_cancel::is_cancelled(&cancel_id) {
        process::stop_silently(&cancel_id)?;
        return Err(CANCELLED_MESSAGE.to_string());
    }
    Ok(LaunchReply {
        instance_id: outcome.instance_id,
        port: outcome.port,
        pid: outcome.pid,
        report,
        reconcile,
    })
}

/// 取消本次启动的返回文案（前端不据此判断，只用于日志与兜底展示）。
const CANCELLED_MESSAGE: &str = "已取消启动";

/// 链上检查点：已取消就中断本次启动。
fn ensure_not_cancelled(instance_id: &str) -> Result<(), String> {
    if launch_cancel::is_cancelled(instance_id) {
        return Err(CANCELLED_MESSAGE.to_string());
    }
    Ok(())
}

/// 取消本次启动：登记令牌（启动链下一个检查点就会收尾），并把**已经拉起**的进程
/// 停掉——取消可能发生在「进程已开始拉起、链还没走到检查点」的窗口里，只登记
/// 令牌的话那个进程会活下来。停止抑制它那条 instance-exit（前端是自己点的取消，
/// 收尾依据是本次调用的返回值与启动链的 Err），幂等：没在跑就直接成功。
#[tauri::command]
fn instance_launch_cancel(id: String) -> Result<(), String> {
    launch_cancel::request(&id);
    process::stop_silently(&id)
}

/// 实例进程状态：是否仍在运行（含上一会话遗留的孤儿）、是否需要在启动时重启。
#[tauri::command]
fn instance_process_state(id: String) -> Result<process::ProcessState, String> {
    process::process_state(&id)
}

/// 停止实例进程树；未在运行时幂等成功。
#[tauri::command]
fn instance_stop(id: String) -> Result<(), String> {
    process::stop(&id)
}

/// 当前运行中的实例列表（端口/进程）。
#[tauri::command]
fn instance_running_list() -> Vec<process::RunStatus> {
    process::list_running()
}

/// 拉取插件市场信息流（FR-03；T-4 口径聚合 + 离线快照降级，社区镜像源 =
/// dshhub 时走 dsh-hub.cc 数据源）。参数缺省 = 推荐流（热度序、全量）。
#[tauri::command]
async fn market_feed(
    sort: Option<String>,
    keyword: Option<String>,
    source: Option<String>,
    kind: Option<String>,
) -> Result<market::MarketFeed, String> {
    let query = market::MarketQuery {
        sort: sort.unwrap_or_else(|| "hot".into()),
        keyword: keyword.unwrap_or_default(),
        source: source.unwrap_or_else(|| "all".into()),
        kind: kind.unwrap_or_else(|| "all".into()),
    };
    tauri::async_runtime::spawn_blocking(move || market::fetch_feed(&query))
        .await
        .map_err(|e| format!("市场拉取任务崩溃：{e:?}"))?
}

/// 拉取 npm 包的历史版本与每版本周下载量（折叠卡片展开视图；T-4 口径），
/// 同时回带「是否声明插件层」（源头拦截依赖库冒充插件）。
#[tauri::command]
async fn market_versions(package: String) -> Result<market::VersionsReply, String> {
    tauri::async_runtime::spawn_blocking(move || market::fetch_versions(&package))
        .await
        .map_err(|e| format!("版本拉取任务崩溃：{e:?}"))?
}

/// GitHub 仓库 Releases（git 直装卡详情页的「历代版本」；网关 + Token + ETag 缓存）。
#[tauri::command]
async fn market_github_releases(repo: String) -> Result<Vec<github::ReleaseSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || github::repo_releases(&repo))
        .await
        .map_err(|e| format!("Releases 拉取任务崩溃：{e:?}"))?
}

/// 已装插件三分组清单（dsh 内置 / 第三方 / 自建，内置组不可删）。
#[tauri::command]
fn plugin_list(id: String) -> Result<plugin_manage::PluginList, String> {
    plugin_manage::list(&id)
}

/// 从市场/展开视图安装一个插件到实例（官方 `dsh plugin add` 链；进度经
/// `plugin-install-progress` 事件 {instanceId, spec, detail} 推送——spec 供
/// 前端把插件与整合包的并发安装分流到各自状态行）。
#[tauri::command]
async fn plugin_install(
    app: tauri::AppHandle,
    id: String,
    spec: String,
) -> Result<market_install::PluginInstallOutcome, String> {
    use tauri::Emitter;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        let instance_id = id.clone();
        market_install::plugin_add(&id, &spec, &mut |detail| {
            let _ = emitter.emit(
                market_install::PROGRESS_EVENT,
                serde_json::json!({ "instanceId": instance_id, "spec": spec, "detail": detail }),
            );
        })
    })
    .await
    .map_err(|e| format!("插件安装任务崩溃：{e:?}"))?;
    result
}

/// 从实例 profile 移除一个插件（内置组拒绝；官方 remove 链）。
#[tauri::command]
async fn plugin_remove(
    app: tauri::AppHandle,
    id: String,
    name: String,
) -> Result<(), String> {
    use tauri::Emitter;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        plugin_manage::remove(&id, &name, &mut |detail| {
            let _ = emitter.emit(
                market_install::PROGRESS_EVENT,
                serde_json::json!({ "instanceId": id, "detail": detail }),
            );
        })
    })
    .await
    .map_err(|e| format!("插件移除任务崩溃：{e:?}"))?;
    result
}

/// 检查第三方插件更新（对比 npm dist-tags.latest；显式触发）。
#[tauri::command]
async fn plugin_check_updates(id: String) -> Result<Vec<plugin_manage::PluginUpdate>, String> {
    tauri::async_runtime::spawn_blocking(move || plugin_manage::check_updates(&id))
        .await
        .map_err(|e| format!("更新检查任务崩溃：{e:?}"))?
}

/// 切换插件启用状态（bundles 层成员切换；停用 = 依赖保留、下次启动生效；
/// 官方 add/remove 的 reconcile 会复活停用项——边界由前端告知）。
#[tauri::command]
fn plugin_set_enabled(id: String, name: String, enabled: bool) -> Result<(), String> {
    plugin_manage::set_enabled(&id, &name, enabled)
}

/// 组合包子插件启停（写版本用户层 cordis.patch.yml 的 `{id, disabled}` 覆盖；
/// 运行中实例经 DSH 热重载即时生效，未运行则下次启动生效）。
#[tauri::command]
fn plugin_set_child_enabled(id: String, row_id: String, enabled: bool) -> Result<(), String> {
    plugin_manage::set_child_enabled(&id, &row_id, enabled)
}

/// Skill 中心信息流（GitHub `dsh-skill` 聚合 + 离线快照降级；FR-14）。
#[tauri::command]
async fn skill_feed() -> Result<skills::SkillFeed, String> {
    tauri::async_runtime::spawn_blocking(skills::fetch_feed)
        .await
        .map_err(|e| format!("技能目录任务崩溃：{e:?}"))?
}

/// 安装一个技能仓库到实例（落点 `<实例>/skills/<name>/`；进度经
/// `skill-install-progress` 事件推送）。
#[tauri::command]
async fn skill_install(
    app: tauri::AppHandle,
    id: String,
    repo: String,
) -> Result<skills::SkillInstallOutcome, String> {
    use tauri::Emitter;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        let instance_id = id.clone();
        skills::install(&id, &repo, &mut |detail| {
            let _ = emitter.emit(
                skills::PROGRESS_EVENT,
                serde_json::json!({ "instanceId": instance_id, "detail": detail }),
            );
        })
    })
    .await
    .map_err(|e| format!("技能安装任务崩溃：{e:?}"))?;
    result
}

/// 整合包栏信息流（P3 浏览层：聚合型资源目录——全家桶/聚合包/套件；
/// 文件级导入导出属 P4）。
#[tauri::command]
async fn modpack_feed(
    sort: Option<String>,
    keyword: Option<String>,
    source: Option<String>,
) -> Result<market::MarketFeed, String> {
    let query = market::MarketQuery {
        sort: sort.unwrap_or_else(|| "hot".into()),
        keyword: keyword.unwrap_or_default(),
        source: source.unwrap_or_else(|| "all".into()),
        kind: "all".into(),
    };
    tauri::async_runtime::spawn_blocking(move || market::fetch_modpack_feed(&query))
        .await
        .map_err(|e| format!("整合包目录任务崩溃：{e:?}"))?
}

/// 导出整合包（P4：勾选插件 + 设置内容 → 自包含离线 tgz；进度经
/// `modpack-progress` 事件推送）。dest = 保存对话框返回的完整路径。
#[tauri::command]
async fn modpack_export(
    app: tauri::AppHandle,
    opts: modpack::ExportOptions,
    dest: String,
) -> Result<modpack::Manifest, String> {
    use tauri::Emitter;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        modpack::export_pack(&opts, std::path::Path::new(&dest), &mut |detail| {
            let _ = emitter.emit("modpack-progress", serde_json::json!({ "detail": detail }));
        })
    })
    .await
    .map_err(|e| format!("导出任务崩溃：{e:?}"))?;
    result
}

/// 读取整合包清单（不解包；导入确认弹窗的预览数据源）。
#[tauri::command]
async fn modpack_peek(path: String) -> Result<modpack::Manifest, String> {
    tauri::async_runtime::spawn_blocking(move || modpack::peek_manifest(std::path::Path::new(&path)))
        .await
        .map_err(|e| format!("清单读取任务崩溃：{e:?}"))?
}

/// 导入整合包（P4：file: 离线安装 + 失败回滚 + 设置按落点写入；进度走
/// `modpack-progress`）。`settings_target` = "global"/"instance"（无设置节时传 None）。
#[tauri::command]
async fn modpack_import(
    app: tauri::AppHandle,
    path: String,
    instance_id: String,
    settings_target: Option<String>,
) -> Result<modpack::ImportReport, String> {
    use tauri::Emitter;
    let target = match settings_target.as_deref() {
        None => None,
        Some("global") => Some(modpack::SettingsTarget::Global),
        Some("instance") => Some(modpack::SettingsTarget::Instance),
        Some(other) => return Err(format!("未知设置落点：{other:?}")),
    };
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        modpack::import_pack(
            std::path::Path::new(&path),
            &instance_id,
            target,
            &mut |detail| {
                let _ = emitter.emit("modpack-progress", serde_json::json!({ "detail": detail }));
            },
        )
    })
    .await
    .map_err(|e| format!("导入任务崩溃：{e:?}"))?;
    result
}

/// 拖入暂存：把 HTML5 drop 拿到的文件字节落到临时文件（原生拖放已禁用，
/// WebView 的 File 对象拿不到真实路径，改走内容通道）。返回可导入的临时
/// tgz 路径；文件名只取 basename 防穿越，内容原样落盘。
#[tauri::command]
fn modpack_stage_drop(name: String, bytes: Vec<u8>) -> Result<String, String> {
    let base = name
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("pack.tgz")
        .to_string();
    if base.trim().is_empty() {
        return Err("拖入文件名为空".into());
    }
    let dir = std::env::temp_dir().join(format!("dsh-modpack-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建暂存目录失败：{e}"))?;
    let path = dir.join(&base);
    std::fs::write(&path, bytes).map_err(|e| format!("写入暂存文件失败：{e}"))?;
    Ok(path.display().to_string())
}
