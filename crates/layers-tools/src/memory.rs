//! Memory tools: hybrid vector+keyword search and chunk retrieval.

use std::sync::Arc;

use serde::Deserialize;
use tracing::debug;

use layers_core::{LayersError, Result, Tool, ToolContext, ToolOutput};
use layers_store::embedding_pipeline::RealEmbeddingPipeline;

// ---------------------------------------------------------------------------
// Memory search
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MemorySearchParams {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    session_id: Option<String>,
}

/// Hybrid vector+keyword search over memory using Reciprocal Rank Fusion.
pub struct MemorySearchTool {
    pipeline: Option<Arc<RealEmbeddingPipeline>>,
}

impl MemorySearchTool {
    #[must_use]
    pub fn new() -> Self {
        Self { pipeline: None }
    }

    /// Create with a configured embedding pipeline for real search.
    #[must_use]
    pub fn with_pipeline(pipeline: Arc<RealEmbeddingPipeline>) -> Self {
        Self {
            pipeline: Some(pipeline),
        }
    }
}

impl Default for MemorySearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Search memory using hybrid vector+keyword search with Reciprocal Rank Fusion. \
         Returns relevant memory chunks ranked by combined relevance."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (natural language)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 10)"
                },
                "session_id": {
                    "type": "string",
                    "description": "Filter to a specific session's memory"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value, _context: ToolContext) -> Result<ToolOutput> {
        let params: MemorySearchParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid memory_search params: {e}")))?;

        let limit = params.limit.unwrap_or(10);
        debug!(query = %params.query, limit, "memory search");

        let Some(pipeline) = &self.pipeline else {
            return Ok(ToolOutput {
                content: serde_json::json!({
                    "query": params.query,
                    "results": [],
                    "total": 0,
                    "note": "memory search requires configured vector store"
                })
                .to_string(),
                attachments: Vec::new(),
                structured_content: None,
                is_error: None,
            });
        };

        let results = pipeline
            .search_text(&params.query, limit)
            .await
            .map_err(|e| LayersError::Tool(format!("memory search failed: {e}")))?;

        let result_json: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.chunk.id,
                    "source_path": r.chunk.source_path,
                    "content": r.chunk.content,
                    "role": r.chunk.role,
                    "session_id": r.chunk.session_id,
                    "score": r.score,
                })
            })
            .collect();

        let total = result_json.len();
        Ok(ToolOutput {
            content: serde_json::json!({
                "query": params.query,
                "results": result_json,
                "total": total,
            })
            .to_string(),
            attachments: Vec::new(),
            structured_content: None,
            is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Memory get
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MemoryGetParams {
    path: String,
}

/// Get a specific memory chunk by path.
pub struct MemoryGetTool {
    pipeline: Option<Arc<RealEmbeddingPipeline>>,
}

impl MemoryGetTool {
    #[must_use]
    pub fn new() -> Self {
        Self { pipeline: None }
    }

    /// Create with a configured embedding pipeline for real retrieval.
    #[must_use]
    pub fn with_pipeline(pipeline: Arc<RealEmbeddingPipeline>) -> Self {
        Self {
            pipeline: Some(pipeline),
        }
    }
}

impl Default for MemoryGetTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }

    fn description(&self) -> &str {
        "Get a specific memory chunk by its path identifier."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path identifier of the memory chunk"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: serde_json::Value, _context: ToolContext) -> Result<ToolOutput> {
        let params: MemoryGetParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid memory_get params: {e}")))?;

        debug!(path = %params.path, "memory get");

        let Some(pipeline) = &self.pipeline else {
            return Ok(ToolOutput {
                content: serde_json::json!({
                    "path": params.path,
                    "content": null,
                    "note": "memory get requires configured memory store"
                })
                .to_string(),
                attachments: Vec::new(),
                structured_content: None,
                is_error: None,
            });
        };

        let results = pipeline
            .search_text(&params.path, 1)
            .await
            .map_err(|e| LayersError::Tool(format!("memory get failed: {e}")))?;

        if let Some(result) = results.first() {
            Ok(ToolOutput {
                content: serde_json::json!({
                    "path": result.chunk.source_path,
                    "content": result.chunk.content,
                    "role": result.chunk.role,
                    "session_id": result.chunk.session_id,
                })
                .to_string(),
                attachments: Vec::new(),
                structured_content: None,
                is_error: None,
            })
        } else {
            Ok(ToolOutput {
                content: serde_json::json!({
                    "path": params.path,
                    "content": null,
                    "note": "chunk not found"
                })
                .to_string(),
                attachments: Vec::new(),
                structured_content: None,
                is_error: None,
            })
        }
    }
}
