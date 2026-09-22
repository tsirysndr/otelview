//! Newline-delimited JSON-RPC over stdin/stdout.
//!
//! The transport every desktop AI client speaks: it launches the binary and
//! talks to it over pipes. Nothing but protocol may be written to stdout —
//! a stray `println!` is a parse error on the other end — so logging goes
//! to stderr, which the client shows as server output.
//!
//! There is no token here, by design. The pipe *is* the boundary: whoever
//! can write to this process's stdin already started it, with this user's
//! privileges and this user's config, and could read the storage directly.
//! Asking them for a token would secure nothing. Only [`super::http`],
//! which anyone on the network can reach, has something to authenticate.

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::Mcp;

/// Read requests until stdin closes, which is how the client says goodbye.
pub async fn serve(mcp: Mcp) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    tracing::info!("otelview MCP server ready on stdio (no token: the pipe is the boundary)");
    let mut served = 0u64;
    while let Some(line) = lines.next_line().await.context("reading from stdin")? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        tracing::trace!(bytes = line.len(), "mcp message in");
        served += 1;
        let Some(response) = mcp.handle(line).await else {
            continue;
        };
        // One message per line, and flushed: the client is blocking on
        // this read, so a buffered response is a hang.
        stdout
            .write_all(response.as_bytes())
            .await
            .context("writing to stdout")?;
        stdout.write_all(b"\n").await.context("writing to stdout")?;
        stdout.flush().await.context("flushing stdout")?;
    }
    tracing::info!(served, "stdin closed, MCP server exiting");
    Ok(())
}
