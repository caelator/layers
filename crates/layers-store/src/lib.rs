#![allow(clippy::doc_markdown)]
//! Storage backends: SQLite, LanceDB, JSONL, and config store.

pub mod config;
pub mod jsonl;
pub mod lancedb_store;
pub mod sqlite;
