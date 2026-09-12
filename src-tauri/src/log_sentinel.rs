//! 日志哨兵（FR-16 后半）：订阅实例控制台输出行流，把 DSH 本体的插件
//! 加载报错翻译成可读诊断卡片。纯读侧——只匹配、只上报，绝不干预进程。
//!
//! 匹配规则来自 R-2.2 侦察（官方源码 + 真实启动样本）：
//! 1. `dsh: fatal load failure: <stack>` —— 启动期致命失败（stderr，随后 exit 1）；
//! 2. `dsh: <N> entry/entries did not activate` —— 激活审计汇总；
//! 3. `failed to <stage> loader entry <id> (<name>): <detail>` —— 加载器条目失败；
//! 4. `<name>: pending (waiting for service(s): X, Y)` —— 插件等待缺失依赖服务。
//!
//! 去重：同一指纹（级别+标题）只报一次，避免长会话刷屏。

use std::collections::HashSet;

/// 一张诊断卡片。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// "error" | "warning"。
    pub level: String,
    /// 卡片标题（人类可读结论）。
    pub title: String,
    /// 原始日志行（证据）。
    pub detail: String,
}

/// 对单行控制台输出做匹配；命中返回诊断卡片。
pub fn inspect(line: &str) -> Option<Diagnostic> {
    let detail = line.trim_end();
    if detail.is_empty() {
        return None;
    }

    if detail.contains("fatal load failure:") {
        return Some(Diagnostic {
            level: "error".into(),
            title: "DSH 启动失败：插件初始化被拒绝".into(),
            detail: detail.to_string(),
        });
    }
    if detail.contains("did not activate") {
        return Some(Diagnostic {
            level: "error".into(),
            title: "有插件未激活".into(),
            detail: detail.to_string(),
        });
    }
    if detail.contains("loader entry") && detail.contains("failed to ") {
        return Some(Diagnostic {
            level: "error".into(),
            title: "插件加载失败".into(),
            detail: detail.to_string(),
        });
    }
    if detail.contains("pending (waiting for service") {
        let subject = detail.split(':').next().unwrap_or(detail).trim().to_string();
        return Some(Diagnostic {
            level: "warning".into(),
            title: format!("插件 {subject} 正在等待依赖服务"),
            detail: detail.to_string(),
        });
    }
    None
}

/// 每实例一份的去重会话；指纹 = (level, title)。
#[derive(Default)]
pub struct SentinelSession {
    seen: HashSet<(String, String)>,
}

impl SentinelSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// 送入一行；首次命中的诊断返回 Some，重复指纹返回 None。
    pub fn feed(&mut self, line: &str) -> Option<Diagnostic> {
        let diagnostic = inspect(line)?;
        let fingerprint = (diagnostic.level.clone(), diagnostic.title.clone());
        if self.seen.insert(fingerprint) {
            Some(diagnostic)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fatal_load_failure_is_error() {
        let d = inspect("dsh: fatal load failure: Error: failed to load loader entry webserver (@deepseek-ai/dsh-host-webserver): listen EADDRINUSE").unwrap();
        assert_eq!(d.level, "error");
        assert!(d.detail.contains("EADDRINUSE"));
    }

    #[test]
    fn activation_summary_is_error() {
        let d = inspect(
            "dsh: 1 entries did not activate",
        )
        .unwrap();
        assert_eq!(d.level, "error");
        assert_eq!(d.title, "有插件未激活");
    }

    #[test]
    fn loader_entry_failure_is_error_with_detail() {
        let d = inspect(
            "Error: failed to apply loader entry webserver (@deepseek-ai/dsh-host-webserver): duplicate route",
        )
        .unwrap();
        assert_eq!(d.level, "error");
        assert_eq!(d.title, "插件加载失败");
    }

    /// R7-2/真机证据 T3a（0.1.5-rc.1）：插件 patch 声明了、入口 import 抛异常时，
    /// DSH 的报错原文长这样——启动器必须把它认成「插件加载失败」并给出插件名，
    /// 否则用户在加载页只会看到「已异常退出（exit 1）」。
    #[test]
    fn real_plugin_boom_line_is_error_naming_plugin() {
        let d = inspect(
            "Error: dsh: plugin tree failed to load: failed to apply loader entry include (cordis:include): \
             failed to import loader entry computer-use (@anionex/dsh-computer-use): r7probe activation boom",
        )
        .unwrap();
        assert_eq!(d.level, "error");
        assert_eq!(d.title, "插件加载失败");
        assert!(d.detail.contains("@anionex/dsh-computer-use"), "详情应带插件名：{}", d.detail);
    }

    #[test]
    fn pending_service_wait_is_warning_naming_subject() {        let d =
            inspect("my-fancy-plugin: pending (waiting for services: llm)").unwrap();
        assert_eq!(d.level, "warning");
        assert!(d.title.contains("my-fancy-plugin"), "标题应含插件名：{}", d.title);
    }

    #[test]
    fn ordinary_lines_and_runtime_logs_pass_through() {
        // 真实启动样本（R-2.2）：普通传输错误与 URL 行不触发。
        assert!(inspect("dsh: TRANSPORT: DeepSeek API request to http://example-invalid.test failed").is_none());
        assert!(inspect("").is_none());
        assert!(inspect("   ").is_none());
        assert!(inspect("[INFO] server ready on port 3080").is_none());
    }

    #[test]
    fn session_dedupes_by_fingerprint_but_allows_new_kinds() {
        let mut session = SentinelSession::new();
        let line_a = "dsh: fatal load failure: boom";
        let line_a2 = "dsh: fatal load failure: boom again";
        let line_b = "worker: pending (waiting for service: shell)";

        assert!(session.feed(line_a).is_some(), "首报");
        assert!(session.feed(line_a2).is_none(), "同指纹去重");
        assert!(session.feed(line_b).is_some(), "不同种类仍报");
        assert!(session.feed(line_b).is_none());
    }
}
