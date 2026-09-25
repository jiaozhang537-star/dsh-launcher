//! 托管根目录解析：启动器全部本地数据（Node 代管、版本实例）的顶层位置。
//!
//! 一期 Windows 落在 `%LOCALAPPDATA%/dsh-launcher`；路径规则集中在此模块，
//! 其余代码不得自行拼接根目录，保证未来换根/移植只改这里。
//! 结构定稿见 docs/FILE-LAYOUT.md（三级路由：L1 全局层 → L2 versions → L3 版本内部）。

use std::path::{Path, PathBuf};

/// 托管根目录名（位于应用数据目录下）。
const ROOT_DIR_NAME: &str = "dsh-launcher";

/// Node 便携版存放子目录。
pub const NODES_DIR: &str = "nodes";

/// 版本实例存放子目录（2026-09-12 结构定稿：旧名 `instances` 启动时自动迁移）。
pub const VERSIONS_DIR: &str = "versions";

/// DSH 源码归档存放子目录（FR-22：无版本归属的归档按下载版本名落此；版本内的落版本目录）。
pub const SOURCES_DIR: &str = "sources";

/// 隔离开关共享区（FR-17，Phase 5 落地；本期仅占名，见 FILE-LAYOUT §2）。
pub const SHARED_DIR: &str = "shared";

/// 全局 AGENTS.md 文件名（位于托管根下；版本内的落版本目录，DSH 原生读取位）。
pub const AGENTS_MD_FILE: &str = "AGENTS.md";

/// 实例注册表文件名（位于托管根下；文件名沿用旧称，指向 VERSIONS_DIR）。
pub const INSTANCES_FILE: &str = "instances.json";

/// 市场缓存文件名（位于托管根下）：GitHub ETag 条件缓存 + 聚合快照。
pub const MARKET_CACHE_FILE: &str = "market-cache.json";

/// 运行记录文件名（位于托管根下）：本启动器拉起的实例进程（跨会话识别孤儿）。
pub const RUNNING_FILE: &str = "running.json";

/// 解析应用本地数据目录（Windows = `%LOCALAPPDATA%`，可移植到其他平台时替换实现）。
///
/// 目录不存在时不创建——创建是显式的 `ensure_dirs` 决策，避免只读探测产生副作用。
fn app_data_dir() -> Result<PathBuf, String> {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .map_err(|_| "环境变量 LOCALAPPDATA 未设置，无法定位应用数据目录".to_string())
}

/// 解析托管根目录：`<app data>/dsh-launcher`。
pub fn managed_root() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join(ROOT_DIR_NAME))
}

/// 解析托管 Node 根目录：`<managed root>/nodes`。
pub fn nodes_dir() -> Result<PathBuf, String> {
    Ok(managed_root()?.join(NODES_DIR))
}

/// 解析版本实例根目录：`<managed root>/versions`。
pub fn versions_dir() -> Result<PathBuf, String> {
    Ok(managed_root()?.join(VERSIONS_DIR))
}

/// 解析源码归档根目录：`<managed root>/sources`（FR-22）。
pub fn sources_dir() -> Result<PathBuf, String> {
    Ok(managed_root()?.join(SOURCES_DIR))
}

/// 解析隔离开关共享区：`<managed root>/shared`（Phase 5 落地，本期仅占名）。
pub fn shared_dir() -> Result<PathBuf, String> {
    Ok(managed_root()?.join(SHARED_DIR))
}

/// 全局共享技能目录：`<managed root>/shared/skills`（2026-09-12 需求变更：
/// skills 不随实例隔离，所有版本共享一份；实例内为指向此处的目录联接）。
pub fn shared_skills_dir() -> Result<PathBuf, String> {
    Ok(shared_dir()?.join("skills"))
}

/// 解析全局 AGENTS.md：`<managed root>/AGENTS.md`（设置页输入框落点）。
pub fn global_agents_md_file() -> Result<PathBuf, String> {
    Ok(managed_root()?.join(AGENTS_MD_FILE))
}

/// 解析实例注册表文件：`<managed root>/instances.json`。
pub fn instances_file() -> Result<PathBuf, String> {
    Ok(managed_root()?.join(INSTANCES_FILE))
}

/// 解析市场缓存文件：`<managed root>/market-cache.json`。
pub fn market_cache_file() -> Result<PathBuf, String> {
    Ok(managed_root()?.join(MARKET_CACHE_FILE))
}

/// 解析运行记录文件：`<managed root>/running.json`。
pub fn running_file() -> Result<PathBuf, String> {
    Ok(managed_root()?.join(RUNNING_FILE))
}

/// 一次性迁移（FILE-LAYOUT §4）：旧 `instances/` 整体改名 `versions/`。
///
/// `versions/` 已存在（迁移完成过）或旧目录不存在时是无害 no-op；目录被占用
/// （运行中实例持有句柄）时 Windows rename 会失败——**fail-loud** 给可读指引，
/// 不静默吞掉、不降级双目录并存。`.purging-*` 临时目录随整体改名一并带走。
fn migrate_legacy_instances_dir(root: &Path) -> Result<(), String> {
    let legacy = root.join("instances");
    let target = root.join(VERSIONS_DIR);
    if !legacy.is_dir() || target.is_dir() {
        return Ok(());
    }
    std::fs::rename(&legacy, &target).map_err(|e| {
        format!(
            "迁移旧实例目录失败（instances → versions）：{e}。\
             若有实例正在运行，请关闭后重启启动器完成迁移"
        )
    })?;
    sweep_stale_pnpm_stores(&target);
    Ok(())
}

/// 迁移一次性成本：pnpm 的 node_modules 元数据（`.modules.yaml` 的
/// virtualStoreDir）记录了旧 `instances/` 的**绝对路径**，整体改名后必然失配
/// （e2e 实证：安装报 "pnpm now wants to use the virtual store at …"）。清掉
/// node_modules 与 pnpm-lock.yaml（package.json / bundles 清单保留），插件从
/// 市场或整合包重装即可。MVP 未发布，仅开发机数据受影响；留说明文件供用户知悉。
fn sweep_stale_pnpm_stores(versions_root: &Path) {
    let entries = match std::fs::read_dir(versions_root) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut touched = 0usize;
    for entry in entries.flatten() {
        let profiles = entry.path().join("profiles");
        let Ok(list) = std::fs::read_dir(&profiles) else {
            continue;
        };
        for profile in list.flatten() {
            let profile_dir = profile.path();
            let nm = profile_dir.join("node_modules");
            if nm.is_dir() {
                let _ = std::fs::remove_dir_all(&nm);
                touched += 1;
            }
            let _ = std::fs::remove_file(profile_dir.join("pnpm-lock.yaml"));
        }
    }
    if touched > 0 {
        let note = versions_root.join("MIGRATION-2026-09-12.txt");
        let _ = std::fs::write(
            &note,
            "启动器目录已从 instances/ 迁移为 versions/。\n\
             由于 pnpm 记录了旧路径，各版本已装插件的 node_modules 被一并清理；\n\
             插件清单（package.json）保留——到「下载 · 插件市场」或整合包重装即可。\n",
        );
    }
}

/// 确保托管根本身与固定子目录存在；返回根目录。
pub fn ensure_managed_dirs() -> Result<PathBuf, String> {
    let root = managed_root()?;
    std::fs::create_dir_all(&root).map_err(|e| format!("创建目录 {} 失败：{e}", root.display()))?;
    migrate_legacy_instances_dir(&root)?;
    for dir in [
        root.clone(),
        root.join(NODES_DIR),
        root.join(VERSIONS_DIR),
    ] {
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录 {} 失败：{e}", dir.display()))?;
    }
    Ok(root)
}

/// 读状态文件文本，并剥掉开头可能存在的 UTF-8 BOM。
///
/// **为什么必须容忍**：`serde_json` 不接受 BOM（报 `expected value at line 1
/// column 1`），而 Windows 上的编辑器与 PowerShell 5.1 的 `Set-Content -Encoding
/// UTF8` 默认就会写 BOM。2026-09-10 实际事故：手工把 `instances.json` 的端口从
/// 3080 改成 3090 时带进 BOM，登记表整体读不出来——启动页显示「页面数据加载
/// 失败」、下载版本与插件安装全部失效。**状态文件被外部工具碰过是常态，一个
/// BOM 不该让整个启动器瘫痪**；这里只剥 BOM，其余内容原样交给 serde，真正的
/// 损坏仍然 fail loud（不静默清空用户数据）。
pub fn read_text(path: &Path) -> Result<String, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    Ok(strip_bom(&raw).to_string())
}

/// 剥掉字符串开头的 UTF-8 BOM（`\u{feff}`），其余原样返回。
pub fn strip_bom(raw: &str) -> &str {
    raw.strip_prefix('\u{feff}').unwrap_or(raw)
}

/// 把 zip 条目名解析为「解压根 `root` 之下」的安全路径（zip-slip 防护）。
///
/// 条目名来自远端归档（技能仓库 / Node 发行包 / 源码 tag），不可信：反斜杠
/// 先归一为 zip 规范的 `/`，再拒绝绝对路径、盘符/UNC 前缀与 `.`/`..` 段——
/// 只允许普通段拼接，结果保证仍在 `root` 内。Windows 上 `C:evil` 这类盘符
/// 相对路径会在 components 里暴露为 Prefix 段被拒绝；非 Windows 上它只是
/// 一个普通文件名，放行无害。
pub fn safe_entry_path(root: &Path, entry_name: &str) -> Result<PathBuf, String> {
    let normalized = entry_name.replace('\\', "/");
    if normalized.is_empty() {
        return Err("zip 条目名为空".to_string());
    }
    let rel = Path::new(&normalized);
    if rel.is_absolute() || rel.has_root() {
        return Err(format!("zip 条目名不是相对路径，拒绝解压：{entry_name:?}"));
    }
    let mut out = root.to_path_buf();
    for comp in rel.components() {
        match comp {
            std::path::Component::Normal(seg) => out.push(seg),
            _ => return Err(format!("zip 条目名含非法路径段，拒绝解压：{entry_name:?}")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances::tests::sandbox;

    #[test]
    fn root_layout_is_stable() {
        // 用假 LOCALAPPDATA 验证拼装规则本身，不触碰真实用户目录；
        // 沙箱复用 instances 测试模块的互斥锁，防止并行用例互相改环境变量。
        let (_lock, tmp) = sandbox("paths");

        let root = managed_root().unwrap();
        assert_eq!(root, tmp.join("dsh-launcher"));
        assert_eq!(nodes_dir().unwrap(), root.join("nodes"));
        assert_eq!(versions_dir().unwrap(), root.join("versions"));
        assert_eq!(sources_dir().unwrap(), root.join("sources"));
        assert_eq!(shared_dir().unwrap(), root.join("shared"));
        assert_eq!(
            global_agents_md_file().unwrap(),
            root.join("AGENTS.md")
        );
        assert_eq!(instances_file().unwrap(), root.join("instances.json"));
        assert_eq!(market_cache_file().unwrap(), root.join("market-cache.json"));

        let ensured = ensure_managed_dirs().unwrap();
        assert!(ensured.join(NODES_DIR).is_dir());
        assert!(ensured.join(VERSIONS_DIR).is_dir());
    }

    #[test]
    fn legacy_instances_dir_migrates_once() {
        let (_lock, _tmp) = sandbox("paths-migrate");
        let root = managed_root().unwrap();

        // 旧布局：instances/ 里有真实实例目录与 .purging 临时目录，均应整体带走。
        let legacy_inst = root.join("instances").join("dsh-0.7.3");
        std::fs::create_dir_all(&legacy_inst).unwrap();
        std::fs::write(legacy_inst.join("instance.json"), b"{}").unwrap();
        let legacy_purge = root.join("instances").join(".purging-dsh-x-1");
        std::fs::create_dir_all(&legacy_purge).unwrap();

        ensure_managed_dirs().unwrap();
        assert!(!root.join("instances").exists(), "旧目录应已改名消失");
        assert!(
            root.join("versions").join("dsh-0.7.3").join("instance.json").is_file(),
            "实例目录应整体迁入 versions/"
        );
        assert!(root.join("versions").join(".purging-dsh-x-1").is_dir());

        // 幂等：再次 ensure 不再动 versions/，也不因旧目录消失而报错。
        ensure_managed_dirs().unwrap();
        assert!(root.join("versions").join("dsh-0.7.3").is_dir());
    }

    #[test]
    fn migration_skipped_when_versions_exists() {
        let (_lock, _tmp) = sandbox("paths-migrate-skip");
        let root = managed_root().unwrap();

        // versions/ 已存在（迁移完成过）而旧 instances/ 残留：不得覆盖新目录。
        std::fs::create_dir_all(root.join("instances").join("old")).unwrap();
        let keep = root.join("versions").join("dsh-0.8.0");
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::write(keep.join("instance.json"), b"{}").unwrap();

        ensure_managed_dirs().unwrap();
        assert!(root.join("instances").exists(), "残留旧目录应原样保留不覆盖");
        assert!(root.join("versions").join("dsh-0.8.0").join("instance.json").is_file());
    }

    #[test]
    fn read_text_strips_bom_only() {
        let (_lock, tmp) = sandbox("paths-bom");
        let dir = tmp.join("bom");
        std::fs::create_dir_all(&dir).unwrap();

        // 带 BOM 的文件：读出来必须是纯正文。
        let with_bom = dir.join("with-bom.json");
        std::fs::write(&with_bom, "\u{feff}{\"a\":1}").unwrap();
        assert_eq!(read_text(&with_bom).unwrap(), "{\"a\":1}");

        // 不带 BOM：逐字不变（正文里出现的 U+FEFF 不能被误删）。
        let plain = dir.join("plain.json");
        std::fs::write(&plain, "{\"a\":\"\u{feff}\"}").unwrap();
        assert_eq!(read_text(&plain).unwrap(), "{\"a\":\"\u{feff}\"}");

        // 读不到的路径如实报错，不静默成空串。
        let missing = dir.join("nope.json");
        assert!(read_text(&missing).is_err());
    }

    #[test]
    fn safe_entry_path_blocks_slip_and_keeps_normal() {
        let root = std::env::temp_dir().join("dsh-paths-test-root");

        // 普通相对条目必须原样落在 root 下。
        let ok = safe_entry_path(&root, "node-v22-win-x64/node.exe").unwrap();
        assert_eq!(ok, root.join("node-v22-win-x64").join("node.exe"));

        // 各类穿越/逃逸形态全部拒绝。
        for bad in [
            "../evil.txt",
            "a/../../evil.txt",
            "..\\..\\evil.txt",
            "/abs/evil.txt",
            "./evil.txt",
            "",
            "a//b/../..//evil.txt",
        ] {
            assert!(safe_entry_path(&root, bad).is_err(), "应拒绝：{bad:?}");
        }

        // 盘符相对路径（`C:evil`）在 Windows 上是 Prefix 段必须拒绝；
        // 非 Windows 上它只是普通文件名，放行无害（可移植差异显式钉住）。
        let drive = safe_entry_path(&root, "C:evil");
        if cfg!(windows) {
            assert!(drive.is_err(), "Windows 上应拒绝盘符路径");
        } else {
            assert!(drive.is_ok());
        }
    }
}
