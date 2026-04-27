#![allow(clippy::doc_markdown)]
//! Runtime engine: agent loop, context management, sessions, and queue.

pub mod actor;
pub mod agent_loop;
pub mod brain;
pub mod context;
pub mod engine;
pub mod failover;
pub mod queue;
pub mod session;
pub mod streaming;
pub mod subagent;
pub mod system_prompt;
pub mod tool_dispatch;
