//! Where the telemetry comes from.
//!
//! Two implementations, because there are two ways to want this server:
//!
//! - [`direct::Direct`] runs inside a process that already has the storage
//!   open — the otelview server serving `/mcp` beside its own UI, or
//!   `otelview mcp --storage duckdb` reading a database file offline.
//! - [`rest::Rest`] talks to a running otelview over its query API, which
//!   is how a desktop AI client reaches an instance it does not host.
//!
//! The tools are written against the trait, so every tool works both ways.

use async_trait::async_trait;
use otelview_api::analytics::{FieldInfo, LogBucket, ServiceGraph, ServiceStats};
use otelview_api::query::{LogSearch, QueryResult, SeriesSearch, TraceSearch, Window};
use otelview_model::{
    ExemplarHit, LogRecord, MetricInfo, MetricSeries, SpanRecord, StorageStats, TraceSummary,
};
use serde_json::Value;

pub mod direct;
pub mod rest;

/// Everything the tool layer can ask of an otelview.
///
/// Deliberately the same set of calls the web UI makes: the point of the
/// MCP server is that an agent can see exactly what a human can, not a
/// reduced version of it.
#[async_trait]
pub trait Otel: Send + Sync + 'static {
    async fn services(&self) -> QueryResult<Vec<String>>;
    async fn operations(&self, service: &str) -> QueryResult<Vec<String>>;

    async fn search_traces(&self, p: TraceSearch) -> QueryResult<Vec<TraceSummary>>;
    async fn get_trace(&self, trace_id: &str) -> QueryResult<Vec<SpanRecord>>;
    async fn trace_fields(&self, service: Option<String>, w: Window)
        -> QueryResult<Vec<FieldInfo>>;

    async fn search_logs(&self, p: LogSearch) -> QueryResult<Vec<LogRecord>>;
    async fn log_histogram(&self, p: LogSearch, buckets: usize) -> QueryResult<Vec<LogBucket>>;
    async fn log_fields(
        &self,
        service: Option<String>,
        min_severity: Option<i32>,
        w: Window,
    ) -> QueryResult<Vec<FieldInfo>>;

    async fn metrics(&self) -> QueryResult<Vec<MetricInfo>>;
    async fn metric_series(&self, p: SeriesSearch) -> QueryResult<Vec<MetricSeries>>;
    async fn exemplars(
        &self,
        trace_id: &str,
        span_id: Option<&str>,
        limit: Option<usize>,
    ) -> QueryResult<Vec<ExemplarHit>>;

    async fn service_stats(&self, w: Window) -> QueryResult<Vec<ServiceStats>>;
    async fn service_graph(&self, w: Window) -> QueryResult<ServiceGraph>;
    async fn stats(&self) -> QueryResult<StorageStats>;
    async fn config(&self) -> QueryResult<Value>;

    /// One line naming what this server is pointed at, for the instructions
    /// the model is given at initialize time.
    fn describe(&self) -> String;
}
