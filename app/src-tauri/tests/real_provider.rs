//! Manual smoke test against a real provider. Never runs in CI.
//!
//!   GROQ_API_KEY=... cargo test --test real_provider -- --ignored --nocapture
//!
//! Uses whichever of GROQ_API_KEY / OPENROUTER_API_KEY / GEMINI_API_KEY is set,
//! or a local Ollama if LAMPCODE_SMOKE_OLLAMA_MODEL is set. Optionally pick the
//! model with LAMPCODE_SMOKE_MODEL.

use age::secrecy::SecretString;
use futures_util::StreamExt;
use lampcode_lib::providers::adapters::{self, Endpoint};
use lampcode_lib::providers::preset::{PresetRegistry, source_builtin_dir};
use lampcode_lib::providers::{ChatEvent, ChatMessage, ChatRequest};
use lampcode_lib::security::net::{Allowlist, GuardedClient};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
#[ignore]
async fn real_provider_round_trip() {
    let reg = PresetRegistry::load(&source_builtin_dir(), None).unwrap();
    let candidates = [
        ("groq", "GROQ_API_KEY", "llama-3.1-8b-instant"),
        (
            "openrouter",
            "OPENROUTER_API_KEY",
            "nvidia/nemotron-3.5-lightning:free",
        ),
        ("gemini", "GEMINI_API_KEY", "gemini-2.5-flash"),
    ];
    let (id, key, default_model) = if let Ok(m) = std::env::var("LAMPCODE_SMOKE_OLLAMA_MODEL") {
        ("ollama", None, m)
    } else {
        let (id, var, model) = candidates
            .iter()
            .find(|(_, var, _)| std::env::var(var).is_ok_and(|v| !v.is_empty()))
            .expect("set GROQ_API_KEY, OPENROUTER_API_KEY, GEMINI_API_KEY or LAMPCODE_SMOKE_OLLAMA_MODEL");
        (*id, Some(std::env::var(var).unwrap()), model.to_string())
    };
    let model = std::env::var("LAMPCODE_SMOKE_MODEL").unwrap_or(default_model);
    let preset = reg.get(id).unwrap().clone();
    let list = Allowlist::new();
    list.allow(&preset.host().unwrap(), "smoke test");
    let ep = Endpoint {
        base_url: preset.base_url.clone(),
        preset: Arc::new(preset),
        key: key.map(|k| Arc::new(SecretString::from(k))),
        client: GuardedClient::new(list),
    };
    let p = adapters::build(ep);

    let models = p.list_models().await.expect("list models");
    println!("{id}: {} models listed", models.len());

    let req = ChatRequest {
        model: model.clone(),
        messages: vec![ChatMessage::user("Reply with exactly: OK")],
        max_tokens: Some(256),
        ..Default::default()
    };
    let mut s = p.chat_stream(req, CancellationToken::new()).await.expect("chat starts");
    let mut text = String::new();
    let mut done = false;
    while let Some(ev) = s.next().await {
        match ev.expect("stream event") {
            ChatEvent::TextDelta(t) => text.push_str(&t),
            ChatEvent::Usage(u) => println!("usage: {u:?}"),
            ChatEvent::RateLimit(r) => println!("rate limit: {r:?}"),
            ChatEvent::Done { stop_reason } => {
                println!("done: {stop_reason}");
                done = true;
            }
            _ => {}
        }
    }
    println!("{id}/{model} said: {text:?}");
    assert!(done, "stream finished");
    assert!(text.to_uppercase().contains("OK"));
}
