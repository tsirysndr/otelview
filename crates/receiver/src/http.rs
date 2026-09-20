//! OTLP/HTTP receiver: POST /v1/{traces,logs,metrics} accepting
//! application/x-protobuf and application/json (per the OTLP spec), with
//! optional gzip request encoding and optional header auth.

use std::io::Read;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use otelview_config::Auth;
use otelview_storage::{otlp, DynStorage};
use prost::Message;

#[derive(Clone)]
pub struct HttpState {
    pub storage: DynStorage,
    pub auth: Arc<Auth>,
}

pub fn router(storage: DynStorage, auth: &Auth) -> Router {
    let state = HttpState {
        storage,
        auth: Arc::new(auth.clone()),
    };
    Router::new()
        .route("/v1/traces", post(ingest_traces))
        .route("/v1/logs", post(ingest_logs))
        .route("/v1/metrics", post(ingest_metrics))
        .with_state(state)
}

enum Payload {
    Protobuf(Bytes),
    Json(Bytes),
}

fn unauthorized(msg: String) -> Response {
    (StatusCode::UNAUTHORIZED, msg).into_response()
}

fn bad_request(msg: String) -> Response {
    (StatusCode::BAD_REQUEST, msg).into_response()
}

#[allow(clippy::result_large_err)]
fn check_auth(auth: &Auth, headers: &HeaderMap) -> Result<(), Response> {
    if !auth.enabled() {
        return Ok(());
    }
    let expected = auth.token.as_deref().unwrap_or_default();
    match headers.get(auth.header.as_str()) {
        Some(v) if v.to_str().map(|v| v == expected).unwrap_or(false) => Ok(()),
        _ => Err(unauthorized(format!(
            "missing or invalid {} header",
            auth.header
        ))),
    }
}

#[allow(clippy::result_large_err)]
fn decode_body(headers: &HeaderMap, body: Bytes) -> Result<Payload, Response> {
    let body = match headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
    {
        Some(enc) if enc.eq_ignore_ascii_case("gzip") => {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(body.as_ref())
                .read_to_end(&mut out)
                .map_err(|e| bad_request(format!("invalid gzip body: {e}")))?;
            Bytes::from(out)
        }
        Some(enc) if !enc.is_empty() && !enc.eq_ignore_ascii_case("identity") => {
            return Err(bad_request(format!("unsupported content-encoding {enc}")));
        }
        _ => body,
    };
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/x-protobuf");
    if content_type.starts_with("application/json") {
        Ok(Payload::Json(body))
    } else if content_type.starts_with("application/x-protobuf")
        || content_type.starts_with("application/protobuf")
    {
        Ok(Payload::Protobuf(body))
    } else {
        Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            format!("unsupported content-type {content_type}"),
        )
            .into_response())
    }
}

/// Empty OTLP success response mirroring the request encoding.
fn success(payload: &Payload) -> Response {
    match payload {
        Payload::Json(_) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            "{\"partialSuccess\":{}}",
        )
            .into_response(),
        Payload::Protobuf(_) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/x-protobuf")],
            Bytes::new(),
        )
            .into_response(),
    }
}

#[allow(clippy::result_large_err)]
fn parse<T: Message + Default + serde::de::DeserializeOwned>(
    payload: &Payload,
) -> Result<T, Response> {
    match payload {
        Payload::Protobuf(b) => {
            T::decode(b.as_ref()).map_err(|e| bad_request(format!("invalid protobuf: {e}")))
        }
        Payload::Json(b) => {
            serde_json::from_slice(b).map_err(|e| bad_request(format!("invalid OTLP JSON: {e}")))
        }
    }
}

fn internal(e: anyhow::Error) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response()
}

async fn ingest_traces(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = check_auth(&state.auth, &headers) {
        return resp;
    }
    let payload = match decode_body(&headers, body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let request: ExportTraceServiceRequest = match parse(&payload) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let spans = otlp::spans_from_resource_spans(&request.resource_spans);
    tracing::debug!(count = spans.len(), "ingested spans via HTTP");
    match state.storage.insert_spans(spans).await {
        Ok(()) => success(&payload),
        Err(e) => internal(e),
    }
}

async fn ingest_logs(State(state): State<HttpState>, headers: HeaderMap, body: Bytes) -> Response {
    if let Err(resp) = check_auth(&state.auth, &headers) {
        return resp;
    }
    let payload = match decode_body(&headers, body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let request: ExportLogsServiceRequest = match parse(&payload) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let logs = otlp::logs_from_resource_logs(&request.resource_logs);
    tracing::debug!(count = logs.len(), "ingested logs via HTTP");
    match state.storage.insert_logs(logs).await {
        Ok(()) => success(&payload),
        Err(e) => internal(e),
    }
}

async fn ingest_metrics(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(resp) = check_auth(&state.auth, &headers) {
        return resp;
    }
    let payload = match decode_body(&headers, body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    let request: ExportMetricsServiceRequest = match parse(&payload) {
        Ok(r) => r,
        Err(resp) => return resp,
    };
    let points = otlp::metrics_from_resource_metrics(&request.resource_metrics);
    tracing::debug!(count = points.len(), "ingested metric points via HTTP");
    match state.storage.insert_metrics(points).await {
        Ok(()) => success(&payload),
        Err(e) => internal(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use otelview_config::MemoryConfig;
    use otelview_storage::memory::MemoryStorage;
    use otelview_storage::Storage;
    use tower::ServiceExt;

    fn otlp_json_traces() -> &'static str {
        r#"{
          "resourceSpans": [{
            "resource": {
              "attributes": [{"key": "service.name", "value": {"stringValue": "test-svc"}}]
            },
            "scopeSpans": [{
              "spans": [{
                "traceId": "0102030405060708090a0b0c0d0e0f10",
                "spanId": "0102030405060708",
                "name": "GET /hello",
                "kind": 2,
                "startTimeUnixNano": "1700000000000000000",
                "endTimeUnixNano": "1700000000500000000",
                "status": {"code": 2, "message": "boom"}
              }]
            }]
          }]
        }"#
    }

    #[tokio::test]
    async fn ingests_otlp_json_traces() {
        let storage: DynStorage = Arc::new(MemoryStorage::new(&MemoryConfig::default()));
        let app = router(storage.clone(), &Auth::default());
        let resp = app
            .oneshot(
                Request::post("/v1/traces")
                    .header("content-type", "application/json")
                    .body(Body::from(otlp_json_traces()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let stats = storage.stats().await.unwrap();
        assert_eq!(stats.spans, 1);
        let trace = storage
            .get_trace("0102030405060708090a0b0c0d0e0f10")
            .await
            .unwrap();
        assert_eq!(trace[0].service_name, "test-svc");
        assert_eq!(trace[0].name, "GET /hello");
        assert!(trace[0].is_error());
    }

    #[tokio::test]
    async fn rejects_bad_token_and_accepts_good_one() {
        let storage: DynStorage = Arc::new(MemoryStorage::new(&MemoryConfig::default()));
        let auth = Auth {
            header: "x-otelview-token".into(),
            token: Some("sekret".into()),
            protect_api: false,
        };
        let app = router(storage.clone(), &auth);

        let resp = app
            .clone()
            .oneshot(
                Request::post("/v1/traces")
                    .header("content-type", "application/json")
                    .body(Body::from(otlp_json_traces()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = app
            .oneshot(
                Request::post("/v1/traces")
                    .header("content-type", "application/json")
                    .header("x-otelview-token", "sekret")
                    .body(Body::from(otlp_json_traces()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(storage.stats().await.unwrap().spans, 1);
    }

    #[tokio::test]
    async fn ingests_protobuf_traces() {
        use prost::Message;
        let storage: DynStorage = Arc::new(MemoryStorage::new(&MemoryConfig::default()));
        let app = router(storage.clone(), &Auth::default());

        // Build the request from the JSON fixture to avoid duplicating it.
        let request: ExportTraceServiceRequest = serde_json::from_str(otlp_json_traces()).unwrap();
        let body = request.encode_to_vec();
        let resp = app
            .oneshot(
                Request::post("/v1/traces")
                    .header("content-type", "application/x-protobuf")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        assert_eq!(storage.stats().await.unwrap().spans, 1);
    }
}
