//! System prompt construction from fixed sections, bootstrap files, and dynamic context.

use std::path::PathBuf;

use tracing::warn;

use layers_core::Session;
use crate::tool_dispatch::ToolRegistry;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum characters per bootstrap file.
const MAX_FILE_CHARS: usize = 20_000;
/// Maximum total system prompt characters.
const MAX_TOTAL_CHARS: usize = 150_000;

/// Bootstrap files loaded from the workspace, in order.
const BOOTSTRAP_FILES: &[&str] = &[
    "AGENTS.md",
    "SOUL.md",
    "IDENTITY.md",
    "USER.md",
    "TOOLS.md",
    "HEARTBEAT.md",
    "MEMORY.md",
];

/// Reduced set for sub-agent mode.
const SUBAGENT_FILES: &[&str] = &["AGENTS.md", "TOOLS.md"];

// ---------------------------------------------------------------------------
// SystemPromptBuilder
// ---------------------------------------------------------------------------

/// Builds the full system prompt from fixed sections, workspace files, and dynamic context.
pub struct SystemPromptBuilder {
    /// Workspace root directory for loading bootstrap files.
    workspace: Option<PathBuf>,
    /// Fixed preamble sections (safety, tooling rules, etc.).
    fixed_sections: Vec<PromptSection>,
    /// Whether this builder is for a sub-agent (reduced prompt).
    is_subagent: bool,
    /// Extra dynamic sections injected at build time.
    extra_sections: Vec<PromptSection>,
}

/// A named section of the system prompt.
#[derive(Debug, Clone)]
pub struct PromptSection {
    pub name: String,
    pub content: String,
}

impl SystemPromptBuilder {
    pub fn new(workspace: Option<PathBuf>) -> Self {
        Self {
            workspace,
            fixed_sections: default_fixed_sections(),
            is_subagent: false,
            extra_sections: Vec::new(),
        }
    }

    pub fn subagent(workspace: Option<PathBuf>) -> Self {
        Self {
            workspace,
            fixed_sections: Vec::new(), // Sub-agents get minimal fixed sections.
            is_subagent: true,
            extra_sections: Vec::new(),
        }
    }

    pub fn add_section(&mut self, name: impl Into<String>, content: impl Into<String>) {
        self.extra_sections.push(PromptSection {
            name: name.into(),
            content: content.into(),
        });
    }

    /// Build the full system prompt string.
    pub fn build(&self, session: &Session, tools: &ToolRegistry) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut total_chars: usize = 0;

        // 1. Fixed sections.
        for section in &self.fixed_sections {
            let addition = format!("## {}\n\n{}\n\n", section.name, section.content);
            if total_chars + addition.len() > MAX_TOTAL_CHARS {
                break;
            }
            total_chars += addition.len();
            parts.push(addition);
        }

        // 2. Runtime metadata.
        let meta = format!(
            "## Runtime\n\n- Session: {}\n- Agent: {}\n- Model: {}\n\n",
            session.id,
            session.agent_id,
            session.model.as_deref().unwrap_or("default"),
        );
        if total_chars + meta.len() <= MAX_TOTAL_CHARS {
            total_chars += meta.len();
            parts.push(meta);
        }

        // 3. Available tools listing.
        let tool_names: Vec<&str> = tools.names().into_iter().collect();
        if !tool_names.is_empty() {
            let tools_section = format!(
                "## Available Tools\n\n{}\n\n",
                tool_names.join(", ")
            );
            if total_chars + tools_section.len() <= MAX_TOTAL_CHARS {
                total_chars += tools_section.len();
                parts.push(tools_section);
            }
        }

        // 4. Workspace bootstrap files.
        let files = if self.is_subagent {
            SUBAGENT_FILES
        } else {
            BOOTSTRAP_FILES
        };

        if let Some(ref workspace) = self.workspace {
            for filename in files {
                // MEMORY.md only in main session (not shared/group contexts).
                if *filename == "MEMORY.md" && self.is_subagent {
                    continue;
                }

                let path = workspace.join(filename);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let truncated = if content.len() > MAX_FILE_CHARS {
                        format!(
                            "{}...\n[truncated at {} chars]",
                            &content[..MAX_FILE_CHARS],
                            content.len()
                        )
                    } else {
                        content
                    };

                    let section = format!("## {}\n\n{}\n\n", filename, truncated);
                    if total_chars + section.len() > MAX_TOTAL_CHARS {
                        warn!(
                            file = filename,
                            "system prompt total char limit reached, skipping remaining files"
                        );
                        break;
                    }
                    total_chars += section.len();
                    parts.push(section);
                }
            }
        }

        // 5. Extra dynamic sections.
        for section in &self.extra_sections {
            let addition = format!("## {}\n\n{}\n\n", section.name, section.content);
            if total_chars + addition.len() > MAX_TOTAL_CHARS {
                break;
            }
            total_chars += addition.len();
            parts.push(addition);
        }

        parts.concat()
    }
}

/// Default fixed sections (safety, workspace, tooling rules).
fn default_fixed_sections() -> Vec<PromptSection> {
    vec![
        PromptSection {
            name: "Safety".into(),
            content: "You are a helpful AI assistant. Follow all safety guidelines. \
                      Do not produce harmful content. Respect user boundaries."
                .into(),
        },
        PromptSection {
            name: "Tooling Rules".into(),
            content: "When using tools, always check the result before proceeding. \
                      Report errors clearly. Do not retry failed tool calls without adjusting parameters."
                .into(),
        },
    ]
}
