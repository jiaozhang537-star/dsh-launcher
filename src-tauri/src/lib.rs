// Phase 2 · 启动闭环：模块挂载与 Tauri command 注册。
mod dsh_install;
mod gateway;
mod github;
mod inject;
mod instances;
mod keyvault;
mod launch_cancel;
mod launch_flow;
mod llm_bridge;
mod log_sentinel;
mod market;
mod market_cache;
mod market_install;
mod mirror_config;
mod modpack;
mod node_runtime;
mod paths;
mod personalize;
mod plugin_manage;
mod precheck;
mod process;
mod provider;
mod providers;
mod registry_meta;
mod skills;
mod snapshot;
mod source_archive;
mod task_queue;
mod self_update;
/// 无头 CLI（P7.5 / FR-26）：`main.rs` 分流入口，pub 供 bin crate 调用。
pub mod cli;
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
            instance_size,
            instance_set_advanced,
            instance_set_isolation,
            update_check,
            update_download,
            update_install,
            instance_get,
            instance_update,
            managed_root_info,
            node_system_info,
            node_managed_versions,
            node_download,
            task_pause,
            task_resume,
            task_cancel,
            max_downloads_get,
            max_downloads_set,
            proxy_get,
            proxy_set,
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
    ui_background_open,
    ui_background_random,
    ui_background_clear,
    ui_logo_get,
    ui_logo_set,
    ui_logo_clear,
])
        .setup(|app| {
            // 禁止最大化守卫安装（P6 验收补丁，需求方裁决「禁止放大、保留拖边框改尺寸」）。
            // 注意：不要在此做任何 set_size / DPI 换算——conf 850×500 + center:true 的
            // 创建路径本来就正确（c4d0e57a 实测 1071×630 物理）；此前的
            // current_monitor().scale_factor() 修正是用**监视器 DPI**（169%）覆盖了
            // **内容渲染 DPI**（126%），反而把窗口放大到 1440×846——已撤。
            #[cfg(all(windows, target_arch = "x86_64"))]
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main") {
                    if let Ok(hwnd) = window.hwnd() {
                        window_guard::install(hwnd.0 as isize);
                    }
                    // 上游 wry/WebView2Feedback#2549：无装饰+透明窗口在非标准缩放下
                    // WebView2 子窗边界可能小于父窗客户区（wry 的 SetBounds 是异步
                    // SWP_ASYNCWINDOWPOS，创建期可能还没跑完一次完整布局）→ 右缘
                    // 露出底色缝隙。此处 +1px/-1px 微调触发两次 WM_SIZE → wry 重设
                    // 子窗边界铺满父窗客户区，消除缝隙。
                    let size = window.inner_size()?;
                    let _ = window.set_size(tauri::PhysicalSize::new(
                        size.width + 1,
                        size.height + 1,
                    ));
                    let _ = window.set_size(size);
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 最大化守卫（P6 验收补丁，需求方裁决「禁止放大、保留拖边框改尺寸」）。
///
/// 分层事实（2026-10-01 源码级复核，tao 0.35.3 / tauri 2.11.5）：
/// - conf `maximizable: false` 只移除 WS_MAXIMIZEBOX——灰化系统菜单项、断 Win+Up，
///   **拦不住**直接投递的 `WM_SYSCOMMAND(SC_MAXIMIZE)`（UIA/无障碍 WindowPattern 路径）；
/// - 标题栏 `startDragging` 投递 `WM_NCLBUTTONDOWN(HTCAPTION)`，快速双击被系统合成
///   `WM_NCLBUTTONDBLCLK(HTCAPTION)` → DefWindowProc 最大化（上游 tauri#11945）——
///   phase-02 的「移除 drag-region」只禁了前端双击处理，没禁这条系统路径；
/// - 此前试验过的「Resized 事件里 unmaximize」守卫已**废除**：SW_RESTORE 在 126%
///   非标准缩放下把窗口还原到损坏几何（实测 1440×821、约 1000×600，低于 minWidth），
///   并在交互式缩放的模态循环内触发 ShowWindow 中止拖拽（边缘放大失效的根源）。
///
/// 本模块改用 **comctl32 SetWindowSubclass**(Windows 官方的多子类堆叠机制,链序由
/// 系统管理,与 tao/wry 的窗口过程链互不干扰):仅吃掉上述两条消息(返回 0),其余全部
/// `DefSubclassProc` 透传——不触碰 WM_NCHITTEST / WM_GETMINMAXINFO / 模态缩放循环,
/// 边缘双向缩放零影响;窗口**永不进入最大化态**,也就永不 SW_RESTORE,几何损坏源归零。
/// (历史:GWLP_WNDPROC 替换式子类与 tao/wry 过程链相互踩踏,实机复现主窗口几何
/// 损坏飞出屏幕外,已弃——见 phase-06 日志 2026-10-01 分离实验。)
#[cfg(all(windows, target_arch = "x86_64"))]
mod window_guard {
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{HTCAPTION, WM_NCLBUTTONDBLCLK, WM_SYSCOMMAND};

    /// 子类实例标识(同 hwnd 同 id 重复安装会失败;单窗口单子类,取 1)。
    const SUBCLASS_ID: usize = 1;

    /// SC_MAXIMIZE 低 4 位为系统内部位,标准判定法:掩掉后比较。
    const SC_MAXIMIZE_MASK: usize = 0xF030;

    /// 安装守卫:挂子类过程。必须在窗口创建后调用(run() 的 setup 内,单窗口一次)。
    pub(super) fn install(hwnd: isize) {
        unsafe {
            SetWindowSubclass(HWND(hwnd as *mut _), Some(guard_subclass_proc), SUBCLASS_ID, 0)
                .ok()
                .expect("安装最大化守卫子类失败");
        }
    }

    unsafe extern "system" fn guard_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        let blocked = match msg {
            // UIA/无障碍/系统菜单/Win+Up 的最大化直投路径。
            WM_SYSCOMMAND => (wparam.0 & 0xFFF0) == SC_MAXIMIZE_MASK,
            // 标题栏双击(startDragging 的 HTCAPTION 非客户双击合成)。
            WM_NCLBUTTONDBLCLK => wparam.0 as u32 == HTCAPTION,
            _ => false,
        };
        if blocked {
            return LRESULT(0);
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
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
/// FR-09：任务键 `node-<version>`，支持暂停 / 继续 / 取消（task_* 命令）与
/// Range 断点续传；受并发额度闸约束（排队等待期间同样可暂停 / 取消）。
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
    let key = format!("node-{version}");
    let control = task_queue::register(&key)?;
    let dist_bases = gateway::node_dist_bases();
    let handle = app.clone();
    let part = task_queue::part_path(&key);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        let outcome = (|| {
            let _permit = task_queue::acquire(&control)?;
            node_runtime::download_and_install(
                &version,
                &dist_bases,
                Some(&control),
                &part,
                &mut |received, total| {
                    let _ = emitter.emit(
                        node_runtime::PROGRESS_EVENT,
                        serde_json::json!({ "received": received, "total": total }),
                    );
                },
            )
        })();
        task_queue::finish(&key);
        outcome
    })
    .await
    .map_err(|e| format!("下载任务崩溃：{e:?}"))?;
    result.map(|dir| dir.display().to_string())
}

/// 任务控制（FR-09）：暂停 / 继续 / 取消，key = 前端任务 store 的任务键。
/// 对「排队等待并发额度」的任务同样生效（等待循环过检查点）。
#[tauri::command]
fn task_pause(key: String) -> Result<(), String> {
    let control =
        task_queue::lookup(&key).ok_or_else(|| format!("任务不存在或已结束：{key}"))?;
    control.pause();
    Ok(())
}

#[tauri::command]
fn task_resume(key: String) -> Result<(), String> {
    let control =
        task_queue::lookup(&key).ok_or_else(|| format!("任务不存在或已结束：{key}"))?;
    control.resume();
    Ok(())
}

#[tauri::command]
fn task_cancel(key: String) -> Result<(), String> {
    let control =
        task_queue::lookup(&key).ok_or_else(|| format!("任务不存在或已结束：{key}"))?;
    control.cancel();
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

/// 下载并发上限（FR-09/FR-11；默认 3，收敛 1~10）。
#[tauri::command]
fn max_downloads_get() -> Result<u32, String> {
    Ok(node_runtime::load_config()?.max_downloads.unwrap_or(3))
}

#[tauri::command]
fn max_downloads_set(value: u32) -> Result<(), String> {
    if !(1..=10).contains(&value) {
        return Err(format!("并发数须在 1~10 之间：{value}"));
    }
    let mut config = node_runtime::load_config()?;
    config.max_downloads = Some(value);
    node_runtime::save_config(&config)
}

/// HTTP(S) 代理（FR-11；空串 = 直连。设置时经 ureq 校验地址合法性）。
#[tauri::command]
fn proxy_get() -> Result<String, String> {
    Ok(node_runtime::load_config()?.proxy.unwrap_or_default())
}

#[tauri::command]
fn proxy_set(proxy: String) -> Result<(), String> {
    let trimmed = proxy.trim().to_string();
    if !trimmed.is_empty() {
        ureq::Proxy::new(&trimmed).map_err(|e| format!("代理地址无效：{e}"))?;
    }
    let mut config = node_runtime::load_config()?;
    config.proxy = if trimmed.is_empty() { None } else { Some(trimmed) };
    node_runtime::save_config(&config)
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

/// 实例目录体量统计（FR-10 删除确认的「数据量级说明」；尽力而为参考值）。
#[tauri::command]
fn instance_size(id: String) -> Result<instances::DirStats, String> {
    instances::dir_stats(&id)
}

/// FR-12：实例级高级设置（启动参数 / 工作目录覆盖；None / 空 = 清除覆盖）。
#[tauri::command]
fn instance_set_advanced(
    id: String,
    launch_args: Option<String>,
    work_dir: Option<String>,
) -> Result<(), String> {
    instances::set_advanced(&id, launch_args, work_dir)
}

/// FR-17：切换实例隔离（插件目录 junction 迁移 + 配置单源切换）。返回人读报告。
#[tauri::command]
fn instance_set_isolation(id: String, isolation: bool) -> Result<String, String> {
    instances::set_isolation(&id, isolation)
}

/// FR-13：检查启动器更新（GitHub Releases；更新源 = config.json 的 update_repo，
/// 未配置时如实报告）。阻塞网络调用，spawn_blocking 执行。
#[tauri::command]
async fn update_check(current_version: String) -> Result<self_update::UpdateStatus, String> {
    tauri::async_runtime::spawn_blocking(move || self_update::check(&current_version))
        .await
        .map_err(|e| format!("检查更新任务崩溃：{e:?}"))?
}

/// FR-13：下载最新安装包（统一队列任务键 `update`，断点续传）。返回本地路径。
#[tauri::command]
async fn update_download(current_version: String) -> Result<String, String> {
    let key = "update".to_string();
    let control = task_queue::register(&key)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let outcome = (|| {
            let _permit = task_queue::acquire(&control)?;
            self_update::download(&current_version, &control)
        })();
        task_queue::finish(&key);
        outcome
    })
    .await
    .map_err(|e| format!("更新下载任务崩溃：{e:?}"))?;
    result.map(|p| p.display().to_string())
}

/// FR-13：打开已下载的安装包（交给系统安装器，启动器自身不落地安装逻辑）。
#[tauri::command]
fn update_install(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("打开安装包失败：{e}"))
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
/// FR-09：任务键与前端一致（source_archive::task_key），支持暂停 / 继续 /
/// 取消与 Range 断点续传；受并发额度闸约束。
#[tauri::command]
async fn source_download(
    app: tauri::AppHandle,
    version: String,
    instance_id: Option<String>,
) -> Result<String, String> {
    use tauri::Emitter;
    let key = source_archive::task_key(&version, instance_id.as_deref());
    let control = task_queue::register(&key)?;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        let outcome = (|| {
            let _permit = task_queue::acquire(&control)?;
            source_archive::download(
                &version,
                instance_id.as_deref(),
                Some(&control),
                &mut |received, total| {
                    let _ = emitter.emit(
                        source_archive::PROGRESS_EVENT,
                        serde_json::json!({
                            "version": version,
                            "instanceId": instance_id,
                            "received": received,
                            "total": total,
                        }),
                    );
                },
            )
        })();
        task_queue::finish(&key);
        outcome
    })
    .await
    .map_err(|e| format!("源码下载任务崩溃：{e:?}"))?;
    result.map(|dir| dir.display().to_string())
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
/// FR-09：任务键 `install-<instanceId>`，支持取消（npm 子进程链杀进程收尾；
/// 暂停/继续不适用于子进程链）；受并发额度闸约束。
#[tauri::command]
async fn instance_install_dsh(
    app: tauri::AppHandle,
    id: String,
    version: String,
) -> Result<dsh_install::InstallOutcome, String> {
    use tauri::Emitter;
    let key = format!("install-{id}");
    let control = task_queue::register(&key)?;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        let instance_id = id.clone();
        let outcome = (|| {
            let _permit = task_queue::acquire(&control)?;
            let stage_sink = move |stage: dsh_install::InstallStage| {
                let _ = emitter.emit(
                    "instance-install-progress",
                    serde_json::json!({ "instanceId": instance_id, "stage": stage }),
                );
            };
            // InstallStage 不是 Copy，闭包捕获需要再包一层引用转发。
            dsh_install::install(&id, &version, &mut |stage| stage_sink(stage), Some(&control))
        })();
        task_queue::finish(&key);
        outcome
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
///
/// 编排本体在 `launch_flow::launch_pipeline`（P7.5 起与无头 CLI 共用同一份）；
/// 答复形状 = 旧 LaunchReply（camelCase：instanceId/port/pid/report/reconcile）。
#[tauri::command]
async fn instance_launch(
    app: tauri::AppHandle,
    id: String,
    force: Option<bool>,
) -> Result<launch_flow::LaunchResult, String> {
    // 整条链跑在阻塞线程池上（Node 自愈/拉起本就是阻塞活）；进度回调在池线程
    // 上发 launch-progress 事件，事件序列与抽取前一致。
    let launch_app = app.clone();
    let launch_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        launch_flow::launch_pipeline(
            Some(&launch_app),
            &launch_id,
            force.unwrap_or(false),
            &mut |percent, stage| emit_launch_progress(&launch_app, &launch_id, percent, stage),
        )
    })
    .await
    .map_err(|e| format!("启动任务崩溃：{e:?}"))?
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

/// 停止实例进程树；未在运行时幂等成功。启动链进行中同样生效（对抗审查
/// R2 P2-01）：登记取消令牌让链在下一个检查点收尾——旧行为是 no-op 却返回
/// 成功，进程随后照常被拉起，用户视角「刚停止成功的实例自己跑起来了」。
/// R1 的代际化保证该标记不误伤下一次启动（标记挂在当前代，新启动用更高代）。
#[tauri::command]
fn instance_stop(id: String) -> Result<(), String> {
    launch_cancel::request(&id);
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
/// FR-09：任务键 `plugin-<spec>`，支持取消；受并发额度闸约束。
#[tauri::command]
async fn plugin_install(
    app: tauri::AppHandle,
    id: String,
    spec: String,
) -> Result<market_install::PluginInstallOutcome, String> {
    use tauri::Emitter;
    let key = format!("plugin-{spec}");
    let control = task_queue::register(&key)?;
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emitter = handle.clone();
        let instance_id = id.clone();
        let outcome = (|| {
            let _permit = task_queue::acquire(&control)?;
            market_install::plugin_add(&id, &spec, &mut |detail| {
                let _ = emitter.emit(
                    market_install::PROGRESS_EVENT,
                    serde_json::json!({ "instanceId": instance_id, "spec": spec, "detail": detail }),
                );
            }, Some(&control))
        })();
        task_queue::finish(&key);
        outcome
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

// ===== 个性化（FR-27，P7.5）：命令薄壳，本体在 personalize.rs =====

/// 打开背景图片文件夹（不存在则创建）。
#[tauri::command]
fn ui_background_open(app: tauri::AppHandle) -> Result<(), String> {
    personalize::background_open(&app)
}

/// 随机取一张背景图（data URL；空文件夹 = None）。
#[tauri::command]
fn ui_background_random() -> Result<Option<personalize::PersonalizeImage>, String> {
    personalize::background_random()
}

/// 清空背景图片文件夹里的图片，返回清掉张数。
#[tauri::command]
fn ui_background_clear() -> Result<u32, String> {
    personalize::background_clear().map(|n| n as u32)
}

/// 当前标题栏图片（未设置 = None）。
#[tauri::command]
fn ui_logo_get() -> Result<Option<personalize::PersonalizeImage>, String> {
    personalize::logo_get()
}

/// 从文件设置标题栏图片（复制进托管根）。
#[tauri::command]
fn ui_logo_set(src: String) -> Result<personalize::PersonalizeImage, String> {
    personalize::logo_set_from(&src)
}

/// 清除标题栏图片。
#[tauri::command]
fn ui_logo_clear() -> Result<bool, String> {
    personalize::logo_clear()
}
