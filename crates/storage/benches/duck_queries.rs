//! Timed runs of every DuckDB query path against a realistic dataset.
//!
//! Not a criterion harness — one seeded run, timings printed as a table that
//! can be pasted into the README. The dataset shape mirrors a small
//! production deployment: a handful of services and routes, JSON attributes
//! on everything, many traces of a few spans each.
//!
//!     cargo bench -p otelview-storage --bench duck_queries
//!     cargo bench -p otelview-storage --bench duck_queries -- 500000

use std::time::Instant;

use otelview_model::{
    LogQuery, LogRecord, MetricPoint, MetricQuery, MetricType, SpanRecord, TraceQuery,
};
use otelview_storage::{duck::DuckdbStorage, Storage};
use serde_json::json;

const SERVICES: &[&str] = &[
    "rocksky-api",
    "rocksky-xrpc",
    "scrobbler",
    "jetstream",
    "mirror",
    "spotify-proxy",
    "musicbrainz",
    "deezer",
];
const ROUTES: &[&str] = &[
    "/search",
    "/hydrate",
    "/v1/*",
    "/xrpc/app.rocksky.feed",
    "/enrich",
];

fn span(i: usize, spans_per_trace: usize) -> SpanRecord {
    let trace = i / spans_per_trace;
    SpanRecord {
        trace_id: format!("{trace:032x}"),
        span_id: format!("{i:016x}"),
        parent_span_id: if i % spans_per_trace == 0 {
            String::new()
        } else {
            format!("{:016x}", trace * spans_per_trace)
        },
        name: format!("{} {}", "GET", ROUTES[i % ROUTES.len()]),
        service_name: SERVICES[i % SERVICES.len()].into(),
        kind: "server".into(),
        start_time_unix_nano: 1_700_000_000_000_000_000 + (i as u64) * 1_000_000,
        end_time_unix_nano: 1_700_000_000_000_000_000 + (i as u64) * 1_000_000 + 3_500_000,
        status_code: if i % 50 == 0 { 2 } else { 0 },
        status_message: String::new(),
        attributes: json!({
            "http.route": ROUTES[i % ROUTES.len()],
            "http.method": "GET",
            "http.status_code": if i % 50 == 0 { 500 } else { 200 },
            "mb.query.track": format!("track number {i}"),
        }),
        resource_attributes: json!({"service.name": SERVICES[i % SERVICES.len()]}),
        events: json!([]),
        links: json!([]),
        scope_name: "bench".into(),
        scope_version: "1".into(),
    }
}

fn log(i: usize) -> LogRecord {
    let t = 1_700_000_000_000_000_000 + (i as u64) * 700_000;
    LogRecord {
        time_unix_nano: t,
        observed_time_unix_nano: t,
        severity_number: if i % 40 == 0 { 17 } else { 9 },
        severity_text: if i % 40 == 0 { "ERROR" } else { "INFO" }.into(),
        body: json!(format!(
            "http request {} handled in {}ms",
            ROUTES[i % ROUTES.len()],
            i % 900
        )),
        attributes: json!({"http.route": ROUTES[i % ROUTES.len()], "duration_ms": i % 900}),
        resource_attributes: json!({"service.name": SERVICES[i % SERVICES.len()]}),
        service_name: SERVICES[i % SERVICES.len()].into(),
        trace_id: format!("{:032x}", i / 4),
        span_id: String::new(),
        scope_name: "bench".into(),
    }
}

fn metric(i: usize) -> MetricPoint {
    MetricPoint {
        name: format!("metric.{}", i % 25),
        description: "bench".into(),
        unit: "1".into(),
        metric_type: MetricType::Gauge,
        service_name: SERVICES[i % SERVICES.len()].into(),
        time_unix_nano: 1_700_000_000_000_000_000 + (i as u64) * 400_000,
        value: (i % 100) as f64,
        count: 0,
        attributes: json!({"instance": i % 4}),
        resource_attributes: json!({"service.name": SERVICES[i % SERVICES.len()]}),
        extra: json!({}),
    }
}

#[tokio::main]
async fn main() {
    let spans_n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(200_000);
    let logs_n = spans_n;
    let metrics_n = spans_n * 2;
    const SPANS_PER_TRACE: usize = 4;

    let store = DuckdbStorage::open(":memory:").expect("open");

    let mut rows = Vec::new();
    println!("| operation | time | rate |");
    println!("| --- | --- | --- |");

    let mut timed = |label: String, elapsed: std::time::Duration, count: Option<usize>| {
        let rate = count
            .map(|n| format!("{:.0}/s", n as f64 / elapsed.as_secs_f64()))
            .unwrap_or_else(|| "—".into());
        rows.push((label.clone(), elapsed, rate.clone()));
        println!("| {label} | {elapsed:.1?} | {rate} |");
    };

    // ---- ingest ----
    let spans: Vec<_> = (0..spans_n).map(|i| span(i, SPANS_PER_TRACE)).collect();
    let t = Instant::now();
    store.insert_spans(spans).await.expect("spans");
    timed(
        format!("insert {spans_n} spans"),
        t.elapsed(),
        Some(spans_n),
    );

    let logs: Vec<_> = (0..logs_n).map(log).collect();
    let t = Instant::now();
    store.insert_logs(logs).await.expect("logs");
    timed(format!("insert {logs_n} logs"), t.elapsed(), Some(logs_n));

    let metrics: Vec<_> = (0..metrics_n).map(metric).collect();
    let t = Instant::now();
    store.insert_metrics(metrics).await.expect("metrics");
    timed(
        format!("insert {metrics_n} metric points"),
        t.elapsed(),
        Some(metrics_n),
    );

    // ---- queries ----
    let t = Instant::now();
    let services = store.list_services().await.expect("services");
    timed(
        format!("list_services ({})", services.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let ops = store.list_operations("rocksky-api").await.expect("ops");
    timed(
        format!("list_operations ({})", ops.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let traces = store
        .find_traces(TraceQuery {
            limit: 20,
            ..Default::default()
        })
        .await
        .expect("find");
    timed(
        format!("find_traces newest 20 ({})", traces.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let traces = store
        .find_traces(TraceQuery {
            service: Some("scrobbler".into()),
            errors_only: true,
            limit: 20,
            ..Default::default()
        })
        .await
        .expect("find");
    timed(
        format!("find_traces service+errors ({})", traces.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let traces = store
        .find_traces(TraceQuery {
            attribute_query: Some("http.route=/search".into()),
            limit: 20,
            ..Default::default()
        })
        .await
        .expect("find");
    timed(
        format!("find_traces attr key=value ({})", traces.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let traces = store
        .find_traces(TraceQuery {
            attribute_query: Some("track number 4242".into()),
            limit: 20,
            ..Default::default()
        })
        .await
        .expect("find");
    timed(
        format!("find_traces attr substring ({})", traces.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let spans = store
        .get_trace(&format!("{:032x}", 1234))
        .await
        .expect("get");
    timed(
        format!("get_trace ({} spans)", spans.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let logs = store
        .query_logs(LogQuery {
            limit: 300,
            ..Default::default()
        })
        .await
        .expect("logs");
    timed(
        format!("query_logs newest 300 ({})", logs.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let logs = store
        .query_logs(LogQuery {
            search: Some("handled in 500ms".into()),
            limit: 300,
            ..Default::default()
        })
        .await
        .expect("logs");
    timed(
        format!("query_logs substring ({})", logs.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let logs = store
        .query_logs(LogQuery {
            min_severity: Some(17),
            limit: 300,
            ..Default::default()
        })
        .await
        .expect("logs");
    timed(
        format!("query_logs errors ({})", logs.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let metrics = store.list_metrics().await.expect("metrics");
    timed(
        format!("list_metrics ({})", metrics.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    let series = store
        .query_metric_series(MetricQuery {
            name: "metric.7".into(),
            ..Default::default()
        })
        .await
        .expect("series");
    let points: usize = series.iter().map(|s| s.points.len()).sum();
    timed(
        format!("metric series ({} series, {points} pts)", series.len()),
        t.elapsed(),
        None,
    );

    let t = Instant::now();
    store.stats().await.expect("stats");
    timed("stats".into(), t.elapsed(), None);

    let cutoff = 1_700_000_000_000_000_000 + (spans_n as u64 / 2) * 1_000_000;
    let t = Instant::now();
    let deleted = store.sweep_expired(cutoff).await.expect("sweep").unwrap();
    timed(
        format!(
            "retention sweep ({} rows)",
            deleted.0 + deleted.1 + deleted.2
        ),
        t.elapsed(),
        Some((deleted.0 + deleted.1 + deleted.2) as usize),
    );
}
