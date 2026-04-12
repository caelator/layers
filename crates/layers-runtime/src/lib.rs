#![allow(clippy::doc_markdown)]
//! Runtime engine: agent loop, context management, sessions, and queue.

pub mod session;
pub mod agent_loop;
pub mod context;
pub mod system_prompt;
pub mod queue;
pub mod tool_dispatch;
pub mod streaming;
pub mod failover;
pub mod subagent;
