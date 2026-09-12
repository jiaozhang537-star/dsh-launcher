//! DSH 源码归档下载（FR-22）：把官方仓库按版本对应的 tag 下载解压，供 AI 辅助
//! 插件开发等场景参考源码。
//!
//! 版本↔tag 映射（侦察验证 7/7 成立）：npm 版本 `X` ↔ GitHub tag `dsh-vX`，
//! 仓库 `deepseek-ai/deepseek-harness`（npm packument repository 字段）。
//! 0.1.0-rc.6 及更早版本没有 tag，404 转译为可读报错。
//!
//! 落点（FR-22 修订，需求方 2026-09-12 定稿，见 FILE-LAYOUT §5）：
//! - 随实例下载 / 版本设置页补下载 → `versions/<id>/sources/`（目录存在 = 已下载过）；
//! - 仅源码下载（不建实例）→ 全局 `sources/<tag>/`，重名自动加副本后缀（`-副本2`…）。
//!
//! 边界（需求方拍板）：这是「可选项」，默认下载路径仍是创建实例；源码归档
//! 启动器**不安装、不管理、不可用于启动**——产物只是一份静态参考源码树。
//! 下载经统一下载网关（GitHub 类 URL，custom 前缀生效），取消与停滞看门狗
//! 复用 node_runtime 的 DOWNLOAD_CANCEL。

use std::path::{Path, PathBuf};

use crate::gateway;
use crate::mirror_config;
use crate::node_runtime;
use crate::paths;
use crate::registry_meta;

/// 进度事件名（前端 listen 用；payload 携带 version 供多任务分发）。
pub const PROGRESS_EVENT: &str = "source-download-progress";

/// 官方源码仓库（npm packument repository 字段指向的 monorepo）。
pub const SOURCE_REPO: &str = "deepseek-ai/deepseek-harness";

/// npm 版本号 → 官方源码 tag（确定性映射，侦察验证 7/7）。
/// 版本号必须先通过 semver 键校验，杜绝把任意字符串拼进下载 URL。
pub fn dsh_source_tag(version: &str) -> Result<String, String> {
    registry_meta::parse_semver_key(version)
        .ok_or_else(|| format!("非法版本号：{version:?}，无法映射源码 tag"))?;
    Ok(format!("dsh-v{version}"))
}

/// tag 对应的官方归档 URL（zip 形态：复用现有 zip 依赖与解压手法，零新增依赖）。
pub fn archive_url(tag: &str) -> String {
    format!("{}/{SOURCE_REPO}/archive/refs/tags/{tag}.zip", gateway::GITHUB_OFFICIAL)
}

/// 全局归档落盘目录：`<managed root>/sources/<tag>/`；重名自动加副本后缀
/// （`-副本2`、`-副本3`…需求方 2026-09-12 定稿）——绝不覆盖已下载的内容。
pub fn resolve_global_dir(tag: &str) -> Result<PathBuf, String> {
    let root = paths::sources_dir()?;
    let first = root.join(tag);
    if !first.exists() {
        return Ok(first);
    }
    let mut n = 2u32;
    loop {
        let candidate = root.join(format!("{tag}-副本{n}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
        n += 1;
    }
}

/// 版本内归档落盘目录：`versions/<id>/sources/`（目录存在 = 该实例已下载过）。
pub fn instance_sources_dir(instance_id: &str) -> Result<PathBuf, String> {
    Ok(crate::instances::instance_dir(instance_id)?.join("sources"))
}

/// 下载并解压指定版本的源码归档（阻塞调用——command 侧放 spawn_blocking）。
/// `instance_id` = Some 时落版本目录（已存在即报错，不覆盖）；None 时落全局
/// `sources/<tag>/`（重名自动加副本后缀）。成功返回解压后的源码目录；
/// `progress` 每 ≥256KB 回调一次 (received, total)。
pub fn download(
    version: &str,
    instance_id: Option<&str>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let tag = dsh_source_tag(version)?;
    let settings = mirror_config::load()?;
    let url = gateway::resolve(&archive_url(&tag), &settings)?;
    let target = match instance_id {
        Some(id) => {
            crate::instances::get_instance(id)?;
            let dir = instance_sources_dir(id)?;
            if dir.exists() {
                return Err(format!(
                    "该实例已下载过源码：{}（如需重新下载请先删除该目录）",
                    dir.display()
                ));
            }
            dir
        }
        None => resolve_global_dir(&tag)?,
    };
    let parent = target
        .parent()
        .ok_or_else(|| "内部错误：源码目标目录缺少父级".to_string())?;
    std::fs::create_dir_all(&parent).map_err(|e| format!("创建 {} 失败：{e}", parent.display()))?;

    match node_runtime::try_download(&url, &format!("{tag}.zip"), progress) {
        Ok(zip_path) => {
            extract_zip_to(&zip_path, &target)?;
            let _ = std::fs::remove_file(&zip_path);
            Ok(target)
        }
        // 官方对不存在的 tag 返回 404（自定义前缀代理失效同样以 404 呈现）。
        Err(e) if e.contains("status code 404") => Err(format!(
            "官方未为版本 {version} 发布源码 tag（{tag} 不存在）——\
             0.1.0-rc.6 及更早版本没有源码标签；若在用自定义镜像前缀，也请确认其可用"
        )),
        Err(e) => Err(e),
    }
}

/// 解压 GitHub 归档 zip 到 target：剥掉压缩包首层目录（`deepseek-harness-<tag>/`），
/// 完成后校验目录非空（缺文件视为产物异常）。
fn extract_zip_to(zip_path: &Path, target: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开 zip 失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| format!("读取 zip 结构失败：{e}"))?;

    std::fs::create_dir_all(target).map_err(|e| format!("创建 {} 失败：{e}", target.display()))?;

    let mut files = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("遍历 zip 条目失败：{e}"))?;
        let raw_name = entry.name().to_string();
        let Some(relative) = raw_name.split_once('/').map(|(_, rest)| rest.to_string()) else {
            continue; // 首层目录项本身跳过
        };
        if relative.is_empty() {
            continue;
        }
        // 剥层后的 relative 才是落在 target 下的部分；helper 在其上再做
        // zip-slip 防护（错误信息携带的即条目内的相对路径）。
        let out_path = crate::paths::safe_entry_path(target, &relative)?;
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("创建 {} 失败：{e}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压 {relative} 失败：{e}"))?;
        files += 1;
    }

    if files == 0 {
        return Err(format!(
            "解压完成但 {} 内没有任何文件，产物异常",
            target.display()
        ));
    }
    Ok(())
}

/// 在系统文件管理器中打开已解压的源码目录（目录不存在时报错）。
/// `instance_id` = Some 时开版本内 `sources/`，None 时开全局 `<tag>/`。
pub fn open_dir(app: &tauri::AppHandle, version: &str, instance_id: Option<&str>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let _tag = dsh_source_tag(version)?;
    let dir = match instance_id {
        Some(id) => {
            crate::instances::get_instance(id)?;
            instance_sources_dir(id)?
        }
        None => paths::sources_dir()?.join(dsh_source_tag(version)?),
    };
    if !dir.is_dir() {
        return Err(format!("源码目录不存在（尚未下载或已被删除）：{}", dir.display()));
    }
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| format!("打开源码目录失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 侦察实证的 7 个 tag 一一对应；非法版本被拒绝。
    #[test]
    fn source_tag_mapping_matches_recon_7_of_7() {
        for (npm, tag) in [
            ("0.1.2-alpha.3", "dsh-v0.1.2-alpha.3"),
            ("0.1.2-alpha.2", "dsh-v0.1.2-alpha.2"),
            ("0.1.2-alpha.1", "dsh-v0.1.2-alpha.1"),
            ("0.1.1-rc.2", "dsh-v0.1.1-rc.2"),
            ("0.1.1-rc.1", "dsh-v0.1.1-rc.1"),
            ("0.1.0-rc.8", "dsh-v0.1.0-rc.8"),
            ("0.1.0-rc.7", "dsh-v0.1.0-rc.7"),
        ] {
            assert_eq!(dsh_source_tag(npm).unwrap(), tag);
        }
        for bad in ["", "banana", "1.2", "../evil", "1.2.3.4"] {
            assert!(dsh_source_tag(bad).is_err(), "{bad:?} 应被拒绝");
        }
    }

    #[test]
    fn archive_url_has_official_shape() {
        assert_eq!(
            archive_url("dsh-v0.1.1-rc.2"),
            "https://github.com/deepseek-ai/deepseek-harness/archive/refs/tags/dsh-v0.1.1-rc.2.zip"
        );
    }

    /// zip-slip 防护：含穿越条目的归档整体拒绝，恶意条目不得落到 target 之外。
    #[test]
    fn extract_zip_to_rejects_slip_entries() {
        let base = std::env::temp_dir().join(format!("dsh-source-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let target = base.join("target");
        let zip_path = base.join("evil.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, data) in [
            ("deepseek-harness-dsh-v0.1.1-rc.2/README.md", "ok".as_bytes()),
            ("deepseek-harness-dsh-v0.1.1-rc.2/../../evil.txt", b"pwned".as_slice()),
        ] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, data).unwrap();
        }
        writer.finish().unwrap();

        assert!(extract_zip_to(&zip_path, &target).is_err());
        assert!(!base.join("evil.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 全局落点重名自动加副本后缀（FR-22 修订）：不覆盖既有目录，编号顺延。
    #[test]
    fn global_collision_uses_copy_suffix() {
        let (_lock, _tmp) = crate::instances::tests::sandbox("source-copy");
        let tag = dsh_source_tag("0.1.1-rc.2").unwrap();

        // 无同名：直接用 <tag>。
        let first = resolve_global_dir(&tag).unwrap();
        assert_eq!(first.file_name().unwrap().to_string_lossy(), tag);

        // 同名已存在：副本2、副本3 顺延，绝不指向已存在的目录。
        std::fs::create_dir_all(&first).unwrap();
        let second = resolve_global_dir(&tag).unwrap();
        assert_eq!(second.file_name().unwrap().to_string_lossy(), format!("{tag}-副本2"));
        std::fs::create_dir_all(&second).unwrap();
        let third = resolve_global_dir(&tag).unwrap();
        assert_eq!(third.file_name().unwrap().to_string_lossy(), format!("{tag}-副本3"));
    }

    /// 版本内落点：目录存在（= 已下载过）即拒绝，fail loud 不覆盖。
    #[test]
    fn instance_sources_refuses_existing_dir() {
        let (_lock, tmp) = crate::instances::tests::sandbox("source-inst");
        let instance = crate::instances::create_instance("源码实例", "headless").unwrap();
        let dir = instance_sources_dir(&instance.id).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("keep"), b"x").unwrap();
        let mut sink = |_r: u64, _t: u64| {};
        let err = download("0.1.1-rc.2", Some(&instance.id), &mut sink).unwrap_err();
        assert!(err.contains("已下载过源码"), "实际错误：{err}");
        assert!(dir.join("keep").exists(), "既有内容不得被覆盖");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 版本内落点：实例必须存在（fail loud），不静默建孤儿目录。
    #[test]
    fn instance_sources_requires_existing_instance() {
        let (_lock, _tmp) = crate::instances::tests::sandbox("source-noinst");
        let mut sink = |_r: u64, _t: u64| {};
        let err = download("0.1.1-rc.2", Some("i-missing"), &mut sink).unwrap_err();
        assert!(err.contains("实例"), "实际错误：{err}");
    }

    /// 真实下载 e2e（显式触发）：0.1.1-rc.2 归档解压后必须含 monorepo 的
    /// apps/cli 目录（npm packument repository.directory 的侦察事实）。
    /// 需网络 + 数十 MB 下载，默认 `#[ignore]`。
    #[test]
    #[ignore]
    fn real_download_extracts_source_tree_with_apps_cli() {
        let (_lock, tmp) = crate::instances::tests::sandbox("source-real");
        let mut events = 0usize;
        let dir = download("0.1.1-rc.2", None, &mut |received, total| {
            events += 1;
            println!("[源码] 进度 #{events}: {received}/{total} 字节");
        })
        .unwrap_or_else(|e| panic!("源码下载失败：{e}"));
        assert!(dir.join("apps").join("cli").is_dir(), "解压产物缺 apps/cli：{}", dir.display());
        assert!(events > 0, "全程未收到进度回调");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 无 tag 版本（0.1.0-rc.1 无对应 tag）报错可读。需网络，默认 `#[ignore]`。
    #[test]
    #[ignore]
    fn real_download_without_tag_fails_readable() {
        let (_lock, tmp) = crate::instances::tests::sandbox("source-404");
        let mut sink = |_r: u64, _t: u64| {};
        let err = download("0.1.0-rc.1", None, &mut sink).unwrap_err();
        assert!(err.contains("源码 tag"), "实际错误：{err}");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
