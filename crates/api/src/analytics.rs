//! Derived analytics: service RED stats, the service dependency graph and
//! the log-volume histogram.
//!
//! Everything is computed from the [`Storage`] trait's existing queries so
//! all backends (memory, duckdb, jaeger, remote) get these for free: recent
//! traces are sampled (up to [`MAX_TRACES`]) and aggregated span-by-span.

use std::collections::BTreeMap;

use anyhow::Context;
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
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
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
        // By characters, not bytes: String::truncate panics when the byte
        // offset lands inside a multibyte character, and track titles made
        // that a live crash — every field-sidebar request against a value
        // with an accent past position 60 took the API worker down.
        if let Some((cut, _)) = value.char_indices().nth(60) {
            value.truncate(cut);
        }
        *fields.entry(name).or_default().entry(value).or_default() += 1;
    }
    let mut out: Vec<FieldInfo> = fields
        .into_iter()
        .map(|(name, values)| {
            let count = values.values().sum();
            let mut top: Vec<(String, u64)> = values.into_iter().collect();
            top.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
            top.truncate(5);
            FieldInfo {
                name,
                count,
                top_values: top,
            }
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

    // Traces fetch concurrently. Each get_trace against a remote storage
    // costs a round-trip (~200ms here), and fetching a few hundred samples
    // one after another put the services screen at a minute per load — all
    // of it network waiting, none of it query time. Sixteen in flight keeps
    // well under the storage pool while collapsing the wall time to
    // roughly samples/16 round-trips.
    const CONCURRENT_FETCHES: usize = 16;
    let mut spans = Vec::new();
    for batch in summaries.chunks(CONCURRENT_FETCHES) {
        let mut tasks = tokio::task::JoinSet::new();
        for s in batch {
            let storage = storage.clone();
            let trace_id = s.trace_id.clone();
            tasks.spawn(async move { storage.get_trace(&trace_id).await });
        }
        while let Some(joined) = tasks.join_next().await {
            spans.extend(joined.context("trace fetch task")??);
        }
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
    let (spans, _) =
        sample_spans(storage, start_time_min_unix_nano, start_time_max_unix_nano).await?;
    let mut by_service: BTreeMap<&str, Vec<&SpanRecord>> = BTreeMap::new();
    for s in &spans {
        by_service
            .entry(s.service_name.as_str())
            .or_default()
            .push(s);
    }
    let t_min = spans
        .iter()
        .map(|s| s.start_time_unix_nano)
        .min()
        .unwrap_or(0);
    let t_max = spans
        .iter()
        .map(|s| s.end_time_unix_nano)
        .max()
        .unwrap_or(0);
    let window_secs = ((t_max.saturating_sub(t_min)) as f64 / 1e9).max(1.0);

    Ok(by_service
        .into_iter()
        .map(|(service, spans)| {
            let mut durations: Vec<f64> = spans
                .iter()
                .map(|s| s.duration_nanos() as f64 / 1e6)
                .collect();
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
    let by_id: BTreeMap<(&str, &str), &SpanRecord> = spans
        .iter()
        .map(|s| ((s.trace_id.as_str(), s.span_id.as_str()), s))
        .collect();
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

/// Logs pulled per page while walking the interval.
const HISTOGRAM_PAGE: usize = 10_000;

/// Ceiling on rows scanned for one histogram, so an interval holding millions
/// of logs cannot pin the process. Only a timestamp and a severity are kept
/// per row, so the working set here is a few tens of MB at the limit rather
/// than that many whole records.
const HISTOGRAM_MAX_SCANNED: usize = 1_000_000;

/// Bucket matching logs over time, split by coarse severity level.
///
/// The interval is walked newest-first in pages rather than read in one capped
/// query. A single capped query counted only the newest N logs while the
/// buckets still spanned the whole window, so anything busier than that cap
/// rendered as bars over the most recent slice and zeros across the rest — a
/// chart that reported a quiet hour with a late spike no matter what the hour
/// actually held.
pub async fn log_histogram(
    storage: &DynStorage,
    q: LogQuery,
    buckets: usize,
    time_min: Option<u64>,
    time_max: Option<u64>,
    // Optional query predicate. Taken as a closure rather than a specific
    // AST so KQL and Lucene share this path; both evaluate in Rust anyway.
    matches: Option<&(dyn Fn(&LogRecord) -> bool + Send + Sync)>,
) -> anyhow::Result<Vec<LogBucket>> {
    // Only what bucketing needs, so the cap can be generous.
    let mut samples: Vec<(u64, i32)> = Vec::new();
    let mut cursor = time_max;
    let mut scanned = 0usize;

    loop {
        let mut page_q = q.clone();
        page_q.limit = HISTOGRAM_PAGE;
        page_q.time_max_unix_nano = cursor;

        let page = storage.query_logs(page_q).await?;
        let page_len = page.len();
        scanned += page_len;

        let mut oldest: Option<u64> = None;
        for l in &page {
            oldest = Some(oldest.map_or(l.time_unix_nano, |o: u64| o.min(l.time_unix_nano)));
        }
        for l in page {
            if matches.is_none_or(|f| f(&l)) {
                samples.push((l.time_unix_nano, l.severity_number));
            }
        }

        // Only an empty page proves the interval is exhausted. "Shorter than
        // requested" does not: a backend may clamp the page to its own search
        // depth (the postgres storage caps at MAX_SEARCH_DEPTH, 1000 by
        // default), and treating its clamped-but-full pages as the end put
        // every log after the first thousand back out of the histogram — the
        // very truncation this pager exists to remove. The price of the
        // stricter test is one empty query at the end of the walk.
        if page_len == 0 {
            break;
        }
        if scanned >= HISTOGRAM_MAX_SCANNED {
            tracing::warn!(
                scanned,
                limit = HISTOGRAM_MAX_SCANNED,
                "log histogram hit its scan ceiling; older buckets in this window are incomplete"
            );
            break;
        }
        let Some(oldest) = oldest else { break };

        // Step strictly past the oldest row of this page. Backends differ on
        // whether the bound is inclusive (DuckDB) or exclusive (the remote
        // storage API), and stepping a nanosecond past it terminates on both.
        // Rows sharing that exact nanosecond across a page boundary are the
        // one thing this can miss, which needs 10_000 logs to land on the same
        // nanosecond to happen at all.
        let next = oldest.saturating_sub(1);
        if time_min.is_some_and(|t_min| next < t_min) || cursor == Some(next) || next == 0 {
            break;
        }
        cursor = Some(next);
    }

    if samples.is_empty() {
        return Ok(Vec::new());
    }

    let t_min = time_min.unwrap_or_else(|| samples.iter().map(|(t, _)| *t).min().unwrap());
    let t_max = time_max
        .unwrap_or_else(|| samples.iter().map(|(t, _)| *t).max().unwrap())
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
    for (time, severity) in &samples {
        let idx = (time.saturating_sub(t_min) / width).min(buckets - 1) as usize;
        bump(&mut out[idx], *severity);
    }
    Ok(out)
}

fn bump(b: &mut LogBucket, severity_number: i32) {
    match severity_number {
        1..=4 => b.trace += 1,
        5..=8 => b.debug += 1,
        13..=16 => b.warn += 1,
        17..=20 => b.error += 1,
        21..=24 => b.fatal += 1,
        _ => b.info += 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// String::truncate panics mid-character; a track title with an accent
    /// past position 60 took the whole fields endpoint down in production.
    #[test]
    fn field_values_truncate_on_character_boundaries() {
        // 59 ASCII chars then a two-byte character spanning bytes 59..61,
        // putting byte offset 60 inside it.
        let long = format!("{}ééééé", "x".repeat(59));
        let fields = summarize_fields(vec![("title".to_string(), long)].into_iter(), 10);
        assert_eq!(fields.len(), 1);
        let value = &fields[0].top_values[0].0;
        assert_eq!(value.chars().count(), 60);
    }
}
