//! Full remote storage: another otelview instance (or compatible backend) at
//! one gRPC endpoint.
//!
//! Traces: `jaeger.storage.v2.TraceReader` + OTLP `TraceService/Export`.
//! Logs: `otelview.storage.v1.LogReader` + OTLP `LogsService/Export`.
//! Metrics: `otelview.storage.v1.MetricReader` + OTLP `MetricsService/Export`.
//! Stats: `otelview.storage.v1.Diagnostics`.

use anyhow::{Context, Result};
use async_trait::async_trait;
use opentelemetry_proto::tonic::collector::logs::v1::logs_service_client::LogsServiceClient;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_client::MetricsServiceClient;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use otelview_config::RemoteConfig;
use otelview_model::{
    LogQuery, LogRecord, MetricInfo, MetricPoint, MetricQuery, MetricSeries, MetricType,
    SpanRecord, StorageStats, TraceQuery, TraceSummary,
};
use tonic::metadata::{MetadataKey, MetadataValue};
use tonic::service::Interceptor;
use tonic::transport::Channel;

use crate::jaeger::JaegerStorage;
use crate::memory::group_series;
use crate::proto::storage::v1 as osv1;
use crate::proto::storage::v1::diagnostics_client::DiagnosticsClient;
use crate::proto::storage::v1::log_reader_client::LogReaderClient;
use crate::proto::storage::v1::metric_reader_client::MetricReaderClient;
use crate::{otlp, DynStorage, Storage};

/// Adds the configured auth header to every outgoing request.
#[derive(Clone)]
pub struct AuthInterceptor {
    header: Option<(
        MetadataKey<tonic::metadata::Ascii>,
        MetadataValue<tonic::metadata::Ascii>,
    )>,
}

impl AuthInterceptor {
    pub fn new(header: &str, token: Option<&str>) -> Result<Self> {
        let header = match token.filter(|t| !t.is_empty()) {
            Some(token) => Some((
                header
                    .parse::<MetadataKey<_>>()
                    .context("invalid auth header name")?,
                token
                    .parse::<MetadataValue<_>>()
                    .context("invalid auth token value")?,
            )),
            None => None,
        };
        Ok(Self { header })
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some((key, value)) = &self.header {
            req.metadata_mut().insert(key.clone(), value.clone());
        }
        Ok(req)
    }
}

pub struct RemoteStorage {
    channel: Channel,
    interceptor: AuthInterceptor,
    /// Trace reads/writes are delegated to the Jaeger v2 client, which shares
    /// the same channel; its fallback is never reached because logs/metrics
    /// calls go through the readers below.
    traces: JaegerStorage,
    endpoint: String,
}

fn nanos_to_timestamp(nanos: u64) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: (nanos / 1_000_000_000) as i64,
        nanos: (nanos % 1_000_000_000) as i32,
    }
}

fn timestamp_opt(nanos: Option<u64>) -> Option<prost_types::Timestamp> {
    nanos.map(nanos_to_timestamp)
}

impl RemoteStorage {
    pub async fn connect(cfg: &RemoteConfig) -> Result<Self> {
        let channel = Channel::from_shared(cfg.endpoint.clone())
            .with_context(|| format!("invalid remote endpoint {}", cfg.endpoint))?
            .connect_lazy();
        let interceptor = AuthInterceptor::new(&cfg.auth_header, cfg.auth_token.as_deref())?;
        // The trace half never uses its fallback; a tiny memory store satisfies
        // the constructor.
        let fallback: DynStorage = std::sync::Arc::new(crate::memory::MemoryStorage::new(
            &otelview_config::MemoryConfig {
                max_spans: 1,
                max_logs: 1,
                max_metric_points: 1,
            },
        ));
        let traces = JaegerStorage::from_channel(
            channel.clone(),
            fallback,
            cfg.endpoint.clone(),
            interceptor.clone(),
        );
        Ok(Self {
            channel,
            interceptor,
            traces,
            endpoint: cfg.endpoint.clone(),
        })
    }

    fn log_reader(
        &self,
    ) -> LogReaderClient<tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>>
    {
        LogReaderClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    fn metric_reader(
        &self,
    ) -> MetricReaderClient<tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>>
    {
        MetricReaderClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    fn diagnostics(
        &self,
    ) -> DiagnosticsClient<tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>>
    {
        DiagnosticsClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }
}

#[async_trait]
impl Storage for RemoteStorage {
    async fn insert_spans(&self, spans: Vec<SpanRecord>) -> Result<()> {
        self.traces.insert_spans(spans).await
    }

    async fn insert_logs(&self, logs: Vec<LogRecord>) -> Result<()> {
        if logs.is_empty() {
            return Ok(());
        }
        let data = otlp::logs_to_logs_data(&logs);
        LogsServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
            .export(ExportLogsServiceRequest {
                resource_logs: data.resource_logs,
            })
            .await
            .with_context(|| format!("exporting logs to remote storage at {}", self.endpoint))?;
        Ok(())
    }

    async fn insert_metrics(&self, points: Vec<MetricPoint>) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }
        let data = otlp::metric_points_to_metrics_data(&points);
        MetricsServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
            .export(ExportMetricsServiceRequest {
                resource_metrics: data.resource_metrics,
            })
            .await
            .with_context(|| format!("exporting metrics to remote storage at {}", self.endpoint))?;
        Ok(())
    }

    async fn list_services(&self) -> Result<Vec<String>> {
        // Union of what each signal knows; tolerate individual failures so a
        // trace-only backend still lists services.
        let mut services = std::collections::BTreeSet::new();
        if let Ok(s) = self.traces.list_services().await {
            services.extend(s);
        }
        if let Ok(resp) = self
            .log_reader()
            .get_services(osv1::GetServicesRequest {})
            .await
        {
            services.extend(resp.into_inner().services);
        }
        if let Ok(resp) = self
            .metric_reader()
            .get_services(osv1::GetServicesRequest {})
            .await
        {
            services.extend(resp.into_inner().services);
        }
        Ok(services.into_iter().collect())
    }

    async fn list_operations(&self, service: &str) -> Result<Vec<String>> {
        self.traces.list_operations(service).await
    }

    async fn find_traces(&self, q: TraceQuery) -> Result<Vec<TraceSummary>> {
        self.traces.find_traces(q).await
    }

    async fn get_trace(&self, trace_id: &str) -> Result<Vec<SpanRecord>> {
        self.traces.get_trace(trace_id).await
    }

    async fn query_logs(&self, q: LogQuery) -> Result<Vec<LogRecord>> {
        let request = osv1::FindLogsRequest {
            query: Some(osv1::LogQueryParameters {
                service_name: q.service.clone().unwrap_or_default(),
                min_severity: q.min_severity.unwrap_or(0),
                search: q.search.clone().unwrap_or_default(),
                trace_id: q.trace_id.clone().unwrap_or_default(),
                time_min: timestamp_opt(q.time_min_unix_nano),
                time_max: timestamp_opt(q.time_max_unix_nano),
                search_depth: q.limit as i32,
            }),
        };
        let mut stream = self
            .log_reader()
            .find_logs(request)
            .await
            .context("remote FindLogs")?
            .into_inner();
        let mut out = Vec::new();
        while let Some(chunk) = stream.message().await.context("logs stream")? {
            out.extend(otlp::logs_from_resource_logs(&chunk.resource_logs));
        }
        out.sort_by_key(|l| std::cmp::Reverse(l.time_unix_nano));
        Ok(out)
    }

    async fn list_metrics(&self) -> Result<Vec<MetricInfo>> {
        let resp = self
            .metric_reader()
            .list_metrics(osv1::ListMetricsRequest {})
            .await
            .context("remote ListMetrics")?;
        Ok(resp
            .into_inner()
            .metrics
            .into_iter()
            .map(|m| MetricInfo {
                name: m.name,
                description: m.description,
                unit: m.unit,
                metric_type: MetricType::parse(&m.metric_type).unwrap_or(MetricType::Gauge),
                services: m.services,
            })
            .collect())
    }

    async fn query_metric_series(&self, q: MetricQuery) -> Result<Vec<MetricSeries>> {
        let request = osv1::FindMetricsRequest {
            query: Some(osv1::MetricQueryParameters {
                metric_name: q.name.clone(),
                service_name: q.service.clone().unwrap_or_default(),
                time_min: timestamp_opt(q.time_min_unix_nano),
                time_max: timestamp_opt(q.time_max_unix_nano),
                max_points: q.max_points as i32,
            }),
        };
        let mut stream = self
            .metric_reader()
            .find_metrics(request)
            .await
            .context("remote FindMetrics")?
            .into_inner();
        let mut points = Vec::new();
        while let Some(chunk) = stream.message().await.context("metrics stream")? {
            points.extend(otlp::metrics_from_resource_metrics(&chunk.resource_metrics));
        }
        Ok(group_series(points.iter().collect(), q.max_points))
    }

    async fn stats(&self) -> Result<StorageStats> {
        match self.diagnostics().get_stats(osv1::GetStatsRequest {}).await {
            Ok(resp) => {
                let s = resp.into_inner();
                Ok(StorageStats {
                    spans: s.spans,
                    logs: s.logs,
                    metric_points: s.metric_points,
                    services: s.services,
                    backend: format!("remote ({}) → {}", self.endpoint, s.backend),
                })
            }
            // A jaeger-only backend won't serve Diagnostics; degrade gracefully.
            Err(_) => Ok(StorageStats {
                backend: format!("remote ({})", self.endpoint),
                ..Default::default()
            }),
        }
    }
}
