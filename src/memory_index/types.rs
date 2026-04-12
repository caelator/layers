//! Types for the memory indexing pipeline.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A chunk of memory content with optional embedding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryChunk {
    /// Deterministic ID: `sha256(source_path + content_prefix)[..16]`.
    pub id: String,
    /// Path to the source file.
    pub source_path: String,
    /// Text content of this chunk.
    pub content: String,
    /// Role associated with this chunk (e.g. "user", "assistant").
    pub role: String,
    /// Session identifier.
    pub session_id: String,
    /// When this chunk was indexed.
    pub timestamp: DateTime<Utc>,
    /// Embedding vector, populated after embedding generation.
    pub embedding: Option<Vec<f32>>,
}

/// How a search result was found.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SearchSource {
    /// Found via vector similarity search.
    Vector,
    /// Found via keyword matching.
    Keyword,
    /// Merged result from both vector and keyword search.
    Hybrid,
}

/// A search result with score and provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The matching chunk.
    pub chunk: MemoryChunk,
    /// Relevance score (higher is better).
    pub score: f32,
    /// How this result was found.
    pub source: SearchSource,
}

/// Embedding provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmbeddingProvider {
    /// OpenAI embedding API.
    OpenAi {
        /// API key for authentication.
        api_key: String,
    },
    /// Local Ollama embedding service.
    Ollama {
        /// Base URL (e.g. `http://localhost:11434`).
        base_url: String,
    },
}

/// Configuration for embedding generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Which embedding service to use.
    pub provider: EmbeddingProvider,
    /// Model name (e.g. "text-embedding-3-small").
    pub model: String,
    /// Embedding vector dimensions.
    pub dimensions: usize,
}

/// Tracks the state of the embedding index for a given corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexState {
    /// Corpus identifier (typically a file or directory path).
    pub corpus: String,
    /// Name of the embedding model used.
    pub embedding_model: String,
    /// When the corpus was last indexed.
    pub last_indexed_at: DateTime<Utc>,
    /// Index schema version.
    pub index_version: u32,
    /// Additional metadata.
    pub metadata: serde_json::Value,
}
