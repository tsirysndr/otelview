//! Derived analytics: service RED stats, the service dependency graph and
//! the log-volume histogram.
//!
//! Everything is computed from the [`Storage`] trait's existing queries so
//! all backends (memory, duckdb, jaeger, remote) get these for free: recent
//! traces are sampled (up to [`MAX_TRACES`]) and aggregated span-by-span.

use std::collections::BTreeMap;

use otelview_model::{LogQuery, LogRecord, SpanRecord, TraceQuery};
use otelview_storage::DynStorage;
use serde::Serialize;

/// Cap on how many traces are loaded per analytics request.
pub const MAX_TRACES: usize = 250;

#[derive(Debug, Serialize)]
pub struct ServiceStats {
    pub service: String,
    pub span_count: u64,
    /// Spans with kind=server or root spans — a proxy for "requests".
    pub request_count: u64,
    pub error_count: u64,
    pub error_rate: f64,
    /// Requests per second across the sampled window.
    pub rate_per_sec: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct GraphNode {
    pub service: String,
    pub span_count: u64,
    pub error_count: u64,
    pub avg_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub calls: u64,
    pub errors: u64,
    pub avg_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct ServiceGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub sampled_traces: usize,
}

#[derive(Debug, Serialize)]
pub struct FieldInfo {
    pub name: String,
    pub count: u64,
    pub top_values: Vec<(String, u64)>,
}

pub fn flatten_json(prefix: &str, v: &serde_json::Value, out: &mut Vec<(String, String)>) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten_json(&key, val, out);
            }
        }
        serde_json::Value::String(s) => out.push((prefix.to_string(), s.clone())),
        other => out.push((prefix.to_string(), other.to_string())),
    }
}

/// Turn (field, value) observations into ranked FieldInfo entries.
pub fn summarize_fields(
    observations: impl Iterator<Item = (String, String)>,
    max_fields: usize,
) -> Vec<FieldInfo> {
    let mut fields: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for (name, mut value) in observations {
        if name.is_empty() {
            continue;
        }
        value.truncate(60);
        *fields.entry(name).or_default().entry(value).or_default() += 1;
    }
    let mut out: Vec<FieldInfo> = fields
        .into_iter()
        .map(|(name, values)| {
            let count = values.values().sum();
            let mut top: Vec<(String, u64)> = values.into_iter().collect();
            top.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
            top.truncate(5);
            FieldInfo { name, count, top_values: top }
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    out.truncate(max_fields);
    out
}

/// Attribute keys (span + resource) with top values, from sampled traces.
pub async fn trace_fields(
    storage: &DynStorage,
    service: Option<String>,
    start_time_min_unix_nano: Option<u64>,
    start_time_max_unix_nano: Option<u64>,
) -> anyhow::Result<Vec<FieldInfo>> {
    let summaries = storage
        .find_traces(TraceQuery {
            service,
            start_time_min_unix_nano,
            start_time_max_unix_nano,
            limit: MAX_TRACES,
            ..Default::default()
        })
        .await?;
    let mut spans = Vec::new();
    for s in summaries {
        spans.extend(storage.get_trace(&s.trace_id).await?);
    }
    let mut kvs: Vec<(String, String)> = Vec::new();
    for s in &spans {
        flatten_json("", &s.attributes, &mut kvs);
        flatten_json("", &s.resource_attributes, &mut kvs);
    }
    Ok(summarize_fields(kvs.into_iter(), 50))
}

#[derive(Debug, Serialize)]
pub struct LogBucket {
    pub time_unix_nano: u64,
    pub trace: u64,
    pub debug: u64,
    pub info: u64,
    pub warn: u64,
    pub error: u64,
    pub fatal: u64,
}

/// Load spans of recent traces in the window (bounded sample).
async fn sample_spans(
    storage: &DynStorage,
    start_time_min_unix_nano: Option<u64>,
    start_time_max_unix_nano: Option<u64>,
) -> anyhow::Result<(Vec<SpanRecord>, usize)> {
    let summaries = storage
        .find_traces(TraceQuery {
            start_time_min_unix_nano,
            start_time_max_unix_nano,
            limit: MAX_TRACES,
            ..Default::default()
        })
        .await?;
    let sampled = summaries.len();
    let mut spans = Vec::new();
    for s in summaries {
        spans.extend(storage.get_trace(&s.trace_id).await?);
    }
    Ok((spans, sampled))
}

fn percentile(sorted_ms: &[f64], p: f64) -> f64 {
    if sorted_ms.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ms.len() as f64 - 1.0) * p).round() as usize;
    sorted_ms[idx.min(sorted_ms.len() - 1)]
}

pub async fn service_stats(
    storage: &DynStorage,
    start_time_min_unix_nano: Option<u64>,
    start_time_max_unix_nano: Option<u64>,
) -> anyhow::Result<Vec<ServiceStats>> {
    let (spans, _) = sample_spans(storage, start_time_min_unix_nano, start_time_max_unix_nano).await?;
    let mut by_service: BTreeMap<&str, Vec<&SpanRecord>> = BTreeMap::new();
    for s in &spans {
        by_service.entry(s.service_name.as_str()).or_default().push(s);
    }
    let t_min = spans.iter().map(|s| s.start_time_unix_nano).min().unwrap_or(0);
    let t_max = spans.iter().map(|s| s.end_time_unix_nano).max().unwrap_or(0);
    let window_secs = ((t_max.saturating_sub(t_min)) as f64 / 1e9).max(1.0);

    Ok(by_service
        .into_iter()
        .map(|(service, spans)| {
            let mut durations: Vec<f64> =
                spans.iter().map(|s| s.duration_nanos() as f64 / 1e6).collect();
            durations.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let requests = spans
                .iter()
                .filter(|s| s.kind == "server" || s.is_root())
                .count()
                .max(1) as u64;
            let errors = spans.iter().filter(|s| s.is_error()).count() as u64;
            ServiceStats {
                service: service.to_string(),
                span_count: spans.len() as u64,
                request_count: requests,
                error_count: errors,
                error_rate: errors as f64 / spans.len().max(1) as f64,
                rate_per_sec: requests as f64 / window_secs,
                p50_ms: percentile(&durations, 0.50),
                p95_ms: percentile(&durations, 0.95),
                p99_ms: percentile(&durations, 0.99),
            }
        })
        .collect())
}

pub async fn service_graph(
    storage: &DynStorage,
    start_time_min_unix_nano: Option<u64>,
    start_time_max_unix_nano: Option<u64>,
) -> anyhow::Result<ServiceGraph> {
    let (spans, sampled) =
        sample_spans(storage, start_time_min_unix_nano, start_time_max_unix_nano).await?;

    let mut nodes: BTreeMap<&str, (u64, u64, f64)> = BTreeMap::new();
    for s in &spans {
        let e = nodes.entry(s.service_name.as_str()).or_default();
        e.0 += 1;
        if s.is_error() {
            e.1 += 1;
        }
        e.2 += s.duration_nanos() as f64 / 1e6;
    }

    // Cross-service parent → child calls.
    let by_id: BTreeMap<(&str, &str), &SpanRecord> =
        spans.iter().map(|s| ((s.trace_id.as_str(), s.span_id.as_str()), s)).collect();
    let mut edges: BTreeMap<(&str, &str), (u64, u64, f64)> = BTreeMap::new();
    for s in &spans {
        if s.parent_span_id.is_empty() {
            continue;
        }
        let Some(parent) = by_id.get(&(s.trace_id.as_str(), s.parent_span_id.as_str())) else {
            continue;
        };
        if parent.service_name == s.service_name {
            continue;
        }
        let e = edges
            .entry((parent.service_name.as_str(), s.service_name.as_str()))
            .or_default();
        e.0 += 1;
        if s.is_error() {
            e.1 += 1;
        }
        e.2 += s.duration_nanos() as f64 / 1e6;
    }

    Ok(ServiceGraph {
        nodes: nodes
            .into_iter()
            .map(|(service, (count, errors, total_ms))| GraphNode {
                service: service.to_string(),
                span_count: count,
                error_count: errors,
                avg_ms: total_ms / count.max(1) as f64,
            })
            .collect(),
        edges: edges
            .into_iter()
            .map(|((source, target), (calls, errors, total_ms))| GraphEdge {
                source: source.to_string(),
                target: target.to_string(),
                calls,
                errors,
                avg_ms: total_ms / calls.max(1) as f64,
            })
            .collect(),
        sampled_traces: sampled,
    })
}

/// Bucket matching logs over time, split by coarse severity level.
pub async fn log_histogram(
    storage: &DynStorage,
    mut q: LogQuery,
    buckets: usize,
    time_min: Option<u64>,
    time_max: Option<u64>,
    kql: Option<&crate::kql::Expr>,
) -> anyhow::Result<Vec<LogBucket>> {
    q.limit = 5_000;
    let mut logs = storage.query_logs(q).await?;
    if let Some(expr) = kql {
        logs.retain(|l| crate::kql::eval(expr, l));
    }
    if logs.is_empty() {
        return Ok(Vec::new());
    }
    let t_min = time_min.unwrap_or_else(|| logs.iter().map(|l| l.time_unix_nano).min().unwrap());
    let t_max = time_max
        .unwrap_or_else(|| logs.iter().map(|l| l.time_unix_nano).max().unwrap())
        .max(t_min + 1);
    let buckets = buckets.clamp(5, 200) as u64;
    let width = ((t_max - t_min) / buckets).max(1);

    let mut out: Vec<LogBucket> = (0..buckets)
        .map(|i| LogBucket {
            time_unix_nano: t_min + i * width,
            trace: 0,
            debug: 0,
            info: 0,
            warn: 0,
            error: 0,
            fatal: 0,
        })
        .collect();
    for l in &logs {
        let idx = (l.time_unix_nano.saturating_sub(t_min) / width).min(buckets - 1) as usize;
        bump(&mut out[idx], l);
    }
    Ok(out)
}

fn bump(b: &mut LogBucket, l: &LogRecord) {
    match l.severity_number {
        1..=4 => b.trace += 1,
        5..=8 => b.debug += 1,
        13..=16 => b.warn += 1,
        17..=20 => b.error += 1,
        21..=24 => b.fatal += 1,
        _ => b.info += 1,
    }
}
