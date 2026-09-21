//! Trace summarization shared by all storage backends.

use otelview_model::{SpanRecord, TraceSummary};
use std::collections::BTreeMap;

/// Group spans by trace id and build one summary per trace, newest first.
/// Exemplar hits for a trace, newest first, from any iterator of metric
/// points. Shared so the memory and duckdb backends agree on what counts as
/// a match and on the ordering.
pub fn collect_exemplar_hits<'a>(
    points: impl Iterator<Item = &'a otelview_model::MetricPoint>,
    trace_id: &str,
    span_id: Option<&str>,
    limit: usize,
) -> Vec<otelview_model::ExemplarHit> {
    let mut hits: Vec<otelview_model::ExemplarHit> = Vec::new();
    for p in points {
        for e in p.exemplars() {
            if !e.trace_id.eq_ignore_ascii_case(trace_id) {
                continue;
            }
            if let Some(s) = span_id {
                if !e.span_id.eq_ignore_ascii_case(s) {
                    continue;
                }
            }
            hits.push(otelview_model::ExemplarHit {
                metric_name: p.name.clone(),
                service_name: p.service_name.clone(),
                metric_type: p.metric_type,
                unit: p.unit.clone(),
                exemplar: e,
            });
        }
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.exemplar.time_unix_nano));
    if limit > 0 {
        hits.truncate(limit);
    }
    hits
}

pub fn build_trace_summaries(spans: &[SpanRecord]) -> Vec<TraceSummary> {
    let mut by_trace: BTreeMap<&str, Vec<&SpanRecord>> = BTreeMap::new();
    for s in spans {
        by_trace.entry(s.trace_id.as_str()).or_default().push(s);
    }
    let mut out: Vec<TraceSummary> = by_trace
        .into_iter()
        .map(|(trace_id, spans)| summarize_trace(trace_id, &spans))
        .collect();
    out.sort_by_key(|t| std::cmp::Reverse(t.start_time_unix_nano));
    out
}

fn summarize_trace(trace_id: &str, spans: &[&SpanRecord]) -> TraceSummary {
    // Root = span without a parent, or the earliest span when the root span
    // wasn't received (partial trace).
    let root = spans
        .iter()
        .find(|s| s.is_root())
        .or_else(|| spans.iter().min_by_key(|s| s.start_time_unix_nano))
        .expect("summarize_trace called with at least one span");
    let start = spans
        .iter()
        .map(|s| s.start_time_unix_nano)
        .min()
        .unwrap_or(0);
    let end = spans
        .iter()
        .map(|s| s.end_time_unix_nano)
        .max()
        .unwrap_or(0);
    let mut services: Vec<String> = spans.iter().map(|s| s.service_name.clone()).collect();
    services.sort();
    services.dedup();
    TraceSummary {
        trace_id: trace_id.to_string(),
        root_name: root.name.clone(),
        root_service: root.service_name.clone(),
        start_time_unix_nano: start,
        duration_nanos: end.saturating_sub(start),
        span_count: spans.len() as u64,
        error_count: spans.iter().filter(|s| s.is_error()).count() as u64,
        services,
    }
}

/// True when a span satisfies every span-level predicate of the query.
pub fn span_matches(s: &SpanRecord, q: &otelview_model::TraceQuery) -> bool {
    if let Some(service) = &q.service {
        if !service.is_empty() && &s.service_name != service {
            return false;
        }
    }
    if let Some(op) = &q.operation {
        if !op.is_empty() && &s.name != op {
            return false;
        }
    }
    if q.errors_only && !s.is_error() {
        return false;
    }
    let dur = s.duration_nanos();
    if let Some(min) = q.min_duration_nanos {
        if dur < min {
            return false;
        }
    }
    if let Some(max) = q.max_duration_nanos {
        if dur > max {
            return false;
        }
    }
    if let Some(min) = q.start_time_min_unix_nano {
        if s.start_time_unix_nano < min {
            return false;
        }
    }
    if let Some(max) = q.start_time_max_unix_nano {
        if s.start_time_unix_nano > max {
            return false;
        }
    }
    if let Some(attr_q) = &q.attribute_query {
        if !attr_q.is_empty() {
            let hay = format!("{} {}", s.attributes, s.resource_attributes);
            // "key=value" filters on a specific attribute, anything else is a
            // plain substring match over all attributes.
            let matched = match attr_q.split_once('=') {
                Some((k, v)) => {
                    lookup(&s.attributes, k)
                        .map(|found| found == v.trim())
                        .unwrap_or(false)
                        || lookup(&s.resource_attributes, k)
                            .map(|found| found == v.trim())
                            .unwrap_or(false)
                }
                None => hay.contains(attr_q.as_str()),
            };
            if !matched {
                return false;
            }
        }
    }
    true
}

fn lookup(attrs: &serde_json::Value, key: &str) -> Option<String> {
    attrs.get(key.trim()).map(|v| match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelview_model::TraceQuery;
    use serde_json::json;

    fn span(trace: &str, id: &str, parent: &str, svc: &str, start: u64, end: u64) -> SpanRecord {
        SpanRecord {
            trace_id: trace.into(),
            span_id: id.into(),
            parent_span_id: parent.into(),
            name: format!("op-{id}"),
            service_name: svc.into(),
            kind: "server".into(),
            start_time_unix_nano: start,
            end_time_unix_nano: end,
            status_code: 0,
            status_message: String::new(),
            attributes: json!({"http.method": "GET"}),
            resource_attributes: json!({"service.name": svc}),
            events: json!([]),
            links: json!([]),
            scope_name: String::new(),
            scope_version: String::new(),
        }
    }

    #[test]
    fn summarizes_and_sorts_newest_first() {
        let spans = vec![
            span("t1", "a", "", "svc-a", 100, 500),
            span("t1", "b", "a", "svc-b", 150, 400),
            span("t2", "c", "", "svc-a", 900, 950),
        ];
        let sums = build_trace_summaries(&spans);
        assert_eq!(sums.len(), 2);
        assert_eq!(sums[0].trace_id, "t2");
        let t1 = &sums[1];
        assert_eq!(t1.root_name, "op-a");
        assert_eq!(t1.span_count, 2);
        assert_eq!(t1.duration_nanos, 400);
        assert_eq!(t1.services, vec!["svc-a".to_string(), "svc-b".to_string()]);
    }

    #[test]
    fn matches_attribute_queries() {
        let s = span("t", "a", "", "svc", 0, 10);
        let mut q = TraceQuery {
            attribute_query: Some("http.method=GET".into()),
            ..Default::default()
        };
        assert!(span_matches(&s, &q));
        q.attribute_query = Some("http.method=POST".into());
        assert!(!span_matches(&s, &q));
        q.attribute_query = Some("GET".into());
        assert!(span_matches(&s, &q));
        q.attribute_query = Some("service.name=svc".into());
        assert!(span_matches(&s, &q));
    }

    #[test]
    fn matches_duration_and_errors() {
        let mut s = span("t", "a", "", "svc", 0, 100);
        let q = TraceQuery {
            min_duration_nanos: Some(50),
            ..Default::default()
        };
        assert!(span_matches(&s, &q));
        let q = TraceQuery {
            min_duration_nanos: Some(200),
            ..Default::default()
        };
        assert!(!span_matches(&s, &q));
        let q = TraceQuery {
            errors_only: true,
            ..Default::default()
        };
        assert!(!span_matches(&s, &q));
        s.status_code = 2;
        assert!(span_matches(&s, &q));
    }
}
