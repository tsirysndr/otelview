//! The JSON-RPC surface: MCP methods registered on a jsonrpsee module.
//!
//! jsonrpsee owns dispatch, parameter parsing and error objects; the
//! transports own framing. [`Mcp::handle`] is the seam — one JSON-RPC
//! message in, at most one out — and both stdio and HTTP sit on top of it
//! unchanged.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use jsonrpsee::types::{ErrorCode, ErrorObject, ErrorObjectOwned};
use jsonrpsee::RpcModule;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::backend::Otel;
use crate::fmt::elapsed_ms;
use crate::protocol::{
    negotiate_version, Implementation, InitializeParams, InitializeResult, ServerCapabilities,
};
use crate::{prompts, resources, tools};

pub const SERVER_NAME: &str = "otelview";

struct Ctx {
    otel: Arc<dyn Otel>,
    instructions: String,
}

/// An MCP server bound to one otelview.
#[derive(Clone)]
pub struct Mcp {
    module: Arc<RpcModule<Ctx>>,
}

impl Mcp {
    pub fn new(otel: Arc<dyn Otel>) -> Self {
        let instructions = instructions(otel.as_ref());
        let mut module = RpcModule::new(Ctx { otel, instructions });
        register(&mut module);
        Self {
            module: Arc::new(module),
        }
    }

    /// Handle one JSON-RPC message.
    ///
    /// `None` means "nothing to send back", which is the correct answer to
    /// a notification and to a batch of nothing but notifications.
    pub async fn handle(&self, request: &str) -> Option<String> {
        let parsed: Value = match serde_json::from_str(request) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    bytes = request.len(),
                    "mcp message was not valid JSON"
                );
                return Some(parse_error(&e.to_string()));
            }
        };
        match parsed {
            Value::Array(items) if items.is_empty() => {
                tracing::warn!("mcp batch was empty");
                Some(invalid_request("empty batch"))
            }
            Value::Array(items) => {
                tracing::debug!(messages = items.len(), "mcp batch");
                let mut out = Vec::new();
                for item in items {
                    if let Some(resp) = self.dispatch(item).await {
                        out.push(resp);
                    }
                }
                (!out.is_empty()).then(|| format!("[{}]", out.join(",")))
            }
            other => self.dispatch(other).await,
        }
    }

    /// One request object. Notifications (no `id`) are run for their effect
    /// and answered with nothing, per JSON-RPC — jsonrpsee's request type
    /// requires an id, so they never reach it.
    async fn dispatch(&self, request: Value) -> Option<String> {
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        if request.get("id").is_none() {
            tracing::debug!(method, "mcp notification");
            return None;
        }
        // Said here rather than left to the generic error response: a
        // client calling a method this server has never had is a version
        // mismatch worth seeing in the log, not a routine 404.
        if !self.module.method_names().any(|known| known == method) {
            tracing::warn!(method, "mcp method not found");
        }
        let started = Instant::now();
        let raw = request.to_string();
        let out = match self.module.raw_json_request(&raw, 1).await {
            Ok((response, _)) => Some(response.get().to_string()),
            // Only reachable if the object does not parse as a JSON-RPC
            // request at all — a missing method, say.
            Err(e) => {
                tracing::warn!(method, error = %e, "mcp request is not a JSON-RPC call");
                Some(invalid_request(&e.to_string()))
            }
        };
        tracing::debug!(
            method,
            elapsed_ms = elapsed_ms(started),
            bytes = out.as_ref().map_or(0, String::len),
            "mcp request served"
        );
        out
    }

    /// Method names this server answers, for diagnostics and tests.
    pub fn methods(&self) -> Vec<&'static str> {
        self.module.method_names().collect()
    }
}

fn parse_error(msg: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": Value::Null,
        "error": {"code": -32700, "message": format!("parse error: {msg}")}
    })
    .to_string()
}

fn invalid_request(msg: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": Value::Null,
        "error": {"code": -32600, "message": format!("invalid request: {msg}")}
    })
    .to_string()
}

/// What the model is told this server is, before it calls anything.
fn instructions(otel: &dyn Otel) -> String {
    format!(
        "otelview is an OpenTelemetry viewer holding traces, logs and metrics for {}.\n\n\
         Start with list_services or service_stats to see what exists and what is unhealthy, \
         then narrow: search_traces for requests, search_logs for records, query_metric for \
         series. investigate_trace answers everything about one trace — waterfall, error spans, \
         correlated logs, metric exemplars — in a single call.\n\n\
         Three query languages are available: KQL for logs, TraceQL for traces, and Lucene for \
         both. Call query_syntax before writing one, and list_log_fields or list_trace_fields to \
         find the field names that actually exist here rather than assuming.\n\n\
         Time windows are `lookback` (\"15m\", \"6h\", \"7d\") or an absolute start_ms/end_ms in \
         unix milliseconds. Omitting the window searches everything stored, which is slower and \
         rarely what you want. Everything is read-only: nothing here can modify or delete data.",
        otel.describe()
    )
}

/* ---------------------------------------------------------- registry -- */

#[derive(Debug, Deserialize)]
struct CallToolParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Deserialize)]
struct ReadResourceParams {
    uri: String,
}

#[derive(Debug, Deserialize)]
struct GetPromptParams {
    name: String,
    #[serde(default)]
    arguments: HashMap<String, String>,
}

/// Every method is registered here. `register_async_method` only fails on a
/// duplicate name, which is a bug in this function rather than a runtime
/// condition, so the expects are load-bearing assertions.
fn register(module: &mut RpcModule<Ctx>) {
    module
        .register_async_method("initialize", |params, ctx, _| async move {
            // A client that sends no params at all is still initializing.
            let p: InitializeParams = params.parse().unwrap_or_default();
            let negotiated = negotiate_version(p.protocol_version.as_deref());
            let client = p.client_info.as_ref();
            // One line per session, with everything needed to reproduce a
            // report: who connected, and which revision both sides settled
            // on when the client asked for one this server does not know.
            tracing::info!(
                client = client.map_or("unknown", |c| c.name.as_str()),
                client_version = client.map_or("", |c| c.version.as_str()),
                requested_protocol = p.protocol_version.as_deref().unwrap_or("unspecified"),
                protocol = negotiated,
                tools = tools::catalog().len(),
                "mcp client connected"
            );
            Ok::<_, ErrorObjectOwned>(json!(InitializeResult {
                protocol_version: negotiated,
                capabilities: ServerCapabilities::new(),
                server_info: Implementation {
                    name: SERVER_NAME.into(),
                    title: Some("otelview — OpenTelemetry viewer".into()),
                    version: env!("CARGO_PKG_VERSION").into(),
                },
                instructions: ctx.instructions.clone(),
            }))
        })
        .expect("initialize");

    module
        .register_async_method("ping", |_, _, _| async move {
            Ok::<_, ErrorObjectOwned>(json!({}))
        })
        .expect("ping");

    // Accepted and ignored: the capability is advertised so clients do not
    // treat setting a level as an error, but this server logs to its own
    // process, not over the protocol.
    module
        .register_async_method("logging/setLevel", |_, _, _| async move {
            Ok::<_, ErrorObjectOwned>(json!({}))
        })
        .expect("logging/setLevel");

    module
        .register_async_method("tools/list", |_, _, _| async move {
            Ok::<_, ErrorObjectOwned>(json!({ "tools": tools::catalog() }))
        })
        .expect("tools/list");

    module
        .register_async_method("tools/call", |params, ctx, _| async move {
            let p: CallToolParams = params
                .parse()
                .inspect_err(|e| tracing::warn!(error = %e, "tools/call params did not parse"))?;
            let started = Instant::now();
            let args = summarize(&p.arguments);
            match tools::call(ctx.otel.as_ref(), &p.name, p.arguments).await {
                Ok(result) => {
                    // The access log of this server: what the agent asked
                    // for, with what, and how long it took. `failed` has
                    // already said *why* an error result is an error.
                    let elapsed_ms = elapsed_ms(started);
                    if result.is_error {
                        tracing::warn!(tool = %p.name, %args, elapsed_ms, "mcp tool call failed");
                    } else {
                        tracing::info!(tool = %p.name, %args, elapsed_ms, "mcp tool call");
                    }
                    Ok(json!(result))
                }
                Err(e) => {
                    tracing::warn!(tool = %p.name, %args, error = %e, "mcp tool call rejected");
                    Err(invalid_params(e.to_string()))
                }
            }
        })
        .expect("tools/call");

    module
        .register_async_method("resources/list", |_, _, _| async move {
            Ok::<_, ErrorObjectOwned>(json!({ "resources": resources::catalog() }))
        })
        .expect("resources/list");

    // No templated resources, but the method has to exist: a client that
    // asks and gets "method not found" logs it as a server fault.
    module
        .register_async_method("resources/templates/list", |_, _, _| async move {
            Ok::<_, ErrorObjectOwned>(json!({ "resourceTemplates": [] }))
        })
        .expect("resources/templates/list");

    module
        .register_async_method("resources/read", |params, ctx, _| async move {
            let p: ReadResourceParams = params.parse().inspect_err(
                |e| tracing::warn!(error = %e, "resources/read params did not parse"),
            )?;
            let started = Instant::now();
            match resources::read(ctx.otel.as_ref(), &p.uri).await {
                Some(Ok(contents)) => {
                    tracing::info!(
                        uri = %p.uri,
                        elapsed_ms = elapsed_ms(started),
                        bytes = contents.text.len(),
                        "mcp resource read"
                    );
                    Ok(json!({ "contents": [contents] }))
                }
                Some(Err(e)) => {
                    tracing::warn!(uri = %p.uri, error = %format!("{e:#}"), "mcp resource failed");
                    Err(ErrorObject::owned(
                        ErrorCode::InternalError.code(),
                        format!("reading {}: {e:#}", p.uri),
                        None::<()>,
                    ))
                }
                // The spec's own code for "that resource does not exist".
                None => {
                    tracing::warn!(uri = %p.uri, "mcp resource not found");
                    Err(ErrorObject::owned(
                        -32002,
                        format!("unknown resource {}", p.uri),
                        Some(json!({ "uri": p.uri })),
                    ))
                }
            }
        })
        .expect("resources/read");

    module
        .register_async_method("prompts/list", |_, _, _| async move {
            Ok::<_, ErrorObjectOwned>(json!({ "prompts": prompts::catalog() }))
        })
        .expect("prompts/list");

    module
        .register_async_method("prompts/get", |params, _, _| async move {
            let p: GetPromptParams = params
                .parse()
                .inspect_err(|e| tracing::warn!(error = %e, "prompts/get params did not parse"))?;
            match prompts::get(&p.name, &p.arguments) {
                Some(result) => {
                    tracing::info!(prompt = %p.name, arguments = p.arguments.len(), "mcp prompt");
                    Ok(json!(result))
                }
                None => {
                    tracing::warn!(prompt = %p.name, "mcp prompt not found");
                    Err(invalid_params(format!("unknown prompt {:?}", p.name)))
                }
            }
        })
        .expect("prompts/get");

    // Never reached through `handle`, which answers notifications with
    // nothing, but registered so a client that wrongly sends them with an
    // id gets an ack rather than "method not found".
    for notification in ["notifications/initialized", "notifications/cancelled"] {
        module
            .register_async_method(notification, |_, _, _| async move {
                Ok::<_, ErrorObjectOwned>(json!({}))
            })
            .expect("notification ack");
    }
}

/// Tool arguments as one short line. Logging what an agent asked for is
/// most of the value of logging the call at all, but a Lucene query with a
/// thousand terms should not take the log with it.
fn summarize(arguments: &Value) -> String {
    match arguments {
        Value::Null => "{}".to_string(),
        other => crate::fmt::clip(&other.to_string(), 200),
    }
}

fn invalid_params(msg: String) -> ErrorObjectOwned {
    ErrorObject::owned(ErrorCode::InvalidParams.code(), msg, None::<()>)
}

/// The unused-variable lint has no way to know `_` params are the API.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::sample_otel;

    async fn call(mcp: &Mcp, body: Value) -> Value {
        let out = mcp
            .handle(&body.to_string())
            .await
            .expect("a request with an id gets a response");
        serde_json::from_str(&out).unwrap()
    }

    #[tokio::test]
    async fn initialize_reports_capabilities_and_instructions() {
        let mcp = Mcp::new(sample_otel().await);
        let v = call(
            &mcp,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-03-26",
                "clientInfo":{"name":"test","version":"1"},
                "capabilities":{}
            }}),
        )
        .await;
        // The client asked for a version we support, so it is echoed back
        // rather than replaced with ours.
        assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(v["result"]["serverInfo"]["name"], "otelview");
        assert!(v["result"]["capabilities"]["tools"].is_object());
        assert!(v["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("query_syntax"));
    }

    #[tokio::test]
    async fn an_unknown_protocol_version_falls_back_to_ours() {
        let mcp = Mcp::new(sample_otel().await);
        let v = call(
            &mcp,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}),
        )
        .await;
        assert_eq!(
            v["result"]["protocolVersion"],
            crate::protocol::PROTOCOL_VERSION
        );
    }

    #[tokio::test]
    async fn notifications_get_no_response() {
        let mcp = Mcp::new(sample_otel().await);
        let out = mcp
            .handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await;
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn a_batch_answers_only_the_requests() {
        let mcp = Mcp::new(sample_otel().await);
        let out = mcp
            .handle(
                &json!([
                    {"jsonrpc":"2.0","method":"notifications/initialized"},
                    {"jsonrpc":"2.0","id":7,"method":"ping"}
                ])
                .to_string(),
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["id"], 7);
    }

    #[tokio::test]
    async fn malformed_json_is_a_parse_error() {
        let mcp = Mcp::new(sample_otel().await);
        let out = mcp.handle("{not json").await.unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["error"]["code"], -32700);
    }

    #[tokio::test]
    async fn an_unknown_method_is_method_not_found() {
        let mcp = Mcp::new(sample_otel().await);
        let v = call(&mcp, json!({"jsonrpc":"2.0","id":1,"method":"nope/atall"})).await;
        assert_eq!(v["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn tools_list_matches_the_catalog() {
        let mcp = Mcp::new(sample_otel().await);
        let v = call(&mcp, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;
        let listed = v["result"]["tools"].as_array().unwrap();
        assert_eq!(listed.len(), tools::catalog().len());
        assert!(listed
            .iter()
            .any(|t| t["name"] == "search_traces" && t["inputSchema"]["type"] == "object"));
    }

    #[tokio::test]
    async fn every_catalogued_tool_is_dispatchable() {
        let mcp = Mcp::new(sample_otel().await);
        for tool in tools::catalog() {
            // Required arguments get a plausible value so the call gets
            // past parsing; the point is that no tool answers "unknown".
            let mut args = serde_json::Map::new();
            for req in tool.input_schema["required"].as_array().unwrap() {
                let key = req.as_str().unwrap();
                args.insert(
                    key.to_string(),
                    match key {
                        "language" => json!("kql"),
                        _ => json!("svc-a"),
                    },
                );
            }
            let v = call(
                &mcp,
                json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                    "name": tool.name, "arguments": args
                }}),
            )
            .await;
            assert!(
                v["result"]["content"][0]["text"].is_string(),
                "{} answered {v}",
                tool.name
            );
        }
    }

    #[tokio::test]
    async fn an_unknown_tool_is_an_invalid_params_error() {
        let mcp = Mcp::new(sample_otel().await);
        let v = call(
            &mcp,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"drop_everything"}}),
        )
        .await;
        assert_eq!(v["error"]["code"], -32602);
        assert!(v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("drop_everything"));
    }

    #[tokio::test]
    async fn resources_and_prompts_round_trip() {
        let mcp = Mcp::new(sample_otel().await);
        let v = call(
            &mcp,
            json!({"jsonrpc":"2.0","id":1,"method":"resources/list"}),
        )
        .await;
        assert!(!v["result"]["resources"].as_array().unwrap().is_empty());

        let v = call(
            &mcp,
            json!({"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"otelview://syntax/traceql"}}),
        )
        .await;
        assert!(v["result"]["contents"][0]["text"]
            .as_str()
            .unwrap()
            .contains("spanset"));

        let v = call(
            &mcp,
            json!({"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"otelview://nothing"}}),
        )
        .await;
        assert_eq!(v["error"]["code"], -32002);

        let v = call(
            &mcp,
            json!({"jsonrpc":"2.0","id":4,"method":"prompts/get","params":{
                "name":"investigate_errors","arguments":{"service":"svc-a","lookback":"30m"}
            }}),
        )
        .await;
        let text = v["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap();
        assert!(text.contains("svc-a") && text.contains("30m"));
    }
}
