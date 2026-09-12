//! 统一下载网关（FR-18）：所有远程取数的 URL 都经这里改写，业务模块禁止
//! 自行拼直链。镜像只改下载 URL 的取用通道，不改变聚合元数据的来源与解析。
//!
//! FR-18 修订（需求方 2026-09-01 拍板）：默认「官方源：npm 优先，GitHub 默认」——
//! npm / Node 发行包通道**恒为 npmmirror 优先 + 官方兜底**（优先≠独占，调用方按
//! 候选链自动回落），npmmirror 不再是可选择激活源；设置只影响 GitHub 类 URL
//! （custom 前缀）与社区资源通道。GitHub 类（github.com / codeload / raw /
//! objects）默认官方直连，custom 源应用 `{prefix}/{原始完整 URL}`。
//! 官方直连 = 原样返回；不认识的 URL 一律原样放行（网关不做黑名单）。
//!
//! 连通性测试也在此实现：对源的探测端点发一次真实请求，任意 HTTP 响应
//! （含 4xx/5xx）都算「可达」，只有传输层失败才算不可达。

use std::time::Instant;

use crate::mirror_config::{self, MirrorSettings, CUSTOM_ID, OFFICIAL_ID};

/// 官方 npm registry 基址。
pub const NPM_OFFICIAL_REGISTRY: &str = "https://registry.npmjs.org";
/// 内置 npmmirror 的 npm registry 基址（官方源模式下 npm 类资源的优先通道）。
pub const NPM_NPMMIRROR_REGISTRY: &str = "https://registry.npmmirror.com";
/// 官方 npm 下载量统计 API 基址（审查 R1 归入网关治理）。npmmirror 无此 API
/// （探证 404）——恒走官方单通道、不进候选链；归入 Npm 类仅为取数单点治理
/// 与 resolve 幂等，无改写语义。
pub const NPM_DOWNLOADS_OFFICIAL: &str = "https://api.npmjs.org";
/// 官方 Node 发行包基址。
pub const NODE_DIST_OFFICIAL: &str = "https://nodejs.org/dist";
/// 内置 npmmirror 的 Node 发行包基址。
pub const NODE_DIST_NPMMIRROR: &str = "https://registry.npmmirror.com/-/binary/node";
/// 官方 GitHub 基址（归档 / release / raw 等都以它为起点构造）。
pub const GITHUB_OFFICIAL: &str = "https://github.com";
/// 社区资源镜像：Gitee 基址（Phase 3 按仓库映射表消费）。
pub const GITEE_OFFICIAL: &str = "https://gitee.com";
/// dsh-hub.cc 健康检查端点（侦察确认存在；Phase 3 作为聚合数据源接入，
/// 本批仅作连通性探测点）。
pub const DSHHUB_HEALTH: &str = "https://dsh-hub.cc/api/v1/health";
/// dsh-hub.cc 社区插件目录站基址（社区资源通道 = dshhub 时的聚合数据源）。
pub const DSHHUB_OFFICIAL: &str = "https://dsh-hub.cc";

/// GitHub 类主机后缀集合（精确 host 匹配）。
const GITHUB_HOSTS: &[&str] = &[
    "github.com",
    "api.github.com",
    "codeload.github.com",
    "raw.githubusercontent.com",
    "objects.githubusercontent.com",
];

/// URL 类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// npm registry 上的 packument 或 tarball。
    Npm,
    /// nodejs.org 发行包树。
    NodeDist,
    /// GitHub 仓库内容（release 资产、archive、codeload、raw）。
    Github,
    /// 不属于任何已知类别；网关原样放行。
    Unknown,
}

/// 按 host 判断 URL 类别（scheme 必须是 https，否则视为 Unknown）。
pub fn classify(url: &str) -> ResourceKind {
    let Some(rest) = url.strip_prefix("https://") else {
        return ResourceKind::Unknown;
    };
    let host = rest.split('/').next().unwrap_or("");
    match host {
        "registry.npmjs.org" | "registry.npmmirror.com" | "api.npmjs.org" => ResourceKind::Npm,
        "nodejs.org" => ResourceKind::NodeDist,
        _ if GITHUB_HOSTS.contains(&host) => ResourceKind::Github,
        _ => ResourceKind::Unknown,
    }
}

/// 把官方形态的 URL 改写为当前激活源下的取用 URL。
///
/// 输入约定为「官方直连形态」（registry.npmjs.org / nodejs.org / github.com…）；
/// 已经指向镜像的 URL 按同一类别规则幂等放行。npm / Node 类 URL 在任何激活源下
/// 都原样返回官方形态——mirror 优先语义由 `npm_registry_bases` / `node_dist_bases`
/// 候选链表达，不在这条单 URL 改写里。
pub fn resolve(url: &str, settings: &MirrorSettings) -> Result<String, String> {
    if url.starts_with("http://") {
        // 明文 http 只出现在本地探测场景，生产资源一律 https；原样放行。
        return Ok(url.to_string());
    }

    match classify(url) {
        ResourceKind::Github => {
            if settings.active == CUSTOM_ID {
                let prefix = mirror_config::normalize_prefix(&settings.custom_prefix);
                mirror_config::validate(settings)?;
                Ok(format!("{prefix}/{url}"))
            } else {
                Ok(url.to_string())
            }
        }
        ResourceKind::Npm | ResourceKind::NodeDist | ResourceKind::Unknown => Ok(url.to_string()),
    }
}

/// npm registry 基址候选序列：npmmirror 优先、官方兜底（FR-18 修订后的恒定
/// 通道顺序，优先≠独占——安装失败时调用方逐个回落并在错误里点名通道）。
pub fn npm_registry_bases() -> Vec<&'static str> {
    vec![NPM_NPMMIRROR_REGISTRY, NPM_OFFICIAL_REGISTRY]
}

/// Node 发行包基址候选序列：npmmirror 优先、官方兜底（真机实证：官方直连
/// 会数据流停滞，npmmirror 数十秒完成）。
pub fn node_dist_bases() -> Vec<&'static str> {
    vec![NODE_DIST_NPMMIRROR, NODE_DIST_OFFICIAL]
}

/// 单个探测点的结果行。
struct ProbeLine {
    ok: bool,
    latency_ms: u128,
    detail: String,
}

/// 对单个 URL 做一次真实 GET 探测：任意 HTTP 状态都算可达。
fn probe_url(url: &str) -> ProbeLine {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(20))
        .build();
    let started = Instant::now();
    match agent.get(url).call() {
        Ok(response) => ProbeLine {
            ok: true,
            latency_ms: started.elapsed().as_millis(),
            detail: format!("HTTP {}", response.status()),
        },
        Err(ureq::Error::Status(code, _)) => ProbeLine {
            ok: true,
            latency_ms: started.elapsed().as_millis(),
            detail: format!("HTTP {code}"),
        },
        Err(e) => ProbeLine {
            ok: false,
            latency_ms: 0,
            detail: format!("{e}"),
        },
    }
}

/// 连通性测试结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProbeResult {
    /// 是否可达（任意 HTTP 状态都算可达，含 404/403）。
    pub ok: bool,
    /// 往返耗时（毫秒）；多探测点取最慢者，不可达时为 0。
    pub latency_ms: u128,
    /// 可达时的 HTTP 状态码或不可达时的错误摘要；多探测点逐行拼接。
    pub detail: String,
}

/// 测试指定源连通性。阻塞网络调用——command 侧放 spawn_blocking。
///
/// 源 id：`official`（版本通道双探测点：npm 镜像 + GitHub 直连，分别展示延迟）、
/// `github`（社区通道官方探测点）、`custom`（需带 prefix）、`gitee`、`dshhub`。
pub fn test_source(source_id: &str, custom_prefix: &str) -> ProbeResult {
    let probes: Vec<(&str, String)> = match source_id {
        // 官方源 = 双通道策略，两个探测点各自报告（npm 镜像优先通道 + GitHub 直连）。
        OFFICIAL_ID => vec![
            ("npm 镜像", format!("{NPM_NPMMIRROR_REGISTRY}/@deepseek-ai/dsh")),
            ("GitHub 直连", format!("{GITHUB_OFFICIAL}/deepseek-ai/deepseek-harness")),
        ],
        "github" => vec![("GitHub", format!("{GITHUB_OFFICIAL}/deepseek-ai/deepseek-harness"))],
        "gitee" => vec![("Gitee", GITEE_OFFICIAL.to_string())],
        "dshhub" => vec![("dsh-hub", DSHHUB_HEALTH.to_string())],
        CUSTOM_ID => {
            if mirror_config::valid_prefix(custom_prefix) {
                vec![(
                    "自定义前缀",
                    format!(
                        "{}/https://github.com/deepseek-ai/deepseek-harness",
                        mirror_config::normalize_prefix(custom_prefix)
                    ),
                )]
            } else {
                return ProbeResult {
                    ok: false,
                    latency_ms: 0,
                    detail: "自定义前缀非法，先修正再测试".into(),
                };
            }
        }
        other => {
            return ProbeResult {
                ok: false,
                latency_ms: 0,
                detail: format!("未知源 id：{other:?}"),
            }
        }
    };

    let mut lines = Vec::new();
    let mut all_ok = true;
    let mut max_latency = 0u128;
    for (label, url) in probes {
        let line = probe_url(&url);
        all_ok &= line.ok;
        max_latency = max_latency.max(line.latency_ms);
        lines.push(format!("{label} {} · {} ms", line.detail, line.latency_ms));
    }
    ProbeResult {
        ok: all_ok,
        latency_ms: max_latency,
        detail: lines.join(" ｜ "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mirror_config::NPMMIRROR_ID;

    fn settings(active: &str, prefix: &str) -> MirrorSettings {
        MirrorSettings {
            active: active.into(),
            custom_prefix: prefix.into(),
            community_active: OFFICIAL_ID.into(),
        }
    }

    #[test]
    fn classify_recognizes_known_hosts() {
        assert_eq!(classify("https://registry.npmjs.org/@deepseek-ai/dsh"), ResourceKind::Npm);
        assert_eq!(
            classify("https://registry.npmjs.org/@deepseek-ai/dsh/-/dsh-0.1.1-rc.2.tgz"),
            ResourceKind::Npm
        );
        assert_eq!(classify("https://nodejs.org/dist/v24.12.0/node.zip"), ResourceKind::NodeDist);
        assert_eq!(
            classify("https://github.com/a/b/releases/download/v1/f.zip"),
            ResourceKind::Github
        );
        assert_eq!(classify("https://codeload.github.com/a/b/zip/refs/heads/main"), ResourceKind::Github);
        assert_eq!(classify("https://api.github.com/search/repositories"), ResourceKind::Github, "REST API 同属 GitHub 类，custom 前缀生效");
        assert_eq!(classify("http://registry.npmjs.org/x"), ResourceKind::Unknown, "明文 http 不改写");
        assert_eq!(classify("https://example.com/x"), ResourceKind::Unknown);
    }

    /// npm 下载量统计 API 归 Npm 类但恒官方直通（审查 R1：npmmirror 无此 API，
    /// 不进候选链；任何激活源下 resolve 都不改写）。
    #[test]
    fn npm_downloads_api_is_classified_and_passthrough() {
        let url = "https://api.npmjs.org/downloads/point/last-month/dsh-cost-meter";
        assert_eq!(classify(url), ResourceKind::Npm);
        for active in [OFFICIAL_ID, NPMMIRROR_ID] {
            assert_eq!(
                resolve(url, &settings(active, "")).unwrap(),
                url,
                "下载量 API 恒官方单通道"
            );
        }
    }

    #[test]
    fn official_is_identity_for_every_kind() {
        let s = settings(OFFICIAL_ID, "");
        for url in [
            "https://registry.npmjs.org/@deepseek-ai/dsh",
            "https://nodejs.org/dist/v24/node.exe",
            "https://github.com/a/b/archive/x.zip",
        ] {
            assert_eq!(resolve(url, &s).unwrap(), url);
        }
    }

    /// npmmirror 内化后不再是激活源；resolve 对 npm/Node 类 URL 恒返回官方
    /// 形态（mirror 优先由候选链表达），GitHub 类只在 custom 下改写。
    #[test]
    fn resolve_never_rewrites_npm_or_node_and_keeps_github_custom() {
        for active in [OFFICIAL_ID, NPMMIRROR_ID] {
            let s = settings(active, "");
            assert_eq!(
                resolve("https://registry.npmjs.org/@deepseek-ai/dsh", &s).unwrap(),
                "https://registry.npmjs.org/@deepseek-ai/dsh"
            );
            assert_eq!(
                resolve("https://nodejs.org/dist/v24.12.0/node-v24.12.0-win-x64.zip", &s).unwrap(),
                "https://nodejs.org/dist/v24.12.0/node-v24.12.0-win-x64.zip"
            );
            assert_eq!(
                resolve("https://github.com/a/b/releases/download/v1/f.zip", &s).unwrap(),
                "https://github.com/a/b/releases/download/v1/f.zip"
            );
        }
        // 已是镜像形态的 npm URL 幂等放行。
        let s = settings(OFFICIAL_ID, "");
        assert_eq!(
            resolve("https://registry.npmmirror.com/@deepseek-ai/dsh", &s).unwrap(),
            "https://registry.npmmirror.com/@deepseek-ai/dsh"
        );
        // custom 只作用于 GitHub 类。
        let c = settings(CUSTOM_ID, "https://gh.example.top/");
        assert_eq!(
            resolve("https://github.com/deepseek-ai/deepseek-harness/archive/x.zip", &c).unwrap(),
            "https://gh.example.top/https://github.com/deepseek-ai/deepseek-harness/archive/x.zip"
        );
        assert_eq!(
            resolve("https://registry.npmjs.org/@deepseek-ai/dsh", &c).unwrap(),
            "https://registry.npmjs.org/@deepseek-ai/dsh",
            "npm 在 custom 源下走官方 registry"
        );
        assert_eq!(
            resolve("https://nodejs.org/dist/v24/x.zip", &c).unwrap(),
            "https://nodejs.org/dist/v24/x.zip"
        );
    }

    #[test]
    fn custom_with_invalid_prefix_fails_loud_on_github_rewrite() {
        let s = settings(CUSTOM_ID, "not-a-url");
        assert!(resolve("https://github.com/a/b", &s).is_err());
    }

    #[test]
    fn unknown_host_is_passthrough_not_rewrite() {
        let s = settings(OFFICIAL_ID, "");
        // 精确 host 匹配：相似域名不命中规则，原样放行（网关不做黑名单）。
        assert_eq!(
            resolve("https://nodejs.org.evil/v24/x", &s).unwrap(),
            "https://nodejs.org.evil/v24/x"
        );
    }

    /// FR-18 修订：候选链恒为 npmmirror 优先、官方兜底，与激活源无关
    /// （优先≠独占，调用方失败回落）。
    #[test]
    fn channel_candidates_are_mirror_first_constant() {
        let bases = npm_registry_bases();
        assert_eq!(bases, vec![NPM_NPMMIRROR_REGISTRY, NPM_OFFICIAL_REGISTRY]);
        let node_bases = node_dist_bases();
        assert_eq!(node_bases, vec![NODE_DIST_NPMMIRROR, NODE_DIST_OFFICIAL]);
    }
}
