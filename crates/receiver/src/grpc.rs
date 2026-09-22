//! OTLP gRPC ingest services and server-side header auth.

use opentelemetry_proto::tonic::collector::logs::v1::logs_service_server::LogsService;
use opentelemetry_proto::tonic::collector::logs::v1::{
    ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_server::MetricsService;
use opentelemetry_proto::tonic::collector::metrics::v1::{
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::TraceService;
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use otelview_config::Auth;
use otelview_storage::{otlp, DynStorage};
use tonic::{Request, Response, Status};

/// Validates the configured auth header on incoming requests.
#[derive(Clone, Default)]
pub struct ServerAuth {
    header: String,
    token: Option<String>,
}

impl ServerAuth {
    pub fn from_config(auth: &Auth) -> Self {
        Self {
            header: auth.header.to_lowercase(),
            token: auth.token.clone().filter(|t| !t.is_empty()),
        }
    }
}

impl tonic::service::Interceptor for ServerAuth {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        let Some(expected) = &self.token else {
            return Ok(req);
        };
        match req.metadata().get(self.header.as_str()) {
            Some(value) if value.to_str().map(|v| v == expected).unwrap_or(false) => Ok(req),
            _ => Err(Status::unauthenticated(format!(
                "missing or invalid {} header",
                self.header
            ))),
        }
    }
}

fn internal(e: anyhow::Error) -> Status {
    Status::internal(format!("{e:#}"))
}

pub struct TraceReceiver {
    storage: DynStorage,
}

impl TraceReceiver {
    pub fn new(storage: DynStorage) -> Self {
        Self { storage }
    }
}

#[tonic::async_trait]
impl TraceService for TraceReceiver {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        let spans = otlp::spans_from_resource_spans(&request.into_inner().resource_spans);
        let count = spans.len();
        self.storage.insert_spans(spans).await.map_err(internal)?;
        tracing::debug!(count, "ingested spans via gRPC");
        Ok(Response::new(ExportTraceServiceResponse {
            partial_success: None,
        }))
    }
}

pub struct LogsReceiver {
    storage: DynStorage,
}

impl LogsReceiver {
    pub fn new(storage: DynStorage) -> Self {
        Self { storage }
    }
}

#[tonic::async_trait]
impl LogsService for LogsReceiver {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        let logs = otlp::logs_from_resource_logs(&request.into_inner().resource_logs);
        let count = logs.len();
        self.storage.insert_logs(logs).await.map_err(internal)?;
        tracing::debug!(count, "ingested logs via gRPC");
        Ok(Response::new(ExportLogsServiceResponse {
            partial_success: None,
        }))
    }
}

pub struct MetricsReceiver {
    storage: DynStorage,
}

impl MetricsReceiver {
    pub fn new(storage: DynStorage) -> Self {
        Self { storage }
    }
}

#[tonic::async_trait]
impl MetricsService for MetricsReceiver {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        let points = otlp::metrics_from_resource_metrics(&request.into_inner().resource_metrics);
        let count = points.len();
        self.storage
            .insert_metrics(points)
            .await
            .map_err(internal)?;
        tracing::debug!(count, "ingested metric points via gRPC");
        Ok(Response::new(ExportMetricsServiceResponse {
            partial_success: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::service::Interceptor;

    fn req_with_header(key: &'static str, value: &'static str) -> Request<()> {
        let mut req = Request::new(());
        req.metadata_mut().insert(key, value.parse().unwrap());
        req
    }

    #[test]
    fn auth_disabled_allows_everything() {
        let mut auth = ServerAuth::from_config(&Auth::default());
        assert!(auth.call(Request::new(())).is_ok());
    }

    #[test]
    fn auth_enforces_token() {
        let cfg = Auth {
            header: "x-otelview-token".into(),
            token: Some("sekret".into()),
            ..Default::default()
        };
        let mut auth = ServerAuth::from_config(&cfg);
        assert!(auth.call(Request::new(())).is_err());
        assert!(auth
            .call(req_with_header("x-otelview-token", "wrong"))
            .is_err());
        assert!(auth
            .call(req_with_header("x-otelview-token", "sekret"))
            .is_ok());
    }
}
