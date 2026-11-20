//! Provider clients. Every protocol adapter turns a [`ChatRequest`] into a
//! stream of [`ChatEvent`]s; presets add per-provider quirks on top.

pub mod adapters;
pub mod catalog;
pub mod hub;
pub mod preset;
pub mod quirks;
pub mod ratelimit;
pub mod sse;
pub mod text_tools;

use crate::security::redact;
use async_trait::async_trait;
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

pub use preset::{Preset, PresetRegistry, Protocol};
pub use ratelimit::RateLimitInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    /// Result of a tool call, answering `tool_call_id`.
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// JSON text of the arguments, exactly as the model produced it.
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self { role: Role::User, content: text.into(), reasoning: None, tool_calls: vec![], tool_call_id: None }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self { role: Role::Assistant, ..Self::user(text) }
    }
    pub fn tool_result(call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self { role: Role::Tool, tool_call_id: Some(call_id.into()), ..Self::user(text) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments object.
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input: u32,
    pub output: u32,
    pub cached: u32,
    /// USD, when the provider reports it directly.
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    TextDelta(String),
    ReasoningDelta(String),
    /// Pieces of a tool call. `id` and `name` arrive on the first piece of each
    /// `index`; `args_delta` pieces are concatenated in order.
    ToolCallDelta { index: u32, id: Option<String>, name: Option<String>, args_delta: String },
    Usage(Usage),
    RateLimit(RateLimitInfo),
    Done { stop_reason: String },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: Option<String>,
    pub context: Option<u32>,
    /// USD per 1M tokens, when the provider's model list includes it.
    pub price_in: Option<f64>,
    pub price_out: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProviderError {
    #[error("rate limited{}", retry_after.map(|s| format!(", retry in {s}s")).unwrap_or_default())]
    RateLimited { retry_after: Option<u64> },
    #[error("the provider rejected the key: {0}")]
    Auth(String),
    #[error("provider error {status}: {body}")]
    Server { status: u16, body: String },
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("timed out")]
    Timeout,
    #[error("blocked: {0} is not on the outbound allowlist")]
    Blocked(String),
    #[error("cancelled")]
    Cancelled,
    #[error("no API key set for {0}")]
    NoKey(String),
    #[error("{0}")]
    Other(String),
}

impl ProviderError {
    /// Text safe to show in the UI or a log: always redacted and bounded.
    pub fn display_redacted(&self) -> String {
        let s = self.to_string();
        let s = redact(&s).into_owned();
        if s.chars().count() > 600 { s.chars().take(600).collect::<String>() + "..." } else { s }
    }

    pub fn from_status(status: u16, body: &str, retry_after: Option<u64>) -> Self {
        // Error bodies can echo request data, so they are redacted before they
        // are stored anywhere.
        let body: String = redact(body).chars().take(2000).collect();
        match status {
            429 => ProviderError::RateLimited { retry_after },
            401 | 403 => ProviderError::Auth(body),
            400 | 404 | 422 => ProviderError::BadRequest(body),
            s => ProviderError::Server { status: s, body },
        }
    }

    pub fn from_reqwest(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            ProviderError::Timeout
        } else {
            // reqwest errors include the URL; strip the query in case a key is there.
            ProviderError::Other(redact(&e.without_url().to_string()).into_owned())
        }
    }
}

pub type EventStream = BoxStream<'static, Result<ChatEvent, ProviderError>>;

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat_stream(&self, req: ChatRequest, cancel: CancellationToken) -> Result<EventStream, ProviderError>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_redacted() {
        let key = ["sk-", "proj-", &"A1b2C3d4".repeat(6)].concat();
        let e = ProviderError::from_status(401, &format!("invalid key {key}"), None);
        let shown = e.display_redacted();
        assert!(!shown.contains(&key), "{shown}");
        assert!(shown.contains("[REDACTED"));
        let e = ProviderError::Server { status: 500, body: format!("echo: Bearer {key}") };
        assert!(!e.display_redacted().contains(&key));
    }

    #[test]
    fn status_mapping() {
        assert_eq!(ProviderError::from_status(429, "", Some(7)), ProviderError::RateLimited { retry_after: Some(7) });
        assert!(matches!(ProviderError::from_status(403, "no", None), ProviderError::Auth(_)));
        assert!(matches!(ProviderError::from_status(503, "", None), ProviderError::Server { status: 503, .. }));
        assert_eq!(ProviderError::RateLimited { retry_after: Some(3) }.to_string(), "rate limited, retry in 3s");
    }

    #[test]
    fn long_errors_are_truncated() {
        let e = ProviderError::Other("x".repeat(5000));
        assert!(e.display_redacted().len() < 700);
    }
}
