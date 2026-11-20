-- Plain chat (milestone 2): model per session and full-text search.
ALTER TABLE sessions ADD COLUMN provider_id TEXT;
ALTER TABLE sessions ADD COLUMN model_id TEXT;

CREATE INDEX sessions_updated ON sessions (updated_at DESC);
CREATE INDEX messages_session_seq ON messages (session_id, seq);

-- Search index over message text. Rows are written by the app next to each
-- message and removed with it.
CREATE VIRTUAL TABLE messages_fts USING fts5(
    text,
    message_id UNINDEXED,
    session_id UNINDEXED,
    tokenize = 'unicode61'
);

CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
    DELETE FROM messages_fts WHERE message_id = old.id;
END;
