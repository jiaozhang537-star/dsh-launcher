//! 个性化（FR-27，P7.5）：背景图片与标题栏图片的文件侧。
//!
//! PCL 口径（PageSetupUI.xaml:104-157 / 186-219，2026-10-24 需求方指令按 PCL
//! 源码理解需求）：背景不是「选一张图」而是 **Pictures/ 文件夹 + 每次启动随机
//! 加载其中一张**；标题栏图片 = 单文件存托管根。图片以 data URL 交给前端
//! （CSP `img-src data:` 已放行；WebView 无法直接读任意路径）。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 交给前端的一张图片（name 仅供展示）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalizeImage {
    pub name: String,
    pub data_url: String,
}

/// 背景图片文件夹（PCL 同款：用户自行把图放进去，启动器每次随机一张）。
fn background_dir() -> Result<PathBuf, String> {
    Ok(crate::paths::managed_root()?.join("Pictures"))
}

/// 支持的图片扩展（展示用途的常见集；WebView 能渲染的都收）。
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"];

fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_image(path: &Path) -> bool {
    IMAGE_EXTS.contains(&ext_of(path).as_str())
}

fn mime_of(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

fn read_as_data_url(path: &Path) -> Result<PersonalizeImage, String> {
    let ext = ext_of(path);
    let bytes = std::fs::read(path).map_err(|e| format!("读取图片 {} 失败：{e}", path.display()))?;
    let encoded = crate::keyvault::base64_encode(&bytes);
    Ok(PersonalizeImage {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        data_url: format!("data:{};base64,{}", mime_of(&ext), encoded),
    })
}

/// 打开背景文件夹（不存在则创建——PCL 同款引导：把图片放进去即可）。
pub fn background_open(app: &tauri::AppHandle) -> Result<(), String> {
    let dir = background_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建背景文件夹失败：{e}"))?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| format!("打开背景文件夹失败：{e}"))
}

/// 随机挑一张背景图（PCL：每次打开启动器随机加载一张）。空文件夹 = None。
pub fn background_random() -> Result<Option<PersonalizeImage>, String> {
    let dir = background_dir()?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(None);
    };
    let images: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_image(p))
        .collect();
    match images.len() {
        0 => Ok(None),
        n => {
            // 无外部 RNG 依赖：以系统纳秒时钟取模——「随机换一张」的体验足够，
            // 不需要密码学均匀性。
            let pick = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as usize % n)
                .unwrap_or(0);
            read_as_data_url(&images[pick]).map(Some)
        }
    }
}

/// 清空背景文件夹里的图片（PCL「清空背景图片」）。返回清掉的张数。
pub fn background_clear() -> Result<usize, String> {
    let dir = background_dir()?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(0);
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && is_image(&path) {
            std::fs::remove_file(&path)
                .map_err(|e| format!("删除 {} 失败：{e}", path.display()))?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// 标题栏图片的落点前缀（`<托管根>/logo.<ext>`，同前缀先清后写保证唯一）。
fn logo_prefix() -> Result<PathBuf, String> {
    Ok(crate::paths::managed_root()?.join("logo"))
}

fn existing_logo() -> Result<Option<PathBuf>, String> {
    let prefix = logo_prefix()?;
    for ext in IMAGE_EXTS {
        let candidate = prefix.with_extension(ext);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// 当前标题栏图片；未设置 = None。
pub fn logo_get() -> Result<Option<PersonalizeImage>, String> {
    match existing_logo()? {
        Some(path) => read_as_data_url(&path).map(Some),
        None => Ok(None),
    }
}

/// 从用户选择的文件设置标题栏图片（复制进托管根，替换旧图）。
pub fn logo_set_from(src: &str) -> Result<PersonalizeImage, String> {
    let src_path = Path::new(src);
    if !src_path.is_file() {
        return Err(format!("找不到图片文件：{src}"));
    }
    let ext = ext_of(src_path);
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return Err(format!("不支持的图片格式「.{ext}」——请用 png / jpg / webp / gif / bmp"));
    }
    let dest = logo_prefix()?.with_extension(&ext);
    logo_clear()?;
    std::fs::copy(src_path, &dest)
        .map_err(|e| format!("保存标题栏图片失败：{e}"))
        .and_then(|_| read_as_data_url(&dest))
}

/// 清除标题栏图片；返回是否原本存在。
pub fn logo_clear() -> Result<bool, String> {
    match existing_logo()? {
        Some(path) => {
            std::fs::remove_file(&path).map_err(|e| format!("删除标题栏图片失败：{e}"))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 背景随机/清空/扩展名过滤与标题栏设置/替换/清除的全链（沙箱托管根）。
    #[test]
    fn background_and_logo_roundtrip() {
        let (_sandbox, tmp) = crate::instances::tests::sandbox("personalize");
        let pics = background_dir().unwrap();
        std::fs::create_dir_all(&pics).unwrap();

        // 空文件夹：random = None；clear 幂等 0。
        assert!(background_random().unwrap().is_none());
        assert_eq!(background_clear().unwrap(), 0);

        // 放两张图 + 一个非图片文件：random 命中图片之一；非图片不参与。
        std::fs::write(pics.join("a.png"), b"fake-png-bytes").unwrap();
        std::fs::write(pics.join("b.jpg"), b"fake-jpg-bytes").unwrap();
        std::fs::write(pics.join("note.txt"), b"not an image").unwrap();
        let picked = background_random().unwrap().expect("应挑到一张");
        assert!(["a.png", "b.jpg"].contains(&picked.name.as_str()));
        assert!(picked.data_url.starts_with("data:image/"));
        // 随机兜底（多次取，至少命中有效集合）。
        for _ in 0..8 {
            let p = background_random().unwrap().unwrap();
            assert!(["a.png", "b.jpg"].contains(&p.name.as_str()));
        }
        assert_eq!(background_clear().unwrap(), 2, "只清图片，txt 保留");
        assert!(pics.join("note.txt").exists());

        // 标题栏：未设置 → None；设图 → 可读回；换格式替换旧文件；清空。
        assert!(logo_get().unwrap().is_none());
        let src = tmp.join("brand.webp");
        std::fs::write(&src, b"fake-webp").unwrap();
        let saved = logo_set_from(src.to_str().unwrap()).unwrap();
        assert_eq!(saved.name, "logo.webp");
        assert!(saved.data_url.starts_with("data:image/webp;base64,"));
        assert!(logo_get().unwrap().is_some());
        let src2 = tmp.join("brand2.png");
        std::fs::write(&src2, b"fake-png").unwrap();
        logo_set_from(src2.to_str().unwrap()).unwrap();
        assert!(logo_prefix().unwrap().with_extension("webp").exists() == false, "旧格式文件应被替换清除");
        assert_eq!(logo_get().unwrap().unwrap().name, "logo.png");
        assert!(logo_clear().unwrap());
        assert!(logo_get().unwrap().is_none());
        assert!(!logo_clear().unwrap(), "幂等");

        // 非图片格式拒绝。
        let bad = tmp.join("evil.exe");
        std::fs::write(&bad, b"MZ").unwrap();
        assert!(logo_set_from(bad.to_str().unwrap()).is_err());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
