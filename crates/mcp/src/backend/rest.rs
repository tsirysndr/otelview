//! Remote backend: another otelview's query API over HTTP.
//!
//! This is how a desktop AI client reaches an instance it does not host —
//! `otelview mcp --endpoint https://otelview.internal`. It speaks the same
//! `/api/*` the web UI does, so anything the browser can see, an agent
//! pointed here can see too.

use async_trait::async_trait;
use otelview_api::analytics::{FieldInfo, LogBucket, ServiceGraph, ServiceStats};
use otelview_api::query::{LogSearch, QueryError, QueryResult, SeriesSearch, TraceSearch, Window};
use otelview_model::{
    ExemplarHit, LogRecord, MetricInfo, MetricSeries, SpanRecord, StorageStats, TraceSummary,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::Otel;

/// How long a single query may take. Generous: a trace search with a
/// language filter over a cold remote store is slow but worth waiting for.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

pub struct Rest {
    client: reqwest::Client,
    /// Base URL with no trailing slash, e.g. `http://127.0.0.1:4319`.
    base: String,
    token: Option<String>,
}

impl Rest {
    pub fn new(endpoint: &str, token: Option<String>) -> anyhow::Result<Self> {
        let base = endpoint.trim().trim_end_matches('/').to_string();
        if base.is_empty() {
            anyhow::bail!("the otelview endpoint is empty");
        }
        if !base.starts_with("http://") && !base.starts_with("https://") {
            anyhow::bail!("the otelview endpoint {base:?} needs an http:// or https:// scheme");
        }
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .user_agent(concat!("otelview-mcp/", env!("CARGO_PKG_VERSION")))
                .build()?,
            base,
            token: token.filter(|t| !t.trim().is_empty()),
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, q: Q) -> QueryResult<T> {
        let url = format!("{}/api{path}", self.base);
        let mut req = self.client.get(&url).query(&q.0);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        // The query string is the whole query, so it is logged: when a tool
        // answers something surprising, the next question is always what it
        // actually asked the server.
        tracing::debug!(path, params = %q, "querying the otelview api");
        let started = std::time::Instant::now();
        let resp = req.send().await.map_err(|e| {
            tracing::warn!(%url, error = %e, "otelview api request failed");
            QueryError::Backend(anyhow::anyhow!("GET {url}: {e}"))
        })?;
        let status = resp.status();
        tracing::debug!(
            path,
            status = status.as_u16(),
            elapsed_ms = crate::fmt::elapsed_ms(started),
            "otelview api answered"
        );
        if status.is_success() {
            return resp
                .json::<T>()
                .await
                .map_err(|e| QueryError::Backend(anyhow::anyhow!("decoding {url}: {e}")));
        }
        let body = resp.text().await.unwrap_or_default();
        // The API answers a mistyped query with 400 and the parser's own
        // message; that belongs to the user, not in a stack trace.
        if status == reqwest::StatusCode::BAD_REQUEST {
            return Err(QueryError::BadQuery(body));
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(QueryError::Backend(anyhow::anyhow!(
                "otelview at {} rejected the API token (401)",
                self.base
            )));
        }
        Err(QueryError::Backend(anyhow::anyhow!(
            "GET {url} failed: {status} {}",
            body.trim()
        )))
    }
}

/// Query-string builder that drops anything unset, so an omitted filter is
/// absent rather than sent as an empty string the API would then match on.
#[derive(Default)]
struct Q(Vec<(String, String)>);

impl Q {
    fn new() -> Self {
        Self::default()
    }

    fn put(mut self, key: &str, value: Option<impl ToString>) -> Self {
        if let Some(v) = value {
            let v = v.to_string();
            if !v.is_empty() {
                self.0.push((key.to_string(), v));
            }
        }
        self
    }

    fn window(self, w: &Window) -> Self {
        self.put("lookback", w.lookback.clone())
            .put("start_ms", w.start_ms)
            .put("end_ms", w.end_ms)
    }
}

impl std::fmt::Display for Q {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("&")?;
            }
            write!(f, "{k}={v}")?;
        }
        Ok(())
    }
}

#[async_trait]
impl Otel for Rest {
    async fn services(&self) -> QueryResult<Vec<String>> {
        self.get("/services", Q::new()).await
    }

    async fn operations(&self, service: &str) -> QueryResult<Vec<String>> {
        self.get("/operations", Q::new().put("service", Some(service)))
            .await
    }

    async fn search_traces(&self, p: TraceSearch) -> QueryResult<Vec<TraceSummary>> {
        let q = Q::new()
            .put("service", p.service)
            .put("operation", p.operation)
            .put("q", p.q)
            .put("traceql", p.traceql)
            .put("lucene", p.lucene)
            .put("min_duration_ms", p.min_duration_ms)
            .put("max_duration_ms", p.max_duration_ms)
            .put("errors_only", p.errors_only)
            .put("limit", p.limit)
            .window(&p.window);
        self.get("/traces", q).await
    }

    async fn get_trace(&self, trace_id: &str) -> QueryResult<Vec<SpanRecord>> {
        // The API answers 404 for a trace it does not hold; an empty span
        // list says the same thing and is what the direct backend returns,
        // so the tools above can stay identical.
        match self
            .get::<Vec<SpanRecord>>(&format!("/traces/{trace_id}"), Q::new())
            .await
        {
            Ok(spans) => Ok(spans),
            Err(QueryError::Backend(e)) if e.to_string().contains("404") => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    async fn trace_fields(
        &self,
        service: Option<String>,
        w: Window,
    ) -> QueryResult<Vec<FieldInfo>> {
        self.get(
            "/traces/fields",
            Q::new().put("service", service).window(&w),
        )
        .await
    }

    async fn search_logs(&self, p: LogSearch) -> QueryResult<Vec<LogRecord>> {
        self.get("/logs", log_query(&p).put("limit", p.limit)).await
    }

    async fn log_histogram(&self, p: LogSearch, buckets: usize) -> QueryResult<Vec<LogBucket>> {
        self.get(
            "/logs/histogram",
            log_query(&p).put("buckets", Some(buckets)),
        )
        .await
    }

    async fn log_fields(
        &self,
        service: Option<String>,
        min_severity: Option<i32>,
        w: Window,
    ) -> QueryResult<Vec<FieldInfo>> {
        self.get(
            "/logs/fields",
            Q::new()
                .put("service", service)
                .put("min_severity", min_severity)
                .window(&w),
        )
        .await
    }

    async fn metrics(&self) -> QueryResult<Vec<MetricInfo>> {
        self.get("/metrics", Q::new()).await
    }

    async fn metric_series(&self, p: SeriesSearch) -> QueryResult<Vec<MetricSeries>> {
        let q = Q::new()
            .put("name", Some(p.name))
            .put("service", p.service)
            .put("func", p.func)
            .put("agg", p.agg)
            .put("max_points", p.max_points)
            .window(&p.window);
        self.get("/metrics/series", q).await
    }

    async fn exemplars(
        &self,
        trace_id: &str,
        span_id: Option<&str>,
        limit: Option<usize>,
    ) -> QueryResult<Vec<ExemplarHit>> {
        self.get(
            "/metrics/exemplars",
            Q::new()
                .put("trace_id", Some(trace_id))
                .put("span_id", span_id)
                .put("limit", limit),
        )
        .await
    }

    async fn service_stats(&self, w: Window) -> QueryResult<Vec<ServiceStats>> {
        self.get("/services/stats", Q::new().window(&w)).await
    }

    async fn service_graph(&self, w: Window) -> QueryResult<ServiceGraph> {
        self.get("/service-graph", Q::new().window(&w)).await
    }

    async fn stats(&self) -> QueryResult<StorageStats> {
        self.get("/stats", Q::new()).await
    }

    async fn config(&self) -> QueryResult<Value> {
        self.get("/config", Q::new()).await
    }

    fn describe(&self) -> String {
        format!("the otelview instance at {}", self.base)
    }
}

/// The filters `/logs` and `/logs/histogram` share.
fn log_query(p: &LogSearch) -> Q {
    Q::new()
        .put("service", p.service.clone())
        .put("min_severity", p.min_severity)
        .put("search", p.search.clone())
        .put("kql", p.kql.clone())
        .put("lucene", p.lucene.clone())
        .put("trace_id", p.trace_id.clone())
        .window(&p.window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_endpoint_needs_a_scheme() {
        assert!(Rest::new("otelview.internal:4319", None).is_err());
        assert!(Rest::new("", None).is_err());
        assert!(Rest::new("http://127.0.0.1:4319/", None).is_ok());
    }

    #[test]
    fn the_trailing_slash_does_not_double_up() {
        let r = Rest::new("http://host:4319/", None).unwrap();
        assert_eq!(r.base, "http://host:4319");
        assert!(r.describe().ends_with("http://host:4319"));
    }

    #[test]
    fn unset_filters_are_left_out_entirely() {
        let q = log_query(&LogSearch {
            service: Some("svc".into()),
            search: Some(String::new()),
            ..Default::default()
        });
        assert_eq!(q.0, vec![("service".to_string(), "svc".to_string())]);
    }
}
