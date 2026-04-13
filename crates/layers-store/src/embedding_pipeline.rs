//! Embedding pipeline: chunking, embedding generation, and LanceDB storage.
//!
//! Provides a configurable pipeline that takes text content, splits it into
//! chunks, generates embeddings via an [`EmbeddingProvider`], and stores them
//! in a [`LanceStore`]. Index state is tracked via [`EmbeddingIndexStore`].

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tracing::debug;

use layers_core::error::Result;
use layers_core::traits::EmbeddingIndexStore;
use layers_core::types::EmbeddingIndexState;

use crate::lancedb_store::{EmbeddingChunk, LanceStore, SearchResult};

// ---------------------------------------------------------------------------
// EmbeddingProvider trait
// ---------------------------------------------------------------------------

/// Trait for generating vector embeddings from text.
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embeddings for a batch of texts.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    /// The dimensionality of the embedding vectors.
    fn dimension(&self) -> i32;
}

// ---------------------------------------------------------------------------
// LocalEmbeddingProvider (stub / testing)
// ---------------------------------------------------------------------------

/// A stub provider that returns deterministic pseudo-random vectors.
///
/// Useful for testing the pipeline without a real embedding model.
pub struct LocalEmbeddingProvider {
    dim: i32,
}

impl LocalEmbeddingProvider {
    #[must_use]
    pub fn new() -> Self {
        Self { dim: 1536 }
    }

    /// Create a stub provider with a custom dimension (for tests).
    #[must_use]
    pub fn with_dimension(dim: i32) -> Self {
        Self { dim }
    }
}

impl Default for LocalEmbeddingProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for LocalEmbeddingProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .enumerate()
            .map(|(i, text)| {
                // Deterministic pseudo-random vector seeded from text length + index.
                let seed = text.len() as f32 + i as f32;
                (0..self.dim)
                    .map(|d| (seed * 0.1 + d as f32 * 0.37).sin() * 0.5 + 0.5)
                    .collect()
            })
            .collect())
    }

    fn dimension(&self) -> i32 {
        self.dim
    }
}

// ---------------------------------------------------------------------------
// Text chunking
// ---------------------------------------------------------------------------

/// Split text into chunks suitable for embedding.
///
/// Strategy: split on double-newlines (paragraphs) first, then on single
/// newlines / sentence boundaries if a paragraph exceeds `max_tokens` chars.
/// Uses character count as a proxy for tokens (~4 chars per token).
pub fn chunk_text(text: &str, max_tokens: usize) -> Vec<String> {
    let max_chars = max_tokens * 4; // rough chars-per-token estimate

    if text.is_empty() {
        return Vec::new();
    }

    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    let mut chunks = Vec::new();

    for para in paragraphs {
        let trimmed = para.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.len() <= max_chars {
            chunks.push(trimmed.to_string());
        } else {
            // Split long paragraphs on sentence boundaries.
            split_sentences(trimmed, max_chars, &mut chunks);
        }
    }

    // Merge very small chunks with their neighbor to avoid tiny fragments.
    merge_small_chunks(&mut chunks, max_chars);

    chunks
}

/// Split a paragraph into sentence-sized chunks that fit within `max_chars`.
fn split_sentences(text: &str, max_chars: usize, out: &mut Vec<String>) {
    let mut current = String::new();

    for sentence in SentenceIter::new(text) {
        if current.len() + sentence.len() > max_chars && !current.is_empty() {
            out.push(current.trim().to_string());
            current.clear();
        }
        current.push_str(sentence);
    }

    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
}

/// Merge chunks shorter than 25% of max into their predecessor.
fn merge_small_chunks(chunks: &mut Vec<String>, max_chars: usize) {
    let min_chars = max_chars / 4;
    let mut merged = Vec::with_capacity(chunks.len());

    for chunk in chunks.drain(..) {
        if let Some(last) = merged.last_mut() {
            let last_str: &mut String = last;
            if chunk.len() < min_chars && last_str.len() + chunk.len() < max_chars {
                last_str.push('\n');
                last_str.push_str(&chunk);
                continue;
            }
        }
        merged.push(chunk);
    }

    *chunks = merged;
}

/// Simple sentence iterator that splits on `. `, `? `, `! ` boundaries.
struct SentenceIter<'a> {
    remaining: &'a str,
}

impl<'a> SentenceIter<'a> {
    fn new(text: &'a str) -> Self {
        Self { remaining: text }
    }
}

impl<'a> Iterator for SentenceIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }

        // Look for sentence-ending punctuation followed by space.
        let end_markers = [". ", "? ", "! "];
        let mut earliest = None;

        for marker in &end_markers {
            if let Some(pos) = self.remaining.find(marker) {
                let end = pos + marker.len();
                earliest = Some(match earliest {
                    Some(prev) if end < prev => end,
                    Some(prev) => prev,
                    None => end,
                });
            }
        }

        match earliest {
            Some(end) => {
                let (sentence, rest) = self.remaining.split_at(end);
                self.remaining = rest;
                Some(sentence)
            }
            None => {
                let sentence = self.remaining;
                self.remaining = "";
                Some(sentence)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RealEmbeddingPipeline
// ---------------------------------------------------------------------------

/// Full embedding pipeline: chunk → embed → store → track state.
pub struct RealEmbeddingPipeline {
    provider: Arc<dyn EmbeddingProvider>,
    store: Arc<LanceStore>,
    index_store: Arc<dyn EmbeddingIndexStore>,
}

impl RealEmbeddingPipeline {
    pub fn new(
        provider: Arc<dyn EmbeddingProvider>,
        store: Arc<LanceStore>,
        index_store: Arc<dyn EmbeddingIndexStore>,
    ) -> Self {
        Self {
            provider,
            store,
            index_store,
        }
    }

    /// Index a single document: chunk its content, embed, store, update state.
    pub async fn index_document(
        &self,
        path: &str,
        content: &str,
        session_id: &str,
        role: &str,
    ) -> Result<usize> {
        let chunks_text = chunk_text(content, 512);
        if chunks_text.is_empty() {
            return Ok(0);
        }

        debug!(path, chunks = chunks_text.len(), "indexing document");

        // Delete previous chunks for this path to avoid duplicates.
        self.store.delete_by_source_path(path).await?;

        // Generate embeddings.
        let embeddings = self.provider.embed(&chunks_text).await?;

        let now = Utc::now().timestamp();
        let chunks: Vec<EmbeddingChunk> = chunks_text
            .iter()
            .zip(embeddings.iter())
            .enumerate()
            .map(|(i, (text, embedding))| EmbeddingChunk {
                id: format!("{path}#chunk-{i}"),
                source_path: path.to_string(),
                content: text.clone(),
                role: role.to_string(),
                session_id: session_id.to_string(),
                timestamp: now,
                embedding: embedding.clone(),
            })
            .collect();

        let chunk_count = chunks.len();
        self.store.upsert_chunks(&chunks).await?;

        // Update index state.
        self.update_index_state(path, chunk_count).await?;

        Ok(chunk_count)
    }

    /// Batch-index all messages from a session.
    pub async fn index_session_messages(
        &self,
        session_id: &str,
        messages: &[(String, String)], // (role, content)
    ) -> Result<usize> {
        let mut total = 0;
        for (i, (role, content)) in messages.iter().enumerate() {
            let path = format!("session://{session_id}/msg-{i}");
            let count = self.index_document(&path, content, session_id, role).await?;
            total += count;
        }
        Ok(total)
    }

    /// Delegate vector search to the underlying LanceStore.
    pub async fn search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        self.store.vector_search(query_embedding, limit).await
    }

    /// Embed a query string and search.
    pub async fn search_text(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let embeddings = self.provider.embed(&[query.to_string()]).await?;
        let query_vec = embeddings.into_iter().next().unwrap_or_default();
        self.store.vector_search(&query_vec, limit).await
    }

    /// Update the embedding index state in the backing store.
    async fn update_index_state(&self, last_path: &str, chunk_count: usize) -> Result<()> {
        let corpus = "memory";

        // Try to get existing state, or create fresh.
        let mut metadata = match self.index_store.get(corpus).await {
            Ok(existing) => existing.metadata,
            Err(_) => HashMap::new(),
        };

        // Update metadata fields.
        metadata.insert(
            "last_indexed_path".into(),
            serde_json::json!(last_path),
        );
        let prev_count = metadata
            .get("total_chunks")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        metadata.insert(
            "total_chunks".into(),
            serde_json::json!(prev_count + chunk_count as u64),
        );

        let version = metadata
            .get("index_version")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            + 1;

        let state = EmbeddingIndexState {
            corpus: corpus.to_string(),
            embedding_model: "local-stub".to_string(),
            last_indexed_at: Utc::now(),
            index_version: version,
            metadata,
        };

        self.index_store.put(state).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- chunk_text tests ---------------------------------------------------

    #[test]
    fn chunk_text_empty_returns_empty() {
        assert!(chunk_text("", 256).is_empty());
    }

    #[test]
    fn chunk_text_splits_paragraphs() {
        let text = "First paragraph here.\n\nSecond paragraph here.\n\nThird paragraph.";
        let chunks = chunk_text(text, 256);
        // Three short paragraphs should stay separate (or merge small ones).
        assert!(!chunks.is_empty());
        // All original content should be present.
        let joined: String = chunks.join(" ");
        assert!(joined.contains("First paragraph"));
        assert!(joined.contains("Second paragraph"));
        assert!(joined.contains("Third paragraph"));
    }

    #[test]
    fn chunk_text_splits_long_paragraph_on_sentences() {
        // Create a paragraph that exceeds max_tokens * 4 chars.
        let sentence = "This is a moderately long sentence that takes up some space. ";
        let long_para = sentence.repeat(20); // ~1200 chars
        let chunks = chunk_text(&long_para, 64); // 64 tokens ≈ 256 chars
        assert!(chunks.len() > 1, "should split into multiple chunks, got {}", chunks.len());
        for chunk in &chunks {
            // Each chunk should be ≤ max_chars (with some tolerance for sentence boundaries).
            assert!(chunk.len() <= 300, "chunk too long: {} chars", chunk.len());
        }
    }

    #[test]
    fn chunk_text_preserves_short_content() {
        let text = "Short content.";
        let chunks = chunk_text(text, 256);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Short content.");
    }

    // -- LocalEmbeddingProvider tests ----------------------------------------

    #[tokio::test]
    async fn local_provider_returns_correct_dimension() {
        let provider = LocalEmbeddingProvider::new();
        assert_eq!(provider.dimension(), 1536);

        let vecs = provider.embed(&["hello".into()]).await.unwrap();
        assert_eq!(vecs.len(), 1);
        assert_eq!(vecs[0].len(), 1536);
    }

    #[tokio::test]
    async fn local_provider_returns_right_count() {
        let provider = LocalEmbeddingProvider::with_dimension(8);
        let texts: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let vecs = provider.embed(&texts).await.unwrap();
        assert_eq!(vecs.len(), 3);
        for v in &vecs {
            assert_eq!(v.len(), 8);
        }
    }

    #[tokio::test]
    async fn local_provider_deterministic() {
        let provider = LocalEmbeddingProvider::with_dimension(4);
        let v1 = provider.embed(&["hello".into()]).await.unwrap();
        let v2 = provider.embed(&["hello".into()]).await.unwrap();
        assert_eq!(v1, v2);
    }

    // -- Pipeline integration tests ------------------------------------------

    #[tokio::test]
    async fn pipeline_index_and_search() {
        let dir = tempfile::tempdir().unwrap();
        let dim = 8;

        let provider = Arc::new(LocalEmbeddingProvider::with_dimension(dim));
        let lance = Arc::new(
            LanceStore::open_with_dim(dir.path().join("lance"), "pipeline_test", dim)
                .await
                .unwrap(),
        );

        // Use a simple in-memory index store.
        let index_store = Arc::new(InMemoryIndexStore::new());

        let pipeline = RealEmbeddingPipeline::new(provider.clone(), lance, index_store.clone());

        // Index a document.
        let count = pipeline
            .index_document(
                "/docs/test.md",
                "First paragraph about Rust.\n\nSecond paragraph about embeddings.",
                "sess-1",
                "user",
            )
            .await
            .unwrap();
        assert!(count > 0, "should have indexed at least one chunk");

        // Search should return results.
        let results = pipeline.search_text("Rust embeddings", 5).await.unwrap();
        assert!(!results.is_empty(), "search should return results");

        // Verify index state was updated.
        let state = index_store.get("memory").await.unwrap();
        assert_eq!(state.corpus, "memory");
        assert!(state.metadata.contains_key("total_chunks"));
        assert!(state.metadata.contains_key("last_indexed_path"));
    }

    #[tokio::test]
    async fn pipeline_index_session_messages() {
        let dir = tempfile::tempdir().unwrap();
        let dim = 4;

        let provider = Arc::new(LocalEmbeddingProvider::with_dimension(dim));
        let lance = Arc::new(
            LanceStore::open_with_dim(dir.path().join("lance"), "session_test", dim)
                .await
                .unwrap(),
        );
        let index_store = Arc::new(InMemoryIndexStore::new());

        let pipeline = RealEmbeddingPipeline::new(provider, lance, index_store);

        let messages = vec![
            ("user".to_string(), "How do I use LanceDB?".to_string()),
            ("assistant".to_string(), "You can use LanceDB by creating a connection and table.".to_string()),
        ];

        let total = pipeline
            .index_session_messages("sess-42", &messages)
            .await
            .unwrap();
        assert!(total >= 2, "should index at least one chunk per message");
    }

    // -- In-memory EmbeddingIndexStore for tests -----------------------------

    use tokio::sync::Mutex;

    struct InMemoryIndexStore {
        data: Mutex<HashMap<String, EmbeddingIndexState>>,
    }

    impl InMemoryIndexStore {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl EmbeddingIndexStore for InMemoryIndexStore {
        async fn put(&self, state: EmbeddingIndexState) -> Result<()> {
            self.data.lock().await.insert(state.corpus.clone(), state);
            Ok(())
        }
        async fn get(&self, corpus: &str) -> Result<EmbeddingIndexState> {
            self.data
                .lock()
                .await
                .get(corpus)
                .cloned()
                .ok_or_else(|| {
                    layers_core::error::LayersError::Config(format!(
                        "embedding index state not found: {corpus}"
                    ))
                })
        }
    }
}
