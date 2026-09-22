//! A full MCP session against a seeded instance, over the HTTP transport.
//!
//! The unit tests check pieces; this drives the thing a client actually
//! drives — initialize, the initialized notification, then every tool in
//! the catalog — and asserts on what comes back, because a tool that
//! answers successfully with the wrong telemetry is the failure that a
//! green test suite is supposed to catch.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use otelview_config::Config;
use otelview_mcp::{http, Direct, Mcp};
use serde_json::{json, Value};
use tower::ServiceExt;

mod common;
use common::{app, seeded, TRACE};

/// One JSON-RPC round trip, as a client makes it.
async fn rpc(app: &Router, body: Value) -> Value {
    let resp = app
        .clone()
        .oneshot(
            Request::post("/mcp")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "request {body} was refused");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Call a tool and return (text, structuredContent, is_error).
async fn tool(app: &Router, name: &str, arguments: Value) -> (String, Value, bool) {
    let v = rpc(
        app,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
               "params":{"name":name,"arguments":arguments}}),
    )
    .await;
    let result = v
        .get("result")
        .unwrap_or_else(|| panic!("{name} failed at the protocol level: {v}"));
    (
        result["content"][0]["text"].as_str().unwrap().to_string(),
        result["structuredContent"].clone(),
        result["isError"].as_bool().unwrap(),
    )
}

async fn ok_tool(app: &Router, name: &str, arguments: Value) -> (String, Value) {
    let (text, data, is_error) = tool(app, name, arguments).await;
    assert!(!is_error, "{name} answered with an error: {text}");
    (text, data)
}

#[tokio::test]
async fn a_client_can_open_a_session_and_list_what_is_on_offer() {
    let app = app().await;

    let init = rpc(
        &app,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18",
            "clientInfo":{"name":"integration","version":"1.0"},
            "capabilities":{}
        }}),
    )
    .await;
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "otelview");

    // The notification that follows initialize carries no id, so it gets
    // an accepted-with-no-body rather than a response.
    let resp = app
        .clone()
        .oneshot(
            Request::post("/mcp")
                .body(Body::from(
                    json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let tools = rpc(&app, json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for expected in [
        "list_services",
        "search_traces",
        "get_trace",
        "search_logs",
        "query_metric",
        "investigate_trace",
        "query_syntax",
    ] {
        assert!(names.contains(&expected), "{expected} is not listed");
    }

    let prompts = rpc(
        &app,
        json!({"jsonrpc":"2.0","id":3,"method":"prompts/list"}),
    )
    .await;
    assert!(!prompts["result"]["prompts"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn the_service_tools_see_the_topology() {
    let app = app().await;

    let (text, data) = ok_tool(&app, "list_services", json!({})).await;
    assert!(text.contains("gateway") && text.contains("payments") && text.contains("db"));
    assert_eq!(data["result"].as_array().unwrap().len(), 3);

    let (text, _) = ok_tool(&app, "list_operations", json!({"service": "gateway"})).await;
    assert!(text.contains("POST /checkout"), "{text}");

    let (text, _) = ok_tool(&app, "service_stats", json!({})).await;
    // payments failed its only request, so it leads a list sorted by error
    // rate — the ordering is the point of the tool.
    let first = text
        .lines()
        .find(|l| l.starts_with("| payments") || l.starts_with("| gateway"))
        .unwrap();
    assert!(first.starts_with("| payments"), "{text}");
    assert!(first.contains("100.0%"), "{text}");

    let (text, data) = ok_tool(&app, "service_graph", json!({})).await;
    assert!(text.contains("gateway") && text.contains("payments"));
    let edges = data["edges"].as_array().unwrap();
    assert!(edges
        .iter()
        .any(|e| e["source"] == "gateway" && e["target"] == "payments" && e["errors"] == 1));
}

#[tokio::test]
async fn traces_can_be_found_by_filter_and_by_either_query_language() {
    let app = app().await;

    let (text, data) = ok_tool(&app, "search_traces", json!({"limit": 10})).await;
    assert_eq!(data["result"].as_array().unwrap().len(), 2, "{text}");

    let (text, data) = ok_tool(&app, "search_traces", json!({"errors_only": true})).await;
    assert_eq!(data["result"].as_array().unwrap().len(), 1, "{text}");
    assert!(text.contains(TRACE));

    // TraceQL predicates on the whole spanset.
    let (text, data) = ok_tool(
        &app,
        "search_traces",
        json!({"traceql": "{ status = error && duration > 100ms }"}),
    )
    .await;
    assert_eq!(data["result"].as_array().unwrap().len(), 1, "{text}");
    assert!(text.contains(TRACE));

    // Lucene matches a trace when any one of its spans matches.
    let (_, data) = ok_tool(
        &app,
        "search_traces",
        json!({"lucene": "http.status_code:[500 TO *]"}),
    )
    .await;
    assert_eq!(data["result"].as_array().unwrap().len(), 1);

    // A query that matches nothing is an empty result, not an error.
    let (text, is_empty) = ok_tool(
        &app,
        "search_traces",
        json!({"traceql": "{ name = \"nothing-here\" }"}),
    )
    .await;
    assert!(text.contains("No traces matched"), "{text}");
    assert!(is_empty["result"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_trace_renders_as_a_waterfall_with_its_spans_nested() {
    let app = app().await;

    let (text, data) = ok_tool(&app, "get_trace", json!({"trace_id": TRACE})).await;
    assert_eq!(data["result"].as_array().unwrap().len(), 3);
    assert!(text.contains("POST /checkout") && text.contains("SELECT accounts"));
    assert!(text.contains("ERROR: upstream timeout"), "{text}");

    let gateway = text.find("gateway").unwrap();
    let payments = text.find("payments").unwrap();
    let db = text.find("db SELECT").unwrap();
    let indent = |at: usize| {
        let line_start = text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
        text[line_start..at].chars().count()
    };
    assert!(indent(gateway) < indent(payments), "{text}");
    assert!(indent(payments) < indent(db), "{text}");

    // The JSON form carries everything the waterfall leaves out.
    let (text, _) = ok_tool(
        &app,
        "get_trace",
        json!({"trace_id": TRACE, "format": "json"}),
    )
    .await;
    assert!(text.contains("resource_attributes") && text.contains("node-1"));
}

#[tokio::test]
async fn logs_can_be_searched_bucketed_and_discovered() {
    let app = app().await;

    let (text, data) = ok_tool(&app, "search_logs", json!({})).await;
    assert_eq!(data["result"].as_array().unwrap().len(), 3, "{text}");

    let (text, data) = ok_tool(&app, "search_logs", json!({"kql": "level:error"})).await;
    assert_eq!(data["result"].as_array().unwrap().len(), 1, "{text}");
    assert!(text.contains("connection refused"));

    let (_, data) = ok_tool(
        &app,
        "search_logs",
        json!({"lucene": "\"connection refused\" AND service:payments"}),
    )
    .await;
    assert_eq!(data["result"].as_array().unwrap().len(), 1);

    let (_, data) = ok_tool(&app, "search_logs", json!({"trace_id": TRACE})).await;
    assert_eq!(data["result"].as_array().unwrap().len(), 2);

    let (text, data) = ok_tool(&app, "log_histogram", json!({"buckets": 4})).await;
    let total: u64 = data["result"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            ["trace", "debug", "info", "warn", "error", "fatal"]
                .iter()
                .map(|k| b[k].as_u64().unwrap())
                .sum::<u64>()
        })
        .sum();
    assert_eq!(total, 3, "{text}");

    let (text, _) = ok_tool(&app, "list_log_fields", json!({})).await;
    // The synthesised facets and the real attributes both show up.
    assert!(text.contains("service") && text.contains("level"), "{text}");
    assert!(text.contains("http.method"), "{text}");
}

#[tokio::test]
async fn metrics_and_their_exemplars_are_reachable() {
    let app = app().await;

    let (text, _) = ok_tool(&app, "list_metrics", json!({})).await;
    assert!(
        text.contains("http.server.requests") && text.contains("sum"),
        "{text}"
    );

    let (text, data) = ok_tool(
        &app,
        "query_metric",
        json!({"name": "http.server.requests"}),
    )
    .await;
    assert_eq!(data["result"].as_array().unwrap().len(), 1, "{text}");
    assert!(text.contains("gateway"));

    // The exemplar is the only honest metric-to-trace link, and it points
    // at the span that failed.
    let (text, data) = ok_tool(&app, "find_exemplars", json!({"trace_id": TRACE})).await;
    assert_eq!(data["result"].as_array().unwrap().len(), 1, "{text}");
    assert!(text.contains("http.server.requests") && text.contains("a2"));

    // An all-zero id is what an unset trace context serializes to; it
    // matches nothing rather than being searched for.
    let (_, data) = ok_tool(
        &app,
        "find_exemplars",
        json!({"trace_id": "00000000000000000000000000000000"}),
    )
    .await;
    assert!(data["result"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn investigate_trace_answers_across_all_three_signals_at_once() {
    let app = app().await;
    let (text, data) = ok_tool(&app, "investigate_trace", json!({"trace_id": TRACE})).await;

    assert!(text.contains("# Trace 4bf92f"), "{text}");
    assert!(text.contains("Root: gateway POST /checkout"), "{text}");
    // The error span, named with its status message.
    assert!(text.contains("payments charge") && text.contains("upstream timeout"));
    // The waterfall.
    assert!(text.contains("SELECT accounts"));
    // The logs correlated by trace id — and not the unrelated one.
    assert!(text.contains("connection refused"));
    assert!(!text.contains("retrying checkout"), "{text}");
    // The exemplar.
    assert!(text.contains("http.server.requests"));

    assert_eq!(data["span_count"], 3);
    assert_eq!(data["error_count"], 1);
    assert_eq!(data["logs"].as_array().unwrap().len(), 2);
    assert_eq!(data["exemplars"].as_array().unwrap().len(), 1);
    assert_eq!(data["services"], json!(["db", "gateway", "payments"]));
}

#[tokio::test]
async fn a_mistyped_query_comes_back_to_the_model_rather_than_as_a_protocol_error() {
    let app = app().await;

    let (text, _, is_error) = tool(&app, "search_traces", json!({"traceql": "{bad"})).await;
    assert!(is_error);
    assert!(text.contains("invalid TraceQL query"), "{text}");
    // And it is told where to look the syntax up.
    assert!(text.contains("query_syntax"), "{text}");

    let (text, _, is_error) = tool(&app, "search_logs", json!({"kql": "level:(("})).await;
    assert!(is_error);
    assert!(text.contains("invalid KQL query"), "{text}");

    // A trace that does not exist is a tool error too, not a 500.
    let (text, _, is_error) = tool(&app, "get_trace", json!({"trace_id": "nope"})).await;
    assert!(is_error);
    assert!(text.contains("No trace"), "{text}");

    // Arguments that do not parse are a *protocol* error: the model did
    // not call the tool wrongly, the client did.
    let v = rpc(
        &app,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
               "params":{"name":"get_trace","arguments":{"trace_id": 7}}}),
    )
    .await;
    assert_eq!(v["error"]["code"], -32602);
}

#[tokio::test]
async fn the_query_language_references_are_served_as_tools_and_as_resources() {
    let app = app().await;

    for (language, marker) in [
        ("kql", "Kibana-style"),
        ("traceql", "spanset"),
        ("lucene", "proximity"),
    ] {
        let (text, _) = ok_tool(&app, "query_syntax", json!({"language": language})).await;
        assert!(
            text.contains(marker),
            "{language} sheet reads wrong: {text}"
        );

        let v = rpc(
            &app,
            json!({"jsonrpc":"2.0","id":1,"method":"resources/read",
                   "params":{"uri": format!("otelview://syntax/{language}")}}),
        )
        .await;
        assert!(v["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains(marker));
    }

    let (text, _, is_error) = tool(&app, "query_syntax", json!({"language": "promql"})).await;
    assert!(is_error);
    assert!(text.contains("kql"), "{text}");
}

#[tokio::test]
async fn live_resources_read_the_same_instance_the_tools_do() {
    let app = app().await;

    let v = rpc(
        &app,
        json!({"jsonrpc":"2.0","id":1,"method":"resources/read",
               "params":{"uri":"otelview://services"}}),
    )
    .await;
    let text = v["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("gateway") && text.contains("payments"));
    assert_eq!(v["result"]["contents"][0]["mimeType"], "application/json");

    let v = rpc(
        &app,
        json!({"jsonrpc":"2.0","id":2,"method":"resources/read",
               "params":{"uri":"otelview://stats"}}),
    )
    .await;
    let stats: Value =
        serde_json::from_str(v["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(stats["spans"], 4);
    assert_eq!(stats["logs"], 3);

    // A redacted config, never the live one.
    let v = rpc(
        &app,
        json!({"jsonrpc":"2.0","id":3,"method":"resources/read",
               "params":{"uri":"otelview://config"}}),
    )
    .await;
    assert!(v["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("backend"));
}

/// Whatever a tool answers, the structured half has to be an object — the
/// spec requires it, and a client that validates will reject a bare array.
#[tokio::test]
async fn every_tool_returns_text_and_a_structured_object() {
    let app = app().await;
    let listed = rpc(&app, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;

    for t in listed["result"]["tools"].as_array().unwrap() {
        let name = t["name"].as_str().unwrap();
        let mut arguments = serde_json::Map::new();
        for req in t["inputSchema"]["required"].as_array().unwrap() {
            let key = req.as_str().unwrap();
            arguments.insert(
                key.to_string(),
                match key {
                    "language" => json!("lucene"),
                    "trace_id" => json!(TRACE),
                    "name" => json!("http.server.requests"),
                    _ => json!("gateway"),
                },
            );
        }
        let (text, data) = ok_tool(&app, name, Value::Object(arguments)).await;
        assert!(!text.trim().is_empty(), "{name} rendered nothing");
        assert!(data.is_object(), "{name} returned {data}, not an object");
    }
}

#[tokio::test]
async fn a_guarded_endpoint_needs_its_token_and_refuses_foreign_origins() {
    let mcp = Mcp::new(Arc::new(Direct::new(
        seeded().await,
        Arc::new(Config::default()),
    )));
    let app = http::router(mcp, "/mcp", http::Auth::bearer("sekret"));
    let ping = || json!({"jsonrpc":"2.0","id":1,"method":"ping"}).to_string();

    let send = |app: Router, headers: Vec<(&'static str, &'static str)>| async move {
        let mut req = Request::post("/mcp");
        for (k, v) in headers {
            req = req.header(k, v);
        }
        app.oneshot(req.body(Body::from(ping())).unwrap())
            .await
            .unwrap()
            .status()
    };

    assert_eq!(send(app.clone(), vec![]).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        send(app.clone(), vec![("authorization", "Bearer wrong")]).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(app.clone(), vec![("authorization", "Bearer sekret")]).await,
        StatusCode::OK
    );
    // The origin check runs before the token, so a foreign page cannot
    // even use a token it somehow obtained.
    assert_eq!(
        send(
            app,
            vec![
                ("authorization", "Bearer sekret"),
                ("origin", "https://evil.example")
            ]
        )
        .await,
        StatusCode::FORBIDDEN
    );
}
