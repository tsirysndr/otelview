//! The tools an agent can call, and what they do.
//!
//! Every query the web UI can make is here, under names and descriptions
//! written for a reader who has never seen this instance: the descriptions
//! are the entire interface, so they say which query language a parameter
//! takes and what the tool is *for*, not just what it returns.
//!
//! The catalog and the dispatcher are separate lists; a test walks both to
//! make sure neither grows a name the other has not heard of.

use otelview_api::query::{LogSearch, QueryError, QueryResult, SeriesSearch, TraceSearch, Window};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::backend::Otel;
use crate::fmt;
use crate::protocol::{CallToolResult, Tool, ToolAnnotations};
use crate::render;
use crate::syntax;

/// A call that never reached the tool: JSON-RPC's problem, not the model's.
#[derive(Debug)]
pub enum ToolError {
    UnknownTool(String),
    BadArguments(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::UnknownTool(name) => write!(f, "unknown tool {name:?}"),
            ToolError::BadArguments(msg) => write!(f, "invalid arguments: {msg}"),
        }
    }
}

fn args<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, ToolError> {
    // An omitted `arguments` and an empty object mean the same thing.
    let v = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(v).map_err(|e| ToolError::BadArguments(e.to_string()))
}

/* ------------------------------------------------------------ schemas -- */

fn object(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
    })
}

const NO_ARGS: fn() -> Value = || object(json!({}), &[]);

/// The three time-window parameters every windowed tool shares.
fn window_props() -> serde_json::Map<String, Value> {
    json!({
        "lookback": {
            "type": "string",
            "description": "Relative window ending now: \"30s\", \"15m\", \"6h\", \"7d\". Omit to search everything stored."
        },
        "start_ms": {
            "type": "integer",
            "description": "Absolute window start, unix epoch milliseconds. Takes precedence over lookback."
        },
        "end_ms": {
            "type": "integer",
            "description": "Absolute window end, unix epoch milliseconds."
        }
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

/// An object schema whose properties are `props` plus the shared time
/// window. Returns the whole schema, not just the properties — a tool
/// published without `"type": "object"` is one a client cannot call.
fn windowed(props: Value, required: &[&str]) -> Value {
    let mut map = props.as_object().cloned().unwrap_or_default();
    map.extend(window_props());
    object(Value::Object(map), required)
}

/* ------------------------------------------------------------ catalog -- */

pub fn catalog() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_services",
            title: "List services",
            description: "Every service that has sent telemetry to this otelview. Start here \
                when you do not yet know what is running."
                .into(),
            input_schema: NO_ARGS(),
            annotations: ToolAnnotations::read_only("List services"),
        },
        Tool {
            name: "list_operations",
            title: "List operations",
            description: "Span names (operations) recorded for one service — the endpoints, \
                jobs and queries it handles. Useful before filtering a trace search by operation."
                .into(),
            input_schema: object(
                json!({
                    "service": {"type": "string", "description": "Service name, as returned by list_services."}
                }),
                &["service"],
            ),
            annotations: ToolAnnotations::read_only("List operations"),
        },
        Tool {
            name: "service_stats",
            title: "Service RED metrics",
            description: "Request rate, error rate and p50/p95/p99 latency per service, \
                computed from a sample of recent traces. The fastest way to see which service \
                is unhealthy."
                .into(),
            input_schema: windowed(json!({}), &[]),
            annotations: ToolAnnotations::read_only("Service RED metrics"),
        },
        Tool {
            name: "service_graph",
            title: "Service dependency graph",
            description: "Which services call which, with call counts, error counts and average \
                latency per edge. Derived from parent/child spans across sampled traces."
                .into(),
            input_schema: windowed(json!({}), &[]),
            annotations: ToolAnnotations::read_only("Service dependency graph"),
        },
        Tool {
            name: "search_traces",
            title: "Search traces",
            description: "Find traces. Filter by service, operation, duration, errors and time, \
                or write a query in TraceQL (`traceql`) or Lucene (`lucene`) — call query_syntax \
                for either grammar. Returns one summary row per trace; pass a trace_id to \
                get_trace for the spans."
                .into(),
            input_schema: windowed(
                json!({
                    "service": {"type": "string", "description": "Only traces involving this service."},
                    "operation": {"type": "string", "description": "Only traces whose root span has this name."},
                    "q": {"type": "string", "description": "Plain substring or key=value match over span attributes. Not a query language."},
                    "traceql": {"type": "string", "description": "TraceQL, e.g. `{ status = error && duration > 100ms }`."},
                    "lucene": {"type": "string", "description": "Lucene, e.g. `service:gateway AND http.status_code:[500 TO *]`. A trace matches when any one span does."},
                    "min_duration_ms": {"type": "number", "description": "Only traces at least this long."},
                    "max_duration_ms": {"type": "number", "description": "Only traces at most this long."},
                    "errors_only": {"type": "boolean", "description": "Only traces containing an error span."},
                    "limit": {"type": "integer", "description": "Maximum traces to return (default 20, max 500)."}
                }),
                &[],
            ),
            annotations: ToolAnnotations::read_only("Search traces"),
        },
        Tool {
            name: "get_trace",
            title: "Get a trace",
            description: "Every span of one trace, drawn as a waterfall: parent/child nesting, \
                each span placed and sized against the trace's own duration, errors marked. \
                Pass format=\"json\" for the raw spans with all attributes and events."
                .into(),
            input_schema: object(
                json!({
                    "trace_id": {"type": "string", "description": "Hex trace id, as returned by search_traces."},
                    "format": {
                        "type": "string",
                        "enum": ["waterfall", "json"],
                        "description": "waterfall (default) reads as a tree; json returns full span records."
                    }
                }),
                &["trace_id"],
            ),
            annotations: ToolAnnotations::read_only("Get a trace"),
        },
        Tool {
            name: "investigate_trace",
            title: "Investigate a trace",
            description: "Everything known about one trace in a single call: the waterfall, the \
                error spans called out, the log records correlated to it, and any metric \
                exemplars that point at it. Use this instead of chaining get_trace, search_logs \
                and find_exemplars by hand."
                .into(),
            input_schema: object(
                json!({
                    "trace_id": {"type": "string", "description": "Hex trace id."},
                    "log_limit": {"type": "integer", "description": "Maximum correlated log records (default 100)."}
                }),
                &["trace_id"],
            ),
            annotations: ToolAnnotations::read_only("Investigate a trace"),
        },
        Tool {
            name: "list_trace_fields",
            title: "Discover span attributes",
            description: "Span and resource attribute keys present in recent traces, with their \
                most common values. Call this before writing a TraceQL or Lucene trace query so \
                the field names are real ones."
                .into(),
            input_schema: windowed(
                json!({
                    "service": {"type": "string", "description": "Restrict the sample to one service."}
                }),
                &[],
            ),
            annotations: ToolAnnotations::read_only("Discover span attributes"),
        },
        Tool {
            name: "search_logs",
            title: "Search logs",
            description: "Find log records. Filter by service, severity, plain substring, trace \
                id and time, or write a query in KQL (`kql`) or Lucene (`lucene`) — call \
                query_syntax for either grammar. Newest first."
                .into(),
            input_schema: windowed(
                json!({
                    "service": {"type": "string", "description": "Only logs from this service."},
                    "min_severity": {"type": "integer", "description": "Minimum OTLP severity number, inclusive: 5 debug, 9 info, 13 warn, 17 error, 21 fatal."},
                    "search": {"type": "string", "description": "Plain substring match over body and attributes. Not a query language."},
                    "kql": {"type": "string", "description": "KQL, e.g. `http.method:POST and http.status_code:>=500`."},
                    "lucene": {"type": "string", "description": "Lucene, e.g. `level:ERROR AND \"connection refused\"`."},
                    "trace_id": {"type": "string", "description": "Only logs correlated to this trace."},
                    "limit": {"type": "integer", "description": "Maximum records to return (default 200, max 5000)."}
                }),
                &[],
            ),
            annotations: ToolAnnotations::read_only("Search logs"),
        },
        Tool {
            name: "log_histogram",
            title: "Log volume over time",
            description: "Log counts per severity, bucketed across the window, drawn as a bar \
                chart. Takes the same filters as search_logs, so you can see when a pattern \
                spiked before reading the records."
                .into(),
            input_schema: windowed(
                json!({
                    "service": {"type": "string", "description": "Only logs from this service."},
                    "min_severity": {"type": "integer", "description": "Minimum OTLP severity number, inclusive."},
                    "search": {"type": "string", "description": "Plain substring match over body and attributes."},
                    "kql": {"type": "string", "description": "KQL filter."},
                    "lucene": {"type": "string", "description": "Lucene filter."},
                    "buckets": {"type": "integer", "description": "Number of time buckets (default 40, max 500)."}
                }),
                &[],
            ),
            annotations: ToolAnnotations::read_only("Log volume over time"),
        },
        Tool {
            name: "list_log_fields",
            title: "Discover log fields",
            description: "Log attribute keys present in the window, with their most common \
                values, plus the synthesised `service`, `level` and `scope` fields. Call this \
                before writing a KQL or Lucene log query."
                .into(),
            input_schema: windowed(
                json!({
                    "service": {"type": "string", "description": "Restrict the sample to one service."},
                    "min_severity": {"type": "integer", "description": "Restrict the sample by severity."}
                }),
                &[],
            ),
            annotations: ToolAnnotations::read_only("Discover log fields"),
        },
        Tool {
            name: "list_metrics",
            title: "List metrics",
            description: "The metric catalog: every metric name received, with its type \
                (gauge, sum, histogram, …), unit, description and reporting services."
                .into(),
            input_schema: NO_ARGS(),
            annotations: ToolAnnotations::read_only("List metrics"),
        },
        Tool {
            name: "query_metric",
            title: "Query a metric",
            description: "Time series for one metric, one row per (service, attributes) series \
                with point count and first/last/min/max/avg. `func` applies rate or increase per \
                series; `agg` collapses all series into one with sum, avg, min or max."
                .into(),
            input_schema: windowed(
                json!({
                    "name": {"type": "string", "description": "Metric name, as returned by list_metrics."},
                    "service": {"type": "string", "description": "Only series from this service."},
                    "func": {"type": "string", "enum": ["raw", "rate", "increase"], "description": "Per-series function (default raw)."},
                    "agg": {"type": "string", "enum": ["none", "sum", "avg", "min", "max"], "description": "Cross-series aggregation (default none)."},
                    "max_points": {"type": "integer", "description": "Maximum points per series, downsampled if exceeded (default 500)."}
                }),
                &["name"],
            ),
            annotations: ToolAnnotations::read_only("Query a metric"),
        },
        Tool {
            name: "find_exemplars",
            title: "Find metric exemplars",
            description: "Metrics carrying an exemplar that points at a trace or span. This is \
                the only real metric-to-trace link in OpenTelemetry: without an exemplar a \
                metric relates to a service and a time window, not to a span."
                .into(),
            input_schema: object(
                json!({
                    "trace_id": {"type": "string", "description": "Hex trace id."},
                    "span_id": {"type": "string", "description": "Narrow to one span within the trace."},
                    "limit": {"type": "integer", "description": "Maximum hits (default 50, max 500)."}
                }),
                &["trace_id"],
            ),
            annotations: ToolAnnotations::read_only("Find metric exemplars"),
        },
        Tool {
            name: "storage_stats",
            title: "Storage stats",
            description: "How much telemetry this instance holds — spans, logs, metric points, \
                services — and which storage backend is behind it."
                .into(),
            input_schema: NO_ARGS(),
            annotations: ToolAnnotations::read_only("Storage stats"),
        },
        Tool {
            name: "get_config",
            title: "Get configuration",
            description: "The running configuration with secrets redacted: receivers, storage \
                backend, retention and UI settings."
                .into(),
            input_schema: NO_ARGS(),
            annotations: ToolAnnotations::read_only("Get configuration"),
        },
        Tool {
            name: "query_syntax",
            title: "Query language reference",
            description: "The grammar and worked examples for one of the query languages: \
                \"kql\" (logs), \"traceql\" (traces) or \"lucene\" (both). Read this before \
                writing a query rather than guessing at the syntax."
                .into(),
            input_schema: object(
                json!({
                    "language": {"type": "string", "enum": ["kql", "traceql", "lucene"], "description": "Which language to explain."}
                }),
                &["language"],
            ),
            annotations: ToolAnnotations::read_only("Query language reference"),
        },
    ]
}

/* ----------------------------------------------------------- dispatch -- */

#[derive(Debug, Deserialize)]
struct ServiceArg {
    service: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WindowArgs {
    #[serde(flatten)]
    window: Window,
    service: Option<String>,
    min_severity: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct TraceIdArgs {
    trace_id: String,
    #[serde(default)]
    format: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InvestigateArgs {
    trace_id: String,
    #[serde(default)]
    log_limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ExemplarArgs {
    trace_id: String,
    #[serde(default)]
    span_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct HistogramArgs {
    #[serde(flatten)]
    search: LogSearch,
    buckets: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SyntaxArgs {
    language: String,
}

/// Run one tool. A failure inside the query comes back as a tool result
/// with `is_error` set, because the model is the one who can act on it;
/// only an unknown tool or unparseable arguments is a protocol error.
pub async fn call(
    otel: &dyn Otel,
    name: &str,
    arguments: Value,
) -> Result<CallToolResult, ToolError> {
    let result = match name {
        "list_services" => run(otel.services().await, |v| render::services(v)),
        "list_operations" => {
            let a: ServiceArg = args(arguments)?;
            run(otel.operations(&a.service).await, move |v| {
                render::operations(&a.service, v)
            })
        }
        "service_stats" => {
            let a: WindowArgs = args(arguments)?;
            run(otel.service_stats(a.window).await, |v| {
                render::service_stats(v)
            })
        }
        "service_graph" => {
            let a: WindowArgs = args(arguments)?;
            run(otel.service_graph(a.window).await, |v| {
                render::service_graph(v)
            })
        }
        "search_traces" => {
            let a: TraceSearch = args(arguments)?;
            run(otel.search_traces(a).await, |v| render::traces(v))
        }
        "get_trace" => {
            let a: TraceIdArgs = args(arguments)?;
            let json_format = a.format.as_deref() == Some("json");
            match otel.get_trace(&a.trace_id).await {
                Ok(spans) if spans.is_empty() => CallToolResult::error(format!(
                    "No trace {:?} in storage. It may have aged out, or the id may be from \
                     another instance.",
                    a.trace_id
                )),
                Ok(spans) => {
                    let text = if json_format {
                        serde_json::to_string_pretty(&spans).unwrap_or_default()
                    } else {
                        fmt::waterfall(&spans)
                    };
                    ok(text, &spans)
                }
                Err(e) => failed(e),
            }
        }
        "investigate_trace" => {
            let a: InvestigateArgs = args(arguments)?;
            investigate(otel, &a.trace_id, a.log_limit.unwrap_or(100)).await
        }
        "list_trace_fields" => {
            let a: WindowArgs = args(arguments)?;
            run(otel.trace_fields(a.service, a.window).await, |v| {
                render::fields("span attribute", v)
            })
        }
        "search_logs" => {
            let a: LogSearch = args(arguments)?;
            run(otel.search_logs(a).await, |v| render::logs(v))
        }
        "log_histogram" => {
            let a: HistogramArgs = args(arguments)?;
            let buckets = a
                .buckets
                .unwrap_or(otelview_api::query::DEFAULT_HISTOGRAM_BUCKETS);
            run(otel.log_histogram(a.search, buckets).await, |v| {
                render::histogram(v)
            })
        }
        "list_log_fields" => {
            let a: WindowArgs = args(arguments)?;
            run(
                otel.log_fields(a.service, a.min_severity, a.window).await,
                |v| render::fields("log", v),
            )
        }
        "list_metrics" => run(otel.metrics().await, |v| render::metrics(v)),
        "query_metric" => {
            let a: SeriesSearch = args(arguments)?;
            if a.name.trim().is_empty() {
                return Ok(CallToolResult::error(
                    "query_metric needs a metric `name`; call list_metrics for the catalog.",
                ));
            }
            let name = a.name.clone();
            run(otel.metric_series(a).await, move |v| {
                render::series(&name, v)
            })
        }
        "find_exemplars" => {
            let a: ExemplarArgs = args(arguments)?;
            run(
                otel.exemplars(&a.trace_id, a.span_id.as_deref(), a.limit)
                    .await,
                |v| render::exemplars(v),
            )
        }
        "storage_stats" => run(otel.stats().await, render::storage_stats),
        "get_config" => run(otel.config().await, |v| {
            serde_json::to_string_pretty(v).unwrap_or_default()
        }),
        "query_syntax" => {
            let a: SyntaxArgs = args(arguments)?;
            match syntax::sheet(&a.language) {
                Some(sheet) => CallToolResult::ok(
                    sheet,
                    json!({"language": a.language.to_lowercase(), "reference": sheet}),
                ),
                None => CallToolResult::error(format!(
                    "No reference for {:?}. Known languages: {}.",
                    a.language,
                    syntax::LANGUAGES.join(", ")
                )),
            }
        }
        other => return Err(ToolError::UnknownTool(other.to_string())),
    };
    Ok(result)
}

/// Serialize the answer and render it, or report why it could not be had.
fn run<T: serde::Serialize>(
    result: QueryResult<T>,
    render: impl FnOnce(&T) -> String,
) -> CallToolResult {
    match result {
        Ok(v) => {
            let text = render(&v);
            ok(text, &v)
        }
        Err(e) => failed(e),
    }
}

fn ok<T: serde::Serialize>(text: String, value: &T) -> CallToolResult {
    CallToolResult::ok(text, serde_json::to_value(value).unwrap_or(Value::Null))
}

/// Phrased so the model can fix it: a rejected query is quoted back with
/// the parser's complaint, and a storage failure says that it is one.
fn failed(e: QueryError) -> CallToolResult {
    match e {
        QueryError::BadQuery(msg) => {
            // The model wrote a query that does not parse. That is a normal
            // turn in a conversation, not a fault of this server, so it is
            // recorded at debug and handed back for the model to fix.
            tracing::debug!(%msg, "mcp tool call used an invalid query");
            CallToolResult::error(format!(
                "{msg}\n\nCall query_syntax for the grammar of the language you are writing in."
            ))
        }
        QueryError::Backend(e) => {
            tracing::error!(error = %format!("{e:#}"), "storage failed during an mcp tool call");
            CallToolResult::error(format!("otelview could not answer that: {e:#}"))
        }
    }
}

/* -------------------------------------------------------- investigate -- */

/// One trace, from every angle the data supports.
async fn investigate(otel: &dyn Otel, trace_id: &str, log_limit: usize) -> CallToolResult {
    let spans = match otel.get_trace(trace_id).await {
        Ok(s) => s,
        Err(e) => return failed(e),
    };
    if spans.is_empty() {
        return CallToolResult::error(format!(
            "No trace {trace_id:?} in storage. It may have aged out, or the id may be from \
             another instance."
        ));
    }

    // A trace is still worth reading when the logs or exemplars behind it
    // cannot be fetched, so neither failure aborts the investigation — but
    // neither disappears silently either.
    let logs = otel
        .search_logs(LogSearch {
            trace_id: Some(trace_id.to_string()),
            limit: Some(log_limit),
            ..Default::default()
        })
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(trace_id, error = %e, "correlated logs could not be fetched");
            Vec::new()
        });
    // Exemplars are optional in a way logs are not: three of the four
    // storage backends have no metric store to search, so an empty answer
    // here is normal and must not read as a failure.
    let exemplars = otel
        .exemplars(trace_id, None, None)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(trace_id, error = %e, "exemplars could not be fetched");
            Vec::new()
        });

    let errors: Vec<&otelview_model::SpanRecord> = spans.iter().filter(|s| s.is_error()).collect();
    let mut services: Vec<&str> = spans.iter().map(|s| s.service_name.as_str()).collect();
    services.sort_unstable();
    services.dedup();

    let root = spans.iter().find(|s| s.is_root()).or_else(|| spans.first());
    let mut text = format!("# Trace {trace_id}\n\n");
    if let Some(root) = root {
        text.push_str(&format!(
            "Root: {} {} — {}\nServices: {}\nSpans: {} ({} errors)\n\n",
            root.service_name,
            root.name,
            fmt::dur(root.duration_nanos()),
            services.join(", "),
            spans.len(),
            errors.len()
        ));
    }

    if errors.is_empty() {
        text.push_str("No span in this trace is marked as an error.\n\n");
    } else {
        text.push_str(&format!("## {} error spans\n\n", errors.len()));
        for s in errors.iter().take(20) {
            text.push_str(&format!(
                "- {} {} ({}) [{}]{}\n",
                s.service_name,
                s.name,
                fmt::dur(s.duration_nanos()),
                s.span_id,
                if s.status_message.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", fmt::clip(&s.status_message, 120))
                }
            ));
        }
        text.push('\n');
    }

    text.push_str(&format!("## Waterfall\n\n{}\n", fmt::waterfall(&spans)));
    text.push_str(&format!("## Correlated logs\n\n{}\n", render::logs(&logs)));
    text.push_str(&format!(
        "\n## Metric exemplars\n\n{}\n",
        render::exemplars(&exemplars)
    ));

    CallToolResult::ok(
        text,
        json!({
            "trace_id": trace_id,
            "services": services,
            "span_count": spans.len(),
            "error_count": errors.len(),
            "spans": spans,
            "logs": logs,
            "exemplars": exemplars,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_description_and_an_object_schema() {
        for t in catalog() {
            assert!(!t.description.is_empty(), "{} has no description", t.name);
            assert_eq!(t.input_schema["type"], "object", "{}", t.name);
            assert!(
                t.input_schema["properties"].is_object(),
                "{} has no properties",
                t.name
            );
            assert!(t.annotations.read_only_hint, "{} is not read-only", t.name);
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let mut names: Vec<&str> = catalog().iter().map(|t| t.name).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate tool name in the catalog");
    }

    /// Required parameters have to exist in `properties`, or a client
    /// generating a form from the schema asks for a field that is ignored.
    #[test]
    fn required_parameters_are_declared() {
        for t in catalog() {
            let props = t.input_schema["properties"].as_object().unwrap();
            for req in t.input_schema["required"].as_array().unwrap() {
                let key = req.as_str().unwrap();
                assert!(
                    props.contains_key(key),
                    "{}: required {key} is not a property",
                    t.name
                );
            }
        }
    }
}
