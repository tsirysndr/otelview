//! Query API for the web UI, plus the embedded UI assets themselves.
//!
//! Everything the UI needs lives under `/api/*`; any other path falls back to
//! the embedded single-page app.

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use otelview_config::Config;
use otelview_model::{LogQuery, MetricQuery, TraceQuery};
use otelview_storage::DynStorage;
use rust_embed::RustEmbed;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

#[derive(RustEmbed)]
#[folder = "../../ui/dist"]
struct UiAssets;

#[derive(Clone)]
pub struct ApiState {
    pub storage: DynStorage,
    pub config: Arc<Config>,
}

/// Serve the UI + API. Runs until aborted.
pub async fn serve(cfg: &Config, storage: DynStorage) -> Result<()> {
    let addr: std::net::SocketAddr = cfg
        .ui
        .listen
        .parse()
        .with_context(|| format!("invalid ui listen address {}", cfg.ui.listen))?;
    let router = router(cfg, storage);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding UI/API server to {addr}"))?;
    tracing::info!("web UI listening on http://{addr}");
    axum::serve(listener, router).await.context("UI/API server failed")
}

pub fn router(cfg: &Config, storage: DynStorage) -> Router {
    let state = ApiState { storage, config: Arc::new(cfg.clone()) };
    let mut api = Router::new()
        .route("/services", get(services))
        .route("/operations", get(operations))
        .route("/traces", get(traces))
        .route("/traces/{trace_id}", get(trace_detail))
        .route("/logs", get(logs))
        .route("/metrics", get(metrics))
        .route("/metrics/series", get(metric_series))
        .route("/stats", get(stats))
        .route("/config", get(config_view))
        .with_state(state.clone());
    if cfg.auth.enabled() && cfg.auth.protect_api {
        api = api.layer(axum::middleware::from_fn_with_state(state, require_token));
    }
    let mut app = Router::new().nest("/api", api).fallback(static_handler);
    if cfg.ui.cors {
        app = app.layer(CorsLayer::very_permissive());
    }
    app
}

async fn require_token(
    State(state): State<ApiState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let auth = &state.config.auth;
    let expected = auth.token.as_deref().unwrap_or_default();
    // Accept the token either as the configured header or as a Bearer token
    // (browsers and the desktop app use the latter).
    let provided = headers
        .get(auth.header.as_str())
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        });
    if provided == Some(expected) {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, "missing or invalid API token").into_response()
    }
}

fn internal(e: anyhow::Error) -> Response {
    tracing::error!("api error: {e:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response()
}

fn now_unix_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// "30s", "15m", "6h", "7d" or plain seconds.
fn parse_lookback(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() || s == "all" {
        return None;
    }
    let (num, mult) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1u64),
        Some('m') => (&s[..s.len() - 1], 60),
        Some('h') => (&s[..s.len() - 1], 3600),
        Some('d') => (&s[..s.len() - 1], 86_400),
        _ => (s, 1),
    };
    num.parse::<f64>().ok().map(|n| (n * mult as f64 * 1e9) as u64)
}

#[derive(Deserialize)]
struct ServiceParams {
    service: Option<String>,
}

async fn services(State(state): State<ApiState>) -> Response {
    match state.storage.list_services().await {
        Ok(s) => Json(s).into_response(),
        Err(e) => internal(e),
    }
}

async fn operations(
    State(state): State<ApiState>,
    Query(p): Query<ServiceParams>,
) -> Response {
    match state.storage.list_operations(p.service.as_deref().unwrap_or("")).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct TraceParams {
    service: Option<String>,
    operation: Option<String>,
    /// Substring or key=value match over attributes.
    q: Option<String>,
    /// Minimum span duration in milliseconds.
    min_duration_ms: Option<f64>,
    max_duration_ms: Option<f64>,
    /// Lookback window like "15m", "1h", "7d".
    lookback: Option<String>,
    /// Absolute range (unix millis); overrides lookback when set.
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    errors_only: Option<bool>,
    limit: Option<usize>,
}

async fn traces(State(state): State<ApiState>, Query(p): Query<TraceParams>) -> Response {
    let start_time_min_unix_nano = p.start_ms.map(|ms| ms * 1_000_000).or_else(|| {
        p.lookback
            .as_deref()
            .and_then(parse_lookback)
            .map(|window| now_unix_nanos().saturating_sub(window))
    });
    let start_time_max_unix_nano = p.end_ms.map(|ms| ms * 1_000_000);
    let q = TraceQuery {
        service: p.service.filter(|s| !s.is_empty()),
        operation: p.operation.filter(|s| !s.is_empty()),
        attribute_query: p.q.filter(|s| !s.is_empty()),
        min_duration_nanos: p.min_duration_ms.map(|ms| (ms * 1e6) as u64),
        max_duration_nanos: p.max_duration_ms.map(|ms| (ms * 1e6) as u64),
        start_time_min_unix_nano,
        start_time_max_unix_nano,
        errors_only: p.errors_only.unwrap_or(false),
        limit: p.limit.unwrap_or(20).clamp(1, 500),
    };
    match state.storage.find_traces(q).await {
        Ok(t) => Json(t).into_response(),
        Err(e) => internal(e),
    }
}

async fn trace_detail(
    State(state): State<ApiState>,
    Path(trace_id): Path<String>,
) -> Response {
    match state.storage.get_trace(&trace_id).await {
        Ok(spans) if spans.is_empty() => {
            (StatusCode::NOT_FOUND, format!("trace {trace_id} not found")).into_response()
        }
        Ok(spans) => Json(spans).into_response(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct LogParams {
    service: Option<String>,
    min_severity: Option<i32>,
    search: Option<String>,
    trace_id: Option<String>,
    lookback: Option<String>,
    /// Absolute range (unix millis); overrides lookback when set.
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    limit: Option<usize>,
}

async fn logs(State(state): State<ApiState>, Query(p): Query<LogParams>) -> Response {
    let time_min_unix_nano = p.start_ms.map(|ms| ms * 1_000_000).or_else(|| {
        p.lookback
            .as_deref()
            .and_then(parse_lookback)
            .map(|window| now_unix_nanos().saturating_sub(window))
    });
    let time_max_unix_nano = p.end_ms.map(|ms| ms * 1_000_000);
    let q = LogQuery {
        service: p.service.filter(|s| !s.is_empty()),
        min_severity: p.min_severity.filter(|s| *s > 0),
        search: p.search.filter(|s| !s.is_empty()),
        trace_id: p.trace_id.filter(|s| !s.is_empty()),
        time_min_unix_nano,
        time_max_unix_nano,
        limit: p.limit.unwrap_or(200).clamp(1, 5000),
    };
    match state.storage.query_logs(q).await {
        Ok(l) => Json(l).into_response(),
        Err(e) => internal(e),
    }
}

async fn metrics(State(state): State<ApiState>) -> Response {
    match state.storage.list_metrics().await {
        Ok(m) => Json(m).into_response(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct SeriesParams {
    name: String,
    service: Option<String>,
    lookback: Option<String>,
    /// Absolute range (unix millis); overrides lookback when set.
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    max_points: Option<usize>,
}

async fn metric_series(
    State(state): State<ApiState>,
    Query(p): Query<SeriesParams>,
) -> Response {
    let time_min_unix_nano = p.start_ms.map(|ms| ms * 1_000_000).or_else(|| {
        p.lookback
            .as_deref()
            .and_then(parse_lookback)
            .map(|window| now_unix_nanos().saturating_sub(window))
    });
    let time_max_unix_nano = p.end_ms.map(|ms| ms * 1_000_000);
    let q = MetricQuery {
        name: p.name,
        service: p.service.filter(|s| !s.is_empty()),
        time_min_unix_nano,
        time_max_unix_nano,
        max_points: p.max_points.unwrap_or(500).clamp(10, 10_000),
    };
    match state.storage.query_metric_series(q).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => internal(e),
    }
}

async fn stats(State(state): State<ApiState>) -> Response {
    match state.storage.stats().await {
        Ok(s) => Json(s).into_response(),
        Err(e) => internal(e),
    }
}

async fn config_view(State(state): State<ApiState>) -> Response {
    Json(state.config.sanitized()).into_response()
}

/// Embedded SPA: serve the asset if it exists, otherwise index.html.
async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match UiAssets::get(path).or_else(|| UiAssets::get("index.html")) {
        Some(content) => {
            let mime = mime_guess(path);
            ([(header::CONTENT_TYPE, mime)], content.data).into_response()
        }
        None => (StatusCode::NOT_FOUND, "UI assets not embedded in this build").into_response(),
    }
}

fn mime_guess(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json",
        _ => "application/octet-stream",
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
    use serde_json::json;
    use tower::ServiceExt;

    fn test_state() -> (Config, DynStorage) {
        (Config::default(), Arc::new(MemoryStorage::new(&MemoryConfig::default())))
    }

    fn span(trace: &str, svc: &str, start: u64) -> otelview_model::SpanRecord {
        otelview_model::SpanRecord {
            trace_id: trace.into(),
            span_id: "s1".into(),
            parent_span_id: String::new(),
            name: "op".into(),
            service_name: svc.into(),
            kind: "server".into(),
            start_time_unix_nano: start,
            end_time_unix_nano: start + 1_000_000,
            status_code: 0,
            status_message: String::new(),
            attributes: json!({}),
            resource_attributes: json!({}),
            events: json!([]),
            links: json!([]),
            scope_name: String::new(),
            scope_version: String::new(),
        }
    }

    async fn get_json(app: Router, path: &str) -> (StatusCode, serde_json::Value) {
        let resp = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    #[tokio::test]
    async fn traces_and_services_endpoints() {
        let (cfg, storage) = test_state();
        storage.insert_spans(vec![span("t1", "svc-a", 100)]).await.unwrap();
        let app = router(&cfg, storage);

        let (status, v) = get_json(app.clone(), "/api/services").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v, json!(["svc-a"]));

        let (status, v) = get_json(app.clone(), "/api/traces?service=svc-a&limit=5").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v[0]["trace_id"], "t1");

        let (status, v) = get_json(app.clone(), "/api/traces/t1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v[0]["span_id"], "s1");

        let (status, _) = get_json(app, "/api/traces/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn protect_api_enforces_bearer_token() {
        let (mut cfg, storage) = test_state();
        cfg.auth.token = Some("sekret".into());
        cfg.auth.protect_api = true;
        let app = router(&cfg, storage);

        let (status, _) = get_json(app.clone(), "/api/services").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let resp = app
            .oneshot(
                Request::get("/api/services")
                    .header("authorization", "Bearer sekret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn absolute_time_range_filters_traces() {
        let (cfg, storage) = test_state();
        storage
            .insert_spans(vec![
                span("old", "svc", 1_000_000_000),
                span("mid", "svc", 5_000_000_000),
                span("new", "svc", 9_000_000_000),
            ])
            .await
            .unwrap();
        let app = router(&cfg, storage);
        // Window [3s, 7s] in millis picks only the middle trace.
        let (status, v) =
            get_json(app, "/api/traces?start_ms=3000&end_ms=7000&limit=10").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["trace_id"], "mid");
    }

    #[test]
    fn lookback_parsing() {
        assert_eq!(parse_lookback("30s"), Some(30_000_000_000));
        assert_eq!(parse_lookback("15m"), Some(900_000_000_000));
        assert_eq!(parse_lookback("2h"), Some(7_200_000_000_000));
        assert_eq!(parse_lookback("all"), None);
        assert_eq!(parse_lookback("90"), Some(90_000_000_000));
    }
}
