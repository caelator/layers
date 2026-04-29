//! `layers chat` — interactive REPL-style chat surface.
//!
//! Provides a simple stdin/stdout chat loop that can be used for quick
//! one-shot queries or multi-turn conversations using the Layers runtime.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use layers_runtime::tool_dispatch::ToolCapabilityPolicy;
use layers_tools::bootstrap::{runtime_policy_from_cli, runtime_registry};

/// Arguments for the `layers chat` command.
pub struct ChatArgs {
    /// Optional system prompt override.
    pub system_prompt: Option<String>,
    /// Optional model override (e.g. "openai/gpt-4").
    pub model: Option<String>,
    /// Runtime-backed tool profile baseline, either a built-in profile name or a named config profile.
    pub tool_profile: String,
    /// Named tool profiles loaded from config.
    pub named_tool_profiles: HashMap<String, Vec<String>>,
    /// Explicit per-tool allow-list, applied after profile selection.
    pub allow_tools: Vec<String>,
    /// Explicit per-tool deny-list, applied last.
    pub deny_tools: Vec<String>,
    /// Maximum turns before exiting (0 = unlimited).
    pub max_turns: usize,
    /// Output as JSON.
    pub json: bool,
}

impl ChatArgs {
    fn tool_policy(&self) -> anyhow::Result<ToolCapabilityPolicy> {
        runtime_policy_from_cli(
            &self.tool_profile,
            &self.named_tool_profiles,
            &self.allow_tools,
            &self.deny_tools,
        )
        .map_err(anyhow::Error::from)
    }

    fn runtime_tool_names(&self) -> anyhow::Result<Vec<String>> {
        let registry = runtime_registry(self.tool_policy()?);
        let mut names = registry
            .names()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }

    fn resolved_profile_label(&self) -> &str {
        self.tool_profile.trim()
    }
}

/// Run the interactive chat loop.
///
/// Reads lines from stdin, processes each as a query through the Layers
/// routing pipeline, and prints the assembled context/response.
pub fn handle_chat(args: &ChatArgs) -> anyhow::Result<()> {
    let workspace = crate::config::workspace_root();
    let runtime_tools = args.runtime_tool_names()?;

    println!("layers chat — type your query (Ctrl-D or 'exit' to quit)");
    println!("workspace: {}", workspace.display());
    if let Some(ref model) = args.model {
        println!("model override: {model}");
    }
    if let Some(ref prompt) = args.system_prompt {
        println!("system prompt: {prompt}");
    }
    println!("runtime tool profile: {}", args.resolved_profile_label());
    println!(
        "runtime-backed tools: {}",
        if runtime_tools.is_empty() {
            "(none)".to_string()
        } else {
            runtime_tools.join(", ")
        }
    );
    println!();

    let mut turn = 0usize;
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        if args.max_turns > 0 && turn >= args.max_turns {
            break;
        }

        print!("layers> ");
        stdout.flush()?;

        let mut line = String::new();
        let bytes = stdin.lock().read_line(&mut line)?;
        if bytes == 0 {
            // EOF
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input == "exit" || input == "quit" || input == ":q" {
            break;
        }

        turn += 1;

        // Delegate to the existing query pipeline.
        // Note: system_prompt and model overrides are accepted but not yet
        // wired into the query pipeline. They will be used once the runtime
        // integration lands (Epic 1).
        let _ = (&args.system_prompt, &args.model, args.tool_policy()?);
        match crate::cmd::query::handle_query(input, args.json, false, false, 1) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error: {e:#}");
            }
        }

        println!();
    }

    if turn > 0 {
        println!("— {turn} turn(s) completed —");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::ChatArgs;

    fn args(profile: &str) -> ChatArgs {
        ChatArgs {
            system_prompt: None,
            model: None,
            tool_profile: profile.to_string(),
            named_tool_profiles: HashMap::new(),
            allow_tools: Vec::new(),
            deny_tools: Vec::new(),
            max_turns: 0,
            json: false,
        }
    }

    #[test]
    fn chat_tool_policy_uses_selected_builtin_profile() {
        let args = args("coding");
        let names = args.runtime_tool_names().expect("runtime tool names");

        assert_eq!(names, vec!["read".to_string(), "write".to_string()]);
    }

    #[test]
    fn chat_tool_policy_supports_named_config_profiles() {
        let mut args = args("fs-readonly");
        args.named_tool_profiles
            .insert("fs-readonly".to_string(), vec!["read".to_string()]);

        assert_eq!(
            args.runtime_tool_names().expect("runtime tool names"),
            vec!["read".to_string()]
        );
    }

    #[test]
    fn chat_tool_policy_honors_allow_override() {
        let mut args = args("coding");
        args.allow_tools = vec!["read".to_string()];

        assert_eq!(
            args.runtime_tool_names().expect("runtime tool names"),
            vec!["read".to_string()]
        );
    }

    #[test]
    fn chat_tool_policy_honors_deny_override() {
        let mut args = args("coding");
        args.deny_tools = vec!["write".to_string()];

        assert_eq!(
            args.runtime_tool_names().expect("runtime tool names"),
            vec!["read".to_string()]
        );
    }

    #[test]
    fn chat_tool_policy_rejects_unknown_profiles() {
        let err = args("definitely-not-real")
            .runtime_tool_names()
            .expect_err("unknown profile should fail");

        assert!(err.to_string().contains("unknown tool profile"));
    }
}
