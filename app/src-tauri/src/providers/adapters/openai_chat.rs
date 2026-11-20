//! OpenAI Chat Completions and everything compatible with it.

use super::{Endpoint, StreamState, drive, json_path_f64, send};
use crate::providers::quirks::{Quirks, ReasoningLoc, ToolSupport};
use crate::providers::sse::{SseEvent, sse_events};
use crate::providers::text_tools::{TextToolParser, ThinkExtractor, render_call, tool_instructions};
use crate::providers::{ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, Role, Usage};
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub struct OpenAiChat(pub Endpoint);

pub fn build_body(req: &ChatRequest, q: &Quirks) -> Value {
    let text_tools = q.tools == ToolSupport::None && !req.tools.is_empty();
    let mut messages = Vec::new();
    let mut system = req.system.clone().unwrap_or_default();
    if text_tools {
        if !system.is_empty() {
            system.push_str("\n\n");
        }
        system.push_str(&tool_instructions(&req.tools));
    }
    if !system.is_empty() {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for m in &req.messages {
        match m.role {
            Role::User => messages.push(json!({ "role": "user", "content": m.content })),
            Role::Assistant => {
                let mut content = m.content.clone();
                let mut msg = json!({ "role": "assistant" });
                if text_tools {
                    for c in &m.tool_calls {
                        content.push_str(&render_call(&c.name, &c.arguments));
                    }
                } else if !m.tool_calls.is_empty() {
                    msg["tool_calls"] = m
                        .tool_calls
                        .iter()
                        .map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments } }))
                        .collect();
                }
                msg["content"] = if content.is_empty() && msg.get("tool_calls").is_some() {
                    Value::Null
                } else {
                    json!(content)
                };
                if q.send_reasoning_back
                    && let Some(r) = &m.reasoning
                {
                    msg["reasoning_content"] = json!(r);
                }
                messages.push(msg);
            }
            Role::Tool => {
                if text_tools {
                    messages.push(json!({ "role": "user", "content": format!("Tool result:\n{}", m.content) }));
                } else {
                    messages.push(json!({ "role": "tool", "tool_call_id": m.tool_call_id, "content": m.content }));
                }
            }
        }
    }
    let mut body = json!({ "model": req.model, "messages": messages, "stream": true });
    if let Some(n) = req.max_tokens {
        body[q.max_tokens_field.key()] = json!(n);
    }
    if let Some(t) = q.temperature.apply(req.temperature) {
        body["temperature"] = json!(t);
    }
    if q.stream_usage {
        body["stream_options"] = json!({ "include_usage": true });
    }
    if !req.tools.is_empty() && !text_tools {
        body["tools"] = req
            .tools
            .iter()
            .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.parameters } }))
            .collect();
        if q.tools == ToolSupport::NoParallel {
            body["parallel_tool_calls"] = json!(false);
        }
    }
    body
}

pub struct OpenAiStream {
    reasoning: ReasoningLoc,
    cost_field: Option<String>,
    think: Option<ThinkExtractor>,
    text_tools: Option<TextToolParser>,
    finish_reason: Option<String>,
}

impl OpenAiStream {
    pub fn new(q: &Quirks, text_tools: bool) -> Self {
        Self {
            reasoning: q.reasoning,
            cost_field: q.cost_field.clone(),
            think: matches!(q.reasoning, ReasoningLoc::Auto | ReasoningLoc::ThinkTags).then(ThinkExtractor::default),
            text_tools: text_tools.then(TextToolParser::default),
            finish_reason: None,
        }
    }

    fn content(&mut self, text: &str, out: &mut Vec<ChatEvent>) {
        let evs = match &mut self.think {
            Some(t) => t.push(text),
            None => vec![ChatEvent::TextDelta(text.to_string())],
        };
        self.route(evs, out);
    }

    /// Text deltas go through the text tool parser when it is active.
    fn route(&mut self, evs: Vec<ChatEvent>, out: &mut Vec<ChatEvent>) {
        for e in evs {
            match (&mut self.text_tools, e) {
                (Some(p), ChatEvent::TextDelta(t)) => out.extend(p.push(&t)),
                (_, e) => out.push(e),
            }
        }
    }

    fn usage(&self, u: &Value) -> Usage {
        let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
        Usage {
            input: n("prompt_tokens"),
            output: n("completion_tokens"),
            cached: u
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            cost: self.cost_field.as_deref().and_then(|p| json_path_f64(u, p)),
        }
    }

    fn done(&mut self) -> Vec<ChatEvent> {
        let mut out = Vec::new();
        if let Some(t) = &mut self.think {
            let evs = t.finish();
            self.route(evs, &mut out);
        }
        if let Some(p) = &mut self.text_tools {
            out.extend(p.finish());
            if p.calls_made() > 0 && self.finish_reason.as_deref() != Some("length") {
                self.finish_reason = Some("tool_calls".into());
            }
        }
        out.push(ChatEvent::Done {
            stop_reason: self.finish_reason.clone().unwrap_or_else(|| "eof".into()),
        });
        out
    }
}

impl StreamState for OpenAiStream {
    type Item = SseEvent;

    fn on_item(&mut self, ev: SseEvent) -> Result<(Vec<ChatEvent>, bool), ProviderError> {
        let data = ev.data.trim();
        if data == "[DONE]" {
            return Ok((self.done(), true));
        }
        if data.is_empty() {
            return Ok((vec![], false));
        }
        let v: Value =
            serde_json::from_str(data).map_err(|e| ProviderError::Other(format!("bad stream chunk: {e}")))?;
        if let Some(err) = v.get("error") {
            let msg = err.get("message").and_then(Value::as_str).unwrap_or("stream error");
            let code = err.get("code").and_then(Value::as_u64).unwrap_or(500) as u16;
            return Err(ProviderError::from_status(code, msg, None));
        }
        let mut out = Vec::new();
        if let Some(choice) = v.get("choices").and_then(|c| c.get(0)) {
            let delta = choice.get("delta").unwrap_or(&Value::Null);
            let field = |k: &str| delta.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
            let reasoning = match self.reasoning {
                ReasoningLoc::Auto => field("reasoning_content").or_else(|| field("reasoning")),
                ReasoningLoc::ReasoningContent => field("reasoning_content"),
                ReasoningLoc::Reasoning => field("reasoning"),
                _ => None,
            };
            if let Some(r) = reasoning {
                out.push(ChatEvent::ReasoningDelta(r.to_string()));
            }
            if let Some(text) = field("content") {
                self.content(text, &mut out);
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for (pos, c) in calls.iter().enumerate() {
                    out.push(ChatEvent::ToolCallDelta {
                        index: c.get("index").and_then(Value::as_u64).unwrap_or(pos as u64) as u32,
                        id: c.get("id").and_then(Value::as_str).map(String::from),
                        name: c.pointer("/function/name").and_then(Value::as_str).map(String::from),
                        args_delta: c
                            .pointer("/function/arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    });
                }
            }
            if let Some(f) = choice.get("finish_reason").and_then(Value::as_str) {
                self.finish_reason = Some(f.to_string());
            }
        }
        let usage = v
            .get("usage")
            .filter(|u| u.is_object())
            .or_else(|| v.pointer("/x_groq/usage"));
        if let Some(u) = usage {
            out.push(ChatEvent::Usage(self.usage(u)));
        }
        Ok((out, false))
    }

    fn on_eof(&mut self) -> Vec<ChatEvent> {
        self.done()
    }
}

pub fn parse_models(v: &Value) -> Vec<ModelInfo> {
    let per_million = |p: Option<&Value>| {
        let x = match p? {
            Value::String(s) => s.parse::<f64>().ok()?,
            Value::Number(n) => n.as_f64()?,
            _ => return None,
        };
        Some(x * 1_000_000.0)
    };
    v.get("data")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    Some(ModelInfo {
                        id: m.get("id")?.as_str()?.to_string(),
                        name: m.get("name").and_then(Value::as_str).map(String::from),
                        context: m
                            .get("context_length")
                            .or_else(|| m.get("context_window"))
                            .and_then(Value::as_u64)
                            .map(|c| c as u32),
                        price_in: per_million(m.pointer("/pricing/prompt")),
                        price_out: per_million(m.pointer("/pricing/completion")),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl Provider for OpenAiChat {
    async fn chat_stream(&self, req: ChatRequest, cancel: CancellationToken) -> Result<EventStream, ProviderError> {
        let e = &self.0;
        e.require_key()?;
        let q = &e.preset.quirks;
        let body = build_body(&req, q);
        let mut headers = e.base_headers();
        e.bearer(&mut headers);
        let rb = e
            .client
            .request(reqwest::Method::POST, &e.url("chat/completions"))
            .map(|r| r.headers(headers).json(&body));
        let (resp, info) = send(e, rb, &cancel).await?;
        let first = if info.is_empty() {
            vec![]
        } else {
            vec![ChatEvent::RateLimit(info)]
        };
        let text_tools = q.tools == ToolSupport::None && !req.tools.is_empty();
        Ok(drive(
            sse_events(resp.bytes_stream()),
            OpenAiStream::new(q, text_tools),
            first,
            cancel,
        ))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let e = &self.0;
        e.require_key()?;
        let mut headers = e.base_headers();
        e.bearer(&mut headers);
        let rb = e
            .client
            .request(reqwest::Method::GET, &e.url("models"))
            .map(|r| r.headers(headers));
        let (resp, _) = send(e, rb, &CancellationToken::new()).await?;
        let v: Value = resp.json().await.map_err(ProviderError::from_reqwest)?;
        Ok(parse_models(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::quirks::{MaxTokensField, TempRule};
    use crate::providers::{ChatMessage, ToolCall, ToolSpec};

    fn req() -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            system: Some("be nice".into()),
            messages: vec![ChatMessage::user("hi")],
            max_tokens: Some(100),
            temperature: Some(1.4),
            tools: vec![],
        }
    }

    fn tool() -> ToolSpec {
        ToolSpec {
            name: "ls".into(),
            description: "list files".into(),
            parameters: json!({"type":"object"}),
        }
    }

    fn run(state: &mut OpenAiStream, chunks: &[&str]) -> Vec<ChatEvent> {
        let mut out = Vec::new();
        for c in chunks {
            let (evs, end) = state
                .on_item(SseEvent {
                    event: None,
                    data: c.to_string(),
                })
                .unwrap();
            out.extend(evs);
            if end {
                return out;
            }
        }
        out.extend(state.on_eof());
        out
    }

    fn text(evs: &[ChatEvent]) -> String {
        evs.iter()
            .filter_map(|e| {
                if let ChatEvent::TextDelta(t) = e {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect()
    }
    fn reasoning(evs: &[ChatEvent]) -> String {
        evs.iter()
            .filter_map(|e| {
                if let ChatEvent::ReasoningDelta(t) = e {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn max_completion_tokens_field() {
        let q = Quirks {
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            ..Default::default()
        };
        let b = build_body(&req(), &q);
        assert_eq!(b["max_completion_tokens"], 100);
        assert!(b.get("max_tokens").is_none());
        assert_eq!(build_body(&req(), &Quirks::default())["max_tokens"], 100);
    }

    #[test]
    fn temperature_dropped_or_clamped() {
        let omit = Quirks {
            temperature: TempRule::Omit,
            ..Default::default()
        };
        assert!(build_body(&req(), &omit).get("temperature").is_none());
        let clamp = Quirks {
            temperature: TempRule::Clamp { min: 0.0, max: 1.0 },
            ..Default::default()
        };
        assert_eq!(build_body(&req(), &clamp)["temperature"], 1.0);
    }

    #[test]
    fn stream_usage_option() {
        let q = Quirks {
            stream_usage: true,
            ..Default::default()
        };
        assert_eq!(build_body(&req(), &q)["stream_options"]["include_usage"], true);
        assert!(build_body(&req(), &Quirks::default()).get("stream_options").is_none());
    }

    #[test]
    fn native_tools_and_tool_messages() {
        let mut r = req();
        r.tools = vec![tool()];
        r.messages.push(ChatMessage {
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "ls".into(),
                arguments: "{}".into(),
            }],
            ..ChatMessage::assistant("")
        });
        r.messages.push(ChatMessage::tool_result("c1", "a.txt"));
        let q = Quirks {
            tools: ToolSupport::NoParallel,
            ..Default::default()
        };
        let b = build_body(&r, &q);
        assert_eq!(b["tools"][0]["function"]["name"], "ls");
        assert_eq!(b["parallel_tool_calls"], false);
        assert_eq!(b["messages"][2]["tool_calls"][0]["id"], "c1");
        assert!(b["messages"][2]["content"].is_null());
        assert_eq!(
            b["messages"][3],
            json!({"role":"tool","tool_call_id":"c1","content":"a.txt"})
        );
    }

    #[test]
    fn reasoning_content_vs_reasoning_vs_think_tags() {
        let q = Quirks::default(); // Auto
        let evs = run(
            &mut OpenAiStream::new(&q, false),
            &[
                r#"{"choices":[{"delta":{"reasoning_content":"hmm "}}]}"#,
                r#"{"choices":[{"delta":{"reasoning":"ok"}}]}"#,
                r#"{"choices":[{"delta":{"content":"<think>more</think>Hi"}}]}"#,
                "[DONE]",
            ],
        );
        assert_eq!(reasoning(&evs), "hmm okmore");
        assert_eq!(text(&evs), "Hi");

        let q = Quirks {
            reasoning: ReasoningLoc::None,
            ..Default::default()
        };
        let evs = run(
            &mut OpenAiStream::new(&q, false),
            &[r#"{"choices":[{"delta":{"content":"<think>x</think>y","reasoning":"z"}}]}"#],
        );
        assert_eq!(text(&evs), "<think>x</think>y");
        assert_eq!(reasoning(&evs), "");
    }

    #[test]
    fn cost_field_and_usage_are_read() {
        let q = Quirks {
            cost_field: Some("cost".into()),
            ..Default::default()
        };
        let evs = run(
            &mut OpenAiStream::new(&q, false),
            &[
                r#"{"choices":[{"delta":{"content":"a"},"finish_reason":"stop"}]}"#,
                r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":4},"cost":0.0012}}"#,
                "[DONE]",
            ],
        );
        assert!(evs.contains(&ChatEvent::Usage(Usage {
            input: 10,
            output: 3,
            cached: 4,
            cost: Some(0.0012)
        })));
        assert_eq!(
            evs.last(),
            Some(&ChatEvent::Done {
                stop_reason: "stop".into()
            })
        );
    }

    #[test]
    fn tool_call_args_across_many_deltas() {
        let evs = run(
            &mut OpenAiStream::new(&Quirks::default(), false),
            &[
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"ls","arguments":""}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"pa"}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\".\"}"}}]},"finish_reason":"tool_calls"}]}"#,
                "[DONE]",
            ],
        );
        let args: String = evs
            .iter()
            .filter_map(|e| {
                if let ChatEvent::ToolCallDelta { args_delta, .. } = e {
                    Some(args_delta.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(args, r#"{"path":"."}"#);
        assert_eq!(
            evs.last(),
            Some(&ChatEvent::Done {
                stop_reason: "tool_calls".into()
            })
        );
    }

    #[test]
    fn stream_eof_without_done_finishes_turn() {
        let evs = run(
            &mut OpenAiStream::new(&Quirks::default(), false),
            &[r#"{"choices":[{"delta":{"content":"partial <thi"}}]}"#],
        );
        assert_eq!(text(&evs), "partial <thi");
        assert_eq!(evs.iter().filter(|e| matches!(e, ChatEvent::Done { .. })).count(), 1);
        assert_eq!(
            evs.last(),
            Some(&ChatEvent::Done {
                stop_reason: "eof".into()
            })
        );
    }

    #[test]
    fn mid_stream_error_is_an_error() {
        let mut s = OpenAiStream::new(&Quirks::default(), false);
        let err = s
            .on_item(SseEvent {
                event: None,
                data: r#"{"error":{"message":"overloaded","code":503}}"#.into(),
            })
            .unwrap_err();
        assert!(matches!(err, ProviderError::Server { status: 503, .. }));
    }

    #[test]
    fn tool_support_none_uses_text_protocol_round_trip() {
        let q = Quirks {
            tools: ToolSupport::None,
            ..Default::default()
        };
        let mut r = req();
        r.tools = vec![tool()];
        r.messages.push(ChatMessage {
            tool_calls: vec![ToolCall {
                id: "call_text_0".into(),
                name: "ls".into(),
                arguments: r#"{"path":"."}"#.into(),
            }],
            ..ChatMessage::assistant("Looking. ")
        });
        r.messages.push(ChatMessage::tool_result("call_text_0", "a.txt"));
        let b = build_body(&r, &q);
        assert!(b.get("tools").is_none());
        assert!(b["messages"][0]["content"].as_str().unwrap().contains("<tool_call>"));
        let sent = b["messages"][2]["content"].as_str().unwrap().to_string();
        assert!(sent.starts_with("Looking. <tool_call>"));
        assert_eq!(b["messages"][3]["role"], "user");

        // The model answers in the same format; it comes back as a tool call.
        let chunk = json!({"choices":[{"delta":{"content": sent}}]}).to_string();
        let evs = run(&mut OpenAiStream::new(&q, true), &[&chunk, "[DONE]"]);
        assert_eq!(text(&evs), "Looking. ");
        assert!(
            evs.iter()
                .any(|e| matches!(e, ChatEvent::ToolCallDelta { name: Some(n), .. } if n == "ls"))
        );
        assert_eq!(
            evs.last(),
            Some(&ChatEvent::Done {
                stop_reason: "tool_calls".into()
            })
        );
    }

    #[test]
    fn models_list_with_openrouter_pricing() {
        let v = json!({"data":[
            {"id":"meta/llama:free","name":"Llama","context_length":131072,"pricing":{"prompt":"0","completion":"0"}},
            {"id":"gpt-x","pricing":{"prompt":"0.000002","completion":"0.000008"}},
            {"id":"plain"}
        ]});
        let m = parse_models(&v);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].price_in, Some(0.0));
        assert_eq!(m[0].context, Some(131072));
        assert!((m[1].price_out.unwrap() - 8.0).abs() < 1e-9);
        assert_eq!(m[2].price_in, None);
    }
}
