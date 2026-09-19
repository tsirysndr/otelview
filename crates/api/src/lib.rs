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

pub mod analytics;
pub mod kql;
pub mod seriesfns;

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
        .route("/services/stats", get(service_stats_handler))
        .route("/service-graph", get(service_graph_handler))
        .route("/logs/histogram", get(log_histogram_handler))
        .route("/logs/fields", get(log_fields_handler))
        .route("/traces/fields", get(trace_fields_handler))
        .route("/config", get(config_view))
        .with_state(state.clone());
    if cfg.ui.auth_enabled() || (cfg.auth.enabled() && cfg.auth.protect_api) {
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
    // The UI token guards the query API when set; otherwise fall back to the
    // ingest token (protect_api mode).
    let expected = state
        .config
        .ui
        .token
        .as_deref()
        .filter(|t| !t.is_empty())
        .or(auth.token.as_deref())
        .unwrap_or_default();
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
    /// KQL query, e.g. `http.method:POST and status_code:>=500`.
    kql: Option<String>,
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
    let kql_expr = match p.kql.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(q) => match kql::parse(q) {
            Ok(expr) => expr,
            Err(e) => {
                return (StatusCode::BAD_REQUEST, format!("invalid KQL query: {e}"))
                    .into_response()
            }
        },
        None => None,
    };
    let limit = p.limit.unwrap_or(200).clamp(1, 5000);
    let q = LogQuery {
        service: p.service.filter(|s| !s.is_empty()),
        min_severity: p.min_severity.filter(|s| *s > 0),
        search: p.search.filter(|s| !s.is_empty()),
        trace_id: p.trace_id.filter(|s| !s.is_empty()),
        time_min_unix_nano,
        time_max_unix_nano,
        // With a KQL filter, over-fetch and filter down to the limit.
        limit: if kql_expr.is_some() { 5000 } else { limit },
    };
    match state.storage.query_logs(q).await {
        Ok(mut l) => {
            if let Some(expr) = kql_expr {
                l.retain(|log| kql::eval(&expr, log));
                l.truncate(limit);
            }
            Json(l).into_response()
        }
        Err(e) => internal(e),
    }
}

/// Kibana-style field discovery: flattened attribute keys with counts and
/// top values, from a sample of matching logs.
async fn log_fields_handler(
    State(state): State<ApiState>,
    Query(p): Query<LogParams>,
) -> Response {
    let time_min = p
        .lookback
        .as_deref()
        .and_then(parse_lookback)
        .map(|window| now_unix_nanos().saturating_sub(window));
    let q = LogQuery {
        service: p.service.filter(|s| !s.is_empty()),
        min_severity: p.min_severity.filter(|s| *s > 0),
        search: None,
        trace_id: None,
        time_min_unix_nano: p.start_ms.map(|ms| ms * 1_000_000).or(time_min),
        time_max_unix_nano: p.end_ms.map(|ms| ms * 1_000_000),
        limit: 2000,
    };
    let logs = match state.storage.query_logs(q).await {
        Ok(l) => l,
        Err(e) => return internal(e),
    };
    let mut kvs: Vec<(String, String)> = Vec::new();
    for l in &logs {
        kvs.push(("service".into(), l.service_name.clone()));
        kvs.push((
            "level".into(),
            otelview_model::severity_level(l.severity_number).to_string(),
        ));
        if !l.scope_name.is_empty() {
            kvs.push(("scope".into(), l.scope_name.clone()));
        }
        analytics::flatten_json("", &l.attributes, &mut kvs);
        analytics::flatten_json("", &l.resource_attributes, &mut kvs);
    }
    Json(analytics::summarize_fields(kvs.into_iter(), 50)).into_response()
}

#[derive(Deserialize)]
struct TraceFieldsParams {
    service: Option<String>,
    lookback: Option<String>,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
}

/// Span/resource attribute discovery for the traces attribute filter.
async fn trace_fields_handler(
    State(state): State<ApiState>,
    Query(p): Query<TraceFieldsParams>,
) -> Response {
    let min = p.start_ms.map(|ms| ms * 1_000_000).or_else(|| {
        p.lookback
            .as_deref()
            .and_then(parse_lookback)
            .map(|w| now_unix_nanos().saturating_sub(w))
    });
    let max = p.end_ms.map(|ms| ms * 1_000_000);
    match analytics::trace_fields(&state.storage, p.service.filter(|s| !s.is_empty()), min, max)
        .await
    {
        Ok(v) => Json(v).into_response(),
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
    /// Per-series function: raw | rate | increase.
    func: Option<String>,
    /// Cross-series aggregation: none | sum | avg | min | max.
    agg: Option<String>,
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
        Ok(mut series) => {
            if let Some(func) = p.func.as_deref() {
                seriesfns::apply_function(&mut series, func);
            }
            if let Some(agg) = p.agg.as_deref() {
                series = seriesfns::aggregate(series, agg, 120);
            }
            Json(series).into_response()
        }
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct WindowParams {
    lookback: Option<String>,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
}

impl WindowParams {
    fn bounds(&self) -> (Option<u64>, Option<u64>) {
        let min = self.start_ms.map(|ms| ms * 1_000_000).or_else(|| {
            self.lookback
                .as_deref()
                .and_then(parse_lookback)
                .map(|w| now_unix_nanos().saturating_sub(w))
        });
        (min, self.end_ms.map(|ms| ms * 1_000_000))
    }
}

async fn service_stats_handler(
    State(state): State<ApiState>,
    Query(p): Query<WindowParams>,
) -> Response {
    let (min, max) = p.bounds();
    match analytics::service_stats(&state.storage, min, max).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
}

async fn service_graph_handler(
    State(state): State<ApiState>,
    Query(p): Query<WindowParams>,
) -> Response {
    let (min, max) = p.bounds();
    match analytics::service_graph(&state.storage, min, max).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct LogHistogramParams {
    service: Option<String>,
    min_severity: Option<i32>,
    search: Option<String>,
    kql: Option<String>,
    lookback: Option<String>,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    buckets: Option<usize>,
}

async fn log_histogram_handler(
    State(state): State<ApiState>,
    Query(p): Query<LogHistogramParams>,
) -> Response {
    let time_min = p.start_ms.map(|ms| ms * 1_000_000).or_else(|| {
        p.lookback
            .as_deref()
            .and_then(parse_lookback)
            .map(|w| now_unix_nanos().saturating_sub(w))
    });
    let time_max = p.end_ms.map(|ms| ms * 1_000_000);
    let q = LogQuery {
        service: p.service.filter(|s| !s.is_empty()),
        min_severity: p.min_severity.filter(|s| *s > 0),
        search: p.search.filter(|s| !s.is_empty()),
        trace_id: None,
        time_min_unix_nano: time_min,
        time_max_unix_nano: time_max,
        limit: 0,
    };
    let kql_expr = match p.kql.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(query) => match kql::parse(query) {
            Ok(expr) => expr,
            Err(e) => {
                return (StatusCode::BAD_REQUEST, format!("invalid KQL query: {e}"))
                    .into_response()
            }
        },
        None => None,
    };
    match analytics::log_histogram(
        &state.storage,
        q,
        p.buckets.unwrap_or(40),
        time_min,
        time_max,
        kql_expr.as_ref(),
    )
    .await
    {
        Ok(v) => Json(v).into_response(),
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
    async fn ui_token_gates_the_api() {
        let (mut cfg, storage) = test_state();
        cfg.ui.token = Some("ui-sekret".into());
        let app = router(&cfg, storage);

        let (status, _) = get_json(app.clone(), "/api/services").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Static shell stays public so the login screen can load.
        let resp = app
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app
            .oneshot(
                Request::get("/api/services")
                    .header("authorization", "Bearer ui-sekret")
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

    fn span_kv(
        trace: &str,
        id: &str,
        parent: &str,
        svc: &str,
        start: u64,
        dur: u64,
        err: bool,
    ) -> otelview_model::SpanRecord {
        let mut s = span(trace, svc, start);
        s.span_id = id.into();
        s.parent_span_id = parent.into();
        s.end_time_unix_nano = start + dur;
        s.status_code = if err { 2 } else { 0 };
        s
    }

    #[tokio::test]
    async fn service_stats_and_graph() {
        let (cfg, storage) = test_state();
        storage
            .insert_spans(vec![
                span_kv("t1", "a", "", "gateway", 1_000, 10_000_000, false),
                span_kv("t1", "b", "a", "db", 2_000, 4_000_000, true),
                span_kv("t2", "c", "", "gateway", 9_000, 20_000_000, false),
                span_kv("t2", "d", "c", "db", 9_500, 2_000_000, false),
            ])
            .await
            .unwrap();
        let app = router(&cfg, storage);

        let (status, v) = get_json(app.clone(), "/api/services/stats").await;
        assert_eq!(status, StatusCode::OK);
        let stats = v.as_array().unwrap();
        assert_eq!(stats.len(), 2);
        let db = stats.iter().find(|s| s["service"] == "db").unwrap();
        assert_eq!(db["span_count"], 2);
        assert_eq!(db["error_count"], 1);

        let (status, v) = get_json(app, "/api/service-graph").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["edges"][0]["source"], "gateway");
        assert_eq!(v["edges"][0]["target"], "db");
        assert_eq!(v["edges"][0]["calls"], 2);
        assert_eq!(v["edges"][0]["errors"], 1);
        assert_eq!(v["sampled_traces"], 2);
    }

    #[tokio::test]
    async fn log_histogram_buckets_by_severity() {
        let (cfg, storage) = test_state();
        let mk = |t: u64, sev: i32| otelview_model::LogRecord {
            time_unix_nano: t,
            observed_time_unix_nano: t,
            severity_number: sev,
            severity_text: String::new(),
            body: serde_json::json!("x"),
            attributes: serde_json::json!({}),
            resource_attributes: serde_json::json!({}),
            service_name: "svc".into(),
            trace_id: String::new(),
            span_id: String::new(),
            scope_name: String::new(),
        };
        storage
            .insert_logs(vec![mk(1_000, 9), mk(2_000, 17), mk(500_000, 17)])
            .await
            .unwrap();
        let app = router(&cfg, storage);
        let (status, v) = get_json(app, "/api/logs/histogram?buckets=5").await;
        assert_eq!(status, StatusCode::OK);
        let buckets = v.as_array().unwrap();
        assert_eq!(buckets.len(), 5);
        let total_err: u64 =
            buckets.iter().map(|b| b["error"].as_u64().unwrap()).sum();
        let total_info: u64 =
            buckets.iter().map(|b| b["info"].as_u64().unwrap()).sum();
        assert_eq!(total_err, 2);
        assert_eq!(total_info, 1);
    }

    #[tokio::test]
    async fn kql_filters_logs() {
        let (cfg, storage) = test_state();
        let mk = |sev: i32, method: &str, code: i64| otelview_model::LogRecord {
            time_unix_nano: 1_000,
            observed_time_unix_nano: 1_000,
            severity_number: sev,
            severity_text: String::new(),
            body: serde_json::json!("req done"),
            attributes: serde_json::json!({"http.method": method, "http.status_code": code}),
            resource_attributes: serde_json::json!({}),
            service_name: "svc".into(),
            trace_id: String::new(),
            span_id: String::new(),
            scope_name: String::new(),
        };
        storage
            .insert_logs(vec![mk(9, "GET", 200), mk(17, "POST", 500), mk(9, "POST", 201)])
            .await
            .unwrap();
        let app = router(&cfg, storage);

        let (status, v) = get_json(
            app.clone(),
            "/api/logs?kql=http.method%3APOST%20and%20http.status_code%3A%3E%3D500",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["severity_number"], 17);

        let (status, _) = get_json(app.clone(), "/api/logs?kql=(bad").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, v) = get_json(app, "/api/logs/fields").await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> =
            v.as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"http.method"));
        assert!(names.contains(&"level"));
    }

    #[tokio::test]
    async fn trace_fields_discovers_attributes() {
        let (cfg, storage) = test_state();
        let mut sp = span("t1", "svc", 100);
        sp.attributes = serde_json::json!({"http.method": "GET", "http.route": "/x"});
        sp.resource_attributes = serde_json::json!({"host.name": "app-1"});
        storage.insert_spans(vec![sp]).await.unwrap();
        let app = router(&cfg, storage);
        let (status, v) = get_json(app, "/api/traces/fields").await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> =
            v.as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"http.method"));
        assert!(names.contains(&"host.name"));
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
