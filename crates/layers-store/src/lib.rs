#![allow(clippy::doc_markdown)]
//! Storage backends: SQLite, LanceDB, JSONL, config store, and embedding pipeline.

pub mod config;
pub mod embedding_pipeline;
pub mod jsonl;
pub mod lancedb_store;
pub mod sqlite;
