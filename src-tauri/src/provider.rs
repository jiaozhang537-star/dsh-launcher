//! 模型列表拉取（FR-06「模型拉取」/ FR-20 多接口形态）：按提供方目录的
//! `style` 分派——OpenAI 兼容 `GET {base}/models`（Bearer）、Anthropic
//! `GET {base}/models`（x-api-key + anthropic-version）、Gemini
//! `GET {base}/models`（x-goog-api-key，名字带 `models/` 前缀）、Ollama
//! `GET {base}/api/tags`（无鉴权）。
//!
//! 地址来自提供方卡片（空 = 该厂商目录默认地址），Key 由壳从保险库解密注入
//! 请求头，明文不出后端。

use std::time::Duration;

use crate::providers::ApiStyle;

/// 从 OpenAI 兼容响应解析模型 id：trim、去空、按首次出现去重。
pub fn parse_models_json(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(data) = value.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for item in data {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let trimmed = id.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// 从 Gemini `GET {base}/models` 响应解析：`{"models":[{"name":"models/x"}]}`，
/// 名字剥掉 `models/` 前缀。
pub fn parse_gemini_json(text: &str) -> Vec<String> {
    list_of_names(text, "models", "name")
        .into_iter()
        .map(|n| n.strip_prefix("models/").unwrap_or(&n).to_string())
        .collect()
}

/// 从 Ollama `GET {base}/api/tags` 响应解析：`{"models":[{"name":"llama3"}]}`。
pub fn parse_ollama_json(text: &str) -> Vec<String> {
    list_of_names(text, "models", "name")
}

/// 通用 `{ "models": [ { <key>: <value> } ] }` 数组字段提取。
fn list_of_names(text: &str, array_key: &str, name_key: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(items) = value.get(array_key).and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for item in items {
        let Some(id) = item.get(name_key).and_then(|v| v.as_str()) else {
            continue;
        };
        let trimmed = id.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// 按接口形态拉取模型列表（阻塞调用——command 侧放 spawn_blocking）。
/// Ollama 允许空 Key（本地服务无鉴权）；其余形态要求非空 Key。
pub fn fetch_models_style(base_url: &str, key: &str, style: ApiStyle) -> Result<Vec<String>, String> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("提供方地址为空".into());
    }
    let key = key.trim();
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build();
    let (url, parse): (String, fn(&str) -> Vec<String>) = match style {
        ApiStyle::Openai | ApiStyle::Responses => {
            if key.is_empty() {
                return Err("尚未保存该厂商的 API Key".into());
            }
            (format!("{base}/models"), parse_models_json)
        }
        ApiStyle::Anthropic => {
            if key.is_empty() {
                return Err("尚未保存该厂商的 API Key".into());
            }
            (format!("{base}/models"), parse_models_json)
        }
        ApiStyle::Gemini => {
            if key.is_empty() {
                return Err("尚未保存该厂商的 API Key".into());
            }
            (format!("{base}/models"), parse_gemini_json)
        }
        ApiStyle::Ollama => (format!("{base}/api/tags"), parse_ollama_json),
    };
    let mut request = agent.get(&url).set("User-Agent", "dsh-launcher");
    request = match style {
        ApiStyle::Openai | ApiStyle::Responses => {
            request.set("Authorization", &format!("Bearer {key}"))
        }
        ApiStyle::Anthropic => request
            .set("x-api-key", key)
            .set("anthropic-version", "2023-06-01"),
        ApiStyle::Gemini => request.set("x-goog-api-key", key),
        ApiStyle::Ollama => request,
    };
    let response = request.call().map_err(|e| format!("请求 {url} 失败：{e}"))?;
    let text = response
        .into_string()
        .map_err(|e| format!("读取响应失败：{e}"))?;
    let models = parse(&text);
    if models.is_empty() {
        return Err(format!(
            "响应中没有模型——请检查地址（当前 {url}）与 Key 是否匹配"
        ));
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_style_model_list() {
        let text = serde_json::json!({
            "object": "list",
            "data": [
                { "id": " deepseek-v4-pro ", "object": "model" },
                { "id": "deepseek-v4-flash" },
                { "id": "deepseek-v4-pro" },
                { "id": "" },
                { "nope": true }
            ]
        })
        .to_string();
        assert_eq!(
            parse_models_json(&text),
            vec!["deepseek-v4-pro".to_string(), "deepseek-v4-flash".to_string()]
        );
    }

    #[test]
    fn garbage_or_missing_data_yields_empty() {
        assert!(parse_models_json("not json").is_empty());
        assert!(parse_models_json("{\"object\":\"list\"}").is_empty());
    }

    #[test]
    fn parses_gemini_names_with_prefix_stripped() {
        let text = serde_json::json!({
            "models": [
                { "name": "models/gemini-2.5-pro", "displayName": "Gemini 2.5 Pro" },
                { "name": "models/gemini-2.5-flash" },
                { "name": "models/gemini-2.5-pro" },
                { "name": "" }
            ]
        })
        .to_string();
        assert_eq!(
            parse_gemini_json(&text),
            vec!["gemini-2.5-pro".to_string(), "gemini-2.5-flash".to_string()]
        );
        assert!(parse_gemini_json("not json").is_empty());
    }

    #[test]
    fn parses_ollama_tags() {
        let text = serde_json::json!({
            "models": [
                { "name": "llama3:latest", "size": 4700000000u64 },
                { "name": "qwen2.5:7b" },
                { "nope": true }
            ]
        })
        .to_string();
        assert_eq!(parse_ollama_json(&text), vec!["llama3:latest".to_string(), "qwen2.5:7b".to_string()]);
        assert!(parse_ollama_json("garbage").is_empty());
    }

    #[test]
    fn empty_key_rejected_except_ollama() {
        assert!(fetch_models_style("https://api.openai.com/v1", "  ", ApiStyle::Openai).is_err());
        assert!(fetch_models_style("https://api.openai.com/v1", "  ", ApiStyle::Responses).is_err());
        assert!(fetch_models_style("https://api.anthropic.com/v1", "", ApiStyle::Anthropic).is_err());
        assert!(fetch_models_style("https://x", "", ApiStyle::Gemini).is_err());
        // Ollama 空 Key 允许（本地无鉴权）；连不上本地服务时报网络错误而非 Key 错误。
        let err = fetch_models_style("http://127.0.0.1:9", "", ApiStyle::Ollama).unwrap_err();
        assert!(err.contains("请求"), "实际：{err}");
    }

    /// Responses 形态与 OpenAI 兼容层同构（同 URL + Bearer）：本地 mock 实测
    /// 请求头与解析路径一致（R5 新增形态的锚点）。
    #[test]
    fn responses_fetch_matches_openai_shape_against_local_mock() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            use std::io::{Read, Write};
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            assert!(request.contains("GET /models"), "Responses 形态应请求 {{base}}/models：{request}");
            assert!(request.contains("Authorization: Bearer sk-resp-test"), "Responses 形态应带 Bearer 头");
            let body = r#"{"object":"list","data":[{"id":"gpt-5.2"},{"id":"gpt-5.2-mini"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        });
        let models =
            fetch_models_style(&format!("http://127.0.0.1:{port}"), "sk-resp-test", ApiStyle::Responses).unwrap();
        assert_eq!(models, vec!["gpt-5.2".to_string(), "gpt-5.2-mini".to_string()]);
        server.join().unwrap();
    }

    /// 本地 mock 服务器实测 Ollama 形态全链（真实 HTTP 往返：请求构造 →
    /// 响应解析）。FR-20 验收「Ollama 形态实测可用」的离线证据。
    #[test]
    fn ollama_fetch_against_local_mock() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            use std::io::{Read, Write};
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf); // 请求行/头（内容不校验，够触发即可）
            let body = r#"{"models":[{"name":"llama3:latest"},{"name":"qwen2.5:7b"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        });
        let models = fetch_models_style(&format!("http://127.0.0.1:{port}"), "", ApiStyle::Ollama).unwrap();
        assert_eq!(models, vec!["llama3:latest".to_string(), "qwen2.5:7b".to_string()]);
        server.join().unwrap();
    }
}
