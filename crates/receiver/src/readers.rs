//! Server implementations of the remote-storage read APIs, backed by the
//! local [`Storage`]: `jaeger.storage.v2.TraceReader` plus
//! `otelview.storage.v1.{LogReader, MetricReader, Diagnostics}`.

use std::pin::Pin;

use opentelemetry_proto::tonic::logs::v1::LogsData;
use opentelemetry_proto::tonic::metrics::v1::MetricsData;
use opentelemetry_proto::tonic::trace::v1::TracesData;
use otelview_model::{LogQuery, MetricQuery, TraceQuery};
use otelview_storage::proto::storage::v1 as osv1;
use otelview_storage::proto::storage::v2 as jsv2;
use otelview_storage::{otlp, DynStorage};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

type Stream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send>>;

fn internal(e: anyhow::Error) -> Status {
    Status::internal(format!("{e:#}"))
}

fn timestamp_to_nanos(ts: Option<&prost_types::Timestamp>) -> Option<u64> {
    ts.map(|t| (t.seconds.max(0) as u64) * 1_000_000_000 + t.nanos.max(0) as u64)
}

fn duration_to_nanos(d: Option<&prost_types::Duration>) -> Option<u64> {
    d.map(|d| (d.seconds.max(0) as u64) * 1_000_000_000 + d.nanos.max(0) as u64)
}

fn trace_query_from_params(p: Option<&jsv2::TraceQueryParameters>) -> TraceQuery {
    let Some(p) = p else {
        return TraceQuery {
            limit: 20,
            ..Default::default()
        };
    };
    let attribute_query = p.attributes.first().map(|kv| {
        let value = kv
            .value
            .as_ref()
            .and_then(|v| match &v.value {
                Some(jsv2::any_value::Value::StringValue(s)) => Some(s.clone()),
                Some(other) => Some(format!("{other:?}")),
                None => None,
            })
            .unwrap_or_default();
        format!("{}={}", kv.key, value)
    });
    TraceQuery {
        service: Some(p.service_name.clone()).filter(|s| !s.is_empty()),
        operation: Some(p.operation_name.clone()).filter(|s| !s.is_empty()),
        attribute_query,
        min_duration_nanos: duration_to_nanos(p.duration_min.as_ref()),
        max_duration_nanos: duration_to_nanos(p.duration_max.as_ref()),
        start_time_min_unix_nano: timestamp_to_nanos(p.start_time_min.as_ref()),
        start_time_max_unix_nano: timestamp_to_nanos(p.start_time_max.as_ref()),
        errors_only: false,
        limit: if p.search_depth <= 0 {
            20
        } else {
            p.search_depth as usize
        },
    }
}

/// One-shot stream from an already-collected list of chunks.
fn stream_from<T: Send + 'static>(chunks: Vec<T>) -> Stream<T> {
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        for chunk in chunks {
            if tx.send(Ok(chunk)).await.is_err() {
                break;
            }
        }
    });
    Box::pin(ReceiverStream::new(rx))
}

pub struct TraceReaderService {
    storage: DynStorage,
}

impl TraceReaderService {
    pub fn new(storage: DynStorage) -> Self {
        Self { storage }
    }

    async fn traces_chunks(&self, trace_ids: Vec<String>) -> Result<Vec<TracesData>, Status> {
        let mut chunks = Vec::new();
        for id in trace_ids {
            let spans = self.storage.get_trace(&id).await.map_err(internal)?;
            // Spec: a chunk must not be empty and must hold a single trace.
            if !spans.is_empty() {
                chunks.push(otlp::spans_to_traces_data(&spans));
            }
        }
        Ok(chunks)
    }
}

#[tonic::async_trait]
impl jsv2::trace_reader_server::TraceReader for TraceReaderService {
    type GetTracesStream = Stream<TracesData>;
    type FindSpansStream = Stream<jsv2::FindSpansResponse>;
    type FindTracesStream = Stream<TracesData>;
    type FindTraceSummariesStream = Stream<jsv2::FindTraceSummariesResponse>;

    async fn get_traces(
        &self,
        request: Request<jsv2::GetTracesRequest>,
    ) -> Result<Response<Self::GetTracesStream>, Status> {
        let ids = request
            .into_inner()
            .query
            .into_iter()
            .map(|p| hex::encode(&p.trace_id))
            .collect();
        let chunks = self.traces_chunks(ids).await?;
        Ok(Response::new(stream_from(chunks)))
    }

    async fn get_services(
        &self,
        _request: Request<jsv2::GetServicesRequest>,
    ) -> Result<Response<jsv2::GetServicesResponse>, Status> {
        let services = self.storage.list_services().await.map_err(internal)?;
        Ok(Response::new(jsv2::GetServicesResponse { services }))
    }

    async fn get_operations(
        &self,
        request: Request<jsv2::GetOperationsRequest>,
    ) -> Result<Response<jsv2::GetOperationsResponse>, Status> {
        let service = request.into_inner().service;
        let ops = self
            .storage
            .list_operations(&service)
            .await
            .map_err(internal)?;
        Ok(Response::new(jsv2::GetOperationsResponse {
            operations: ops
                .into_iter()
                .map(|name| jsv2::Operation {
                    name,
                    span_kind: String::new(),
                })
                .collect(),
        }))
    }

    async fn find_spans(
        &self,
        _request: Request<jsv2::FindSpansRequest>,
    ) -> Result<Response<Self::FindSpansStream>, Status> {
        Err(Status::unimplemented("FindSpans is not supported"))
    }

    async fn find_traces(
        &self,
        request: Request<jsv2::FindTracesRequest>,
    ) -> Result<Response<Self::FindTracesStream>, Status> {
        let q = trace_query_from_params(request.into_inner().query.as_ref());
        let summaries = self.storage.find_traces(q).await.map_err(internal)?;
        let ids = summaries.into_iter().map(|s| s.trace_id).collect();
        let chunks = self.traces_chunks(ids).await?;
        Ok(Response::new(stream_from(chunks)))
    }

    async fn find_trace_i_ds(
        &self,
        request: Request<jsv2::FindTraceIDsRequest>,
    ) -> Result<Response<jsv2::FindTraceIDsResponse>, Status> {
        let q = trace_query_from_params(request.into_inner().query.as_ref());
        let summaries = self.storage.find_traces(q).await.map_err(internal)?;
        Ok(Response::new(jsv2::FindTraceIDsResponse {
            trace_ids: summaries
                .into_iter()
                .map(|s| jsv2::FoundTraceId {
                    trace_id: hex::decode(&s.trace_id).unwrap_or_default(),
                    start: None,
                    end: None,
                })
                .collect(),
            next_page_token: String::new(),
        }))
    }

    async fn find_trace_summaries(
        &self,
        request: Request<jsv2::FindTraceSummariesRequest>,
    ) -> Result<Response<Self::FindTraceSummariesStream>, Status> {
        let q = trace_query_from_params(request.into_inner().query.as_ref());
        let summaries = self.storage.find_traces(q).await.map_err(internal)?;
        let response = jsv2::FindTraceSummariesResponse {
            summaries: summaries
                .into_iter()
                .map(|s| jsv2::TraceSummary {
                    trace_id: hex::decode(&s.trace_id).unwrap_or_default(),
                    root_service_name: s.root_service,
                    root_operation_name: s.root_name,
                    min_start_time_unix_nano: s.start_time_unix_nano,
                    max_end_time_unix_nano: s.start_time_unix_nano + s.duration_nanos,
                    span_count: s.span_count as i32,
                    error_span_count: s.error_count as i32,
                    orphan_span_count: 0,
                    services: s
                        .services
                        .into_iter()
                        .map(|name| jsv2::ServiceSummary {
                            name,
                            span_count: 0,
                            error_span_count: 0,
                        })
                        .collect(),
                })
                .collect(),
            next_page_token: String::new(),
        };
        Ok(Response::new(stream_from(vec![response])))
    }
}

pub struct LogReaderService {
    storage: DynStorage,
}

impl LogReaderService {
    pub fn new(storage: DynStorage) -> Self {
        Self { storage }
    }
}

#[tonic::async_trait]
impl osv1::log_reader_server::LogReader for LogReaderService {
    type FindLogsStream = Stream<LogsData>;

    async fn get_services(
        &self,
        _request: Request<osv1::GetServicesRequest>,
    ) -> Result<Response<osv1::GetServicesResponse>, Status> {
        let services = self.storage.list_services().await.map_err(internal)?;
        Ok(Response::new(osv1::GetServicesResponse { services }))
    }

    async fn find_logs(
        &self,
        request: Request<osv1::FindLogsRequest>,
    ) -> Result<Response<Self::FindLogsStream>, Status> {
        let p = request.into_inner().query.unwrap_or_default();
        let q = LogQuery {
            service: Some(p.service_name).filter(|s| !s.is_empty()),
            min_severity: Some(p.min_severity).filter(|s| *s > 0),
            search: Some(p.search).filter(|s| !s.is_empty()),
            trace_id: Some(p.trace_id).filter(|s| !s.is_empty()),
            time_min_unix_nano: timestamp_to_nanos(p.time_min.as_ref()),
            time_max_unix_nano: timestamp_to_nanos(p.time_max.as_ref()),
            limit: if p.search_depth <= 0 {
                200
            } else {
                p.search_depth as usize
            },
        };
        let logs = self.storage.query_logs(q).await.map_err(internal)?;
        let chunks = if logs.is_empty() {
            Vec::new()
        } else {
            vec![otlp::logs_to_logs_data(&logs)]
        };
        Ok(Response::new(stream_from(chunks)))
    }
}

pub struct MetricReaderService {
    storage: DynStorage,
}

impl MetricReaderService {
    pub fn new(storage: DynStorage) -> Self {
        Self { storage }
    }
}

#[tonic::async_trait]
impl osv1::metric_reader_server::MetricReader for MetricReaderService {
    type FindMetricsStream = Stream<MetricsData>;

    async fn get_services(
        &self,
        _request: Request<osv1::GetServicesRequest>,
    ) -> Result<Response<osv1::GetServicesResponse>, Status> {
        let services = self.storage.list_services().await.map_err(internal)?;
        Ok(Response::new(osv1::GetServicesResponse { services }))
    }

    async fn list_metrics(
        &self,
        _request: Request<osv1::ListMetricsRequest>,
    ) -> Result<Response<osv1::ListMetricsResponse>, Status> {
        let infos = self.storage.list_metrics().await.map_err(internal)?;
        Ok(Response::new(osv1::ListMetricsResponse {
            metrics: infos
                .into_iter()
                .map(|m| osv1::MetricInfo {
                    name: m.name,
                    description: m.description,
                    unit: m.unit,
                    metric_type: m.metric_type.as_str().to_string(),
                    services: m.services,
                })
                .collect(),
        }))
    }

    async fn find_metrics(
        &self,
        request: Request<osv1::FindMetricsRequest>,
    ) -> Result<Response<Self::FindMetricsStream>, Status> {
        let p = request.into_inner().query.unwrap_or_default();
        let q = MetricQuery {
            name: p.metric_name,
            service: Some(p.service_name).filter(|s| !s.is_empty()),
            time_min_unix_nano: timestamp_to_nanos(p.time_min.as_ref()),
            time_max_unix_nano: timestamp_to_nanos(p.time_max.as_ref()),
            max_points: if p.max_points <= 0 {
                500
            } else {
                p.max_points as usize
            },
        };
        // Serve raw points reconstructed from the stored series so the
        // client can re-group them however it wants.
        let series = self
            .storage
            .query_metric_series(q.clone())
            .await
            .map_err(internal)?;
        let infos = self.storage.list_metrics().await.map_err(internal)?;
        let info = infos.into_iter().find(|m| m.name == q.name);
        let name = q.name.clone();
        let points: Vec<otelview_model::MetricPoint> =
            series
                .into_iter()
                .flat_map(|s| {
                    let attrs = s.attributes.clone();
                    let service = s.service_name.clone();
                    let info = info.clone();
                    let name = name.clone();
                    s.points.into_iter().map(move |pt| otelview_model::MetricPoint {
                    name: name.clone(),
                    description: info.as_ref().map(|i| i.description.clone()).unwrap_or_default(),
                    unit: info.as_ref().map(|i| i.unit.clone()).unwrap_or_default(),
                    metric_type: info
                        .as_ref()
                        .map(|i| i.metric_type)
                        .unwrap_or(otelview_model::MetricType::Gauge),
                    service_name: service.clone(),
                    time_unix_nano: pt.time_unix_nano,
                    value: pt.value,
                    count: 0,
                    attributes: attrs.clone(),
                    resource_attributes: serde_json::json!({"service.name": service.clone()}),
                    extra: serde_json::json!({}),
                })
                })
                .collect();
        let chunks = if points.is_empty() {
            Vec::new()
        } else {
            vec![otlp::metric_points_to_metrics_data(&points)]
        };
        Ok(Response::new(stream_from(chunks)))
    }
}

pub struct DiagnosticsService {
    storage: DynStorage,
}

impl DiagnosticsService {
    pub fn new(storage: DynStorage) -> Self {
        Self { storage }
    }
}

#[tonic::async_trait]
impl osv1::diagnostics_server::Diagnostics for DiagnosticsService {
    async fn get_stats(
        &self,
        _request: Request<osv1::GetStatsRequest>,
    ) -> Result<Response<osv1::GetStatsResponse>, Status> {
        let s = self.storage.stats().await.map_err(internal)?;
        Ok(Response::new(osv1::GetStatsResponse {
            spans: s.spans,
            logs: s.logs,
            metric_points: s.metric_points,
            services: s.services,
            backend: s.backend,
        }))
    }
}
