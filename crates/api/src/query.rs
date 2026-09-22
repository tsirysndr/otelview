//! The query layer: time windows, the three query languages, and the search
//! calls that run them against a storage backend.
//!
//! The REST handlers and the MCP server both call these, so a trace search
//! means the same thing whether it came from the web UI or from an agent —
//! including the over-fetch rules that a span-level filter needs, which are
//! easy to get subtly wrong twice.

use otelview_model::{
    LogQuery, LogRecord, MetricQuery, MetricSeries, SpanRecord, TraceQuery, TraceSummary,
};
use otelview_storage::DynStorage;
use serde::{Deserialize, Serialize};

use crate::analytics::{self, LogBucket};
use crate::{kql, lucene, seriesfns, traceql};

/// How many candidate traces a span-level query pulls spans for before
/// giving up. Each candidate costs one `get_trace`, so this bounds the work
/// a single request can do; matching stops as soon as `limit` traces are
/// found.
pub const TRACE_SCAN_LIMIT: usize = 200;

/// Over-fetch depth for a log query with a language filter: the predicate
/// runs in Rust over fetched records, so the page has to be deep enough for
/// the filter to have something to reject.
pub const LOG_SCAN_LIMIT: usize = 5000;

pub const DEFAULT_TRACE_LIMIT: usize = 20;
pub const MAX_TRACE_LIMIT: usize = 500;
pub const DEFAULT_LOG_LIMIT: usize = 200;
pub const MAX_LOG_LIMIT: usize = 5000;
pub const DEFAULT_EXEMPLAR_LIMIT: usize = 50;
pub const MAX_EXEMPLAR_LIMIT: usize = 500;
pub const DEFAULT_HISTOGRAM_BUCKETS: usize = 40;
pub const MAX_HISTOGRAM_BUCKETS: usize = 500;

/// Something the caller asked for that cannot be served.
///
/// The split is the whole point: a query the user mistyped is a 400 and
/// should be shown back to them, a storage failure is a 500 and should be
/// logged. Callers that speak neither HTTP nor JSON-RPC still get to tell
/// the two apart.
#[derive(Debug)]
pub enum QueryError {
    /// A query the user wrote that does not parse.
    BadQuery(String),
    /// The storage backend failed.
    Backend(anyhow::Error),
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryError::BadQuery(msg) => write!(f, "{msg}"),
            QueryError::Backend(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for QueryError {}

impl From<anyhow::Error> for QueryError {
    fn from(e: anyhow::Error) -> Self {
        QueryError::Backend(e)
    }
}

pub type QueryResult<T> = Result<T, QueryError>;

fn now_unix_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// "30s", "15m", "6h", "7d" or plain seconds.
pub fn parse_lookback(s: &str) -> Option<u64> {
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
    num.parse::<f64>()
        .ok()
        .map(|n| (n * mult as f64 * 1e9) as u64)
}

/// The time range a query covers: either a lookback from now, or an
/// absolute range in unix millis. Absolute wins when both are given.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Window {
    /// Relative window ending now: "15m", "6h", "7d".
    pub lookback: Option<String>,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
}

impl Window {
    /// (min, max) as unix nanos.
    pub fn bounds(&self) -> (Option<u64>, Option<u64>) {
        let min = self.start_ms.map(|ms| ms * 1_000_000).or_else(|| {
            self.lookback
                .as_deref()
                .and_then(parse_lookback)
                .map(|w| now_unix_nanos().saturating_sub(w))
        });
        (min, self.end_ms.map(|ms| ms * 1_000_000))
    }

    pub fn lookback(s: impl Into<String>) -> Self {
        Self {
            lookback: Some(s.into()),
            ..Default::default()
        }
    }
}

/// A log query in whichever language the caller named.
///
/// Both parse to an AST and evaluate as a Rust predicate over fetched
/// records rather than lowering to SQL, so either works over every storage
/// backend and they can share one code path here.
pub enum LogFilter {
    Kql(kql::Expr),
    Lucene(lucene::Query),
}

impl LogFilter {
    /// `None` when neither param carries a query. KQL wins if a caller
    /// somehow sends both; the UI only ever sends the mode you are in.
    pub fn parse(kql_q: Option<&str>, lucene_q: Option<&str>) -> QueryResult<Option<Self>> {
        if let Some(q) = nonempty(kql_q) {
            return match kql::parse(q) {
                Ok(expr) => Ok(expr.map(LogFilter::Kql)),
                Err(e) => Err(bad_query("KQL", &e)),
            };
        }
        if let Some(q) = nonempty(lucene_q) {
            return match lucene::parse(q) {
                Ok(expr) => Ok(expr.map(LogFilter::Lucene)),
                Err(e) => Err(bad_query("Lucene", &e)),
            };
        }
        Ok(None)
    }

    pub fn matches(&self, log: &LogRecord) -> bool {
        match self {
            LogFilter::Kql(expr) => kql::eval(expr, log),
            LogFilter::Lucene(q) => lucene::eval(q, log),
        }
    }
}

/// A trace query in whichever language the caller named.
///
/// TraceQL predicates on the spanset, so it sees the whole trace at once.
/// Lucene has no notion of a spanset, so a trace matches when any single
/// span does — the same rule a user gets from the logs view.
pub enum TraceFilter {
    TraceQl(traceql::Expr),
    Lucene(lucene::Query),
}

impl TraceFilter {
    pub fn parse(traceql_q: Option<&str>, lucene_q: Option<&str>) -> QueryResult<Option<Self>> {
        if let Some(q) = nonempty(traceql_q) {
            return match traceql::parse(q) {
                Ok(expr) => Ok(expr.map(TraceFilter::TraceQl)),
                Err(e) => Err(bad_query("TraceQL", &e)),
            };
        }
        if let Some(q) = nonempty(lucene_q) {
            return match lucene::parse(q) {
                Ok(expr) => Ok(expr.map(TraceFilter::Lucene)),
                Err(e) => Err(bad_query("Lucene", &e)),
            };
        }
        Ok(None)
    }

    pub fn matches(&self, spans: &[SpanRecord]) -> bool {
        match self {
            TraceFilter::TraceQl(expr) => traceql::eval(expr, spans),
            TraceFilter::Lucene(q) => spans.iter().any(|s| lucene::eval(q, s)),
        }
    }
}

fn nonempty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn some_nonempty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The parse-failure message, phrased so the user can see which language
/// rejected what they typed.
fn bad_query(lang: &str, err: &str) -> QueryError {
    QueryError::BadQuery(format!("invalid {lang} query: {err}"))
}

/// Everything the trace search accepts, in the units a caller thinks in
/// (milliseconds and a time window) rather than the nanos storage wants.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TraceSearch {
    pub service: Option<String>,
    pub operation: Option<String>,
    /// Substring or `key=value` match over span attributes.
    pub q: Option<String>,
    /// TraceQL, e.g. `{ status = error && duration > 100ms }`.
    pub traceql: Option<String>,
    /// Lucene, e.g. `service:gateway AND http.status_code:[500 TO *]`.
    pub lucene: Option<String>,
    pub min_duration_ms: Option<f64>,
    pub max_duration_ms: Option<f64>,
    pub errors_only: Option<bool>,
    #[serde(flatten)]
    pub window: Window,
    pub limit: Option<usize>,
}

/// Trace summaries matching `p`, newest first.
pub async fn search_traces(storage: &DynStorage, p: TraceSearch) -> QueryResult<Vec<TraceSummary>> {
    let (start_time_min_unix_nano, start_time_max_unix_nano) = p.window.bounds();
    let filter = TraceFilter::parse(p.traceql.as_deref(), p.lucene.as_deref())?;
    let limit = p
        .limit
        .unwrap_or(DEFAULT_TRACE_LIMIT)
        .clamp(1, MAX_TRACE_LIMIT);
    let q = TraceQuery {
        service: some_nonempty(p.service),
        operation: some_nonempty(p.operation),
        attribute_query: some_nonempty(p.q),
        min_duration_nanos: p.min_duration_ms.map(|ms| (ms * 1e6) as u64),
        max_duration_nanos: p.max_duration_ms.map(|ms| (ms * 1e6) as u64),
        start_time_min_unix_nano,
        start_time_max_unix_nano,
        errors_only: p.errors_only.unwrap_or(false),
        // A span-level query needs a candidate pool to filter down from,
        // since it predicates on spans the summary does not carry.
        limit: match filter {
            Some(_) => TRACE_SCAN_LIMIT.max(limit),
            None => limit,
        },
    };
    let candidates = storage.find_traces(q).await?;
    let Some(filter) = filter else {
        return Ok(candidates);
    };
    // Summaries come back newest-first, so taking the first `limit` matches
    // gives the newest matching traces without scanning the whole pool.
    let mut out = Vec::with_capacity(limit);
    for summary in candidates {
        let spans = storage.get_trace(&summary.trace_id).await?;
        if filter.matches(&spans) {
            out.push(summary);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

/// Everything the log search accepts.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LogSearch {
    pub service: Option<String>,
    /// OTLP severity number, inclusive: 9 info, 13 warn, 17 error.
    pub min_severity: Option<i32>,
    /// Plain substring match over body and attributes.
    pub search: Option<String>,
    /// KQL, e.g. `http.method:POST and status_code:>=500`.
    pub kql: Option<String>,
    /// Lucene, e.g. `level:ERROR AND "connection refused"`.
    pub lucene: Option<String>,
    pub trace_id: Option<String>,
    #[serde(flatten)]
    pub window: Window,
    pub limit: Option<usize>,
}

impl LogSearch {
    /// The storage-level query, minus any language filter (which storage
    /// cannot express and this layer applies in Rust).
    fn to_storage_query(&self, limit: usize) -> LogQuery {
        let (time_min_unix_nano, time_max_unix_nano) = self.window.bounds();
        LogQuery {
            service: some_nonempty(self.service.clone()),
            min_severity: self.min_severity.filter(|s| *s > 0),
            search: some_nonempty(self.search.clone()),
            trace_id: some_nonempty(self.trace_id.clone()),
            time_min_unix_nano,
            time_max_unix_nano,
            limit,
        }
    }
}

/// Log records matching `p`, newest first.
pub async fn search_logs(storage: &DynStorage, p: LogSearch) -> QueryResult<Vec<LogRecord>> {
    let filter = LogFilter::parse(p.kql.as_deref(), p.lucene.as_deref())?;
    let limit = p.limit.unwrap_or(DEFAULT_LOG_LIMIT).clamp(1, MAX_LOG_LIMIT);
    // With a query filter, over-fetch and filter down to the limit.
    let depth = if filter.is_some() {
        LOG_SCAN_LIMIT
    } else {
        limit
    };
    let mut logs = storage.query_logs(p.to_storage_query(depth)).await?;
    if let Some(filter) = filter {
        logs.retain(|log| filter.matches(log));
        logs.truncate(limit);
    }
    Ok(logs)
}

/// Log counts per severity over `buckets` slices of the window.
pub async fn log_histogram(
    storage: &DynStorage,
    p: LogSearch,
    buckets: usize,
) -> QueryResult<Vec<LogBucket>> {
    let filter = LogFilter::parse(p.kql.as_deref(), p.lucene.as_deref())?;
    // limit 0 means "the histogram pager decides how deep to walk".
    let q = p.to_storage_query(0);
    let (time_min, time_max) = (q.time_min_unix_nano, q.time_max_unix_nano);
    let predicate = filter.map(|f| move |l: &LogRecord| f.matches(l));
    Ok(analytics::log_histogram(
        storage,
        q,
        buckets.clamp(1, MAX_HISTOGRAM_BUCKETS),
        time_min,
        time_max,
        predicate
            .as_ref()
            .map(|f| f as &(dyn Fn(&LogRecord) -> bool + Send + Sync)),
    )
    .await?)
}

/// Metrics carrying an exemplar that points at `trace_id` (and `span_id`
/// when given).
///
/// An all-zero trace id is what a span with no recorded parent context
/// serializes to; it matches nothing real, so it answers empty rather than
/// making the backend scan for it.
pub async fn find_exemplars(
    storage: &DynStorage,
    trace_id: &str,
    span_id: Option<&str>,
    limit: Option<usize>,
) -> anyhow::Result<Vec<otelview_model::ExemplarHit>> {
    let trace_id = trace_id.trim();
    if trace_id.is_empty() || trace_id.chars().all(|c| c == '0') {
        return Ok(Vec::new());
    }
    let span_id = span_id.map(str::trim).filter(|s| !s.is_empty());
    let limit = limit
        .unwrap_or(DEFAULT_EXEMPLAR_LIMIT)
        .clamp(1, MAX_EXEMPLAR_LIMIT);
    storage.find_exemplars(trace_id, span_id, limit).await
}

/// Everything the metric series query accepts.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SeriesSearch {
    pub name: String,
    pub service: Option<String>,
    /// Per-series function: raw | rate | increase.
    pub func: Option<String>,
    /// Cross-series aggregation: none | sum | avg | min | max.
    pub agg: Option<String>,
    #[serde(flatten)]
    pub window: Window,
    pub max_points: Option<usize>,
}

/// How many buckets a cross-series aggregation lines points up on.
const AGG_BUCKETS: usize = 120;

pub async fn metric_series(
    storage: &DynStorage,
    p: SeriesSearch,
) -> QueryResult<Vec<MetricSeries>> {
    let (time_min_unix_nano, time_max_unix_nano) = p.window.bounds();
    let q = MetricQuery {
        name: p.name,
        service: some_nonempty(p.service),
        time_min_unix_nano,
        time_max_unix_nano,
        max_points: p.max_points.unwrap_or(500).clamp(10, 10_000),
    };
    let mut series = storage.query_metric_series(q).await?;
    if let Some(func) = p.func.as_deref() {
        seriesfns::apply_function(&mut series, func);
    }
    if let Some(agg) = p.agg.as_deref() {
        series = seriesfns::aggregate(series, agg, AGG_BUCKETS);
    }
    Ok(series)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookback_units() {
        assert_eq!(parse_lookback("30s"), Some(30_000_000_000));
        assert_eq!(parse_lookback("15m"), Some(900_000_000_000));
        assert_eq!(parse_lookback("2h"), Some(7_200_000_000_000));
        assert_eq!(parse_lookback("90"), Some(90_000_000_000));
        assert_eq!(parse_lookback("all"), None);
        assert_eq!(parse_lookback(""), None);
    }

    #[test]
    fn absolute_window_beats_lookback() {
        let w = Window {
            lookback: Some("1h".into()),
            start_ms: Some(3_000),
            end_ms: Some(7_000),
        };
        assert_eq!(w.bounds(), (Some(3_000_000_000), Some(7_000_000_000)));
    }

    /// The filters hold parsed ASTs, so they carry no `Debug`; unwrapping
    /// the error needs the match rather than `unwrap_err`.
    fn err_of<T>(r: QueryResult<T>) -> QueryError {
        match r {
            Err(e) => e,
            Ok(_) => panic!("expected the query to be rejected"),
        }
    }

    #[test]
    fn bad_query_names_its_language() {
        let err = err_of(TraceFilter::parse(Some("{bad"), None));
        assert!(matches!(err, QueryError::BadQuery(_)));
        assert!(err.to_string().starts_with("invalid TraceQL query:"));
        let err = err_of(LogFilter::parse(None, Some("(bad")));
        assert!(err.to_string().starts_with("invalid Lucene query:"));
    }
}
