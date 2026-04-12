//! `LanceDB` vector storage and `SQLite` index state management.
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow_array::{ArrayRef, FixedSizeListArray, Float32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use chrono::Utc;
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use rusqlite::Connection;

use crate::memory_index::types::{IndexState, MemoryChunk};

const TABLE_NAME: &str = "memory_chunks";

/// Vector storage (`LanceDB`) and index state (`SQLite`) manager.
pub struct MemoryStore {
    sqlite: Connection,
    rt: tokio::runtime::Runtime,
    db: lancedb::Connection,
    schema: Arc<Schema>,
}

impl MemoryStore {
    /// Open or create the memory store at the given path.
    pub fn open_or_create(db_path: &Path, dimensions: usize) -> Result<Self> {
        std::fs::create_dir_all(db_path)?;

        // SQLite setup
        let sqlite_path = db_path.join("index_state.db");
        let sqlite = Connection::open(sqlite_path)?;
        sqlite.execute_batch(
            "CREATE TABLE IF NOT EXISTS embedding_index_state (
                corpus TEXT PRIMARY KEY,
                embedding_model TEXT NOT NULL,
                last_indexed_at TEXT NOT NULL,
                index_version INTEGER NOT NULL DEFAULT 1,
                metadata TEXT NOT NULL DEFAULT '{}'
            )",
        )?;

        // LanceDB setup
        let dims = i32::try_from(dimensions)?;
        let schema = build_schema(dims);
        let rt = tokio::runtime::Runtime::new()?;
        let lance_path = db_path.join("vectors.lance");
        let lance_uri = lance_path.to_string_lossy().to_string();
        let db = rt.block_on(lancedb::connect(&lance_uri).execute())?;

        // Ensure table exists
        let table_names = rt.block_on(db.table_names().execute())?;
        if !table_names.contains(&TABLE_NAME.to_string()) {
            rt.block_on(db.create_empty_table(TABLE_NAME, schema.clone()).execute())?;
        }

        Ok(Self {
            sqlite,
            rt,
            db,
            schema,
        })
    }

    /// Insert or replace chunks (deletes existing chunks from same source paths first).
    pub fn upsert_chunks(&self, chunks: &[MemoryChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        // Delete existing chunks from same source paths
        let source_paths: HashSet<&str> = chunks.iter().map(|c| c.source_path.as_str()).collect();
        for sp in &source_paths {
            self.delete_by_source_path(sp)?;
        }

        // Add new chunks
        let batch = chunks_to_batch(chunks, &self.schema)?;
        let table = self.open_table()?;
        self.rt.block_on(table.add(vec![batch]).execute())?;

        Ok(())
    }

    /// Delete all chunks for a given source path.
    pub fn delete_by_source_path(&self, source_path: &str) -> Result<()> {
        let table = self.open_table()?;
        let escaped = source_path.replace('\'', "''");
        self.rt
            .block_on(table.delete(&format!("source_path = '{escaped}'")))?;
        Ok(())
    }

    /// Find chunks most similar to the query embedding.
    pub fn vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(MemoryChunk, f32)>> {
        let table = self.open_table()?;
        let count = self.rt.block_on(table.count_rows(None))?;
        if count == 0 {
            return Ok(Vec::new());
        }

        let stream = self.rt.block_on(
            table
                .vector_search(query_embedding.to_vec())?
                .limit(limit)
                .execute(),
        )?;
        let batches: Vec<RecordBatch> = self.rt.block_on(stream.try_collect())?;

        let mut results = Vec::new();
        for batch in &batches {
            let chunks = batch_to_chunks(batch);
            let distances = batch
                .column_by_name("_distance")
                .context("missing _distance column")?
                .as_any()
                .downcast_ref::<Float32Array>()
                .context("_distance not Float32")?;
            for (i, chunk) in chunks.into_iter().enumerate() {
                let distance = distances.value(i);
                let score = 1.0 / (1.0 + distance);
                results.push((chunk, score));
            }
        }

        Ok(results)
    }

    /// Find chunks containing query keywords.
    pub fn keyword_search(&self, query: &str, limit: usize) -> Result<Vec<(MemoryChunk, f32)>> {
        let table = self.open_table()?;
        let count = self.rt.block_on(table.count_rows(None))?;
        if count == 0 {
            return Ok(Vec::new());
        }

        let words: Vec<&str> = query.split_whitespace().collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }

        let conditions: Vec<String> = words
            .iter()
            .map(|w| {
                let esc = w.replace('\'', "''");
                format!("content LIKE '%{esc}%'")
            })
            .collect();
        let filter = conditions.join(" OR ");

        let stream = self
            .rt
            .block_on(table.query().only_if(filter).limit(limit).execute())?;
        let batches: Vec<RecordBatch> = self.rt.block_on(stream.try_collect())?;

        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        for batch in &batches {
            let chunks = batch_to_chunks(batch);
            for chunk in chunks {
                let content_lower = chunk.content.to_lowercase();
                let score = query_lower
                    .split_whitespace()
                    .filter(|w| w.len() > 2 && content_lower.contains(w))
                    .count();
                results.push((chunk, score as f32));
            }
        }

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        Ok(results)
    }

    /// Clear all stored chunks (drops and recreates the vector table).
    pub fn clear(&self) -> Result<()> {
        let table_names = self.rt.block_on(self.db.table_names().execute())?;
        if table_names.contains(&TABLE_NAME.to_string()) {
            self.rt.block_on(self.db.drop_table(TABLE_NAME, &[]))?;
        }
        self.rt
            .block_on(self.db.create_empty_table(TABLE_NAME, self.schema.clone()).execute())?;
        Ok(())
    }

    /// Get the index state for a corpus.
    pub fn get_index_state(&self, corpus: &str) -> Result<Option<IndexState>> {
        let mut stmt = self.sqlite.prepare(
            "SELECT corpus, embedding_model, last_indexed_at, index_version, metadata
             FROM embedding_index_state WHERE corpus = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![corpus], |row| {
            Ok(IndexState {
                corpus: row.get(0)?,
                embedding_model: row.get(1)?,
                last_indexed_at: {
                    let s: String = row.get(2)?;
                    s.parse().unwrap_or_else(|_| Utc::now())
                },
                index_version: row.get(3)?,
                metadata: {
                    let s: String = row.get(4)?;
                    serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)
                },
            })
        })?;
        match rows.next() {
            Some(Ok(state)) => Ok(Some(state)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Update or insert the index state for a corpus.
    pub fn update_index_state(&self, corpus: &str, model: &str) -> Result<()> {
        self.sqlite.execute(
            "INSERT INTO embedding_index_state (corpus, embedding_model, last_indexed_at, index_version, metadata)
             VALUES (?1, ?2, ?3, 1, '{}')
             ON CONFLICT(corpus) DO UPDATE SET
                embedding_model = excluded.embedding_model,
                last_indexed_at = excluded.last_indexed_at,
                index_version = index_version + 1",
            rusqlite::params![corpus, model, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    fn open_table(&self) -> Result<lancedb::Table> {
        Ok(self
            .rt
            .block_on(self.db.open_table(TABLE_NAME).execute())?)
    }
}

fn build_schema(dimensions: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("source_path", DataType::Utf8, false),
        Field::new("content", DataType::Utf8, false),
        Field::new("role", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        Field::new("timestamp", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dimensions,
            ),
            false,
        ),
    ]))
}

fn chunks_to_batch(chunks: &[MemoryChunk], schema: &Arc<Schema>) -> Result<RecordBatch> {
    let dims = schema
        .field_with_name("embedding")
        .ok()
        .and_then(|f| match f.data_type() {
            DataType::FixedSizeList(_, size) => Some(*size as usize),
            _ => None,
        })
        .context("cannot determine embedding dimensions from schema")?;

    let ids: Vec<&str> = chunks.iter().map(|c| c.id.as_str()).collect();
    let source_paths: Vec<&str> = chunks.iter().map(|c| c.source_path.as_str()).collect();
    let contents: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
    let roles: Vec<&str> = chunks.iter().map(|c| c.role.as_str()).collect();
    let session_ids: Vec<&str> = chunks.iter().map(|c| c.session_id.as_str()).collect();
    let timestamps: Vec<String> = chunks.iter().map(|c| c.timestamp.to_rfc3339()).collect();
    let timestamp_refs: Vec<&str> = timestamps.iter().map(String::as_str).collect();

    let flat_embeddings: Vec<f32> = chunks
        .iter()
        .flat_map(|c| {
            c.embedding
                .clone()
                .unwrap_or_else(|| vec![0.0; dims])
        })
        .collect();

    let values: ArrayRef = Arc::new(Float32Array::from(flat_embeddings));
    let field = Arc::new(Field::new("item", DataType::Float32, true));
    let embedding_array = FixedSizeListArray::new(field, dims as i32, values, None);

    let columns: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(ids)),
        Arc::new(StringArray::from(source_paths)),
        Arc::new(StringArray::from(contents)),
        Arc::new(StringArray::from(roles)),
        Arc::new(StringArray::from(session_ids)),
        Arc::new(StringArray::from(timestamp_refs)),
        Arc::new(embedding_array),
    ];

    Ok(RecordBatch::try_new(schema.clone(), columns)?)
}

fn batch_to_chunks(batch: &RecordBatch) -> Vec<MemoryChunk> {
    let ids = batch
        .column_by_name("id")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let source_paths = batch
        .column_by_name("source_path")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let contents = batch
        .column_by_name("content")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let roles = batch
        .column_by_name("role")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let session_ids = batch
        .column_by_name("session_id")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let timestamps = batch
        .column_by_name("timestamp")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let embeddings = batch
        .column_by_name("embedding")
        .unwrap()
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .unwrap();

    (0..batch.num_rows())
        .map(|i| {
            let emb_values = embeddings.value(i);
            let emb_array = emb_values
                .as_any()
                .downcast_ref::<Float32Array>()
                .unwrap();
            let embedding: Vec<f32> = emb_array.values().to_vec();

            MemoryChunk {
                id: ids.value(i).to_string(),
                source_path: source_paths.value(i).to_string(),
                content: contents.value(i).to_string(),
                role: roles.value(i).to_string(),
                session_id: session_ids.value(i).to_string(),
                timestamp: timestamps.value(i).parse().unwrap_or_else(|_| Utc::now()),
                embedding: Some(embedding),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(id: &str, source: &str, content: &str, embedding: Vec<f32>) -> MemoryChunk {
        MemoryChunk {
            id: id.to_string(),
            source_path: source.to_string(),
            content: content.to_string(),
            role: "user".to_string(),
            session_id: "s1".to_string(),
            timestamp: Utc::now(),
            embedding: Some(embedding),
        }
    }

    #[test]
    fn test_store_crud() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open_or_create(dir.path(), 8).unwrap();

        let chunks = vec![
            make_chunk("c1", "a.md", "hello world", vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
            make_chunk("c2", "a.md", "goodbye world", vec![0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ];

        // Upsert
        store.upsert_chunks(&chunks).unwrap();

        // Vector search
        let results = store
            .vector_search(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 10)
            .unwrap();
        assert_eq!(results.len(), 2);
        // Closest to [1,0,0,...] should be "hello world"
        assert_eq!(results[0].0.id, "c1");

        // Keyword search
        let results = store.keyword_search("hello", 10).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].0.content.contains("hello"));

        // Delete by source path
        store.delete_by_source_path("a.md").unwrap();
        let results = store
            .vector_search(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 10)
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_store_upsert_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open_or_create(dir.path(), 4).unwrap();

        let chunks_v1 = vec![make_chunk("c1", "f.md", "version one", vec![1.0, 0.0, 0.0, 0.0])];
        store.upsert_chunks(&chunks_v1).unwrap();

        // Upsert with same source path replaces
        let chunks_v2 = vec![make_chunk("c2", "f.md", "version two", vec![0.0, 1.0, 0.0, 0.0])];
        store.upsert_chunks(&chunks_v2).unwrap();

        let results = store.vector_search(&[0.0, 1.0, 0.0, 0.0], 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.content, "version two");
    }

    #[test]
    fn test_index_state() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open_or_create(dir.path(), 8).unwrap();

        assert!(store.get_index_state("test_corpus").unwrap().is_none());

        store
            .update_index_state("test_corpus", "test-model")
            .unwrap();
        let state = store.get_index_state("test_corpus").unwrap().unwrap();
        assert_eq!(state.corpus, "test_corpus");
        assert_eq!(state.embedding_model, "test-model");
        assert_eq!(state.index_version, 1);

        // Second update increments version
        store
            .update_index_state("test_corpus", "test-model-v2")
            .unwrap();
        let state = store.get_index_state("test_corpus").unwrap().unwrap();
        assert_eq!(state.embedding_model, "test-model-v2");
        assert_eq!(state.index_version, 2);
    }

    #[test]
    fn test_clear() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryStore::open_or_create(dir.path(), 4).unwrap();

        let chunks = vec![make_chunk("c1", "x.md", "some content", vec![1.0, 0.0, 0.0, 0.0])];
        store.upsert_chunks(&chunks).unwrap();

        store.clear().unwrap();

        let results = store.vector_search(&[1.0, 0.0, 0.0, 0.0], 10).unwrap();
        assert!(results.is_empty());
    }
}
