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
            attachments: Vec::new(),
            is_error: if errors.is_empty() { None } else { Some(true) },
        })
    }
}
