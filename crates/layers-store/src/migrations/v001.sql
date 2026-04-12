-- v001: Initial schema
-- Sessions, messages, archives, cron, auth profiles, embedding state, process runs

CREATE TABLE IF NOT EXISTS sessions (
    key             TEXT PRIMARY KEY,
    agent_id        TEXT NOT NULL,
    model           TEXT,
    created_at      TEXT NOT NULL,
    last_active     TEXT NOT NULL,
    total_tokens    INTEGER NOT NULL DEFAULT 0,
    compacted_count INTEGER NOT NULL DEFAULT 0,
    auth_profile    TEXT,
    dm_scope        TEXT,  -- JSON
    thread_binding  TEXT,  -- JSON
    metadata        TEXT NOT NULL DEFAULT '{}'  -- JSON
);

CREATE TABLE IF NOT EXISTS messages (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    session_key  TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    role         TEXT NOT NULL,
    content      TEXT NOT NULL,     -- JSON
    tool_calls   TEXT,              -- JSON
    tool_call_id TEXT,
    name         TEXT,
    reasoning    TEXT,              -- JSON
    timestamp    TEXT
);

CREATE INDEX IF NOT EXISTS idx_messages_session_seq ON messages(session_key, seq);

CREATE TABLE IF NOT EXISTS archives (
    id            TEXT PRIMARY KEY,
    session_key   TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
    archived_at   TEXT NOT NULL,
    message_count INTEGER NOT NULL,
    summary       TEXT
);

CREATE TABLE IF NOT EXISTS cron_jobs (
    id               TEXT PRIMARY KEY,
    name             TEXT,
    schedule         TEXT NOT NULL,  -- JSON
    payload          TEXT NOT NULL,  -- JSON
    session_target   TEXT,           -- JSON
    delivery         TEXT,           -- JSON
    enabled          INTEGER NOT NULL DEFAULT 1,
    delete_after_run INTEGER NOT NULL DEFAULT 0,
    misfire_policy   TEXT,
    agent_id         TEXT,
    failure_alert    TEXT            -- JSON
);

CREATE TABLE IF NOT EXISTS cron_runs (
    id            TEXT PRIMARY KEY,
    job_id        TEXT NOT NULL REFERENCES cron_jobs(id) ON DELETE CASCADE,
    started_at    TEXT NOT NULL,
    finished_at   TEXT,
    status        TEXT NOT NULL,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_cron_runs_job ON cron_runs(job_id, started_at);

CREATE TABLE IF NOT EXISTS auth_profiles (
    name              TEXT PRIMARY KEY,
    provider          TEXT NOT NULL,
    api_key_encrypted TEXT,
    api_base          TEXT,
    models            TEXT,  -- JSON
    created_at        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS embedding_index_state (
    corpus          TEXT PRIMARY KEY,
    embedding_model TEXT NOT NULL,
    last_indexed_at TEXT NOT NULL,
    index_version   INTEGER NOT NULL DEFAULT 0,
    metadata        TEXT  -- JSON
);

CREATE TABLE IF NOT EXISTS process_runs (
    id                 TEXT PRIMARY KEY,
    parent_session_key TEXT,
    agent_id           TEXT,
    status             TEXT NOT NULL,
    started_at         TEXT NOT NULL,
    finished_at        TEXT,
    result_summary     TEXT
);
