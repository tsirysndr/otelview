//! Rendering telemetry for a reader.
//!
//! Tool results carry the same answer twice: JSON for a client that parses
//! it, and this — tables and a waterfall — for the model that reads it. A
//! hundred trace summaries as raw JSON is mostly punctuation; as a table it
//! is something you can scan, and the model does scan it.

use chrono::{DateTime, SecondsFormat, Utc};
use otelview_model::SpanRecord;
use serde_json::Value;

/// Unix nanos as an ISO-8601 instant, or "-" for an unset timestamp.
pub fn ts(nanos: u64) -> String {
    if nanos == 0 {
        return "-".into();
    }
    match DateTime::<Utc>::from_timestamp(
        (nanos / 1_000_000_000) as i64,
        (nanos % 1_000_000_000) as u32,
    ) {
        Some(t) => t.to_rfc3339_opts(SecondsFormat::Millis, true),
        None => nanos.to_string(),
    }
}

/// A duration in nanos, in whichever unit reads smallest.
pub fn dur(nanos: u64) -> String {
    let n = nanos as f64;
    if nanos < 1_000 {
        format!("{nanos}ns")
    } else if nanos < 1_000_000 {
        format!("{:.1}µs", n / 1e3)
    } else if nanos < 1_000_000_000 {
        format!("{:.1}ms", n / 1e6)
    } else if nanos < 60_000_000_000 {
        format!("{:.2}s", n / 1e9)
    } else {
        format!("{:.1}m", n / 6e10)
    }
}

/// Cut to `max` characters (not bytes — a multibyte cut panics), with an
/// ellipsis when something was removed.
pub fn clip(s: &str, max: usize) -> String {
    let s = s.replace(['\n', '\r'], " ");
    if s.chars().count() <= max {
        return s;
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// Milliseconds since `started`, rounded to microseconds.
///
/// Rounded because this is a log field: `1.2138749999999998` is the same
/// measurement as `1.214` and harder to read in every place it appears.
pub fn elapsed_ms(started: std::time::Instant) -> f64 {
    (started.elapsed().as_secs_f64() * 1e6).round() / 1e3
}

/// A log body (or any JSON value) as a single line.
pub fn one_line(v: &Value) -> String {
    match v {
        Value::String(s) => s.replace(['\n', '\r'], " "),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// A markdown table. Empty rows render as a note instead, because an
/// empty table reads as a rendering failure rather than an empty result.
pub fn table(headers: &[&str], rows: Vec<Vec<String>>) -> String {
    if rows.is_empty() {
        return "(no rows)".into();
    }
    let mut out = String::new();
    out.push_str(&format!("| {} |\n", headers.join(" | ")));
    out.push_str(&format!(
        "| {} |\n",
        headers
            .iter()
            .map(|_| "---")
            .collect::<Vec<_>>()
            .join(" | ")
    ));
    for row in rows {
        out.push_str(&format!("| {} |\n", row.join(" | ")));
    }
    out
}

/// How many spans a waterfall draws before it stops.
const MAX_WATERFALL_SPANS: usize = 300;

/// Width of the timing bar, in characters.
const BAR: usize = 32;

/// The trace as a tree, each span placed and sized against the trace's own
/// span of time — the same picture the UI draws, in text.
///
/// Orphans (a span whose parent was sampled away or has not arrived) are
/// rendered as roots rather than dropped: a partial trace is still worth
/// reading, and silently losing half of it is worse than showing it flat.
pub fn waterfall(spans: &[SpanRecord]) -> String {
    if spans.is_empty() {
        return "(no spans)".into();
    }
    let t0 = spans
        .iter()
        .map(|s| s.start_time_unix_nano)
        .min()
        .unwrap_or(0);
    let t1 = spans
        .iter()
        .map(|s| s.end_time_unix_nano)
        .max()
        .unwrap_or(t0 + 1);
    let total = (t1.saturating_sub(t0)).max(1) as f64;

    let ids: std::collections::HashSet<&str> = spans.iter().map(|s| s.span_id.as_str()).collect();
    let mut children: std::collections::HashMap<&str, Vec<&SpanRecord>> = Default::default();
    let mut roots: Vec<&SpanRecord> = Vec::new();
    for s in spans {
        if s.parent_span_id.is_empty() || !ids.contains(s.parent_span_id.as_str()) {
            roots.push(s);
        } else {
            children
                .entry(s.parent_span_id.as_str())
                .or_default()
                .push(s);
        }
    }
    let by_start = |v: &mut Vec<&SpanRecord>| v.sort_by_key(|s| s.start_time_unix_nano);
    by_start(&mut roots);
    for v in children.values_mut() {
        by_start(v);
    }

    let mut out = String::new();
    out.push_str(&format!(
        "{} spans over {} from {}\n\n",
        spans.len(),
        dur(t1 - t0),
        ts(t0)
    ));
    let mut drawn = 0usize;
    let mut stack: Vec<(&SpanRecord, usize)> = roots.iter().rev().map(|s| (*s, 0usize)).collect();
    while let Some((span, depth)) = stack.pop() {
        if drawn >= MAX_WATERFALL_SPANS {
            out.push_str(&format!("… {} more spans not shown\n", spans.len() - drawn));
            break;
        }
        drawn += 1;
        let offset = ((span.start_time_unix_nano.saturating_sub(t0)) as f64 / total * BAR as f64)
            .floor() as usize;
        let width = ((span.duration_nanos() as f64 / total * BAR as f64).round() as usize).max(1);
        let offset = offset.min(BAR.saturating_sub(1));
        let width = width.min(BAR - offset);
        let bar = format!(
            "{}{}{}",
            " ".repeat(offset),
            "█".repeat(width),
            " ".repeat(BAR - offset - width)
        );
        out.push_str(&format!(
            "{bar} {:>9}  {}{} {} [{}]{}\n",
            dur(span.duration_nanos()),
            "  ".repeat(depth),
            span.service_name,
            span.name,
            span.span_id,
            if span.is_error() {
                format!(
                    " ERROR{}",
                    if span.status_message.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", clip(&span.status_message, 60))
                    }
                )
            } else {
                String::new()
            }
        ));
        if let Some(kids) = children.get(span.span_id.as_str()) {
            for kid in kids.iter().rev() {
                stack.push((kid, depth + 1));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn span(id: &str, parent: &str, start: u64, dur: u64) -> SpanRecord {
        SpanRecord {
            trace_id: "t".into(),
            span_id: id.into(),
            parent_span_id: parent.into(),
            name: format!("op-{id}"),
            service_name: "svc".into(),
            kind: "server".into(),
            start_time_unix_nano: start,
            end_time_unix_nano: start + dur,
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

    #[test]
    fn durations_pick_a_readable_unit() {
        assert_eq!(dur(900), "900ns");
        assert_eq!(dur(1_500), "1.5µs");
        assert_eq!(dur(2_500_000), "2.5ms");
        assert_eq!(dur(1_500_000_000), "1.50s");
    }

    #[test]
    fn elapsed_is_rounded_to_microseconds() {
        let started = std::time::Instant::now();
        let ms = elapsed_ms(started);
        assert!(ms >= 0.0);
        // Three decimal places, whatever the measurement was.
        assert_eq!(ms, (ms * 1e3).round() / 1e3);
    }

    #[test]
    fn clip_counts_characters_not_bytes() {
        assert_eq!(clip("héllo wörld", 6), "héllo…");
        assert_eq!(clip("short", 40), "short");
    }

    #[test]
    fn waterfall_nests_children_under_parents() {
        let spans = vec![
            span("a", "", 0, 1_000_000),
            span("b", "a", 100_000, 400_000),
            span("c", "b", 200_000, 100_000),
        ];
        let out = waterfall(&spans);
        let lines: Vec<&str> = out.lines().filter(|l| l.contains("op-")).collect();
        assert_eq!(lines.len(), 3);
        // Depth shows as indentation, so each level is further right than
        // the one above it. Counted in characters: the bar is drawn with a
        // three-byte block, so byte offsets move with the bar, not the
        // indent.
        let indent = |l: &str| l[..l.find("svc").unwrap()].chars().count();
        assert!(indent(lines[0]) < indent(lines[1]));
        assert!(indent(lines[1]) < indent(lines[2]));
    }

    /// A span whose parent was never received still has to appear.
    #[test]
    fn orphans_are_drawn_as_roots() {
        let spans = vec![span("b", "missing-parent", 0, 10)];
        assert!(waterfall(&spans).contains("op-b"));
    }
}
