pub mod agent;
pub mod ipc;
pub mod providers;
pub mod security;
pub mod storage;

use providers::catalog::Catalog;
use providers::hub::Hub;
use providers::preset::PresetRegistry;
use security::keys::{AgeFile, KeyStore, OsKeychain};
use security::net::Allowlist;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = ipc::builder();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let data_dir = app.path().app_data_dir()?;
            let resources = app.path().resource_dir()?.join("resources");
            let db = tauri::async_runtime::block_on(storage::Db::open(&data_dir.join("lampcode.db")))?;
            let keys = KeyStore::new(
                Box::new(OsKeychain::new(ipc::KEYCHAIN_SERVICE)),
                AgeFile::new(data_dir.join("keys.age")),
            );
            let registry = PresetRegistry::load(&resources.join("providers"), Some(&data_dir.join("providers")))?;
            for w in &registry.warnings {
                tracing::warn!("{w}");
            }
            let catalog = Catalog::load(&resources.join(providers::catalog::SNAPSHOT_FILE), &data_dir);
            let hub = Hub::new(registry, catalog, Allowlist::new());
            hub.register_env_vars(&keys);
            let disabled = tauri::async_runtime::block_on(db.settings()).map(|s| s.disabled_providers).unwrap_or_default();
            hub.refresh_allowlist(&keys, &disabled);
            app.manage(db);
            app.manage(keys);
            app.manage(hub);
            app.manage(ipc::Turns::default());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
