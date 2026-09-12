//! Skill 中心（FR-14，T-4 定稿）：与插件市场同级的资源分类。
//!
//! Skill 与插件本质不同（T-4 结论）：插件是声明 `dsh.bundle.patch` 的 npm
//! 包，Skill 是**文件布局**——`<name>/SKILL.md`（或平铺 `<name>.md`，不支持
//! 递归扫描），frontmatter name/description/whenToUse。分发源 = GitHub 话题
//! `dsh-skill`（已存在、145 成员、与 dsh-plugin 同样嘈杂）；安装 = 经网关
//! 下载仓库归档，扫描根级与一级子目录的 SKILL.md，整目录拷入
//! **`<实例>/skills/<name>/`**（DSH skill-filesystem 的 `user-dsh` 根，
//! rank 400，`DSH_HOME`=实例目录 ⇒ 实例级隔离；DSH 自带目录 watcher）。
//!
//! dsh-hub 无技能目录（T-4 实证），社区镜像源=dshhub 时技能仍走官方 GitHub
//! 聚合；离线降级复用 market-cache 的快照机制（独立 skills 键）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::gateway;
use crate::github;
use crate::instances;
use crate::market_cache;
use crate::mirror_config;
use crate::node_runtime;

/// GitHub 技能聚合话题（T-4 定稿）。
pub const GITHUB_TOPIC: &str = "dsh-skill";

/// 技能安装落点：`<实例>/skills`（skill-filesystem `user-dsh` 根）。
pub const SKILLS_DIR_NAME: &str = "skills";

/// 技能进度事件（Tauri 事件名）。
pub const PROGRESS_EVENT: &str = "skill-install-progress";

/// 技能目录条目（浏览列表一行；需要 Deserialize：离线快照回放）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCard {
    pub name: String,
    /// `owner/repo`。
    pub sub_name: String,
    pub description: String,
    pub icon_url: String,
    pub stars: u64,
    /// 最近 push（RFC3339；空串 = 未知）。
    pub updated_at: String,
    pub repo: String,
}

/// 技能信息流（快照降级语义同市场）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFeed {
    pub cards: Vec<SkillCard>,
    pub offline: bool,
    pub fetched_at: String,
}

/// 安装结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallOutcome {
    #[serde(rename = "instanceId")]
    pub instance_id: String,
    /// 实际安装的技能名列表（一个仓库可能含多个技能目录）。
    pub installed: Vec<String>,
    /// 技能落盘根目录：`<实例>/skills`。
    pub target_dir: String,
    /// 是否发生了覆盖安装（同名技能已存在）。
    pub overwritten: bool,
}

/// 仓库标识合法性：恰好 `owner/repo` 且两段非空无危险字符。
pub fn validate_repo(repo: &str) -> Result<(), String> {
    let repo = repo.trim();
    if repo.matches('/').count() != 1 {
        return Err(format!("仓库标识必须是 owner/repo 形态：{repo:?}"));
    }
    for part in repo.split('/') {
        if part.is_empty()
            || part.contains([' ', '?', '#', '&', '\\', '\'', '"'])
            || part == "."
            || part == ".."
        {
            return Err(format!("仓库标识含非法段：{repo:?}"));
        }
    }
    Ok(())
}

/// 从 SKILL.md 文本解析 frontmatter 的 (name, description)。
/// 结构：首行 `---` 与下一个 `---` 之间每行 `key: value`；缺 name 视为无
/// frontmatter（安装时回落目录名）。
pub fn parse_frontmatter(text: &str) -> (Option<String>, Option<String>) {
    let mut lines = text.lines().map(str::trim);
    if lines.next() != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut description = None;
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().trim_matches(['"', '\'']).to_string();
            match key.trim() {
                "name" if !value.is_empty() => name = Some(value),
                "description" if !value.is_empty() => description = Some(value),
                _ => {}
            }
        }
    }
    (name, description)
}

/// 技能目录名：frontmatter name 优先，目录名兜底，两者都不可用给无害缺省名。
/// 校验是跨平台的（frontmatter 来自远端仓库，不可信）：拒绝路径分隔符/空白/
/// 冒号（Windows 盘符与 ADS 流名）/结尾点（Win32 别名折叠）/Windows 保留
/// 设备名——否则 `C:evil` 之类名字经 `join` 会整体替换落点。
pub fn skill_dir_name(front_name: Option<&str>, fallback: &str) -> String {
    if let Some(name) = front_name {
        let trimmed = name.trim();
        if valid_dir_component(trimmed) {
            return trimmed.to_string();
        }
    }
    let fb = fallback.trim();
    if valid_dir_component(fb) {
        return fb.to_string();
    }
    "skill".to_string()
}

/// 目录名可用性：可直接作 `<实例>/skills/<name>` 的最后一段。
fn valid_dir_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.contains(['/', '\\', ' ', ':'])
        && !name.ends_with('.')
        && name != "."
        && name != ".."
        && !is_windows_reserved_name(name)
}

/// Windows 保留设备名（含 `CON.txt` 这类带扩展名形态，按首个点前段判定）。
fn is_windows_reserved_name(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or("");
    matches!(
        base.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9"
            | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"
    )
}

/// 枚举解压树里的技能目录：根级 SKILL.md 算一个（技能名 = 仓库名），每个
/// 一级子目录的 SKILL.md 各算一个（T-4：不支持更深递归——与 DSH 扫描口径
/// 一致）。
pub fn discover_skill_dirs(root: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    if root.join("SKILL.md").is_file() {
        out.push((root.to_path_buf(), root.file_name().unwrap_or_default().to_string_lossy().to_string()));
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        if dir.join("SKILL.md").is_file() {
            out.push((
                dir.clone(),
                dir.file_name().unwrap_or_default().to_string_lossy().to_string(),
            ));
        }
    }
    out
}

/// 技能信息流（阻塞网络调用——command 侧放 spawn_blocking）：GitHub 话题
/// 聚合（2 页 × 100），全败回同键快照标 offline，无快照报错。
pub fn fetch_feed() -> Result<SkillFeed, String> {
    let mut errors = Vec::new();
    let mut cards: Vec<SkillCard> = Vec::new();
    for page in 1..=2u32 {
        match github::search_topic_page(GITHUB_TOPIC, page) {
            Ok(repos) => {
                for repo in repos {
                    if repo.full_name.eq_ignore_ascii_case(crate::market::HARNESS_REPO) {
                        continue;
                    }
                    cards.push(SkillCard {
                        name: repo.full_name.rsplit('/').next().unwrap_or(&repo.full_name).to_string(),
                        sub_name: repo.full_name.clone(),
                        description: repo.description.clone(),
                        icon_url: repo.avatar_url.clone(),
                        stars: repo.stars,
                        updated_at: repo.pushed_at.clone(),
                        repo: repo.full_name.clone(),
                    });
                }
            }
            Err(e) => {
                errors.push(e);
                break;
            }
        }
    }
    if !cards.is_empty() {
        let fetched_at = instances::now_rfc3339();
        market_cache::update(|cache| {
            cache.skills = Some(market_cache::make_snapshot(
                fetched_at.clone(),
                format!("skills:{GITHUB_TOPIC}"),
                cards
                    .iter()
                    .map(|c| serde_json::to_value(c).unwrap_or_default())
                    .collect(),
            ));
        })?;
        return Ok(SkillFeed { cards, offline: false, fetched_at });
    }
    let cache = market_cache::load()?;
    let marker = format!("skills:{GITHUB_TOPIC}");
    // P3-05：离线回放前验完整性（校验和不符即判无效，宁报错不喂脏数据）。
    if let Some(snapshot) = cache.skills.filter(|s| {
        s.community_active == marker && !s.cards.is_empty() && market_cache::snapshot_intact(s)
    }) {
        let mut cards: Vec<SkillCard> = snapshot
            .cards
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        cards.sort_by(|a, b| b.stars.cmp(&a.stars).then_with(|| a.name.cmp(&b.name)));
        return Ok(SkillFeed { cards, offline: true, fetched_at: snapshot.fetched_at });
    }
    Err(if errors.is_empty() {
        "技能目录为空".into()
    } else {
        errors.join("\n")
    })
}

/// 仓库归档 URL（默认分支 zip；经网关 resolve，custom 前缀生效）。
fn archive_url(repo: &str, branch: &str) -> String {
    format!("{}/{repo}/archive/refs/heads/{branch}.zip", gateway::GITHUB_OFFICIAL)
}

/// 递归拷贝目录（技能体积小，无进度粒度需求）。
fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("创建 {} 失败：{e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("读 {} 失败：{e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| format!("拷贝 {} 失败：{e}", src.display()))?;
        }
    }
    Ok(())
}

/// 把一个仓库的技能装进实例（阻塞调用——command 侧放 spawn_blocking）。
/// 流程：repo 元数据取默认分支 → 网关下载归档 → 解压到实例内 staging →
/// 扫描 SKILL.md（frontmatter 定名）→ 逐技能拷入 `<实例>/skills/<name>/`
/// （同名覆盖安装）→ 清理 staging。同一仓库重复安装 = 覆盖，幂等。
pub fn install(
    instance_id: &str,
    repo: &str,
    progress: &mut dyn FnMut(String),
) -> Result<SkillInstallOutcome, String> {
    let repo = repo.trim().trim_matches('/').to_string();
    validate_repo(&repo)?;
    let instance_dir = instances::instance_dir(instance_id)?;
    let skills_dir = instance_dir.join(SKILLS_DIR_NAME);

    progress("获取仓库信息…".into());
    let branch = repo_default_branch(&repo)?;

    progress(format!("下载 {repo}（分支 {branch}）…"));
    let settings = mirror_config::load()?;
    let url = gateway::resolve(&archive_url(&repo, &branch), &settings)?;
    let zip_name = format!("skill-{}.zip", repo.replace('/', "-"));
    let zip_path = node_runtime::try_download(&url, &zip_name, &mut |received, total| {
        if total > 0 {
            let pct = received * 100 / total;
            if pct % 10 == 0 {
                progress(format!("下载中 {pct}%"));
            }
        }
    })
    .map_err(|e| format!("下载技能仓库失败：{e}"))?;

    // 解压到实例内 staging（同盘保证可原子改名），扫完即清。
    let staging = instance_dir.join(format!(".skill-staging-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let extract_result = extract_zip_root(&zip_path, &staging);
    let _ = std::fs::remove_file(&zip_path);
    extract_result?;

    // GitHub 归档带首层 `<repo>-<branch>/` 目录；技能树从那里面扫。
    let tree_root = match first_child_dir(&staging) {
        Some(dir) => dir,
        None => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err("归档解压后没有内容——产物异常".into());
        }
    };
    let candidates = discover_skill_dirs(&tree_root);
    if candidates.is_empty() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!(
            "{repo} 里没有找到 SKILL.md（根级或一级子目录）——它可能不是技能仓库"
        ));
    }

    std::fs::create_dir_all(&skills_dir).map_err(|e| format!("创建 {} 失败：{e}", skills_dir.display()))?;
    let mut installed = Vec::new();
    let mut overwritten = false;
    for (dir, fallback_name) in &candidates {
        let text = std::fs::read_to_string(dir.join("SKILL.md"))
            .map_err(|e| format!("读 SKILL.md 失败：{e}"))?;
        let (front_name, _description) = parse_frontmatter(&text);
        let name = skill_dir_name(front_name.as_deref(), fallback_name);
        let target = skills_dir.join(&name);
        if target.exists() {
            overwritten = true;
            std::fs::remove_dir_all(&target).map_err(|e| format!("清理旧技能 {name} 失败：{e}"))?;
        }
        copy_dir(dir, &target)?;
        installed.push(name);
    }
    let _ = std::fs::remove_dir_all(&staging);
    progress(format!("已安装 {} 个技能：{}", installed.len(), installed.join("、")));
    Ok(SkillInstallOutcome {
        instance_id: instance_id.to_string(),
        installed,
        target_dir: skills_dir.display().to_string(),
        overwritten,
    })
}

/// 仓库默认分支（GET /repos/{repo}，ETag 缓存生效）。
fn repo_default_branch(repo: &str) -> Result<String, String> {
    validate_repo(repo)?;
    let outcome = github::api_get_json(&format!("/repos/{repo}"), "")?;
    outcome
        .body
        .get("default_branch")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("{repo} 的元数据缺 default_branch"))
}

/// 解压 zip 到 staging，保留归档首层结构（首层目录由 first_child_dir 定位）。
fn extract_zip_root(zip_path: &Path, staging: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开 zip 失败：{e}"))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| format!("读取 zip 结构失败：{e}"))?;
    std::fs::create_dir_all(staging).map_err(|e| format!("创建 staging 失败：{e}"))?;
    let mut files = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("遍历 zip 失败：{e}"))?;
        let raw = entry.name().to_string();
        if raw.is_empty() || raw.ends_with('/') {
            continue;
        }
        let out_path = crate::paths::safe_entry_path(staging, &raw)?;
        let Some(parent) = out_path.parent() else { continue };
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("创建 {} 失败：{e}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压 {raw} 失败：{e}"))?;
        files += 1;
    }
    if files == 0 {
        return Err("归档里没有任何文件".into());
    }
    Ok(())
}

/// staging 下唯一的首层目录（GitHub 归档形态 `repo-branch/…`）。
fn first_child_dir(root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_validation_rejects_bad_shapes() {
        assert!(validate_repo("owner/repo").is_ok());
        assert!(validate_repo("justname").is_err());
        assert!(validate_repo("a/b/c").is_err());
        assert!(validate_repo("a/b c").is_err());
        assert!(validate_repo("../etc").is_err());
        assert!(validate_repo("").is_err());
    }

    #[test]
    fn frontmatter_parses_name_and_description() {
        let text = "---\nname: math-skill\ndescription: 数学研究技能\nwhenToUse: 算数学题\n---\n# body";
        let (name, desc) = parse_frontmatter(text);
        assert_eq!(name.as_deref(), Some("math-skill"));
        assert_eq!(desc.as_deref(), Some("数学研究技能"));

        let (name, desc) = parse_frontmatter("# 无 frontmatter\n内容");
        assert_eq!(name, None);
        assert_eq!(desc, None);

        let (name, _) = parse_frontmatter("---\ndescription: 只有描述\n---\n");
        assert_eq!(name, None, "缺 name 不硬造");
    }

    #[test]
    fn skill_dir_name_sanitizes_unsafe_frontmatter() {
        assert_eq!(skill_dir_name(Some("math-skill"), "repo"), "math-skill");
        assert_eq!(skill_dir_name(Some("../evil"), "repo"), "repo", "路径穿越回落目录名");
        assert_eq!(skill_dir_name(Some("a b"), "repo"), "repo", "含空格回落目录名");
        assert_eq!(skill_dir_name(Some(""), "repo"), "repo");
        assert_eq!(skill_dir_name(None, "fallback"), "fallback");
    }

    #[test]
    fn skill_dir_name_blocks_windows_specific_escapes() {
        // 盘符相对路径：join 会整体替换落点，必须回落。
        assert_eq!(skill_dir_name(Some("C:evil"), "repo"), "repo");
        assert_eq!(skill_dir_name(Some("a:stream"), "repo"), "repo", "ADS 流名回落");
        // Windows 保留设备名与结尾点（Win32 别名折叠）。
        assert_eq!(skill_dir_name(Some("con"), "repo"), "repo");
        assert_eq!(skill_dir_name(Some("NUL"), "repo"), "repo");
        assert_eq!(skill_dir_name(Some("com1"), "repo"), "repo");
        assert_eq!(skill_dir_name(Some("CON.txt"), "repo"), "repo");
        assert_eq!(skill_dir_name(Some("x."), "repo"), "repo", "尾点折叠回落");
        // 兜底目录名也不可用时给无害缺省名，绝不把不可信名字落到 skills 下。
        assert_eq!(skill_dir_name(Some("C:evil"), "con"), "skill");
        assert_eq!(skill_dir_name(None, ".."), "skill");
        // 前导点的普通名字仍合法（原行为保留）。
        assert_eq!(skill_dir_name(Some(".hidden"), "repo"), ".hidden");
    }

    /// 构造测试 zip：条目名 → 内容。
    fn craft_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, data) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, data).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn extract_zip_root_rejects_slip_entries() {
        let base = std::env::temp_dir().join(format!("dsh-skill-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let staging = base.join("staging");
        let zip_path = base.join("evil.zip");
        craft_zip(
            &zip_path,
            &[
                ("repo-main/SKILL.md", b"---\nname: ok\n---\n".as_slice()),
                ("../../evil.txt", b"pwned".as_slice()),
            ],
        );
        let result = extract_zip_root(&zip_path, &staging);
        assert!(result.is_err(), "含穿越条目的归档必须整体拒绝");
        let _ = std::fs::remove_dir_all(&staging);
        // 恶意条目不得写到 staging 之外。
        assert!(!base.join("evil.txt").exists());
        assert!(!base.parent().unwrap().join("evil.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn extract_zip_root_accepts_normal_archive() {
        let base = std::env::temp_dir().join(format!("dsh-skill-zip-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let staging = base.join("staging");
        let zip_path = base.join("ok.zip");
        craft_zip(
            &zip_path,
            &[
                ("repo-main/SKILL.md", b"---\nname: ok\n---\n".as_slice()),
                ("repo-main/sub/nested.txt", b"data".as_slice()),
            ],
        );
        extract_zip_root(&zip_path, &staging).expect("正常归档应解压成功");
        assert!(staging.join("repo-main").join("SKILL.md").is_file());
        assert!(staging.join("repo-main").join("sub").join("nested.txt").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn discover_scans_root_and_first_level_only() {
        let base = std::env::temp_dir().join(format!("dsh-skill-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("repo-main");
        for dir in ["", "alpha", "beta", "gamma", "alpha/nested"] {
            let path = root.join(dir);
            std::fs::create_dir_all(&path).unwrap();
        }
        for skill in ["SKILL.md", "alpha/SKILL.md", "gamma/SKILL.md", "alpha/nested/SKILL.md", "beta/README.md"] {
            std::fs::write(root.join(skill), b"---\nname: x\n---\n").unwrap();
        }
        let found = discover_skill_dirs(&root);
        let names: Vec<String> = found.iter().map(|(_, n)| n.clone()).collect();
        assert_eq!(names, vec!["repo-main", "alpha", "gamma"], "根级 + 一级子目录；beta 无 SKILL.md；不递归");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn archive_url_shape() {
        assert_eq!(
            archive_url("owner/repo", "main"),
            "https://github.com/owner/repo/archive/refs/heads/main.zip"
        );
    }

    /// 真实 e2e（显式触发）：把 the-thinker0/math-skill（根级 SKILL.md）装进
    /// 沙箱实例，断言落盘 `<实例>/skills/<name>/SKILL.md` 且重复安装幂等覆盖。
    #[test]
    #[ignore = "真实网络：下载 GitHub 仓库归档并安装"]
    fn real_install_skill_repo_into_instance() {
        let (_lock, tmp) = crate::instances::tests::sandbox("skills-real");
        let instance = crate::instances::create_instance("skill-test", "web").unwrap();
        let mut sink = |d: String| println!("  [进度] {d}");
        let outcome = install(&instance.id, "the-thinker0/math-skill", &mut sink)
            .unwrap_or_else(|e| panic!("技能安装失败：{e}"));
        assert_eq!(outcome.installed.len(), 1, "根级 SKILL.md → 恰一个技能：{:?}", outcome.installed);
        assert!(!outcome.overwritten);
        let name = &outcome.installed[0];
        let skill_md = instance_dir_of(&instance.id).join(SKILLS_DIR_NAME).join(name).join("SKILL.md");
        assert!(skill_md.is_file(), "落盘缺 SKILL.md：{}", skill_md.display());

        // 重复安装 = 覆盖，幂等。
        let again = install(&instance.id, "the-thinker0/math-skill", &mut sink).unwrap();
        assert!(again.overwritten);
        assert_eq!(again.installed, outcome.installed);
        std::fs::remove_dir_all(&tmp).ok();
    }

    fn instance_dir_of(id: &str) -> PathBuf {
        crate::instances::instance_dir(id).unwrap()
    }

    /// FR-14 会话级认可实证（验收锚点，显式触发）。
    ///
    /// 手段：用**实例自装**的 `@deepseek-ai/dsh-skill-filesystem`（与运行中的
    /// web 实例同一份代码、同一 `resolveDshHome()`=DSH_HOME 契约）跑一次真实
    /// discovery，好坏对照断言——
    /// ① 好技能从 `<实例>/skills/` 被发现并接受（source=user-dsh，rank 400）；
    /// ② 坏 frontmatter 的对照技能被显式拒绝（ignored 警告）。
    ///
    /// 为什么不直接看运行日志/技能中心面板：cordis logger 默认只进内存环形
    /// 缓冲不落 stdout；0.1.x 的技能中心面板不渲染 user-dsh 分组（GUI 呈现
    /// 缺口，已记 phase-03 日志）。因此认可证据以 DSH 自己的 discovery 结果
    /// 为准；完整启动加载另有 real_plugin_recognized_by_dsh_launch 同型锚点。
    #[test]
    #[ignore = "真实实例：用 DSH 自装 skill-filesystem 跑 discovery（好坏对照）"]
    fn real_skill_recognized_by_dsh_session() {
        let all = crate::instances::list_instances().expect("实例注册表可读");
        let target = all
            .iter()
            .find(|i| i.dsh_version.is_some())
            .expect("本机需要一个已安装 DSH 的实例")
            .clone();
        println!("e2e 目标实例：{}（profile {}）", target.id, target.profile);

        let app_dir = crate::dsh_install::app_dir(&target.id).expect("安装区可解析");
        let sf_lib = app_dir
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh-skill-filesystem")
            .join("lib")
            .join("index.js");
        assert!(sf_lib.is_file(), "实例缺 skill-filesystem：{}", sf_lib.display());
        let node_exe = crate::dsh_install::resolve_install_node(&target, &mut |_stage| {})
            .expect("无可用 Node");

        let home = crate::instances::instance_dir(&target.id).unwrap();
        let skills_root = home.join(SKILLS_DIR_NAME);
        // 幂等起点 + 失败也清场（Drop 守卫）。
        let _guard = SkillDirsCleanup(skills_root.clone());
        let _ = std::fs::remove_dir_all(skills_root.join("dsh-e2e-good"));
        let _ = std::fs::remove_dir_all(skills_root.join("dsh-e2e-bad"));
        std::fs::create_dir_all(skills_root.join("dsh-e2e-good")).unwrap();
        std::fs::create_dir_all(skills_root.join("dsh-e2e-bad")).unwrap();
        std::fs::write(
            skills_root.join("dsh-e2e-good").join("SKILL.md"),
            "---\nname: dsh-e2e-good\ndescription: 启动器 e2e 会话认可验证用技能\n---\n\nBody of good skill.\n",
        )
        .unwrap();
        // 坏样本：缺 description → DSH 必然警告 ignored（根被扫描的证据）。
        std::fs::write(
            skills_root.join("dsh-e2e-bad").join("SKILL.md"),
            "---\nname: dsh-e2e-bad\n---\n\nBody of bad skill.\n",
        )
        .unwrap();

        // 探针脚本：只依赖实例自装的 skill-filesystem 与 node 标准库。
        let probe = std::env::temp_dir().join(format!("dsh-skill-e2e-probe-{}.mjs", std::process::id()));
        std::fs::write(&probe, PROBE_SCRIPT).unwrap();

        let out = std::process::Command::new(&node_exe)
            .arg(&probe)
            .arg(&home)
            .arg(&sf_lib)
            .env("NO_COLOR", "1")
            .creation_flags_cv()
            .output()
            .expect("运行 discovery 探针失败");
        use crate::node_runtime::CreationFlagsExt;
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        println!("探针输出：\n{text}");
        let _ = std::fs::remove_file(&probe);

        assert!(
            out.status.success() && text.contains("VERDICT: PASS"),
            "DSH discovery 未认可实例技能（好坏对照失败）"
        );
        println!("会话级认可实证通过：user-dsh 根被扫描（坏样本有警告）+ 好样本被接受");
    }

    /// 失败路径也清场的 Drop 守卫：删掉两个 e2e 技能目录。
    struct SkillDirsCleanup(PathBuf);
    impl Drop for SkillDirsCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.join("dsh-e2e-good"));
            let _ = std::fs::remove_dir_all(self.0.join("dsh-e2e-bad"));
        }
    }

    /// discovery 探针（node ESM）：好坏对照断言在脚本内完成，PASS 才退出 0。
    /// 返回结构兼容数组形态（0.1.1-rc.2）与 {skills: [...]} 形态（新版）。
    const PROBE_SCRIPT: &str = r#"
import { pathToFileURL } from 'node:url';
const [instanceDir, sfLib] = process.argv.slice(2);
const mod = await import(pathToFileURL(sfLib).href);
const warnings = [];
const fakeLogger = new Proxy({}, {
  get: (_t, prop) => (prop === 'warn') ? (m) => warnings.push(String(m)) : () => {},
});
const ctx = { logger: fakeLogger, get: () => undefined };
const control = { signal: new AbortController().signal, invalidate: () => {} };
const provider = new mod.FileSystemSkillProvider(ctx, control, { dshHome: instanceDir });
const result = await provider.list({ cwd: 'D:/nonexistent-workspace' });
const skills = Array.isArray(result) ? result : [...(result.skills ?? [])];
console.log('discovered =', skills.map((s) => s.name + ' [source=' + s.source + ']'));
console.log('warnings =', warnings);
const good = skills.find((s) => s.name === 'dsh-e2e-good' && s.source === 'user-dsh');
const badWarned = warnings.some((w) => w.includes('ignored') && w.includes('dsh-e2e-bad'));
const goodWarned = warnings.some((w) => w.includes('ignored') && w.includes('dsh-e2e-good'));
if (!good) { console.log('VERDICT: FAIL - user-dsh 根未发现 dsh-e2e-good'); process.exit(1); }
if (!badWarned) { console.log('VERDICT: FAIL - 坏技能未被拒绝（根可能未被扫描）'); process.exit(1); }
if (goodWarned) { console.log('VERDICT: FAIL - 好技能被拒绝'); process.exit(1); }
console.log('VERDICT: PASS - DSH discovery 从 <实例>/skills 接受 dsh-e2e-good，坏技能被拒');
// watcher 句柄会让 node 挂着不退：断言完必须显式退出。
process.exit(0);
"#;
}
