//! Prompts: investigations worth doing the same way every time.
//!
//! Each one is a plan the model can follow with the tools this server
//! already exposes — naming the order to call them in, which is the part a
//! model left to itself tends to get wrong (reading logs before knowing
//! which service is failing, or guessing query syntax before checking it).

use std::collections::HashMap;

use crate::protocol::{Content, GetPromptResult, Prompt, PromptArgument, PromptMessage};

pub fn catalog() -> Vec<Prompt> {
    vec![
        Prompt {
            name: "investigate_errors",
            title: "Investigate errors",
            description: "Find what is failing right now and why, from RED metrics down to a \
                single failing trace and its logs.",
            arguments: vec![
                PromptArgument {
                    name: "service",
                    description: "Service to focus on. Omit to start from the whole fleet.",
                    required: false,
                },
                PromptArgument {
                    name: "lookback",
                    description: "Time window, e.g. 15m, 1h, 24h. Defaults to 1h.",
                    required: false,
                },
            ],
        },
        Prompt {
            name: "diagnose_latency",
            title: "Diagnose latency",
            description: "Work out where time is going: which service is slow, which operation, \
                and which span inside a representative slow trace.",
            arguments: vec![
                PromptArgument {
                    name: "service",
                    description: "Service that is reported slow.",
                    required: false,
                },
                PromptArgument {
                    name: "lookback",
                    description: "Time window, e.g. 15m, 1h, 24h. Defaults to 1h.",
                    required: false,
                },
            ],
        },
        Prompt {
            name: "explain_trace",
            title: "Explain a trace",
            description: "Read one trace end to end and explain what the request did, where the \
                time went and what went wrong.",
            arguments: vec![PromptArgument {
                name: "trace_id",
                description: "Hex trace id.",
                required: true,
            }],
        },
        Prompt {
            name: "health_report",
            title: "Health report",
            description: "A written summary of the system: topology, per-service RED metrics, \
                log volume and anything anomalous.",
            arguments: vec![PromptArgument {
                name: "lookback",
                description: "Time window, e.g. 1h, 24h, 7d. Defaults to 1h.",
                required: false,
            }],
        },
    ]
}

/// `None` when the name is not one of ours.
pub fn get(name: &str, args: &HashMap<String, String>) -> Option<GetPromptResult> {
    let arg = |k: &str, default: &str| {
        args.get(k)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(default)
            .to_string()
    };
    let lookback = arg("lookback", "1h");
    let service = args
        .get("service")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let scope = match &service {
        Some(s) => format!("the service `{s}`"),
        None => "the whole system".to_string(),
    };
    let service_param = match &service {
        Some(s) => format!("\n   Pass service=\"{s}\" to every tool that accepts it."),
        None => String::new(),
    };

    let (description, text) = match name {
        "investigate_errors" => (
            format!("Investigate errors in {scope} over the last {lookback}"),
            format!(
                "Investigate what is failing in {scope} over the last {lookback}, using the \
                 otelview tools.{service_param}\n\n\
                 1. Call service_stats with lookback=\"{lookback}\" and identify the services \
                    with the highest error rate.\n\
                 2. Call service_graph for the same window to see who calls the failing service \
                    and who it calls — the cause is often downstream of the symptom.\n\
                 3. Call search_traces with errors_only=true (and the failing service) to get \
                    failing traces.\n\
                 4. Call investigate_trace on the most representative one. Read the error spans, \
                    the waterfall and the correlated logs together.\n\
                 5. Call log_histogram with min_severity=17 to see when the errors started, and \
                    search_logs to read them.\n\n\
                 Then report: what is failing, the error message and status, when it started, \
                 which service originates it versus which ones merely report it, and how many \
                 requests are affected. Quote specific trace ids and log lines as evidence. If \
                 the data does not support a conclusion, say what is missing rather than \
                 guessing."
            ),
        ),
        "diagnose_latency" => (
            format!("Diagnose latency in {scope} over the last {lookback}"),
            format!(
                "Find where time is going in {scope} over the last {lookback}, using the \
                 otelview tools.{service_param}\n\n\
                 1. Call service_stats with lookback=\"{lookback}\" and compare p50 against p95 \
                    and p99 — a wide gap means a slow tail, an even spread means everything is \
                    slow.\n\
                 2. Call list_operations for the slow service, then search_traces with \
                    min_duration_ms set near its p95 to collect slow traces.\n\
                 3. Call get_trace on several of them and read the waterfall: look for the span \
                    that holds the time, whether children run in sequence or in parallel, and \
                    whether the gap is inside a span or between them.\n\
                 4. Call query_metric on any relevant latency or saturation metric to see \
                    whether the slowness is constant or spiking.\n\n\
                 Then report: which operation is slow, which span holds the time, whether it is \
                 the tail or the whole distribution, and what the traces suggest is responsible \
                 (a downstream call, a lock, a retry, a cold cache). Name the trace ids you \
                 read."
            ),
        ),
        "explain_trace" => {
            let trace_id = arg("trace_id", "");
            (
                format!("Explain trace {trace_id}"),
                format!(
                    "Call investigate_trace with trace_id=\"{trace_id}\", then explain the \
                     request in plain language:\n\n\
                     - what the request was and which services handled it, in order\n\
                     - where the time went, as a share of the total\n\
                     - what failed, if anything, and which span failed first (a parent marked \
                       as an error is usually reporting a child's failure)\n\
                     - what the correlated logs add that the spans do not\n\n\
                     If the trace looks incomplete — orphan spans, a missing root — say so; a \
                     sampled-away parent changes what the waterfall means."
                ),
            )
        }
        "health_report" => (
            format!("Health report for the last {lookback}"),
            format!(
                "Write a health report for this system over the last {lookback}, using the \
                 otelview tools.\n\n\
                 1. storage_stats — how much telemetry there is and which backend holds it.\n\
                 2. list_services and service_graph (lookback=\"{lookback}\") — the topology.\n\
                 3. service_stats (lookback=\"{lookback}\") — rate, errors and latency per \
                    service.\n\
                 4. log_histogram (lookback=\"{lookback}\", min_severity=13) — when warnings and \
                    errors cluster.\n\
                 5. list_metrics, and query_metric on anything that looks load- or \
                    saturation-related.\n\n\
                 Report the shape of the system, what looks healthy, what does not, and what you \
                 would look at next. Be specific with numbers. Where the data is too thin to \
                 judge — a service with a handful of spans, a window with no traffic — say that \
                 rather than drawing a conclusion from it."
            ),
        ),
        _ => return None,
    };

    Some(GetPromptResult {
        description,
        messages: vec![PromptMessage {
            role: "user",
            content: Content::text(text),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalogued_prompt_can_be_fetched() {
        for p in catalog() {
            let mut args = HashMap::new();
            // Fill the required ones so nothing renders a placeholder.
            for a in &p.arguments {
                if a.required {
                    args.insert(a.name.to_string(), "abc123".to_string());
                }
            }
            let got = get(p.name, &args).unwrap_or_else(|| panic!("{} is not dispatched", p.name));
            assert!(!got.messages.is_empty(), "{} renders nothing", p.name);
        }
        assert!(get("nope", &HashMap::new()).is_none());
    }

    #[test]
    fn the_window_argument_reaches_the_text() {
        let args = HashMap::from([("lookback".to_string(), "24h".to_string())]);
        let got = get("health_report", &args).unwrap();
        let Content::Text { text } = &got.messages[0].content;
        assert!(text.contains("24h"), "{text}");
    }

    /// An argument given as an empty string is the same as not given.
    #[test]
    fn blank_arguments_fall_back_to_defaults() {
        let args = HashMap::from([("lookback".to_string(), "  ".to_string())]);
        let got = get("investigate_errors", &args).unwrap();
        let Content::Text { text } = &got.messages[0].content;
        assert!(text.contains("1h"), "{text}");
    }
}
