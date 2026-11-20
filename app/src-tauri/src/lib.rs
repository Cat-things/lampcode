pub mod ipc;
pub mod security;
pub mod storage;

use security::keys::{AgeFile, KeyStore, OsKeychain};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = ipc::builder();

    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let data_dir = app.path().app_data_dir()?;
            let db = tauri::async_runtime::block_on(storage::Db::open(&data_dir.join("lampcode.db")))?;
            app.manage(db);
            app.manage(KeyStore::new(
                Box::new(OsKeychain::new(ipc::KEYCHAIN_SERVICE)),
                AgeFile::new(data_dir.join("keys.age")),
            ));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
