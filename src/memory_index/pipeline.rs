//! Memory indexing pipeline: chunking, embedding, and storage orchestration.
#![allow(clippy::missing_errors_doc)]

use std::fmt::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::memory_index::store::MemoryStore;
use crate::memory_index::types::{EmbeddingConfig, EmbeddingProvider, MemoryChunk};

/// Maximum characters per chunk.
const MAX_CHUNK_SIZE: usize = 2048;

/// Overlap in characters between consecutive chunks.
const CHUNK_OVERLAP: usize = 200;

/// Maximum texts per embedding API call.
const EMBEDDING_BATCH_SIZE: usize = 64;

/// Compute a deterministic chunk ID from source path and content prefix.
///
/// Returns the first 16 hex characters of `sha256(source_path + content_prefix)`.
#[must_use]
pub fn chunk_id(source_path: &str, content_prefix: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_path.as_bytes());
    hasher.update(content_prefix.as_bytes());
    let hash = hasher.finalize();
    hash[..8].iter().fold(String::with_capacity(16), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Split text into chunks using paragraph boundaries.
///
/// Chunks target at most ~2048 characters with ~200 character overlap.
#[must_use]
pub fn chunk_text(content: &str, source_path: &str) -> Vec<MemoryChunk> {
    let paragraphs: Vec<&str> = content.split("\n\n").collect();
    let mut chunks = Vec::new();
    let mut current = String::new();
    let now = Utc::now();

    for para in &paragraphs {
        if !current.is_empty() && current.len() + para.len() + 2 > MAX_CHUNK_SIZE {
            push_chunk(&mut chunks, &current, source_path, now);
            let overlap_start = current.len().saturating_sub(CHUNK_OVERLAP);
            current = current[overlap_start..].to_string();
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }

    if !current.is_empty() {
        push_chunk(&mut chunks, &current, source_path, now);
    }

    chunks
}

fn push_chunk(
    chunks: &mut Vec<MemoryChunk>,
    content: &str,
    source_path: &str,
    timestamp: chrono::DateTime<Utc>,
) {
    let prefix = &content[..content.len().min(64)];
    let id = chunk_id(source_path, prefix);
    chunks.push(MemoryChunk {
        id,
        source_path: source_path.to_string(),
        content: content.to_string(),
        role: String::new(),
        session_id: String::new(),
        timestamp,
        embedding: None,
    });
}

/// Generate embeddings for texts using the configured provider.
///
/// Batches requests at most 64 texts per API call.
pub fn generate_embeddings(config: &EmbeddingConfig, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
    let mut all = Vec::with_capacity(texts.len());
    for batch in texts.chunks(EMBEDDING_BATCH_SIZE) {
        let embeddings = match &config.provider {
            EmbeddingProvider::OpenAi { api_key } => embed_openai(api_key, &config.model, batch)?,
            EmbeddingProvider::Ollama { base_url } => {
                embed_ollama(base_url, &config.model, batch)?
            }
        };
        all.extend(embeddings);
    }
    Ok(all)
}

fn embed_openai(api_key: &str, model: &str, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
    let body = serde_json::json!({
        "input": texts,
        "model": model,
    });
    let resp: serde_json::Value = {
        let response = ureq::post("https://api.openai.com/v1/embeddings")
            .set("Authorization", &format!("Bearer {api_key}"))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())?;
        serde_json::from_reader(response.into_reader())?
    };
    let data = resp["data"].as_array().context("missing data in response")?;
    data.iter()
        .map(|item| {
            item["embedding"]
                .as_array()
                .context("missing embedding field")
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v: &serde_json::Value| v.as_f64().map(|f: f64| f as f32))
                        .collect()
                })
        })
        .collect()
}

fn embed_ollama(base_url: &str, model: &str, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
    texts
        .iter()
        .map(|text| {
            let body = serde_json::json!({
                "model": model,
                "input": text,
            });
            let url = format!("{base_url}/api/embed");
            let resp: serde_json::Value = {
                let response = ureq::post(&url)
                    .set("Content-Type", "application/json")
                    .send_string(&body.to_string())?;
                serde_json::from_reader(response.into_reader())?
            };
            resp["embeddings"][0]
                .as_array()
                .context("missing embeddings in response")
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v: &serde_json::Value| v.as_f64().map(|f: f64| f as f32))
                        .collect()
                })
        })
        .collect()
}

/// Orchestrates file indexing: chunking, embedding, and storage.
pub struct MemoryPipeline {
    /// Root directory for file discovery.
    pub workspace_path: PathBuf,
    /// Embedding generation configuration.
    pub embedding_config: EmbeddingConfig,
    /// Path to the database directory.
    pub db_path: PathBuf,
}

impl MemoryPipeline {
    /// Create a new pipeline instance.
    #[must_use]
    pub fn new(
        workspace_path: PathBuf,
        embedding_config: EmbeddingConfig,
        db_path: PathBuf,
    ) -> Self {
        Self {
            workspace_path,
            embedding_config,
            db_path,
        }
    }

    /// Index a single file: chunk, embed, and store.
    pub fn index_file(&self, path: &Path) -> Result<Vec<MemoryChunk>> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let source_path = path.to_string_lossy().to_string();
        let mut chunks = chunk_text(&content, &source_path);

        let texts: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
        let embeddings = generate_embeddings(&self.embedding_config, &texts)?;

        for (chunk, emb) in chunks.iter_mut().zip(embeddings) {
            chunk.embedding = Some(emb);
        }

        let store = MemoryStore::open_or_create(&self.db_path, self.embedding_config.dimensions)?;
        store.upsert_chunks(&chunks)?;
        store.update_index_state(&source_path, &self.embedding_config.model)?;

        Ok(chunks)
    }

    /// Index all files in the workspace matching a glob pattern.
    pub fn index_directory(&self, pattern: &str) -> Result<usize> {
        let glob = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()?
            .compile_matcher();

        let files = walk_dir(&self.workspace_path)?;
        let mut count = 0;
        for file in &files {
            if glob.is_match(file) {
                self.index_file(file)?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Clear the index and re-index all matching files.
    pub fn rebuild_index(&self, pattern: &str) -> Result<usize> {
        let store = MemoryStore::open_or_create(&self.db_path, self.embedding_config.dimensions)?;
        store.clear()?;
        self.index_directory(pattern)
    }

    /// Remove all chunks associated with a file.
    pub fn remove_file(&self, path: &Path) -> Result<()> {
        let source_path = path.to_string_lossy().to_string();
        let store = MemoryStore::open_or_create(&self.db_path, self.embedding_config.dimensions)?;
        store.delete_by_source_path(&source_path)
    }
}

fn walk_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk_dir(&path)?);
        } else {
            files.push(path);
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_id_deterministic() {
        let id1 = chunk_id("test.md", "hello world");
        let id2 = chunk_id("test.md", "hello world");
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 16);

        // Different source path produces different ID
        let id3 = chunk_id("other.md", "hello world");
        assert_ne!(id1, id3);

        // Different content produces different ID
        let id4 = chunk_id("test.md", "goodbye world");
        assert_ne!(id1, id4);
    }

    #[test]
    fn test_chunking_basic() {
        let content = "paragraph one\n\nparagraph two\n\nparagraph three";
        let chunks = chunk_text(content, "test.md");
        assert_eq!(chunks.len(), 1); // Small enough for one chunk
        assert!(chunks[0].content.contains("paragraph one"));
        assert!(chunks[0].content.contains("paragraph three"));
        assert_eq!(chunks[0].source_path, "test.md");
        assert!(chunks[0].embedding.is_none());
    }

    #[test]
    fn test_chunking_splits_large_content() {
        let para = "x".repeat(1000);
        let content = format!("{para}\n\n{para}\n\n{para}");
        let chunks = chunk_text(&content, "big.md");
        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );
        for chunk in &chunks {
            assert!(!chunk.content.is_empty());
            assert_eq!(chunk.source_path, "big.md");
        }
    }

    #[test]
    fn test_chunking_overlap() {
        // Create content that will produce exactly 2 chunks
        let para_a = "a".repeat(1200);
        let para_b = "b".repeat(1200);
        let content = format!("{para_a}\n\n{para_b}");
        let chunks = chunk_text(&content, "overlap.md");
        assert_eq!(chunks.len(), 2);
        // Second chunk should start with overlap from first chunk's tail
        let first_tail = &chunks[0].content[chunks[0].content.len().saturating_sub(CHUNK_OVERLAP)..];
        assert!(
            chunks[1].content.starts_with(first_tail),
            "second chunk should contain overlap from first"
        );
    }

    #[test]
    fn test_chunking_empty() {
        let chunks = chunk_text("", "empty.md");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_embedding_batch_size() {
        assert_eq!(EMBEDDING_BATCH_SIZE, 64);
    }
}
