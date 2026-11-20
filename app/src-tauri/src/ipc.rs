//! Typed IPC surface. Every command and event the frontend can use is
//! registered here; `app/src/ipc/bindings.ts` is generated from this builder
//! and must never be edited by hand.

use crate::agent::{SessionEvent, TurnInput, run_turn};
use crate::providers::catalog::{ModelEntry, SNAPSHOT_FILE, Snapshot};
use crate::providers::hub::{Hub, KeyState, ProviderView};
use crate::providers::{ProviderError, RateLimitInfo, adapters};
use crate::security::keys::{self, KeyPreview, KeyStore, KeychainSelfTest, OsKeychain, SecretBackend};
use crate::security::net::BlockedAttempt;
use crate::storage::sessions::{Project, SearchHit, SessionSummary, StoredMessage};
use crate::storage::{Db, ModelRef, Settings};
use age::secrecy::SecretString;
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, Runtime, State, Wry};
use tauri_specta::{Builder, collect_commands};
use tokio_util::sync::CancellationToken;

/// Path of the generated bindings, relative to `src-tauri/`.
pub const BINDINGS_PATH: &str = "../src/ipc/bindings.ts";

pub const KEYCHAIN_SERVICE: &str = "LampCode";
const KEYCHAIN_SELFTEST_SERVICE: &str = "LampCode-selftest";

/// Running turns, so they can be cancelled.
#[derive(Default)]
pub struct Turns(Mutex<HashMap<String, CancellationToken>>);

fn ui_err(e: impl std::fmt::Display) -> String {
    crate::security::redact(&e.to_string()).into_owned()
}

fn provider_err(e: ProviderError) -> String {
    e.display_redacted()
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub data_dir: String,
}

#[tauri::command]
#[specta::specta]
fn app_info<R: Runtime>(app: AppHandle<R>) -> Result<AppInfo, String> {
    let info = app.package_info();
    let data_dir = app.path().app_data_dir().map_err(ui_err)?;
    Ok(AppInfo { name: info.name.clone(), version: info.version.to_string(), data_dir: data_dir.display().to_string() })
}

async fn refresh_allowlist(hub: &Hub, keys: &KeyStore, db: &Db) {
    let disabled = db.settings().await.map(|s| s.disabled_providers).unwrap_or_default();
    hub.refresh_allowlist(keys, &disabled);
}

// ---- keys ----

/// The frontend sends a key once, here; it never gets the full key back.
#[tauri::command]
#[specta::specta]
async fn key_set(
    keys: State<'_, KeyStore>,
    hub: State<'_, Hub>,
    db: State<'_, Db>,
    provider_id: String,
    key: String,
) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("The key is empty.".into());
    }
    keys.set(&provider_id, SecretString::from(key)).map_err(ui_err)?;
    refresh_allowlist(&hub, &keys, &db).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn key_delete(keys: State<'_, KeyStore>, hub: State<'_, Hub>, db: State<'_, Db>, provider_id: String) -> Result<(), String> {
    keys.delete(&provider_id).map_err(ui_err)?;
    refresh_allowlist(&hub, &keys, &db).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn key_preview(keys: State<'_, KeyStore>, provider_id: String) -> Result<Option<KeyPreview>, String> {
    keys.preview(&provider_id).map_err(ui_err)
}

/// Unlock the encrypted key file (only used when there is no OS keychain).
/// The passphrase stays in memory until the app quits.
#[tauri::command]
#[specta::specta]
async fn age_unlock(keys: State<'_, KeyStore>, hub: State<'_, Hub>, db: State<'_, Db>, passphrase: String) -> Result<(), String> {
    if passphrase.is_empty() {
        return Err("The passphrase is empty.".into());
    }
    keys.age().unlock(SecretString::from(passphrase));
    if keys.age().exists() {
        // Fail fast on a wrong passphrase instead of on the next key lookup.
        keys.age().get("__probe__").map_err(ui_err)?;
    }
    refresh_allowlist(&hub, &keys, &db).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn keychain_self_test() -> KeychainSelfTest {
    keys::self_test(&OsKeychain::new(KEYCHAIN_SELFTEST_SERVICE))
}

// ---- settings ----

#[tauri::command]
#[specta::specta]
async fn settings_get(db: State<'_, Db>) -> Result<Settings, String> {
    db.settings().await.map_err(ui_err)
}

#[tauri::command]
#[specta::specta]
async fn settings_set(db: State<'_, Db>, hub: State<'_, Hub>, keys: State<'_, KeyStore>, settings: Settings) -> Result<(), String> {
    db.set_settings(&settings).await.map_err(ui_err)?;
    hub.refresh_allowlist(&keys, &settings.disabled_providers);
    Ok(())
}

// ---- providers and models ----

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProvidersState {
    pub providers: Vec<ProviderView>,
    /// Problems with user provider packs.
    pub warnings: Vec<String>,
    pub age_locked: bool,
}

#[tauri::command]
#[specta::specta]
async fn providers_list(hub: State<'_, Hub>, keys: State<'_, KeyStore>, db: State<'_, Db>) -> Result<ProvidersState, String> {
    let disabled = db.settings().await.map_err(ui_err)?.disabled_providers;
    let providers = hub.views(&keys, &disabled);
    let age_locked = providers.iter().any(|p| matches!(p.key, KeyState::Locked));
    Ok(ProvidersState { providers, warnings: hub.warnings(), age_locked })
}

/// Models for one provider: the live list if it was fetched, else the snapshot.
#[tauri::command]
#[specta::specta]
fn models_list(hub: State<'_, Hub>, provider_id: String) -> Result<Vec<ModelEntry>, String> {
    let preset = hub.preset(&provider_id).ok_or("unknown provider")?;
    Ok(hub.catalog.models(&preset))
}

/// Fetch the provider's live `/models` list.
#[tauri::command]
#[specta::specta]
async fn models_refresh(hub: State<'_, Hub>, keys: State<'_, KeyStore>, provider_id: String) -> Result<Vec<ModelEntry>, String> {
    let provider = hub.provider(&provider_id, &keys).map_err(provider_err)?;
    let live = provider.list_models().await.map_err(provider_err)?;
    hub.catalog.set_live(&provider_id, live);
    let preset = hub.preset(&provider_id).ok_or("unknown provider")?;
    Ok(hub.catalog.models(&preset))
}

/// Date of the model catalog snapshot in use.
#[tauri::command]
#[specta::specta]
fn catalog_info(hub: State<'_, Hub>) -> String {
    hub.catalog.snapshot_date()
}

/// Manual, user-initiated download of fresh models.dev data. models.dev is
/// allowed for this one request only.
#[tauri::command]
#[specta::specta]
async fn catalog_refresh<R: Runtime>(app: AppHandle<R>, hub: State<'_, Hub>) -> Result<String, String> {
    let rb = hub
        .client()
        .request_once_to(reqwest::Method::GET, "https://models.dev/api.json", "models.dev")
        .map_err(ui_err)?;
    let resp = rb.send().await.map_err(|e| provider_err(ProviderError::from_reqwest(e)))?;
    if !resp.status().is_success() {
        return Err(format!("models.dev returned {}", resp.status()));
    }
    let raw: serde_json::Value = resp.json().await.map_err(ui_err)?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let snap = Snapshot::from_models_dev(&raw, &today)?;
    let dir = app.path().app_data_dir().map_err(ui_err)?;
    std::fs::create_dir_all(&dir).map_err(ui_err)?;
    std::fs::write(dir.join(SNAPSHOT_FILE), serde_json::to_string(&snap).map_err(ui_err)?).map_err(ui_err)?;
    hub.catalog.replace_snapshot(snap);
    Ok(today)
}

/// Last rate-limit info seen per provider (feeds the headroom bars).
#[tauri::command]
#[specta::specta]
fn ratelimits(hub: State<'_, Hub>) -> HashMap<String, RateLimitInfo> {
    hub.ratelimits()
}

#[tauri::command]
#[specta::specta]
async fn ollama_pull(hub: State<'_, Hub>, keys: State<'_, KeyStore>, model: String) -> Result<(), String> {
    let ep = hub.endpoint("ollama", &keys).map_err(provider_err)?;
    adapters::ollama::Ollama(ep).pull(&model).await.map_err(provider_err)
}

#[tauri::command]
#[specta::specta]
async fn ollama_delete(hub: State<'_, Hub>, keys: State<'_, KeyStore>, model: String) -> Result<(), String> {
    let ep = hub.endpoint("ollama", &keys).map_err(provider_err)?;
    adapters::ollama::Ollama(ep).delete(&model).await.map_err(provider_err)
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AllowedHost {
    pub host: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    pub allowed: Vec<AllowedHost>,
    pub blocked: Vec<BlockedAttempt>,
}

#[tauri::command]
#[specta::specta]
fn network_status(hub: State<'_, Hub>) -> NetworkStatus {
    let list = hub.client().allowlist();
    NetworkStatus {
        allowed: list.entries().into_iter().map(|(host, reason)| AllowedHost { host, reason }).collect(),
        blocked: list.blocked(),
    }
}

// ---- projects and sessions ----

#[tauri::command]
#[specta::specta]
async fn projects_list(db: State<'_, Db>) -> Result<Vec<Project>, String> {
    db.projects().await.map_err(ui_err)
}

/// Opens the OS folder picker. Returns `None` if the user cancels.
#[tauri::command]
#[specta::specta]
async fn project_pick<R: Runtime>(app: AppHandle<R>, db: State<'_, Db>) -> Result<Option<Project>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().set_title("Open a project folder").pick_folder(move |f| {
        let _ = tx.send(f);
    });
    let Some(folder) = rx.await.map_err(ui_err)? else { return Ok(None) };
    let path = folder.into_path().map_err(ui_err)?;
    db.upsert_project(&path.display().to_string()).await.map(Some).map_err(ui_err)
}

#[tauri::command]
#[specta::specta]
async fn project_remove(db: State<'_, Db>, project_id: String) -> Result<(), String> {
    db.remove_project(&project_id).await.map_err(ui_err)
}

#[tauri::command]
#[specta::specta]
async fn sessions_list(db: State<'_, Db>) -> Result<Vec<SessionSummary>, String> {
    db.sessions().await.map_err(ui_err)
}

#[tauri::command]
#[specta::specta]
async fn session_create(db: State<'_, Db>, project_id: Option<String>, model: Option<ModelRef>) -> Result<SessionSummary, String> {
    db.create_session(project_id.as_deref(), model.as_ref()).await.map_err(ui_err)
}

#[tauri::command]
#[specta::specta]
async fn session_rename(db: State<'_, Db>, session_id: String, title: String) -> Result<(), String> {
    db.rename_session(&session_id, &title).await.map_err(ui_err)
}

#[tauri::command]
#[specta::specta]
async fn session_delete(db: State<'_, Db>, turns: State<'_, Turns>, session_id: String) -> Result<(), String> {
    let running = turns.0.lock().unwrap().remove(&session_id);
    if let Some(t) = running {
        t.cancel();
    }
    db.delete_session(&session_id).await.map_err(ui_err)
}

#[tauri::command]
#[specta::specta]
async fn session_messages(db: State<'_, Db>, session_id: String) -> Result<Vec<StoredMessage>, String> {
    db.messages(&session_id).await.map_err(ui_err)
}

#[tauri::command]
#[specta::specta]
async fn sessions_search(db: State<'_, Db>, query: String) -> Result<Vec<SearchHit>, String> {
    db.search(&query).await.map_err(ui_err)
}

// ---- chat ----

/// Run one chat turn. Progress streams through `on_event` (errors too); the
/// command returns when the turn is over.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
async fn chat_send(
    db: State<'_, Db>,
    hub: State<'_, Hub>,
    keys: State<'_, KeyStore>,
    turns: State<'_, Turns>,
    session_id: String,
    model: ModelRef,
    text: String,
    on_event: Channel<SessionEvent>,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("The message is empty.".into());
    }
    let provider = hub.provider(&model.provider_id, &keys).map_err(provider_err)?;
    let prices = hub.preset(&model.provider_id).and_then(|p| {
        hub.catalog.models(&p).into_iter().find(|m| m.id == model.model_id).and_then(|m| m.price_in.zip(m.price_out))
    });
    let cancel = CancellationToken::new();
    {
        let mut map = turns.0.lock().unwrap();
        if map.contains_key(&session_id) {
            return Err("A reply is still running in this chat.".into());
        }
        map.insert(session_id.clone(), cancel.clone());
    }
    let mut settings = db.settings().await.unwrap_or_default();
    if settings.last_model.as_ref() != Some(&model) {
        settings.last_model = Some(model.clone());
        let _ = db.set_settings(&settings).await;
    }
    let hub_ref: &Hub = &hub;
    let emit = |ev: SessionEvent| {
        if let SessionEvent::RateLimit { provider_id, info } = &ev {
            hub_ref.record_ratelimit(provider_id, info.clone());
        }
        let _ = on_event.send(ev);
    };
    let input = TurnInput { db: &db, provider: provider.as_ref(), model, session_id: &session_id, user_text: text, prices, cancel };
    // Failures were already sent as an `Error` event.
    let _ = run_turn(input, &emit).await;
    turns.0.lock().unwrap().remove(&session_id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn chat_cancel(turns: State<'_, Turns>, session_id: String) {
    if let Some(t) = turns.0.lock().unwrap().get(&session_id) {
        t.cancel();
    }
}

pub fn builder() -> Builder<Wry> {
    Builder::<Wry>::new().commands(collect_commands![
        app_info::<Wry>,
        key_set,
        key_delete,
        key_preview,
        age_unlock,
        keychain_self_test,
        settings_get,
        settings_set,
        providers_list,
        models_list,
        models_refresh,
        catalog_info,
        catalog_refresh::<Wry>,
        ratelimits,
        ollama_pull,
        ollama_delete,
        network_status,
        projects_list,
        project_pick::<Wry>,
        project_remove,
        sessions_list,
        session_create,
        session_rename,
        session_delete,
        session_messages,
        sessions_search,
        chat_send,
        chat_cancel,
    ])
}

#[cfg(test)]
fn export_to(path: &std::path::Path) {
    builder()
        .export(
            specta_typescript::Typescript::default()
                .header("// GENERATED by tauri-specta from src-tauri/src/ipc.rs. Do not edit.\n// Regenerate with `pnpm gen:ipc`.\n"),
            path,
        )
        .expect("failed to export typescript bindings");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn committed() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(BINDINGS_PATH)
    }

    fn normalize(s: &str) -> String {
        s.replace("\r\n", "\n")
    }

    /// Writes the real bindings file. Run via `pnpm gen:ipc`.
    #[test]
    #[ignore]
    fn export_bindings() {
        let path = committed();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        export_to(&path);
    }

    #[test]
    fn bindings_are_up_to_date() {
        let tmp = std::env::temp_dir().join(format!("lampcode-bindings-{}.ts", std::process::id()));
        export_to(&tmp);
        let fresh = normalize(&std::fs::read_to_string(&tmp).unwrap());
        let _ = std::fs::remove_file(&tmp);
        let on_disk = std::fs::read_to_string(committed()).map(|s| normalize(&s)).unwrap_or_default();
        assert!(fresh == on_disk, "app/src/ipc/bindings.ts is stale or missing; run `pnpm gen:ipc` and commit it");
    }
}
