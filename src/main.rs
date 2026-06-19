//! Binary entry point for the sheets_mcp MCP server.
//!
//! Initializes tracing (logging to stderr) and starts the MCP server
//! over stdio JSON-RPC transport using rmcp.

use rmcp::ServiceExt;
use rmcp::transport::stdio;

use sheets_mcp::tools::SheetsServer;

/// Main entry point. Starts the MCP server over stdio.
///
/// Logs are written to stderr (stdout is reserved for the MCP transport).
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing — logs go to stderr (stdio transport uses stdout)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("sheets_mcp starting up");

    // Create and serve the MCP server over stdio
    let service = SheetsServer::new().serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    service.waiting().await?;
    Ok(())
}
