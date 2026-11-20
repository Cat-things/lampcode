//! Protocol adapters. Each one is a pure request builder plus a stream state
//! machine (both unit tested), wired to HTTP through the guarded client.

pub mod anthropic;
pub mod ollama;
pub mod openai_chat;

use super::{ChatEvent, EventStream, Preset, Protocol, Provider, ProviderError, ratelimit};
use crate::security::net::{GuardedClient, NetError};
use age::secrecy::{ExposeSecret, SecretString};
use futures_util::{Stream, StreamExt};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Everything an adapter needs to talk to one configured provider.
#[derive(Clone)]
pub struct Endpoint {
    pub preset: Arc<Preset>,
    /// Base URL in use (the preset's, or a variant's).
    pub base_url: String,
    pub key: Option<Arc<SecretString>>,
    pub client: GuardedClient,
}

impl Endpoint {
    pub fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path.trim_start_matches('/'))
    }

    /// Headers shared by every request: preset extras and user agent. Auth
    /// headers are added by each adapter and marked sensitive so they never
    /// show up in debug output.
    pub fn base_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in &self.preset.quirks.extra_headers {
            if let (Ok(name), Ok(val)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) {
                h.insert(name, val);
            }
        }
        if let Some(ua) = &self.preset.quirks.user_agent
            && let Ok(v) = HeaderValue::from_str(ua)
        {
            h.insert(reqwest::header::USER_AGENT, v);
        }
        h
    }

    pub fn secret_header(value: &str) -> Option<HeaderValue> {
        let mut v = HeaderValue::from_str(value).ok()?;
        v.set_sensitive(true);
        Some(v)
    }

    pub fn bearer(&self, h: &mut HeaderMap) {
        if let Some(k) = &self.key
            && let Some(v) = Self::secret_header(&format!("Bearer {}", k.expose_secret()))
        {
            h.insert(reqwest::header::AUTHORIZATION, v);
        }
    }

    pub fn require_key(&self) -> Result<(), ProviderError> {
        if self.preset.key_required && self.key.is_none() {
            return Err(ProviderError::NoKey(self.preset.name.clone()));
        }
        Ok(())
    }
}

pub fn build(endpoint: Endpoint) -> Box<dyn Provider> {
    match endpoint.preset.protocol {
        Protocol::OpenaiChat => Box::new(openai_chat::OpenAiChat(endpoint)),
        Protocol::Anthropic => Box::new(anthropic::Anthropic(endpoint)),
        Protocol::Ollama => Box::new(ollama::Ollama(endpoint)),
    }
}

fn net_err(e: NetError) -> ProviderError {
    match e {
        NetError::Blocked(h) => ProviderError::Blocked(h),
        NetError::BadUrl => ProviderError::Other("invalid provider URL".into()),
    }
}

/// Send a request, mapping transport and HTTP errors. On success also returns
/// the rate-limit info from the response headers.
pub async fn send(
    endpoint: &Endpoint,
    req: Result<reqwest::RequestBuilder, NetError>,
    cancel: &CancellationToken,
) -> Result<(reqwest::Response, ratelimit::RateLimitInfo), ProviderError> {
    let req = req.map_err(net_err)?;
    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
        r = req.send() => r.map_err(|e| {
            // A redirect refused by the allowlist surfaces as a reqwest error.
            let blocked = std::error::Error::source(&e)
                .and_then(|s| s.downcast_ref::<NetError>())
                .cloned();
            match blocked {
                Some(NetError::Blocked(h)) => ProviderError::Blocked(h),
                _ => ProviderError::from_reqwest(e),
            }
        })?,
    };
    let info = ratelimit::parse(endpoint.preset.quirks.ratelimit_headers, resp.headers());
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let retry = ratelimit::retry_after(resp.headers());
        let body = resp.text().await.unwrap_or_default();
        return Err(ProviderError::from_status(status, &body, retry));
    }
    Ok((resp, info))
}

/// A stream state machine: raw items in, chat events out.
pub trait StreamState: Send + 'static {
    type Item: Send + 'static;
    /// Returns events, and `true` when the stream is finished.
    fn on_item(&mut self, item: Self::Item) -> Result<(Vec<ChatEvent>, bool), ProviderError>;
    /// The transport ended. Must end with exactly one `Done`.
    fn on_eof(&mut self) -> Vec<ChatEvent>;
}

/// Drive a state machine over a transport stream, honouring cancellation.
/// Guarantees: at most one `Done`, nothing after an error, and a `Done` even
/// when the provider closes the stream without its end marker.
pub fn drive<S, St>(items: S, state: St, first: Vec<ChatEvent>, cancel: CancellationToken) -> EventStream
where
    S: Stream<Item = Result<St::Item, String>> + Send + 'static,
    St: StreamState,
{
    use std::collections::VecDeque;
    struct Run<S, St> {
        items: std::pin::Pin<Box<S>>,
        state: St,
        queue: VecDeque<Result<ChatEvent, ProviderError>>,
        ended: bool,
        cancel: CancellationToken,
    }
    let run = Run {
        items: Box::pin(items),
        state,
        queue: first.into_iter().map(Ok).collect(),
        ended: false,
        cancel,
    };
    futures_util::stream::unfold(run, |mut r| async move {
        loop {
            if let Some(ev) = r.queue.pop_front() {
                return Some((ev, r));
            }
            if r.ended {
                return None;
            }
            let next = tokio::select! {
                _ = r.cancel.cancelled() => {
                    r.ended = true;
                    r.queue.push_back(Err(ProviderError::Cancelled));
                    continue;
                }
                n = r.items.next() => n,
            };
            let step = match next {
                Some(Ok(i)) => r.state.on_item(i),
                Some(Err(e)) => Err(ProviderError::Other(crate::security::redact(&e).into_owned())),
                None => Ok((r.state.on_eof(), true)),
            };
            match step {
                Ok((evs, finished)) => {
                    r.queue.extend(evs.into_iter().map(Ok));
                    r.ended = finished;
                }
                Err(e) => {
                    r.queue.push_back(Err(e));
                    r.ended = true;
                }
            }
        }
    })
    .boxed()
}

/// Read a JSON number at a dotted path, e.g. `cost` or `cost_details.total`.
pub fn json_path_f64(v: &serde_json::Value, path: &str) -> Option<f64> {
    path.split('.').try_fold(v, |cur, k| cur.get(k))?.as_f64()
}
