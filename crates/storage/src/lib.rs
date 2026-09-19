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
}

pub type DynStorage = Arc<dyn Storage>;

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
