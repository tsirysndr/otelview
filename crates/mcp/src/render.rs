//! Per-result renderers: one telemetry shape in, one readable block out.

use otelview_api::analytics::{FieldInfo, LogBucket, ServiceGraph, ServiceStats};
use otelview_model::{
    severity_level, ExemplarHit, LogRecord, MetricInfo, MetricSeries, StorageStats, TraceSummary,
};

use crate::fmt::{clip, dur, one_line, table, ts};

pub fn services(names: &[String]) -> String {
    if names.is_empty() {
        return "No services have reported any telemetry.".into();
    }
    format!("{} services:\n{}", names.len(), bullets(names))
}

pub fn operations(service: &str, names: &[String]) -> String {
    if names.is_empty() {
        return format!("No operations recorded for service {service:?}.");
    }
    format!(
        "{} operations in {service}:\n{}",
        names.len(),
        bullets(names)
    )
}

fn bullets(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("- {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn traces(list: &[TraceSummary]) -> String {
    if list.is_empty() {
        return "No traces matched.".into();
    }
    let rows = list
        .iter()
        .map(|t| {
            vec![
                t.trace_id.clone(),
                clip(&t.root_service, 24),
                clip(&t.root_name, 40),
                ts(t.start_time_unix_nano),
                dur(t.duration_nanos),
                t.span_count.to_string(),
                t.error_count.to_string(),
                clip(&t.services.join(", "), 48),
            ]
        })
        .collect();
    format!(
        "{} traces:\n\n{}",
        list.len(),
        table(
            &[
                "trace_id",
                "service",
                "root span",
                "start",
                "duration",
                "spans",
                "errors",
                "services"
            ],
            rows
        )
    )
}

pub fn logs(list: &[LogRecord]) -> String {
    if list.is_empty() {
        return "No logs matched.".into();
    }
    let rows = list
        .iter()
        .map(|l| {
            vec![
                ts(l.time_unix_nano),
                severity_level(l.severity_number).to_string(),
                clip(&l.service_name, 20),
                clip(&one_line(&l.body), 100),
                if l.trace_id.is_empty() {
                    "-".into()
                } else {
                    l.trace_id.clone()
                },
            ]
        })
        .collect();
    format!(
        "{} log records (newest first):\n\n{}",
        list.len(),
        table(&["time", "level", "service", "body", "trace_id"], rows)
    )
}

pub fn histogram(buckets: &[LogBucket]) -> String {
    if buckets.is_empty() {
        return "No logs in the window.".into();
    }
    let total = |b: &LogBucket| b.trace + b.debug + b.info + b.warn + b.error + b.fatal;
    let peak = buckets.iter().map(total).max().unwrap_or(0).max(1);
    let mut out = String::new();
    for b in buckets {
        let n = total(b);
        // The bar is relative to the busiest bucket, which is the only
        // scale that makes a shape visible without knowing the volume.
        let width = ((n as f64 / peak as f64) * 40.0).round() as usize;
        out.push_str(&format!(
            "{} {:<40} {:>7}{}\n",
            ts(b.time_unix_nano),
            "█".repeat(width),
            n,
            if b.error + b.fatal > 0 {
                format!("  ({} err)", b.error + b.fatal)
            } else {
                String::new()
            }
        ));
    }
    let sum: u64 = buckets.iter().map(total).sum();
    let errors: u64 = buckets.iter().map(|b| b.error + b.fatal).sum();
    let warns: u64 = buckets.iter().map(|b| b.warn).sum();
    format!(
        "{sum} logs, {warns} warn, {errors} error/fatal, over {} buckets:\n\n{out}",
        buckets.len()
    )
}

pub fn fields(what: &str, list: &[FieldInfo]) -> String {
    if list.is_empty() {
        return format!("No {what} fields found in the window.");
    }
    let rows = list
        .iter()
        .map(|f| {
            vec![
                f.name.clone(),
                f.count.to_string(),
                clip(
                    &f.top_values
                        .iter()
                        .map(|(v, c)| format!("{v} ({c})"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    90,
                ),
            ]
        })
        .collect();
    format!(
        "{} {what} fields, most common first:\n\n{}",
        list.len(),
        table(&["field", "count", "top values"], rows)
    )
}

pub fn service_stats(list: &[ServiceStats]) -> String {
    if list.is_empty() {
        return "No traces in the window, so there are no service stats.".into();
    }
    let mut list = list.to_vec();
    // Worst first: the reason to ask for RED metrics is to find what is
    // broken, and the answer should not need re-sorting.
    list.sort_by(|a, b| {
        b.error_rate
            .total_cmp(&a.error_rate)
            .then(b.p95_ms.total_cmp(&a.p95_ms))
    });
    let rows = list
        .iter()
        .map(|s| {
            vec![
                s.service.clone(),
                format!("{:.2}", s.rate_per_sec),
                format!("{:.1}%", s.error_rate * 100.0),
                format!("{:.1}", s.p50_ms),
                format!("{:.1}", s.p95_ms),
                format!("{:.1}", s.p99_ms),
                s.request_count.to_string(),
                s.error_count.to_string(),
            ]
        })
        .collect();
    format!(
        "RED metrics per service (worst error rate first):\n\n{}",
        table(
            &["service", "req/s", "errors", "p50 ms", "p95 ms", "p99 ms", "requests", "errors#"],
            rows
        )
    )
}

pub fn service_graph(g: &ServiceGraph) -> String {
    if g.edges.is_empty() && g.nodes.is_empty() {
        return "No traces in the window, so there is no dependency graph.".into();
    }
    let edges = table(
        &["caller", "callee", "calls", "errors", "avg ms"],
        g.edges
            .iter()
            .map(|e| {
                vec![
                    e.source.clone(),
                    e.target.clone(),
                    e.calls.to_string(),
                    e.errors.to_string(),
                    format!("{:.1}", e.avg_ms),
                ]
            })
            .collect(),
    );
    let nodes = table(
        &["service", "spans", "errors", "avg ms"],
        g.nodes
            .iter()
            .map(|n| {
                vec![
                    n.service.clone(),
                    n.span_count.to_string(),
                    n.error_count.to_string(),
                    format!("{:.1}", n.avg_ms),
                ]
            })
            .collect(),
    );
    format!(
        "Service dependency graph from {} sampled traces.\n\nCalls:\n\n{edges}\nServices:\n\n{nodes}",
        g.sampled_traces
    )
}

pub fn metrics(list: &[MetricInfo]) -> String {
    if list.is_empty() {
        return "No metrics have been received.".into();
    }
    let rows = list
        .iter()
        .map(|m| {
            vec![
                m.name.clone(),
                m.metric_type.as_str().to_string(),
                if m.unit.is_empty() {
                    "-".into()
                } else {
                    m.unit.clone()
                },
                clip(&m.services.join(", "), 40),
                clip(&m.description, 60),
            ]
        })
        .collect();
    format!(
        "{} metrics:\n\n{}",
        list.len(),
        table(&["name", "type", "unit", "services", "description"], rows)
    )
}

pub fn series(name: &str, list: &[MetricSeries]) -> String {
    if list.is_empty() {
        return format!("No series for metric {name:?} in this window.");
    }
    let rows = list
        .iter()
        .map(|s| {
            let vals: Vec<f64> = s.points.iter().map(|p| p.value).collect();
            let (min, max, avg) = if vals.is_empty() {
                (None, None, None)
            } else {
                (
                    Some(vals.iter().copied().fold(f64::INFINITY, f64::min)),
                    Some(vals.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
                    Some(vals.iter().sum::<f64>() / vals.len() as f64),
                )
            };
            vec![
                clip(&s.service_name, 20),
                clip(&one_line(&s.attributes), 50),
                s.points.len().to_string(),
                fnum(vals.first().copied()),
                fnum(vals.last().copied()),
                fnum(min),
                fnum(max),
                fnum(avg),
                s.points
                    .last()
                    .map(|p| ts(p.time_unix_nano))
                    .unwrap_or_else(|| "-".into()),
            ]
        })
        .collect();
    format!(
        "{} series for {name}:\n\n{}",
        list.len(),
        table(
            &[
                "service",
                "attributes",
                "points",
                "first",
                "last",
                "min",
                "max",
                "avg",
                "latest at"
            ],
            rows
        )
    )
}

fn fnum(v: Option<f64>) -> String {
    match v {
        Some(v) => format!("{v:.4}"),
        None => "-".into(),
    }
}

pub fn exemplars(list: &[ExemplarHit]) -> String {
    if list.is_empty() {
        return "No metric exemplars point at this trace. Metrics can still be \
                correlated by service and time window, but not by span."
            .into();
    }
    let rows = list
        .iter()
        .map(|h| {
            vec![
                h.metric_name.clone(),
                h.metric_type.as_str().to_string(),
                h.service_name.clone(),
                format!("{:.4}", h.exemplar.value),
                ts(h.exemplar.time_unix_nano),
                if h.exemplar.span_id.is_empty() {
                    "-".into()
                } else {
                    h.exemplar.span_id.clone()
                },
            ]
        })
        .collect();
    format!(
        "{} metric exemplars linked to this trace:\n\n{}",
        list.len(),
        table(
            &["metric", "type", "service", "value", "time", "span_id"],
            rows
        )
    )
}

pub fn storage_stats(s: &StorageStats) -> String {
    format!(
        "backend: {}\nspans: {}\nlogs: {}\nmetric points: {}\nservices: {}",
        s.backend, s.spans, s.logs, s.metric_points, s.services
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn log(sev: i32, body: &str) -> LogRecord {
        LogRecord {
            time_unix_nano: 1_700_000_000_000_000_000,
            observed_time_unix_nano: 0,
            severity_number: sev,
            severity_text: String::new(),
            body: json!(body),
            attributes: json!({}),
            resource_attributes: json!({}),
            service_name: "svc".into(),
            trace_id: String::new(),
            span_id: String::new(),
            scope_name: String::new(),
        }
    }

    #[test]
    fn empty_results_say_so_rather_than_drawing_an_empty_table() {
        assert!(traces(&[]).contains("No traces matched"));
        assert!(logs(&[]).contains("No logs matched"));
        assert!(services(&[]).contains("No services"));
    }

    #[test]
    fn log_rows_carry_the_level_name() {
        let out = logs(&[log(17, "boom")]);
        assert!(out.contains("error"), "{out}");
        assert!(out.contains("boom"));
    }

    /// Newlines in a body would otherwise break the table apart.
    #[test]
    fn multiline_bodies_stay_on_one_row() {
        let out = logs(&[log(9, "line one\nline two")]);
        assert_eq!(out.lines().filter(|l| l.contains("line one")).count(), 1);
        assert!(out.contains("line one line two"));
    }
}
