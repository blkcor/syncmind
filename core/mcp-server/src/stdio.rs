use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::protocol::*;
use crate::server::McpServer;

/// Run the MCP server over stdio.
///
/// Reads JSON-RPC requests from stdin line-by-line and writes responses to stdout.
/// All diagnostics must go to stderr or tracing (which is configured to stderr).
pub async fn run_stdio_server(server: Arc<McpServer>) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();
    let mut stdout = stdout;

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break; // EOF
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<JsonRpcRequest>(line) {
            Ok(req) => {
                if let Some(resp) = server.handle_request(req).await {
                    let json = serde_json::to_string(&resp)?;
                    stdout.write_all(json.as_bytes()).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to parse JSON-RPC request");
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    body: JsonRpcResponseBody::Error {
                        error: JsonRpcError::invalid_request(e.to_string()),
                    },
                };
                let json = serde_json::to_string(&resp)?;
                stdout.write_all(json.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        }
    }

    Ok(())
}
