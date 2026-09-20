//! Storage backends for otelview.
//!
//! Three implementations of the [`Storage`] trait:
//! - [`memory::MemoryStorage`]: bounded ring buffers, zero dependencies.
//! - [`duck::DuckdbStorage`]: embedded DuckDB database (file or in-memory).
//! - [`jaeger::JaegerStorage`]: traces in an external Jaeger v2
//!   remote-storage gRPC backend, logs/metrics in a local fallback.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use otelview_config::{Backend, FallbackBackend, StorageConfig};
use otelview_model::{
    LogQuery, LogRecord, MetricInfo, MetricPoint, MetricQuery, MetricSeries, SpanRecord,
    StorageStats, TraceQuery, TraceSummary,
};

pub mod duck;
pub mod jaeger;
pub mod memory;
pub mod otlp;
pub mod remote;
pub mod summary;

/// Generated code for `jaeger.storage.v2` (and its `jaeger.expression.v1`
/// and `openapi.v3` dependencies).
pub mod proto {
    pub mod openapi {
        #[allow(clippy::large_enum_variant)]
        pub mod v3 {
            tonic::include_proto!("openapi.v3");
        }
    }
    pub mod expression {
        pub mod v1 {
            tonic::include_proto!("jaeger.expression.v1");
        }
    }
    pub mod storage {
        pub mod v2 {
            tonic::include_proto!("jaeger.storage.v2");
        }
        pub mod v1 {
            tonic::include_proto!("otelview.storage.v1");
        }
    }
}

#[async_trait]
pub trait Storage: Send + Sync + 'static {
    async fn insert_spans(&self, spans: Vec<SpanRecord>) -> Result<()>;
    async fn insert_logs(&self, logs: Vec<LogRecord>) -> Result<()>;
    async fn insert_metrics(&self, points: Vec<MetricPoint>) -> Result<()>;

    async fn list_services(&self) -> Result<Vec<String>>;
    async fn list_operations(&self, service: &str) -> Result<Vec<String>>;
    async fn find_traces(&self, q: TraceQuery) -> Result<Vec<TraceSummary>>;
    async fn get_trace(&self, trace_id: &str) -> Result<Vec<SpanRecord>>;
    async fn query_logs(&self, q: LogQuery) -> Result<Vec<LogRecord>>;
    async fn list_metrics(&self) -> Result<Vec<MetricInfo>>;
    async fn query_metric_series(&self, q: MetricQuery) -> Result<Vec<MetricSeries>>;
    async fn stats(&self) -> Result<StorageStats>;

    /// Delete every span, log and metric point older than `cutoff_unix_nano`,
    /// returning (spans, logs, metric_points) deleted — or `None` when this
    /// backend does not own its data. The default is `None`: memory already
    /// bounds itself with ring buffers, and the jaeger/remote backends
    /// delegate retention to the storage server they sit in front of.
    async fn sweep_expired(&self, cutoff_unix_nano: u64) -> Result<Option<(u64, u64, u64)>> {
        let _ = cutoff_unix_nano;
        Ok(None)
    }
}

pub type DynStorage = Arc<dyn Storage>;

/// The retention loop: an immediate sweep, then one per interval, forever.
///
/// Takes the trait object rather than living on a backend, so whichever
/// storage supports [`Storage::sweep_expired`] gets it. Sweep failures are
/// logged and retried next round rather than taking the process down —
/// losing a sweep is recoverable, losing ingest is not.
pub async fn run_retention(
    storage: DynStorage,
    retention: std::time::Duration,
    every: std::time::Duration,
) {
    tracing::info!(
        retention_hours = retention.as_secs() / 3600,
        sweep_interval_secs = every.as_secs(),
        "retention enabled"
    );
    let mut ticker = tokio::time::interval(every);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|now| now.as_nanos().saturating_sub(retention.as_nanos()) as u64)
            .unwrap_or(0);
        match storage.sweep_expired(cutoff).await {
            Ok(Some((spans, logs, metric_points))) => {
                if spans + logs + metric_points > 0 {
                    tracing::info!(
                        spans,
                        logs,
                        metric_points,
                        "retention sweep deleted expired rows"
                    );
                } else {
                    tracing::debug!("retention sweep: nothing expired");
                }
            }
            Ok(None) => {
                // Said once, loudly: a configured retention that this backend
                // cannot honour must not pass silently.
                tracing::warn!(
                    "storage.retention is set but this backend does not own its data; \
                     nothing will be deleted (the remote storage has its own knob)"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "retention sweep failed; retrying next interval");
            }
        }
    }
}

/// Build the storage backend described by the config.
pub async fn make_storage(cfg: &StorageConfig) -> Result<DynStorage> {
    Ok(match cfg.backend {
        Backend::Memory => Arc::new(memory::MemoryStorage::new(&cfg.memory)),
        Backend::Duckdb => Arc::new(duck::DuckdbStorage::open(&cfg.duckdb.path)?),
        Backend::Jaeger => {
            let fallback: DynStorage = match cfg.jaeger.fallback {
                FallbackBackend::Memory => Arc::new(memory::MemoryStorage::new(&cfg.memory)),
                FallbackBackend::Duckdb => Arc::new(duck::DuckdbStorage::open(&cfg.duckdb.path)?),
            };
            Arc::new(jaeger::JaegerStorage::connect(&cfg.jaeger.endpoint, fallback).await?)
        }
        Backend::Remote => Arc::new(remote::RemoteStorage::connect(&cfg.remote).await?),
    })
}
