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
use otelview_storage::DynStorage;
use query::{LogSearch, QueryError, SeriesSearch, TraceSearch, Window, DEFAULT_HISTOGRAM_BUCKETS};
use rust_embed::RustEmbed;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

pub mod analytics;
pub mod kql;
pub mod lucene;
pub mod query;
pub mod seriesfns;
pub mod traceql;

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
    serve_with(cfg, storage, None).await
}

/// Serve the UI + API with `extra` merged in at the root.
///
/// That is how the MCP endpoint gets mounted without this crate having to
/// know what MCP is — the mcp crate depends on this one, so the arrow
/// cannot point the other way.
///
/// Separate from [`serve`] rather than an argument on it: the desktop shell
/// embeds a server too, and it lives outside this workspace where a changed
/// signature is found by CI rather than by the compiler here.
pub async fn serve_with(cfg: &Config, storage: DynStorage, extra: Option<Router>) -> Result<()> {
    let addr: std::net::SocketAddr = cfg
        .ui
        .listen
        .parse()
        .with_context(|| format!("invalid ui listen address {}", cfg.ui.listen))?;
    let router = router_with(cfg, storage, extra);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding UI/API server to {addr}"))?;
    tracing::info!("web UI listening on http://{addr}");
    axum::serve(listener, router)
        .await
        .context("UI/API server failed")
}

pub fn router(cfg: &Config, storage: DynStorage) -> Router {
    router_with(cfg, storage, None)
}

pub fn router_with(cfg: &Config, storage: DynStorage, extra: Option<Router>) -> Router {
    let state = ApiState {
        storage,
        config: Arc::new(cfg.clone()),
    };
    let mut api = Router::new()
        .route("/services", get(services))
        .route("/operations", get(operations))
        .route("/traces", get(traces))
        .route("/traces/{trace_id}", get(trace_detail))
        .route("/logs", get(logs))
        .route("/metrics", get(metrics))
        .route("/metrics/series", get(metric_series))
        .route("/metrics/exemplars", get(metric_exemplars))
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
    // `extra` carries its own auth: it is also served standalone, where
    // this router isn't in the picture, so the guard has to travel with it.
    let mut app = Router::new().nest("/api", api);
    if let Some(extra) = extra {
        app = app.merge(extra);
    }
    let mut app = app.fallback(static_handler);
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

/// A mistyped query is the user's to fix (400); a storage failure is ours
/// (500). Both arrive here as one error, so the split happens once.
fn query_failed(e: QueryError) -> Response {
    match e {
        QueryError::BadQuery(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
        QueryError::Backend(e) => internal(e),
    }
}

/// Serialize a query result, or turn its error into a response.
fn query_json<T: serde::Serialize>(r: Result<T, QueryError>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => query_failed(e),
    }
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

async fn operations(State(state): State<ApiState>, Query(p): Query<ServiceParams>) -> Response {
    match state
        .storage
        .list_operations(p.service.as_deref().unwrap_or(""))
        .await
    {
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
    /// TraceQL query, e.g. `{ status = error && duration > 100ms }`.
    traceql: Option<String>,
    /// Lucene query, e.g. `service:gateway AND http.status_code:[500 TO *]`.
    lucene: Option<String>,
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

impl From<TraceParams> for TraceSearch {
    fn from(p: TraceParams) -> Self {
        TraceSearch {
            service: p.service,
            operation: p.operation,
            q: p.q,
            traceql: p.traceql,
            lucene: p.lucene,
            min_duration_ms: p.min_duration_ms,
            max_duration_ms: p.max_duration_ms,
            errors_only: p.errors_only,
            window: Window {
                lookback: p.lookback,
                start_ms: p.start_ms,
                end_ms: p.end_ms,
            },
            limit: p.limit,
        }
    }
}

async fn traces(State(state): State<ApiState>, Query(p): Query<TraceParams>) -> Response {
    query_json(query::search_traces(&state.storage, p.into()).await)
}

async fn trace_detail(State(state): State<ApiState>, Path(trace_id): Path<String>) -> Response {
    match state.storage.get_trace(&trace_id).await {
        Ok(spans) if spans.is_empty() => {
            (StatusCode::NOT_FOUND, format!("trace {trace_id} not found")).into_response()
        }
        Ok(spans) => Json(spans).into_response(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
struct ExemplarParams {
    trace_id: String,
    /// Narrow to one span within the trace.
    span_id: Option<String>,
    limit: Option<usize>,
}

/// Metrics carrying an exemplar that points at a trace or span — the only
/// link between a metric and a trace that the data actually supports.
async fn metric_exemplars(
    State(state): State<ApiState>,
    Query(p): Query<ExemplarParams>,
) -> Response {
    match query::find_exemplars(&state.storage, &p.trace_id, p.span_id.as_deref(), p.limit).await {
        Ok(hits) => Json(hits).into_response(),
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
    /// Lucene query, e.g. `level:ERROR AND "connection refused"`.
    lucene: Option<String>,
    trace_id: Option<String>,
    lookback: Option<String>,
    /// Absolute range (unix millis); overrides lookback when set.
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    limit: Option<usize>,
}

impl From<LogParams> for LogSearch {
    fn from(p: LogParams) -> Self {
        LogSearch {
            service: p.service,
            min_severity: p.min_severity,
            search: p.search,
            kql: p.kql,
            lucene: p.lucene,
            trace_id: p.trace_id,
            window: Window {
                lookback: p.lookback,
                start_ms: p.start_ms,
                end_ms: p.end_ms,
            },
            limit: p.limit,
        }
    }
}

async fn logs(State(state): State<ApiState>, Query(p): Query<LogParams>) -> Response {
    query_json(query::search_logs(&state.storage, p.into()).await)
}

/// Kibana-style field discovery: flattened attribute keys with counts and
/// top values, from a sample of matching logs.
async fn log_fields_handler(State(state): State<ApiState>, Query(p): Query<LogParams>) -> Response {
    let p: LogSearch = p.into();
    let (min, max) = p.window.bounds();
    match analytics::log_fields(&state.storage, p.service, p.min_severity, min, max).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => internal(e),
    }
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
    let (min, max) = Window {
        lookback: p.lookback,
        start_ms: p.start_ms,
        end_ms: p.end_ms,
    }
    .bounds();
    match analytics::trace_fields(
        &state.storage,
        p.service.filter(|s| !s.is_empty()),
        min,
        max,
    )
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

impl From<SeriesParams> for SeriesSearch {
    fn from(p: SeriesParams) -> Self {
        SeriesSearch {
            name: p.name,
            service: p.service,
            func: p.func,
            agg: p.agg,
            window: Window {
                lookback: p.lookback,
                start_ms: p.start_ms,
                end_ms: p.end_ms,
            },
            max_points: p.max_points,
        }
    }
}

async fn metric_series(State(state): State<ApiState>, Query(p): Query<SeriesParams>) -> Response {
    query_json(query::metric_series(&state.storage, p.into()).await)
}

#[derive(Deserialize)]
struct WindowParams {
    lookback: Option<String>,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
}

impl WindowParams {
    fn bounds(&self) -> (Option<u64>, Option<u64>) {
        Window {
            lookback: self.lookback.clone(),
            start_ms: self.start_ms,
            end_ms: self.end_ms,
        }
        .bounds()
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
    lucene: Option<String>,
    lookback: Option<String>,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    buckets: Option<usize>,
}

/// The histogram shares the log search's shape so the chart and the list
/// under it always filter on the same thing.
impl From<LogHistogramParams> for LogSearch {
    fn from(p: LogHistogramParams) -> Self {
        LogSearch {
            service: p.service,
            min_severity: p.min_severity,
            search: p.search,
            kql: p.kql,
            lucene: p.lucene,
            trace_id: None,
            window: Window {
                lookback: p.lookback,
                start_ms: p.start_ms,
                end_ms: p.end_ms,
            },
            limit: None,
        }
    }
}

async fn log_histogram_handler(
    State(state): State<ApiState>,
    Query(p): Query<LogHistogramParams>,
) -> Response {
    let buckets = p.buckets.unwrap_or(DEFAULT_HISTOGRAM_BUCKETS);
    query_json(query::log_histogram(&state.storage, p.into(), buckets).await)
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
    let asset = UiAssets::get(path).map(|content| (path, content));
    let (served, content) =
        match asset.or_else(|| UiAssets::get("index.html").map(|c| ("index.html", c))) {
            Some(found) => found,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    "UI assets not embedded in this build",
                )
                    .into_response()
            }
        };

    // Without explicit caching the browser falls back to heuristics, and a
    // deployed UI change shows up whenever the browser feels like it — the
    // symptom being an old bundle after an upgrade until a hard refresh.
    // Vite hashes every file under assets/ by content, so those are safe to
    // cache forever; the entry document is what names them, so it must be
    // revalidated on every load.
    let cache = if served.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, mime_guess(served)),
            (header::CACHE_CONTROL, cache),
        ],
        content.data,
    )
        .into_response()
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
    use serde_json::json;
    use tower::ServiceExt;

    fn test_state() -> (Config, DynStorage) {
        (
            Config::default(),
            Arc::new(MemoryStorage::new(&MemoryConfig::default())),
        )
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

    /// For the error paths, where the body is a plain message rather than JSON.
    async fn get_text(app: Router, path: &str) -> (StatusCode, String) {
        let resp = app
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn traces_and_services_endpoints() {
        let (cfg, storage) = test_state();
        storage
            .insert_spans(vec![span("t1", "svc-a", 100)])
            .await
            .unwrap();
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
        let (status, v) = get_json(app, "/api/traces?start_ms=3000&end_ms=7000&limit=10").await;
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
        let total_err: u64 = buckets.iter().map(|b| b["error"].as_u64().unwrap()).sum();
        let total_info: u64 = buckets.iter().map(|b| b["info"].as_u64().unwrap()).sum();
        assert_eq!(total_err, 2);
        assert_eq!(total_info, 1);
    }

    /// A backend that caps every log query at its own search depth,
    /// regardless of the limit asked for — the postgres storage does exactly
    /// this (MAX_SEARCH_DEPTH, default 1000). The histogram pager must not
    /// read a clamped-but-full page as the end of the window: in production
    /// that stopped the walk after the newest thousand logs, and the chart
    /// was back to a single bar.
    struct ClampedStorage {
        inner: MemoryStorage,
        cap: usize,
    }

    #[async_trait::async_trait]
    impl otelview_storage::Storage for ClampedStorage {
        async fn insert_spans(&self, s: Vec<otelview_model::SpanRecord>) -> anyhow::Result<()> {
            self.inner.insert_spans(s).await
        }
        async fn insert_logs(&self, l: Vec<otelview_model::LogRecord>) -> anyhow::Result<()> {
            self.inner.insert_logs(l).await
        }
        async fn insert_metrics(&self, p: Vec<otelview_model::MetricPoint>) -> anyhow::Result<()> {
            self.inner.insert_metrics(p).await
        }
        async fn list_services(&self) -> anyhow::Result<Vec<String>> {
            self.inner.list_services().await
        }
        async fn list_operations(&self, s: &str) -> anyhow::Result<Vec<String>> {
            self.inner.list_operations(s).await
        }
        async fn find_traces(
            &self,
            q: otelview_model::TraceQuery,
        ) -> anyhow::Result<Vec<otelview_model::TraceSummary>> {
            self.inner.find_traces(q).await
        }
        async fn get_trace(&self, t: &str) -> anyhow::Result<Vec<otelview_model::SpanRecord>> {
            self.inner.get_trace(t).await
        }
        async fn query_logs(
            &self,
            mut q: otelview_model::LogQuery,
        ) -> anyhow::Result<Vec<otelview_model::LogRecord>> {
            q.limit = if q.limit == 0 {
                self.cap
            } else {
                q.limit.min(self.cap)
            };
            self.inner.query_logs(q).await
        }
        async fn list_metrics(&self) -> anyhow::Result<Vec<otelview_model::MetricInfo>> {
            self.inner.list_metrics().await
        }
        async fn query_metric_series(
            &self,
            q: otelview_model::MetricQuery,
        ) -> anyhow::Result<Vec<otelview_model::MetricSeries>> {
            self.inner.query_metric_series(q).await
        }
        async fn stats(&self) -> anyhow::Result<otelview_model::StorageStats> {
            self.inner.stats().await
        }
    }

    #[tokio::test]
    async fn log_histogram_survives_a_backend_that_clamps_page_sizes() {
        let cfg = Config::default();
        let storage: DynStorage = Arc::new(ClampedStorage {
            inner: MemoryStorage::new(&MemoryConfig::default()),
            cap: 1_000,
        });

        // Well past the clamp, spread over a known window.
        const COUNT: u64 = 3_500;
        let logs: Vec<_> = (0..COUNT)
            .map(|i| otelview_model::LogRecord {
                time_unix_nano: 1_000_000_000 + i * 1_000_000,
                observed_time_unix_nano: 1_000_000_000 + i * 1_000_000,
                severity_number: 9,
                severity_text: String::new(),
                body: serde_json::json!("x"),
                attributes: serde_json::json!({}),
                resource_attributes: serde_json::json!({}),
                service_name: "svc".into(),
                trace_id: String::new(),
                span_id: String::new(),
                scope_name: String::new(),
            })
            .collect();
        storage.insert_logs(logs).await.unwrap();

        let app = router(&cfg, storage);
        let (status, v) = get_json(
            app,
            "/api/logs/histogram?buckets=7&start_ms=1000&end_ms=4500",
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let total: u64 = v
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["info"].as_u64().unwrap())
            .sum();
        assert_eq!(
            total, COUNT,
            "the pager must walk past the backend's clamp, not stop at it"
        );
    }

    /// Every bucket in the window must be counted, not just the newest page.
    ///
    /// The histogram used to read one capped query of the newest logs while
    /// bucketing across the whole window, so a busy interval rendered as bars
    /// over its tail and zeros everywhere older. 6_000 logs is past the old
    /// 5_000 cap, so on that code the oldest buckets come back empty and the
    /// total is short.
    #[tokio::test]
    async fn log_histogram_counts_the_whole_window_not_just_the_newest_page() {
        let (cfg, storage) = test_state();

        // One log per millisecond from 1_000ms to 6_999ms.
        const COUNT: u64 = 6_000;
        const BASE_NANOS: u64 = 1_000_000_000;
        const STEP_NANOS: u64 = 1_000_000;

        let logs: Vec<_> = (0..COUNT)
            .map(|i| otelview_model::LogRecord {
                time_unix_nano: BASE_NANOS + i * STEP_NANOS,
                observed_time_unix_nano: BASE_NANOS + i * STEP_NANOS,
                severity_number: 9,
                severity_text: String::new(),
                body: serde_json::json!("x"),
                attributes: serde_json::json!({}),
                resource_attributes: serde_json::json!({}),
                service_name: "svc".into(),
                trace_id: String::new(),
                span_id: String::new(),
                scope_name: String::new(),
            })
            .collect();
        storage.insert_logs(logs).await.unwrap();

        let app = router(&cfg, storage);
        let (status, v) = get_json(
            app,
            "/api/logs/histogram?buckets=10&start_ms=1000&end_ms=7000",
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let buckets = v.as_array().unwrap();
        assert_eq!(buckets.len(), 10);

        let total: u64 = buckets.iter().map(|b| b["info"].as_u64().unwrap()).sum();
        assert_eq!(total, COUNT, "every log in the window should be counted");

        // The symptom this guards: the far end of the window reading as empty
        // because only the newest logs were ever fetched.
        assert_eq!(
            buckets[0]["info"].as_u64().unwrap(),
            600,
            "the oldest bucket should be as full as the newest"
        );
        for (i, b) in buckets.iter().enumerate() {
            assert_eq!(b["info"].as_u64().unwrap(), 600, "bucket {i} is uneven");
        }
    }

    #[tokio::test]
    async fn exemplars_link_metrics_to_a_trace() {
        use otelview_model::{MetricPoint, MetricType};
        let (cfg, storage) = test_state();
        let mk = |name: &str, trace: &str, span: &str| MetricPoint {
            name: name.into(),
            description: String::new(),
            unit: "ms".into(),
            metric_type: MetricType::Histogram,
            service_name: "checkout".into(),
            time_unix_nano: 5_000,
            value: 1.0,
            count: 1,
            attributes: json!({}),
            resource_attributes: json!({}),
            extra: json!({
                "exemplars": [
                    {"trace_id": trace, "span_id": span, "time_unix_nano": 5_000, "value": 12.5}
                ]
            }),
        };
        let mut plain = mk("no.exemplars", "", "");
        plain.extra = json!({});
        storage
            .insert_metrics(vec![
                mk("http.server.duration", "aaa111", "s1"),
                mk("db.client.duration", "aaa111", "s2"),
                mk("other.metric", "bbb222", "s9"),
                plain,
            ])
            .await
            .unwrap();
        let app = router(&cfg, storage);

        // Both metrics that reference the trace come back.
        let (status, v) = get_json(app.clone(), "/api/metrics/exemplars?trace_id=aaa111").await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["metric_name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"http.server.duration"));
        assert!(names.contains(&"db.client.duration"));
        assert_eq!(v[0]["exemplar"]["value"], 12.5);

        // Narrowing to a span keeps only that span's metric.
        let (_, v) = get_json(
            app.clone(),
            "/api/metrics/exemplars?trace_id=aaa111&span_id=s2",
        )
        .await;
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["metric_name"], "db.client.duration");

        // A trace nothing points at is empty, not an error.
        let (status, v) = get_json(app.clone(), "/api/metrics/exemplars?trace_id=nope").await;
        assert_eq!(status, StatusCode::OK);
        assert!(v.as_array().unwrap().is_empty());

        // An all-zero trace id links nothing.
        let (_, v) = get_json(app, "/api/metrics/exemplars?trace_id=0000000000000000").await;
        assert!(v.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn traceql_filters_traces() {
        let (cfg, storage) = test_state();
        // Two traces: one healthy and fast, one slow with an erroring span.
        let mut ok_root = span("t-ok", "gateway", 1_000);
        ok_root.attributes = json!({"http.method": "GET"});
        let mut bad_root = span("t-bad", "gateway", 2_000);
        bad_root.attributes = json!({"http.method": "GET"});
        let mut bad_child = span("t-bad", "payments", 2_000);
        bad_child.span_id = "s2".into();
        bad_child.parent_span_id = "s1".into();
        bad_child.name = "charge".into();
        bad_child.status_code = 2;
        bad_child.end_time_unix_nano = 2_000 + 300_000_000; // 300ms
        bad_child.attributes = json!({"http.status_code": 502});
        storage
            .insert_spans(vec![ok_root, bad_root, bad_child])
            .await
            .unwrap();
        let app = router(&cfg, storage);

        // `{ status = error }` keeps only the trace with the failing span.
        let (status, v) = get_json(
            app.clone(),
            "/api/traces?traceql=%7B%20status%20%3D%20error%20%7D",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let got = v.as_array().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["trace_id"], "t-bad");

        // Duration predicate over span intrinsics.
        let (status, v) = get_json(
            app.clone(),
            "/api/traces?traceql=%7B%20duration%20%3E%20100ms%20%7D",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 1);

        // Spanset `&&` is satisfied across different spans of one trace.
        let (status, v) = get_json(
            app.clone(),
            "/api/traces?traceql=%7B%20name%20%3D%20%22charge%22%20%7D%20%26%26%20%7B%20.http.method%20%3D%20%22GET%22%20%7D",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 1);

        // Nothing matches -> empty array, not an error.
        let (status, v) = get_json(
            app.clone(),
            "/api/traces?traceql=%7B%20status%20%3D%20ok%20%7D",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(v.as_array().unwrap().is_empty());

        // A malformed query is a 400, matching the KQL contract.
        let (status, _) = get_json(app.clone(), "/api/traces?traceql=%7Bbad").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Unsupported structural operators are refused, not silently ignored.
        let (status, _) = get_json(
            app.clone(),
            "/api/traces?traceql=%7Bname%3D%22a%22%7D%20%3E%3E%20%7Bname%3D%22b%22%7D",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // An empty traceql param behaves as no filter at all.
        let (status, v) = get_json(app, "/api/traces?traceql=").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn lucene_filters_traces() {
        let (cfg, storage) = test_state();
        let mut ok_root = span("t-ok", "gateway", 1_000);
        ok_root.attributes = json!({"http.method": "GET", "http.status_code": 200});
        let bad_root = span("t-bad", "gateway", 2_000);
        let mut bad_child = span("t-bad", "payments", 2_000);
        bad_child.span_id = "s2".into();
        bad_child.name = "charge".into();
        bad_child.status_code = 2;
        bad_child.attributes = json!({"http.method": "POST", "http.status_code": 502});
        storage
            .insert_spans(vec![ok_root, bad_root, bad_child])
            .await
            .unwrap();
        let app = router(&cfg, storage);

        // A trace matches when any of its spans does, so a predicate on the
        // child keeps the whole trace.
        let (status, v) = get_json(app.clone(), "/api/traces?lucene=name%3Acharge").await;
        assert_eq!(status, StatusCode::OK);
        let got = v.as_array().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["trace_id"], "t-bad");

        // Ranges over a numeric attribute.
        let (status, v) = get_json(
            app.clone(),
            "/api/traces?lucene=http.status_code%3A%5B500%20TO%20*%5D",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 1);

        // Boolean combination, and the catch-all that keeps both.
        let (status, v) = get_json(
            app.clone(),
            "/api/traces?lucene=service%3Apayments%20OR%20http.method%3AGET",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 2);

        // Malformed is a 400, named for the language that failed.
        let (status, body) = get_text(app.clone(), "/api/traces?lucene=(bad").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("invalid Lucene query"), "{body}");

        // Empty means no filter.
        let (status, v) = get_json(app, "/api/traces?lucene=").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 2);
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
            .insert_logs(vec![
                mk(9, "GET", 200),
                mk(17, "POST", 500),
                mk(9, "POST", 201),
            ])
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

        // The same request in Lucene, over the same records.
        let (status, v) = get_json(
            app.clone(),
            "/api/logs?lucene=http.method%3APOST%20AND%20http.status_code%3A%5B500%20TO%20*%5D",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["severity_number"], 17);

        // A bare term searches the whole record.
        let (status, v) = get_json(app.clone(), "/api/logs?lucene=%22req%20done%22").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v.as_array().unwrap().len(), 3);

        // The histogram takes the same filter.
        let (status, v) =
            get_json(app.clone(), "/api/logs/histogram?lucene=http.method%3AGET").await;
        assert_eq!(status, StatusCode::OK);
        // Buckets count per severity; only the one GET log survives.
        let total: u64 = v
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|b| {
                ["trace", "debug", "info", "warn", "error", "fatal"].map(|k| b[k].as_u64().unwrap())
            })
            .sum();
        assert_eq!(total, 1);

        let (status, body) = get_text(app.clone(), "/api/logs?lucene=%22unterminated").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("invalid Lucene query"), "{body}");

        let (status, v) = get_json(app, "/api/logs/fields").await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
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
        let names: Vec<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"http.method"));
        assert!(names.contains(&"host.name"));
    }
}
