//! A plain chat turn end to end: mock provider, real streaming, real SQLite.

mod support;

use age::secrecy::SecretString;
use lampcode_lib::agent::{SessionEvent, TurnInput, run_turn};
use lampcode_lib::providers::Role;
use lampcode_lib::providers::adapters::{self, Endpoint};
use lampcode_lib::providers::preset::parse_preset;
use lampcode_lib::security::net::{Allowlist, GuardedClient};
use lampcode_lib::storage::{Db, ModelRef};
use std::sync::{Arc, Mutex};
use support::mock_server::{MockServer, Reply};
use tokio_util::sync::CancellationToken;

fn endpoint(server: &MockServer) -> Endpoint {
    let preset = parse_preset(
        "mock.json",
        r#"{"id":"mock","name":"Mock","icon":"x","protocol":"openai_chat","base_url":"http://127.0.0.1:1",
        "key_env":"MOCK_KEY","docs_url":"https://example.com","checked":"2026-11-20","free_tier":"none","region":"Local",
        "quirks":{"stream_usage":true}}"#,
    )
    .unwrap();
    let list = Allowlist::new();
    list.allow("127.0.0.1", "test");
    Endpoint {
        preset: Arc::new(preset),
        base_url: server.base("/v1"),
        key: Some(Arc::new(SecretString::from("test-key-not-real"))),
        client: GuardedClient::new(list),
    }
}

#[tokio::test]
async fn plain_chat_against_mock_streams_and_persists() {
    let server = MockServer::start().await;
    server.push(Reply::sse(&[
        r#"{"choices":[{"delta":{"content":"Hello "}}]}"#,
        r#"{"choices":[{"delta":{"content":"there"},"finish_reason":"stop"}]}"#,
        r#"{"choices":[],"usage":{"prompt_tokens":1000,"completion_tokens":500}}"#,
        "[DONE]",
    ]));
    server.push(Reply::sse(&[
        r#"{"choices":[{"delta":{"content":"again"},"finish_reason":"stop"}]}"#,
        "[DONE]",
    ]));

    let db = Db::open_in_memory().await.unwrap();
    let model = ModelRef {
        provider_id: "mock".into(),
        model_id: "m-1".into(),
    };
    let session = db.create_session(None, None).await.unwrap();
    let provider = adapters::build(endpoint(&server));
    let events = Mutex::new(Vec::new());
    let emit = |e: SessionEvent| events.lock().unwrap().push(e);

    let saved = run_turn(
        TurnInput {
            db: &db,
            provider: provider.as_ref(),
            model: model.clone(),
            session_id: &session.id,
            user_text: "say hello".into(),
            prices: Some((2.0, 10.0)),
            cancel: CancellationToken::new(),
        },
        &emit,
    )
    .await
    .unwrap();

    assert_eq!(saved.content, "Hello there");
    assert_eq!(saved.cost, Some(0.007), "1000 in at $2/M + 500 out at $10/M");
    let evs = events.lock().unwrap().clone();
    let deltas: String = evs
        .iter()
        .filter_map(|e| {
            if let SessionEvent::TextDelta { text } = e {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(deltas, "Hello there");
    assert!(matches!(evs.first(), Some(SessionEvent::UserSaved { .. })));
    assert!(matches!(evs.last(), Some(SessionEvent::Finished { stop_reason, .. }) if stop_reason == "stop"));

    let stored = db.messages(&session.id).await.unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!((stored[0].role, stored[0].content.as_str()), (Role::User, "say hello"));
    assert_eq!(stored[1].usage.unwrap().output, 500);
    let s = db.session(&session.id).await.unwrap().unwrap();
    assert_eq!(s.title, "say hello");
    assert_eq!(s.model, Some(model.clone()));

    // Second turn sends the whole history.
    run_turn(
        TurnInput {
            db: &db,
            provider: provider.as_ref(),
            model,
            session_id: &session.id,
            user_text: "again".into(),
            prices: None,
            cancel: CancellationToken::new(),
        },
        &emit,
    )
    .await
    .unwrap();
    let body = &server.seen()[1].body;
    let roles: Vec<&str> = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles, ["system", "user", "assistant", "user"]);
    assert_eq!(db.messages(&session.id).await.unwrap().len(), 4);
}

#[tokio::test]
async fn failed_turn_keeps_partial_text_marked_as_error() {
    let server = MockServer::start().await;
    server.push(Reply::sse(&[
        r#"{"choices":[{"delta":{"content":"half"}}]}"#,
        r#"{"error":{"message":"boom","code":502}}"#,
    ]));
    let db = Db::open_in_memory().await.unwrap();
    let session = db.create_session(None, None).await.unwrap();
    let provider = adapters::build(endpoint(&server));
    let err = run_turn(
        TurnInput {
            db: &db,
            provider: provider.as_ref(),
            model: ModelRef {
                provider_id: "mock".into(),
                model_id: "m".into(),
            },
            session_id: &session.id,
            user_text: "go".into(),
            prices: None,
            cancel: CancellationToken::new(),
        },
        &|_| {},
    )
    .await
    .unwrap_err();
    assert!(err.contains("502"));
    let stored = db.messages(&session.id).await.unwrap();
    assert_eq!(stored[1].content, "half");
    assert!(stored[1].error.is_some());
}
