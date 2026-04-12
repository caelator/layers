//! Append-only JSONL transcript persistence.
//!
//! Each session gets its own `.jsonl` file under the transcripts directory,
//! keyed by a SHA-256 hash of the session ID. Files are automatically
//! rotated when they exceed a configurable size threshold.

use std::path::{Path, PathBuf};

use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tracing::debug;

use layers_core::error::{LayersError, Result};
use layers_core::types::Message;

/// Default maximum file size before rotation (10 MB).
const DEFAULT_MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// JSONL transcript store for append-only message logging.
pub struct JsonlStore {
    base_dir: PathBuf,
    max_file_size: u64,
}

impl JsonlStore {
    /// Create a new JSONL store rooted at the given directory.
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            max_file_size: DEFAULT_MAX_FILE_SIZE,
        }
    }

    /// Create a store using the default path (`~/.layers/transcripts`).
    pub fn default_path() -> Result<Self> {
        let home = dirs::home_dir()
            .ok_or_else(|| LayersError::Config("cannot determine home directory".into()))?;
        Ok(Self::new(home.join(".layers").join("transcripts")))
    }

    /// Set the maximum file size before rotation.
    pub fn with_max_file_size(mut self, bytes: u64) -> Self {
        self.max_file_size = bytes;
        self
    }

    /// Append a message to the session transcript.
    pub async fn append(&self, session_id: &str, message: &Message) -> Result<()> {
        tokio::fs::create_dir_all(&self.base_dir).await?;

        let path = self.transcript_path(session_id);

        // Check rotation
        if let Ok(meta) = tokio::fs::metadata(&path).await {
            if meta.len() >= self.max_file_size {
                self.rotate(session_id).await?;
            }
        }

        let line = serde_json::to_string(message)?;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;

        Ok(())
    }

    /// Read all messages from a session transcript.
    pub async fn read_all(&self, session_id: &str) -> Result<Vec<Message>> {
        let path = self.transcript_path(session_id);

        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = tokio::fs::read_to_string(&path).await?;
        let mut messages = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Message>(line) {
                Ok(msg) => messages.push(msg),
                Err(e) => {
                    debug!("skipping malformed JSONL line: {e}");
                }
            }
        }

        Ok(messages)
    }

    /// Read messages from all transcript files for a session (including rotated).
    pub async fn read_all_with_rotated(&self, session_id: &str) -> Result<Vec<Message>> {
        let hash = Self::session_hash(session_id);
        let mut messages = Vec::new();

        let mut entries = tokio::fs::read_dir(&self.base_dir).await?;
        let mut paths = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&hash) && name.ends_with(".jsonl") {
                paths.push(entry.path());
            }
        }

        paths.sort();

        for path in paths {
            let content = tokio::fs::read_to_string(&path).await?;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(msg) = serde_json::from_str::<Message>(line) {
                    messages.push(msg);
                }
            }
        }

        Ok(messages)
    }

    /// Get the primary transcript file path for a session.
    fn transcript_path(&self, session_id: &str) -> PathBuf {
        let hash = Self::session_hash(session_id);
        self.base_dir.join(format!("{hash}.jsonl"))
    }

    /// Rotate the current transcript file by renaming it with a timestamp.
    async fn rotate(&self, session_id: &str) -> Result<()> {
        let current = self.transcript_path(session_id);
        let hash = Self::session_hash(session_id);
        let ts = Utc::now().format("%Y%m%d%H%M%S");
        let rotated = self.base_dir.join(format!("{hash}.{ts}.jsonl"));

        debug!("rotating transcript {} -> {}", current.display(), rotated.display());
        tokio::fs::rename(&current, &rotated).await?;

        Ok(())
    }

    /// Compute the SHA-256 hash prefix for a session ID.
    fn session_hash(session_id: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(session_id.as_bytes());
        let result = hasher.finalize();
        // Use first 16 hex chars for a compact but collision-resistant name
        hex_encode(&result[..8])
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
