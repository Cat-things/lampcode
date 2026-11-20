//! Ollama native API: `/api/chat` (NDJSON stream), `/api/tags`, `/api/pull`,
//! `/api/delete`.

use super::{Endpoint, StreamState, drive, send};
use crate::providers::quirks::Quirks;
use crate::providers::sse::ndjson_lines;
use crate::providers::text_tools::ThinkExtractor;
use crate::providers::{ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, Role, Usage};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub struct Ollama(pub Endpoint);

pub fn build_body(req: &ChatRequest, q: &Quirks) -> Value {
    let mut messages = Vec::new();
    if let Some(s) = req.system.as_deref().filter(|s| !s.is_empty()) {
        messages.push(json!({ "role": "system", "content": s }));
    }
    for m in &req.messages {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        let mut msg = json!({ "role": role, "content": m.content });
        if !m.tool_calls.is_empty() {
            msg["tool_calls"] = m
                .tool_calls
                .iter()
                .map(|c| {
                    let args: Value = serde_json::from_str(&c.arguments).unwrap_or_else(|_| json!({}));
                    json!({ "function": { "name": c.name, "arguments": args } })
                })
                .collect();
        }
        if q.send_reasoning_back
            && let Some(r) = &m.reasoning
        {
            msg["thinking"] = json!(r);
        }
        messages.push(msg);
    }
    let mut body = json!({ "model": req.model, "messages": messages, "stream": true });
    let mut options = serde_json::Map::new();
    if let Some(n) = req.max_tokens {
        options.insert("num_predict".into(), json!(n));
    }
    if let Some(t) = q.temperature.apply(req.temperature) {
        options.insert("temperature".into(), json!(t));
    }
    if !options.is_empty() {
        body["options"] = Value::Object(options);
    }
    if !req.tools.is_empty() {
        body["tools"] = req
            .tools
            .iter()
            .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.parameters } }))
            .collect();
    }
    body
}

#[derive(Default)]
pub struct OllamaStream {
    think: ThinkExtractor,
    calls: u32,
}

impl StreamState for OllamaStream {
    type Item = String;

    fn on_item(&mut self, line: String) -> Result<(Vec<ChatEvent>, bool), ProviderError> {
        let v: Value = serde_json::from_str(&line).map_err(|e| ProviderError::Other(format!("bad stream line: {e}")))?;
        if let Some(err) = v.get("error").and_then(Value::as_str) {
            return Err(ProviderError::Other(crate::security::redact(err).into_owned()));
        }
        let mut out = Vec::new();
        let msg = &v["message"];
        if let Some(t) = msg["thinking"].as_str().filter(|t| !t.is_empty()) {
            out.push(ChatEvent::ReasoningDelta(t.into()));
        }
        if let Some(c) = msg["content"].as_str().filter(|t| !t.is_empty()) {
            out.extend(self.think.push(c));
        }
        if let Some(calls) = msg["tool_calls"].as_array() {
            for c in calls {
                let index = self.calls;
                self.calls += 1;
                out.push(ChatEvent::ToolCallDelta {
                    index,
                    id: Some(format!("call_{index}")),
                    name: c.pointer("/function/name").and_then(Value::as_str).map(String::from),
                    args_delta: c.pointer("/function/arguments").map(|a| a.to_string()).unwrap_or_else(|| "{}".into()),
                });
            }
        }
        if v["done"].as_bool() == Some(true) {
            out.extend(self.think.finish());
            let n = |k: &str| v[k].as_u64().unwrap_or(0) as u32;
            out.push(ChatEvent::Usage(Usage { input: n("prompt_eval_count"), output: n("eval_count"), cached: 0, cost: Some(0.0) }));
            let reason = if self.calls > 0 { "tool_calls" } else { v["done_reason"].as_str().unwrap_or("stop") };
            out.push(ChatEvent::Done { stop_reason: reason.into() });
            return Ok((out, true));
        }
        Ok((out, false))
    }

    fn on_eof(&mut self) -> Vec<ChatEvent> {
        let mut out = self.think.finish();
        out.push(ChatEvent::Done { stop_reason: "eof".into() });
        out
    }
}

impl Ollama {
    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut h = self.0.base_headers();
        self.0.bearer(&mut h); // remote servers behind a token
        h
    }

    /// Download a model. Waits until Ollama reports it is done.
    pub async fn pull(&self, model: &str) -> Result<(), ProviderError> {
        let e = &self.0;
        let rb = e
            .client
            .request(reqwest::Method::POST, &e.url("api/pull"))
            .map(|r| r.headers(self.headers()).timeout(std::time::Duration::from_secs(6 * 3600)).json(&json!({ "model": model, "stream": false })));
        let (resp, _) = send(e, rb, &CancellationToken::new()).await?;
        let v: Value = resp.json().await.map_err(ProviderError::from_reqwest)?;
        match v.get("error").and_then(Value::as_str) {
            Some(err) => Err(ProviderError::Other(err.into())),
            None => Ok(()),
        }
    }

    pub async fn delete(&self, model: &str) -> Result<(), ProviderError> {
        let e = &self.0;
        let rb = e.client.request(reqwest::Method::DELETE, &e.url("api/delete")).map(|r| r.headers(self.headers()).json(&json!({ "model": model })));
        send(e, rb, &CancellationToken::new()).await.map(|_| ())
    }
}

#[async_trait]
impl Provider for Ollama {
    async fn chat_stream(&self, req: ChatRequest, cancel: CancellationToken) -> Result<EventStream, ProviderError> {
        let e = &self.0;
        let body = build_body(&req, &e.preset.quirks);
        let rb = e.client.request(reqwest::Method::POST, &e.url("api/chat")).map(|r| r.headers(self.headers()).json(&body));
        let (resp, _) = send(e, rb, &cancel).await?;
        Ok(drive(ndjson_lines(resp.bytes_stream()), OllamaStream::default(), vec![], cancel))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let e = &self.0;
        let rb = e.client.request(reqwest::Method::GET, &e.url("api/tags")).map(|r| r.headers(self.headers()));
        let (resp, _) = send(e, rb, &CancellationToken::new()).await?;
        let v: Value = resp.json().await.map_err(ProviderError::from_reqwest)?;
        Ok(v["models"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| {
                        Some(ModelInfo {
                            id: m["name"].as_str().or(m["model"].as_str())?.into(),
                            price_in: Some(0.0),
                            price_out: Some(0.0),
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
    use crate::providers::{ChatMessage, ToolCall};

    #[test]
    fn body_uses_options_and_object_arguments() {
        let req = ChatRequest {
            model: "llama3".into(),
            system: Some("s".into()),
            messages: vec![
                ChatMessage::user("hi"),
                ChatMessage {
                    tool_calls: vec![ToolCall { id: "call_0".into(), name: "ls".into(), arguments: r#"{"path":"."}"#.into() }],
                    ..ChatMessage::assistant("")
                },
                ChatMessage::tool_result("call_0", "a.txt"),
            ],
            max_tokens: Some(64),
            temperature: Some(0.3),
            tools: vec![],
        };
        let b = build_body(&req, &Quirks::default());
        assert_eq!(b["options"]["num_predict"], 64);
        assert!((b["options"]["temperature"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(b["messages"][2]["tool_calls"][0]["function"]["arguments"]["path"], ".");
        assert_eq!(b["messages"][3]["role"], "tool");
    }

    #[test]
    fn ndjson_stream_with_thinking_tools_and_usage() {
        let mut s = OllamaStream::default();
        let mut out = Vec::new();
        for l in [
            r#"{"message":{"role":"assistant","content":"","thinking":"hmm"},"done":false}"#,
            r#"{"message":{"role":"assistant","content":"Hi"},"done":false}"#,
            r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"ls","arguments":{"path":"."}}}]},"done":false}"#,
            r#"{"message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":12,"eval_count":4}"#,
        ] {
            let (evs, end) = s.on_item(l.into()).unwrap();
            out.extend(evs);
            if end {
                break;
            }
        }
        assert!(out.contains(&ChatEvent::ReasoningDelta("hmm".into())));
        assert!(out.contains(&ChatEvent::TextDelta("Hi".into())));
        assert!(out.iter().any(|e| matches!(e, ChatEvent::ToolCallDelta { name: Some(n), args_delta, .. } if n == "ls" && args_delta == r#"{"path":"."}"#)));
        assert!(out.contains(&ChatEvent::Usage(Usage { input: 12, output: 4, cached: 0, cost: Some(0.0) })));
        assert_eq!(out.last(), Some(&ChatEvent::Done { stop_reason: "tool_calls".into() }));
    }

    #[test]
    fn error_line_is_an_error() {
        let mut s = OllamaStream::default();
        assert!(s.on_item(r#"{"error":"model not found"}"#.into()).is_err());
    }
}
