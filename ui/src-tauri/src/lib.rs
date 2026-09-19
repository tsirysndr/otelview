//! otelview desktop shell.
//!
//! On startup, if no otelview server is reachable on 127.0.0.1:4319, an
//! embedded server (OTLP receivers + query API, DuckDB storage in the app
//! data directory) is started inside this process — the desktop app works
//! standalone out of the box. When a server is already running (or a remote
//! URL is configured in Settings), the embedded one stays off.

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use tauri::Manager;

const LOCAL_API: &str = "127.0.0.1:4319";

fn server_reachable() -> bool {
    let addr: SocketAddr = LOCAL_API.parse().expect("valid local api addr");
    TcpStream::connect_timeout(&addr, Duration::from_millis(400)).is_ok()
}

async fn run_embedded_server(db_path: PathBuf) {
    let mut cfg = otelview_config::Config::default();
    cfg.storage.backend = otelview_config::Backend::Duckdb;
    cfg.storage.duckdb.path = db_path.to_string_lossy().into_owned();
    cfg.ui.listen = LOCAL_API.to_string();

    let storage = match otelview_storage::make_storage(&cfg.storage).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("otelview-desktop: embedded storage failed: {e:#}");
            return;
        }
    };
    eprintln!(
        "otelview-desktop: embedded server starting (duckdb at {})",
        cfg.storage.duckdb.path
    );

    let grpc = {
        let cfg = cfg.clone();
        let storage = storage.clone();
        async move {
            if let Err(e) = otelview_receiver::serve_grpc(&cfg, storage).await {
                eprintln!("otelview-desktop: gRPC receiver stopped: {e:#}");
            }
        }
    };
    let http = {
        let cfg = cfg.clone();
        let storage = storage.clone();
        async move {
            if let Err(e) = otelview_receiver::serve_http(&cfg, storage).await {
                eprintln!("otelview-desktop: HTTP receiver stopped: {e:#}");
            }
        }
    };
    let api = async move {
        if let Err(e) = otelview_api::serve(&cfg, storage).await {
            eprintln!("otelview-desktop: API server stopped: {e:#}");
        }
    };
    tokio::join!(grpc, http, api);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            if server_reachable() {
                eprintln!("otelview-desktop: server already running on {LOCAL_API}, not embedding");
            } else {
                let data_dir = app.path().app_data_dir()?;
                std::fs::create_dir_all(&data_dir)?;
                let db_path = data_dir.join("otelview.duckdb");
                tauri::async_runtime::spawn(run_embedded_server(db_path));
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running otelview desktop");
}
