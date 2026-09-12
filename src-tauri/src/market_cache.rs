//! 市场缓存（`<managed root>/market-cache.json`）：一层文件承载两件事——
//! ① GitHub API 的 ETag 条件请求缓存（URL → etag + 响应体），是匿名配额
//! 卫生的根基：命中 304 不消耗配额意义上的新鲜拉取；② 最近一次成功的
//! 聚合卡片快照，是断网降级「离线快照」的根基。
//!
//! 文件缺失/损坏一律按空缓存处理——缓存永远不能变成启动器的负担。
//! 读-改-写经进程内互斥串行（市场与 GitHub 客户端并发触达）。

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::paths;

/// 缓存结构版本；不兼容时整体丢弃重建。
const MARKET_CACHE_VERSION: u32 = 1;

/// ETag 条目容量上限（P3-05：表只增不减会随搜索词无限膨胀）。超出时按
/// fetched_at 淘汰最旧——304 命中依赖的是「最近用过的端点」，容量取
/// 覆盖常用搜索面的量级绰绰有余。
pub const ETAG_CAP: usize = 256;

/// 一个 URL 的 ETag 条件缓存条目。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EtagEntry {
    pub etag: String,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: String,
    pub body: serde_json::Value,
}

/// 最近一次成功聚合的卡片快照。
///
/// `checksum`（P3-05）= 内容指纹（sha256，覆盖 fetched_at/community/cards），
/// 写入时计算、回放前校验——market-cache.json 被意外损坏或手工篡改时快照
/// 判无效并整体重建，而不是把脏卡片喂给下游 UI。边界：校验和与内容同文件
/// 存放，防的是损坏与非对抗性篡改；真正的攻击者可连校验和一起改写，那类
/// 对抗由镜像源信任与网关通道治理承担。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeedSnapshot {
    #[serde(rename = "fetchedAt")]
    pub fetched_at: String,
    /// 快照所属的社区资源通道；通道切换后旧快照不作为离线降级数据源。
    #[serde(rename = "communityActive")]
    pub community_active: String,
    /// 序列化的 MarketCard 列表（按 market.rs 当前结构）。
    pub cards: Vec<serde_json::Value>,
    /// 内容指纹（旧缓存文件无此字段 → 回放判无效，联网后重建）。
    #[serde(default)]
    pub checksum: String,
}

/// 快照内容指纹：sha256 hex，覆盖 fetched_at + community_active + cards 的
/// 确定性 JSON 序列化（serde_json preserve_order 保证同构造同字节）。
pub fn snapshot_checksum(cards: &[serde_json::Value], fetched_at: &str, community: &str) -> String {
    let payload = serde_json::json!({
        "fetchedAt": fetched_at,
        "communityActive": community,
        "cards": cards,
    });
    let bytes = serde_json::to_vec(&payload).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    format!("{:x}", hasher.finalize())
}

/// 回放前的完整性判定：校验和非空且与内容一致。
pub fn snapshot_intact(snapshot: &FeedSnapshot) -> bool {
    !snapshot.checksum.is_empty()
        && snapshot.checksum == snapshot_checksum(&snapshot.cards, &snapshot.fetched_at, &snapshot.community_active)
}

/// 快照构造助手：统一在此计算校验和，写侧不可能漏。
pub fn make_snapshot(fetched_at: String, community_active: String, cards: Vec<serde_json::Value>) -> FeedSnapshot {
    let checksum = snapshot_checksum(&cards, &fetched_at, &community_active);
    FeedSnapshot { fetched_at, community_active, cards, checksum }
}

/// ETag 表容量修剪：超上限时按 fetched_at 升序淘汰最旧（时间串同格式下
/// 字典序即时间序；并列按 key 保持确定性）。
fn evict_etags(cache: &mut MarketCache) {
    if cache.etags.len() <= ETAG_CAP {
        return;
    }
    let mut by_time: Vec<(String, String)> = cache
        .etags
        .iter()
        .map(|(k, v)| (k.clone(), v.fetched_at.clone()))
        .collect();
    by_time.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let drop_n = cache.etags.len() - ETAG_CAP;
    for (key, _) in by_time.into_iter().take(drop_n) {
        cache.etags.remove(&key);
    }
}

/// 市场缓存持久化根结构。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarketCache {
    pub version: u32,
    #[serde(default, rename = "etags")]
    pub etags: BTreeMap<String, EtagEntry>,
    #[serde(default)]
    pub feed: Option<FeedSnapshot>,
    /// Skill 中心的独立快照（skills.rs 消费；与插件市场同文件分层存放）。
    #[serde(default)]
    pub skills: Option<FeedSnapshot>,
    /// 整合包栏的独立快照（聚合资源目录；P3 浏览层）。
    #[serde(default)]
    pub modpacks: Option<FeedSnapshot>,
}

static CACHE_LOCK: Mutex<()> = Mutex::new(());

/// 读缓存文件；缺失或损坏返回默认空缓存（损坏时顺手删掉坏文件）。
pub fn load() -> Result<MarketCache, String> {
    let path = paths::market_cache_file()?;
    let text = match paths::read_text(&path) {
        Ok(text) => text,
        Err(_) if !path.exists() => return Ok(MarketCache::default()),
        Err(e) => return Err(format!("读取市场缓存失败：{e}")),
    };
    let cache: MarketCache = match serde_json::from_str(&text) {
        Ok(cache) => cache,
        Err(_) => {
            std::fs::remove_file(&path).ok();
            return Ok(MarketCache::default());
        }
    };
    if cache.version != MARKET_CACHE_VERSION {
        return Ok(MarketCache::default());
    }
    Ok(cache)
}

/// 在互斥内读-改-写缓存；闭包拿到 `&mut MarketCache`，返回值透传。
pub fn update<T>(mutate: impl FnOnce(&mut MarketCache) -> T) -> Result<T, String> {
    // 锁毒容错与全仓口径一致（instances/keyvault 同款），中毒不至永久 panic。
    let _guard = CACHE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut cache = load()?;
    let out = mutate(&mut cache);
    evict_etags(&mut cache);
    cache.version = MARKET_CACHE_VERSION;
    let path = paths::market_cache_file()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建缓存目录失败：{e}"))?;
    }
    let text = serde_json::to_string(&cache).map_err(|e| format!("序列化市场缓存失败：{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写入市场缓存失败：{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换市场缓存失败：{e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instances;

    fn sandbox(tag: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        instances::tests::sandbox(&format!("market-cache-{tag}"))
    }

    #[test]
    fn missing_file_loads_default_and_update_persists() {
        let (_lock, tmp) = sandbox("roundtrip");
        assert!(!paths::market_cache_file().unwrap().exists());
        let fresh = load().unwrap();
        assert!(fresh.etags.is_empty());
        assert!(fresh.feed.is_none());

        update(|cache| {
            cache.etags.insert(
                "https://api.github.com/x".into(),
                EtagEntry {
                    etag: "W/\"abc\"".into(),
                    fetched_at: "2026-09-01T00:00:00Z".into(),
                    body: serde_json::json!({ "items": [] }),
                },
            );
        })
        .unwrap();

        let reloaded = load().unwrap();
        let entry = reloaded.etags.get("https://api.github.com/x").unwrap();
        assert_eq!(entry.etag, "W/\"abc\"");
        assert!(paths::market_cache_file().unwrap().exists());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn corrupt_file_is_dropped_not_fatal() {
        let (_lock, tmp) = sandbox("corrupt");
        let path = paths::market_cache_file().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        let cache = load().unwrap();
        assert!(cache.etags.is_empty());
        assert!(!path.exists(), "损坏文件应被清除");
        // 版本不匹配同样整体重建。
        std::fs::write(&path, r#"{ "version": 99 }"#).unwrap();
        assert!(load().unwrap().etags.is_empty());
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// P3-05：ETag 表容量上限——只增不减的表会随搜索词无限膨胀，超限淘汰最旧。
    #[test]
    fn etag_table_is_capped_with_oldest_evicted() {
        let (_lock, tmp) = sandbox("etag-cap");
        for i in 0..(ETAG_CAP as u32 + 8) {
            let i = i.to_string();
            update(|cache| {
                cache.etags.insert(
                    format!("https://api.github.com/x?n={i}"),
                    EtagEntry {
                        etag: format!("W/\"{i}\""),
                        fetched_at: format!("2026-09-01T00:{:02}:{:02}Z", i.len(), i.parse::<u32>().unwrap() % 60),
                        body: serde_json::json!({ "n": i }),
                    },
                );
            })
            .unwrap();
        }
        let cache = load().unwrap();
        assert_eq!(cache.etags.len(), ETAG_CAP, "超限后必须剪到上限");
        // 最旧的（编号 0~7）被淘汰；最新的全部还在。
        assert!(!cache.etags.contains_key("https://api.github.com/x?n=0"));
        assert!(cache.etags.contains_key(&format!("https://api.github.com/x?n={}", ETAG_CAP + 7)));
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// P3-05：快照校验和——写入即计算，回放可验，篡改可察。
    #[test]
    fn snapshot_checksum_detects_tampering() {
        let cards = vec![serde_json::json!({ "name": "a", "monthly": 1 })];
        let snapshot = make_snapshot("2026-09-05T00:00:00Z".into(), "official".into(), cards.clone());
        assert!(snapshot_intact(&snapshot), "原样快照必须判完整");

        let mut tampered = snapshot.clone();
        tampered.cards[0]["monthly"] = serde_json::json!(99999);
        assert!(!snapshot_intact(&tampered), "卡片被改必须判无效");

        let mut relabeled = snapshot.clone();
        relabeled.community_active = "gitee".into();
        assert!(!snapshot_intact(&relabeled), "通道被改必须判无效");

        let mut legacy = snapshot.clone();
        legacy.checksum = String::new();
        assert!(!snapshot_intact(&legacy), "旧格式（无校验和）回放判无效 → 联网重建");
    }
}
