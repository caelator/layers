#![allow(clippy::doc_markdown)]
//! Storage backends: SQLite, LanceDB, JSONL, config store, and embedding pipeline.

pub mod config;
pub mod jsonl;
pub mod sqlite;

#[cfg(feature = "vector-store")]
pub mod embedding_pipeline;
#[cfg(feature = "vector-store")]
pub mod lancedb_store;
