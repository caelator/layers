//! SQLite persistence layer with DB-worker pattern.
//!
//! A single writer thread processes mutations via an mpsc channel,
//! while readers can query through the async interface.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info};

use layers_core::error::{LayersError, Result};
use layers_core::traits::{SessionStore, SessionTransaction};
use layers_core::types::*;

// ---------------------------------------------------------------------------
// Schema version
// ---------------------------------------------------------------------------

const CURRENT_SCHEMA_VERSION: i64 = 1;

// ---------------------------------------------------------------------------
// Writer commands
// ---------------------------------------------------------------------------

enum DbCommand {
    PutSession {
        session: Session,
        reply: oneshot::Sender<Result<()>>,
    },
    GetSession {
        session_id: String,
        reply: oneshot::Sender<Result<Session>>,
    },
    ListSessions {
        filter: SessionFilter,
        reply: oneshot::Sender<Result<Vec<Session>>>,
    },
    DeleteSession {
        session_id: String,
        reply: oneshot::Sender<Result<()>>,
    },
    AppendMessage {
        session_id: String,
        message: Message,
        reply: oneshot::Sender<Result<()>>,
    },
    GetMessages {
        session_id: String,
        limit: Option<usize>,
        reply: oneshot::Sender<Result<Vec<Message>>>,
    },
    UpdateModel {
        session_id: String,
        model: String,
        reply: oneshot::Sender<Result<()>>,
    },
    BeginTx {
        session_id: String,
        reply: oneshot::Sender<Result<Vec<(Session, Vec<Message>)>>>,
    },
    CommitTx {
        session_id: String,
        session: Option<Session>,
        messages: Vec<Message>,
        reply: oneshot::Sender<Result<()>>,
    },
}

// ---------------------------------------------------------------------------
// SqliteStore
// ---------------------------------------------------------------------------

/// SQLite-backed session and message store.
///
/// Uses a single-writer pattern: all mutations are serialized through an
/// internal mpsc channel to a dedicated blocking thread that owns the
/// connection.
pub struct SqliteStore {
    cmd_tx: mpsc::Sender<DbCommand>,
    _worker: tokio::task::JoinHandle<()>,
}

impl SqliteStore {
    /// Open (or create) a SQLite database at the given path.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let (cmd_tx, cmd_rx) = mpsc::channel(256);

        let worker = tokio::task::spawn_blocking(move || {
            if let Err(e) = worker_loop(path, cmd_rx) {
                error!("sqlite worker exited with error: {e}");
            }
        });

        Ok(Self {
            cmd_tx,
            _worker: worker,
        })
    }

    /// Open an in-memory database (useful for tests).
    pub async fn open_in_memory() -> Result<Self> {
        let (cmd_tx, cmd_rx) = mpsc::channel(256);

        let worker = tokio::task::spawn_blocking(move || {
            if let Err(e) = worker_loop_with_conn(
                Connection::open_in_memory().expect("failed to open in-memory db"),
                cmd_rx,
            ) {
                error!("sqlite worker exited with error: {e}");
            }
        });

        Ok(Self {
            cmd_tx,
            _worker: worker,
        })
    }

    async fn send_cmd<T>(&self, build: impl FnOnce(oneshot::Sender<Result<T>>) -> DbCommand) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(build(tx))
            .await
            .map_err(|_| LayersError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "db worker gone")))?;
        rx.await
            .map_err(|_| LayersError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "db worker dropped reply")))?
    }
}

#[async_trait::async_trait]
impl SessionStore for SqliteStore {
    async fn get(&self, session_id: &str) -> Result<Session> {
        let id = session_id.to_string();
        self.send_cmd(|reply| DbCommand::GetSession { session_id: id, reply }).await
    }

    async fn put(&self, session: &Session) -> Result<()> {
        let s = session.clone();
        self.send_cmd(|reply| DbCommand::PutSession { session: s, reply }).await
    }

    async fn list(&self, filter: &SessionFilter) -> Result<Vec<Session>> {
        let f = filter.clone();
        self.send_cmd(|reply| DbCommand::ListSessions { filter: f, reply }).await
    }

    async fn delete(&self, session_id: &str) -> Result<()> {
        let id = session_id.to_string();
        self.send_cmd(|reply| DbCommand::DeleteSession { session_id: id, reply }).await
    }

    async fn append_message(&self, session_id: &str, message: Message) -> Result<()> {
        let id = session_id.to_string();
        self.send_cmd(|reply| DbCommand::AppendMessage { session_id: id, message, reply }).await
    }

    async fn get_messages(&self, session_id: &str, limit: Option<usize>) -> Result<Vec<Message>> {
        let id = session_id.to_string();
        self.send_cmd(|reply| DbCommand::GetMessages { session_id: id, limit, reply }).await
    }

    async fn update_model(&self, session_id: &str, model: &str) -> Result<()> {
        let id = session_id.to_string();
        let m = model.to_string();
        self.send_cmd(|reply| DbCommand::UpdateModel { session_id: id, model: m, reply }).await
    }

    async fn begin_session_tx(&self, session_id: &str) -> Result<Box<dyn SessionTransaction>> {
        let id = session_id.to_string();
        let _snapshot = self
            .send_cmd(|reply| DbCommand::BeginTx { session_id: id.clone(), reply })
            .await?;
        Ok(Box::new(SqliteTx {
            session_id: id,
            cmd_tx: self.cmd_tx.clone(),
            pending_session: None,
            pending_messages: Vec::new(),
        }))
    }
}

// ---------------------------------------------------------------------------
// Transaction
// ---------------------------------------------------------------------------

struct SqliteTx {
    session_id: String,
    cmd_tx: mpsc::Sender<DbCommand>,
    pending_session: Option<Session>,
    pending_messages: Vec<Message>,
}

#[async_trait::async_trait]
impl SessionTransaction for SqliteTx {
    async fn append_message(&mut self, message: Message) -> Result<()> {
        self.pending_messages.push(message);
        Ok(())
    }

    async fn update_session(&mut self, session: Session) -> Result<()> {
        self.pending_session = Some(session);
        Ok(())
    }

    async fn commit(self: Box<Self>) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DbCommand::CommitTx {
                session_id: self.session_id.clone(),
                session: self.pending_session,
                messages: self.pending_messages,
                reply: tx,
            })
            .await
            .map_err(|_| LayersError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "db worker gone")))?;
        rx.await
            .map_err(|_| LayersError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "db worker dropped reply")))?
    }
}

// ---------------------------------------------------------------------------
// Worker loop
// ---------------------------------------------------------------------------

fn worker_loop(path: PathBuf, cmd_rx: mpsc::Receiver<DbCommand>) -> std::result::Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path)?;
    worker_loop_with_conn(conn, cmd_rx)
}

fn worker_loop_with_conn(
    conn: Connection,
    mut cmd_rx: mpsc::Receiver<DbCommand>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    run_migrations(&conn)?;

    while let Some(cmd) = cmd_rx.blocking_recv() {
        match cmd {
            DbCommand::PutSession { session, reply } => {
                let _ = reply.send(do_put_session(&conn, &session));
            }
            DbCommand::GetSession { session_id, reply } => {
                let _ = reply.send(do_get_session(&conn, &session_id));
            }
            DbCommand::ListSessions { filter, reply } => {
                let _ = reply.send(do_list_sessions(&conn, &filter));
            }
            DbCommand::DeleteSession { session_id, reply } => {
                let _ = reply.send(do_delete_session(&conn, &session_id));
            }
            DbCommand::AppendMessage { session_id, message, reply } => {
                let _ = reply.send(do_append_message(&conn, &session_id, &message));
            }
            DbCommand::GetMessages { session_id, limit, reply } => {
                let _ = reply.send(do_get_messages(&conn, &session_id, limit));
            }
            DbCommand::UpdateModel { session_id, model, reply } => {
                let _ = reply.send(do_update_model(&conn, &session_id, &model));
            }
            DbCommand::BeginTx { session_id, reply } => {
                let _ = reply.send(do_begin_tx(&conn, &session_id));
            }
            DbCommand::CommitTx { session_id, session, messages, reply } => {
                let _ = reply.send(do_commit_tx(&conn, &session_id, session.as_ref(), &messages));
            }
        }
    }

    info!("sqlite worker shutting down");
    Ok(())
}

// ---------------------------------------------------------------------------
// Migrations
// ---------------------------------------------------------------------------

fn run_migrations(conn: &Connection) -> std::result::Result<(), Box<dyn std::error::Error>> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )"
    )?;

    let version: i64 = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM _meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if version < 1 {
        debug!("running migration v1");
        conn.execute_batch(include_str!("migrations/v001.sql"))?;
        conn.execute(
            "INSERT OR REPLACE INTO _meta (key, value) VALUES ('schema_version', ?1)",
            params![CURRENT_SCHEMA_VERSION],
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// DB operations
// ---------------------------------------------------------------------------

fn map_rusqlite(e: rusqlite::Error) -> LayersError {
    LayersError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
}

fn do_put_session(conn: &Connection, session: &Session) -> Result<()> {
    let metadata_json = serde_json::to_string(&session.metadata)?;
    let dm_scope_json = session.dm_scope.as_ref().map(|d| serde_json::to_string(d)).transpose()?;
    let thread_binding_json = session.thread_binding.as_ref().map(|t| serde_json::to_string(t)).transpose()?;

    conn.execute(
        "INSERT OR REPLACE INTO sessions
         (key, agent_id, model, created_at, last_active, total_tokens,
          compacted_count, dm_scope, thread_binding, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            session.id,
            session.agent_id,
            session.model,
            session.created_at.to_rfc3339(),
            session.updated_at.to_rfc3339(),
            session.token_count as i64,
            session.message_count as i64,
            dm_scope_json,
            thread_binding_json,
            metadata_json,
        ],
    )
    .map_err(map_rusqlite)?;

    Ok(())
}

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<Session> {
    let id: String = row.get("key")?;
    let agent_id: String = row.get("agent_id")?;
    let model: Option<String> = row.get("model")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("last_active")?;
    let token_count: i64 = row.get("total_tokens")?;
    let message_count: i64 = row.get("compacted_count")?;
    let dm_scope_json: Option<String> = row.get("dm_scope")?;
    let thread_binding_json: Option<String> = row.get("thread_binding")?;
    let metadata_json: String = row.get("metadata")?;

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let dm_scope = dm_scope_json.and_then(|j| serde_json::from_str(&j).ok());
    let thread_binding = thread_binding_json.and_then(|j| serde_json::from_str(&j).ok());
    let metadata = serde_json::from_str(&metadata_json).unwrap_or_default();

    Ok(Session {
        id,
        agent_id,
        dm_scope,
        thread_binding,
        created_at,
        updated_at,
        model,
        metadata,
        message_count: message_count as usize,
        token_count: token_count as usize,
    })
}

fn do_get_session(conn: &Connection, session_id: &str) -> Result<Session> {
    conn.query_row(
        "SELECT * FROM sessions WHERE key = ?1",
        params![session_id],
        row_to_session,
    )
    .optional()
    .map_err(map_rusqlite)?
    .ok_or_else(|| LayersError::SessionNotFound(session_id.to_string()))
}

fn do_list_sessions(conn: &Connection, filter: &SessionFilter) -> Result<Vec<Session>> {
    let mut sql = "SELECT * FROM sessions WHERE 1=1".to_string();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref agent_id) = filter.agent_id {
        sql.push_str(&format!(" AND agent_id = ?{}", param_values.len() + 1));
        param_values.push(Box::new(agent_id.clone()));
    }
    if let Some(ref since) = filter.since {
        sql.push_str(&format!(" AND last_active >= ?{}", param_values.len() + 1));
        param_values.push(Box::new(since.to_rfc3339()));
    }

    sql.push_str(" ORDER BY last_active DESC");

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(map_rusqlite)?;
    let rows = stmt
        .query_map(params_refs.as_slice(), row_to_session)
        .map_err(map_rusqlite)?;

    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row.map_err(map_rusqlite)?);
    }
    Ok(sessions)
}

fn do_delete_session(conn: &Connection, session_id: &str) -> Result<()> {
    conn.execute("DELETE FROM messages WHERE session_key = ?1", params![session_id])
        .map_err(map_rusqlite)?;
    conn.execute("DELETE FROM sessions WHERE key = ?1", params![session_id])
        .map_err(map_rusqlite)?;
    Ok(())
}

fn do_append_message(conn: &Connection, session_id: &str, message: &Message) -> Result<()> {
    let seq: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE session_key = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(map_rusqlite)?;

    let role_str = serde_json::to_string(&message.role)?;
    let content_json = serde_json::to_string(&message.content)?;
    let tool_calls_json = message.tool_calls.as_ref().map(|tc| serde_json::to_string(tc)).transpose()?;
    let reasoning_json = message.reasoning.as_ref().map(|r| serde_json::to_string(r)).transpose()?;
    let timestamp = message
        .timestamp
        .unwrap_or_else(Utc::now)
        .to_rfc3339();

    conn.execute(
        "INSERT INTO messages
         (session_key, seq, role, content, tool_calls, tool_call_id, name, reasoning, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            session_id,
            seq,
            role_str,
            content_json,
            tool_calls_json,
            message.tool_call_id,
            message.name,
            reasoning_json,
            timestamp,
        ],
    )
    .map_err(map_rusqlite)?;

    // Update session message count
    conn.execute(
        "UPDATE sessions SET compacted_count = compacted_count + 1, last_active = ?2 WHERE key = ?1",
        params![session_id, Utc::now().to_rfc3339()],
    )
    .map_err(map_rusqlite)?;

    Ok(())
}

fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<Message> {
    let role_str: String = row.get("role")?;
    let content_json: String = row.get("content")?;
    let tool_calls_json: Option<String> = row.get("tool_calls")?;
    let tool_call_id: Option<String> = row.get("tool_call_id")?;
    let name: Option<String> = row.get("name")?;
    let reasoning_json: Option<String> = row.get("reasoning")?;
    let timestamp_str: Option<String> = row.get("timestamp")?;

    let role: MessageRole = serde_json::from_str(&role_str).unwrap_or(MessageRole::User);
    let content: MessageContent = serde_json::from_str(&content_json)
        .unwrap_or(MessageContent::Text(String::new()));
    let tool_calls = tool_calls_json.and_then(|j| serde_json::from_str(&j).ok());
    let reasoning = reasoning_json.and_then(|j| serde_json::from_str(&j).ok());
    let timestamp = timestamp_str.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
    });

    Ok(Message {
        role,
        content,
        name,
        tool_calls,
        tool_call_id,
        reasoning,
        timestamp,
    })
}

fn do_get_messages(conn: &Connection, session_id: &str, limit: Option<usize>) -> Result<Vec<Message>> {
    let sql = if let Some(n) = limit {
        format!(
            "SELECT * FROM (SELECT * FROM messages WHERE session_key = ?1 ORDER BY seq DESC LIMIT {n}) ORDER BY seq ASC"
        )
    } else {
        "SELECT * FROM messages WHERE session_key = ?1 ORDER BY seq ASC".to_string()
    };

    let mut stmt = conn.prepare(&sql).map_err(map_rusqlite)?;
    let rows = stmt.query_map(params![session_id], row_to_message).map_err(map_rusqlite)?;

    let mut messages = Vec::new();
    for row in rows {
        messages.push(row.map_err(map_rusqlite)?);
    }
    Ok(messages)
}

fn do_update_model(conn: &Connection, session_id: &str, model: &str) -> Result<()> {
    let changed = conn
        .execute(
            "UPDATE sessions SET model = ?2, last_active = ?3 WHERE key = ?1",
            params![session_id, model, Utc::now().to_rfc3339()],
        )
        .map_err(map_rusqlite)?;

    if changed == 0 {
        return Err(LayersError::SessionNotFound(session_id.to_string()));
    }
    Ok(())
}

fn do_begin_tx(conn: &Connection, session_id: &str) -> Result<Vec<(Session, Vec<Message>)>> {
    let session = do_get_session(conn, session_id)?;
    let messages = do_get_messages(conn, session_id, None)?;
    Ok(vec![(session, messages)])
}

fn do_commit_tx(
    conn: &Connection,
    session_id: &str,
    session: Option<&Session>,
    messages: &[Message],
) -> Result<()> {
    let tx = conn.unchecked_transaction().map_err(map_rusqlite)?;

    if let Some(s) = session {
        do_put_session(&tx, s)?;
    }
    for msg in messages {
        do_append_message(&tx, session_id, msg)?;
    }

    tx.commit().map_err(map_rusqlite)?;
    Ok(())
}

// Allow do_put_session / do_append_message to work with Transaction too,
// since Transaction derefs to Connection.
fn _assert_send_sync() {
    fn _assert<T: Send + Sync>() {}
    _assert::<SqliteStore>();
}
