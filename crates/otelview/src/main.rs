//! otelview — a fast, beautiful OpenTelemetry viewer in a single binary.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use otelview_config::Config;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "otelview",
    version,
    about = "OpenTelemetry viewer: OTLP receivers, storage and a web UI in one binary"
)]
struct Cli {
    /// Path to a YAML or TOML config file.
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Print the default configuration as YAML and exit.
    #[arg(long)]
    print_config: bool,

    /// Override storage backend: memory | duckdb | jaeger | remote.
    #[arg(long, global = true)]
    storage: Option<String>,

    /// Override DuckDB database path (or ":memory:").
    #[arg(long, global = true)]
    duckdb_path: Option<String>,

    /// Override the UI/API listen address (e.g. 0.0.0.0:4319).
    #[arg(long)]
    listen: Option<String>,

    /// Require this token in the auth header on OTLP ingest.
    #[arg(long)]
    token: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the Model Context Protocol server, exposing traces, logs and
    /// metrics as tools for an AI agent.
    ///
    /// Speaks JSON-RPC over stdin/stdout by default, which is what desktop
    /// AI clients launch. With --endpoint it queries a running otelview
    /// over HTTP; without one it opens the configured storage directly,
    /// which a DuckDB file cannot do while a server has it open.
    Mcp {
        /// Query the otelview running at this URL instead of opening
        /// storage in this process, e.g. http://127.0.0.1:4319.
        #[arg(long)]
        endpoint: Option<String>,

        /// Bearer token to send to --endpoint.
        #[arg(long, env = "OTELVIEW_TOKEN", hide_env_values = true)]
        api_token: Option<String>,

        /// Serve MCP over HTTP at this address instead of on stdio.
        /// A non-loopback address requires a token.
        #[arg(long)]
        http: Option<String>,

        /// Path the HTTP endpoint is served at.
        #[arg(long)]
        path: Option<String>,
    },
}

fn apply_overrides(cfg: &mut Config, cli: &Cli) -> Result<()> {
    if let Some(backend) = &cli.storage {
        cfg.storage.backend = match backend.as_str() {
            "memory" => otelview_config::Backend::Memory,
            "duckdb" => otelview_config::Backend::Duckdb,
            "jaeger" => otelview_config::Backend::Jaeger,
            "remote" => otelview_config::Backend::Remote,
            other => anyhow::bail!("unknown storage backend {other}"),
        };
    }
    if let Some(path) = &cli.duckdb_path {
        cfg.storage.duckdb.path = path.clone();
    }
    if let Some(listen) = &cli.listen {
        cfg.ui.listen = listen.clone();
    }
    if let Some(token) = &cli.token {
        cfg.auth.token = Some(token.clone());
    }
    cfg.validate()
}

fn load_config(cli: &Cli) -> Result<Config> {
    let mut cfg = match &cli.config {
        Some(path) => Config::load(path)?,
        None => Config::default(),
    };
    apply_overrides(&mut cfg, cli)?;
    Ok(cfg)
}

/// `to_stderr` is not a preference: on stdio, stdout carries JSON-RPC and
/// a log line written there is a parse error in the client.
fn init_tracing(cfg: &Config, to_stderr: bool) {
    let filter = cfg
        .log_level
        .clone()
        .unwrap_or_else(|| "info,tower_http=warn".to_string());
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter));
    let builder = tracing_subscriber::fmt().with_env_filter(env_filter);
    if to_stderr {
        builder.with_writer(std::io::stderr).init();
    } else {
        builder.init();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.print_config {
        print!("{}", serde_yaml::to_string(&Config::default())?);
        return Ok(());
    }

    match &cli.command {
        Some(Command::Mcp {
            endpoint,
            api_token,
            http,
            path,
        }) => {
            let cfg = load_config(&cli)?;
            init_tracing(&cfg, true);
            run_mcp(
                cfg,
                endpoint.clone(),
                api_token.clone(),
                http.clone(),
                path.clone(),
            )
            .await
        }
        None => {
            let cfg = load_config(&cli)?;
            init_tracing(&cfg, false);
            run_server(cfg).await
        }
    }
}

async fn run_mcp(
    cfg: Config,
    endpoint: Option<String>,
    api_token: Option<String>,
    http: Option<String>,
    path: Option<String>,
) -> Result<()> {
    let mcp = match &endpoint {
        Some(url) => {
            tracing::info!(%url, "MCP server querying a remote otelview");
            otelview_mcp::from_endpoint(url, api_token)?
        }
        None => {
            tracing::info!(backend = ?cfg.storage.backend, "MCP server opening storage directly");
            let storage = otelview_storage::make_storage(&cfg.storage).await.context(
                "initializing storage backend (a DuckDB file cannot be opened twice — \
                     use --endpoint to query a running otelview instead)",
            )?;
            let oidc = otelview_auth::authenticator(&cfg)?;
            let (mcp, _) = otelview_mcp::from_storage(&cfg, storage, oidc);
            mcp
        }
    };

    let path = path.unwrap_or_else(|| cfg.mcp.path.clone());
    match http {
        Some(addr) => {
            let addr: std::net::SocketAddr = addr
                .parse()
                .with_context(|| format!("invalid MCP listen address {addr}"))?;
            let auth = otelview_mcp::http::Auth {
                token: cfg.mcp.resolved_token(&cfg),
                header: cfg.auth.header.clone(),
                oidc: otelview_auth::authenticator(&cfg)?,
            };
            otelview_mcp::http::serve(mcp, addr, &path, auth).await
        }
        None => otelview_mcp::stdio::serve(mcp).await,
    }
}

async fn run_server(cfg: Config) -> Result<()> {
    tracing::info!(
        backend = ?cfg.storage.backend,
        auth = cfg.auth.enabled(),
        mcp = cfg.mcp.enabled,
        "starting otelview"
    );

    let storage = otelview_storage::make_storage(&cfg.storage)
        .await
        .context("initializing storage backend")?;

    // Retention runs beside the servers, not in their request path. The
    // durations were validated with the rest of the config, so these parses
    // cannot fail here.
    if let Some(raw) = cfg
        .storage
        .retention
        .as_deref()
        .filter(|r| !r.trim().is_empty())
    {
        let retention = otelview_config::parse_duration(raw).context("storage.retention")?;
        let every = otelview_config::parse_duration(&cfg.storage.retention_sweep_interval)
            .context("storage.retention_sweep_interval")?;
        tokio::spawn(otelview_storage::run_retention(
            storage.clone(),
            retention,
            every,
        ));
    }

    let mut tasks: Vec<tokio::task::JoinHandle<Result<()>>> = Vec::new();
    if cfg.receivers.grpc.enabled {
        let cfg = cfg.clone();
        let storage = storage.clone();
        tasks.push(tokio::spawn(async move {
            otelview_receiver::serve_grpc(&cfg, storage).await
        }));
    }
    if cfg.receivers.http.enabled {
        let cfg = cfg.clone();
        let storage = storage.clone();
        tasks.push(tokio::spawn(async move {
            otelview_receiver::serve_http(&cfg, storage).await
        }));
    }
    {
        // One authenticator for the whole process: the API, the UI and
        // MCP share a session table and a key cache rather than each
        // keeping their own view of who is signed in.
        let oidc = otelview_auth::authenticator(&cfg)?;
        // MCP rides on the UI port, guarded by the same credentials, so a
        // running otelview *is* an MCP server with nothing else to start.
        let mcp = otelview_mcp::mounted_router(&cfg, storage.clone(), oidc.clone());
        if mcp.is_some() {
            tracing::info!(path = %cfg.mcp.path, "MCP endpoint enabled");
        }
        let opts = otelview_api::ServeOptions::default()
            .with_extra(mcp)
            .with_auth(oidc);
        let cfg = cfg.clone();
        let storage = storage.clone();
        tasks.push(tokio::spawn(async move {
            otelview_api::serve_with(&cfg, storage, opts).await
        }));
    }

    // First task to exit (usually with a bind error) brings the process down.
    let (result, _, _) = futures::future::select_all(tasks).await;
    result.context("server task panicked")?
}
