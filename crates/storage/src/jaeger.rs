//! External trace storage via the Jaeger v2 remote-storage gRPC API.
//!
//! Reads go through `jaeger.storage.v2.TraceReader`; writes push spans with
//! the standard OTLP `TraceService/Export` (as the storage API prescribes).
//! Logs and metrics are not part of the Jaeger storage API and are delegated
//! to a local fallback backend.

use anyhow::{Context, Result};
use async_trait::async_trait;
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_client::TraceServiceClient;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use otelview_model::{
    LogQuery, LogRecord, MetricInfo, MetricPoint, MetricQuery, MetricSeries, SpanRecord,
    StorageStats, TraceQuery, TraceSummary,
};
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Channel;
use tonic::Code;

use crate::proto::storage::v2 as jsv2;
use crate::proto::storage::v2::trace_reader_client::TraceReaderClient;
use crate::remote::AuthInterceptor;
use crate::summary::build_trace_summaries;
use crate::{otlp, DynStorage, Storage};

pub struct JaegerStorage {
    channel: Channel,
    interceptor: AuthInterceptor,
    fallback: DynStorage,
    endpoint: String,
}

impl JaegerStorage {
    /// Connect lazily: the backend may come up after us, calls fail per-call.
    pub async fn connect(endpoint: &str, fallback: DynStorage) -> Result<Self> {
        let channel = Channel::from_shared(endpoint.to_string())
            .with_context(|| format!("invalid jaeger endpoint {endpoint}"))?
            .connect_lazy();
        Ok(Self::from_channel(
            channel,
            fallback,
            endpoint.to_string(),
            AuthInterceptor::new("x-otelview-token", None)?,
        ))
    }

    pub fn from_channel(
        channel: Channel,
        fallback: DynStorage,
        endpoint: String,
        interceptor: AuthInterceptor,
    ) -> Self {
        Self {
            channel,
            interceptor,
            fallback,
            endpoint,
        }
    }

    fn reader(&self) -> TraceReaderClient<InterceptedService<Channel, AuthInterceptor>> {
        TraceReaderClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    fn writer(&self) -> TraceServiceClient<InterceptedService<Channel, AuthInterceptor>> {
        TraceServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }
}

fn nanos_to_timestamp(nanos: u64) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: (nanos / 1_000_000_000) as i64,
        nanos: (nanos % 1_000_000_000) as i32,
    }
}

fn nanos_to_duration(nanos: u64) -> prost_types::Duration {
    prost_types::Duration {
        seconds: (nanos / 1_000_000_000) as i64,
        nanos: (nanos % 1_000_000_000) as i32,
    }
}

fn build_query_params(q: &TraceQuery) -> jsv2::TraceQueryParameters {
    let mut attributes = Vec::new();
    if let Some(attr_q) = q.attribute_query.as_deref().filter(|s| !s.is_empty()) {
        // The storage API takes exact key=value attribute matches.
        if let Some((k, v)) = attr_q.split_once('=') {
            attributes.push(jsv2::KeyValue {
                key: k.trim().to_string(),
                value: Some(jsv2::AnyValue {
                    value: Some(jsv2::any_value::Value::StringValue(v.trim().to_string())),
                }),
            });
        }
    }
    jsv2::TraceQueryParameters {
        service_name: q.service.clone().unwrap_or_default(),
        operation_name: q.operation.clone().unwrap_or_default(),
        attributes,
        start_time_min: q.start_time_min_unix_nano.map(nanos_to_timestamp),
        start_time_max: q.start_time_max_unix_nano.map(nanos_to_timestamp),
        duration_min: q.min_duration_nanos.map(nanos_to_duration),
        duration_max: q.max_duration_nanos.map(nanos_to_duration),
        search_depth: if q.limit == 0 { 20 } else { q.limit as i32 },
        filter: None,
        pagination: None,
    }
}

#[async_trait]
impl Storage for JaegerStorage {
    async fn insert_spans(&self, spans: Vec<SpanRecord>) -> Result<()> {
        if spans.is_empty() {
            return Ok(());
        }
        let traces_data = otlp::spans_to_traces_data(&spans);
        self.writer()
            .export(ExportTraceServiceRequest {
                resource_spans: traces_data.resource_spans,
            })
            .await
            .with_context(|| format!("exporting spans to jaeger storage at {}", self.endpoint))?;
        Ok(())
    }

    async fn insert_logs(&self, logs: Vec<LogRecord>) -> Result<()> {
        self.fallback.insert_logs(logs).await
    }

    async fn insert_metrics(&self, points: Vec<MetricPoint>) -> Result<()> {
        self.fallback.insert_metrics(points).await
    }

    async fn list_services(&self) -> Result<Vec<String>> {
        let resp = self
            .reader()
            .get_services(jsv2::GetServicesRequest {})
            .await
            .context("jaeger GetServices")?;
        Ok(resp.into_inner().services)
    }

    async fn list_operations(&self, service: &str) -> Result<Vec<String>> {
        let resp = self
            .reader()
            .get_operations(jsv2::GetOperationsRequest {
                service: service.to_string(),
                span_kind: String::new(),
            })
            .await
            .context("jaeger GetOperations")?;
        let mut ops: Vec<String> = resp
            .into_inner()
            .operations
            .into_iter()
            .map(|o| o.name)
            .collect();
        ops.sort();
        ops.dedup();
        Ok(ops)
    }

    async fn find_traces(&self, q: TraceQuery) -> Result<Vec<TraceSummary>> {
        let params = build_query_params(&q);

        // Preferred: lightweight summaries straight from the backend.
        let summaries = self
            .reader()
            .find_trace_summaries(jsv2::FindTraceSummariesRequest {
                query: Some(params.clone()),
            })
            .await;
        match summaries {
            Ok(stream) => {
                let mut stream = stream.into_inner();
                let mut out = Vec::new();
                while let Some(chunk) = stream.message().await.context("summary stream")? {
                    for s in chunk.summaries {
                        out.push(TraceSummary {
                            trace_id: hex::encode(&s.trace_id),
                            root_name: s.root_operation_name,
                            root_service: s.root_service_name,
                            start_time_unix_nano: s.min_start_time_unix_nano,
                            duration_nanos: s
                                .max_end_time_unix_nano
                                .saturating_sub(s.min_start_time_unix_nano),
                            span_count: s.span_count.max(0) as u64,
                            error_count: s.error_span_count.max(0) as u64,
                            services: s.services.into_iter().map(|svc| svc.name).collect(),
                        });
                    }
                }
                out.sort_by_key(|t| std::cmp::Reverse(t.start_time_unix_nano));
                Ok(out)
            }
            // Optional RPC: fall back to FindTraces + client-side aggregation.
            Err(status) if status.code() == Code::Unimplemented => {
                let mut stream = self
                    .reader()
                    .find_traces(jsv2::FindTracesRequest {
                        query: Some(params),
                    })
                    .await
                    .context("jaeger FindTraces")?
                    .into_inner();
                let mut spans = Vec::new();
                while let Some(chunk) = stream.message().await.context("traces stream")? {
                    spans.extend(otlp::spans_from_resource_spans(&chunk.resource_spans));
                }
                Ok(build_trace_summaries(&spans))
            }
            Err(status) => Err(status).context("jaeger FindTraceSummaries"),
        }
    }

    async fn get_trace(&self, trace_id: &str) -> Result<Vec<SpanRecord>> {
        let id_bytes = hex::decode(trace_id).context("trace id must be hex")?;
        let mut stream = self
            .reader()
            .get_traces(jsv2::GetTracesRequest {
                query: vec![jsv2::GetTraceParams {
                    trace_id: id_bytes,
                    start_time: None,
                    end_time: None,
                }],
            })
            .await
            .context("jaeger GetTraces")?
            .into_inner();
        let mut spans = Vec::new();
        while let Some(chunk) = stream.message().await.context("trace stream")? {
            spans.extend(otlp::spans_from_resource_spans(&chunk.resource_spans));
        }
        spans.sort_by_key(|s| s.start_time_unix_nano);
        Ok(spans)
    }

    async fn query_logs(&self, q: LogQuery) -> Result<Vec<LogRecord>> {
        self.fallback.query_logs(q).await
    }

    async fn list_metrics(&self) -> Result<Vec<MetricInfo>> {
        self.fallback.list_metrics().await
    }

    async fn query_metric_series(&self, q: MetricQuery) -> Result<Vec<MetricSeries>> {
        self.fallback.query_metric_series(q).await
    }

    async fn stats(&self) -> Result<StorageStats> {
        let mut stats = self.fallback.stats().await?;
        stats.backend = format!("jaeger ({})", self.endpoint);
        // Span counts live in the remote backend; surface service count from
        // it when reachable so the status line shows something meaningful.
        if let Ok(services) = self.list_services().await {
            stats.services = stats.services.max(services.len() as u64);
        }
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_params_mapping() {
        let q = TraceQuery {
            service: Some("svc".into()),
            operation: Some("op".into()),
            attribute_query: Some("http.method=GET".into()),
            min_duration_nanos: Some(1_500_000_000),
            limit: 50,
            ..Default::default()
        };
        let p = build_query_params(&q);
        assert_eq!(p.service_name, "svc");
        assert_eq!(p.operation_name, "op");
        assert_eq!(p.search_depth, 50);
        assert_eq!(p.attributes.len(), 1);
        assert_eq!(p.attributes[0].key, "http.method");
        let d = p.duration_min.unwrap();
        assert_eq!((d.seconds, d.nanos), (1, 500_000_000));
    }
}
