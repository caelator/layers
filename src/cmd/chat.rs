//! `layers chat` — interactive REPL-style chat surface.
//!
//! Provides a simple stdin/stdout chat loop that can be used for quick
//! one-shot queries or multi-turn conversations using the Layers runtime.

use std::io::{self, BufRead, Write};

/// Arguments for the `layers chat` command.
pub struct ChatArgs {
    /// Optional system prompt override.
    pub system_prompt: Option<String>,
    /// Optional model override (e.g. "openai/gpt-4").
    pub model: Option<String>,
    /// Maximum turns before exiting (0 = unlimited).
    pub max_turns: usize,
    /// Output as JSON.
    pub json: bool,
}

/// Run the interactive chat loop.
///
/// Reads lines from stdin, processes each as a query through the Layers
/// routing pipeline, and prints the assembled context/response.
pub fn handle_chat(args: &ChatArgs) -> anyhow::Result<()> {
    let workspace = crate::config::workspace_root();

    println!("layers chat — type your query (Ctrl-D or 'exit' to quit)");
    println!("workspace: {}", workspace.display());
    if let Some(ref model) = args.model {
        println!("model override: {model}");
    }
    if let Some(ref prompt) = args.system_prompt {
        println!("system prompt: {prompt}");
    }
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
        match crate::cmd::query::handle_query(input, args.json, false, 1) {
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
