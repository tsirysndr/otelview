//! OTLP receivers (gRPC and HTTP) and the remote-storage reader services.
//!
//! The gRPC listener serves, on one port:
//! - OTLP ingest: `TraceService`, `LogsService`, `MetricsService`
//! - Jaeger v2 remote-storage reads: `jaeger.storage.v2.TraceReader`
//! - otelview remote-storage reads: `otelview.storage.v1.{LogReader,
//!   MetricReader, Diagnostics}`
//!
//! so every otelview instance is simultaneously an OTLP collector and a
//! remote storage backend for other instances (and for Jaeger v2 itself).

use anyhow::{Context, Result};
use otelview_config::Config;
use otelview_storage::DynStorage;

pub mod grpc;
pub mod http;
pub mod readers;

/// Serve the gRPC endpoint (ingest + readers). Runs until aborted.
pub async fn serve_grpc(cfg: &Config, storage: DynStorage) -> Result<()> {
    let addr = cfg
        .receivers
        .grpc
        .listen
        .parse()
        .with_context(|| format!("invalid gRPC listen address {}", cfg.receivers.grpc.listen))?;
    let auth = grpc::ServerAuth::from_config(&cfg.auth);

    use opentelemetry_proto::tonic::collector::logs::v1::logs_service_server::LogsServiceServer;
    use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_server::MetricsServiceServer;
    use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::TraceServiceServer;
    use otelview_storage::proto::storage::v1::diagnostics_server::DiagnosticsServer;
    use otelview_storage::proto::storage::v1::log_reader_server::LogReaderServer;
    use otelview_storage::proto::storage::v1::metric_reader_server::MetricReaderServer;
    use otelview_storage::proto::storage::v2::trace_reader_server::TraceReaderServer;

    tracing::info!(%addr, "OTLP gRPC receiver + storage readers listening");
    tonic::transport::Server::builder()
        .add_service(TraceServiceServer::with_interceptor(
            grpc::TraceReceiver::new(storage.clone()),
            auth.clone(),
        ))
        .add_service(LogsServiceServer::with_interceptor(
            grpc::LogsReceiver::new(storage.clone()),
            auth.clone(),
        ))
        .add_service(MetricsServiceServer::with_interceptor(
            grpc::MetricsReceiver::new(storage.clone()),
            auth.clone(),
        ))
        .add_service(TraceReaderServer::with_interceptor(
            readers::TraceReaderService::new(storage.clone()),
            auth.clone(),
        ))
        .add_service(LogReaderServer::with_interceptor(
            readers::LogReaderService::new(storage.clone()),
            auth.clone(),
        ))
        .add_service(MetricReaderServer::with_interceptor(
            readers::MetricReaderService::new(storage.clone()),
            auth.clone(),
        ))
        .add_service(DiagnosticsServer::with_interceptor(
            readers::DiagnosticsService::new(storage),
            auth.clone(),
        ))
        .serve(addr)
        .await
        .context("gRPC receiver failed")
}

/// Serve the OTLP/HTTP endpoint. Runs until aborted.
pub async fn serve_http(cfg: &Config, storage: DynStorage) -> Result<()> {
    let addr: std::net::SocketAddr = cfg
        .receivers
        .http
        .listen
        .parse()
        .with_context(|| format!("invalid HTTP listen address {}", cfg.receivers.http.listen))?;
    let router = http::router(storage, &cfg.auth);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding OTLP HTTP receiver to {addr}"))?;
    tracing::info!(%addr, "OTLP HTTP receiver listening");
    axum::serve(listener, router).await.context("HTTP receiver failed")
}
