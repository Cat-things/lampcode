//! Anthropic Messages API, also used for Anthropic-compatible endpoints (set a
//! different base URL in the preset).

use super::{Endpoint, StreamState, drive, send};
use crate::providers::quirks::Quirks;
use crate::providers::sse::{SseEvent, sse_events};
use crate::providers::{ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, Role, Usage};
use age::secrecy::ExposeSecret;
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub struct Anthropic(pub Endpoint);

pub const DEFAULT_VERSION: &str = "2023-06-01";
/// The Messages API requires `max_tokens`.
pub const DEFAULT_MAX_TOKENS: u32 = 8192;

fn ephemeral() -> Value {
    json!({ "type": "ephemeral" })
}

pub fn build_body(req: &ChatRequest, q: &Quirks) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    for m in &req.messages {
        match m.role {
            Role::User => messages.push(json!({ "role": "user", "content": [{ "type": "text", "text": m.content }] })),
            Role::Assistant => {
                let mut blocks = Vec::new();
                if !m.content.is_empty() {
                    blocks.push(json!({ "type": "text", "text": m.content }));
                }
                for c in &m.tool_calls {
                    let input: Value = serde_json::from_str(&c.arguments).unwrap_or_else(|_| json!({}));
                    blocks.push(json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": input }));
                }
                if !blocks.is_empty() {
                    messages.push(json!({ "role": "assistant", "content": blocks }));
                }
            }
            Role::Tool => {
                let block = json!({ "type": "tool_result", "tool_use_id": m.tool_call_id, "content": m.content });
                // Results for one assistant turn go together in one user message.
                match messages.last_mut() {
                    Some(last)
                        if last["role"] == "user"
                            && last["content"]
                                .as_array()
                                .is_some_and(|a| a.iter().all(|b| b["type"] == "tool_result")) =>
                    {
                        last["content"].as_array_mut().unwrap().push(block)
                    }
                    _ => messages.push(json!({ "role": "user", "content": [block] })),
                }
            }
        }
    }
    // Cache breakpoint on the last block of the last user turn.
    if let Some(last_user) = messages.iter_mut().rev().find(|m| m["role"] == "user")
        && let Some(block) = last_user["content"].as_array_mut().and_then(|a| a.last_mut())
    {
        block["cache_control"] = ephemeral();
    }
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "messages": messages,
        "stream": true,
    });
    if let Some(s) = req.system.as_deref().filter(|s| !s.is_empty()) {
        body["system"] = json!([{ "type": "text", "text": s, "cache_control": ephemeral() }]);
    }
    if let Some(t) = q.temperature.apply(req.temperature) {
        body["temperature"] = json!(t);
    }
    if !req.tools.is_empty() {
        body["tools"] = req
            .tools
            .iter()
            .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters }))
            .collect();
    }
    body
}

#[derive(Default)]
pub struct AnthropicStream {
    usage: Usage,
    stop_reason: Option<String>,
}

impl StreamState for AnthropicStream {
    type Item = SseEvent;

    fn on_item(&mut self, ev: SseEvent) -> Result<(Vec<ChatEvent>, bool), ProviderError> {
        if ev.data.trim().is_empty() {
            return Ok((vec![], false));
        }
        let v: Value =
            serde_json::from_str(&ev.data).map_err(|e| ProviderError::Other(format!("bad stream chunk: {e}")))?;
        let kind = v
            .get("type")
            .and_then(Value::as_str)
            .or(ev.event.as_deref())
            .unwrap_or("");
        let n = |p: &str| v.pointer(p).and_then(Value::as_u64).map(|x| x as u32);
        let mut out = Vec::new();
        match kind {
            "message_start" => {
                self.usage.input = n("/message/usage/input_tokens").unwrap_or(0);
                self.usage.cached = n("/message/usage/cache_read_input_tokens").unwrap_or(0);
            }
            "content_block_start" => {
                let index = n("/index").unwrap_or(0);
                let block = &v["content_block"];
                match block["type"].as_str() {
                    Some("tool_use") => out.push(ChatEvent::ToolCallDelta {
                        index,
                        id: block["id"].as_str().map(String::from),
                        name: block["name"].as_str().map(String::from),
                        args_delta: String::new(),
                    }),
                    Some("text") => {
                        if let Some(t) = block["text"].as_str().filter(|t| !t.is_empty()) {
                            out.push(ChatEvent::TextDelta(t.into()));
                        }
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let index = n("/index").unwrap_or(0);
                let d = &v["delta"];
                match d["type"].as_str() {
                    Some("text_delta") => out.push(ChatEvent::TextDelta(d["text"].as_str().unwrap_or("").into())),
                    Some("thinking_delta") => {
                        out.push(ChatEvent::ReasoningDelta(d["thinking"].as_str().unwrap_or("").into()))
                    }
                    Some("input_json_delta") => out.push(ChatEvent::ToolCallDelta {
                        index,
                        id: None,
                        name: None,
                        args_delta: d["partial_json"].as_str().unwrap_or("").into(),
                    }),
                    _ => {} // signature_delta and future types
                }
            }
            "message_delta" => {
                if let Some(r) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(r.into());
                }
                if let Some(o) = n("/usage/output_tokens") {
                    self.usage.output = o;
                }
                out.push(ChatEvent::Usage(self.usage));
            }
            "message_stop" => {
                out.push(self.done());
                return Ok((out, true));
            }
            "error" => {
                let t = v.pointer("/error/type").and_then(Value::as_str).unwrap_or("");
                let msg = v
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("stream error");
                let status = match t {
                    "overloaded_error" => 529,
                    "rate_limit_error" => 429,
                    "authentication_error" => 401,
                    "invalid_request_error" => 400,
                    _ => 500,
                };
                return Err(ProviderError::from_status(status, msg, None));
            }
            _ => {} // ping and unknown events
        }
        Ok((out, false))
    }

    fn on_eof(&mut self) -> Vec<ChatEvent> {
        vec![self.done()]
    }
}

impl AnthropicStream {
    fn done(&self) -> ChatEvent {
        let r = match self.stop_reason.as_deref() {
            Some("tool_use") => "tool_calls",
            Some("end_turn") | Some("stop_sequence") => "stop",
            Some("max_tokens") => "length",
            Some(other) => other,
            None => "eof",
        };
        ChatEvent::Done { stop_reason: r.into() }
    }
}

impl Anthropic {
    fn headers(&self) -> reqwest::header::HeaderMap {
        let e = &self.0;
        let mut h = e.base_headers();
        if let Some(k) = &e.key
            && let Some(v) = Endpoint::secret_header(k.expose_secret())
        {
            h.insert("x-api-key", v);
        }
        let version = e.preset.quirks.anthropic_version.as_deref().unwrap_or(DEFAULT_VERSION);
        if let Ok(v) = reqwest::header::HeaderValue::from_str(version) {
            h.insert("anthropic-version", v);
        }
        h
    }
}

#[async_trait]
impl Provider for Anthropic {
    async fn chat_stream(&self, req: ChatRequest, cancel: CancellationToken) -> Result<EventStream, ProviderError> {
        let e = &self.0;
        e.require_key()?;
        let body = build_body(&req, &e.preset.quirks);
        let rb = e
            .client
            .request(reqwest::Method::POST, &e.url("v1/messages"))
            .map(|r| r.headers(self.headers()).json(&body));
        let (resp, info) = send(e, rb, &cancel).await?;
        let first = if info.is_empty() {
            vec![]
        } else {
            vec![ChatEvent::RateLimit(info)]
        };
        Ok(drive(
            sse_events(resp.bytes_stream()),
            AnthropicStream::default(),
            first,
            cancel,
        ))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let e = &self.0;
        e.require_key()?;
        let rb = e
            .client
            .request(reqwest::Method::GET, &e.url("v1/models?limit=1000"))
            .map(|r| r.headers(self.headers()));
        let (resp, _) = send(e, rb, &CancellationToken::new()).await?;
        let v: Value = resp.json().await.map_err(ProviderError::from_reqwest)?;
        Ok(v["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| {
                        Some(ModelInfo {
                            id: m["id"].as_str()?.into(),
                            name: m["display_name"].as_str().map(String::from),
                            ..Default::default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ChatMessage, ToolCall, ToolSpec};

    fn ev(data: &str) -> SseEvent {
        SseEvent {
            event: None,
            data: data.into(),
        }
    }

    #[test]
    fn cache_control_on_system_and_last_user_turn() {
        let req = ChatRequest {
            model: "claude".into(),
            system: Some("sys".into()),
            messages: vec![
                ChatMessage::user("one"),
                ChatMessage::assistant("two"),
                ChatMessage::user("three"),
            ],
            ..Default::default()
        };
        let b = build_body(&req, &Quirks::default());
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(b["messages"][2]["content"][0]["cache_control"]["type"], "ephemeral");
        assert!(b["messages"][0]["content"][0].get("cache_control").is_none());
        assert_eq!(b["max_tokens"], DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn tool_use_and_tool_result_mapping() {
        let req = ChatRequest {
            model: "claude".into(),
            messages: vec![
                ChatMessage::user("list"),
                ChatMessage {
                    tool_calls: vec![
                        ToolCall {
                            id: "tu_1".into(),
                            name: "ls".into(),
                            arguments: r#"{"path":"."}"#.into(),
                        },
                        ToolCall {
                            id: "tu_2".into(),
                            name: "ls".into(),
                            arguments: r#"{"path":"src"}"#.into(),
                        },
                    ],
                    ..ChatMessage::assistant("")
                },
                ChatMessage::tool_result("tu_1", "a"),
                ChatMessage::tool_result("tu_2", "b"),
            ],
            tools: vec![ToolSpec {
                name: "ls".into(),
                description: "d".into(),
                parameters: json!({"type":"object"}),
            }],
            ..Default::default()
        };
        let b = build_body(&req, &Quirks::default());
        assert_eq!(b["tools"][0]["input_schema"]["type"], "object");
        let asst = &b["messages"][1];
        assert_eq!(asst["content"].as_array().unwrap().len(), 2, "no empty text block");
        assert_eq!(asst["content"][0]["input"]["path"], ".");
        let results = b["messages"][2]["content"].as_array().unwrap();
        assert_eq!(b["messages"].as_array().unwrap().len(), 3);
        assert_eq!(results.len(), 2);
        assert_eq!(results[1]["tool_use_id"], "tu_2");
    }

    #[test]
    fn thinking_text_and_tool_stream() {
        let mut s = AnthropicStream::default();
        let mut out = Vec::new();
        for d in [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":20,"cache_read_input_tokens":5}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"plan"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"x"}}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Sure"}}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"tu_1","name":"ls","input":{}}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\".\"}"}}"#,
            r#"{"type":"ping"}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}"#,
            r#"{"type":"message_stop"}"#,
        ] {
            let (evs, end) = s.on_item(ev(d)).unwrap();
            out.extend(evs);
            if end {
                break;
            }
        }
        assert!(out.contains(&ChatEvent::ReasoningDelta("plan".into())));
        assert!(out.contains(&ChatEvent::TextDelta("Sure".into())));
        assert!(out.contains(&ChatEvent::ToolCallDelta {
            index: 2,
            id: Some("tu_1".into()),
            name: Some("ls".into()),
            args_delta: "".into()
        }));
        assert!(out.contains(&ChatEvent::Usage(Usage {
            input: 20,
            output: 9,
            cached: 5,
            cost: None
        })));
        assert_eq!(
            out.last(),
            Some(&ChatEvent::Done {
                stop_reason: "tool_calls".into()
            })
        );
    }

    #[test]
    fn error_event_maps_status() {
        let mut s = AnthropicStream::default();
        let e = s
            .on_item(ev(
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            ))
            .unwrap_err();
        assert!(matches!(e, ProviderError::Server { status: 529, .. }));
    }

    #[test]
    fn eof_without_message_stop() {
        let mut s = AnthropicStream::default();
        s.on_item(ev(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x"}}"#,
        ))
        .unwrap();
        assert_eq!(
            s.on_eof(),
            vec![ChatEvent::Done {
                stop_reason: "eof".into()
            }]
        );
    }
}
