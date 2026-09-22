//! An MCP server for otelview: traces, logs and metrics as tools an AI
//! agent can call.
//!
//! Everything the web UI can ask, an agent can ask — through the same query
//! layer, so the two cannot disagree. Seventeen tools cover the three
//! signals, the service map, RED metrics, field discovery and all three
//! query languages; a handful of resources and prompts cover knowing what
//! to ask in the first place.
//!
//! Two ways to run it, either against a storage handle this process holds
//! ([`Direct`]) or against a running otelview over HTTP ([`Rest`]):
//!
//! ```no_run
//! # async fn f(storage: otelview_storage::DynStorage) -> anyhow::Result<()> {
//! use std::sync::Arc;
//! use otelview_mcp::{Mcp, Direct, stdio};
//!
//! let cfg = Arc::new(otelview_config::Config::default());
//! let mcp = Mcp::new(Arc::new(Direct::new(storage, cfg)));
//! stdio::serve(mcp).await?;        // speak JSON-RPC over stdin/stdout
//! # Ok(()) }
//! ```
//!
//! The protocol layer is transport-agnostic: [`Mcp::handle`] takes one
//! JSON-RPC message and returns at most one, and [`stdio`] and [`http`] are
//! the two ways that gets framed.

use std::sync::Arc;

use anyhow::Result;
use otelview_config::Config;
use otelview_storage::DynStorage;

pub mod backend;
pub mod fmt;
pub mod prompts;
pub mod protocol;
pub mod render;
pub mod resources;
pub mod server;
pub mod syntax;
pub mod tools;
pub mod transport;

#[cfg(test)]
mod testing;

pub use backend::direct::Direct;
pub use backend::rest::Rest;
pub use backend::Otel;
pub use server::Mcp;
pub use transport::http;
pub use transport::stdio;

/// The MCP server for a storage handle this process already holds, with
/// the token its HTTP endpoint should require.
///
/// The token comes from `mcp.token`, `ui.token`, `auth.token` or
/// `OTELVIEW_MCP_TOKEN`, in that order of precedence — see
/// [`otelview_config::McpConfig::resolved_token`]. Locking the UI locks
/// this with the same key, so the endpoint cannot be left open by
/// forgetting a second setting.
pub fn from_storage(
    cfg: &Config,
    storage: DynStorage,
    oidc: Option<Arc<otelview_auth::Authenticator>>,
) -> (Mcp, http::Auth) {
    let mcp = Mcp::new(Arc::new(Direct::new(storage, Arc::new(cfg.clone()))));
    let auth = http::Auth {
        token: cfg.mcp.resolved_token(cfg),
        header: cfg.auth.header.clone(),
        oidc,
    };
    (mcp, auth)
}

/// The MCP server for a remote otelview's query API.
pub fn from_endpoint(endpoint: &str, token: Option<String>) -> Result<Mcp> {
    Ok(Mcp::new(Arc::new(Rest::new(endpoint, token)?)))
}

/// The router to merge into the UI/API server, or `None` when MCP is off.
///
/// Carries its own auth so that mounting it somewhere else — or serving it
/// standalone — cannot lose the token by accident.
pub fn mounted_router(
    cfg: &Config,
    storage: DynStorage,
    oidc: Option<Arc<otelview_auth::Authenticator>>,
) -> Option<axum::Router> {
    if !cfg.mcp.enabled {
        return None;
    }
    let (mcp, auth) = from_storage(cfg, storage, oidc);
    if !auth.enabled() {
        tracing::info!(
            path = %cfg.mcp.path,
            "MCP endpoint is open; set mcp.token or OTELVIEW_MCP_TOKEN to require one"
        );
    }
    Some(http::router(mcp, &cfg.mcp.path, auth))
}
