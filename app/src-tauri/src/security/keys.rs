//! API key storage. Lookup order: environment variable, OS keychain, then an
//! age-encrypted file (only used when no keychain is available). Keys never
//! leave the backend in full: the frontend only ever sees [`KeyPreview`].

use age::secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use specta::Type;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub enum KeySource {
    Env,
    Keychain,
    AgeFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyPreview {
    pub source: KeySource,
    pub masked: String,
}

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("no OS keychain available: {0}")]
    Unavailable(String),
    #[error("the encrypted key file is locked; enter your passphrase")]
    Locked,
    #[error("wrong passphrase, or the encrypted key file is damaged")]
    BadPassphrase,
    #[error("key store error: {0}")]
    Other(String),
}

/// A place secrets can be stored. Implemented by the OS keychain and by tests.
pub trait SecretBackend: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<SecretString>, KeyError>;
    fn set(&self, id: &str, value: &SecretString) -> Result<(), KeyError>;
    fn delete(&self, id: &str) -> Result<(), KeyError>;
}

/// Mask a key for display: `sk-...abcd`. Short keys are fully masked.
pub fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() < 12 {
        return "****".into();
    }
    let head: String = chars[..3].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}...{tail}")
}

fn keychain_err(e: keyring::Error) -> KeyError {
    use keyring::Error as E;
    match e {
        E::NoStorageAccess(_) | E::PlatformFailure(_) | E::NoDefaultStore => {
            KeyError::Unavailable(crate::security::redact(&e.to_string()).into_owned())
        }
        other => KeyError::Other(crate::security::redact(&other.to_string()).into_owned()),
    }
}

/// OS keychain (Windows Credential Manager, Secret Service / KWallet on Linux).
pub struct OsKeychain {
    service: String,
}

impl OsKeychain {
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into() }
    }
}

impl OsKeychain {
    fn entry(&self, id: &str) -> Result<keyring::Entry, KeyError> {
        keyring::Entry::new(&self.service, id).map_err(keychain_err)
    }
}

impl SecretBackend for OsKeychain {
    fn get(&self, id: &str) -> Result<Option<SecretString>, KeyError> {
        match self.entry(id)?.get_password() {
            Ok(p) => Ok(Some(SecretString::from(p))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_err(e)),
        }
    }
    fn set(&self, id: &str, value: &SecretString) -> Result<(), KeyError> {
        self.entry(id)?.set_password(value.expose_secret()).map_err(keychain_err)
    }
    fn delete(&self, id: &str) -> Result<(), KeyError> {
        match self.entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain_err(e)),
        }
    }
}

/// Fallback store: one age file (scrypt passphrase) holding a JSON map of
/// provider id to key. The passphrase lives only in memory.
pub struct AgeFile {
    path: PathBuf,
    passphrase: Mutex<Option<SecretString>>,
    work_factor: u8,
}

impl AgeFile {
    pub fn new(path: PathBuf) -> Self {
        Self::with_work_factor(path, 18)
    }

    pub fn with_work_factor(path: PathBuf, work_factor: u8) -> Self {
        Self { path, passphrase: Mutex::new(None), work_factor }
    }

    pub fn unlock(&self, passphrase: SecretString) {
        *self.passphrase.lock().unwrap() = Some(passphrase);
    }

    pub fn is_unlocked(&self) -> bool {
        self.passphrase.lock().unwrap().is_some()
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    fn passphrase(&self) -> Result<SecretString, KeyError> {
        self.passphrase.lock().unwrap().clone().ok_or(KeyError::Locked)
    }

    /// Decrypt the whole map. Missing file = empty map (no passphrase needed).
    fn load(&self) -> Result<Zmap, KeyError> {
        if !self.exists() {
            return Ok(Zmap::default());
        }
        let pass = self.passphrase()?;
        let ciphertext = std::fs::read(&self.path).map_err(|e| KeyError::Other(e.to_string()))?;
        let mut identity = age::scrypt::Identity::new(pass);
        identity.set_max_work_factor(self.work_factor.max(18));
        let plain = zeroize::Zeroizing::new(
            age::decrypt(&identity, &ciphertext).map_err(|_| KeyError::BadPassphrase)?,
        );
        let map: BTreeMap<String, String> =
            serde_json::from_slice(&plain).map_err(|_| KeyError::BadPassphrase)?;
        Ok(Zmap(map))
    }

    fn save(&self, map: &Zmap) -> Result<(), KeyError> {
        let pass = self.passphrase()?;
        let plain = zeroize::Zeroizing::new(
            serde_json::to_vec(&map.0).map_err(|e| KeyError::Other(e.to_string()))?,
        );
        let mut recipient = age::scrypt::Recipient::new(pass);
        recipient.set_work_factor(self.work_factor);
        let ciphertext =
            age::encrypt(&recipient, &plain).map_err(|e| KeyError::Other(e.to_string()))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| KeyError::Other(e.to_string()))?;
        }
        // Write-then-rename so a crash never leaves a half-written key file.
        let tmp = self.path.with_extension("age.tmp");
        std::fs::write(&tmp, &ciphertext).map_err(|e| KeyError::Other(e.to_string()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| KeyError::Other(e.to_string()))
    }
}

/// Decrypted key map that wipes its values when dropped.
#[derive(Default)]
struct Zmap(BTreeMap<String, String>);

impl Drop for Zmap {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        for v in self.0.values_mut() {
            v.zeroize();
        }
    }
}

impl SecretBackend for AgeFile {
    fn get(&self, id: &str) -> Result<Option<SecretString>, KeyError> {
        Ok(self.load()?.0.get(id).map(|v| SecretString::from(v.clone())))
    }
    fn set(&self, id: &str, value: &SecretString) -> Result<(), KeyError> {
        // Check the passphrase up front so a locked store never touches disk.
        self.passphrase()?;
        let mut map = self.load()?;
        map.0.insert(id.to_string(), value.expose_secret().to_string());
        self.save(&map)
    }
    fn delete(&self, id: &str) -> Result<(), KeyError> {
        if !self.exists() {
            return Ok(());
        }
        let mut map = self.load()?;
        if map.0.remove(id).is_some() {
            self.save(&map)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct KeychainSelfTest {
    pub ok: bool,
    pub message: String,
}

/// Write, read back and delete a throwaway value. Never touches real keys.
pub fn self_test(backend: &dyn SecretBackend) -> KeychainSelfTest {
    const ID: &str = "lampcode-selftest";
    let probe = SecretString::from("lampcode-selftest-value-0123456789");
    let run = || -> Result<(), KeyError> {
        backend.set(ID, &probe)?;
        let read = backend.get(ID)?;
        backend.delete(ID)?;
        match read {
            Some(v) if v.expose_secret() == probe.expose_secret() => Ok(()),
            Some(_) => Err(KeyError::Other("read back a different value".into())),
            None => Err(KeyError::Other("value was not stored".into())),
        }
    };
    match run() {
        Ok(()) => KeychainSelfTest { ok: true, message: "Keychain works: wrote, read and deleted a test value.".into() },
        Err(e) => KeychainSelfTest { ok: false, message: e.to_string() },
    }
}

type EnvLookup =Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct KeyStore {
    env_lookup: EnvLookup,
    env_vars: Mutex<HashMap<String, String>>,
    keychain: Box<dyn SecretBackend>,
    age: AgeFile,
}

impl KeyStore {
    pub fn new(keychain: Box<dyn SecretBackend>, age: AgeFile) -> Self {
        Self::with_env(keychain, age, Box::new(|name| std::env::var(name).ok()))
    }

    pub fn with_env(keychain: Box<dyn SecretBackend>, age: AgeFile, env_lookup: EnvLookup) -> Self {
        let defaults = [
            ("openrouter", "OPENROUTER_API_KEY"),
            ("groq", "GROQ_API_KEY"),
            ("nvidia", "NVIDIA_API_KEY"),
            ("gemini", "GEMINI_API_KEY"),
        ];
        Self {
            env_lookup,
            env_vars: Mutex::new(defaults.iter().map(|(p, e)| (p.to_string(), e.to_string())).collect()),
            keychain,
            age,
        }
    }

    /// Associate a provider with the env var that overrides its saved key.
    pub fn register_env_var(&self, provider_id: &str, env_var: &str) {
        self.env_vars
            .lock()
            .unwrap()
            .insert(provider_id.to_string(), env_var.to_string());
    }

    pub fn age(&self) -> &AgeFile {
        &self.age
    }

    fn resolve(&self, provider_id: &str) -> Result<Option<(SecretString, KeySource)>, KeyError> {
        let env_var = self.env_vars.lock().unwrap().get(provider_id).cloned();
        if let Some(v) = env_var.and_then(|name| (self.env_lookup)(&name)) {
            if !v.trim().is_empty() {
                return Ok(Some((SecretString::from(v), KeySource::Env)));
            }
        }
        match self.keychain.get(provider_id) {
            Ok(Some(k)) => return Ok(Some((k, KeySource::Keychain))),
            Ok(None) | Err(KeyError::Unavailable(_)) => {}
            Err(e) => return Err(e),
        }
        Ok(self.age.get(provider_id)?.map(|k| (k, KeySource::AgeFile)))
    }

    pub fn get(&self, provider_id: &str) -> Result<Option<SecretString>, KeyError> {
        Ok(self.resolve(provider_id)?.map(|(k, _)| k))
    }

    /// Save to the keychain; only if no keychain exists, to the age file.
    /// Never falls back to plaintext.
    pub fn set(&self, provider_id: &str, value: SecretString) -> Result<(), KeyError> {
        match self.keychain.set(provider_id, &value) {
            Err(KeyError::Unavailable(_)) => self.age.set(provider_id, &value),
            other => other,
        }
    }

    pub fn delete(&self, provider_id: &str) -> Result<(), KeyError> {
        match self.keychain.delete(provider_id) {
            Ok(()) | Err(KeyError::Unavailable(_)) => {}
            Err(e) => return Err(e),
        }
        // A locked, existing age file returns Locked: the UI asks to unlock first.
        self.age.delete(provider_id)
    }

    pub fn preview(&self, provider_id: &str) -> Result<Option<KeyPreview>, KeyError> {
        Ok(self.resolve(provider_id)?.map(|(k, source)| KeyPreview {
            source,
            masked: mask(k.expose_secret()),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemKeychain(Mutex<HashMap<String, String>>);
    impl SecretBackend for MemKeychain {
        fn get(&self, id: &str) -> Result<Option<SecretString>, KeyError> {
            Ok(self.0.lock().unwrap().get(id).map(|s| SecretString::from(s.clone())))
        }
        fn set(&self, id: &str, v: &SecretString) -> Result<(), KeyError> {
            self.0.lock().unwrap().insert(id.into(), v.expose_secret().into());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), KeyError> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }

    /// Simulates a locked / missing Secret Service.
    struct NoKeychain;
    impl SecretBackend for NoKeychain {
        fn get(&self, _: &str) -> Result<Option<SecretString>, KeyError> {
            Err(KeyError::Unavailable("no secret service".into()))
        }
        fn set(&self, _: &str, _: &SecretString) -> Result<(), KeyError> {
            Err(KeyError::Unavailable("no secret service".into()))
        }
        fn delete(&self, _: &str) -> Result<(), KeyError> {
            Err(KeyError::Unavailable("no secret service".into()))
        }
    }

    const KEY: &str = "gsk_TESTONLYabcdefghijklmnopqrstuvwxyz0123456789WXYZ";

    fn age_in(dir: &tempfile::TempDir) -> AgeFile {
        // Low scrypt work factor keeps tests fast; production uses 18.
        AgeFile::with_work_factor(dir.path().join("keys.age"), 10)
    }

    fn no_env() -> EnvLookup {
        Box::new(|_| None)
    }

    #[test]
    fn env_overrides_saved_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::with_env(
            Box::new(MemKeychain::default()),
            age_in(&dir),
            Box::new(|name| (name == "GROQ_API_KEY").then(|| "gsk_FROMENVxxxxxxxxxxxxxxxx1234".into())),
        );
        store.set("groq", SecretString::from(KEY)).unwrap();
        let got = store.get("groq").unwrap().unwrap();
        assert_eq!(got.expose_secret(), "gsk_FROMENVxxxxxxxxxxxxxxxx1234");
        assert_eq!(store.preview("groq").unwrap().unwrap().source, KeySource::Env);
    }

    #[test]
    fn empty_env_var_does_not_override() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::with_env(Box::new(MemKeychain::default()), age_in(&dir), Box::new(|_| Some(String::new())));
        store.set("groq", SecretString::from(KEY)).unwrap();
        assert_eq!(store.preview("groq").unwrap().unwrap().source, KeySource::Keychain);
    }

    #[test]
    fn registered_env_var_is_used() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::with_env(
            Box::new(MemKeychain::default()),
            age_in(&dir),
            Box::new(|name| (name == "ITERA_API_KEY").then(|| "itera-env-key-0000000000000".into())),
        );
        assert!(store.get("iteracompute").unwrap().is_none());
        store.register_env_var("iteracompute", "ITERA_API_KEY");
        assert!(store.get("iteracompute").unwrap().is_some());
    }

    #[test]
    fn preview_never_contains_full_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::with_env(Box::new(MemKeychain::default()), age_in(&dir), no_env());
        store.set("groq", SecretString::from(KEY)).unwrap();
        let p = store.preview("groq").unwrap().unwrap();
        assert_eq!(p.masked, "gsk...WXYZ");
        assert_eq!(p.source, KeySource::Keychain);
        assert!(!format!("{p:?}").contains("abcdefghijklmnop"));
    }

    #[test]
    fn short_key_preview_is_fully_masked() {
        assert_eq!(mask("abc123"), "****");
        assert_eq!(mask("12345678901"), "****");
    }

    #[test]
    fn missing_key_has_no_preview() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::with_env(Box::new(MemKeychain::default()), age_in(&dir), no_env());
        assert!(store.preview("groq").unwrap().is_none());
    }

    #[test]
    fn locked_keychain_falls_back_to_age() {
        let dir = tempfile::tempdir().unwrap();
        let age = age_in(&dir);
        age.unlock(SecretString::from("correct horse battery staple"));
        let store = KeyStore::with_env(Box::new(NoKeychain), age, no_env());
        store.set("groq", SecretString::from(KEY)).unwrap();
        assert_eq!(store.get("groq").unwrap().unwrap().expose_secret(), KEY);
        assert_eq!(store.preview("groq").unwrap().unwrap().source, KeySource::AgeFile);
        let on_disk = std::fs::read(dir.path().join("keys.age")).unwrap();
        assert!(!String::from_utf8_lossy(&on_disk).contains("TESTONLY"), "key written in plaintext");
    }

    #[test]
    fn no_keychain_and_no_passphrase_refuses_instead_of_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let store = KeyStore::with_env(Box::new(NoKeychain), age_in(&dir), no_env());
        let err = store.set("groq", SecretString::from(KEY)).unwrap_err();
        assert!(matches!(err, KeyError::Locked), "{err:?}");
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "something was written to disk");
    }

    #[test]
    fn age_file_round_trip_and_wrong_passphrase_fails() {
        let dir = tempfile::tempdir().unwrap();
        let a = age_in(&dir);
        a.unlock(SecretString::from("right passphrase"));
        a.set("groq", &SecretString::from(KEY)).unwrap();
        a.set("nvidia", &SecretString::from("nvapi-TESTONLY-0000000000000000000")).unwrap();

        let b = age_in(&dir);
        b.unlock(SecretString::from("right passphrase"));
        assert_eq!(b.get("groq").unwrap().unwrap().expose_secret(), KEY);
        assert!(b.get("nvidia").unwrap().is_some());
        assert!(b.get("openai").unwrap().is_none());

        let c = age_in(&dir);
        c.unlock(SecretString::from("wrong passphrase"));
        assert!(matches!(c.get("groq"), Err(KeyError::BadPassphrase)));

        let d = age_in(&dir);
        assert!(matches!(d.get("groq"), Err(KeyError::Locked)));
    }

    #[test]
    fn age_file_missing_means_no_keys() {
        let dir = tempfile::tempdir().unwrap();
        let a = age_in(&dir);
        assert!(!a.exists());
        assert!(a.get("groq").unwrap().is_none());
    }

    #[test]
    fn delete_removes_from_keychain_and_age() {
        let dir = tempfile::tempdir().unwrap();
        let age = age_in(&dir);
        age.unlock(SecretString::from("pw"));
        age.set("groq", &SecretString::from(KEY)).unwrap();
        let store = KeyStore::with_env(Box::new(MemKeychain::default()), age, no_env());
        store.set("groq", SecretString::from(KEY)).unwrap();
        store.delete("groq").unwrap();
        assert!(store.get("groq").unwrap().is_none());
    }

    #[test]
    fn secret_debug_is_redacted() {
        let s = SecretString::from(KEY);
        assert!(!format!("{s:?}").contains("TESTONLY"));
        let err = KeyError::Unavailable("platform said no".into());
        assert!(!format!("{err:?} {err}").contains("TESTONLY"));
    }

    #[test]
    fn self_test_ok_with_working_backend_and_leaves_nothing_behind() {
        let kc = MemKeychain::default();
        let r = self_test(&kc);
        assert!(r.ok, "{}", r.message);
        assert!(kc.0.lock().unwrap().is_empty());
    }

    #[test]
    fn self_test_reports_missing_keychain() {
        let r = self_test(&NoKeychain);
        assert!(!r.ok);
        assert!(r.message.contains("no OS keychain"), "{}", r.message);
    }

    /// Touches the real OS keychain. Run manually: `cargo test -- --ignored os_keychain`.
    #[test]
    #[ignore]
    fn os_keychain_round_trip() {
        let kc = OsKeychain::new("LampCode-selftest");
        kc.set("selftest", &SecretString::from(KEY)).unwrap();
        assert_eq!(kc.get("selftest").unwrap().unwrap().expose_secret(), KEY);
        kc.delete("selftest").unwrap();
        assert!(kc.get("selftest").unwrap().is_none());
    }
}
