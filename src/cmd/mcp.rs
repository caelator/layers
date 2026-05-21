use std::sync::Arc;

use anyhow::Result;
use clap::Subcommand;
#[cfg(test)]
use layers_mcp::server::stable_context_surface_tools;
use layers_mcp::server::{McpServer, McpServerConfig};
use layers_mcp::stable::stable_context_registry;

/// MCP server commands for the stable Layers context surface.
#[derive(Debug, Subcommand)]
pub enum McpCommands {
    /// Serve stable context compiler MCP tools over stdio.
    Serve,
}

pub fn stable_mcp_server() -> McpServer {
    let registry = Arc::new(stable_context_registry());
    McpServer::new(registry, McpServerConfig::stable_context_surface())
}

#[cfg(test)]
pub fn stable_mcp_tool_names() -> Vec<&'static str> {
    stable_context_surface_tools()
}

pub fn handle_mcp(command: &McpCommands) -> Result<()> {
    match command {
        McpCommands::Serve => {
            let server = stable_mcp_server();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()?;
            runtime.block_on(server.run()).map_err(Into::into)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layers_mcp::server::is_dangerous_tool;

    #[test]
    fn stable_mcp_server_exposes_only_stable_context_tools() {
        let server = stable_mcp_server();
        let mut exposed: Vec<_> = server.exposed_tools().iter().map(String::as_str).collect();
        exposed.sort_unstable();

        let mut expected = stable_mcp_tool_names();
        expected.sort_unstable();

        assert_eq!(exposed, expected);
        assert!(exposed.iter().all(|tool| !is_dangerous_tool(tool)));
    }
}
