//! Projects, sessions and messages. Message text is redacted before it is
//! written, so a key pasted into a chat never lands in the database.

use super::db::{Db, StorageError, db_err};
use super::settings::ModelRef;
use crate::providers::{Role, ToolCall, Usage};
use crate::security::redact;
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::Row;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    /// Unix millis.
    #[specta(type = f64)]
    pub updated_at: i64,
    pub model: Option<ModelRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub id: String,
    pub role: Role,
    pub content: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub model: Option<ModelRef>,
    #[serde(default)]
    pub usage: Option<Usage>,
    /// USD for this turn, when known.
    #[serde(default)]
    pub cost: Option<f64>,
    /// Set when the turn ended in an error or was cancelled.
    #[serde(default)]
    pub error: Option<String>,
    /// Unix millis.
    #[specta(type = f64)]
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub session_id: String,
    pub title: String,
    pub snippet: String,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Turn free text into a safe FTS5 query: every word quoted, last one a prefix.
fn fts_query(q: &str) -> Option<String> {
    let words: Vec<String> = q
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\""))
        .collect();
    if words.is_empty() {
        return None;
    }
    Some(format!("{}*", words.join(" ")))
}

fn summary(r: &sqlx::sqlite::SqliteRow) -> SessionSummary {
    let provider: Option<String> = r.get("provider_id");
    let model: Option<String> = r.get("model_id");
    SessionSummary {
        id: r.get("id"),
        project_id: r.get("project_id"),
        title: r.get("title"),
        updated_at: r.get("updated_at"),
        model: provider
            .zip(model)
            .map(|(provider_id, model_id)| ModelRef { provider_id, model_id }),
    }
}

impl Db {
    pub async fn upsert_project(&self, path: &str) -> Result<Project, StorageError> {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        let now = now_ms();
        let id = new_id();
        sqlx::query(
            "INSERT INTO projects (id, path, name, created_at, opened_at) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET opened_at = excluded.opened_at",
        )
        .bind(&id)
        .bind(path)
        .bind(&name)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        let r = sqlx::query("SELECT id, path, name FROM projects WHERE path = ?")
            .bind(path)
            .fetch_one(self.pool())
            .await
            .map_err(db_err)?;
        Ok(Project {
            id: r.get("id"),
            path: r.get("path"),
            name: r.get("name"),
        })
    }

    pub async fn projects(&self) -> Result<Vec<Project>, StorageError> {
        let rows = sqlx::query("SELECT id, path, name FROM projects ORDER BY opened_at DESC")
            .fetch_all(self.pool())
            .await
            .map_err(db_err)?;
        Ok(rows
            .iter()
            .map(|r| Project {
                id: r.get("id"),
                path: r.get("path"),
                name: r.get("name"),
            })
            .collect())
    }

    pub async fn remove_project(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(db_err)?;
        Ok(())
    }

    pub async fn create_session(
        &self,
        project_id: Option<&str>,
        model: Option<&ModelRef>,
    ) -> Result<SessionSummary, StorageError> {
        let id = new_id();
        let now = now_ms();
        sqlx::query(
            "INSERT INTO sessions (id, project_id, title, created_at, updated_at, provider_id, model_id) VALUES (?, ?, '', ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(project_id)
        .bind(now)
        .bind(now)
        .bind(model.map(|m| &m.provider_id))
        .bind(model.map(|m| &m.model_id))
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(SessionSummary {
            id,
            project_id: project_id.map(String::from),
            title: String::new(),
            updated_at: now,
            model: model.cloned(),
        })
    }

    pub async fn sessions(&self) -> Result<Vec<SessionSummary>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, project_id, title, updated_at, provider_id, model_id FROM sessions ORDER BY updated_at DESC",
        )
        .fetch_all(self.pool())
        .await
        .map_err(db_err)?;
        Ok(rows.iter().map(summary).collect())
    }

    pub async fn session(&self, id: &str) -> Result<Option<SessionSummary>, StorageError> {
        let row =
            sqlx::query("SELECT id, project_id, title, updated_at, provider_id, model_id FROM sessions WHERE id = ?")
                .bind(id)
                .fetch_optional(self.pool())
                .await
                .map_err(db_err)?;
        Ok(row.as_ref().map(summary))
    }

    pub async fn rename_session(&self, id: &str, title: &str) -> Result<(), StorageError> {
        let title: String = redact(title.trim()).chars().take(200).collect();
        sqlx::query("UPDATE sessions SET title = ? WHERE id = ?")
            .bind(title)
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(db_err)?;
        Ok(())
    }

    pub async fn set_session_model(&self, id: &str, model: &ModelRef) -> Result<(), StorageError> {
        sqlx::query("UPDATE sessions SET provider_id = ?, model_id = ? WHERE id = ?")
            .bind(&model.provider_id)
            .bind(&model.model_id)
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(db_err)?;
        Ok(())
    }

    pub async fn delete_session(&self, id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Append a message. Assigns `id` and `created_at` if they are empty.
    pub async fn append_message(
        &self,
        session_id: &str,
        mut msg: StoredMessage,
    ) -> Result<StoredMessage, StorageError> {
        if msg.id.is_empty() {
            msg.id = new_id();
        }
        if msg.created_at == 0 {
            msg.created_at = now_ms();
        }
        let json = serde_json::to_string(&msg).map_err(|e| StorageError::Encode(e.to_string()))?;
        let json = redact(&json).into_owned();
        let text = redact(&msg.content).into_owned();
        let mut tx = self.pool().begin().await.map_err(db_err)?;
        let seq: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE session_id = ?")
            .bind(session_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query(
            "INSERT INTO messages (id, session_id, seq, role, content_json, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&msg.id)
        .bind(session_id)
        .bind(seq)
        .bind(
            serde_json::to_value(msg.role)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
        )
        .bind(&json)
        .bind(msg.created_at)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if !text.trim().is_empty() {
            sqlx::query("INSERT INTO messages_fts (text, message_id, session_id) VALUES (?, ?, ?)")
                .bind(&text)
                .bind(&msg.id)
                .bind(session_id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        sqlx::query("UPDATE sessions SET updated_at = ? WHERE id = ?")
            .bind(msg.created_at)
            .bind(session_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(msg)
    }

    pub async fn messages(&self, session_id: &str) -> Result<Vec<StoredMessage>, StorageError> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT content_json FROM messages WHERE session_id = ? ORDER BY seq")
                .bind(session_id)
                .fetch_all(self.pool())
                .await
                .map_err(db_err)?;
        rows.iter()
            .map(|j| serde_json::from_str(j).map_err(|e| StorageError::Encode(e.to_string())))
            .collect()
    }

    /// Search session titles and message text.
    pub async fn search(&self, query: &str) -> Result<Vec<SearchHit>, StorageError> {
        let Some(fts) = fts_query(query) else { return Ok(vec![]) };
        let like = format!(
            "%{}%",
            query
                .trim()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let rows = sqlx::query(
            "SELECT s.id AS session_id, s.title AS title,
                    snippet(messages_fts, 0, '[', ']', '...', 12) AS snippet, bm25(messages_fts) AS rank
             FROM messages_fts JOIN sessions s ON s.id = messages_fts.session_id
             WHERE messages_fts MATCH ?
             UNION ALL
             SELECT id, title, title, -1000.0 FROM sessions WHERE title LIKE ? ESCAPE '\\'
             ORDER BY rank LIMIT 100",
        )
        .bind(&fts)
        .bind(&like)
        .fetch_all(self.pool())
        .await
        .map_err(db_err)?;
        let mut seen = std::collections::HashSet::new();
        Ok(rows
            .iter()
            .filter_map(|r| {
                let id: String = r.get("session_id");
                seen.insert(id.clone()).then(|| SearchHit {
                    session_id: id,
                    title: r.get("title"),
                    snippet: r.get("snippet"),
                })
            })
            .collect())
    }

    pub async fn record_spend(
        &self,
        provider_id: &str,
        model_id: &str,
        session_id: Option<&str>,
        usage: &Usage,
        cost_usd: f64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO spend (provider_id, model_id, session_id, input_tokens, output_tokens, cost_usd, at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(provider_id)
        .bind(model_id)
        .bind(session_id)
        .bind(usage.input as i64)
        .bind(usage.output as i64)
        .bind(cost_usd)
        .bind(now_ms())
        .execute(self.pool())
        .await
        .map_err(db_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: Role, text: &str) -> StoredMessage {
        StoredMessage {
            id: String::new(),
            role,
            content: text.into(),
            reasoning: None,
            tool_calls: vec![],
            tool_call_id: None,
            model: None,
            usage: None,
            cost: None,
            error: None,
            created_at: 0,
        }
    }

    #[tokio::test]
    async fn session_messages_round_trip_in_order() {
        let db = Db::open_in_memory().await.unwrap();
        let p = db.upsert_project("D:/code/thing").await.unwrap();
        assert_eq!(p.name, "thing");
        assert_eq!(
            db.upsert_project("D:/code/thing").await.unwrap().id,
            p.id,
            "same path, same project"
        );
        let s = db.create_session(Some(&p.id), None).await.unwrap();
        db.append_message(&s.id, msg(Role::User, "hello")).await.unwrap();
        db.append_message(&s.id, msg(Role::Assistant, "hi there"))
            .await
            .unwrap();
        let m = db.messages(&s.id).await.unwrap();
        assert_eq!(
            m.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
            ["hello", "hi there"]
        );
        assert!(!m[0].id.is_empty() && m[0].created_at > 0);
    }

    #[tokio::test]
    async fn keys_in_messages_are_redacted_on_disk() {
        let db = Db::open_in_memory().await.unwrap();
        let s = db.create_session(None, None).await.unwrap();
        let key = ["gsk_", &"Zx9Yw8Vu".repeat(6)].concat();
        db.append_message(&s.id, msg(Role::User, &format!("my key is {key}")))
            .await
            .unwrap();
        let raw: Vec<String> = sqlx::query_scalar("SELECT content_json FROM messages")
            .fetch_all(db.pool())
            .await
            .unwrap();
        assert!(!raw[0].contains(&key));
        let fts: Vec<String> = sqlx::query_scalar("SELECT text FROM messages_fts")
            .fetch_all(db.pool())
            .await
            .unwrap();
        assert!(!fts[0].contains(&key));
    }

    #[tokio::test]
    async fn search_finds_titles_and_text_and_survives_syntax() {
        let db = Db::open_in_memory().await.unwrap();
        let a = db.create_session(None, None).await.unwrap();
        db.rename_session(&a.id, "Fix the router").await.unwrap();
        let b = db.create_session(None, None).await.unwrap();
        db.append_message(&b.id, msg(Role::User, "why does the borrow checker hate me"))
            .await
            .unwrap();
        let hits = db.search("borrow").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id, b.id);
        assert!(hits[0].snippet.contains("[borrow]"));
        assert_eq!(db.search("route").await.unwrap()[0].session_id, a.id, "title match");
        assert_eq!(db.search("borr").await.unwrap().len(), 1, "prefix match");
        for nasty in ["\"", "AND OR NOT", "a*b(", "  ", "100%_"] {
            assert!(db.search(nasty).await.is_ok(), "{nasty:?}");
        }
    }

    #[tokio::test]
    async fn deleting_a_session_removes_its_messages_and_index() {
        let db = Db::open_in_memory().await.unwrap();
        let s = db.create_session(None, None).await.unwrap();
        db.append_message(&s.id, msg(Role::User, "secret plans")).await.unwrap();
        db.delete_session(&s.id).await.unwrap();
        assert!(db.messages(&s.id).await.unwrap().is_empty());
        assert!(db.search("plans").await.unwrap().is_empty());
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn sessions_list_newest_first_with_model() {
        let db = Db::open_in_memory().await.unwrap();
        let m = ModelRef {
            provider_id: "groq".into(),
            model_id: "llama".into(),
        };
        let old = db.create_session(None, Some(&m)).await.unwrap();
        let new = db.create_session(None, None).await.unwrap();
        db.append_message(&old.id, msg(Role::User, "bump")).await.unwrap();
        let list = db.sessions().await.unwrap();
        assert_eq!(list[0].id, old.id, "recent activity first");
        assert_eq!(list[0].model, Some(m));
        assert_eq!(list[1].id, new.id);
    }
}
