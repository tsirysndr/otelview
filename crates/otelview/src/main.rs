//! otelview — a fast, beautiful OpenTelemetry viewer in a single binary.

use anyhow::{Context, Result};
use clap::Parser;
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
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Print the default configuration as YAML and exit.
    #[arg(long)]
    print_config: bool,

    /// Override storage backend: memory | duckdb | jaeger | remote.
    #[arg(long)]
    storage: Option<String>,

    /// Override DuckDB database path (or ":memory:").
    #[arg(long)]
    duckdb_path: Option<String>,

    /// Override the UI/API listen address (e.g. 0.0.0.0:4319).
    #[arg(long)]
    listen: Option<String>,

    /// Require this token in the auth header on OTLP ingest.
    #[arg(long)]
    token: Option<String>,
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.print_config {
        print!("{}", serde_yaml::to_string(&Config::default())?);
        return Ok(());
    }

    let mut cfg = match &cli.config {
        Some(path) => Config::load(path)?,
        None => Config::default(),
    };
    apply_overrides(&mut cfg, &cli)?;

    let filter = cfg
        .log_level
        .clone()
        .unwrap_or_else(|| "info,tower_http=warn".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .init();

    tracing::info!(
        backend = ?cfg.storage.backend,
        auth = cfg.auth.enabled(),
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
        let cfg = cfg.clone();
        let storage = storage.clone();
        tasks.push(tokio::spawn(async move {
            otelview_api::serve(&cfg, storage).await
        }));
    }

    // First task to exit (usually with a bind error) brings the process down.
    let (result, _, _) = futures::future::select_all(tasks).await;
    result.context("server task panicked")?
}
