//! SQLite persistence layer with DB-worker pattern.
//!
//! A single writer thread processes mutations via an mpsc channel,
//! while readers can query through the async interface.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info};

use layers_core::error::{LayersError, Result};
use layers_core::traits::{AuthProfileStore, CronStore, ArchiveStore, ProcessRunStore, EmbeddingIndexStore, SessionStore};
use layers_core::types::*;

// ---------------------------------------------------------------------------
// Schema version
// ---------------------------------------------------------------------------

const CURRENT_SCHEMA_VERSION: i64 = 1;

// ---------------------------------------------------------------------------
// Writer commands
// ---------------------------------------------------------------------------

#[allow(clippy::type_complexity)]
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
    PutAuthProfile {
        profile: AuthProfile,
        reply: oneshot::Sender<Result<()>>,
    },
    GetAuthProfile {
        name: String,
        reply: oneshot::Sender<Result<AuthProfile>>,
    },
    ListAuthProfiles {
        provider: Option<String>,
        reply: oneshot::Sender<Result<Vec<AuthProfile>>>,
    },
    DeleteAuthProfile {
        name: String,
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
    // Cron commands
    PutCronJob {
        job: CronJob,
        reply: oneshot::Sender<Result<()>>,
    },
    GetCronJob {
        id: String,
        reply: oneshot::Sender<Result<CronJob>>,
    },
    ListCronJobs {
        reply: oneshot::Sender<Result<Vec<CronJob>>>,
    },
    DeleteCronJob {
        id: String,
        reply: oneshot::Sender<Result<()>>,
    },
    PutCronRun {
        run: CronRun,
        reply: oneshot::Sender<Result<()>>,
    },
    GetCronRun {
        id: String,
        reply: oneshot::Sender<Result<CronRun>>,
    },
    ListCronRunsForJob {
        job_id: String,
        limit: Option<usize>,
        reply: oneshot::Sender<Result<Vec<CronRun>>>,
    },
    UpdateCronRunStatus {
        id: String,
        status: CronRunStatus,
        finished_at: String,
        error_message: Option<String>,
        reply: oneshot::Sender<Result<()>>,
    },
    // Archive commands
    PutArchive {
        archive: Archive,
        reply: oneshot::Sender<Result<()>>,
    },
    GetArchive {
        id: String,
        reply: oneshot::Sender<Result<Archive>>,
    },
    ListArchivesForSession {
        session_id: String,
        reply: oneshot::Sender<Result<Vec<Archive>>>,
    },
    DeleteArchive {
        id: String,
        reply: oneshot::Sender<Result<()>>,
    },
    // ProcessRun commands
    PutProcessRun {
        run: ProcessRun,
        reply: oneshot::Sender<Result<()>>,
    },
    GetProcessRun {
        id: String,
        reply: oneshot::Sender<Result<ProcessRun>>,
    },
    ListProcessRunsByParent {
        parent_session_id: String,
        reply: oneshot::Sender<Result<Vec<ProcessRun>>>,
    },
    UpdateProcessRunStatus {
        id: String,
        status: ProcessRunStatus,
        finished_at: String,
        result_summary: Option<String>,
        reply: oneshot::Sender<Result<()>>,
    },
    // EmbeddingIndexState commands
    PutEmbeddingIndexState {
        state: EmbeddingIndexState,
        reply: oneshot::Sender<Result<()>>,
    },
    GetEmbeddingIndexState {
        corpus: String,
        reply: oneshot::Sender<Result<EmbeddingIndexState>>,
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
            DbCommand::PutAuthProfile { profile, reply } => {
                let _ = reply.send(do_put_auth_profile(&conn, &profile));
            }
            DbCommand::GetAuthProfile { name, reply } => {
                let _ = reply.send(do_get_auth_profile(&conn, &name));
            }
            DbCommand::ListAuthProfiles { provider, reply } => {
                let _ = reply.send(do_list_auth_profiles(&conn, provider.as_deref()));
            }
            DbCommand::DeleteAuthProfile { name, reply } => {
                let _ = reply.send(do_delete_auth_profile(&conn, &name));
            }
            // Cron
            DbCommand::PutCronJob { job, reply } => {
                let _ = reply.send(do_put_cron_job(&conn, &job));
            }
            DbCommand::GetCronJob { id, reply } => {
                let _ = reply.send(do_get_cron_job(&conn, &id));
            }
            DbCommand::ListCronJobs { reply } => {
                let _ = reply.send(do_list_cron_jobs(&conn));
            }
            DbCommand::DeleteCronJob { id, reply } => {
                let _ = reply.send(do_delete_cron_job(&conn, &id));
            }
            DbCommand::PutCronRun { run, reply } => {
                let _ = reply.send(do_put_cron_run(&conn, &run));
            }
            DbCommand::GetCronRun { id, reply } => {
                let _ = reply.send(do_get_cron_run(&conn, &id));
            }
            DbCommand::ListCronRunsForJob { job_id, limit, reply } => {
                let _ = reply.send(do_list_cron_runs_for_job(&conn, &job_id, limit));
            }
            DbCommand::UpdateCronRunStatus { id, status, finished_at, error_message, reply } => {
                let _ = reply.send(do_update_cron_run_status(&conn, &id, &status, &finished_at, error_message.as_deref()));
            }
            // Archive
            DbCommand::PutArchive { archive, reply } => {
                let _ = reply.send(do_put_archive(&conn, &archive));
            }
            DbCommand::GetArchive { id, reply } => {
                let _ = reply.send(do_get_archive(&conn, &id));
            }
            DbCommand::ListArchivesForSession { session_id, reply } => {
                let _ = reply.send(do_list_archives_for_session(&conn, &session_id));
            }
            DbCommand::DeleteArchive { id, reply } => {
                let _ = reply.send(do_delete_archive(&conn, &id));
            }
            // ProcessRun
            DbCommand::PutProcessRun { run, reply } => {
                let _ = reply.send(do_put_process_run(&conn, &run));
            }
            DbCommand::GetProcessRun { id, reply } => {
                let _ = reply.send(do_get_process_run(&conn, &id));
            }
            DbCommand::ListProcessRunsByParent { parent_session_id, reply } => {
                let _ = reply.send(do_list_process_runs_by_parent(&conn, &parent_session_id));
            }
            DbCommand::UpdateProcessRunStatus { id, status, finished_at, result_summary, reply } => {
                let _ = reply.send(do_update_process_run_status(&conn, &id, &status, &finished_at, result_summary.as_deref()));
            }
            // EmbeddingIndexState
            DbCommand::PutEmbeddingIndexState { state, reply } => {
                let _ = reply.send(do_put_embedding_index_state(&conn, &state));
            }
            DbCommand::GetEmbeddingIndexState { corpus, reply } => {
                let _ = reply.send(do_get_embedding_index_state(&conn, &corpus));
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
    LayersError::Io(std::io::Error::other(e.to_string()))
}

fn do_put_session(conn: &Connection, session: &Session) -> Result<()> {
    let metadata_json = serde_json::to_string(&session.metadata)?;
    let dm_scope_json = session.dm_scope.as_ref().map(serde_json::to_string).transpose()?;
    let thread_binding_json = session.thread_binding.as_ref().map(serde_json::to_string).transpose()?;

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
    let tool_calls_json = message.tool_calls.as_ref().map(serde_json::to_string).transpose()?;
    let reasoning_json = message.reasoning.as_ref().map(serde_json::to_string).transpose()?;
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

// ---------------------------------------------------------------------------
// AuthProfileStore impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl AuthProfileStore for SqliteStore {
    async fn put_profile(&self, profile: AuthProfile) -> Result<()> {
        let p = profile;
        self.send_cmd(|reply| DbCommand::PutAuthProfile { profile: p, reply }).await
    }

    async fn get_profile(&self, name: &str) -> Result<AuthProfile> {
        let n = name.to_string();
        self.send_cmd(|reply| DbCommand::GetAuthProfile { name: n, reply }).await
    }

    async fn list_profiles(&self, provider: Option<&str>) -> Result<Vec<AuthProfile>> {
        let p = provider.map(String::from);
        self.send_cmd(|reply| DbCommand::ListAuthProfiles { provider: p, reply }).await
    }

    async fn delete_profile(&self, name: &str) -> Result<()> {
        let n = name.to_string();
        self.send_cmd(|reply| DbCommand::DeleteAuthProfile { name: n, reply }).await
    }
}

// ---------------------------------------------------------------------------
// CronStore impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl CronStore for SqliteStore {
    async fn put_job(&self, job: CronJob) -> Result<()> {
        self.send_cmd(|reply| DbCommand::PutCronJob { job, reply }).await
    }
    async fn get_job(&self, id: &str) -> Result<CronJob> {
        let id = id.to_string();
        self.send_cmd(|reply| DbCommand::GetCronJob { id, reply }).await
    }
    async fn list_jobs(&self) -> Result<Vec<CronJob>> {
        self.send_cmd(|reply| DbCommand::ListCronJobs { reply }).await
    }
    async fn delete_job(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        self.send_cmd(|reply| DbCommand::DeleteCronJob { id, reply }).await
    }
    async fn put_run(&self, run: CronRun) -> Result<()> {
        self.send_cmd(|reply| DbCommand::PutCronRun { run, reply }).await
    }
    async fn get_run(&self, id: &str) -> Result<CronRun> {
        let id = id.to_string();
        self.send_cmd(|reply| DbCommand::GetCronRun { id, reply }).await
    }
    async fn list_runs_for_job(&self, job_id: &str, limit: Option<usize>) -> Result<Vec<CronRun>> {
        let job_id = job_id.to_string();
        self.send_cmd(|reply| DbCommand::ListCronRunsForJob { job_id, limit, reply }).await
    }
    async fn update_run_status(&self, id: &str, status: CronRunStatus, finished_at: DateTime<Utc>, error_message: Option<&str>) -> Result<()> {
        let id = id.to_string();
        let finished_at = finished_at.to_rfc3339();
        let error_message = error_message.map(String::from);
        self.send_cmd(|reply| DbCommand::UpdateCronRunStatus { id, status, finished_at, error_message, reply }).await
    }
}

// ---------------------------------------------------------------------------
// ArchiveStore impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl ArchiveStore for SqliteStore {
    async fn put(&self, archive: Archive) -> Result<()> {
        self.send_cmd(|reply| DbCommand::PutArchive { archive, reply }).await
    }
    async fn get(&self, id: &str) -> Result<Archive> {
        let id = id.to_string();
        self.send_cmd(|reply| DbCommand::GetArchive { id, reply }).await
    }
    async fn list_for_session(&self, session_id: &str) -> Result<Vec<Archive>> {
        let session_id = session_id.to_string();
        self.send_cmd(|reply| DbCommand::ListArchivesForSession { session_id, reply }).await
    }
    async fn delete(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        self.send_cmd(|reply| DbCommand::DeleteArchive { id, reply }).await
    }
}

// ---------------------------------------------------------------------------
// ProcessRunStore impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl ProcessRunStore for SqliteStore {
    async fn put(&self, run: ProcessRun) -> Result<()> {
        self.send_cmd(|reply| DbCommand::PutProcessRun { run, reply }).await
    }
    async fn get(&self, id: &str) -> Result<ProcessRun> {
        let id = id.to_string();
        self.send_cmd(|reply| DbCommand::GetProcessRun { id, reply }).await
    }
    async fn list_by_parent(&self, parent_session_id: &str) -> Result<Vec<ProcessRun>> {
        let parent_session_id = parent_session_id.to_string();
        self.send_cmd(|reply| DbCommand::ListProcessRunsByParent { parent_session_id, reply }).await
    }
    async fn update_status(&self, id: &str, status: ProcessRunStatus, finished_at: DateTime<Utc>, result_summary: Option<&str>) -> Result<()> {
        let id = id.to_string();
        let finished_at = finished_at.to_rfc3339();
        let result_summary = result_summary.map(String::from);
        self.send_cmd(|reply| DbCommand::UpdateProcessRunStatus { id, status, finished_at, result_summary, reply }).await
    }
}

// ---------------------------------------------------------------------------
// EmbeddingIndexStore impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl EmbeddingIndexStore for SqliteStore {
    async fn put(&self, state: EmbeddingIndexState) -> Result<()> {
        self.send_cmd(|reply| DbCommand::PutEmbeddingIndexState { state, reply }).await
    }
    async fn get(&self, corpus: &str) -> Result<EmbeddingIndexState> {
        let corpus = corpus.to_string();
        self.send_cmd(|reply| DbCommand::GetEmbeddingIndexState { corpus, reply }).await
    }
}

// ---------------------------------------------------------------------------
// Auth profile DB operations
// ---------------------------------------------------------------------------

fn do_put_auth_profile(conn: &Connection, profile: &AuthProfile) -> Result<()> {
    let models_json = serde_json::to_string(&profile.models)?;
    conn.execute(
        "INSERT OR REPLACE INTO auth_profiles (name, provider, api_key_encrypted, api_base, models, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            profile.name,
            profile.provider,
            profile.api_key,
            profile.api_base,
            models_json,
            profile.created_at.to_rfc3339(),
        ],
    )
    .map_err(map_rusqlite)?;
    Ok(())
}

fn row_to_auth_profile(row: &rusqlite::Row) -> rusqlite::Result<AuthProfile> {
    let name: String = row.get("name")?;
    let provider: String = row.get("provider")?;
    let api_key: Option<String> = row.get("api_key_encrypted")?;
    let api_base: Option<String> = row.get("api_base")?;
    let models_json: Option<String> = row.get("models")?;
    let created_at_str: String = row.get("created_at")?;

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let models: Vec<String> = models_json
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();

    Ok(AuthProfile {
        name,
        provider,
        api_key,
        api_base,
        models,
        created_at,
    })
}

fn do_get_auth_profile(conn: &Connection, name: &str) -> Result<AuthProfile> {
    conn.query_row(
        "SELECT * FROM auth_profiles WHERE name = ?1",
        params![name],
        row_to_auth_profile,
    )
    .optional()
    .map_err(map_rusqlite)?
    .ok_or_else(|| LayersError::Config(format!("auth profile not found: {name}")))
}

fn do_list_auth_profiles(conn: &Connection, provider: Option<&str>) -> Result<Vec<AuthProfile>> {
    let sql = if provider.is_some() {
        "SELECT * FROM auth_profiles WHERE provider = ?1 ORDER BY name"
    } else {
        "SELECT * FROM auth_profiles ORDER BY name"
    };

    let mut stmt = conn.prepare(sql).map_err(map_rusqlite)?;
    let rows = if let Some(p) = provider {
        stmt.query_map(params![p], row_to_auth_profile).map_err(map_rusqlite)?
    } else {
        stmt.query_map(params![], row_to_auth_profile).map_err(map_rusqlite)?
    };

    let mut profiles = Vec::new();
    for row in rows {
        profiles.push(row.map_err(map_rusqlite)?);
    }
    Ok(profiles)
}

fn do_delete_auth_profile(conn: &Connection, name: &str) -> Result<()> {
    let changed = conn
        .execute("DELETE FROM auth_profiles WHERE name = ?1", params![name])
        .map_err(map_rusqlite)?;
    if changed == 0 {
        return Err(LayersError::Config(format!("auth profile not found: {name}")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CronStore impl
// ---------------------------------------------------------------------------

fn do_put_cron_job(conn: &Connection, job: &CronJob) -> Result<()> {
    let schedule_json = serde_json::to_string(&job.schedule)?;
    let payload_json = serde_json::to_string(&job.payload)?;
    let session_target_json = job.session_target.as_ref().map(serde_json::to_string).transpose()?;
    let delivery_json = job.delivery.as_ref().map(serde_json::to_string).transpose()?;
    let name = job.schedule.cron.clone(); // use cron expression as a readable name fallback

    // Extract fields from delivery for the table columns
    let (misfire_policy, agent_id, failure_alert_json, delete_after_run) = 
        if let Some(ref d) = job.delivery {
            (d.misfire_policy.as_ref().map(|p| serde_json::to_string(p).unwrap_or_default()),
             None as Option<String>,
             d.failure_alert.as_ref().map(|f| serde_json::to_string(f).unwrap_or_default()),
             false)
        } else {
            (None, None, None, false)
        };

    conn.execute(
        "INSERT OR REPLACE INTO cron_jobs (id, name, schedule, payload, session_target, delivery, enabled, delete_after_run, misfire_policy, agent_id, failure_alert)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            job.id,
            name,
            schedule_json,
            payload_json,
            session_target_json,
            delivery_json,
            job.enabled as i64,
            delete_after_run as i64,
            misfire_policy,
            agent_id,
            failure_alert_json,
        ],
    ).map_err(map_rusqlite)?;
    Ok(())
}

fn row_to_cron_job(row: &rusqlite::Row) -> rusqlite::Result<CronJob> {
    let id: String = row.get("id")?;
    let schedule_json: String = row.get("schedule")?;
    let payload_json: String = row.get("payload")?;
    let session_target_json: Option<String> = row.get("session_target")?;
    let delivery_json: Option<String> = row.get("delivery")?;
    let enabled: i64 = row.get("enabled")?;

    let schedule: CronSchedule = serde_json::from_str(&schedule_json).unwrap();
    let payload: CronPayload = serde_json::from_str(&payload_json).unwrap();
    let session_target = session_target_json.and_then(|j| serde_json::from_str(&j).ok());
    let delivery = delivery_json.and_then(|j| serde_json::from_str(&j).ok());

    Ok(CronJob {
        id,
        schedule,
        payload,
        session_target,
        delivery,
        enabled: enabled != 0,
    })
}

fn do_get_cron_job(conn: &Connection, id: &str) -> Result<CronJob> {
    conn.query_row(
        "SELECT * FROM cron_jobs WHERE id = ?1",
        params![id],
        row_to_cron_job,
    ).optional().map_err(map_rusqlite)?
    .ok_or_else(|| LayersError::Config(format!("cron job not found: {id}")))
}

fn do_list_cron_jobs(conn: &Connection) -> Result<Vec<CronJob>> {
    let mut stmt = conn.prepare("SELECT * FROM cron_jobs ORDER BY id").map_err(map_rusqlite)?;
    let rows = stmt.query_map([], row_to_cron_job).map_err(map_rusqlite)?;
    let mut jobs = Vec::new();
    for row in rows { jobs.push(row.map_err(map_rusqlite)?); }
    Ok(jobs)
}

fn do_delete_cron_job(conn: &Connection, id: &str) -> Result<()> {
    let changed = conn.execute("DELETE FROM cron_jobs WHERE id = ?1", params![id]).map_err(map_rusqlite)?;
    if changed == 0 { return Err(LayersError::Config(format!("cron job not found: {id}"))); }
    Ok(())
}

fn do_put_cron_run(conn: &Connection, run: &CronRun) -> Result<()> {
    let status_str = serde_json::to_string(&run.status)?;
    conn.execute(
        "INSERT OR REPLACE INTO cron_runs (id, job_id, started_at, finished_at, status, error_message)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            run.id,
            run.job_id,
            run.started_at.to_rfc3339(),
            run.finished_at.map(|t| t.to_rfc3339()),
            status_str,
            run.error_message,
        ],
    ).map_err(map_rusqlite)?;
    Ok(())
}

fn row_to_cron_run(row: &rusqlite::Row) -> rusqlite::Result<CronRun> {
    let id: String = row.get("id")?;
    let job_id: String = row.get("job_id")?;
    let started_at_str: String = row.get("started_at")?;
    let finished_at_str: Option<String> = row.get("finished_at")?;
    let status_str: String = row.get("status")?;
    let error_message: Option<String> = row.get("error_message")?;

    let started_at = DateTime::parse_from_rfc3339(&started_at_str).map(|dt| dt.with_timezone(&Utc)).unwrap();
    let finished_at = finished_at_str.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)));
    let status: CronRunStatus = serde_json::from_str(&status_str).unwrap_or(CronRunStatus::Failed);

    Ok(CronRun { id, job_id, started_at, finished_at, status, error_message })
}

fn do_get_cron_run(conn: &Connection, id: &str) -> Result<CronRun> {
    conn.query_row("SELECT * FROM cron_runs WHERE id = ?1", params![id], row_to_cron_run)
        .optional().map_err(map_rusqlite)?
        .ok_or_else(|| LayersError::Config(format!("cron run not found: {id}")))
}

fn do_list_cron_runs_for_job(conn: &Connection, job_id: &str, limit: Option<usize>) -> Result<Vec<CronRun>> {
    let sql = if limit.is_some() {
        "SELECT * FROM cron_runs WHERE job_id = ?1 ORDER BY started_at DESC LIMIT ?2"
    } else {
        "SELECT * FROM cron_runs WHERE job_id = ?1 ORDER BY started_at DESC"
    };
    let mut stmt = conn.prepare(sql).map_err(map_rusqlite)?;
    let rows = if let Some(n) = limit {
        stmt.query_map(params![job_id, n as i64], row_to_cron_run).map_err(map_rusqlite)?
    } else {
        stmt.query_map(params![job_id], row_to_cron_run).map_err(map_rusqlite)?
    };
    let mut runs = Vec::new();
    for row in rows { runs.push(row.map_err(map_rusqlite)?); }
    // Reverse to get chronological order
    runs.reverse();
    Ok(runs)
}

fn do_update_cron_run_status(conn: &Connection, id: &str, status: &CronRunStatus, finished_at: &str, error_message: Option<&str>) -> Result<()> {
    let status_str = serde_json::to_string(status)?;
    let changed = conn.execute(
        "UPDATE cron_runs SET status = ?2, finished_at = ?3, error_message = ?4 WHERE id = ?1",
        params![id, status_str, finished_at, error_message],
    ).map_err(map_rusqlite)?;
    if changed == 0 { return Err(LayersError::Config(format!("cron run not found: {id}"))); }
    Ok(())
}

// ---------------------------------------------------------------------------
// ArchiveStore impl
// ---------------------------------------------------------------------------

fn do_put_archive(conn: &Connection, archive: &Archive) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO archives (id, session_key, archived_at, message_count, summary)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            archive.id,
            archive.session_id,
            archive.archived_at.to_rfc3339(),
            archive.message_count as i64,
            archive.summary,
        ],
    ).map_err(map_rusqlite)?;
    Ok(())
}

fn row_to_archive(row: &rusqlite::Row) -> rusqlite::Result<Archive> {
    let id: String = row.get("id")?;
    let session_id: String = row.get("session_key")?;
    let archived_at_str: String = row.get("archived_at")?;
    let message_count: i64 = row.get("message_count")?;
    let summary: Option<String> = row.get("summary")?;

    let archived_at = DateTime::parse_from_rfc3339(&archived_at_str).map(|dt| dt.with_timezone(&Utc)).unwrap();

    Ok(Archive {
        id,
        session_id,
        archived_at,
        message_count: message_count as usize,
        summary,
    })
}

fn do_get_archive(conn: &Connection, id: &str) -> Result<Archive> {
    conn.query_row("SELECT * FROM archives WHERE id = ?1", params![id], row_to_archive)
        .optional().map_err(map_rusqlite)?
        .ok_or_else(|| LayersError::Config(format!("archive not found: {id}")))
}

fn do_list_archives_for_session(conn: &Connection, session_id: &str) -> Result<Vec<Archive>> {
    let mut stmt = conn.prepare("SELECT * FROM archives WHERE session_key = ?1 ORDER BY archived_at DESC").map_err(map_rusqlite)?;
    let rows = stmt.query_map(params![session_id], row_to_archive).map_err(map_rusqlite)?;
    let mut archives = Vec::new();
    for row in rows { archives.push(row.map_err(map_rusqlite)?); }
    Ok(archives)
}

fn do_delete_archive(conn: &Connection, id: &str) -> Result<()> {
    let changed = conn.execute("DELETE FROM archives WHERE id = ?1", params![id]).map_err(map_rusqlite)?;
    if changed == 0 { return Err(LayersError::Config(format!("archive not found: {id}"))); }
    Ok(())
}

// ---------------------------------------------------------------------------
// ProcessRunStore impl
// ---------------------------------------------------------------------------

fn do_put_process_run(conn: &Connection, run: &ProcessRun) -> Result<()> {
    let status_str = serde_json::to_string(&run.status)?;
    conn.execute(
        "INSERT OR REPLACE INTO process_runs (id, parent_session_key, agent_id, status, started_at, finished_at, result_summary)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            run.id,
            run.parent_session_id,
            run.agent_id,
            status_str,
            run.started_at.to_rfc3339(),
            run.finished_at.map(|t| t.to_rfc3339()),
            run.result_summary,
        ],
    ).map_err(map_rusqlite)?;
    Ok(())
}

fn row_to_process_run(row: &rusqlite::Row) -> rusqlite::Result<ProcessRun> {
    let id: String = row.get("id")?;
    let parent_session_id: Option<String> = row.get("parent_session_key")?;
    let agent_id: Option<String> = row.get("agent_id")?;
    let status_str: String = row.get("status")?;
    let started_at_str: String = row.get("started_at")?;
    let finished_at_str: Option<String> = row.get("finished_at")?;
    let result_summary: Option<String> = row.get("result_summary")?;

    let started_at = DateTime::parse_from_rfc3339(&started_at_str).map(|dt| dt.with_timezone(&Utc)).unwrap();
    let finished_at = finished_at_str.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)));
    let status: ProcessRunStatus = serde_json::from_str(&status_str).unwrap_or(ProcessRunStatus::Failed);

    Ok(ProcessRun { id, parent_session_id, agent_id, status, started_at, finished_at, result_summary })
}

fn do_get_process_run(conn: &Connection, id: &str) -> Result<ProcessRun> {
    conn.query_row("SELECT * FROM process_runs WHERE id = ?1", params![id], row_to_process_run)
        .optional().map_err(map_rusqlite)?
        .ok_or_else(|| LayersError::Config(format!("process run not found: {id}")))
}

fn do_list_process_runs_by_parent(conn: &Connection, parent_session_id: &str) -> Result<Vec<ProcessRun>> {
    let mut stmt = conn.prepare("SELECT * FROM process_runs WHERE parent_session_key = ?1 ORDER BY started_at DESC").map_err(map_rusqlite)?;
    let rows = stmt.query_map(params![parent_session_id], row_to_process_run).map_err(map_rusqlite)?;
    let mut runs = Vec::new();
    for row in rows { runs.push(row.map_err(map_rusqlite)?); }
    Ok(runs)
}

fn do_update_process_run_status(conn: &Connection, id: &str, status: &ProcessRunStatus, finished_at: &str, result_summary: Option<&str>) -> Result<()> {
    let status_str = serde_json::to_string(status)?;
    let changed = conn.execute(
        "UPDATE process_runs SET status = ?2, finished_at = ?3, result_summary = ?4 WHERE id = ?1",
        params![id, status_str, finished_at, result_summary],
    ).map_err(map_rusqlite)?;
    if changed == 0 { return Err(LayersError::Config(format!("process run not found: {id}"))); }
    Ok(())
}

// ---------------------------------------------------------------------------
// EmbeddingIndexStore impl
// ---------------------------------------------------------------------------

fn do_put_embedding_index_state(conn: &Connection, state: &EmbeddingIndexState) -> Result<()> {
    let metadata_json = serde_json::to_string(&state.metadata)?;
    conn.execute(
        "INSERT OR REPLACE INTO embedding_index_state (corpus, embedding_model, last_indexed_at, index_version, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            state.corpus,
            state.embedding_model,
            state.last_indexed_at.to_rfc3339(),
            state.index_version,
            metadata_json,
        ],
    ).map_err(map_rusqlite)?;
    Ok(())
}

fn row_to_embedding_index_state(row: &rusqlite::Row) -> rusqlite::Result<EmbeddingIndexState> {
    let corpus: String = row.get("corpus")?;
    let embedding_model: String = row.get("embedding_model")?;
    let last_indexed_at_str: String = row.get("last_indexed_at")?;
    let index_version: i64 = row.get("index_version")?;
    let metadata_json: Option<String> = row.get("metadata")?;

    let last_indexed_at = DateTime::parse_from_rfc3339(&last_indexed_at_str).map(|dt| dt.with_timezone(&Utc)).unwrap();
    let metadata: HashMap<String, serde_json::Value> = metadata_json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();

    Ok(EmbeddingIndexState { corpus, embedding_model, last_indexed_at, index_version, metadata })
}

fn do_get_embedding_index_state(conn: &Connection, corpus: &str) -> Result<EmbeddingIndexState> {
    conn.query_row("SELECT * FROM embedding_index_state WHERE corpus = ?1", params![corpus], row_to_embedding_index_state)
        .optional().map_err(map_rusqlite)?
        .ok_or_else(|| LayersError::Config(format!("embedding index state not found: {corpus}")))
}

// Allow do_put_session / do_append_message to work with Transaction too,
// since Transaction derefs to Connection.
fn _assert_send_sync() {
    fn _assert<T: Send + Sync>() {}
    _assert::<SqliteStore>();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use layers_core::traits::{AuthProfileStore, CronStore, ArchiveStore, ProcessRunStore, EmbeddingIndexStore, SessionStore};
    use chrono::Utc;
    use std::collections::HashMap;

    #[tokio::test]
    async fn auth_profile_crud_roundtrip() {
        let store = SqliteStore::open_in_memory().await.expect("open");

        let profile = AuthProfile {
            name: "openai-main".into(),
            provider: "openai".into(),
            api_key: Some("sk-test-key".into()),
            api_base: Some("https://api.openai.com".into()),
            models: vec!["gpt-4o".into(), "gpt-4o-mini".into()],
            created_at: Utc::now(),
        };

        // Put
        store.put_profile(profile.clone()).await.expect("put");

        // Get
        let fetched = store.get_profile("openai-main").await.expect("get");
        assert_eq!(fetched.name, "openai-main");
        assert_eq!(fetched.provider, "openai");
        assert_eq!(fetched.api_key.as_deref(), Some("sk-test-key"));
        assert_eq!(fetched.models.len(), 2);

        // List all
        let all = store.list_profiles(None).await.expect("list all");
        assert_eq!(all.len(), 1);

        // List by provider
        let openai = store.list_profiles(Some("openai")).await.expect("list openai");
        assert_eq!(openai.len(), 1);
        let anthropic = store.list_profiles(Some("anthropic")).await.expect("list anthropic");
        assert!(anthropic.is_empty());

        // Delete
        store.delete_profile("openai-main").await.expect("delete");
        let err = store.get_profile("openai-main").await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn auth_profile_upsert_replaces() {
        let store = SqliteStore::open_in_memory().await.expect("open");

        let p1 = AuthProfile {
            name: "anthropic-main".into(),
            provider: "anthropic".into(),
            api_key: Some("key-v1".into()),
            api_base: None,
            models: vec![],
            created_at: Utc::now(),
        };
        store.put_profile(p1).await.expect("put v1");

        let p2 = AuthProfile {
            name: "anthropic-main".into(),
            provider: "anthropic".into(),
            api_key: Some("key-v2".into()),
            api_base: None,
            models: vec!["claude-sonnet-4-6".into()],
            created_at: Utc::now(),
        };
        store.put_profile(p2).await.expect("put v2");

        let fetched = store.get_profile("anthropic-main").await.expect("get");
        assert_eq!(fetched.api_key.as_deref(), Some("key-v2"));
        assert_eq!(fetched.models.len(), 1);
    }

    // -----------------------------------------------------------------------
    // CronStore tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn cron_job_crud_roundtrip() {
        let store = SqliteStore::open_in_memory().await.expect("open");

        let job = CronJob {
            id: "morning-digest".into(),
            schedule: CronSchedule { cron: "0 9 * * *".into(), timezone: None },
            payload: CronPayload { prompt: "Summarize news".into(), system: None, metadata: Default::default() },
            session_target: None,
            delivery: None,
            enabled: true,
        };

        store.put_job(job.clone()).await.expect("put");
        let fetched = store.get_job("morning-digest").await.expect("get");
        assert_eq!(fetched.id, "morning-digest");
        assert_eq!(fetched.schedule.cron, "0 9 * * *");
        assert!(fetched.enabled);

        let all = store.list_jobs().await.expect("list");
        assert_eq!(all.len(), 1);

        store.delete_job("morning-digest").await.expect("delete");
        assert!(store.get_job("morning-digest").await.is_err());
    }

    #[tokio::test]
    async fn cron_run_lifecycle() {
        let store = SqliteStore::open_in_memory().await.expect("open");

        // Need a job first (FK)
        let job = CronJob {
            id: "test-job".into(),
            schedule: CronSchedule { cron: "* * * * *".into(), timezone: None },
            payload: CronPayload { prompt: "test".into(), system: None, metadata: Default::default() },
            session_target: None,
            delivery: None,
            enabled: true,
        };
        store.put_job(job).await.expect("put job");

        let now = Utc::now();
        let run = CronRun {
            id: "run-1".into(),
            job_id: "test-job".into(),
            started_at: now,
            finished_at: None,
            status: CronRunStatus::Running,
            error_message: None,
        };
        store.put_run(run).await.expect("put run");

        // Update to success
        let finished = Utc::now();
        store.update_run_status("run-1", CronRunStatus::Success, finished, None).await.expect("update");

        let fetched = store.get_run("run-1").await.expect("get");
        assert_eq!(fetched.status, CronRunStatus::Success);
        assert!(fetched.finished_at.is_some());

        // List runs for job
        let runs = store.list_runs_for_job("test-job", None).await.expect("list");
        assert_eq!(runs.len(), 1);
    }

    // -----------------------------------------------------------------------
    // ArchiveStore tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn archive_crud_roundtrip() {
        let store = SqliteStore::open_in_memory().await.expect("open");

        // Need a session first (FK)
        let session = Session {
            id: "sess-1".into(),
            agent_id: "agent-1".into(),
            dm_scope: None,
            thread_binding: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            model: None,
            metadata: Default::default(),
            message_count: 5,
            token_count: 100,
        };
        SessionStore::put(&store, &session).await.expect("put session");

        let archive = Archive {
            id: "arch-1".into(),
            session_id: "sess-1".into(),
            archived_at: Utc::now(),
            message_count: 5,
            summary: Some("A conversation about testing".into()),
        };
        ArchiveStore::put(&store, archive.clone()).await.expect("put archive");

        let fetched = ArchiveStore::get(&store, "arch-1").await.expect("get");
        assert_eq!(fetched.session_id, "sess-1");
        assert_eq!(fetched.message_count, 5);
        assert_eq!(fetched.summary.as_deref(), Some("A conversation about testing"));

        let list = store.list_for_session("sess-1").await.expect("list");
        assert_eq!(list.len(), 1);

        ArchiveStore::delete(&store, "arch-1").await.expect("delete");
        assert!(ArchiveStore::get(&store, "arch-1").await.is_err());
    }

    // -----------------------------------------------------------------------
    // ProcessRunStore tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn process_run_crud_lifecycle() {
        let store = SqliteStore::open_in_memory().await.expect("open");

        let run = ProcessRun {
            id: "proc-1".into(),
            parent_session_id: Some("sess-1".into()),
            agent_id: Some("coder".into()),
            status: ProcessRunStatus::Running,
            started_at: Utc::now(),
            finished_at: None,
            result_summary: None,
        };
        ProcessRunStore::put(&store, run.clone()).await.expect("put");

        let fetched = ProcessRunStore::get(&store, "proc-1").await.expect("get");
        assert_eq!(fetched.status, ProcessRunStatus::Running);
        assert!(fetched.finished_at.is_none());

        // Update to completed
        let finished = Utc::now();
        store.update_status("proc-1", ProcessRunStatus::Completed, finished, Some("All done")).await.expect("update");

        let updated = ProcessRunStore::get(&store, "proc-1").await.expect("get updated");
        assert_eq!(updated.status, ProcessRunStatus::Completed);
        assert_eq!(updated.result_summary.as_deref(), Some("All done"));
        assert!(updated.finished_at.is_some());

        // List by parent
        let runs = store.list_by_parent("sess-1").await.expect("list");
        assert_eq!(runs.len(), 1);
    }

    // -----------------------------------------------------------------------
    // EmbeddingIndexStore tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn embedding_index_state_roundtrip() {
        let store = SqliteStore::open_in_memory().await.expect("open");

        let state = EmbeddingIndexState {
            corpus: "memory".into(),
            embedding_model: "text-embedding-3-small".into(),
            last_indexed_at: Utc::now(),
            index_version: 3,
            metadata: {
                let mut m = HashMap::new();
                m.insert("chunk_count".into(), serde_json::json!(42));
                m
            },
        };
        EmbeddingIndexStore::put(&store, state.clone()).await.expect("put");

        let fetched = EmbeddingIndexStore::get(&store, "memory").await.expect("get");
        assert_eq!(fetched.embedding_model, "text-embedding-3-small");
        assert_eq!(fetched.index_version, 3);
        assert_eq!(fetched.metadata.get("chunk_count").unwrap(), &serde_json::json!(42));

        // Upsert
        let state_v2 = EmbeddingIndexState {
            index_version: 4,
            ..state
        };
        EmbeddingIndexStore::put(&store, state_v2).await.expect("put v2");
        let fetched_v2 = EmbeddingIndexStore::get(&store, "memory").await.expect("get v2");
        assert_eq!(fetched_v2.index_version, 4);
    }
}
