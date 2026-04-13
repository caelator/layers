//! Filesystem tools: read, write, and edit files.

use serde::Deserialize;
use tracing::debug;

use layers_core::{LayersError, Result, Tool, ToolContext, ToolOutput};

// ---------------------------------------------------------------------------
// Read tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ReadParams {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

/// Read file contents with optional offset/limit. Truncates at 2000 lines or 50 KB.
pub struct ReadTool;

impl ReadTool {
    const MAX_LINES: usize = 2000;
    const MAX_BYTES: usize = 50 * 1024;

    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a file from the filesystem. Supports offset/limit for partial reads. \
         Truncates at 2000 lines or 50KB by default."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (0-based)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: ReadParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid read params: {e}")))?;

        debug!(path = %params.path, "reading file");

        let content = tokio::fs::read_to_string(&params.path)
            .await
            .map_err(|e| LayersError::Tool(format!("failed to read {}: {e}", params.path)))?;

        // Apply offset and limit.
        let lines: Vec<&str> = content.lines().collect();
        let offset = params.offset.unwrap_or(0);
        let limit = params.limit.unwrap_or(Self::MAX_LINES).min(Self::MAX_LINES);

        let selected: Vec<&str> = lines
            .iter()
            .skip(offset)
            .take(limit)
            .copied()
            .collect();

        let mut result = String::new();
        for (i, line) in selected.iter().enumerate() {
            let line_num = offset + i + 1;
            result.push_str(&format!("{line_num}\t{line}\n"));
        }

        // Truncate to max bytes.
        if result.len() > Self::MAX_BYTES {
            result.truncate(Self::MAX_BYTES);
            result.push_str("\n... (truncated at 50KB)");
        }

        let total_lines = lines.len();
        let shown = selected.len();

        let output = if total_lines > shown {
            format!(
                "{result}\n(showing lines {}-{} of {total_lines})",
                offset + 1,
                offset + shown
            )
        } else {
            result
        };

        Ok(ToolOutput {
            content: output,
            attachments: Vec::new(),
            structured_content: None,
                is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Write tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WriteParams {
    path: String,
    content: String,
}

/// Write content to a file, creating parent directories as needed.
pub struct WriteTool;

impl WriteTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for WriteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if they don't exist. \
         Overwrites the file if it already exists."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: WriteParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid write params: {e}")))?;

        debug!(path = %params.path, "writing file");

        // Create parent directories.
        if let Some(parent) = std::path::Path::new(&params.path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| LayersError::Tool(format!("failed to create dirs: {e}")))?;
        }

        tokio::fs::write(&params.path, &params.content)
            .await
            .map_err(|e| LayersError::Tool(format!("failed to write {}: {e}", params.path)))?;

        let bytes = params.content.len();
        Ok(ToolOutput {
            content: format!("Wrote {bytes} bytes to {}", params.path),
            attachments: Vec::new(),
            structured_content: None,
                is_error: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Edit tool
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct EditParams {
    path: String,
    edits: Vec<EditOp>,
}

#[derive(Debug, Deserialize)]
struct EditOp {
    old_text: String,
    new_text: String,
}

/// Exact text replacement in a file. Applies multiple non-overlapping edits.
pub struct EditTool;

impl EditTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Perform exact text replacements in a file. Each edit specifies old_text to find \
         and new_text to replace it with. Edits must not overlap."
    }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to edit"
                },
                "edits": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_text": {
                                "type": "string",
                                "description": "Exact text to find"
                            },
                            "new_text": {
                                "type": "string",
                                "description": "Text to replace it with"
                            }
                        },
                        "required": ["old_text", "new_text"]
                    },
                    "description": "List of text replacements to apply"
                }
            },
            "required": ["path", "edits"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _context: ToolContext,
    ) -> Result<ToolOutput> {
        let params: EditParams = serde_json::from_value(args)
            .map_err(|e| LayersError::Tool(format!("invalid edit params: {e}")))?;

        debug!(path = %params.path, edits = params.edits.len(), "editing file");

        let mut content = tokio::fs::read_to_string(&params.path)
            .await
            .map_err(|e| LayersError::Tool(format!("failed to read {}: {e}", params.path)))?;

        let mut applied = 0;
        let mut errors = Vec::new();

        for (i, edit) in params.edits.iter().enumerate() {
            if edit.old_text == edit.new_text {
                continue;
            }
            match content.find(&edit.old_text) {
                Some(pos) => {
                    // Check for uniqueness — only replace if exactly one occurrence.
                    let count = content.matches(&edit.old_text).count();
                    if count > 1 {
                        errors.push(format!(
                            "edit {i}: old_text found {count} times, must be unique"
                        ));
                        continue;
                    }
                    content = format!(
                        "{}{}{}",
                        &content[..pos],
                        edit.new_text,
                        &content[pos + edit.old_text.len()..]
                    );
                    applied += 1;
                }
                None => {
                    errors.push(format!("edit {i}: old_text not found in file"));
                }
            }
        }

        if applied > 0 {
            tokio::fs::write(&params.path, &content)
                .await
                .map_err(|e| LayersError::Tool(format!("failed to write {}: {e}", params.path)))?;
        }

        let total = params.edits.len();
        let msg = if errors.is_empty() {
            format!("Applied {applied}/{total} edits to {}", params.path)
        } else {
            format!(
                "Applied {applied}/{total} edits to {}. Errors: {}",
                params.path,
                errors.join("; ")
            )
        };

        Ok(ToolOutput {
            content: msg,
            structured_content: None,
            attachments: Vec::new(),
            is_error: if errors.is_empty() { None } else { Some(true) },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layers_core::{Tool, ToolContext};
    use std::path::PathBuf;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: "test".into(),
            agent_id: "test".into(),
            channel: None,
            metadata: Default::default(),
        }
    }

    async fn write_test_file(dir: &tempfile::TempDir, name: &str, content: &str) -> PathBuf {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(&path, content).await.unwrap();
        path
    }

    #[tokio::test]
    async fn read_file_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_test_file(&dir, "test.txt", "hello\nworld").await;

        let tool = ReadTool::new();
        let result = tool
            .execute(
                serde_json::json!({ "path": path.to_str().unwrap() }),
                test_ctx(),
            )
            .await
            .unwrap();
        assert!(!result.is_error.unwrap_or(false));
        assert!(result.content.contains("hello"));
        assert!(result.content.contains("world"));
    }

    #[tokio::test]
    async fn read_file_with_offset_limit() {
        let dir = tempfile::tempdir().unwrap();
        let content: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        let path = write_test_file(&dir, "test.txt", &content.join("\n")).await;

        let tool = ReadTool::new();
        let result = tool
            .execute(
                serde_json::json!({ "path": path.to_str().unwrap(), "offset": 2, "limit": 3 }),
                test_ctx(),
            )
            .await
            .unwrap();
        assert!(result.content.contains("line 3"));
        assert!(result.content.contains("line 5"));
        assert!(!result.content.contains("line 1"));
        assert!(!result.content.contains("line 6"));
    }

    #[tokio::test]
    async fn read_missing_file_errors() {
        let tool = ReadTool::new();
        let result = tool
            .execute(
                serde_json::json!({ "path": "/nonexistent/path/file.txt" }),
                test_ctx(),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn write_file_creates_parents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/c/test.txt");

        let tool = WriteTool::new();
        let result = tool
            .execute(
                serde_json::json!({ "path": path.to_str().unwrap(), "content": "hello" }),
                test_ctx(),
            )
            .await
            .unwrap();
        assert!(!result.is_error.unwrap_or(false));
        assert!(tokio::fs::read_to_string(&path).await.unwrap() == "hello");
    }

    #[tokio::test]
    async fn write_file_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_test_file(&dir, "test.txt", "old").await;

        let tool = WriteTool::new();
        tool.execute(
            serde_json::json!({ "path": path.to_str().unwrap(), "content": "new" }),
            test_ctx(),
        )
        .await
        .unwrap();

        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "new");
    }

    #[tokio::test]
    async fn edit_file_single_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_test_file(&dir, "test.txt", "hello world").await;

        let tool = EditTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "path": path.to_str().unwrap(),
                    "edits": [{ "old_text": "world", "new_text": "rust" }]
                }),
                test_ctx(),
            )
            .await
            .unwrap();
        assert!(!result.is_error.unwrap_or(false));
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "hello rust");
    }

    #[tokio::test]
    async fn edit_file_multiple_non_overlapping() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_test_file(&dir, "test.txt", "aaa bbb ccc").await;

        let tool = EditTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "path": path.to_str().unwrap(),
                    "edits": [
                        { "old_text": "aaa", "new_text": "xxx" },
                        { "old_text": "ccc", "new_text": "zzz" }
                    ]
                }),
                test_ctx(),
            )
            .await
            .unwrap();
        assert!(!result.is_error.unwrap_or(false));
        assert_eq!(
            tokio::fs::read_to_string(&path).await.unwrap(),
            "xxx bbb zzz"
        );
    }

    #[tokio::test]
    async fn edit_file_text_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_test_file(&dir, "test.txt", "hello").await;

        let tool = EditTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "path": path.to_str().unwrap(),
                    "edits": [{ "old_text": "missing", "new_text": "x" }]
                }),
                test_ctx(),
            )
            .await
            .unwrap();
        assert!(result.is_error.unwrap_or(false));
        // Original unchanged
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "hello");
    }

    #[tokio::test]
    async fn edit_file_ambiguous_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_test_file(&dir, "test.txt", "aaa aaa").await;

        let tool = EditTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "path": path.to_str().unwrap(),
                    "edits": [{ "old_text": "aaa", "new_text": "bbb" }]
                }),
                test_ctx(),
            )
            .await
            .unwrap();
        assert!(result.is_error.unwrap_or(false));
        assert!(result.content.contains("found 2 times"));
    }
}
