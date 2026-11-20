//! One chat turn: store the user message, stream the model's answer to the UI,
//! store the answer with usage and cost.

use crate::providers::{ChatEvent, ChatMessage, ChatRequest, Provider, ProviderError, RateLimitInfo, Role, ToolCall, Usage};
use crate::storage::sessions::StoredMessage;
use crate::storage::{Db, ModelRef};
use futures_util::StreamExt;
use serde::Serialize;
use specta::Type;
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

pub const SYSTEM_PROMPT: &str = "You are LampCode, a coding assistant running on the user's own computer. \
Be direct and concise. Use Markdown, and fenced code blocks with a language tag for code.";

/// Streamed to the frontend while a turn runs.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SessionEvent {
    UserSaved { message: StoredMessage },
    Started { message_id: String, model: ModelRef },
    TextDelta { text: String },
    ReasoningDelta { text: String },
    Usage { usage: Usage },
    RateLimit { provider_id: String, info: RateLimitInfo },
    Finished { message: StoredMessage, stop_reason: String },
    Error { message: String, saved: Option<StoredMessage> },
}

pub struct TurnInput<'a> {
    pub db: &'a Db,
    pub provider: &'a dyn Provider,
    pub model: ModelRef,
    pub session_id: &'a str,
    pub user_text: String,
    /// USD per 1M tokens (in, out) when known, for cost when the provider
    /// doesn't report it.
    pub prices: Option<(f64, f64)>,
    pub cancel: CancellationToken,
}

fn history(stored: &[StoredMessage]) -> Vec<ChatMessage> {
    stored
        .iter()
        // Failed turns with no text would only confuse the model.
        .filter(|m| !(m.role == Role::Assistant && m.content.is_empty() && m.tool_calls.is_empty()))
        .map(|m| ChatMessage {
            role: m.role,
            content: m.content.clone(),
            reasoning: m.reasoning.clone(),
            tool_calls: m.tool_calls.clone(),
            tool_call_id: m.tool_call_id.clone(),
        })
        .collect()
}

pub fn turn_cost(usage: &Usage, prices: Option<(f64, f64)>) -> Option<f64> {
    usage.cost.or_else(|| prices.map(|(i, o)| (usage.input as f64 * i + usage.output as f64 * o) / 1_000_000.0))
}

#[derive(Default)]
struct Partial {
    text: String,
    reasoning: String,
    calls: BTreeMap<u32, ToolCall>,
    usage: Option<Usage>,
}

/// Run one turn. Returns the stored assistant message. Errors and
/// cancellation still store whatever text arrived, marked with `error`.
pub async fn run_turn(input: TurnInput<'_>, emit: &(dyn Fn(SessionEvent) + Send + Sync)) -> Result<StoredMessage, String> {
    let db = input.db;
    let err = |e: crate::storage::StorageError| crate::security::redact(&e.to_string()).into_owned();
    let session = db.session(input.session_id).await.map_err(err)?.ok_or("session not found")?;
    if session.title.is_empty() {
        let title: String = input.user_text.lines().next().unwrap_or("").chars().take(60).collect();
        db.rename_session(input.session_id, &title).await.map_err(err)?;
    }
    if session.model.as_ref() != Some(&input.model) {
        db.set_session_model(input.session_id, &input.model).await.map_err(err)?;
    }
    let user = db
        .append_message(input.session_id, StoredMessage {
            id: String::new(),
            role: Role::User,
            content: input.user_text.clone(),
            reasoning: None,
            tool_calls: vec![],
            tool_call_id: None,
            model: None,
            usage: None,
            cost: None,
            error: None,
            created_at: 0,
        })
        .await
        .map_err(err)?;
    emit(SessionEvent::UserSaved { message: user });

    let messages = history(&db.messages(input.session_id).await.map_err(err)?);
    let req = ChatRequest {
        model: input.model.model_id.clone(),
        system: Some(SYSTEM_PROMPT.into()),
        messages,
        max_tokens: None,
        temperature: None,
        tools: vec![],
    };
    let message_id = uuid::Uuid::new_v4().to_string();
    emit(SessionEvent::Started { message_id: message_id.clone(), model: input.model.clone() });

    let mut p = Partial::default();
    let outcome: Result<String, ProviderError> = async {
        let mut stream = input.provider.chat_stream(req, input.cancel.clone()).await?;
        let mut stop = String::from("eof");
        while let Some(ev) = stream.next().await {
            match ev? {
                ChatEvent::TextDelta(t) => {
                    p.text.push_str(&t);
                    emit(SessionEvent::TextDelta { text: t });
                }
                ChatEvent::ReasoningDelta(t) => {
                    p.reasoning.push_str(&t);
                    emit(SessionEvent::ReasoningDelta { text: t });
                }
                ChatEvent::ToolCallDelta { index, id, name, args_delta } => {
                    let c = p.calls.entry(index).or_insert_with(|| ToolCall { id: String::new(), name: String::new(), arguments: String::new() });
                    if let Some(id) = id {
                        c.id = id;
                    }
                    if let Some(n) = name {
                        c.name = n;
                    }
                    c.arguments.push_str(&args_delta);
                }
                ChatEvent::Usage(u) => {
                    p.usage = Some(u);
                    emit(SessionEvent::Usage { usage: u });
                }
                ChatEvent::RateLimit(info) => {
                    emit(SessionEvent::RateLimit { provider_id: input.model.provider_id.clone(), info });
                }
                ChatEvent::Done { stop_reason } => {
                    stop = stop_reason;
                    break;
                }
            }
        }
        Ok(stop)
    }
    .await;

    let cost = p.usage.as_ref().and_then(|u| turn_cost(u, input.prices));
    let mut stored = StoredMessage {
        id: message_id,
        role: Role::Assistant,
        content: p.text,
        reasoning: (!p.reasoning.is_empty()).then_some(p.reasoning),
        tool_calls: p.calls.into_values().collect(),
        tool_call_id: None,
        model: Some(input.model.clone()),
        usage: p.usage,
        cost,
        error: None,
        created_at: 0,
    };
    match outcome {
        Ok(stop) => {
            let saved = db.append_message(input.session_id, stored).await.map_err(err)?;
            if let Some(u) = &saved.usage {
                db.record_spend(&input.model.provider_id, &input.model.model_id, Some(input.session_id), u, saved.cost.unwrap_or(0.0))
                    .await
                    .map_err(err)?;
            }
            emit(SessionEvent::Finished { message: saved.clone(), stop_reason: stop });
            Ok(saved)
        }
        Err(e) => {
            let msg = e.display_redacted();
            stored.error = Some(msg.clone());
            // Keep what arrived so the transcript matches what the user saw.
            let saved = if stored.content.is_empty() && stored.reasoning.is_none() {
                None
            } else {
                Some(db.append_message(input.session_id, stored).await.map_err(err)?)
            };
            emit(SessionEvent::Error { message: msg.clone(), saved });
            Err(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_prefers_provider_value() {
        let u = Usage { input: 1_000_000, output: 500_000, cached: 0, cost: None };
        assert_eq!(turn_cost(&u, Some((1.0, 4.0))), Some(3.0));
        assert_eq!(turn_cost(&Usage { cost: Some(0.01), ..u }, Some((1.0, 4.0))), Some(0.01));
        assert_eq!(turn_cost(&u, None), None);
    }

    #[test]
    fn empty_failed_turns_are_left_out_of_history() {
        let mk = |role, content: &str| StoredMessage {
            id: "x".into(),
            role,
            content: content.into(),
            reasoning: None,
            tool_calls: vec![],
            tool_call_id: None,
            model: None,
            usage: None,
            cost: None,
            error: None,
            created_at: 1,
        };
        let h = history(&[mk(Role::User, "a"), mk(Role::Assistant, ""), mk(Role::User, "b")]);
        assert_eq!(h.len(), 2);
    }
}
