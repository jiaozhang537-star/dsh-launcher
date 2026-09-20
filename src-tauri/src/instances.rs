//! 实例存储：`instances.json` 注册表 + 每实例目录的生命周期。
//!
//! 一个实例 = 一个独立的 `DSH_HOME`（R-1 侦察结论：DSH 全部用户数据——
//! profiles / sessions / settings.yaml / .credentials.yaml——都落在
//! `DSH_HOME` 内，因此「删实例目录 = 零残留」）。启动器只在实例目录里放
//! 自己的元数据 `instance.json`，其余由 DSH 自行生长。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::paths;

/// 注册表变更的进程内互斥锁：并行的安装任务各自 load-modify-save，
/// 没有这把锁时后写者会覆盖先写者的行（Phase 2 并行下载引入的真实竞态）。
static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

/// 实例元数据（持久化于实例目录的 `instance.json` 与根注册表两处）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    /// 稳定标识（目录名），创建后不可变。
    pub id: String,
    /// 用户可见名称。
    pub name: String,
    /// 默认启动的 DSH profile（web / headless / 自定义）。
    #[serde(default = "default_profile")]
    pub profile: String,
    /// 该实例覆盖的托管 Node 版本；空 = 使用全局默认。
    #[serde(default)]
    pub node_version: Option<String>,
    /// 已安装的 DSH 本体版本；空 = 尚未安装（不可启动）。
    #[serde(default)]
    pub dsh_version: Option<String>,
    /// 最近一次启动时间（RFC3339；空 = 从未启动）。主页「最近使用」排序依据。
    #[serde(default)]
    pub last_launched_at: Option<String>,
    /// 实例专属 Web 端口。首次启动时分配并永久绑定——同一实例永远用同一入口
    /// 地址，浏览器书签与会话心智不漂移；仅当端口被外部程序抢占时自愈式重锚定。
    #[serde(default)]
    pub port: Option<u16>,
    /// 创建时间（RFC3339，本地记录用）。
    pub created_at: String,
    /// 外部实例目录（原地引用，2026-09-12 需求方定稿）：Some = 经「添加已有文件夹」
    /// 注册的绝对路径，数据留在原地，删除 = 仅解除关联不删文件夹；None = 托管实例，
    /// 落 `versions/<id>/`。注册表是唯一事实来源。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

fn default_profile() -> String {
    "headless".to_string()
}

/// 实例的前端视图（IPC 专用）：字段键为 camelCase。
///
/// `Instance` 本体是持久化格式（instances.json / instance.json，snake_case 键，
/// 且 `created_at` 无 default——改键名会让既有用户数据解析失败），因此跨 IPC
/// 一律走本视图，持久化格式保持稳定。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceView {
    pub id: String,
    pub name: String,
    pub profile: String,
    pub node_version: Option<String>,
    pub dsh_version: Option<String>,
    pub last_launched_at: Option<String>,
    pub port: Option<u16>,
    pub created_at: String,
    pub dir: Option<String>,
}

impl From<&Instance> for InstanceView {
    fn from(i: &Instance) -> Self {
        Self {
            id: i.id.clone(),
            name: i.name.clone(),
            profile: i.profile.clone(),
            node_version: i.node_version.clone(),
            dsh_version: i.dsh_version.clone(),
            last_launched_at: i.last_launched_at.clone(),
            port: i.port,
            created_at: i.created_at.clone(),
            dir: i.dir.clone(),
        }
    }
}

/// 实例注册表（持久化于 `<managed root>/instances.json`）。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct InstanceRegistry {
    pub instances: Vec<Instance>,
}

/// 端口分配起点。端口是实例身份的一部分：创建时分配、随名唯一、删号即灭。
///
/// 起点取 3090 而非 DSH 组合层默认的 3080：**3080 是用户自己跑 `dsh web` 时的
/// 惯用端口**（本机实测常驻着一个 GUI 进程）。若实例端口池从 3080 起步，就会
/// ①把用户的 GUI 端口占走，②让「钉定端口被监听」无法区分「本实例进程」与
/// 「用户的 GUI 进程」（旧版本留下的、没有运行记录的实例进程本来也只能靠端口
/// 猜测）。留出 3080~3089 给用户自用，实例从 3090 起分配。
pub const PORT_BASE: u16 = 3090;

/// 端口此刻是否可绑定（bind 探测，避开系统上任何占用者）。
pub fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// 从 `start` 起找第一个空闲且不在 `taken` 集合里的端口；耗尽返回 None。
pub fn allocate_port(start: u16, taken: &[u16]) -> Option<u16> {
    let base = start.max(PORT_BASE);
    (base..=u16::MAX).find(|&p| !taken.contains(&p) && port_free(p))
}

/// 端口决策（纯函数，供测试与 launch 复用）：
///
/// 1. 实例已有固定端口且此刻可用（未被运行实例/其他实例钉定占用、bind 通过）
///    → 原样复用。同一实例永远同一入口地址，重启不漂移；
/// 2. 无固定端口（旧数据）或端口已被外部程序抢占 → 从原值（新实例则从
///    PORT_BASE）向上顺延分配，`reanchored = true` 由调用方写回注册表并在
///    控制台说明缘由。
pub(crate) fn resolve_port(
    pinned: Option<u16>,
    taken: &[u16],
    force_pinned: bool,
) -> Result<(u16, bool), String> {
    match pinned {
        // 重启路径：调用方刚刚停掉占用该端口的旧进程，端口可能还处于 TIME_WAIT
        // （bind 探测会失败），但监听已释放——DSH 侧（libuv 带 SO_REUSEADDR）
        // 仍能绑上。此时强行钉定原端口，避免重启导致入口地址漂移。
        Some(p) if force_pinned && !taken.contains(&p) => Ok((p, false)),
        Some(p) if !taken.contains(&p) && port_free(p) => Ok((p, false)),
        other => {
            let start = other.unwrap_or(PORT_BASE).max(PORT_BASE);
            let fresh =
                allocate_port(start, taken).ok_or_else(|| "无可用的本地端口".to_string())?;
            Ok((fresh, other.is_some()))
        }
    }
}

/// 实例 id 校验：小写字母/数字/连字符组成，1~64 长。
/// 它同时是目录名与 JSON 键，收紧格式避免路径穿越与跨平台文件名问题。
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

/// profile 校验：profile 同时是 `<实例>/profiles/<profile>` 目录名与
/// DSH CLI 的 `--profile` 参数，与实例 id 同级收紧（小写字母/数字/连字符/
/// 下划线）——否则 `../` 型值会让 profile 目录读写逃出实例。
pub(crate) fn valid_profile(profile: &str) -> bool {
    !profile.is_empty()
        && profile.len() <= 32
        && profile
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && !profile.starts_with('-')
}

/// 当前时间 RFC3339 字符串（秒精度）。chrono 不为此引入：格式固定、无时区业务语义。
pub(crate) fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// 天数 → (年, 月, 日)；Howard Hinnant 的 civil_from_days 算法。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 读注册表；文件不存在视为空表（首次运行），解析失败 fail loud。
///
/// 容忍开头 BOM（见 `paths::read_text`）：登记表被编辑器/PowerShell 碰过带进
/// BOM 时不该让整个启动器瘫痪。
pub fn load_registry() -> Result<InstanceRegistry, String> {
    let file = paths::instances_file()?;
    if !file.exists() {
        return Ok(InstanceRegistry::default());
    }
    let raw = paths::read_text(&file)?;
    serde_json::from_str(&raw).map_err(|e| {
        format!(
            "解析 {} 失败：{e}（文件可能被手工改坏；同目录若有 .bak 副本可据以修复，\
             启动器不会自动清空登记表以免实例凭空消失）",
            file.display()
        )
    })
}

/// 原子写注册表：先写临时文件再改名，避免半成品注册表。
pub fn save_registry(registry: &InstanceRegistry) -> Result<(), String> {
    let file = paths::instances_file()?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建 {} 失败：{e}", parent.display()))?;
    }
    let body =
        serde_json::to_string_pretty(registry).map_err(|e| format!("序列化注册表失败：{e}"))?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, &body).map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("替换 {} 失败：{e}", file.display()))?;
    Ok(())
}

/// 实例目录：托管实例 = `<managed root>/versions/<id>`（FILE-LAYOUT L3）；
/// 外部实例（原地引用）= 注册表 `dir` 指向的用户文件夹。
pub fn instance_dir(id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err(format!("非法实例 id：{id:?}"));
    }
    // 外部实例：注册表里的 dir 是唯一事实来源（注册表损坏本就 fail loud）。
    if let Some(instance) = load_registry()?.instances.iter().find(|i| i.id == id) {
        if let Some(dir) = &instance.dir {
            return Ok(PathBuf::from(dir));
        }
    }
    Ok(paths::versions_dir()?.join(id))
}

/// 实例 profile 的插件清单文件：`<实例>/profiles/<profile>/package.json`。
///
/// 它是「这个实例下次启动会加载哪些插件」的唯一权威来源——启动器侧插件启停
/// 与官方 `dsh plugin add/remove` 都只改这里；DSH 启动时按 `dsh.profile.bundles`
/// 组合补丁层，**不做 reconcile**（R-6 侦察结论），所以该文件的修改时间就是
/// 「插件装配自某时刻以来是否变过」的可信信号。
pub fn profile_manifest_path(instance_id: &str) -> Result<PathBuf, String> {
    let instance = get_instance(instance_id)?;
    if !valid_profile(&instance.profile) {
        return Err(format!("非法 profile：{:?}", instance.profile));
    }
    Ok(instance_dir(instance_id)?
        .join("profiles")
        .join(&instance.profile)
        .join("package.json"))
}

/// 插件清单的修改时间（Unix epoch 毫秒）；文件尚不存在时返回 None。
///
/// 返回 None 与「有时间戳」是两种不同事实：前者表示 profile 还没有清单
/// （DSH 未初始化），调用方据「现值 != 记录值」判定装配是否变化，None 参与比较。
pub fn profile_manifest_stamp(instance_id: &str) -> Result<Option<u64>, String> {
    let path = profile_manifest_path(instance_id)?;
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("读取 {} 失败：{e}", path.display())),
    };
    let modified = meta
        .modified()
        .map_err(|e| format!("读取 {} 修改时间失败：{e}", path.display()))?;
    let millis = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("{} 修改时间早于 Unix 纪元：{e}", path.display()))?;
    Ok(Some(millis.as_millis() as u64))
}

/// FR-19 命名规则：重名自动加序号后缀 `-2/-3/…` 直到唯一。
/// 名字未被占用时原样返回。
fn unique_name(base: &str, existing: &[Instance]) -> Option<String> {
    let taken: std::collections::BTreeSet<&str> =
        existing.iter().map(|i| i.name.as_str()).collect();
    if !taken.contains(base) {
        return Some(base.to_string());
    }
    (2u32..).map(|n| format!("{base}-{n}")).find(|c| !taken.contains(c.as_str()))
}

/// 创建实例：登记注册表 + 建目录 + 写实例元数据。
///
/// 命名规则（FR-19）：默认名由调用方给出（惯例 `dsh-<官方版本号>`）；重名时
/// **自动加序号后缀** `-2/-3/…` 直到唯一——同版本可重复安装（各为独立实例），
/// 用户永远不必因撞名而中断。改名路径（update_instance）仍严格拒绝冲突。
pub fn create_instance(name: &str, profile: &str) -> Result<Instance, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("实例名称不能为空".into());
    }
    if !valid_profile(profile) {
        return Err(format!("非法 profile：{profile:?}（限小写字母/数字/连字符/下划线）"));
    }
    // id 用时间戳短码生成（中文名不适合直接做目录名），名称可读性由 name 承担。
    let _guard = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let registry = load_registry()?;
    let final_name =
        unique_name(trimmed, &registry.instances).ok_or_else(|| "无法生成唯一实例名".to_string())?;
    let id = fresh_id(&registry)?;
    // 出生即钉端口（PORT_BASE 起低水位扫描）：排除所有已存在实例的固定端口；
    // bind 探测顺带避开被外部程序占用的地址。极小概率下「创建后被抢占」
    // 由 launch 的自愈重锚定兜底。删除实例即释放，端口号随之回到池中。
    let taken: Vec<u16> = registry.instances.iter().filter_map(|i| i.port).collect();
    let port = allocate_port(PORT_BASE, &taken)
        .ok_or_else(|| format!("无可用的本地端口（{PORT_BASE} 起全被占用）"))?;
    let instance = Instance {
        id: id.clone(),
        name: final_name,
        profile: profile.to_string(),
        node_version: None,
        dsh_version: None,
        last_launched_at: None,
        port: Some(port),
        created_at: now_rfc3339(),
        dir: None,
    };

    let dir = instance_dir(&id)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建实例目录失败：{e}"))?;
    let meta_path = dir.join("instance.json");
    let meta = serde_json::to_string_pretty(&instance)
        .map_err(|e| format!("序列化实例元数据失败：{e}"))?;
    std::fs::write(&meta_path, meta + "\n").map_err(|e| format!("写入实例元数据失败：{e}"))?;

    let mut registry = registry;
    registry.instances.push(instance.clone());
    save_registry(&registry)?;
    Ok(instance)
}

/// skills 全局共享挂载（2026-09-12 需求变更）：`versions/<id>/skills` 指向
/// `<托管根>/shared/skills` 的目录联接——所有实例共享一份技能；DSH 照常读
/// `$DSH_HOME/skills`（联接对它透明），`skill_install` 落实例路径经联接自动
/// 写进共享区。首次建立；存量真实目录先并入共享区（同名保留共享侧）再替换；
/// 已是联接则跳过。**仅托管实例调用**——外部实例不共享（2026-09-20 需求方
/// 裁决方案 A：不动用户文件夹的技能数据、不建联接）。
pub fn ensure_skills_link(id: &str) -> Result<(), String> {
    get_instance(id)?;
    let shared = crate::paths::shared_skills_dir()?;
    std::fs::create_dir_all(&shared).map_err(|e| format!("创建共享技能目录失败：{e}"))?;
    let link = instance_dir(id)?.join("skills");
    if link.is_symlink() {
        return Ok(());
    }
    if link.is_dir() {
        for entry in std::fs::read_dir(&link)
            .map_err(|e| format!("读取 {} 失败：{e}", link.display()))?
            .flatten()
        {
            let target = shared.join(entry.file_name());
            if !target.exists() {
                std::fs::rename(entry.path(), &target).map_err(|e| {
                    format!("并入共享技能目录失败（{}）：{e}", entry.path().display())
                })?;
            }
        }
        std::fs::remove_dir_all(&link)
            .map_err(|e| format!("移除实例本地技能目录失败：{e}"))?;
    }
    #[cfg(windows)]
    junction::create(&shared, &link).map_err(|e| format!("创建技能目录联接失败：{e}"))?;
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&shared, &link)
        .map_err(|e| format!("创建技能目录符号链接失败：{e}"))?;
    Ok(())
}

/// skills 的「打开」落点：托管实例 = 建联接后开全局共享目录；外部实例 =
/// 其自身 skills 目录（不共享、不动用户数据——2026-09-20 需求方裁决方案 A）。
fn skills_display_dir(instance: &Instance) -> Result<PathBuf, String> {
    if instance.dir.is_some() {
        return Ok(instance_dir(&instance.id)?.join("skills"));
    }
    ensure_skills_link(&instance.id)?;
    crate::paths::shared_skills_dir()
}

/// 打开实例子目录（2026-09-12 追加批次）：skills / sessions / sources。
/// skills 打开**全局共享目录**（托管实例；所有实例同一份，需求变更）；
/// sessions 缺失时先创建（空目录对 DSH 无副作用）；sources 不预建——缺失时
/// 报可读错误。
pub fn open_subdir(app: &tauri::AppHandle, id: &str, which: &str) -> Result<PathBuf, String> {
    let instance = get_instance(id)?;
    let dir = match which {
        "skills" => skills_display_dir(&instance)?,
        "sessions" | "sources" => instance_dir(id)?.join(which),
        other => return Err(format!("未知子目录：{other:?}")),
    };
    if which == "sources" && !dir.is_dir() {
        return Err("尚未下载源码——先在「源码归档」卡用「下载源码」补下载。".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录 {} 失败：{e}", dir.display()))?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| format!("打开目录失败：{e}"))?;
    Ok(dir)
}

/// 「添加已有文件夹」（2026-09-12 需求方定稿：**原地引用**）：把一个既有的
/// DSH 实例目录注册进启动器。文件夹留在原地，注册表以 `dir` 指向它；删除
/// 该实例 = 仅解除关联，绝不删用户文件夹。
///
/// 校验（满足其一，否则拒绝并说明缺什么）：①含 `instance.json`（启动器实例
/// 格式）；②含 `app/node_modules/@deepseek-ai/dsh/package.json`（裸 DSH
/// 安装，读其版本）。同一文件夹不允许重复注册。
pub fn register_external(folder: &str) -> Result<Instance, String> {
    let path = PathBuf::from(folder.trim());
    if !path.is_dir() {
        return Err(format!("文件夹不存在：{}", path.display()));
    }
    let meta_path = path.join("instance.json");
    let dsh_pkg = path
        .join("app")
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("package.json");
    if !meta_path.is_file() && !dsh_pkg.is_file() {
        return Err(format!(
            "无法注册：{} 既没有 instance.json（不是启动器实例目录），\
             也没有 app\\node_modules\\@deepseek-ai\\dsh（未安装 DSH 本体）",
            path.display()
        ));
    }
    let dsh_version = std::fs::read_to_string(&dsh_pkg)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("version").and_then(|x| x.as_str()).map(String::from));

    let _guard = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut registry = load_registry()?;
    // 同一文件夹不允许重复注册（id 不同但 dir 相同 = 双条目同一份数据）。
    let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
    let already = registry.instances.iter().any(|i| {
        i.dir
            .as_deref()
            .map(|d| std::path::Path::new(d).canonicalize().map(|p| p == canon).unwrap_or(false))
            .unwrap_or(false)
    });
    if already {
        return Err("该文件夹已被注册为实例，不能重复添加。".into());
    }

    let id = fresh_id(&registry)?;
    let display = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| id.clone());
    let name = unique_name(&display, &registry.instances)
        .ok_or_else(|| "无法生成唯一实例名".to_string())?;
    let taken: Vec<u16> = registry.instances.iter().filter_map(|i| i.port).collect();
    let port = allocate_port(PORT_BASE, &taken)
        .ok_or_else(|| format!("无可用的本地端口（{PORT_BASE} 起全被占用）"))?;
    let instance = Instance {
        id: id.clone(),
        name,
        profile: "web".to_string(),
        node_version: None,
        dsh_version,
        last_launched_at: None,
        port: Some(port),
        created_at: now_rfc3339(),
        dir: Some(path.display().to_string()),
    };

    // 外部目录写一份启动器格式的 instance.json（便于携带与二次校验识别）；
    // 只读目录等写入失败时如实报错。顺序注：先写 meta 再存注册表——注册表
    // 保存失败时外部 meta 会残留，但无害（重注册可通过其校验；注册表才是
    // 唯一事实源），不假称原子。
    let meta = serde_json::to_string_pretty(&instance)
        .map_err(|e| format!("序列化实例元数据失败：{e}"))?;
    std::fs::write(meta_path, meta + "\n")
        .map_err(|e| format!("向该文件夹写入 instance.json 失败（目录只读？）：{e}"))?;

    registry.instances.push(instance.clone());
    save_registry(&registry)?;
    Ok(instance)
}

/// 生成未占用的短 id：`i-<unix毫秒base36>`，碰撞则递增尾缀。
/// P3-04：除注册表外同时查**目录存在性**——删除中的实例目录仍在隔离区
/// 之外短暂存活（改名前）时，同毫秒生成的候选 id 不得与其撞名，否则新
/// 实例目录会被旧目录的分离清除进程一并抹掉。
fn fresh_id(registry: &InstanceRegistry) -> Result<String, String> {
    let instances_root = paths::versions_dir()?;
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .map_err(|e| e.to_string())?;
    let mut n = ms;
    loop {
        let id = format!("i-{}", to_base36(n));
        if !registry.instances.iter().any(|i| i.id == id) && !instances_root.join(&id).exists() {
            return Ok(id);
        }
        n += 1;
    }
}

fn to_base36(mut v: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if v == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while v > 0 {
        out.push(DIGITS[(v % 36) as usize]);
        v /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// 删除实例：注册表先行（前端即时可见），目录物理清除交给分离的系统进程。
///
/// 大体积实例（数百 MB、数万文件）同步删除要数秒，前端会像卡死；且若用户
/// 点完删除立刻退出启动器，删除进程随之终止会留下半删状态。改为：注册表
/// 原子摘除后，spawn 一个分离的 `rd /s /q`（非 Windows 为 `rm -rf`）——
/// std Child drop 不杀子进程，启动器退出它也会继续跑完，保证最终删干净。
///
/// P3-04 三重加固：
/// 1. **隔离改名**——先把目录原子改名到 `.purging-` 隔离区再发起清除：
///    新实例的 `create_dir_all` 永不与清除目标撞名（TOCTOU 根除）；
/// 2. **占用即报错**——运行中/被占用的目录改名失败，转为可读报错，
///    不再静默半删（旧路径：注册表已摘、rd 失败被丢弃、留幽灵目录）；
/// 3. **失败可重试**——清扫上次遗留的隔离目录（启动后首次枚举 + 每次删除），
///    清除失败不再无人知晓，下次启动自动补刀。
pub fn delete_instance(id: &str) -> Result<(), String> {
    let _guard = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut registry = load_registry()?;
    // 外部实例（原地引用）：仅解除关联，绝不动用户文件夹。
    let external_dir = registry
        .instances
        .iter()
        .find(|i| i.id == id)
        .and_then(|i| i.dir.clone());
    registry.instances.retain(|i| i.id != id);
    save_registry(&registry)?;
    if external_dir.is_some() {
        purge_stale_quarantines()?;
        return Ok(());
    }
    let dir = instance_dir(id)?;
    // skills 全局共享：先把联接本身摘除（只删链接不删目标），物理清除进程
    // 即便跟进联接也无可删；共享目录内容绝不随实例删除。
    let skills_link = dir.join("skills");
    if skills_link.is_symlink() {
        std::fs::remove_dir(&skills_link)
            .map_err(|e| format!("移除技能目录联接失败：{e}"))?;
    }
    if dir.exists() {
        let quarantine = quarantine_dir(id)?;
        std::fs::rename(&dir, &quarantine).map_err(|e| {
            format!(
                "实例目录暂时无法删除（可能正在运行或被其他程序占用）：{e} —— 请先停止该实例后重试"
            )
        })?;
        spawn_detached_purge(&quarantine);
    }
    purge_stale_quarantines()?;
    Ok(())
}

/// 隔离区目录名：`<instances>/.purging-<id>-<unix毫秒>`。`.` 前缀保证
/// 永不与合法实例 id 冲突（valid_id 拒绝点号开头）。
fn quarantine_dir(id: &str) -> Result<PathBuf, String> {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    Ok(paths::versions_dir()?.join(format!(".purging-{id}-{ms}")))
}

/// 清扫遗留的隔离目录：对每个 `.purging-*` 重新发起分离清除。成功后目录
/// 消失；失败则留在原地，下次删除/下次启动再试——半删状态永远可见、可重试。
pub fn purge_stale_quarantines() -> Result<(), String> {
    let root = paths::versions_dir()?;
    if !root.exists() {
        return Ok(());
    }
    let entries =
        std::fs::read_dir(&root).map_err(|e| format!("读取 {} 失败：{e}", root.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".purging-") && entry.path().is_dir() {
            spawn_detached_purge(&entry.path());
        }
    }
    Ok(())
}

/// 启动后首次枚举时清扫一次（跨会话遗留的隔离目录自动补刀）。
static QUARANTINE_SWEPT: AtomicBool = AtomicBool::new(false);

/// 枚举实例（按创建顺序，即注册表顺序）。
pub fn list_instances() -> Result<Vec<Instance>, String> {
    if !QUARANTINE_SWEPT.swap(true, Ordering::Relaxed) {
        // 失败不阻塞枚举：隔离目录只是待清除的残留，下次再试。
        let _ = purge_stale_quarantines();
    }
    Ok(load_registry()?.instances)
}

/// 分离的物理清除：Windows `rd /s /q`（CREATE_NO_WINDOW），其余平台 `rm -rf`。
fn spawn_detached_purge(dir: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        let _ = std::process::Command::new("cmd")
            .args(["/c", "rd", "/s", "/q"])
            .arg(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn();
    }
    #[cfg(not(windows))]
    {
        use std::process::Stdio;
        let _ = std::process::Command::new("rm")
            .args(["-rf"])
            .arg(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

/// 读单个实例；不存在返回错误（fail loud，不静默造默认）。
pub fn get_instance(id: &str) -> Result<Instance, String> {
    load_registry()?
        .instances
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| format!("实例不存在：{id}"))
}

/// 更新实例可变字段，同步重写 instance.json 与注册表。
pub fn update_instance(id: &str, mutate: impl FnOnce(&mut Instance)) -> Result<Instance, String> {
    let _guard = REGISTRY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut registry = load_registry()?;
    let instance = registry
        .instances
        .iter_mut()
        .find(|i| i.id == id)
        .ok_or_else(|| format!("实例不存在：{id}"))?;
    mutate(instance);
    let updated = instance.clone();
    // 唯一性守卫（create 与 update 双向把关）：名称全局唯一；端口一旦钉定也全局
    // 唯一——「一个实例一个固定入口地址」是不变量，冲突直接拒绝且不落盘。
    if registry.instances.iter().any(|i| i.id != id && i.name == updated.name) {
        return Err(format!("实例名称「{}」已存在——实例名不可重复", updated.name));
    }
    if let Some(port) = updated.port {
        if registry.instances.iter().any(|i| i.id != id && i.port == Some(port)) {
            return Err(format!("端口 {port} 已被其他实例固定占用"));
        }
    }

    let meta_path = instance_dir(id)?.join("instance.json");
    let meta = serde_json::to_string_pretty(&updated)
        .map_err(|e| format!("序列化实例元数据失败：{e}"))?;
    std::fs::write(meta_path, meta + "\n").map_err(|e| format!("写入实例元数据失败：{e}"))?;

    save_registry(&registry)?;
    Ok(updated)
}

/// 校验实例目录完整性：目录在且含 instance.json（供删除 A 后验证 B 的抽查用）。
/// 双实例抽查在 Phase 1 验收步骤接入；当前仅测试与未来 command 使用。
#[allow(dead_code)]
pub fn instance_dir_intact(dir: &Path) -> bool {
    dir.is_dir() && dir.join("instance.json").is_file()
}

/// 供其他模块的测试复用互斥沙箱（跨模块共享 LOCALAPPDATA 环境变量）。
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 每个用例独立沙箱目录并跨模块串行：所有测试共享进程级 LOCALAPPDATA，
    /// 并行运行会互相改写对方解析出的根目录。锁必须覆盖整个用例生命周期。
    pub fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = std::env::temp_dir()
            .join(format!("dsh-launcher-inst-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("versions")).unwrap();
        std::env::set_var("LOCALAPPDATA", &tmp);
        (guard, tmp)
    }

    #[test]
    fn id_validation_rejects_traversal_and_case() {
        assert!(valid_id("abc-123"));
        assert!(!valid_id(""));
        assert!(!valid_id("-lead"));
        assert!(!valid_id("trail-"));
        assert!(!valid_id("Has Upper"));
        assert!(!valid_id("../etc"));
        assert!(!valid_id("a\\b"));
    }

    /// IPC 视图键名必须是 camelCase（前端按 lastLaunchedAt/createdAt/dshVersion
    /// 读取；曾因漏 rename 导致启动页 init 全灭）。持久化格式不受影响。
    #[test]
    fn instance_view_serializes_camel_case_keys() {
        let instance = Instance {
            id: "i-x".into(),
            name: "dsh-0.1.1".into(),
            profile: "web".into(),
            node_version: None,
            dsh_version: Some("0.1.1".into()),
            last_launched_at: None,
            port: Some(3080),
            created_at: "2026-08-28T00:00:00Z".into(),
            dir: None,
        };
        let json = serde_json::to_value(InstanceView::from(&instance)).unwrap();
        assert_eq!(json["dshVersion"], "0.1.1");
        assert_eq!(json["createdAt"], "2026-08-28T00:00:00Z");
        assert_eq!(json["lastLaunchedAt"], serde_json::Value::Null);
        assert_eq!(json["nodeVersion"], serde_json::Value::Null);
        assert_eq!(json["port"], 3080);
        let raw = serde_json::to_string(&InstanceView::from(&instance)).unwrap();
        assert!(!raw.contains("dsh_version"), "禁止泄漏 snake_case 键：{raw}");
    }

    /// 回归：登记表被写入 UTF-8 BOM 时仍必须读得出来。
    ///
    /// 2026-09-10 真机事故：手工改端口时用 PowerShell 5.1 的
    /// `Set-Content -Encoding UTF8` 写进 BOM，`serde_json` 报
    /// `expected value at line 1 column 1`，于是启动页「页面数据加载失败」、
    /// 建实例/装版本/装插件全挂。BOM 是外部工具的常态产物，不该让登记表瘫痪；
    /// 真正的损坏仍 fail loud（下一条用例钉住）。
    #[test]
    fn registry_with_bom_still_loads() {
        let (_lock, tmp) = sandbox("registry-bom");
        create_instance("甲", "web").unwrap();
        let file = paths::instances_file().unwrap();
        let body = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, format!("\u{feff}{body}")).unwrap();

        let registry = load_registry().unwrap();
        assert_eq!(registry.instances.len(), 1, "带 BOM 的登记表必须照常读出");

        // 写回一次即恢复无 BOM 的干净文件（我们自己的写入从不带 BOM）。
        save_registry(&registry).unwrap();
        let after = std::fs::read_to_string(&file).unwrap();
        assert!(!after.starts_with('\u{feff}'), "save_registry 不得写 BOM");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 真正损坏的登记表必须 fail loud，且文案点明可修复方向（不静默清空）。
    #[test]
    fn corrupt_registry_fails_loud_with_guidance() {
        let (_lock, tmp) = sandbox("registry-corrupt");
        create_instance("甲", "web").unwrap();
        let file = paths::instances_file().unwrap();
        std::fs::write(&file, "{ 这不是 JSON").unwrap();

        let err = load_registry().unwrap_err();
        assert!(err.contains("解析"), "错误必须点明是解析失败：{err}");
        assert!(err.contains("bak"), "错误应提示可用备份修复：{err}");
        // 关键：坏文件必须原地保留，绝不自动清空（实例不能凭空消失）。
        assert!(file.exists(), "损坏的登记表不得被删除");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn create_list_delete_roundtrip() {
        let (_lock, tmp) = sandbox("crud");
        let a = create_instance("主力实例", "headless").unwrap();
        let b = create_instance("测试实例", "web").unwrap();
        assert_ne!(a.id, b.id);

        let listed = list_instances().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "主力实例");

        let dir_b = instance_dir(&b.id).unwrap();
        assert!(instance_dir_intact(&dir_b));

        delete_instance(&a.id).unwrap();
        let listed = list_instances().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, b.id);
        assert!(instance_dir_intact(&instance_dir(&b.id).unwrap()));
        // 物理目录由分离进程异步清除：轮询等待（本地空目录通常毫秒级完成）。
        let a_dir = instance_dir(&a.id).unwrap();
        let mut purged = !a_dir.exists();
        for _ in 0..100 {
            if !a_dir.exists() {
                purged = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(purged, "分离清除应在 5s 内删掉实例目录");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn update_persists_to_both_stores() {
        let (_lock, tmp) = sandbox("update");
        let a = create_instance("旧名", "headless").unwrap();

        let updated = update_instance(&a.id, |i| {
            i.name = "新名".into();
            i.node_version = Some("24.12.0".into());
        })
        .unwrap();
        assert_eq!(updated.name, "新名");
        assert_eq!(updated.node_version.as_deref(), Some("24.12.0"));

        let reread = get_instance(&a.id).unwrap();
        assert_eq!(reread.name, "新名");
        let meta =
            std::fs::read_to_string(instance_dir(&a.id).unwrap().join("instance.json")).unwrap();
        assert!(meta.contains("新名"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn delete_missing_instance_is_error_free_and_idempotent() {
        let (_lock, tmp) = sandbox("idem");
        delete_instance("i-nothing").unwrap(); // 目录与注册表都没有 → 幂等成功
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 「添加已有文件夹」全环（2026-09-12 追加批次）：裸 DSH 目录原地注册 →
    /// instance_dir 解析到外部路径 → 重复注册拒绝 → 删除仅解除关联、文件夹保留。
    #[test]
    fn register_external_references_in_place_and_unlink_keeps_folder() {
        let (_lock, tmp) = sandbox("external");
        // 伪造一个裸 DSH 安装目录（无 instance.json，有 app 下本体 manifest）。
        let ext = tmp.join("my-dsh-folder");
        let pkg = ext
            .join("app")
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            r#"{ "name": "@deepseek-ai/dsh", "version": "0.1.1-rc.2" }"#,
        )
        .unwrap();

        let instance = register_external(ext.to_str().unwrap()).unwrap();
        assert_eq!(instance.dsh_version.as_deref(), Some("0.1.1-rc.2"));
        assert_eq!(instance.dir.as_deref(), Some(ext.to_str().unwrap()));
        assert!(ext.join("instance.json").is_file(), "外部目录应写入实例元数据");
        // instance_dir 解析到外部路径（profiles/sessions 等都落那里）。
        assert_eq!(instance_dir(&instance.id).unwrap(), ext);
        // 重名自动去重：再注册同名文件夹（换内容目录）不冲突。
        // 同一文件夹重复注册拒绝。
        assert!(register_external(ext.to_str().unwrap()).is_err());

        // 删除 = 仅解除关联；文件夹与数据原样保留。
        delete_instance(&instance.id).unwrap();
        assert!(
            !load_registry()
                .unwrap()
                .instances
                .iter()
                .any(|i| i.id == instance.id),
            "注册表应已摘除"
        );
        assert!(ext.is_dir(), "外部文件夹必须原样保留");
        assert!(pkg.join("package.json").is_file());

        // 缺 instance.json 又缺 DSH 本体的目录拒绝并说明。
        let empty = tmp.join("empty-folder");
        std::fs::create_dir_all(&empty).unwrap();
        let err = register_external(empty.to_str().unwrap()).unwrap_err();
        assert!(err.contains("instance.json") && err.contains("本体"), "实际：{err}");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 外部实例不共享 skills（2026-09-20 需求方裁决方案 A，审查 P2-05）：
    /// 「打开 skills」解析到其自身目录、不建联接、不把用户技能迁入共享区。
    /// 注册命令层（lib.rs）同样只对托管实例调 ensure_skills_link。
    #[test]
    fn external_instance_skills_stay_local() {
        let (_lock, tmp) = sandbox("external-skills");
        let ext = tmp.join("my-dsh-folder");
        let pkg = ext
            .join("app")
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            r#"{ "name": "@deepseek-ai/dsh", "version": "0.1.1" }"#,
        )
        .unwrap();
        // 用户文件夹里已有的技能数据。
        std::fs::create_dir_all(ext.join("skills").join("user-skill")).unwrap();
        std::fs::write(ext.join("skills").join("user-skill").join("SKILL.md"), b"x").unwrap();

        let instance = register_external(ext.to_str().unwrap()).unwrap();
        let shared = crate::paths::shared_skills_dir().unwrap();
        assert!(
            !shared.join("user-skill").exists(),
            "用户技能不得被迁入共享区"
        );
        // 打开落点 = 其自身 skills 目录。
        let dir = skills_display_dir(&instance).unwrap();
        assert_eq!(dir, ext.join("skills"));
        assert!(dir.join("user-skill").join("SKILL.md").is_file(), "用户数据原样保留");
        assert!(!dir.is_symlink(), "外部实例 skills 不得被替换为联接");

        // 对照：托管实例仍走共享联接（既有全环测试覆盖，此处只锁外部分支）。
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// skills 全局共享全环（2026-09-12 需求变更）：联接建立 → 经链接写文件落
    /// 共享区 → 存量真实目录并入 → 删除实例摘链接、共享内容保留。
    #[test]
    fn skills_shared_via_link_and_survive_instance_delete() {
        let (_lock, tmp) = sandbox("skills-shared");
        let instance = create_instance("技能共享", "web").unwrap();
        ensure_skills_link(&instance.id).unwrap();
        let link = instance_dir(&instance.id).unwrap().join("skills");
        assert!(link.is_symlink(), "实例 skills 应为目录联接");

        // 经链接写入 = 落共享区（skill_install 的落点语义）。
        let shared = crate::paths::shared_skills_dir().unwrap();
        std::fs::write(link.join("demo-skill"), b"x").unwrap();
        assert!(shared.join("demo-skill").is_file(), "经链接写入应落共享目录");

        // 存量真实目录并入共享区后替换为链接。
        std::fs::remove_dir(&link).unwrap();
        std::fs::create_dir_all(link.join("local-skill")).unwrap();
        ensure_skills_link(&instance.id).unwrap();
        assert!(link.is_symlink());
        assert!(shared.join("local-skill").is_dir(), "存量本地技能应并入共享区");
        assert!(shared.join("demo-skill").is_file());

        // 删除实例：联接摘除，共享内容原样保留。
        delete_instance(&instance.id).unwrap();
        assert!(!link.exists(), "删除实例应摘除联接");
        assert!(shared.join("demo-skill").is_file(), "共享技能绝不随实例删除");

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// P3-04：删除先隔离改名再分离清除——改名瞬间新实例目录永不与清除目标
    /// 撞名；`.purging-` 隔离目录最终被清干净。
    #[test]
    fn delete_quarantines_dir_before_detached_purge() {
        let (_lock, tmp) = sandbox("del-quarantine");
        let a = create_instance("甲", "web").unwrap();
        let a_dir = instance_dir(&a.id).unwrap();
        assert!(a_dir.exists());
        delete_instance(&a.id).unwrap();

        let listed = list_instances().unwrap();
        assert!(listed.iter().all(|i| i.id != a.id), "注册表行应已摘除");
        assert!(!a_dir.exists(), "原目录应已改名进隔离区，不再占位");

        // 立即能看到 .purging- 隔离目录（清除是分离异步的）。
        let root = paths::versions_dir().unwrap();
        let quarantined: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".purging-"))
            .collect();
        assert!(!quarantined.is_empty(), "删除后应存在隔离目录");

        // 分离清除最终完成（本地小目录毫秒级；轮询 5s）。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while quarantined.iter().any(|e| e.path().exists()) {
            assert!(std::time::Instant::now() < deadline, "隔离目录 5s 内未被清除");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// P3-04：上次运行遗留的隔离目录由清扫补刀。
    #[test]
    fn stale_quarantine_is_swept() {
        let (_lock, tmp) = sandbox("stale-quarantine");
        let stale = paths::versions_dir().unwrap().join(format!(".purging-stale-{}", std::process::id()));
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("canary"), b"x").unwrap();
        purge_stale_quarantines().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while stale.exists() {
            assert!(std::time::Instant::now() < deadline, "遗留隔离目录 5s 内未被清扫");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// P3-04：fresh_id 必须跳过已存在目录（注册表没有但目录在——清除中/遗留），
    /// 否则新实例目录会被旧目录的分离清除误删。
    #[test]
    fn fresh_id_skips_preexisting_directories() {
        let (_lock, tmp) = sandbox("fresh-id-dirs");
        let now_ms: u64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let root = paths::versions_dir().unwrap();
        let mut planted = std::collections::BTreeSet::new();
        for n in now_ms..now_ms + 1000 {
            let id = format!("i-{}", to_base36(n));
            std::fs::create_dir_all(root.join(&id)).unwrap();
            planted.insert(id);
        }
        let made = create_instance("新实例", "web").unwrap();
        assert!(
            !planted.contains(&made.id),
            "新实例 id {} 不得与预置目录撞名（否则会被旧清除进程误删）",
            made.id
        );
        assert!(instance_dir(&made.id).unwrap().join("instance.json").is_file());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn empty_name_is_rejected() {
        let (_lock, tmp) = sandbox("empty-name");
        assert!(create_instance("   ", "headless").is_err());
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// FR-19：同版本可重复安装，重名自动加序号后缀；改名仍严格拒绝冲突。
    #[test]
    fn duplicate_names_autosuffix_on_create_reject_on_rename() {
        let (_lock, tmp) = sandbox("dup-name");
        let a = create_instance("主力实例", "web").unwrap();
        let b = create_instance("主力实例", "web").unwrap();
        let c = create_instance("主力实例", "web").unwrap();
        assert_eq!(a.name, "主力实例");
        assert_eq!(b.name, "主力实例-2", "撞名应自动加 -2 后缀");
        assert_eq!(c.name, "主力实例-3", "第三次创建顺延为 -3");
        assert_ne!(a.port, b.port);
        assert_ne!(b.port, c.port, "每个实例钉定端口互异");

        // 手动改名撞已有实例仍然拒绝（FR-10 语义不受自动后缀影响）。
        let rename_err = update_instance(&c.id, |i| i.name = "主力实例".to_string()).unwrap_err();
        assert!(rename_err.contains("不可重复"), "改名冲突必须拦截：{rename_err}");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 端口出生即钉定：多实例端口互异且 >= PORT_BASE；删除释放后新创建可回收
    /// 池中最低位。全部相对断言（本会话环境可能占着低位真实端口）。
    #[test]
    fn ports_pinned_at_creation_unique_and_recycled_after_delete() {
        let (_lock, tmp) = sandbox("port-pin");
        let a = create_instance("甲", "web").unwrap();
        let b = create_instance("乙", "web").unwrap();
        assert_ne!(a.port, b.port);
        assert_eq!(
            get_instance(&a.id).unwrap().port,
            a.port,
            "注册表里的钉定值必须与创建结果一致"
        );
        delete_instance(&a.id).unwrap();
        let c = create_instance("丙", "web").unwrap();
        assert_ne!(c.port, b.port);
        // 甲删乙在，丙只能拿「未被钉定」的端口；具体数值依环境而异。
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resolve_port_semantics_for_pinned_and_stolen_addresses() {
        // 拿一个确定空闲的端口号（绑 0 号口取系统分配，释放后即 free）。
        let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let free_port = probe.local_addr().unwrap().port();
        drop(probe);
        assert!(port_free(free_port));

        // 钉定端口空闲 → 原样复用，不标记重锚定。
        assert_eq!(resolve_port(Some(free_port), &[], false).unwrap(), (free_port, false));

        // 钉定端口被外部占用 → 自愈顺延，标记 reanchored。
        let stolen_holder = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let stolen = stolen_holder.local_addr().unwrap().port();
        let (got, reanchored) = resolve_port(Some(stolen), &[], false).unwrap();
        assert!(reanchored && got != stolen);

        // 重启路径（force_pinned）：端口仍被占用也钉定原值——调用方刚停掉旧进程，
        // 残留的 TIME_WAIT 不该让入口地址漂移（DSH 侧 SO_REUSEADDR 能绑上）。
        assert_eq!(resolve_port(Some(stolen), &[], true).unwrap(), (stolen, false));
        // 但别的实例已钉定的端口绝不偷。
        let (other, _) = resolve_port(Some(stolen), &[stolen], true).unwrap();
        assert_ne!(other, stolen);
        drop(stolen_holder);

        // 新分配钳制下限；taken 列表被跳过。
        let (fresh, re2) = resolve_port(None, &[], false).unwrap();
        assert!(!re2 && fresh >= PORT_BASE);
        let next = resolve_port(None, &[fresh], false).unwrap().0;
        assert_ne!(next, fresh);
    }

    #[test]
    fn profile_validation_blocks_traversal_and_create_rejects() {
        assert!(valid_profile("web"));
        assert!(valid_profile("headless"));
        assert!(valid_profile("my_profile-2"));
        for bad in ["", "../../evil", "a/b", "a\\b", "..", "Web", "a b", "a:b", "-lead", "工具"] {
            assert!(!valid_profile(bad), "{bad:?} 应被拒绝");
        }
        let (_lock, tmp) = sandbox("profile-create");
        assert!(create_instance("t", "../../evil").is_err(), "创建入口必须拒绝穿越 profile");
        assert!(create_instance("t", "web").is_ok());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
