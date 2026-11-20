//! App-wide provider state: presets, catalog, the guarded HTTP client and the
//! last rate-limit info per provider. Also owns the provider part of the
//! outbound allowlist: only configured, enabled providers are reachable.

use super::adapters::{self, Endpoint};
use super::catalog::Catalog;
use super::preset::{FreeTier, Origin, Preset, PresetRegistry, Protocol, Tag};
use super::{Provider, ProviderError, RateLimitInfo};
use crate::security::keys::{KeyError, KeyPreview, KeyStore};
use crate::security::net::{Allowlist, GuardedClient};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

pub const ALLOW_SOURCE: &str = "configured provider";

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum KeyState {
    /// Local provider, no key needed.
    NotNeeded,
    Set(KeyPreview),
    Missing,
    /// Keys are in the encrypted file and it isn't unlocked yet.
    Locked,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub protocol: Protocol,
    pub base_url: String,
    pub key_env: Option<String>,
    pub signup_url: Option<String>,
    pub docs_url: String,
    pub checked: String,
    pub free_tier: FreeTier,
    pub free_tier_note: String,
    pub region: String,
    pub tags: Vec<Tag>,
    pub key: KeyState,
    pub configured: bool,
    pub disabled: bool,
    pub user_pack: bool,
}

pub struct Hub {
    registry: RwLock<PresetRegistry>,
    pub catalog: Catalog,
    client: GuardedClient,
    ratelimits: Mutex<HashMap<String, RateLimitInfo>>,
}

impl Hub {
    pub fn new(registry: PresetRegistry, catalog: Catalog, allowlist: Arc<Allowlist>) -> Self {
        Self { registry: RwLock::new(registry), catalog, client: GuardedClient::new(allowlist), ratelimits: Mutex::new(HashMap::new()) }
    }

    pub fn client(&self) -> &GuardedClient {
        &self.client
    }

    pub fn warnings(&self) -> Vec<String> {
        self.registry.read().unwrap().warnings.clone()
    }

    pub fn preset(&self, id: &str) -> Option<Preset> {
        self.registry.read().unwrap().get(id).cloned()
    }

    /// Tell the key store which env var overrides each provider's key.
    pub fn register_env_vars(&self, keys: &KeyStore) {
        for p in self.registry.read().unwrap().all() {
            if let Some(var) = &p.key_env {
                keys.register_env_var(&p.id, var);
            }
        }
    }

    fn key_state(p: &Preset, keys: &KeyStore) -> KeyState {
        if !p.key_required {
            return KeyState::NotNeeded;
        }
        match keys.preview(&p.id) {
            Ok(Some(k)) => KeyState::Set(k),
            Ok(None) => KeyState::Missing,
            Err(KeyError::Locked) => KeyState::Locked,
            Err(e) => KeyState::Error(crate::security::redact(&e.to_string()).into_owned()),
        }
    }

    pub fn views(&self, keys: &KeyStore, disabled: &[String]) -> Vec<ProviderView> {
        let reg = self.registry.read().unwrap();
        reg.all()
            .map(|p| {
                let key = Self::key_state(p, keys);
                let configured = matches!(key, KeyState::NotNeeded | KeyState::Set(_));
                ProviderView {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    icon: p.icon.clone(),
                    protocol: p.protocol,
                    base_url: p.base_url.clone(),
                    key_env: p.key_env.clone(),
                    signup_url: p.signup_url.clone(),
                    docs_url: p.docs_url.clone(),
                    checked: p.checked.clone(),
                    free_tier: p.free_tier,
                    free_tier_note: p.free_tier_note.clone(),
                    region: p.region.clone(),
                    tags: p.tags.clone(),
                    key,
                    configured,
                    disabled: disabled.contains(&p.id),
                    user_pack: reg.origin(&p.id) == Some(Origin::User),
                }
            })
            .collect()
    }

    /// Recompute which provider hosts may be contacted.
    pub fn refresh_allowlist(&self, keys: &KeyStore, disabled: &[String]) {
        let hosts: Vec<String> = self
            .views(keys, disabled)
            .into_iter()
            .filter(|v| v.configured && !v.disabled)
            .filter_map(|v| self.preset(&v.id).and_then(|p| p.host()))
            .collect();
        self.client.allowlist().replace_source(ALLOW_SOURCE, hosts);
    }

    pub fn endpoint(&self, id: &str, keys: &KeyStore) -> Result<Endpoint, ProviderError> {
        let preset = self.preset(id).ok_or_else(|| ProviderError::Other(format!("unknown provider {id:?}")))?;
        let key = if preset.key_required {
            match keys.get(id) {
                Ok(Some(k)) => Some(Arc::new(k)),
                Ok(None) => return Err(ProviderError::NoKey(preset.name.clone())),
                Err(KeyError::Locked) => {
                    return Err(ProviderError::Other("unlock the encrypted key file first (Providers page)".into()));
                }
                Err(e) => return Err(ProviderError::Other(e.to_string())),
            }
        } else {
            keys.get(id).ok().flatten().map(Arc::new)
        };
        Ok(Endpoint { base_url: preset.base_url.clone(), preset: Arc::new(preset), key, client: self.client.clone() })
    }

    pub fn provider(&self, id: &str, keys: &KeyStore) -> Result<Box<dyn Provider>, ProviderError> {
        Ok(adapters::build(self.endpoint(id, keys)?))
    }

    pub fn record_ratelimit(&self, provider_id: &str, info: RateLimitInfo) {
        self.ratelimits.lock().unwrap().insert(provider_id.to_string(), info);
    }

    pub fn ratelimits(&self) -> HashMap<String, RateLimitInfo> {
        self.ratelimits.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::catalog::Snapshot;
    use crate::providers::preset::source_builtin_dir;
    use crate::security::keys::tests_support::memory_store;
    use age::secrecy::SecretString;

    fn hub() -> Hub {
        let reg = PresetRegistry::load(&source_builtin_dir(), None).unwrap();
        Hub::new(reg, Catalog::new(Snapshot::default()), Allowlist::new())
    }

    #[test]
    fn only_configured_enabled_providers_are_reachable() {
        let h = hub();
        let keys = memory_store();
        h.register_env_vars(&keys);
        h.refresh_allowlist(&keys, &[]);
        let list = h.client().allowlist();
        assert!(list.is_allowed("127.0.0.1"), "local Ollama needs no key");
        assert!(!list.is_allowed("api.groq.com"), "no key, no traffic");

        keys.set("groq", SecretString::from("gsk_test_key_value_123456")).unwrap();
        h.refresh_allowlist(&keys, &[]);
        assert!(list.is_allowed("api.groq.com"));

        h.refresh_allowlist(&keys, &["groq".to_string()]);
        assert!(!list.is_allowed("api.groq.com"), "disabled providers are cut off");
    }

    #[test]
    fn views_never_contain_the_full_key() {
        let h = hub();
        let keys = memory_store();
        let key = "gsk_abcdefghijklmnopqrstuvwxyz0123";
        keys.set("groq", SecretString::from(key)).unwrap();
        let json = serde_json::to_string(&h.views(&keys, &[])).unwrap();
        assert!(!json.contains(key));
        let groq = h.views(&keys, &[]).into_iter().find(|v| v.id == "groq").unwrap();
        assert!(groq.configured);
        assert!(matches!(groq.key, KeyState::Set(_)));
    }

    #[test]
    fn endpoint_without_key_is_an_error() {
        let h = hub();
        let keys = memory_store();
        assert!(matches!(h.endpoint("groq", &keys), Err(ProviderError::NoKey(_))));
        assert!(h.endpoint("ollama", &keys).is_ok());
        assert!(h.endpoint("nope", &keys).is_err());
    }
}
