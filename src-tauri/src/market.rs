//! 插件市场聚合层（FR-03，T-4 口径定稿）：
//! - 聚合主干 = npm `keywords:dsh-plugin` 搜索（可安装宇宙：自带版本/发布
//!   时间/仓库链接），GitHub `topic:dsh-plugin` 聚合并行拉取、以仓库归一去重
//!   补星标与头像；仅 GitHub 存在（未发 npm）的仓库列 git 直装形态，下载人数
//!   降级隐藏。话题成员嘈杂（13062 成员混入无关热门仓库），故 npm 关键词
//!   为主、GitHub 为辅。
//! - 下载人数（T-4）：插件级 = npm 月下载（`downloads/point/last-month`，
//!   批量端点）；每版本 = 周下载（`versions/<pkg>/last-week`，仅周粒度）。
//! - 热度序 = 月下载为主、星标加权；最新序 = 发布/push 时间。
//! - 社区镜像源 = dshhub 时以 dsh-hub.cc 目录为聚合数据源备选（镜像只改
//!   取用通道，两种通道产出同一 MarketCard 模型）；gitee 通道只作用于下载
//!   URL 改写（安装侧消费），不影响本层元数据。
//! - 排序/筛选/类型聚类是纯函数（cargo 可测）；断网回退最近快照并标注
//!   「离线快照」，无快照时报可读错误（含配额指引）。

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::gateway::{self, DSHHUB_OFFICIAL};
use crate::github;
use crate::instances::now_rfc3339;
use crate::market_cache;
use crate::mirror_config::{self, DSHHUB_ID};
use crate::registry_meta;

/// npm 侧聚合关键字（可安装宇宙的口径）。
pub const NPM_KEYWORD: &str = "dsh-plugin";
/// GitHub 侧聚合话题。
pub const GITHUB_TOPIC: &str = "dsh-plugin";
/// 本体仓库自身也打着 dsh-plugin 话题（"Everything is a Plugin"），但它是
/// 被启动的对象而非插件——聚合时显式排除。
pub const HARNESS_REPO: &str = "deepseek-ai/deepseek-harness";

/// query 串编码：空格→`%20`、冒号→`%3A`，保留 RFC3986 unreserved 字符。
/// ureq 不自动编码 query，GitHub/npm 的组合搜索（`词 topic:x`）必须手动来。
pub fn encode_query(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// npm 搜索文本：无关键词 = 关键字目录口径（T-4 主干）；有关键词 = 原文
/// 真搜（npm 把空格当 AND，天然支持「dsh web ui」式输入；不叠加 keywords
/// 过滤——真机上 `@linxin666/dsh-web-ui-all` 未声明 keywords，叠加即漏）。
pub fn npm_search_text(keyword: &str) -> String {
    let trimmed = keyword.trim();
    if trimmed.is_empty() {
        format!("keywords:{NPM_KEYWORD}")
    } else {
        trimmed.to_string()
    }
}

/// 关键词模式下 npm/GitHub 各只拉 1 页：服务端相关度排序会把目标带进前列，
/// 不必也不应烧配额拉满页数。
pub fn search_pages_for(keyword: &str) -> u32 {
    if keyword.trim().is_empty() { NPM_SEARCH_PAGES } else { 1 }
}

/// 归一化：trim + 小写。大小写无视（走查需求⑥）。
pub fn normalize_text(s: &str) -> String {
    s.trim().to_lowercase()
}

/// 压扁：小写并剔除分隔符（空格 / `-` / `_` / `/` / `.` / `@`）。
/// 「dsh web ui」「dsh-web-ui」「DshWebUI」「dshwebui」全部坍缩成
/// `dshwebui`——懒狗输入（走查需求③④）与大小写（⑥）的统一口径。
/// CJK 字符按字母数字保留（民间叫法的中文子串匹配依赖它）。
pub fn squash_text(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// 查询 token 切分：按空白与常见分隔符切；空 token 丢弃。
fn query_tokens(keyword: &str) -> Vec<String> {
    normalize_text(keyword)
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '/' | '.' | '@'))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// 词法相关度打分（纯函数，走查需求①~⑥的统一裁决）。
///
/// 信号层级（高分在前，低层永远压不过高层）：
/// - 1000 归一化全等（复制粘贴全名，含大小写无视）；
/// - 900 压扁全等（懒狗：无空格/无标点/大小写任意）；
/// - 800 名称前缀 / 750 压扁前缀；
/// - 700 名称包含 / 600 压扁包含（≥3 字符，防短串误报）；
/// - 0~300 token 覆盖率：token 落在 名称+副名+描述+标签 即计，名称内命中
///   每个 +50（简名「web ui」、民间叫法走此层；中文按子串匹配有效）。
/// 返回 0 = 毫无关系（调用方丢弃，降噪）。
pub fn relevance_score(card: &MarketCard, keyword: &str) -> i64 {
    let q = normalize_text(keyword);
    if q.is_empty() {
        return 0;
    }
    let qs = squash_text(keyword);
    let name = normalize_text(&card.name);
    // scoped 包（@scope/name）的裸名末段：用户可能粘贴全名也可能粘贴裸名，
    // 两者都视为「全名输入」。
    let bare = normalize_text(card.name.rsplit('/').next().unwrap_or(&card.name));
    // 作者名：repo 的 owner 段，或 scoped 包名的 @scope 段（走查补需求⑥：
    // 按作者名检索——全等命中 850 层，其插件按下载量排前面）。
    let owner_raw: String = card
        .repo
        .as_deref()
        .and_then(|r| r.split('/').next())
        .map(str::to_string)
        .or_else(|| {
            card.name
                .strip_prefix('@')
                .and_then(|rest| rest.split('/').next())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let owner = normalize_text(&owner_raw);
    let name_sq = squash_text(&card.name);
    let bare_sq = squash_text(&bare);
    let sub = normalize_text(&card.sub_name);
    let desc = normalize_text(&card.description);
    let tags = normalize_text(&card.tags.join(" "));
    let haystack = format!("{name} {sub} {desc} {tags}");

    if name == q || bare == q {
        return 1000;
    }
    if !qs.is_empty() && (name_sq == qs || bare_sq == qs) {
        return 900;
    }
    if !owner.is_empty() && owner == q {
        return 850;
    }
    if name.starts_with(&q) || bare.starts_with(&q) {
        return 800;
    }
    if qs.len() >= 2 && (name_sq.starts_with(&qs) || bare_sq.starts_with(&qs)) {
        return 750;
    }
    if name.contains(&q) {
        return 700;
    }
    // 压扁包含只看身份字段（名称+副名）：压扁会把相邻词熔接，放到描述/
    // 标签上会制造幻影子串（真机测试实证：标签含 web-ui 的无关卡借道平齐）。
    if qs.len() >= 3 && squash_text(&format!("{name} {sub}")).contains(&qs) {
        return 600;
    }
    let tokens = query_tokens(keyword);
    if tokens.is_empty() {
        return 0;
    }
    let hits = tokens.iter().filter(|t| haystack.contains(t.as_str())).count();
    if hits == 0 {
        return 0;
    }
    let name_hits = tokens.iter().filter(|t| name.contains(t.as_str())).count();
    ((hits * 100) / tokens.len()) as i64 + (name_hits * 50) as i64
}

/// npm 搜索页大小（registry 上限 250，取 100×2 页 ≈ 覆盖主要生态）。
const NPM_SEARCH_PAGE_SIZE: u32 = 100;
/// npm/GitHub 无关键词目录模式的聚合页数。
const NPM_SEARCH_PAGES: u32 = 2;

/// 类型聚类的规则表（首个命中生效；供「类型」筛选下拉与卡片归类）。
pub const CATEGORIES: &[&str] = &["界面", "搜索", "机器人", "记忆", "视觉", "开发", "效率", "其他"];

/// 参与类型聚类但不作为标签展示的噪声关键字。
const NOISE_KEYWORDS: &[&str] = &[
    "dsh", "dsh-plugin", "deepseek", "deepseek-harness", "cordis", "ai", "agent", "plugin",
    "claude", "claude-code", "codex", "mcp",
];

/// 类型规则表：(聚类名, 命中词)。
const CATEGORY_RULES: &[(&str, &[&str])] = &[
    ("界面", &["ui", "skin", "theme", "sidebar", "desktop", "web", "tui", "界面", "皮肤", "主题", "侧边栏", "桌面"]),
    ("搜索", &["search", "搜索", "research"]),
    ("机器人", &["bot", "im", "feishu", "lark", "slack", "telegram", "qq", "wechat", "dingtalk", "微信", "飞书", "钉钉", "机器人"]),
    ("记忆", &["memory", "knowledge", "rag", "记忆", "知识"]),
    ("视觉", &["vision", "image", "ocr", "screenshot", "视觉", "图片", "看图"]),
    ("开发", &["dev", "workflow", "test", "lint", "git", "开发", "调试", "审查"]),
    ("效率", &["tool", "productivity", "效率", "工具", "翻译", "summary"]),
];

/// 市场卡片：前端渲染一行的全部字段（IPC 键 camelCase）。需要 Deserialize：
/// 离线快照从缓存 JSON 回放为卡片。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketCard {
    pub name: String,
    /// 副名：英文/仓库全名（PCL 截图中 `钠 | Sodium` 的后半）。
    pub sub_name: String,
    pub description: String,
    /// 方形图标 URL（GitHub 头像）；空串 = 前端绘制字母块。
    pub icon_url: String,
    /// 标签徽章（≤4 个，噪声关键字已剔除）。
    pub tags: Vec<String>,
    /// npm 月下载（T-4 口径「下载人数」）；None = 降级隐藏（git 直装/dshhub）。
    pub monthly_downloads: Option<u64>,
    /// npm 最新版本号（元信息行展示）；git 直装/dshhub 形态为 None。
    pub latest_version: Option<String>,
    pub stars: Option<u64>,
    /// 更新时间（npm 发布时间或 GitHub push，RFC3339；空串 = 未知）。
    pub updated_at: String,
    /// 聚合渠道：npm / github / dshhub（「来源」筛选取值域）。
    pub source: String,
    /// npm 包名（npm 渠道必有；git 直装形态为 None）。
    pub npm_name: Option<String>,
    /// `owner/repo`（能归一到 GitHub 仓库时存在）。
    pub repo: Option<String>,
    /// 类型聚类（类型筛选取值域）。
    pub category: String,
    /// dsh-hub 的 verified 标记；非 dshhub 渠道为 None。
    pub verified: Option<bool>,
}

/// 一次信息流查询的参数（全部可选，缺省 = 推荐流）。
#[derive(Debug, Clone)]
pub struct MarketQuery {
    pub sort: String,
    pub keyword: String,
    pub source: String,
    pub kind: String,
}

/// 通道召回计数（空态可自证：0 结果时能说出各通道召回了多少）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recall {
    /// npm 通道卡片数（含目录快照页的 npm 卡）。
    pub npm: u32,
    /// GitHub 通道卡片数（git 直装形态）。
    pub github: u32,
}

/// 信息流返回：卡片列表 + 离线快照标注。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketFeed {
    pub cards: Vec<MarketCard>,
    /// true = 本次来自本地离线快照（断网/配额耗尽降级），前端显著标注。
    pub offline: bool,
    /// 数据截至时间（快照生成时间；离线时尤显重要）。
    pub fetched_at: String,
    pub recall: Recall,
}

/// 参数合法性（fail loud；前端下拉只会传合法值，防 IPC 层注入任意串）。
pub fn validate_query(query: &MarketQuery) -> Result<(), String> {
    if !matches!(query.sort.as_str(), "hot" | "new") {
        return Err(format!("非法排序：{query:?}"));
    }
    if !matches!(query.source.as_str(), "all" | "npm" | "github" | "dshhub") {
        return Err(format!("非法来源筛选：{query:?}"));
    }
    if query.kind != "all" && !CATEGORIES.contains(&query.kind.as_str()) {
        return Err(format!("非法类型筛选：{query:?}"));
    }
    Ok(())
}

/// 类型聚类：dshhub 自带归类优先，否则按关键词/描述命中规则表，未命中归「其他」。
pub fn derive_category(keywords: &[String], description: &str) -> String {
    let haystacks: Vec<String> = keywords
        .iter()
        .map(|k| k.to_lowercase())
        .chain(std::iter::once(description.to_lowercase()))
        .collect();
    for (category, words) in CATEGORY_RULES {
        for word in *words {
            let word = word.to_lowercase();
            if haystacks.iter().any(|h| h.contains(&word)) {
                return (*category).to_string();
            }
        }
    }
    "其他".to_string()
}

/// 是否通过筛选（纯函数：关键词子串 + 来源 + 类型）。
pub fn matches(query: &MarketQuery, card: &MarketCard) -> bool {
    if !query.keyword.trim().is_empty() {
        let needle = query.keyword.trim().to_lowercase();
        let hit = card.name.to_lowercase().contains(&needle)
            || card.sub_name.to_lowercase().contains(&needle)
            || card.description.to_lowercase().contains(&needle);
        if !hit {
            return false;
        }
    }
    if query.source != "all" && card.source != query.source {
        return false;
    }
    if query.kind != "all" && card.category != query.kind {
        return false;
    }
    true
}

/// 热度键（phase-03 观察项裁决落地）：npm 月下载为主；git-only 卡（无月
/// 下载）以 星标×50 折算下载代理——原先 stars 只作并列裁决，纯 git 卡在
/// 热度序恒沉底（zhu1090093659/dsh-web 6683 星实证被埋葬）。50 为记录在案
/// 的折算提案值，需求方可调。
const STARS_DOWNLOAD_FOLD: u64 = 50;

fn hot_key(c: &MarketCard) -> u64 {
    c.monthly_downloads
        .unwrap_or_else(|| c.stars.unwrap_or(0).saturating_mul(STARS_DOWNLOAD_FOLD))
}

/// 就地排序（纯函数）。`keyword` 非空 = 相关度模式：`relevance_score` 降序
/// （「接近到相似」，同分按月下载、再按名称），`sort` 参数被忽略——排序与
/// 热度/最新 tab 解耦由前端负责。无关键词 = 原双排序。
pub fn sort_cards(cards: &mut Vec<MarketCard>, sort: &str, keyword: &str) {
    if !keyword.trim().is_empty() {
        let mut scored: Vec<(i64, &MarketCard)> =
            cards.iter().map(|c| (relevance_score(c, keyword), c)).collect();
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.monthly_downloads.unwrap_or(0).cmp(&a.1.monthly_downloads.unwrap_or(0)))
                .then_with(|| a.1.name.cmp(&b.1.name))
        });
        let reordered: Vec<MarketCard> = scored.into_iter().map(|(_, c)| c.clone()).collect();
        *cards = reordered;
        return;
    }
    match sort {
        "hot" => cards.sort_by(|a, b| {
            hot_key(b)
                .cmp(&hot_key(a))
                .then_with(|| b.stars.unwrap_or(0).cmp(&a.stars.unwrap_or(0)))
                .then_with(|| a.name.cmp(&b.name))
        }),
        // RFC3339 同格式下字典序即时间序；空串（未知）殿后。
        "new" => cards.sort_by(|a, b| {
            let au = (!a.updated_at.is_empty(), a.updated_at.as_str());
            let bu = (!b.updated_at.is_empty(), b.updated_at.as_str());
            bu.cmp(&au)
                .then_with(|| b.monthly_downloads.unwrap_or(0).cmp(&a.monthly_downloads.unwrap_or(0)))
                .then_with(|| a.name.cmp(&b.name))
        }),
        _ => {}
    }
}

/// 从 npm 仓库链接归一出 `owner/repo`；非 GitHub 或解析失败返回 None。
pub fn normalize_repo_url(url: &str) -> Option<String> {
    let cleaned = url
        .trim()
        .trim_start_matches("git+")
        .replacen("git://", "https://", 1);
    let rest = cleaned.strip_prefix("https://github.com/")?;
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.splitn(3, '/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

/// npm 搜索结果中的一条原始条目。
#[derive(Debug, Clone)]
pub struct NpmEntry {
    pub name: String,
    pub version: String,
    pub date: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub repo: Option<String>,
    /// 搜索响应自带的月下载（官方 registry 有此字段；None 走批量 API 兜底）。
    pub monthly_downloads: Option<u64>,
}

/// 解析 `/-/v1/search` 响应；字段缺失的条目跳过。
pub fn parse_npm_search(body: &serde_json::Value) -> Vec<NpmEntry> {
    let Some(objects) = body.get("objects").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    objects
        .iter()
        .filter_map(|obj| {
            let package = obj.get("package")?;
            let name = package.get("name")?.as_str()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            Some(NpmEntry {
                name,
                version: package
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                date: package
                    .get("date")
                    .and_then(|v| v.as_str())
                    .or_else(|| obj.get("updated").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string(),
                description: package
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                keywords: package
                    .get("keywords")
                    .and_then(|v| v.as_array())
                    .map(|list| {
                        list.iter()
                            .filter_map(|k| k.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                repo: package
                    .get("links")
                    .and_then(|l| l.get("repository"))
                    .and_then(|v| v.as_str())
                    .and_then(normalize_repo_url),
                monthly_downloads: obj
                    .get("downloads")
                    .and_then(|d| d.get("monthly"))
                    .and_then(|v| v.as_u64()),
            })
        })
        .collect()
}

/// 展示标签：剔除噪声关键字、小写、去重、至多 4 个。
pub fn display_tags(keywords: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    keywords
        .iter()
        .filter_map(|k| {
            let lowered = k.trim().to_lowercase();
            if lowered.is_empty() || NOISE_KEYWORDS.contains(&lowered.as_str()) {
                return None;
            }
            seen.insert(lowered.clone()).then_some(lowered)
        })
        .take(4)
        .collect()
}

/// 拉一页 npm 关键词搜索（经网关候选链：npmmirror 优先、官方兜底）。
/// npmmirror 的 search 端点实测会返回「200 + 空结果」（真机实证），因此
/// 空结果按通道失败处理、自动回落官方——优先≠独占的语义对元数据同样成立。
/// 拉一页 npm 关键词搜索（经网关候选链：npmmirror 优先、官方兜底）。
/// `keyword` 决定搜索文本（见 `npm_search_text`）。npmmirror 的 search 端点
/// 实测会返回「200 + 空结果」（真机实证），因此空结果按通道失败处理、自动
/// 回落官方——优先≠独占的语义对元数据同样成立。
fn npm_search_page(from: u32, keyword: &str) -> Result<serde_json::Value, String> {
    mirror_config::load()?;
    let agent = crate::gateway::base_agent()
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build();
    let text = encode_query(&npm_search_text(keyword));
    let mut attempts = Vec::new();
    for base in gateway::npm_registry_bases() {
        let url = format!(
            "{base}/-/v1/search?text={text}&size={NPM_SEARCH_PAGE_SIZE}&from={from}"
        );
        let result = agent
            .get(&url)
            .set("User-Agent", "dsh-launcher")
            .call()
            .map_err(|e| format!("拉取 {url} 失败：{e}"))
            .and_then(|response| {
                let mut text = String::new();
                response
                    .into_reader()
                    .take(16 * 1024 * 1024)
                    .read_to_string(&mut text)
                    .map_err(|e| format!("读取响应失败：{e}"))?;
                let body: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|e| format!("搜索响应不是合法 JSON：{e}"))?;
                let count = body
                    .get("objects")
                    .and_then(|v| v.as_array())
                    .map(|list| list.len())
                    .unwrap_or(0);
                if count == 0 {
                    return Err(format!("[{base}] 搜索返回空结果（镜像未收录该视图）"));
                }
                Ok(body)
            });
        match result {
            Ok(body) => return Ok(body),
            Err(e) => attempts.push(e),
        }
    }
    Err(attempts.join("\n"))
}

/// 解析 downloads API 响应为 (包名, 月下载) 对：单包形态 `{downloads}` 或
/// 批量形态 `{ "<name>": {downloads} }`；响应里没有的名字不产出。
pub fn parse_downloads_payload(body: &serde_json::Value, requested: &[String]) -> Vec<(String, u64)> {
    let Some(map) = body.as_object() else {
        return Vec::new();
    };
    if map.contains_key("downloads") {
        // 单包响应：名字取请求里的第一个（chunk 只有一个包时才会长这样）。
        let count = map.get("downloads").and_then(|v| v.as_u64());
        return match (requested.first(), count) {
            (Some(name), Some(n)) => vec![(name.clone(), n)],
            _ => Vec::new(),
        };
    }
    map.iter()
        .filter_map(|(name, entry)| {
            let n = entry.get("downloads").and_then(|v| v.as_u64())?;
            Some((name.clone(), n))
        })
        .collect()
}

/// 批量取 npm 月下载（api.npmjs.org 支持逗号分隔 ≤128 包）；失败静默降级
/// 为空表（下载人数字段隐藏，热度回落星标）。基址与取数归统一网关（审查
/// R1）：npmmirror 无下载量 API（探证 404），官方单通道、不进候选链。
fn npm_monthly_bulk(names: &[String]) -> HashMap<String, u64> {
    let settings = mirror_config::load().ok();
    let mut out = HashMap::new();
    for chunk in names.chunks(100) {
        let raw = format!(
            "{}/downloads/point/last-month/{}",
            gateway::NPM_DOWNLOADS_OFFICIAL,
            chunk.join(",")
        );
        let url = match &settings {
            Some(s) => gateway::resolve(&raw, s).unwrap_or(raw),
            None => raw,
        };
        let Ok(response) = crate::gateway::base_agent()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .get(&url)
            .set("User-Agent", "dsh-launcher")
            .call()
        else {
            continue;
        };
        let mut text = String::new();
        if response.into_reader().take(4 * 1024 * 1024).read_to_string(&mut text).is_err() {
            continue;
        }
        let Ok(body) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        for (name, count) in parse_downloads_payload(&body, chunk) {
            out.insert(name, count);
        }
    }
    out
}

/// 官方源信息流：npm 主干 + GitHub 聚合去重。npm 失败可仅 GitHub 撑场，
/// 反之亦然；两路全败才报错。`keyword` 非空 = 关键词模式：npm 服务端真搜
/// 1 页 + **目录快照 2 页合并**（真机探证：无分隔懒狗输入的服务端召回为 0，
/// 压扁匹配只能在本地快照上发生；npm 搜索无严格限流，GitHub 维持 1 页——
/// 匿名搜索限流 10/分钟）。
fn fetch_feed_official(keyword: &str) -> Result<Vec<MarketCard>, String> {
    let mut npm_errors = Vec::new();
    let mut npm_entries: Vec<NpmEntry> = Vec::new();
    let fetch_npm_page = |page: u32, kw: &str, entries: &mut Vec<NpmEntry>, errors: &mut Vec<String>| {
        match npm_search_page(page * NPM_SEARCH_PAGE_SIZE, kw) {
            Ok(body) => entries.extend(parse_npm_search(&body)),
            Err(e) => errors.push(e),
        }
    };
    if keyword.trim().is_empty() {
        for page in 0..NPM_SEARCH_PAGES {
            let before = npm_entries.len();
            fetch_npm_page(page, keyword, &mut npm_entries, &mut npm_errors);
            if npm_entries.len() == before {
                break; // 第 1 页失败时后续页大概率同因，停止消耗配额。
            }
        }
    } else {
        fetch_npm_page(0, keyword, &mut npm_entries, &mut npm_errors);
        for page in 0..NPM_SEARCH_PAGES {
            fetch_npm_page(page, "", &mut npm_entries, &mut npm_errors);
        }
    }

    let gh_pages = search_pages_for(keyword);
    let mut gh_errors = Vec::new();
    let mut gh_repos: Vec<github::RepoSummary> = Vec::new();
    for page in 1..=gh_pages {
        let q = if keyword.trim().is_empty() {
            format!("topic:{GITHUB_TOPIC}")
        } else {
            format!("{} topic:{GITHUB_TOPIC}", keyword.trim())
        };
        match github::search_repos_raw(&encode_query(&q), page) {
            Ok(repos) => gh_repos.extend(repos),
            Err(e) => {
                gh_errors.push(e);
                break;
            }
        }
    }
    // 作者维度补路（走查定稿）：GitHub 文本搜索**不按 owner 匹配**（探证
    // `zhu1090093659 topic:dsh-plugin` 召回 0），无分隔关键词追加
    // `user:<词> topic:x` 一路，命中按仓库去重合并；补路失败不致命。
    if !keyword.trim().is_empty() && !keyword.trim().contains(char::is_whitespace) {
        let author_q = format!("user:{} topic:{GITHUB_TOPIC}", keyword.trim());
        if let Ok(repos) = github::search_repos_raw(&encode_query(&author_q), 1) {
            for repo in repos {
                if !gh_repos.iter().any(|r| r.full_name.eq_ignore_ascii_case(&repo.full_name)) {
                    gh_repos.push(repo);
                }
            }
        }
    }

    if npm_entries.is_empty() && gh_repos.is_empty() {
        let mut all = npm_errors;
        all.extend(gh_errors);
        return Err(if all.is_empty() {
            "市场数据为空".into()
        } else {
            all.join("\n")
        });
    }

    // GitHub 侧按仓库归一索引。
    let mut gh_index: HashMap<String, &github::RepoSummary> = HashMap::new();
    for repo in &gh_repos {
        gh_index.entry(repo.full_name.clone()).or_insert(repo);
    }

    // 月下载：优先用搜索响应自带字段（官方 registry 恒带）；缺失者批量 API 兜底。
    let missing: Vec<String> = npm_entries
        .iter()
        .filter(|e| e.monthly_downloads.is_none())
        .map(|e| e.name.clone())
        .collect();
    let mut downloads = npm_monthly_bulk(&missing);
    for entry in &npm_entries {
        if let Some(n) = entry.monthly_downloads {
            downloads.insert(entry.name.clone(), n);
        }
    }

    let mut cards: Vec<MarketCard> = Vec::new();
    let mut seen_npm: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in &npm_entries {
        if !seen_npm.insert(entry.name.clone()) {
            continue;
        }
        let gh = entry.repo.as_deref().and_then(|repo| gh_index.get(repo));
        cards.push(MarketCard {
            name: entry.name.clone(),
            sub_name: entry
                .repo
                .clone()
                .or_else(|| Some(entry.name.clone()))
                .unwrap_or_default(),
            description: entry.description.clone(),
            icon_url: gh.map(|g| g.avatar_url.clone()).unwrap_or_default(),
            tags: display_tags(&entry.keywords),
            monthly_downloads: downloads.get(&entry.name).copied(),
            latest_version: (!entry.version.is_empty()).then_some(entry.version.clone()),
            stars: gh.map(|g| g.stars),
            updated_at: entry.date.clone(),
            source: "npm".into(),
            npm_name: Some(entry.name.clone()),
            repo: entry.repo.clone(),
            category: derive_category(&entry.keywords, &entry.description),
            verified: None,
        });
    }
    // 仅 GitHub 存在的仓库 → git 直装形态（下载人数降级隐藏）。
    for repo in &gh_repos {
        if repo.full_name.eq_ignore_ascii_case(HARNESS_REPO) {
            continue; // 本体不是插件（话题成员嘈杂的极端样本）。
        }
        let covered = cards.iter().any(|c| c.repo.as_deref() == Some(repo.full_name.as_str()));
        if covered {
            continue;
        }
        cards.push(MarketCard {
            name: repo.full_name.rsplit('/').next().unwrap_or(&repo.full_name).to_string(),
            sub_name: repo.full_name.clone(),
            description: repo.description.clone(),
            icon_url: repo.avatar_url.clone(),
            tags: display_tags(&repo.topics),
            monthly_downloads: None,
            latest_version: None,
            stars: Some(repo.stars),
            updated_at: repo.pushed_at.clone(),
            source: "github".into(),
            npm_name: None,
            repo: Some(repo.full_name.clone()),
            category: derive_category(&repo.topics, &repo.description),
            verified: None,
        });
    }
    Ok(cards)
}

/// dshhub 通道信息流：dsh-hub.cc 目录即数据（自带中文分类与 verified 标记）。
fn fetch_feed_dshhub() -> Result<Vec<MarketCard>, String> {
    mirror_config::load()?;
    let raw = format!("{DSHHUB_OFFICIAL}/api/v1/plugins?scope=all&limit=200");
    let settings = mirror_config::load()?;
    let url = gateway::resolve(&raw, &settings)?;
    let agent = crate::gateway::base_agent()
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build();
    let response = agent
        .get(&url)
        .set("User-Agent", "dsh-launcher")
        .call()
        .map_err(|e| format!("拉取 dsh-hub 目录失败：{e}"))?;
    let mut text = String::new();
    response
        .into_reader()
        .take(16 * 1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|e| format!("读取 dsh-hub 响应失败：{e}"))?;
    let body: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("dsh-hub 响应不是合法 JSON：{e}"))?;
    let Some(items) = body.get("items").and_then(|v| v.as_array()) else {
        return Err("dsh-hub 目录响应缺少 items".into());
    };
    let mut cards = Vec::new();
    for item in items {
        let Some(full_name) = item.get("fullName").and_then(|v| v.as_str()) else {
            continue;
        };
        let topics: Vec<String> = item
            .get("topics")
            .and_then(|v| v.as_array())
            .map(|list| list.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let hub_category = item
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let category = if CATEGORIES.contains(&hub_category.as_str()) {
            hub_category
        } else {
            derive_category(&topics, &description)
        };
        cards.push(MarketCard {
            name: item.get("name").and_then(|v| v.as_str()).unwrap_or(full_name).to_string(),
            sub_name: full_name.to_string(),
            description,
            icon_url: item.get("avatarUrl").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            tags: display_tags(&topics),
            monthly_downloads: None,
            latest_version: None,
            stars: item.get("stars").and_then(|v| v.as_u64()),
            updated_at: item.get("pushedAt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            source: "dshhub".into(),
            npm_name: None,
            repo: Some(full_name.to_string()),
            category,
            verified: Some(
                item.get("installability").and_then(|v| v.as_str()).unwrap_or("") == "verified",
            ),
        });
    }
    if cards.is_empty() {
        return Err("dsh-hub 目录为空".into());
    }
    Ok(cards)
}

/// 元插件/整合包排除（走查定稿）：全家桶聚合包、市场/商店类、精选清单
/// 不进插件市场——与启动器自身功能重复，或属 Phase 4 整合包范畴。
/// 两层过滤：精确名单（走查截图实锤 + 侦察数据）+ 描述启发；集中常量可审计。
const META_EXACT: &[&str] = &[
    "dshmarket",
    "dsh-market/dsh-market",
    "2bingling/dsh-market",
    "@dsh-suite/plugin-manager",
];

const META_DESC_KEYWORDS: &[&str] = &[
    "全家桶",
    "聚合插件",
    "聚合包",
    "插件市场",
    "插件商店",
    "一键安装全部",
    "plugin market",
    "plugin store",
];

/// 是否为不该出现在插件市场的元插件/整合包类条目。
pub fn is_meta_card(card: &MarketCard) -> bool {
    let name = normalize_text(&card.name);
    let repo = normalize_text(card.repo.as_deref().unwrap_or(""));
    // 本体家族（@deepseek-ai/*）：装机自带的官方组件，不是市场插件——
    // 走查实证 @deepseek-ai/dsh-web 系列挤占搜索前排。
    if name.starts_with("@deepseek-ai/") {
        return true;
    }
    if META_EXACT.iter().any(|e| name == *e || repo == *e) {
        return true;
    }
    // awesome-* 精选清单仓（话题聚合的常客噪声，纯目录不是插件）。
    let repo_tail = repo.rsplit('/').next().unwrap_or("");
    if name.starts_with("awesome-") || repo_tail.starts_with("awesome-") {
        return true;
    }
    let desc = normalize_text(&card.description);
    if META_DESC_KEYWORDS.iter().any(|k| desc.contains(k)) {
        return true;
    }
    false
}

/// 批量验证 npm 卡是否声明插件层（`dsh.bundle.patch`）：关键词搜索结果
/// 少（≤百级），并行拉 packument 验证——**没声明的一律从结果剔除**
/// （走查定稿：搜都搜出来了、点了才说禁装 = 耍用户）。网络失败的条目
/// fail-open 保留（展开时的红字警示仍兜底）。
fn verify_bundle_decls(cards: &mut Vec<MarketCard>) {
    let npm_names: Vec<String> = cards
        .iter()
        .filter(|c| c.source == "npm")
        .filter_map(|c| c.npm_name.clone())
        .collect();
    if npm_names.is_empty() || npm_names.len() > 120 {
        return; // 无需验证 / 目录流不做逐包验证（几百包成本不可控）。
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let verdicts: std::sync::Mutex<HashMap<String, bool>> = std::sync::Mutex::new(HashMap::new());
    std::thread::scope(|scope| {
        for _worker in 0..8 {
            scope.spawn(|| loop {
                let idx = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(name) = npm_names.get(idx) else { break };
                let declares = registry_meta::fetch_packument(name)
                    .map(|p| packument_declares_bundle(&p))
                    .unwrap_or(true); // 网络失败 fail-open：保留，展开时仍有警示
                verdicts
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(name.clone(), declares);
            });
        }
    });
    let verdicts = verdicts.into_inner().unwrap_or_else(|p| p.into_inner());
    cards.retain(|c| {
        let Some(npm_name) = c.npm_name.as_deref() else { return true };
        match verdicts.get(npm_name) {
            Some(&declares) => declares,
            None => true, // 未验证（超出清单/失败）→ 保留
        }
    });
}

/// 整合包栏（P3 浏览层，需求方定稿）：正向判定「聚合型资源」——一键装齐
/// 一组插件的全家桶/聚合包/套件。与 `is_meta_card` 是同一分类语义的两面：
/// 插件市场排除它们，整合包栏收编它们。市场类应用（dshmarket 等）两头都
/// 不算——它是浏览工具，不是聚合包。
pub fn is_modpack_card(card: &MarketCard) -> bool {
    let name = normalize_text(&card.name);
    let repo = normalize_text(card.repo.as_deref().unwrap_or(""));
    // 本体家族两头都不进；市场类应用与精选清单不是聚合包。
    // 注意：**不继承 is_meta_card 的描述启发排除**——「聚合插件/一键安装
    // 全部」在插件市场是排除理由，在整合包栏恰恰是收编理由（两面同语义）。
    if name.starts_with("@deepseek-ai/") {
        return false;
    }
    if META_EXACT.iter().any(|e| name == *e || repo == *e) {
        return false;
    }
    let repo_tail = repo.rsplit('/').next().unwrap_or("");
    if name.starts_with("awesome-") || repo_tail.starts_with("awesome-") {
        return false;
    }
    const KEYWORDS: &[&str] = &[
        "全家桶", "聚合插件", "聚合包", "整合包", "合集", "套件", "一键安装全部",
        "all-in-one", "aggregation", "aggregat", "collection",
    ];
    let hay = normalize_text(&format!(
        "{} {} {} {}",
        card.name,
        card.sub_name,
        card.description,
        card.tags.join(" ")
    ));
    if KEYWORDS.iter().any(|k| hay.contains(k)) {
        return true;
    }
    // 命名惯例：-all / -suite / -bundle 结尾的聚合包。
    name.ends_with("-all") || name.ends_with("-suite") || name.ends_with("-bundle")
}

/// 拉取信息流（阻塞网络调用——command 侧放 spawn_blocking）。
/// 新鲜拉取成功 → 快照落盘 + 按查询过滤排序返回；失败 → 同通道快照降级
/// （offline = true）→ 无快照才报原始错误（含配额指引）。
/// 整合包信息流（阻塞网络调用——command 侧放 spawn_blocking）：复用插件
/// 聚合管线，npm 源按聚合语义取多路文本（全家桶/聚合/整合包/合集/suite），
/// GitHub 侧尝试 topic:dsh-modpack（存在性未探明，fail-open）+ 原文/话题
/// 搜索，结果经 is_modpack_card 正向过滤；独立快照键 modpacks。
pub fn fetch_modpack_feed(query: &MarketQuery) -> Result<MarketFeed, String> {
    validate_query(query)?;
    mirror_config::load()?;
    let keyword = query.keyword.trim();
    let mut npm_errors: Vec<String> = Vec::new();
    let mut npm_entries: Vec<NpmEntry> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut fetch_page = |text: &str, entries: &mut Vec<NpmEntry>, errors: &mut Vec<String>| {
        match npm_search_page(0, text) {
            Ok(body) => {
                for entry in parse_npm_search(&body) {
                    if seen.insert(entry.name.clone()) {
                        entries.push(entry);
                    }
                }
            }
            Err(e) => errors.push(e),
        }
    };
    if keyword.is_empty() {
        for text in ["dsh 全家桶", "dsh 聚合", "dsh 整合包", "dsh 合集", "dsh suite"] {
            fetch_page(text, &mut npm_entries, &mut npm_errors);
        }
    } else {
        fetch_page(keyword, &mut npm_entries, &mut npm_errors);
    }

    let mut gh_repos: Vec<github::RepoSummary> = Vec::new();
    let topics: Vec<String> = if keyword.is_empty() {
        vec!["dsh-modpack".into(), "dsh-bundle".into()]
    } else {
        vec![format!("{} topic:{GITHUB_TOPIC}", keyword)]
    };
    for topic_or_q in &topics {
        let q = if keyword.is_empty() && topic_or_q.starts_with("dsh-") {
            format!("topic:{topic_or_q}")
        } else {
            format!("{topic_or_q}")
        };
        match github::search_repos_raw(&encode_query(&q), 1) {
            Ok(repos) => gh_repos.extend(repos),
            Err(_) => {}
        }
    }
    gh_repos.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    gh_repos.dedup_by(|a, b| a.full_name.eq_ignore_ascii_case(&b.full_name));

    let mut cards: Vec<MarketCard> = Vec::new();
    let gh_index: HashMap<String, &github::RepoSummary> =
        gh_repos.iter().map(|r| (r.full_name.clone(), r)).collect();
    for entry in &npm_entries {
        let gh = entry.repo.as_deref().and_then(|r| gh_index.get(r));
        cards.push(MarketCard {
            name: entry.name.clone(),
            sub_name: entry.repo.clone().unwrap_or_else(|| entry.name.clone()),
            description: entry.description.clone(),
            icon_url: gh.map(|g| g.avatar_url.clone()).unwrap_or_default(),
            tags: display_tags(&entry.keywords),
            monthly_downloads: entry.monthly_downloads,
            latest_version: (!entry.version.is_empty()).then_some(entry.version.clone()),
            stars: gh.map(|g| g.stars),
            updated_at: entry.date.clone(),
            source: "npm".into(),
            npm_name: Some(entry.name.clone()),
            repo: entry.repo.clone(),
            category: derive_category(&entry.keywords, &entry.description),
            verified: None,
        });
    }
    for repo in &gh_repos {
        if repo.full_name.eq_ignore_ascii_case(HARNESS_REPO) {
            continue;
        }
        let covered = cards.iter().any(|c| c.repo.as_deref() == Some(repo.full_name.as_str()));
        if covered {
            continue;
        }
        cards.push(MarketCard {
            name: repo.full_name.rsplit('/').next().unwrap_or(&repo.full_name).to_string(),
            sub_name: repo.full_name.clone(),
            description: repo.description.clone(),
            icon_url: repo.avatar_url.clone(),
            tags: display_tags(&repo.topics),
            monthly_downloads: None,
            latest_version: None,
            stars: Some(repo.stars),
            updated_at: repo.pushed_at.clone(),
            source: "github".into(),
            npm_name: None,
            repo: Some(repo.full_name.clone()),
            category: derive_category(&repo.topics, &repo.description),
            verified: None,
        });
    }
    cards.retain(is_modpack_card);

    let fetched_at = now_rfc3339();
    market_cache::update(|cache| {
        cache.modpacks = Some(market_cache::make_snapshot(
            fetched_at.clone(),
            "modpacks".into(),
            cards.iter().map(|c| serde_json::to_value(c).unwrap_or_default()).collect(),
        ));
    })?;

    let mut cards = cards;
    cards.retain(|c| matches(query, c));
    if !query.keyword.trim().is_empty() {
        cards.retain(|c| relevance_score(c, &query.keyword) > 0);
    }
    sort_cards(&mut cards, &query.sort, &query.keyword);
    let recall = Recall {
        npm: cards.iter().filter(|c| c.source == "npm").count() as u32,
        github: cards.iter().filter(|c| c.source == "github").count() as u32,
    };
    Ok(MarketFeed {
        cards,
        offline: false,
        fetched_at,
        recall,
    })
}

pub fn fetch_feed(query: &MarketQuery) -> Result<MarketFeed, String> {
    validate_query(query)?;
    let settings = mirror_config::load()?;
    let community = settings.community_active.clone();
    let built = if community == DSHHUB_ID {
        fetch_feed_dshhub()
    } else {
        fetch_feed_official(&query.keyword)
    };
    match built {
        Ok(mut cards) => {
            // 元插件/整合包类在快照落盘前就排除（离线快照同样干净）。
            cards.retain(|c| !is_meta_card(c));
            let recall = Recall {
                npm: cards.iter().filter(|c| c.source == "npm").count() as u32,
                github: cards.iter().filter(|c| c.source == "github").count() as u32,
            };
            let fetched_at = now_rfc3339();
            market_cache::update(|cache| {
                cache.feed = Some(market_cache::make_snapshot(
                    fetched_at.clone(),
                    community.clone(),
                    cards.iter().map(|c| serde_json::to_value(c).unwrap_or_default()).collect(),
                ));
            })?;
            let mut cards = cards;
            cards.retain(|c| matches(query, c));
            if !query.keyword.trim().is_empty() {
                // 0 分卡直接丢弃：服务端召回里总有纯噪声（npm AND 语义的
                // 松散命中），与查询毫无关系的条目不进结果列表。
                cards.retain(|c| relevance_score(c, &query.keyword) > 0);
                // 批量验证插件层声明，纯依赖库从结果剔除（源头拦截）——
                // 放在本地过滤**之后**：验证集合只剩相关存活者（几十张），
                // 才能落进批量验证的成本闸（整池 300 张会被上限闸跳过，
                // 真机 e2e 实证）。
                verify_bundle_decls(&mut cards);
            }
            sort_cards(&mut cards, &query.sort, &query.keyword);
            Ok(MarketFeed { cards, offline: false, fetched_at, recall })
        }
        Err(err) => {
            let cache = market_cache::load()?;
            // P3-05：离线回放前验完整性——快照被损坏/篡改判无效，宁可报在线
            // 错误也不把脏卡片喂给下游 UI。
            let snapshot = cache.feed.filter(|s| {
                s.community_active == community && market_cache::snapshot_intact(s)
            });
            match snapshot.filter(|s| !s.cards.is_empty()) {
                Some(snapshot) => {
                    let mut cards: Vec<MarketCard> = snapshot
                        .cards
                        .iter()
                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                        .collect();
                    cards.retain(|c| matches(query, c));
                    if !query.keyword.trim().is_empty() {
                        cards.retain(|c| relevance_score(c, &query.keyword) > 0);
                    }
                    sort_cards(&mut cards, &query.sort, &query.keyword);
                    Ok(MarketFeed {
                        cards,
                        offline: true,
                        fetched_at: snapshot.fetched_at,
                        recall: Recall::default(),
                    })
                }
                None => Err(err),
            }
        }
    }
}

/// 版本枚举应答：版本列表 + 该包是否声明插件层（走查定稿的源头拦截——
/// packument 内含每个版本的完整 manifest，零额外请求即可识别「依赖库
/// 冒充插件」：@isomoes/dsh-web-ui 实证）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionsReply {
    pub versions: Vec<MarketVersion>,
    /// 任一已发布版本声明 `dsh.bundle.patch` 才算插件；false = 纯依赖库。
    pub declares_bundle: bool,
}

/// 一个可安装版本的展开视图（折叠卡片展开列表的一行）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketVersion {
    pub version: String,
    pub latest: bool,
    pub published_at: String,
    /// 每版本周下载（T-4：versions 端点仅周粒度）；None = 缺数据隐藏。
    pub weekly_downloads: Option<u64>,
}

/// npm per-version 周下载表（尽力而为；失败重试 1 次后仍失败返回空表）。
/// scoped 包名的 `/` 必须编码为 `%2F`——裸拼路径会 404（真机走查实证的
/// 「周下载 --」根因；探证：编码后 api.npmjs.org 正常返回分版本数据）。
fn npm_weekly_downloads(package: &str) -> HashMap<String, u64> {
    let mut out = HashMap::new();
    let settings = match mirror_config::load() {
        Ok(s) => s,
        Err(_) => return out,
    };
    let encoded = package.trim().replace('/', "%2F");
    let raw = format!("https://api.npmjs.org/versions/{encoded}/last-week");
    let Ok(url) = gateway::resolve(&raw, &settings) else {
        return out;
    };
    let agent = crate::gateway::base_agent()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build();
    for _attempt in 0..2 {
        let Ok(response) = agent.get(&url).set("User-Agent", "dsh-launcher").call() else {
            continue;
        };
        let mut text = String::new();
        if response
            .into_reader()
            .take(4 * 1024 * 1024)
            .read_to_string(&mut text)
            .is_err()
        {
            continue;
        }
        let Ok(body) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if let Some(map) = body.get("downloads").and_then(|v| v.as_object()) {
            for (version, count) in map {
                if let Some(n) = count.as_u64() {
                    out.insert(version.clone(), n);
                }
            }
            return out;
        }
    }
    out
}

/// packument 中是否存在声明 `dsh.bundle.patch` 的版本（纯函数）。
pub fn packument_declares_bundle(packument: &serde_json::Value) -> bool {
    packument
        .pointer("/versions")
        .and_then(|v| v.as_object())
        .map(|versions| {
            versions
                .values()
                .any(|manifest| manifest.pointer("/dsh/bundle/patch").is_some())
        })
        .unwrap_or(false)
}

/// 拉取 npm 包的历史版本列表（含 dist-tags.latest 标注与每版本周下载）。
/// git 直装形态的版本枚举走 github::repo_tags（安装视图按需调用）。
pub fn fetch_versions(package: &str) -> Result<VersionsReply, String> {
    let packument = registry_meta::fetch_packument(package)?;
    let declares_bundle = packument_declares_bundle(&packument);
    let base = registry_meta::parse_packument(&packument)?;
    let weekly = npm_weekly_downloads(package);
    Ok(VersionsReply {
        versions: base
            .into_iter()
            .map(|v| {
                let weekly_downloads = weekly.get(&v.version).copied();
                MarketVersion {
                    version: v.version,
                    latest: v.latest,
                    published_at: v.published_at,
                    weekly_downloads,
                }
            })
            .collect(),
        declares_bundle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn card(name: &str, downloads: Option<u64>, stars: Option<u64>, updated: &str) -> MarketCard {
        MarketCard {
            name: name.into(),
            sub_name: format!("sub-{name}"),
            description: String::new(),
            icon_url: String::new(),
            tags: Vec::new(),
            monthly_downloads: downloads,
            latest_version: None,
            stars,
            updated_at: updated.into(),
            source: "npm".into(),
            npm_name: Some(name.into()),
            repo: None,
            category: "其他".into(),
            verified: None,
        }
    }

    #[test]
    fn hot_sort_puts_downloads_first_and_zero_signal_last() {
        let mut cards = vec![
            // 无月下载且无星标（零热度信号）→ 殿后；星标经折算参与热度。
            card("b", None, None, "2026-09-01T00:00:00Z"),
            card("c", Some(10), None, "2026-09-02T00:00:00Z"),
            card("a", Some(100), None, "2026-08-01T00:00:00Z"),
        ];
        sort_cards(&mut cards, "hot", "");
        let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["a", "c", "b"]);
    }

    /// 观察项裁决：git-only 卡（无月下载）以 星标×50 折算热度，不再被埋葬。
    #[test]
    fn hot_sort_folds_stars_for_git_only_cards() {
        let mut cards = vec![
            card("git", None, Some(6683), ""), // 6683×50 = 334150
            card("npm", Some(31097), None, ""),
        ];
        sort_cards(&mut cards, "hot", "");
        let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["git", "npm"], "高星 git 卡应折算进入热度序前列");

        // 有月下载的卡不受折算影响（月下载 > 折算值时维持原序）。
        let mut cards = vec![
            card("git-small", None, Some(10), ""), // 500
            card("npm", Some(31097), None, ""),
        ];
        sort_cards(&mut cards, "hot", "");
        let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["npm", "git-small"]);
    }

    #[test]
    fn hot_sort_tiebreaks_by_stars_then_name() {
        let mut cards = vec![
            card("b", Some(50), Some(1), ""),
            card("c", Some(50), Some(9), ""),
            card("a", Some(50), Some(9), ""),
        ];
        sort_cards(&mut cards, "hot", "");
        let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["a", "c", "b"], "下载/星标并列者按名称字典序；星标低者殿后");
    }

    #[test]
    fn new_sort_by_updated_desc_with_empty_last() {
        let mut cards = vec![
            card("old", Some(999), None, "2026-01-01T00:00:00Z"),
            card("none", Some(1), None, ""),
            card("newest", Some(1), None, "2026-09-01T00:00:00Z"),
        ];
        sort_cards(&mut cards, "new", "");
        let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["newest", "old", "none"]);
    }

    #[test]
    fn query_filters_keyword_source_and_kind() {
        let mut plugin = card("dsh-cost-meter", Some(1), None, "");
        plugin.description = "会话费用统计插件".into();
        plugin.category = "效率".into();
        let query = MarketQuery {
            sort: "hot".into(),
            keyword: "费用".into(),
            source: "all".into(),
            kind: "all".into(),
        };
        assert!(matches(&query, &plugin));
        assert!(!matches(&MarketQuery { source: "github".into(), ..query.clone() }, &plugin));
        assert!(!matches(&MarketQuery { kind: "界面".into(), ..query.clone() }, &plugin));
        assert!(matches(&MarketQuery { keyword: "COST".into(), ..query.clone() }, &plugin), "关键词大小写不敏感");
        assert!(!matches(&MarketQuery { keyword: "不存在".into(), ..query }, &plugin));
    }

    #[test]
    fn validate_query_rejects_unknown_values() {
        let ok = MarketQuery { sort: "hot".into(), keyword: String::new(), source: "all".into(), kind: "all".into() };
        assert!(validate_query(&ok).is_ok());
        assert!(validate_query(&MarketQuery { sort: "star".to_string(), ..ok.clone() }).is_err());
        assert!(validate_query(&MarketQuery { source: "npmmirror".to_string(), ..ok.clone() }).is_err());
        assert!(validate_query(&MarketQuery { kind: "皮肤".to_string(), ..ok }).is_err());
    }

    #[test]
    fn category_rules_hit_by_keyword_then_description() {
        assert_eq!(derive_category(&["dsh-plugin".into(), "theme".into()], ""), "界面");
        assert_eq!(derive_category(&[], "支持飞书/Lark 的机器人"), "机器人");
        assert_eq!(derive_category(&["vision".into()], ""), "视觉");
        assert_eq!(derive_category(&["zzz".into()], "毫无特征的描述"), "其他");
    }

    #[test]
    fn normalize_repo_url_strips_git_markers() {
        assert_eq!(
            normalize_repo_url("git+https://github.com/Owner/Repo.git"),
            Some("Owner/Repo".into())
        );
        assert_eq!(normalize_repo_url("https://github.com/a/b"), Some("a/b".into()));
        assert_eq!(normalize_repo_url("https://github.com/a/b/deep/nest"), Some("a/b".into()), "深路径截断到 owner/repo");
        assert_eq!(normalize_repo_url("https://gitlab.com/a/b"), None, "非 GitHub 不归一");
        assert_eq!(normalize_repo_url(""), None);
    }

    #[test]
    fn npm_search_parses_entries_and_skips_incomplete() {
        let body = json!({
            "total": 2,
            "objects": [
                { "package": {
                    "name": " dsh-cost-meter ", "version": "1.7.5",
                    "date": "2026-08-30T00:00:00.000Z",
                    "description": "费用统计",
                    "keywords": ["dsh-plugin", "tool"],
                    "links": { "repository": "git+https://github.com/Han-1413141/dsh-cost-meter.git" }
                }, "downloads": { "monthly": 31097 } },
                { "package": { "nope": true } }
            ]
        });
        let entries = parse_npm_search(&body);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.name, "dsh-cost-meter");
        assert_eq!(entry.repo.as_deref(), Some("Han-1413141/dsh-cost-meter"));
        assert_eq!(entry.keywords, vec!["dsh-plugin", "tool"]);
        assert_eq!(entry.monthly_downloads, Some(31097));
        assert!(parse_npm_search(&json!({})).is_empty());
    }

    #[test]
    fn display_tags_drops_noise_and_caps_at_four() {
        let tags = display_tags(&[
            "dsh-plugin".into(),
            "Tool".into(),
            "dsh".into(),
            "UI".into(),
            "theme".into(),
            "ui".into(),
            "bot".into(),
        ]);
        assert_eq!(tags, vec!["tool", "ui", "theme", "bot"], "噪声剔除、小写去重、至多 4 个");
    }

    #[test]
    fn encode_query_percent_encodes_unsafe_bytes() {
        assert_eq!(encode_query("dsh web ui"), "dsh%20web%20ui");
        assert_eq!(encode_query("keywords:dsh-plugin"), "keywords%3Adsh-plugin");
        assert_eq!(encode_query("dsh-web-ui.1_x~"), "dsh-web-ui.1_x~", "unreserved 字符原样");
        assert_eq!(encode_query(""), "");
    }

    #[test]
    fn search_text_and_pages_follow_keyword_mode() {
        assert_eq!(npm_search_text(""), "keywords:dsh-plugin", "无关键词 = 目录口径");
        assert_eq!(npm_search_text("  dsh-web-ui "), "dsh-web-ui", "关键词模式 = 原文真搜");
        assert_eq!(search_pages_for(""), 2);
        assert_eq!(search_pages_for("dsh web ui"), 1, "关键词模式单页（相关度排序直达）");
    }

    /// 相关度打分六场景（走查需求①~⑥逐一裁决）；对照卡取自走查截图。
    fn ui_card(name: &str, sub: &str, desc: &str) -> MarketCard {
        MarketCard {
            name: name.into(),
            sub_name: sub.into(),
            description: desc.into(),
            icon_url: String::new(),
            tags: vec!["web-ui".into()],
            monthly_downloads: None,
            latest_version: None,
            stars: Some(100),
            updated_at: String::new(),
            source: "npm".into(),
            npm_name: Some(name.into()),
            repo: Some(sub.into()),
            category: "界面".into(),
            verified: None,
        }
    }

    #[test]
    fn relevance_full_name_beats_loose_matches() {
        // 真实形态：npm 卡的 name 即完整 scoped id。
        let mut target = ui_card("@linxin666/dsh-web-ui-all", "linxin666/dsh-web-ui-all", "DSH Web UI 插件聚合包");
        target.monthly_downloads = Some(5000);
        let mut ads = ui_card("dsh-ads", "Nagi-ovo/dsh-ads", "把 DSH 变成 2005 年门户网站");
        ads.monthly_downloads = Some(999999);
        let mut panel = ui_card("dsh-skill-mcp-panel", "Fishquito7/dsh-skill-mcp-panel", "DSH Web UI plugin: skill 管理");
        panel.monthly_downloads = Some(999999);

        // ① 复制粘贴全名：scoped 全名与裸名末段皆满分。
        assert_eq!(relevance_score(&target, "@linxin666/dsh-web-ui-all"), 1000);
        assert_eq!(relevance_score(&target, "dsh-web-ui-all"), 1000, "裸名末段等价");
        // ⑥ 大小写任意混合。
        assert_eq!(relevance_score(&target, "DSH-Web-UI-All"), 1000);
        // ③④ 空格替代标点 / 完全无分隔：非 scoped 卡压扁全等 900。
        let plain = ui_card("dsh-web-ui", "linxin666/dsh-web-ui", "DSH Web UI 基础版");
        assert_eq!(relevance_score(&plain, "dsh web ui"), 900);
        assert_eq!(relevance_score(&plain, "dshwebui"), 900);
        assert_eq!(relevance_score(&plain, "Dsh WebUI"), 900, "大小写+驼峰混排");
        // scoped 卡经裸名压扁前缀层。
        assert!(relevance_score(&target, "dsh web ui") >= 750);
        assert!(relevance_score(&target, "dshwebui") >= 750);
        // ② 简名（省去 dsh- 前缀）→ token 覆盖层。
        assert!(relevance_score(&target, "web ui") > 0);
        // 层级裁决：目标必须压过松散命中（货不对板的根治）。
        for keyword in ["dsh-web-ui", "dsh web ui", "dshwebui", "web ui", "@linxin666/dsh-web-ui-all"] {
            assert!(
                relevance_score(&target, keyword) > relevance_score(&ads, keyword),
                "关键词 {keyword:?}：dsh-web-ui 系必须排在 dsh-ads 之前"
            );
            assert!(
                relevance_score(&target, keyword) > relevance_score(&panel, keyword),
                "关键词 {keyword:?}：dsh-web-ui 系必须排在 dsh-skill-mcp-panel 之前"
            );
        }
    }

    #[test]
    fn relevance_folk_name_hits_description_and_zero_drops_noise() {
        // ⑤ 民间叫法：中文子串命中描述（token 覆盖层）。
        let ads = ui_card("dsh-ads", "Nagi-ovo/dsh-ads", "把 DSH 变成 2005 年门户网站");
        let score = relevance_score(&ads, "门户网站");
        assert!(score > 0, "民间叫法应经描述命中：{score}");
        // 毫无关系的查询 → 0 分（调用方丢弃）。
        assert_eq!(relevance_score(&ads, "费用统计"), 0);
        assert_eq!(relevance_score(&ads, ""), 0);
    }

    #[test]
    fn author_search_hits_owner_layer_and_tops_results() {
        let mut mine = ui_card("@linxin666/dsh-web-ui-all", "linxin666/dsh-web-ui-all", "DSH Web UI 插件聚合包");
        mine.monthly_downloads = Some(100);
        let mut other = ui_card("dsh-cost-meter", "Han-1413141/dsh-cost-meter", "会话费用统计插件");
        other.monthly_downloads = Some(999999);
        // 作者名全等 → 850 层（介于压扁全等与名称前缀之间）。
        assert_eq!(relevance_score(&mine, "linxin666"), 850);
        assert!(relevance_score(&other, "linxin666") < 850, "非该作者的插件不得借道作者层");
        let mut cards = vec![other, mine];
        sort_cards(&mut cards, "hot", "linxin666");
        assert_eq!(cards[0].name, "@linxin666/dsh-web-ui-all", "作者名搜索该作者插件置顶");
    }

    #[test]
    fn meta_cards_excluded_by_exact_list_keywords_and_awesome() {
        let market = ui_card("dshmarket", "dsh-market/dsh-market", "Visual plugin market inside DeepSeek Harness");
        assert!(is_meta_card(&market), "市场类精确名单命中");
        let bundle = ui_card("@linxin666/dsh-web-ui-all", "linxin666/dsh-web-ui-all", "DSH Web UI 全家桶聚合插件：一键安装全部功能插件");
        assert!(is_meta_card(&bundle), "全家桶/聚合描述启发命中");
        let list_repo = ui_card("awesome-dsh-plugin", "awesome-dsh-plugin/awesome-dsh-plugin", "A curated list of plugins");
        assert!(is_meta_card(&list_repo), "awesome 精选清单命中");
        let official = ui_card("@deepseek-ai/dsh-web", "deepseek-ai/dsh-web", "DSH Web 应用");
        assert!(is_meta_card(&official), "本体家族（@deepseek-ai/*）命中");
        let normal = ui_card("dsh-cost-meter", "Han-1413141/dsh-cost-meter", "会话费用统计插件");
        assert!(!is_meta_card(&normal), "普通功能插件不得误伤");
    }

    /// 整合包栏分类：聚合型资源收编，单功能插件/市场应用两头都不进。
    #[test]
    fn modpack_cards_are_aggregation_type_only() {
        let bundle = ui_card("@linxin666/dsh-web-ui-all", "linxin666/dsh-web-ui-all", "DSH Web UI 全家桶聚合插件：一键安装全部功能插件");
        assert!(is_modpack_card(&bundle), "全家桶/聚合描述命中");
        let suite = ui_card("dsh-linxin-suite", "a/b", "ui suite for dsh");
        assert!(is_modpack_card(&suite), "-suite 后缀命中");
        let aggregator = ui_card("dsh-bundle-kit", "a/b", "all-in-one kit");
        assert!(is_modpack_card(&aggregator), "all-in-one 描述命中");
        let normal = ui_card("dsh-cost-meter", "Han-1413141/dsh-cost-meter", "会话费用统计插件");
        assert!(!is_modpack_card(&normal), "单功能插件不进整合包栏");
        let market = ui_card("dshmarket", "dsh-market/dsh-market", "Visual plugin market inside DeepSeek Harness");
        assert!(!is_modpack_card(&market), "市场应用（meta）两头都不进");
        let official = ui_card("@deepseek-ai/dsh-web", "deepseek-ai/dsh-web", "DSH Web 应用");
        assert!(!is_modpack_card(&official), "本体家族不进整合包栏");
    }

    /// 真实网络 e2e（显式触发，注意 GitHub 配额卫生）：官方源聚合返回非空
    /// 卡片、热度首位有下载量；版本枚举含每版周下载；scoped 包 %2F 编码回归；
    /// 依赖库源头拦截（isomoes 冒充样本）。
    #[test]
    #[ignore = "真实网络：GitHub + npm registry + downloads API"]
    fn real_feed_and_versions_e2e() {
        let query = MarketQuery {
            sort: "hot".into(),
            keyword: String::new(),
            source: "all".into(),
            kind: "all".into(),
        };
        let feed = fetch_feed(&query).expect("官方源信息流拉取失败");
        assert!(!feed.offline, "首次拉取不应是离线快照");
        assert!(feed.cards.len() >= 20, "卡片数量异常：{}", feed.cards.len());
        let hot = &feed.cards[0];
        // 热度首位须有真实热度信号：月下载，或 git-only 卡的星标折算。
        assert!(
            hot.monthly_downloads.unwrap_or(0) > 0 || hot.stars.unwrap_or(0) > 0,
            "热度首位应有下载量或星标折算：{hot:?}"
        );
        assert!(
            feed.cards.iter().all(|c| !c.name.starts_with("@deepseek-ai/")),
            "本体家族包不应出现在市场"
        );

        let versions = fetch_versions("dsh-cost-meter").expect("版本枚举失败");
        assert!(versions.versions.len() >= 10);
        assert!(versions.versions.iter().any(|v| v.latest), "应有 dist-tags.latest 标注");
        assert!(
            versions.versions.iter().any(|v| v.weekly_downloads.unwrap_or(0) > 0),
            "至少一个版本有周下载量"
        );
        assert!(versions.declares_bundle, "真插件应声明 bundles 层");
        println!("dsh-cost-meter 版本数：{}，declares_bundle={}", versions.versions.len(), versions.declares_bundle);

        // scoped 包的 per-version 周下载（%2F 编码修复的回归锚点）。
        let scoped = fetch_versions("@linxin666/dsh-web-ui-all").expect("scoped 版本枚举失败");
        assert!(!scoped.versions.is_empty());
        assert!(
            scoped.versions.iter().any(|v| v.weekly_downloads.unwrap_or(0) > 0),
            "scoped 包不应整列「周下载 --」"
        );
        println!(
            "scoped 周下载样本：{:?}",
            scoped.versions.iter().filter(|v| v.weekly_downloads.unwrap_or(0) > 0).take(3)
                .map(|v| format!("{}={}", v.version, v.weekly_downloads.unwrap()))
                .collect::<Vec<_>>()
        );

        // 依赖库冒充插件（keywords 打了 dsh-plugin 但无 dsh 字段）→ 源头拦截。
        let impostor = fetch_versions("@isomoes/dsh-web-ui").expect("依赖库版本枚举失败");
        assert!(!impostor.declares_bundle, "该包无 dsh 字段，不应被判为插件");
    }

    /// 真实 e2e（走查反馈回归，显式触发）：搜索三态——精确名首卡命中 git
    /// 直装卡、依赖库源头剔除、作者名置顶。配额卫生：3 轮，每轮 npm 3 + GitHub 1。
    #[test]
    #[ignore = "真实网络：关键词搜索回归"]
    fn real_feed_keyword_search_finds_dsh_web_ui() {
        let query_of = |keyword: &str| MarketQuery {
            sort: "hot".into(),
            keyword: keyword.into(),
            source: "all".into(),
            kind: "all".into(),
        };

        // ① 精确名：zhu1090093659/dsh-web（6683 星、git 直装）必须置顶——
        //    之前「从来没跳出来过」的根因是热排序键缺失把它埋底，现已解。
        {
            let feed = fetch_feed(&query_of("dsh-web")).expect("dsh-web 搜索失败");
            assert!(!feed.offline);
            assert_eq!(feed.cards[0].name, "dsh-web");
            assert_eq!(feed.cards[0].repo.as_deref(), Some("zhu1090093659/dsh-web"));
            println!("dsh-web → {} 张卡，首卡：{}", feed.cards.len(), feed.cards[0].name);
        }

        // ② 源头拦截：依赖库（@isomoes/dsh-web-ui，无 dsh 字段）从结果剔除。
        {
            let feed = fetch_feed(&query_of("dsh-web-ui")).expect("dsh-web-ui 搜索失败");
            let impostor = feed
                .cards
                .iter()
                .find(|c| c.npm_name.as_deref() == Some("@isomoes/dsh-web-ui"));
            assert!(impostor.is_none(), "依赖库应被批量 packument 验证剔除");
            println!("dsh-web-ui → {} 张卡（依赖库已剔除）", feed.cards.len());
        }

        // ③ 作者名：该作者插件置顶。
        {
            let feed = fetch_feed(&query_of("zhu1090093659")).expect("作者名搜索失败");
            assert!(!feed.cards.is_empty(), "作者名搜索不应为空");
            let owner_of = |c: &MarketCard| {
                c.repo
                    .as_deref()
                    .and_then(|r| r.split('/').next())
                    .map(str::to_string)
                    .or_else(|| {
                        c.npm_name
                            .as_deref()
                            .and_then(|n| n.strip_prefix('@'))
                            .and_then(|rest| rest.split('/').next())
                            .map(str::to_string)
                    })
                    .unwrap_or_default()
            };
            assert_eq!(owner_of(&feed.cards[0]), "zhu1090093659", "首卡应属该作者");
            println!("zhu1090093659 → {} 张卡，首卡：{}", feed.cards.len(), feed.cards[0].name);
        }
    }

    /// 真实 e2e（走查补需求，显式触发）：作者名搜索——该作者插件置顶。
    #[test]
    #[ignore = "真实网络：作者名搜索"]
    fn real_feed_author_search() {
        let query = MarketQuery {
            sort: "hot".into(),
            keyword: "linxin666".into(),
            source: "all".into(),
            kind: "all".into(),
        };
        let feed = fetch_feed(&query).expect("作者名搜索失败");
        assert!(!feed.cards.is_empty(), "作者名搜索不应为空");
        let owner_of = |c: &MarketCard| {
            c.repo
                .as_deref()
                .and_then(|r| r.split('/').next())
                .map(str::to_string)
                .or_else(|| {
                    // 与 relevance_score 同口径：scoped npm 名的 @scope 段也算作者。
                    c.npm_name
                        .as_deref()
                        .and_then(|n| n.strip_prefix('@'))
                        .and_then(|rest| rest.split('/').next())
                        .map(str::to_string)
                })
                .unwrap_or_default()
        };
        assert_eq!(
            owner_of(&feed.cards[0]),
            "linxin666",
            "首卡应属该作者（实际首卡：{}）",
            feed.cards[0].name
        );
        println!("作者名搜索 → {} 张卡，首卡：{}", feed.cards.len(), feed.cards[0].name);
    }



    #[test]
    fn modpack_positive_filter_classifies_aggregates() {
        // 聚合包：全家桶/聚合/套件/命名惯例四路命中。
        let family = ui_card("@linxin666/dsh-web-ui-all", "linxin666/dsh-web-ui-all", "DSH Web UI 全家桶聚合插件：一键安装全部功能插件");
        assert!(is_modpack_card(&family));
        let suite_named = ui_card("dsh-skin-suite", "a/dsh-skin-suite", "Skins and tweaks");
        assert!(is_modpack_card(&suite_named), "-suite 命名惯例");
        let all_named = ui_card("dsh-utility-pack", "a/b", "Utils collection");
        assert!(is_modpack_card(&all_named), "collection 关键词");
        // 单一功能插件与市场应用都不进整合包栏。
        let single = ui_card("dsh-cost-meter", "Han-1413141/dsh-cost-meter", "DeepSeek Harness 会话费用统计插件:本会话成本、当日费用、历史记录与官方价格同步");
        assert!(!is_modpack_card(&single));
        let market_app = ui_card("dshmarket", "dsh-market/dsh-market", "Visual plugin market inside DeepSeek Harness");
        assert!(!is_modpack_card(&market_app), "市场应用两头都不算");
        let official = ui_card("@deepseek-ai/dsh-web", "a/b", "官方 web 前端");
        assert!(!is_modpack_card(&official), "本体家族同样不进整合包栏");
    }

    /// 真实 e2e：整合包目录全为聚合型资源，单一功能插件不混入，
    /// @linxin666/dsh-web-ui-all 应在列（全家桶语义的典型样本）。
    #[test]
    #[ignore = "真实网络：整合包目录聚合"]
    fn real_modpack_feed_contains_aggregates_only() {
        let query = MarketQuery {
            sort: "hot".into(),
            keyword: String::new(),
            source: "all".into(),
            kind: "all".into(),
        };
        let feed = fetch_modpack_feed(&query).expect("整合包目录拉取失败");
        assert!(!feed.offline);
        assert!(!feed.cards.is_empty(), "整合包目录不应为空");
        assert!(feed.cards.iter().all(is_modpack_card), "全部条目应为聚合型");
        assert!(
            feed.cards
                .iter()
                .any(|c| c.npm_name.as_deref() == Some("@linxin666/dsh-web-ui-all")),
            "@linxin666/dsh-web-ui-all 应在整合包目录：{:?}",
            feed.cards.iter().map(|c| c.name.as_str()).take(8).collect::<Vec<_>>()
        );
        assert!(
            feed.cards.iter().all(|c| c.npm_name.as_deref() != Some("dsh-cost-meter")),
            "单一功能插件不应混入整合包栏"
        );
        println!("整合包目录 → {} 张卡", feed.cards.len());
        for c in feed.cards.iter().take(8) {
            println!("  - {}（{}）", c.name, c.description.chars().take(40).collect::<String>());
        }
    }
}
