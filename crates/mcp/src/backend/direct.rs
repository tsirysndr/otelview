//! In-process backend: the storage handle itself.
//!
//! Every call goes through `otelview_api::query` and
//! `otelview_api::analytics`, which is the same code the REST handlers run.
//! An agent and the web UI asking the same question therefore cannot get
//! different answers.

use std::sync::Arc;

use async_trait::async_trait;
use otelview_api::analytics::{self, FieldInfo, LogBucket, ServiceGraph, ServiceStats};
use otelview_api::query::{self, LogSearch, QueryResult, SeriesSearch, TraceSearch, Window};
use otelview_config::Config;
use otelview_model::{
    ExemplarHit, LogRecord, MetricInfo, MetricSeries, SpanRecord, StorageStats, TraceSummary,
};
use otelview_storage::DynStorage;
use serde_json::Value;

use super::Otel;

pub struct Direct {
    storage: DynStorage,
    config: Arc<Config>,
}

impl Direct {
    pub fn new(storage: DynStorage, config: Arc<Config>) -> Self {
        Self { storage, config }
    }
}

#[async_trait]
impl Otel for Direct {
    async fn services(&self) -> QueryResult<Vec<String>> {
        Ok(self.storage.list_services().await?)
    }

    async fn operations(&self, service: &str) -> QueryResult<Vec<String>> {
        Ok(self.storage.list_operations(service).await?)
    }

    async fn search_traces(&self, p: TraceSearch) -> QueryResult<Vec<TraceSummary>> {
        query::search_traces(&self.storage, p).await
    }

    async fn get_trace(&self, trace_id: &str) -> QueryResult<Vec<SpanRecord>> {
        Ok(self.storage.get_trace(trace_id).await?)
    }

    async fn trace_fields(
        &self,
        service: Option<String>,
        w: Window,
    ) -> QueryResult<Vec<FieldInfo>> {
        let (min, max) = w.bounds();
        Ok(analytics::trace_fields(&self.storage, service, min, max).await?)
    }

    async fn search_logs(&self, p: LogSearch) -> QueryResult<Vec<LogRecord>> {
        query::search_logs(&self.storage, p).await
    }

    async fn log_histogram(&self, p: LogSearch, buckets: usize) -> QueryResult<Vec<LogBucket>> {
        query::log_histogram(&self.storage, p, buckets).await
    }

    async fn log_fields(
        &self,
        service: Option<String>,
        min_severity: Option<i32>,
        w: Window,
    ) -> QueryResult<Vec<FieldInfo>> {
        let (min, max) = w.bounds();
        Ok(analytics::log_fields(&self.storage, service, min_severity, min, max).await?)
    }

    async fn metrics(&self) -> QueryResult<Vec<MetricInfo>> {
        Ok(self.storage.list_metrics().await?)
    }

    async fn metric_series(&self, p: SeriesSearch) -> QueryResult<Vec<MetricSeries>> {
        query::metric_series(&self.storage, p).await
    }

    async fn exemplars(
        &self,
        trace_id: &str,
        span_id: Option<&str>,
        limit: Option<usize>,
    ) -> QueryResult<Vec<ExemplarHit>> {
        Ok(query::find_exemplars(&self.storage, trace_id, span_id, limit).await?)
    }

    async fn service_stats(&self, w: Window) -> QueryResult<Vec<ServiceStats>> {
        let (min, max) = w.bounds();
        Ok(analytics::service_stats(&self.storage, min, max).await?)
    }

    async fn service_graph(&self, w: Window) -> QueryResult<ServiceGraph> {
        let (min, max) = w.bounds();
        Ok(analytics::service_graph(&self.storage, min, max).await?)
    }

    async fn stats(&self) -> QueryResult<StorageStats> {
        Ok(self.storage.stats().await?)
    }

    async fn config(&self) -> QueryResult<Value> {
        Ok(serde_json::to_value(self.config.sanitized())
            .map_err(|e| anyhow::anyhow!("serializing config: {e}"))?)
    }

    fn describe(&self) -> String {
        format!(
            "this process, backed by {:?} storage",
            self.config.storage.backend
        )
    }
}
