use super::settings::Settings;
use crate::security::redact;
use serde::{Serialize, de::DeserializeOwned};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Db(String),
    #[error("could not encode data: {0}")]
    Encode(String),
}

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    pub async fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| StorageError::Db(e.to_string()))?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .map_err(db_err)?;
        Self::migrate(pool).await
    }

    pub async fn open_in_memory() -> Result<Self, StorageError> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(db_err)?
            .foreign_keys(true);
        // One connection: every in-memory connection is its own database.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .map_err(db_err)?;
        Self::migrate(pool).await
    }

    async fn migrate(pool: SqlitePool) -> Result<Self, StorageError> {
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| StorageError::Db(e.to_string()))?;
        Ok(Self { pool })
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StorageError> {
        let raw: Option<String> = sqlx::query_scalar("SELECT json FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        raw.map(|r| serde_json::from_str(&r).map_err(|e| StorageError::Encode(e.to_string())))
            .transpose()
    }

    /// Store a JSON value. The serialized text is redacted first, so a key that
    /// ends up in a value by mistake is never written to disk.
    pub async fn put_json<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StorageError> {
        let json = serde_json::to_string(value).map_err(|e| StorageError::Encode(e.to_string()))?;
        let json = redact(&json);
        sqlx::query(
            "INSERT INTO settings (key, json) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET json = excluded.json",
        )
        .bind(key)
        .bind(json.as_ref())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// Unreadable settings (corrupted, or written by an incompatible version)
    /// fall back to defaults so the app always opens.
    pub async fn settings(&self) -> Result<Settings, StorageError> {
        match self.get_json::<Settings>(SETTINGS_KEY).await {
            Ok(s) => Ok(s.unwrap_or_default()),
            Err(StorageError::Encode(e)) => {
                tracing::warn!("settings unreadable, using defaults: {e}");
                Ok(Settings::default())
            }
            Err(e) => Err(e),
        }
    }

    pub async fn set_settings(&self, settings: &Settings) -> Result<(), StorageError> {
        self.put_json(SETTINGS_KEY, settings).await
    }
}

const SETTINGS_KEY: &str = "app";

pub(crate) fn db_err(e: sqlx::Error) -> StorageError {
    StorageError::Db(redact(&e.to_string()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Theme, UpdateCheck};

    async fn tables(db: &Db) -> Vec<String> {
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx%' AND name NOT LIKE 'messages_fts_%' ORDER BY name")
            .fetch_all(&db.pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn migrations_apply_on_empty_db() {
        let db = Db::open_in_memory().await.unwrap();
        assert_eq!(
            tables(&db).await,
            [
                "allowlist_log",
                "messages",
                "messages_fts",
                "projects",
                "sessions",
                "settings",
                "spend"
            ]
        );
    }

    #[tokio::test]
    async fn opening_a_file_twice_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("lampcode.db");
        let s = Settings {
            theme: Theme::Light,
            update_check: UpdateCheck::Disabled,
            ..Default::default()
        };
        Db::open(&path).await.unwrap().set_settings(&s).await.unwrap();
        assert_eq!(Db::open(&path).await.unwrap().settings().await.unwrap(), s);
    }

    #[tokio::test]
    async fn settings_default_when_empty() {
        let db = Db::open_in_memory().await.unwrap();
        let s = db.settings().await.unwrap();
        assert_eq!(s, Settings::default());
        assert_eq!(s.update_check, UpdateCheck::NotAsked);
    }

    #[tokio::test]
    async fn settings_round_trip() {
        let db = Db::open_in_memory().await.unwrap();
        let s = Settings {
            theme: Theme::System,
            update_check: UpdateCheck::Enabled,
            favorite_models: vec!["groq/llama".into()],
            ..Default::default()
        };
        db.set_settings(&s).await.unwrap();
        assert_eq!(db.settings().await.unwrap(), s);
    }

    #[tokio::test]
    async fn corrupt_settings_row_falls_back_to_defaults() {
        let db = Db::open_in_memory().await.unwrap();
        sqlx::query("INSERT INTO settings (key, json) VALUES ('app', '{not json')")
            .execute(&db.pool)
            .await
            .unwrap();
        assert_eq!(db.settings().await.unwrap(), Settings::default());
    }

    #[tokio::test]
    async fn unknown_settings_fields_are_ignored() {
        let db = Db::open_in_memory().await.unwrap();
        sqlx::query(r#"INSERT INTO settings (key, json) VALUES ('app', '{"theme":"light","fromTheFuture":1}')"#)
            .execute(&db.pool)
            .await
            .unwrap();
        assert_eq!(db.settings().await.unwrap().theme, Theme::Light);
    }

    #[tokio::test]
    async fn put_json_redacts_secrets_before_writing() {
        let db = Db::open_in_memory().await.unwrap();
        let key = ["gsk", "_", "TESTONLYabcdefghijklmnopqrstuvwxyz0123456789"].concat();
        db.put_json("note", &serde_json::json!({ "text": format!("my key is {key}") }))
            .await
            .unwrap();
        let raw: String = sqlx::query_scalar("SELECT json FROM settings WHERE key = 'note'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert!(!raw.contains(&key), "secret written to db: {raw}");
        assert!(raw.contains("[REDACTED:groq]"));
    }

    #[test]
    fn settings_have_no_secret_shaped_fields() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        for field in json.as_object().unwrap().keys() {
            let f = field.to_lowercase();
            assert!(
                !["key", "token", "secret", "password"].iter().any(|w| f.contains(w)),
                "Settings field `{field}` looks like it holds a secret; secrets belong in the key store"
            );
        }
    }
}
