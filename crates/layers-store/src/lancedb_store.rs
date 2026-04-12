//! LanceDB vector store for memory embeddings.
//!
//! Stores document chunks with their vector embeddings and supports
//! both vector similarity search and keyword filtering.

use std::path::Path;
use std::sync::Arc;

use arrow_array::types::Float32Type;
use arrow_array::{
    FixedSizeListArray, Float32Array, Int64Array, RecordBatch, RecordBatchIterator,
    RecordBatchReader, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use lancedb::database::CreateTableMode;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection, Table as LanceTable};

use futures::TryStreamExt;
use tracing::debug;

use layers_core::error::{LayersError, Result};

/// Default embedding dimension (matches common models like `text-embedding-3-small`).
const DEFAULT_EMBEDDING_DIM: i32 = 1536;

/// A chunk stored in the vector database.
#[derive(Debug, Clone)]
pub struct EmbeddingChunk {
    /// Unique identifier for this chunk.
    pub id: String,
    /// Source file or document path.
    pub source_path: String,
    /// Text content of the chunk.
    pub content: String,
    /// Role (e.g. "user", "assistant", "system").
    pub role: String,
    /// Originating session ID.
    pub session_id: String,
    /// Unix timestamp (seconds).
    pub timestamp: i64,
    /// Embedding vector.
    pub embedding: Vec<f32>,
}

/// A search result from the vector store.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The matched chunk.
    pub chunk: EmbeddingChunk,
    /// Distance score (lower is more similar for L2, higher for cosine).
    pub score: f32,
}

/// LanceDB-backed vector store for embedding storage and retrieval.
pub struct LanceStore {
    conn: Connection,
    table_name: String,
    embedding_dim: i32,
}

impl LanceStore {
    /// Open or create a LanceDB store at the given directory.
    pub async fn open(db_path: impl AsRef<Path>, table_name: &str) -> Result<Self> {
        Self::open_with_dim(db_path, table_name, DEFAULT_EMBEDDING_DIM).await
    }

    /// Open or create a LanceDB store with a custom embedding dimension.
    pub async fn open_with_dim(
        db_path: impl AsRef<Path>,
        table_name: &str,
        embedding_dim: i32,
    ) -> Result<Self> {
        let path_str = db_path.as_ref().to_string_lossy().to_string();
        let conn = connect(&path_str)
            .execute()
            .await
            .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))?;

        let store = Self {
            conn,
            table_name: table_name.to_string(),
            embedding_dim,
        };

        store.ensure_table().await?;
        Ok(store)
    }

    /// Build the Arrow schema for the embeddings table.
    fn schema(&self) -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("source_path", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("role", DataType::Utf8, false),
            Field::new("session_id", DataType::Utf8, false),
            Field::new("timestamp", DataType::Int64, false),
            Field::new(
                "embedding",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    self.embedding_dim,
                ),
                false,
            ),
        ]))
    }

    /// Ensure the table exists, creating it if necessary.
    async fn ensure_table(&self) -> Result<()> {
        let schema = self.schema();

        // Create an empty batch to bootstrap the table
        let ids = StringArray::from(Vec::<&str>::new());
        let source_paths = StringArray::from(Vec::<&str>::new());
        let contents = StringArray::from(Vec::<&str>::new());
        let roles = StringArray::from(Vec::<&str>::new());
        let session_ids = StringArray::from(Vec::<&str>::new());
        let timestamps = Int64Array::from(Vec::<i64>::new());

        let embeddings = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            Vec::<Option<Vec<Option<f32>>>>::new(),
            self.embedding_dim,
        );

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(ids),
                Arc::new(source_paths),
                Arc::new(contents),
                Arc::new(roles),
                Arc::new(session_ids),
                Arc::new(timestamps),
                Arc::new(embeddings),
            ],
        )
        .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))?;

        let reader: Box<dyn RecordBatchReader + Send> =
            Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema));

        self.conn
            .create_table(&self.table_name, reader)
            .mode(CreateTableMode::exist_ok(|req| req))
            .execute()
            .await
            .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    /// Get a handle to the table.
    async fn table(&self) -> Result<LanceTable> {
        self.conn
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))
    }

    fn chunks_to_batch(&self, chunks: &[EmbeddingChunk]) -> Result<RecordBatch> {
        let schema = self.schema();

        let ids = StringArray::from(chunks.iter().map(|c| c.id.as_str()).collect::<Vec<_>>());
        let source_paths = StringArray::from(chunks.iter().map(|c| c.source_path.as_str()).collect::<Vec<_>>());
        let contents = StringArray::from(chunks.iter().map(|c| c.content.as_str()).collect::<Vec<_>>());
        let roles = StringArray::from(chunks.iter().map(|c| c.role.as_str()).collect::<Vec<_>>());
        let session_ids = StringArray::from(chunks.iter().map(|c| c.session_id.as_str()).collect::<Vec<_>>());
        let timestamps = Int64Array::from(chunks.iter().map(|c| c.timestamp).collect::<Vec<_>>());

        let embedding_values: Vec<Option<Vec<Option<f32>>>> = chunks
            .iter()
            .map(|c| Some(c.embedding.iter().map(|v| Some(*v)).collect()))
            .collect();

        let embeddings = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            embedding_values,
            self.embedding_dim,
        );

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(ids),
                Arc::new(source_paths),
                Arc::new(contents),
                Arc::new(roles),
                Arc::new(session_ids),
                Arc::new(timestamps),
                Arc::new(embeddings),
            ],
        )
        .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))
    }

    /// Insert or update chunks in the vector store.
    pub async fn upsert_chunks(&self, chunks: &[EmbeddingChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        debug!("upserting {} chunks into {}", chunks.len(), self.table_name);

        let table = self.table().await?;
        let batch = self.chunks_to_batch(chunks)?;
        let schema = self.schema();
        let reader: Box<dyn RecordBatchReader + Send> =
            Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema));

        table
            .add(reader)
            .execute()
            .await
            .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    /// Delete all chunks from a given source path.
    pub async fn delete_by_source_path(&self, source_path: &str) -> Result<()> {
        debug!("deleting chunks for source_path={source_path}");
        let table = self.table().await?;

        table
            .delete(&format!("source_path = '{source_path}'"))
            .await
            .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    /// Perform a vector similarity search.
    pub async fn vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let table = self.table().await?;
        let query_vec: Vec<f64> = query_embedding.iter().map(|v| *v as f64).collect();

        let results = table
            .vector_search(query_vec)
            .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))?
            .limit(limit)
            .execute()
            .await
            .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))?;

        let mut search_results = Vec::new();
        for batch in &batches {
            let ids = batch.column_by_name("id").and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let source_paths = batch.column_by_name("source_path").and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let contents = batch.column_by_name("content").and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let roles = batch.column_by_name("role").and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let session_ids_col = batch.column_by_name("session_id").and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let timestamps = batch.column_by_name("timestamp").and_then(|c| c.as_any().downcast_ref::<Int64Array>());
            let distances = batch.column_by_name("_distance").and_then(|c| c.as_any().downcast_ref::<Float32Array>());

            let Some(ids) = ids else { continue };
            let Some(source_paths) = source_paths else { continue };
            let Some(contents) = contents else { continue };
            let Some(roles) = roles else { continue };
            let Some(session_ids_col) = session_ids_col else { continue };
            let Some(timestamps) = timestamps else { continue };

            for i in 0..batch.num_rows() {
                search_results.push(SearchResult {
                    chunk: EmbeddingChunk {
                        id: ids.value(i).to_string(),
                        source_path: source_paths.value(i).to_string(),
                        content: contents.value(i).to_string(),
                        role: roles.value(i).to_string(),
                        session_id: session_ids_col.value(i).to_string(),
                        timestamp: timestamps.value(i),
                        embedding: Vec::new(), // Don't return embeddings in search results
                    },
                    score: distances.map_or(0.0, |d| d.value(i)),
                });
            }
        }

        Ok(search_results)
    }

    /// Perform a keyword/filter search (no vector similarity).
    pub async fn keyword_search(
        &self,
        filter: &str,
        limit: usize,
    ) -> Result<Vec<EmbeddingChunk>> {
        let table = self.table().await?;

        let results = table
            .query()
            .only_if(filter)
            .limit(limit)
            .execute()
            .await
            .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))?;

        let batches: Vec<RecordBatch> = results
            .try_collect()
            .await
            .map_err(|e| LayersError::Io(std::io::Error::other(e.to_string())))?;

        let mut chunks = Vec::new();
        for batch in &batches {
            let ids = batch.column_by_name("id").and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let source_paths = batch.column_by_name("source_path").and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let contents = batch.column_by_name("content").and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let roles = batch.column_by_name("role").and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let session_ids_col = batch.column_by_name("session_id").and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let timestamps = batch.column_by_name("timestamp").and_then(|c| c.as_any().downcast_ref::<Int64Array>());

            let Some(ids) = ids else { continue };
            let Some(source_paths) = source_paths else { continue };
            let Some(contents) = contents else { continue };
            let Some(roles) = roles else { continue };
            let Some(session_ids_col) = session_ids_col else { continue };
            let Some(timestamps) = timestamps else { continue };

            for i in 0..batch.num_rows() {
                chunks.push(EmbeddingChunk {
                    id: ids.value(i).to_string(),
                    source_path: source_paths.value(i).to_string(),
                    content: contents.value(i).to_string(),
                    role: roles.value(i).to_string(),
                    session_id: session_ids_col.value(i).to_string(),
                    timestamp: timestamps.value(i),
                    embedding: Vec::new(),
                });
            }
        }

        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(id: &str, content: &str, session_id: &str, embedding: Vec<f32>) -> EmbeddingChunk {
        EmbeddingChunk {
            id: id.to_string(),
            source_path: format!("/test/{id}.md"),
            content: content.to_string(),
            role: "user".to_string(),
            session_id: session_id.to_string(),
            timestamp: 1700000000,
            embedding,
        }
    }

    fn cosine_embedding(dim: i32, value: f32) -> Vec<f32> {
        vec![value; dim as usize]
    }

    #[tokio::test]
    async fn open_creates_store_and_table() {
        let dir = tempfile::tempdir().unwrap();
        let store = LanceStore::open(dir.path(), "test_open").await.unwrap();
        // Verify we can get the table handle.
        assert!(store.table().await.is_ok());
    }

    #[tokio::test]
    async fn upsert_and_vector_search() {
        let dir = tempfile::tempdir().unwrap();
        let dim = 8;
        let store = LanceStore::open_with_dim(dir.path(), "test_vs", dim).await.unwrap();

        let chunk = make_chunk(
            "c1",
            "hello world",
            "sess-1",
            vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        store.upsert_chunks(&[chunk]).await.unwrap();

        // Search with the same vector should find it.
        let query = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let results = store.vector_search(&query, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.id, "c1");
        assert_eq!(results[0].chunk.content, "hello world");
    }

    #[tokio::test]
    async fn vector_search_returns_closest_match() {
        let dir = tempfile::tempdir().unwrap();
        let dim = 4;
        let store = LanceStore::open_with_dim(dir.path(), "test_nearest", dim).await.unwrap();

        let c1 = make_chunk("near", "near doc", "s1", vec![1.0, 0.0, 0.0, 0.0]);
        let c2 = make_chunk("far", "far doc", "s1", vec![0.0, 0.0, 0.0, 1.0]);
        store.upsert_chunks(&[c1, c2]).await.unwrap();

        let results = store.vector_search(&[1.0, 0.0, 0.0, 0.0], 1).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.id, "near");
    }

    #[tokio::test]
    async fn keyword_search_filters_by_field() {
        let dir = tempfile::tempdir().unwrap();
        let dim = 4;
        let store = LanceStore::open_with_dim(dir.path(), "test_kw", dim).await.unwrap();

        let c1 = make_chunk("k1", "alpha content", "s1", vec![0.0; 4]);
        let c2 = make_chunk("k2", "beta content", "s2", vec![0.0; 4]);
        store.upsert_chunks(&[c1, c2]).await.unwrap();

        let results = store.keyword_search("session_id = 's1'", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "k1");
    }

    #[tokio::test]
    async fn delete_by_source_path_removes_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let dim = 4;
        let store = LanceStore::open_with_dim(dir.path(), "test_del", dim).await.unwrap();

        let c1 = make_chunk("d1", "to delete", "s1", vec![1.0, 0.0, 0.0, 0.0]);
        store.upsert_chunks(&[c1]).await.unwrap();

        store.delete_by_source_path("/test/d1.md").await.unwrap();

        let results = store.vector_search(&[1.0, 0.0, 0.0, 0.0], 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn upsert_empty_chunks_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let store = LanceStore::open(dir.path(), "test_empty").await.unwrap();
        store.upsert_chunks(&[]).await.unwrap();
    }
}
