//! 热键绑定持久化（FILE-LAYOUT：`<托管根>/hotkeys.json`，随数据目录走）。
//!
//! 2026-09-12 起取代前端 localStorage：localStorage 绑在 WebView 配置上，
//! 数据目录整体打包/迁移时带不走。前端仍是唯一编辑者（services/hotkeys.ts
//! 经 `hotkeys_get`/`hotkeys_set` 读写），本模块只管文件的原子落盘与容错读。

use std::collections::BTreeMap;

/// 读覆盖表；文件缺失视为空表（= 全部回目录默认键）。
pub fn load() -> Result<BTreeMap<String, String>, String> {
    let path = crate::paths::hotkeys_file()?;
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let raw = crate::paths::read_text(&path)?;
    serde_json::from_str(&raw).map_err(|e| format!("解析 {} 失败：{e}", path.display()))
}

/// 原子写覆盖表（临时文件 + 改名）。BTreeMap 保证键序稳定，产物可 diff。
pub fn save(overrides: &BTreeMap<String, String>) -> Result<(), String> {
    let path = crate::paths::hotkeys_file()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建 {} 失败：{e}", parent.display()))?;
    }
    let body =
        serde_json::to_string_pretty(overrides).map_err(|e| format!("序列化失败：{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body + "\n").map_err(|e| format!("写入 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 {} 失败：{e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{load, save};
    use crate::instances;
    use std::collections::BTreeMap;

    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        instances::tests::sandbox(&format!("hotkeys-{tag}"))
    }

    #[test]
    fn missing_file_is_empty_and_save_roundtrips() {
        let (_lock, _tmp) = sandbox("roundtrip");
        assert!(load().unwrap().is_empty(), "缺失文件 = 空覆盖表");

        let mut overrides = BTreeMap::new();
        overrides.insert("nav-launch".to_string(), "Ctrl+Alt+L".to_string());
        overrides.insert("nav-more".to_string(), "Alt+4".to_string());
        save(&overrides).unwrap();

        let readback = load().unwrap();
        assert_eq!(readback.get("nav-launch").map(String::as_str), Some("Ctrl+Alt+L"));
        assert_eq!(readback.len(), 2);
    }

    #[test]
    fn bom_tolerated_and_overwrite_replaces_whole_table() {
        let (_lock, _tmp) = sandbox("bom");
        let path = crate::paths::hotkeys_file().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "\u{feff}{\"nav-launch\":\"Alt+9\"}").unwrap();
        assert_eq!(load().unwrap().get("nav-launch").map(String::as_str), Some("Alt+9"));

        // 整体替换语义：新表不含旧键，旧键即消失。
        save(&BTreeMap::new()).unwrap();
        assert!(load().unwrap().is_empty());
    }
}
