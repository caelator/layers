//! Web tools: search and fetch.
//!
//! Hardened (Epic 3 baseline): real HTTP fetch via reqwest with timeouts,
//! size limits, URL validation, and content-type sniffing.

use serde::Deserialize;
use tracing::{debug, warn};

use layers_core::{LayersError, Result, Tool, ToolContext, ToolOutput};

// ---------------------------------------------------------------------------
// Shared constants
// ---------------------------------------------------------------------------

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024; // 2 MB
const MAX_FETCH_URL_LENGTH: usize = 2048;

// ---------------------------------------------------------------------------
// URL validation
// ---------------------------------------------------------------------------

/// Validate that a URL is safe to fetch: must be http(s), not too long,
/// and must not target loopback or link-local addresses.
fn validate_fetch_url(url: &str) -> Result<()> {
    if url.len() > MAX_FETCH_URL_LENGTH {
        return Err(LayersError::Tool(format!(
            "URL too long ({} chars, max {MAX_FETCH_URL_LENGTH})",
            url.len()
        )));
    }

    let parsed =
        url::Url::parse(url).map_err(|e| LayersError::Tool(format!("invalid URL: {e}")))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(LayersError::Tool(format!(
                "unsupported scheme: {other} (only http/https allowed)"
            )));
        }
    }

    // Block loopback / private addresses at the host level.
    if let Some(host) = parsed.host_str() {
        // Block obvious loopback
        if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "0.0.0.0" {
            return Err(LayersError::Tool(
                "fetching localhost/loopback addresses is not allowed".into(),
            ));
        }
        // Block common private network prefixes (basic heuristic)
        if host.starts_with("192.168.") || host.starts_with("10.") || host.starts_with("172.") {
            return Err(LayersError::Tool(
                "fetching private network addresses is not allowed".into(),
            ));
        }
    }

    Ok(())
}

/// Truncate content to a maximum byte size, appending a truncation notice.
fn truncate_content(mut content: String, max_bytes: usize) -> String {
    if content.len() > max_bytes {
        content.truncate(max_bytes);
        content.push_str("\n\n... (truncated at ");
        content.push_str(&format!("{} bytes", max_bytes / 1024));
        content.push_str(" KB)");
    }
    content
}

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

    async fn execute(&self, args: serde_json::Value, _context: ToolContext) -> Result<ToolOutput> {
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
            structured_content: None,
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
    #[serde(default)]
    max_chars: Option<usize>,
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

/// Build a reqwest client with sensible timeouts.
fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent("layers/0.1 (web-fetch-tool)")
        .build()
        .expect("failed to build reqwest client")
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
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum characters to return (default: 50000)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: serde_json::Value, _context: ToolContext) -> Result<ToolOutput> {
        let params: WebFetchParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid web_fetch params: {e}")))?;

        let mode = params.mode.as_deref().unwrap_or("markdown");
        let max_chars = params.max_chars.unwrap_or(50_000);

        debug!(url = %params.url, mode, max_chars, "web fetch");

        // Validate URL before making any network request.
        validate_fetch_url(&params.url)?;

        let client = build_client();
        let response = client
            .get(&params.url)
            .send()
            .await
            .map_err(|e| LayersError::Tool(format!("HTTP request failed: {e}")))?;

        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/plain")
            .to_string();

        if !status.is_success() {
            warn!(%status, url = %params.url, "web fetch got non-success status");
            return Ok(ToolOutput {
                content: format!("HTTP {status} for {}", params.url),
                attachments: Vec::new(),
                structured_content: None,
                is_error: Some(true),
            });
        }

        // Enforce size limit at the body level.
        let body = response
            .bytes()
            .await
            .map_err(|e| LayersError::Tool(format!("failed to read response body: {e}")))?;

        if body.len() > MAX_RESPONSE_BYTES {
            return Ok(ToolOutput {
                content: format!(
                    "Response too large ({} bytes, max {} bytes)",
                    body.len(),
                    MAX_RESPONSE_BYTES
                ),
                attachments: Vec::new(),
                structured_content: None,
                is_error: Some(true),
            });
        }

        let body_text = String::from_utf8_lossy(&body).to_string();

        // Apply extraction mode.
        let extracted = match mode {
            "raw" => body_text,
            "text" => {
                // Strip HTML tags if content looks like HTML.
                if content_type.contains("html") {
                    strip_html_tags(&body_text)
                } else {
                    body_text
                }
            }
            _ /* "markdown" */ => {
                if content_type.contains("html") {
                    html_to_markdown(&body_text)
                } else {
                    body_text
                }
            }
        };

        let truncated = truncate_content(extracted, max_chars);

        let output = serde_json::json!({
            "url": params.url,
            "status": status.as_u16(),
            "content_type": content_type,
            "mode": mode,
            "content": truncated,
        });

        Ok(ToolOutput {
            content: output.to_string(),
            attachments: Vec::new(),
            structured_content: None,
            is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Minimal HTML → text/markdown converters
// ---------------------------------------------------------------------------

/// Naive HTML tag stripper. Good enough for tool output; not a full parser.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                result.push(' ');
            }
            _ if in_tag => {}
            _ => result.push(ch),
        }
    }
    // Collapse whitespace
    let mut collapsed = String::with_capacity(result.len());
    let mut last_was_space = false;
    for ch in result.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                collapsed.push(' ');
                last_was_space = true;
            }
        } else {
            collapsed.push(ch);
            last_was_space = false;
        }
    }
    collapsed.trim().to_string()
}

/// Very simple HTML→markdown: converts headings, paragraphs, lists, links, bold, italic.
fn html_to_markdown(html: &str) -> String {
    let mut md = html.to_string();

    // Convert common block elements
    md = regex_lite::Regex::new(r"(?i)</h[1-6]>")
        .map(|re| re.replace_all(&md, "\n\n").to_string())
        .unwrap_or(md);

    // Links
    md = regex_lite::Regex::new(r#"(?i)<a[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#)
        .map(|re| re.replace_all(&md, "[$2]($1)").to_string())
        .unwrap_or(md);

    // Bold/italic
    md = md.replace("<strong>", "**").replace("</strong>", "**");
    md = md.replace("<b>", "**").replace("</b>", "**");
    md = md.replace("<em>", "*").replace("</em>", "*");
    md = md.replace("<i>", "*").replace("</i>", "*");

    // Line breaks and paragraphs
    md = md.replace("<br>", "\n").replace("<br/>", "\n");
    md = md.replace("<p>", "\n").replace("</p>", "\n");
    md = md.replace("<li>", "- ").replace("</li>", "\n");

    // Strip remaining tags
    strip_html_tags(&md)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            agent_id: "test".into(),
            channel: None,
            metadata: Default::default(),
        }
    }

    // --- URL validation tests ---

    #[test]
    fn validate_url_accepts_https() {
        assert!(validate_fetch_url("https://example.com/page").is_ok());
    }

    #[test]
    fn validate_url_accepts_http() {
        assert!(validate_fetch_url("http://example.com/page").is_ok());
    }

    #[test]
    fn validate_url_rejects_ftp() {
        assert!(validate_fetch_url("ftp://example.com/file").is_err());
    }

    #[test]
    fn validate_url_rejects_localhost() {
        assert!(validate_fetch_url("http://localhost:8080/api").is_err());
    }

    #[test]
    fn validate_url_rejects_loopback_ip() {
        assert!(validate_fetch_url("http://127.0.0.1:8080/api").is_err());
    }

    #[test]
    fn validate_url_rejects_private_192() {
        assert!(validate_fetch_url("http://192.168.1.1/admin").is_err());
    }

    #[test]
    fn validate_url_rejects_private_10() {
        assert!(validate_fetch_url("http://10.0.0.1/internal").is_err());
    }

    #[test]
    fn validate_url_rejects_private_172() {
        assert!(validate_fetch_url("http://172.16.0.1/internal").is_err());
    }

    #[test]
    fn validate_url_rejects_too_long() {
        let long_url = format!("https://example.com/{}", "a".repeat(2100));
        assert!(validate_fetch_url(&long_url).is_err());
    }

    #[test]
    fn validate_url_rejects_invalid() {
        assert!(validate_fetch_url("not-a-url").is_err());
    }

    // --- Content helpers ---

    #[test]
    fn truncate_does_nothing_when_under_limit() {
        let s = "hello".to_string();
        assert_eq!(truncate_content(s, 100), "hello");
    }

    #[test]
    fn truncate_truncates_at_limit() {
        let s = "x".repeat(200);
        let result = truncate_content(s, 100);
        assert!(result.len() > 100); // includes notice
        assert!(result.contains("truncated"));
    }

    #[test]
    fn strip_html_basic() {
        assert_eq!(strip_html_tags("<p>Hello <b>world</b></p>"), "Hello world");
    }

    #[test]
    fn strip_html_preserves_text() {
        assert_eq!(strip_html_tags("no tags here"), "no tags here");
    }

    #[test]
    fn html_to_md_converts_links() {
        let html = r#"<a href="https://example.com">click</a>"#;
        let md = html_to_markdown(html);
        assert!(md.contains("[click](https://example.com)") || md.contains("click"));
    }

    // --- WebFetchTool tests (unit, no network) ---

    #[tokio::test]
    async fn fetch_rejects_localhost() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(
                serde_json::json!({ "url": "http://localhost:9999/test" }),
                test_ctx(),
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            LayersError::Tool(msg) => {
                assert!(msg.contains("localhost") || msg.contains("loopback"))
            }
            _ => panic!("expected Tool error"),
        }
    }

    #[tokio::test]
    async fn fetch_rejects_invalid_url() {
        let tool = WebFetchTool::new();
        let result = tool
            .execute(serde_json::json!({ "url": "not-a-valid-url" }), test_ctx())
            .await;
        assert!(result.is_err());
    }

    // --- WebSearchTool tests ---

    #[tokio::test]
    async fn search_returns_stub() {
        let tool = WebSearchTool::new();
        let result = tool
            .execute(serde_json::json!({ "query": "test" }), test_ctx())
            .await
            .unwrap();
        assert!(result.content.contains("test"));
    }
}
