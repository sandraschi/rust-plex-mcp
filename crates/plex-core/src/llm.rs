//! LLM chat for the dashboard: Ollama, LM Studio, and clouds over one API.
//!
//! One code path serves Ollama (`:11434`), LM Studio (`:1234`), and every
//! OpenAI-compatible cloud (`POST {base}/chat/completions`) — they all speak
//! the same shape. Anthropic rides a second, native path (`/v1/messages`,
//! `x-api-key`). [`chat`] buffers one full turn; [`chat_stream`] delivers
//! true server-sent tokens through an `on_token` callback (OpenAI-compat
//! SSE plus native Anthropic SSE) and returns the assembled reply.
//! Provider/base/model persist in the UI; API keys never persist — explicit
//! arg → `LLM_API_KEY` → `OPENAI_API_KEY`/`ANTHROPIC_API_KEY` per kind.
use crate::creds::{classify_vendor_error, with_timeout_secs};
use crate::error::DeviceError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Ollama,
    LmStudio,
    OpenAi,
    Anthropic,
    Custom,
}

impl Provider {
    pub fn parse(raw: &str) -> Result<Self, DeviceError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "ollama" => Ok(Self::Ollama),
            "lmstudio" | "lm-studio" | "lm_studio" => Ok(Self::LmStudio),
            "openai" => Ok(Self::OpenAi),
            "anthropic" | "claude" => Ok(Self::Anthropic),
            "custom" | "compatible" | "openai-compatible" => Ok(Self::Custom),
            _ => Err(DeviceError::InvalidInput(format!(
                "unknown LLM provider '{raw}': expected ollama|lmstudio|openai|anthropic|custom"
            ))),
        }
    }

    pub fn default_base(&self) -> &'static str {
        match self {
            Self::Ollama => "http://127.0.0.1:11434/v1",
            Self::LmStudio => "http://127.0.0.1:1234/v1",
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Custom => "",
        }
    }

    pub fn default_model(&self) -> &'static str {
        // gemma4:12b: blueprint-copy default per the repconv canon — fits the
        // 24 GB VRAM budget alongside residents with zero scrubbing. Override
        // per machine via the Chat page model field or LLM_MODEL env.
        match self {
            Self::Ollama => "gemma4:12b",
            Self::LmStudio => "local-model",
            Self::OpenAi => "gpt-4o-mini",
            Self::Anthropic => "claude-sonnet-4-20250514",
            Self::Custom => "",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn env_non_empty(name: &str) -> Option<String> {
    clean(std::env::var(name).ok()).or_else(|| crate::import::fleet_secret(name))
}

fn http_client() -> Result<reqwest::Client, DeviceError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| DeviceError::Protocol(format!("llm http client: {e}")))
}

pub struct ResolvedProvider {
    pub kind: Provider,
    pub base: String,
    pub model: String,
    pub api_key: Option<String>,
}

/// Explicit args > `LLM_*` env > provider defaults. Keys stay in memory.
pub fn resolve(
    provider: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
) -> Result<ResolvedProvider, DeviceError> {
    let kind = match clean(provider).or_else(|| env_non_empty("LLM_PROVIDER")) {
        Some(raw) => Provider::parse(&raw)?,
        None => Provider::Ollama,
    };
    let base = clean(base_url)
        .or_else(|| env_non_empty("LLM_BASE_URL"))
        .unwrap_or_else(|| kind.default_base().to_string());
    if base.trim().is_empty() {
        return Err(DeviceError::InvalidInput(
            "LLM base URL required for provider=custom: --base-url or LLM_BASE_URL".to_string(),
        ));
    }
    let model = clean(model)
        .or_else(|| env_non_empty("LLM_MODEL"))
        .unwrap_or_else(|| kind.default_model().to_string());
    if model.trim().is_empty() {
        return Err(DeviceError::InvalidInput(
            "LLM model required: --model or LLM_MODEL".to_string(),
        ));
    }
    let api_key = clean(api_key)
        .or_else(|| env_non_empty("LLM_API_KEY"))
        .or_else(|| {
            env_non_empty(match kind {
                Provider::Anthropic => "ANTHROPIC_API_KEY",
                _ => "OPENAI_API_KEY",
            })
        });
    if matches!(
        kind,
        Provider::OpenAi | Provider::Anthropic | Provider::Custom
    ) && api_key.is_none()
    {
        return Err(DeviceError::Auth(
            "LLM API key required for cloud providers: --api-key or LLM_API_KEY".to_string(),
        ));
    }
    Ok(ResolvedProvider {
        kind,
        base: base.trim_end_matches('/').to_string(),
        model,
        api_key,
    })
}

fn openai_body(model: &str, messages: &[ChatMessage]) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
    })
}

/// SSE line verdict for OpenAI-compatible streams.
#[derive(Debug, PartialEq)]
enum SseLine {
    /// Usable reply text.
    Token(String),
    /// `data: [DONE]` — the turn is over.
    Done,
    /// Comments, empty lines, role-only deltas — skip.
    Ignore,
}

/// Parse one SSE line from an OpenAI-compatible stream (`/chat/completions`
/// with `stream: true`, which Ollama, LM Studio, OpenAI, and Custom clones
/// all emit). Pure function — unit-tested without network.
fn openai_stream_text(line: &str) -> SseLine {
    let line = line.trim();
    let Some(data) = line.strip_prefix("data:") else {
        return SseLine::Ignore;
    };
    let data = data.trim();
    if data == "[DONE]" {
        return SseLine::Done;
    }
    let Ok(body) = serde_json::from_str::<serde_json::Value>(data) else {
        return SseLine::Ignore;
    };
    body.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("content"))
        .and_then(|c| c.as_str())
        .filter(|text| !text.is_empty())
        .map(|text| SseLine::Token(text.to_string()))
        .unwrap_or(SseLine::Ignore)
}

/// Extract reply text from one Anthropic SSE `data:` payload. Only
/// `content_block_delta` / `text_delta` blocks carry tokens; every other
/// event (`message_start`, `ping`, `message_stop`, …) yields `None`.
/// Pure function — unit-tested without network.
fn anthropic_stream_text(data: &str) -> Option<String> {
    let body: serde_json::Value = serde_json::from_str(data.trim()).ok()?;
    if body.get("type").and_then(|t| t.as_str()) != Some("content_block_delta") {
        return None;
    }
    body.get("delta")
        .and_then(|delta| {
            if delta.get("type").and_then(|t| t.as_str()) != Some("text_delta") {
                return None;
            }
            delta.get("text").and_then(|t| t.as_str())
        })
        .filter(|text| !text.is_empty())
        .map(|text| text.to_string())
}

/// True streaming chat. Calls `on_token` for every text fragment the server
/// emits and returns the assembled reply. `max_chars` caps runaway streams.
/// The callback is `Send` so callers can drive it from spawned workers.
pub async fn chat_stream(
    provider: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    messages: Vec<ChatMessage>,
    max_chars: usize,
    on_token: &mut (impl FnMut(&str) + Send),
) -> Result<String, DeviceError> {
    if messages.is_empty() {
        return Err(DeviceError::InvalidInput(
            "chat needs at least one message".to_string(),
        ));
    }
    let resolved = resolve(provider, base_url, model, api_key)?;
    let http = http_client()?;
    if resolved.kind == Provider::Anthropic {
        let key = resolved.api_key.clone().unwrap_or_default();
        let mut body = anthropic_body(&resolved.model, &messages);
        body["stream"] = true.into();
        let resp = http
            .post(format!("{}/v1/messages", resolved.base))
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|e| classify_vendor_error(format!("llm.stream: {e}")))?;
        return pump_anthropic(resp, max_chars, on_token).await;
    }
    let mut body = openai_body(&resolved.model, &messages);
    body["stream"] = true.into();
    let mut req = http
        .post(format!("{}/chat/completions", resolved.base))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .json(&body);
    if let Some(key) = resolved.api_key.clone() {
        req = req.bearer_auth(key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| classify_vendor_error(format!("llm.stream: {e}")))?;
    pump_openai(resp, max_chars, on_token).await
}

/// Cap on one streamed turn so a runaway server cannot OOM the caller.
const MAX_STREAM_CHARS: usize = 256 * 1024;

async fn check_stream_head(
    op: &str,
    resp: reqwest::Response,
) -> Result<reqwest::Response, DeviceError> {
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| DeviceError::Protocol(format!("{op} error body: {e}")))?;
        let clipped = text.chars().take(300).collect::<String>();
        return Err(DeviceError::Protocol(format!(
            "{op} HTTP {status}: {clipped}"
        )));
    }
    Ok(resp)
}

/// Drain an OpenAI-compatible SSE body, firing the callback per token.
async fn pump_openai(
    resp: reqwest::Response,
    max_chars: usize,
    on_token: &mut (impl FnMut(&str) + Send),
) -> Result<String, DeviceError> {
    let mut resp = check_stream_head("llm.stream", resp).await?;
    let mut full = String::new();
    let mut buf = String::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DeviceError::Protocol(format!("llm.stream body: {e}")))?
    {
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            match openai_stream_text(&line) {
                SseLine::Token(text) => {
                    if full.len() + text.len() > max_chars.min(MAX_STREAM_CHARS) {
                        return Err(DeviceError::Protocol(
                            "llm.stream exceeded the turn cap".to_string(),
                        ));
                    }
                    full.push_str(&text);
                    on_token(&text);
                }
                SseLine::Done => return Ok(full),
                SseLine::Ignore => {}
            }
        }
    }
    Ok(full)
}

/// Drain a native Anthropic SSE body, firing the callback per text delta.
async fn pump_anthropic(
    resp: reqwest::Response,
    max_chars: usize,
    on_token: &mut (impl FnMut(&str) + Send),
) -> Result<String, DeviceError> {
    let mut resp = check_stream_head("llm.stream", resp).await?;
    let mut full = String::new();
    let mut buf = String::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DeviceError::Protocol(format!("llm.stream body: {e}")))?
    {
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(':') {
                continue;
            }
            if let Some(data) = trimmed.strip_prefix("data:") {
                if let Some(text) = anthropic_stream_text(data) {
                    if full.len() + text.len() > max_chars.min(MAX_STREAM_CHARS) {
                        return Err(DeviceError::Protocol(
                            "llm.stream exceeded the turn cap".to_string(),
                        ));
                    }
                    full.push_str(&text);
                    on_token(&text);
                }
            }
        }
    }
    Ok(full)
}

fn openai_reply(body: &serde_json::Value) -> Result<String, DeviceError> {
    body.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| DeviceError::Protocol("llm: chat response carried no message".to_string()))
}

fn anthropic_body(model: &str, messages: &[ChatMessage]) -> serde_json::Value {
    let (system, rest): (Vec<_>, Vec<_>) = messages.iter().partition(|msg| msg.role == "system");
    let system_text = system
        .iter()
        .map(|msg| msg.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": 1024,
        "messages": rest.into_iter().map(|msg| {
            serde_json::json!({"role": if msg.role == "assistant" { "assistant" } else { "user" }, "content": msg.content})
        }).collect::<Vec<_>>(),
    });
    if !system_text.is_empty() {
        body["system"] = system_text.into();
    }
    body
}

fn anthropic_reply(body: &serde_json::Value) -> Result<String, DeviceError> {
    body.get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| {
            a.iter().find_map(|block| {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    block.get("text").and_then(|t| t.as_str())
                } else {
                    None
                }
            })
        })
        .map(|s| s.to_string())
        .ok_or_else(|| DeviceError::Protocol("llm: anthropic response carried no text".to_string()))
}

/// One chat turn. Timeouts are generous (local 27B models think slowly).
pub async fn chat(
    provider: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    messages: Vec<ChatMessage>,
) -> Result<String, DeviceError> {
    if messages.is_empty() {
        return Err(DeviceError::InvalidInput(
            "chat needs at least one message".to_string(),
        ));
    }
    let resolved = resolve(provider, base_url, model, api_key)?;
    let http = http_client()?;
    if resolved.kind == Provider::Anthropic {
        let key = resolved.api_key.clone().unwrap_or_default();
        let resp = with_timeout_secs(
            "llm.chat",
            150,
            http.post(format!("{}/v1/messages", resolved.base))
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&anthropic_body(&resolved.model, &messages))
                .send(),
        )
        .await?
        .map_err(|e| classify_vendor_error(format!("llm.chat: {e}")))?;
        return decode("llm.chat", resp, anthropic_reply).await;
    }
    let mut req = http
        .post(format!("{}/chat/completions", resolved.base))
        .header("Content-Type", "application/json")
        .json(&openai_body(&resolved.model, &messages));
    if let Some(key) = resolved.api_key.clone() {
        req = req.bearer_auth(key);
    }
    let resp = with_timeout_secs("llm.chat", 150, req.send())
        .await?
        .map_err(|e| classify_vendor_error(format!("llm.chat: {e}")))?;
    decode("llm.chat", resp, openai_reply).await
}

async fn decode(
    op: &str,
    resp: reqwest::Response,
    extract: fn(&serde_json::Value) -> Result<String, DeviceError>,
) -> Result<String, DeviceError> {
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| DeviceError::Protocol(format!("{op} body: {e}")))?;
    if !status.is_success() {
        return Err(DeviceError::Protocol(format!("{op} HTTP {status}: {text}")));
    }
    let body: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| DeviceError::Protocol(format!("{op} decode: {e}")))?;
    extract(&body)
}

/// Model list for pickers (OpenAI-compatible `GET /v1/models`).
pub async fn models(
    provider: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<Vec<String>, DeviceError> {
    let resolved = resolve(provider, base_url, Some("list".to_string()), api_key)?;
    if resolved.kind == Provider::Anthropic {
        return Err(DeviceError::Unsupported(
            "llm: Anthropic has no list-models endpoint — type the model id".to_string(),
        ));
    }
    let http = http_client()?;
    let mut req = http.get(format!("{}/models", resolved.base));
    if let Some(key) = resolved.api_key.clone() {
        req = req.bearer_auth(key);
    }
    let resp = with_timeout_secs("llm.models", 30, req.send())
        .await?
        .map_err(|e| classify_vendor_error(format!("llm.models: {e}")))?;
    if !resp.status().is_success() {
        return Err(DeviceError::Protocol(format!(
            "llm.models HTTP {}",
            resp.status()
        )));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| DeviceError::Protocol(format!("llm.models decode: {e}")))?;
    // Ollama /api/tags shape AND OpenAI /v1/models shape, both tolerated.
    let mut names = Vec::new();
    if let Some(data) = body.get("data").and_then(|v| v.as_array()) {
        for entry in data {
            if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
                names.push(id.to_string());
            }
        }
    }
    if let Some(models) = body.get("models").and_then(|v| v.as_array()) {
        for entry in models {
            if let Some(name) = entry.get("name").and_then(|v| v.as_str()) {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parse_aliases() {
        assert_eq!(Provider::parse("LM-Studio"), Ok(Provider::LmStudio));
        assert_eq!(Provider::parse("claude"), Ok(Provider::Anthropic));
        assert!(Provider::parse("llama-home-server").is_err());
    }

    #[test]
    fn openai_body_shape() {
        let body = openai_body("m", &[ChatMessage::user("hi")]);
        assert_eq!(body["model"], "m");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn anthropic_body_folds_system() {
        let body = anthropic_body(
            "m",
            &[ChatMessage::system("be brief"), ChatMessage::user("hi")],
        );
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["messages"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(body["max_tokens"], 1024);
    }

    #[test]
    fn reply_extractors_reject_garbage() {
        assert!(openai_reply(&serde_json::json!({})).is_err());
        assert!(anthropic_reply(&serde_json::json!({"content": []})).is_err());
    }

    #[test]
    fn openai_sse_lines_classify() {
        assert_eq!(
            openai_stream_text("data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}"),
            SseLine::Token("Hi".to_string())
        );
        assert_eq!(openai_stream_text("data: [DONE]"), SseLine::Done);
        assert_eq!(openai_stream_text(""), SseLine::Ignore);
        assert_eq!(openai_stream_text(": keep-alive"), SseLine::Ignore);
        assert_eq!(
            openai_stream_text("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}"),
            SseLine::Ignore
        );
        assert_eq!(openai_stream_text("data: not-json{"), SseLine::Ignore);
    }

    #[test]
    fn anthropic_sse_data_filters_to_text_deltas() {
        assert_eq!(
            anthropic_stream_text(
                "{\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}"
            ),
            Some("Hi".to_string())
        );
        assert_eq!(
            anthropic_stream_text("{\"type\":\"message_start\",\"message\":{}}"),
            None
        );
        assert_eq!(anthropic_stream_text("{\"type\":\"message_stop\"}"), None);
        assert_eq!(
            anthropic_stream_text(
                "{\"type\":\"content_block_delta\",\"delta\":{\"type\":\"input_json_delta\"}}"
            ),
            None
        );
        assert_eq!(anthropic_stream_text("garbage"), None);
    }

    /// Serve one canned SSE body split across two TCP writes so the client
    /// must reassemble a token torn across a chunk boundary.
    async fn serve_split(body: &'static str) -> Result<String, DeviceError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| DeviceError::Protocol(format!("test bind: {e}")))?;
        let addr = listener
            .local_addr()
            .map_err(|e| DeviceError::Protocol(format!("test addr: {e}")))?;
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut discard = vec![0u8; 4096];
                let _ = sock.read(&mut discard).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let bytes = body.as_bytes();
                let mid = bytes.len() / 2;
                let _ = sock.write_all(&bytes[..mid]).await;
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                let _ = sock.write_all(&bytes[mid..]).await;
            }
        });
        Ok(format!("http://{addr}"))
    }

    fn custom_args(
        url: String,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        (
            Some("custom".to_string()),
            Some(url),
            Some("test-model".to_string()),
            Some("test-key".to_string()),
        )
    }

    #[tokio::test]
    async fn openai_compat_stream_reassembles_split_chunks() -> Result<(), DeviceError> {
        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n"
        );
        let url = serve_split(sse).await?;
        let (provider, base, model, key) = custom_args(url);
        let mut tokens = Vec::new();
        let full = chat_stream(
            provider,
            base,
            model,
            key,
            vec![ChatMessage::user("hi")],
            4096,
            &mut |token| tokens.push(token.to_string()),
        )
        .await?;
        assert_eq!(full, "Hello");
        assert_eq!(tokens, vec!["Hel".to_string(), "lo".to_string()]);
        Ok(())
    }

    #[tokio::test]
    async fn anthropic_stream_extracts_text_deltas() -> Result<(), DeviceError> {
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\"}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        );
        let url = serve_split(sse).await?;
        let mut tokens = Vec::new();
        let full = chat_stream(
            Some("anthropic".to_string()),
            Some(url),
            Some("test-model".to_string()),
            Some("test-key".to_string()),
            vec![ChatMessage::user("hi")],
            4096,
            &mut |token| tokens.push(token.to_string()),
        )
        .await?;
        assert_eq!(full, "Hello");
        assert_eq!(tokens, vec!["Hel".to_string(), "lo".to_string()]);
        Ok(())
    }

    #[test]
    fn cloud_without_key_is_typed_auth() {
        let result = resolve(
            Some("openai".to_string()),
            None,
            Some("gpt-4o-mini".to_string()),
            None,
        );
        // Passes only when a real key sits in env (fleet machine with one) —
        // either outcome is a valid typed answer, never a panic.
        if std::env::var("OPENAI_API_KEY").is_err() && std::env::var("LLM_API_KEY").is_err() {
            assert!(matches!(result, Err(DeviceError::Auth(_))));
        }
    }
}
