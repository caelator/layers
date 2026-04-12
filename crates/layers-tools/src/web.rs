//! Web tools: search and fetch.

use serde::Deserialize;
use tracing::debug;

use layers_core::{LayersError, Result, Tool, ToolContext, ToolOutput};

// ---------------------------------------------------------------------------
// Web search tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WebSearchParams {
    query: String,
    #[serde(default)]
    max_results: Option<usize>,
}

/// Search the web and return titles, URLs, and snippets.
pub struct WebSearchTool;

impl WebSearchTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for a query. Returns a list of results with titles, URLs, and snippets."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 10)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: WebSearchParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid web_search params: {e}")))?;

        let max = params.max_results.unwrap_or(10);
        debug!(query = %params.query, max, "web search");

        // Stub: actual web search requires an external API integration.
        Ok(ToolOutput {
            content: serde_json::json!({
                "query": params.query,
                "results": [],
                "note": "web search requires external API configuration"
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Web fetch tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WebFetchParams {
    url: String,
    #[serde(default)]
    mode: Option<String>,
}

/// Fetch a URL and return its content as markdown or text.
pub struct WebFetchTool;

impl WebFetchTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a URL and return its content. Supports markdown and text extraction modes."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "mode": {
                    "type": "string",
                    "enum": ["markdown", "text", "raw"],
                    "description": "Content extraction mode (default: markdown)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: WebFetchParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid web_fetch params: {e}")))?;

        let mode = params.mode.as_deref().unwrap_or("markdown");
        debug!(url = %params.url, mode, "web fetch");

        // Stub: actual HTTP fetching requires an HTTP client.
        Ok(ToolOutput {
            content: serde_json::json!({
                "url": params.url,
                "mode": mode,
                "content": "",
                "note": "web fetch requires HTTP client configuration"
            })
            .to_string(),
            attachments: Vec::new(),
            is_error: None,
        })
    }
}
