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

/// Reshape OTLP/JSON into the subset opentelemetry-proto's serde accepts.
///
/// The generated deserializers diverge from the OTLP/JSON spec in ways that
/// fail *silently*: a data point's `data` is a flattened oneof, so a single
/// unparseable field makes the whole oneof deserialize to `None` and the
/// metric disappears rather than erroring.
///
/// 1. `asInt` is spec'd as a *string* (JSON cannot hold int64 exactly), but
///    only a JSON number is accepted — so every integer counter sent as
///    OTLP/JSON was silently stored as zero.
/// 2. An exemplar's value must be nested under `value`, where the spec
///    flattens it onto the exemplar as `asDouble`/`asInt`.
/// 3. Exemplars need `filteredAttributes`, `traceId` and `spanId` present,
///    which senders may legitimately omit when empty — and omitting them
///    took the entire metric down with them.
fn normalize_otlp_json(v: &mut serde_json::Value) {
    use serde_json::{Map, Value};
    match v {
        Value::Array(items) => items.iter_mut().for_each(normalize_otlp_json),
        Value::Object(map) => {
            if let Some(Value::String(s)) = map.get("asInt") {
                if let Ok(n) = s.parse::<i64>() {
                    map.insert("asInt".into(), Value::Number(n.into()));
                }
            }
            if let Some(Value::Array(exemplars)) = map.get_mut("exemplars") {
                for e in exemplars.iter_mut() {
                    let Some(obj) = e.as_object_mut() else { continue };
                    obj.entry("filteredAttributes")
                        .or_insert_with(|| Value::Array(Vec::new()));
                    obj.entry("traceId")
                        .or_insert_with(|| Value::String(String::new()));
                    obj.entry("spanId")
                        .or_insert_with(|| Value::String(String::new()));
                    if !obj.contains_key("value") {
                        let flat = ["asDouble", "asInt"]
                            .iter()
                            .find_map(|k| obj.remove(*k).map(|val| ((*k).to_string(), val)));
                        if let Some((k, val)) = flat {
                            let val = match (&k[..], &val) {
                                ("asInt", Value::String(s)) => s
                                    .parse::<i64>()
                                    .map(|n| Value::Number(n.into()))
                                    .unwrap_or(val),
                                _ => val,
                            };
                            let mut nested = Map::new();
                            nested.insert(k, val);
                            obj.insert("value".into(), Value::Object(nested));
                        }
                    }
                }
            }
            map.values_mut().for_each(normalize_otlp_json);
        }
        _ => {}
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
            let mut v: serde_json::Value = serde_json::from_slice(b)
                .map_err(|e| bad_request(format!("invalid OTLP JSON: {e}")))?;
            normalize_otlp_json(&mut v);
            serde_json::from_value(v).map_err(|e| bad_request(format!("invalid OTLP JSON: {e}")))
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
    use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
    use opentelemetry_proto::tonic::metrics::v1::metric::Data;

    fn metrics_from_json(js: &'static str) -> ExportMetricsServiceRequest {
        super::parse(&super::Payload::Json(bytes::Bytes::from_static(js.as_bytes()))).unwrap()
    }

    /// OTLP/JSON sends int64 as a string. Accepting only numbers meant every
    /// integer counter was silently ingested as zero.
    #[test]
    fn json_int_counters_are_not_silently_zero() {
        let req = metrics_from_json(
            r#"{"resourceMetrics":[{"resource":{"attributes":[]},"scopeMetrics":[{"scope":{},
              "metrics":[{"name":"http.server.requests","unit":"1","sum":{
                "aggregationTemporality":2,"isMonotonic":true,
                "dataPoints":[{"timeUnixNano":"7","asInt":"1234"}]}}]}]}]}"#,
        );
        let s = match &req.resource_metrics[0].scope_metrics[0].metrics[0].data {
            Some(Data::Sum(s)) => s,
            other => panic!("the metric was dropped entirely: {other:?}"),
        };
        use opentelemetry_proto::tonic::metrics::v1::number_data_point::Value as NV;
        assert_eq!(s.data_points[0].value, Some(NV::AsInt(1234)));
    }

    /// Exemplars carry the only real metric->trace link; the spec flattens
    /// their value and lets empty fields be omitted.
    #[test]
    fn json_exemplars_survive_spec_shaped_payloads() {
        let req = metrics_from_json(
            r#"{"resourceMetrics":[{"resource":{"attributes":[]},"scopeMetrics":[{"scope":{},
              "metrics":[{"name":"http.server.duration","unit":"ms","histogram":{
                "aggregationTemporality":2,"dataPoints":[{"timeUnixNano":"7","count":"3","sum":1.0,
                "bucketCounts":["1","2"],"explicitBounds":[10],
                "exemplars":[{"timeUnixNano":"7","asDouble":4.5,
                  "traceId":"aabbccddeeff00112233445566778899",
                  "spanId":"0011223344556677"}]}]}}]}]}]}"#,
        );
        let h = match &req.resource_metrics[0].scope_metrics[0].metrics[0].data {
            Some(Data::Histogram(h)) => h,
            other => panic!("the metric was dropped entirely: {other:?}"),
        };
        let ex = &h.data_points[0].exemplars;
        assert_eq!(ex.len(), 1);
        use opentelemetry_proto::tonic::metrics::v1::exemplar::Value as EV;
        assert_eq!(ex[0].value, Some(EV::AsDouble(4.5)));
        assert_eq!(hex::encode(&ex[0].trace_id), "aabbccddeeff00112233445566778899");
        assert_eq!(hex::encode(&ex[0].span_id), "0011223344556677");
    }

    /// An exemplar with an int value, and with the optional fields omitted.
    #[test]
    fn json_exemplar_with_int_value_and_no_ids() {
        let req = metrics_from_json(
            r#"{"resourceMetrics":[{"resource":{"attributes":[]},"scopeMetrics":[{"scope":{},
              "metrics":[{"name":"m","unit":"1","gauge":{
                "dataPoints":[{"timeUnixNano":"7","asDouble":1.0,
                "exemplars":[{"timeUnixNano":"7","asInt":"99"}]}]}}]}]}]}"#,
        );
        let g = match &req.resource_metrics[0].scope_metrics[0].metrics[0].data {
            Some(Data::Gauge(g)) => g,
            other => panic!("the metric was dropped entirely: {other:?}"),
        };
        use opentelemetry_proto::tonic::metrics::v1::exemplar::Value as EV;
        assert_eq!(g.data_points[0].exemplars[0].value, Some(EV::AsInt(99)));
    }


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
