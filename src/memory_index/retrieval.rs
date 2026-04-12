//! Hybrid search combining vector similarity and keyword matching via RRF.
#![allow(clippy::missing_errors_doc)]

use std::collections::HashMap;

use anyhow::Result;

use crate::memory_index::store::MemoryStore;
use crate::memory_index::types::{MemoryChunk, SearchResult, SearchSource};

/// Reciprocal Rank Fusion constant (controls rank sensitivity).
const RRF_K: f32 = 60.0;

/// Perform hybrid search combining vector similarity and keyword matching.
///
/// Runs vector search (limit×2) and keyword search (limit), then merges
/// results using Reciprocal Rank Fusion.
pub fn hybrid_search(
    store: &MemoryStore,
    query: &str,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let vector_results = store.vector_search(query_embedding, limit * 2)?;
    let keyword_results = store.keyword_search(query, limit)?;
    let merged = reciprocal_rank_fusion(&vector_results, &keyword_results);
    Ok(merged.into_iter().take(limit).collect())
}

/// Merge two ranked lists using Reciprocal Rank Fusion.
///
/// `RRF(d) = Σ 1/(k + rank)` where k=60 and rank is 1-based.
fn reciprocal_rank_fusion(
    vector_results: &[(MemoryChunk, f32)],
    keyword_results: &[(MemoryChunk, f32)],
) -> Vec<SearchResult> {
    let mut scores: HashMap<String, (f32, MemoryChunk, SearchSource)> = HashMap::new();

    for (rank, (chunk, _)) in vector_results.iter().enumerate() {
        let rrf_score = 1.0 / (RRF_K + (rank + 1) as f32);
        scores
            .entry(chunk.id.clone())
            .and_modify(|(s, _, src)| {
                *s += rrf_score;
                *src = SearchSource::Hybrid;
            })
            .or_insert_with(|| (rrf_score, chunk.clone(), SearchSource::Vector));
    }

    for (rank, (chunk, _)) in keyword_results.iter().enumerate() {
        let rrf_score = 1.0 / (RRF_K + (rank + 1) as f32);
        scores
            .entry(chunk.id.clone())
            .and_modify(|(s, _, src)| {
                *s += rrf_score;
                *src = SearchSource::Hybrid;
            })
            .or_insert_with(|| (rrf_score, chunk.clone(), SearchSource::Keyword));
    }

    let mut results: Vec<SearchResult> = scores
        .into_values()
        .map(|(score, chunk, source)| SearchResult {
            chunk,
            score,
            source,
        })
        .collect();

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_chunk(id: &str, content: &str, embedding: Vec<f32>) -> MemoryChunk {
        MemoryChunk {
            id: id.to_string(),
            source_path: "test.md".to_string(),
            content: content.to_string(),
            role: "user".to_string(),
            session_id: "s1".to_string(),
            timestamp: Utc::now(),
            embedding: Some(embedding),
        }
    }

    #[test]
    fn test_rrf_vector_only() {
        let vector = vec![
            (make_chunk("a", "alpha", vec![1.0]), 0.9),
            (make_chunk("b", "beta", vec![0.5]), 0.5),
        ];
        let keyword: Vec<(MemoryChunk, f32)> = vec![];
        let results = reciprocal_rank_fusion(&vector, &keyword);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].chunk.id, "a");
        assert_eq!(results[0].source, SearchSource::Vector);
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn test_rrf_hybrid_boost() {
        // Chunk "c" appears in both — should get a boosted score
        let vector = vec![
            (make_chunk("a", "alpha", vec![1.0]), 0.9),
            (make_chunk("c", "common", vec![0.5]), 0.5),
        ];
        let keyword = vec![
            (make_chunk("c", "common", vec![0.5]), 2.0),
            (make_chunk("b", "beta", vec![0.1]), 1.0),
        ];
        let results = reciprocal_rank_fusion(&vector, &keyword);

        // "c" should be boosted to Hybrid and rank highest
        let c_result = results.iter().find(|r| r.chunk.id == "c").unwrap();
        assert_eq!(c_result.source, SearchSource::Hybrid);
        assert_eq!(results[0].chunk.id, "c");
    }

    #[test]
    fn test_hybrid_search_integration() {
        let dir = tempfile::tempdir().unwrap();
        let store =
            crate::memory_index::store::MemoryStore::open_or_create(dir.path(), 4).unwrap();

        let chunks = vec![
            MemoryChunk {
                id: "r1".to_string(),
                source_path: "lang.md".to_string(),
                content: "rust programming language systems".to_string(),
                role: "user".to_string(),
                session_id: "s1".to_string(),
                timestamp: Utc::now(),
                embedding: Some(vec![1.0, 0.0, 0.0, 0.0]),
            },
            MemoryChunk {
                id: "p1".to_string(),
                source_path: "lang.md".to_string(),
                content: "python scripting data science".to_string(),
                role: "user".to_string(),
                session_id: "s1".to_string(),
                timestamp: Utc::now(),
                embedding: Some(vec![0.0, 1.0, 0.0, 0.0]),
            },
        ];
        store.upsert_chunks(&chunks).unwrap();

        let results = hybrid_search(
            &store,
            "rust programming",
            &[0.9, 0.1, 0.0, 0.0],
            10,
        )
        .unwrap();

        assert!(!results.is_empty());
        // Rust chunk should rank highest (matches both vector and keyword)
        assert_eq!(results[0].chunk.id, "r1");
        assert_eq!(results[0].source, SearchSource::Hybrid);
    }
}
