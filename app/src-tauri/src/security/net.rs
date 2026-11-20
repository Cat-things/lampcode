//! The one choke point for outbound HTTP. Every request goes through
//! [`GuardedClient`], which refuses hosts that aren't on the allowlist
//! (including redirect targets) and records blocked attempts.

use crate::security::redact;
use serde::Serialize;
use specta::Type;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BlockedAttempt {
    pub host: String,
    pub reason: String,
    /// Unix seconds.
    #[specta(type = f64)]
    pub at: i64,
}

/// Hosts the app may talk to, each with why it is allowed (shown in
/// Settings > Privacy > Network).
#[derive(Debug, Default)]
pub struct Allowlist {
    hosts: RwLock<BTreeMap<String, String>>,
    blocked: Mutex<VecDeque<BlockedAttempt>>,
}

const BLOCKED_KEEP: usize = 200;

pub fn normalize_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

impl Allowlist {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn allow(&self, host: &str, reason: &str) {
        self.hosts
            .write()
            .unwrap()
            .insert(normalize_host(host), reason.to_string());
    }

    pub fn revoke(&self, host: &str) {
        self.hosts.write().unwrap().remove(&normalize_host(host));
    }

    /// Replace every entry with the given source label, keeping the others.
    pub fn replace_source(&self, source: &str, hosts: impl IntoIterator<Item = String>) {
        let mut map = self.hosts.write().unwrap();
        map.retain(|_, r| r != source);
        for h in hosts {
            map.insert(normalize_host(&h), source.to_string());
        }
    }

    pub fn is_allowed(&self, host: &str) -> bool {
        self.hosts.read().unwrap().contains_key(&normalize_host(host))
    }

    pub fn entries(&self) -> Vec<(String, String)> {
        self.hosts
            .read()
            .unwrap()
            .iter()
            .map(|(h, r)| (h.clone(), r.clone()))
            .collect()
    }

    pub fn record_block(&self, host: &str, reason: &str) {
        let host = redact(host).into_owned();
        tracing::warn!(%host, reason, "blocked outbound request");
        let mut q = self.blocked.lock().unwrap();
        if q.len() >= BLOCKED_KEEP {
            q.pop_front();
        }
        q.push_back(BlockedAttempt {
            host,
            reason: reason.into(),
            at: chrono::Utc::now().timestamp(),
        });
    }

    pub fn blocked(&self) -> Vec<BlockedAttempt> {
        self.blocked.lock().unwrap().iter().cloned().collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    #[error("blocked: {0} is not on the outbound allowlist")]
    Blocked(String),
    #[error("invalid URL")]
    BadUrl,
}

#[derive(Clone)]
pub struct GuardedClient {
    client: reqwest::Client,
    allowlist: Arc<Allowlist>,
}

impl GuardedClient {
    pub fn new(allowlist: Arc<Allowlist>) -> Self {
        let for_redirects = allowlist.clone();
        let policy = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("too many redirects");
            }
            let host = attempt.url().host_str().map(normalize_host).unwrap_or_default();
            let same_host = attempt
                .previous()
                .last()
                .and_then(|u| u.host_str())
                .is_some_and(|h| normalize_host(h) == host);
            if same_host || for_redirects.is_allowed(&host) {
                attempt.follow()
            } else {
                for_redirects.record_block(&host, "redirect target not allowed");
                attempt.error(NetError::Blocked(host))
            }
        });
        let client = reqwest::Client::builder()
            .redirect(policy)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(120))
            .user_agent(concat!("LampCode/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client builds");
        Self { client, allowlist }
    }

    pub fn allowlist(&self) -> &Arc<Allowlist> {
        &self.allowlist
    }

    fn check(&self, url: &str, extra: Option<&str>) -> Result<reqwest::Url, NetError> {
        let parsed = reqwest::Url::parse(url).map_err(|_| NetError::BadUrl)?;
        if !matches!(parsed.scheme(), "https" | "http") {
            return Err(NetError::BadUrl);
        }
        let host = parsed.host_str().map(normalize_host).ok_or(NetError::BadUrl)?;
        let ok = self.allowlist.is_allowed(&host) || extra.is_some_and(|e| normalize_host(e) == host);
        if !ok {
            self.allowlist.record_block(&host, "host not on allowlist");
            return Err(NetError::Blocked(host));
        }
        Ok(parsed)
    }

    pub fn request(&self, method: reqwest::Method, url: &str) -> Result<reqwest::RequestBuilder, NetError> {
        let u = self.check(url, None)?;
        Ok(self.client.request(method, u))
    }

    /// One request to a host that is allowed only for this call (e.g. the
    /// manual model catalog refresh). Nothing is added to the allowlist.
    pub fn request_once_to(
        &self,
        method: reqwest::Method,
        url: &str,
        host: &str,
    ) -> Result<reqwest::RequestBuilder, NetError> {
        let u = self.check(url, Some(host))?;
        Ok(self.client.request(method, u))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_host_is_refused_and_logged() {
        let list = Allowlist::new();
        list.allow("api.groq.com", "provider: groq");
        let c = GuardedClient::new(list.clone());
        assert!(
            c.request(reqwest::Method::GET, "https://api.groq.com/openai/v1/models")
                .is_ok()
        );
        assert!(c.request(reqwest::Method::GET, "https://API.GROQ.COM./x").is_ok());
        let err = c
            .request(reqwest::Method::GET, "https://evil.example.com/steal")
            .unwrap_err();
        assert_eq!(err, NetError::Blocked("evil.example.com".into()));
        let blocked = list.blocked();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].host, "evil.example.com");
    }

    #[test]
    fn lookalike_hosts_are_not_allowed() {
        let list = Allowlist::new();
        list.allow("api.groq.com", "p");
        let c = GuardedClient::new(list);
        for url in [
            "https://api.groq.com.evil.net/",
            "https://evil.net/?h=api.groq.com",
            "https://api.groq.com@evil.net/",
            "file:///etc/passwd",
            "not a url",
        ] {
            assert!(c.request(reqwest::Method::GET, url).is_err(), "{url}");
        }
    }

    #[test]
    fn one_off_host_is_not_remembered() {
        let list = Allowlist::new();
        let c = GuardedClient::new(list.clone());
        assert!(
            c.request_once_to(reqwest::Method::GET, "https://models.dev/api.json", "models.dev")
                .is_ok()
        );
        assert!(!list.is_allowed("models.dev"));
        assert!(c.request(reqwest::Method::GET, "https://models.dev/api.json").is_err());
    }

    #[test]
    fn replace_source_keeps_other_sources() {
        let list = Allowlist::new();
        list.replace_source("providers", ["a.com".to_string(), "b.com".to_string()]);
        list.allow("mcp.local", "mcp");
        list.replace_source("providers", ["b.com".to_string()]);
        assert!(!list.is_allowed("a.com"));
        assert!(list.is_allowed("b.com") && list.is_allowed("mcp.local"));
    }

    #[test]
    fn blocked_log_is_bounded() {
        let list = Allowlist::new();
        for i in 0..(BLOCKED_KEEP + 50) {
            list.record_block(&format!("h{i}.com"), "x");
        }
        assert_eq!(list.blocked().len(), BLOCKED_KEEP);
    }

    #[tokio::test]
    async fn redirect_to_unlisted_host_is_blocked() {
        use axum::{Router, response::Redirect, routing::get};
        let app = Router::new().route(
            "/go",
            get(|| async { Redirect::temporary("http://localhost.invalid.test/x") }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let list = Allowlist::new();
        list.allow("127.0.0.1", "test");
        let c = GuardedClient::new(list.clone());
        let res = c
            .request(reqwest::Method::GET, &format!("http://{addr}/go"))
            .unwrap()
            .send()
            .await;
        assert!(res.is_err());
        assert!(list.blocked().iter().any(|b| b.host == "localhost.invalid.test"));
    }
}
