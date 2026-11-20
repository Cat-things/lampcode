//! Adapters against a local mock server: real HTTP, real streaming.

mod support;

use age::secrecy::SecretString;
use futures_util::StreamExt;
use lampcode_lib::providers::adapters::{self, Endpoint};
use lampcode_lib::providers::preset::{Preset, parse_preset};
use lampcode_lib::providers::{ChatEvent, ChatMessage, ChatRequest, ProviderError};
use lampcode_lib::security::net::{Allowlist, GuardedClient};
use std::sync::Arc;
use support::mock_server::{MockServer, Reply};
use tokio_util::sync::CancellationToken;

fn fake_key() -> String {
    ["sk-", "or-v1-", &"9f8e7d6c".repeat(8)].concat()
}

fn preset_json(protocol: &str, quirks: &str) -> Preset {
    parse_preset(
        "mock.json",
        &format!(
            r#"{{"id":"mock","name":"Mock","icon":"x","protocol":"{protocol}","base_url":"http://127.0.0.1:1",
            "key_env":"MOCK_KEY","docs_url":"https://example.com","checked":"2026-11-20","free_tier":"none",
            "region":"Local","quirks":{quirks}}}"#
        ),
    )
    .unwrap()
}

fn endpoint(server: &MockServer, preset: Preset, path: &str) -> (Endpoint, Arc<Allowlist>) {
    let list = Allowlist::new();
    list.allow("127.0.0.1", "test mock");
    let ep = Endpoint {
        preset: Arc::new(preset),
        base_url: server.base(path),
        key: Some(Arc::new(SecretString::from(fake_key()))),
        client: GuardedClient::new(list.clone()),
    };
    (ep, list)
}

fn req(model: &str) -> ChatRequest {
    ChatRequest { model: model.into(), messages: vec![ChatMessage::user("hi")], ..Default::default() }
}

async fn collect(p: &dyn lampcode_lib::providers::Provider, r: ChatRequest) -> Result<Vec<ChatEvent>, ProviderError> {
    let mut s = p.chat_stream(r, CancellationToken::new()).await?;
    let mut out = Vec::new();
    while let Some(e) = s.next().await {
        out.push(e?);
    }
    Ok(out)
}

fn text(evs: &[ChatEvent]) -> String {
    evs.iter().filter_map(|e| if let ChatEvent::TextDelta(t) = e { Some(t.as_str()) } else { None }).collect()
}

#[tokio::test]
async fn openai_stream_text_with_ratelimit_headers_and_extra_headers() {
    let server = MockServer::start().await;
    let quirks = r#"{"extra_headers":{"HTTP-Referer":"https://github.com/Cat-things/lampcode","X-OpenRouter-Title":"LampCode"},
        "model_id":{"free_suffix":":free"}}"#;
    let (ep, _) = endpoint(&server, preset_json("openai_chat", quirks), "/api/v1");
    let mut reply = Reply::sse(&[
        r#"{"choices":[{"delta":{"content":"Hel"}}]}"#,
        r#"{"choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}]}"#,
        "[DONE]",
    ]);
    if let Reply::Stream { headers, .. } = &mut reply {
        headers.push(("x-ratelimit-limit-requests", "100".into()));
        headers.push(("x-ratelimit-remaining-requests", "40".into()));
    }
    server.push(reply);
    let p = adapters::build(ep);
    let evs = collect(p.as_ref(), req("meta-llama/llama-3.3-70b-instruct:free")).await.unwrap();
    assert_eq!(text(&evs), "Hello");
    assert!(matches!(&evs[0], ChatEvent::RateLimit(info) if info.headroom() == Some(0.4)));
    let seen = &server.seen()[0];
    assert_eq!(seen.path, "/api/v1/chat/completions");
    assert_eq!(seen.body["model"], "meta-llama/llama-3.3-70b-instruct:free", ":free id kept as is");
    assert_eq!(seen.headers["http-referer"], "https://github.com/Cat-things/lampcode");
    assert_eq!(seen.headers["x-openrouter-title"], "LampCode");
    assert_eq!(seen.headers["authorization"].to_str().unwrap(), format!("Bearer {}", fake_key()));
}

#[tokio::test]
async fn openai_stream_tool_calls() {
    let server = MockServer::start().await;
    let (ep, _) = endpoint(&server, preset_json("openai_chat", "{}"), "/v1");
    server.push(Reply::sse(&[
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"ls","arguments":"{\"p"}}]}}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\":1}"}}]},"finish_reason":"tool_calls"}]}"#,
        "[DONE]",
    ]));
    let evs = collect(adapters::build(ep).as_ref(), req("m")).await.unwrap();
    let args: String = evs
        .iter()
        .filter_map(|e| if let ChatEvent::ToolCallDelta { args_delta, .. } = e { Some(args_delta.as_str()) } else { None })
        .collect();
    assert_eq!(args, r#"{"p":1}"#);
    assert_eq!(evs.last(), Some(&ChatEvent::Done { stop_reason: "tool_calls".into() }));
}

#[tokio::test]
async fn rate_limited_with_retry_after() {
    let server = MockServer::start().await;
    let (ep, _) = endpoint(&server, preset_json("openai_chat", "{}"), "/v1");
    server.push(Reply::Status { status: 429, headers: vec![("retry-after", "12".into())], body: "slow down".into() });
    let err = collect(adapters::build(ep).as_ref(), req("m")).await.unwrap_err();
    assert_eq!(err, ProviderError::RateLimited { retry_after: Some(12) });
}

#[tokio::test]
async fn server_error_body_is_redacted() {
    let server = MockServer::start().await;
    let (ep, _) = endpoint(&server, preset_json("openai_chat", "{}"), "/v1");
    server.push(Reply::Status { status: 500, headers: vec![], body: format!("upstream said: bad key {}", fake_key()) });
    let err = collect(adapters::build(ep).as_ref(), req("m")).await.unwrap_err();
    assert!(matches!(err, ProviderError::Server { status: 500, .. }));
    assert!(!err.to_string().contains(&fake_key()));
    assert!(!format!("{err:?}").contains(&fake_key()));
}

#[tokio::test]
async fn cancel_mid_stream_ends_with_cancelled() {
    let server = MockServer::start().await;
    let (ep, _) = endpoint(&server, preset_json("openai_chat", "{}"), "/v1");
    server.push(Reply::Stall { chunks: vec!["data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n".into()] });
    let cancel = CancellationToken::new();
    let p = adapters::build(ep);
    let mut s = p.chat_stream(req("m"), cancel.clone()).await.unwrap();
    assert_eq!(s.next().await.unwrap().unwrap(), ChatEvent::TextDelta("a".into()));
    cancel.cancel();
    let next = tokio::time::timeout(std::time::Duration::from_secs(5), s.next()).await.expect("cancel is prompt");
    assert_eq!(next.unwrap().unwrap_err(), ProviderError::Cancelled);
    assert!(s.next().await.is_none());
}

#[tokio::test]
async fn slow_stream_still_completes() {
    let server = MockServer::start().await;
    let (ep, _) = endpoint(&server, preset_json("openai_chat", "{}"), "/v1");
    server.push(Reply::Stream {
        content_type: "text/event-stream",
        chunks: vec!["data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n".into(), "data: [DONE]\n\n".into()],
        delay_ms: 300,
        headers: vec![],
    });
    let evs = collect(adapters::build(ep).as_ref(), req("m")).await.unwrap();
    assert_eq!(text(&evs), "x");
}

#[tokio::test]
async fn host_not_on_allowlist_is_never_contacted() {
    let server = MockServer::start().await;
    let (mut ep, list) = endpoint(&server, preset_json("openai_chat", "{}"), "/v1");
    ep.client = GuardedClient::new(Allowlist::new());
    let err = collect(adapters::build(ep).as_ref(), req("m")).await.unwrap_err();
    assert_eq!(err, ProviderError::Blocked("127.0.0.1".into()));
    assert!(server.seen().is_empty());
    drop(list);
}

#[tokio::test]
async fn missing_key_is_reported_before_any_request() {
    let server = MockServer::start().await;
    let (mut ep, _) = endpoint(&server, preset_json("openai_chat", "{}"), "/v1");
    ep.key = None;
    let err = collect(adapters::build(ep).as_ref(), req("m")).await.unwrap_err();
    assert_eq!(err, ProviderError::NoKey("Mock".into()));
    assert!(server.seen().is_empty());
}

#[tokio::test]
async fn openai_list_models() {
    let server = MockServer::start().await;
    let (ep, _) = endpoint(&server, preset_json("openai_chat", "{}"), "/v1");
    server.push(Reply::json(serde_json::json!({"data":[{"id":"a"},{"id":"b:free"}]})));
    let models = adapters::build(ep).list_models().await.unwrap();
    assert_eq!(models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["a", "b:free"]);
    assert_eq!(server.seen()[0].path, "/v1/models");
}

#[tokio::test]
async fn anthropic_headers_and_stream() {
    let server = MockServer::start().await;
    let (ep, _) = endpoint(&server, preset_json("anthropic", r#"{"anthropic_version":"2023-06-01"}"#), "");
    server.push(Reply::Stream {
        content_type: "text/event-stream",
        chunks: vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":5}}}\n\n".into(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Yo\"}}\n\n".into(),
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n".into(),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".into(),
        ],
        delay_ms: 0,
        headers: vec![],
    });
    let evs = collect(adapters::build(ep).as_ref(), req("claude-x")).await.unwrap();
    assert_eq!(text(&evs), "Yo");
    assert_eq!(evs.last(), Some(&ChatEvent::Done { stop_reason: "stop".into() }));
    let seen = &server.seen()[0];
    assert_eq!(seen.path, "/v1/messages");
    assert_eq!(seen.headers["x-api-key"].to_str().unwrap(), fake_key());
    assert_eq!(seen.headers["anthropic-version"], "2023-06-01");
    assert!(seen.headers.get("authorization").is_none());
}

#[tokio::test]
async fn ollama_chat_tags_pull_delete() {
    let server = MockServer::start().await;
    let preset = parse_preset(
        "ollama.json",
        r#"{"id":"ollama","name":"Ollama","icon":"x","protocol":"ollama","base_url":"http://127.0.0.1:11434",
        "key_required":false,"docs_url":"https://docs.ollama.com/faq","checked":"2026-11-20","free_tier":"local","region":"Local"}"#,
    )
    .unwrap();
    let (mut ep, _) = endpoint(&server, preset, "");
    ep.key = None;
    server.push(Reply::Stream {
        content_type: "application/x-ndjson",
        chunks: vec![
            "{\"message\":{\"role\":\"assistant\",\"content\":\"hey\"},\"done\":false}\n".into(),
            "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"prompt_eval_count\":3,\"eval_count\":1}\n".into(),
        ],
        delay_ms: 0,
        headers: vec![],
    });
    server.push(Reply::json(serde_json::json!({"models":[{"name":"llama3:8b"}]})));
    server.push(Reply::json(serde_json::json!({"status":"success"})));
    server.push(Reply::Status { status: 200, headers: vec![], body: String::new() });

    let client = lampcode_lib::providers::adapters::ollama::Ollama(ep.clone());
    let evs = collect(&client, req("llama3:8b")).await.unwrap();
    assert_eq!(text(&evs), "hey");
    use lampcode_lib::providers::Provider;
    assert_eq!(client.list_models().await.unwrap()[0].id, "llama3:8b");
    client.pull("llama3:8b").await.unwrap();
    client.delete("llama3:8b").await.unwrap();
    let paths: Vec<_> = server.seen().iter().map(|r| (r.method.to_string(), r.path.clone())).collect();
    assert_eq!(paths, [
        ("POST".to_string(), "/api/chat".to_string()),
        ("GET".into(), "/api/tags".into()),
        ("POST".into(), "/api/pull".into()),
        ("DELETE".into(), "/api/delete".into()),
    ]);
    assert!(server.seen()[0].headers.get("authorization").is_none(), "no key, no auth header");
}
