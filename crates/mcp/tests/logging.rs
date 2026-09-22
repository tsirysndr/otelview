//! What the server says about itself while it works.
//!
//! Logs are an interface too: they are what an operator has when an agent
//! reports that a tool "returned nothing", and the two things that must
//! hold are that every call leaves a trace of what was asked, and that no
//! line ever contains a credential.

use std::io;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use otelview_mcp::{http, Mcp};
use serde_json::{json, Value};
use tower::ServiceExt;
use tracing::Level;
use tracing_subscriber::fmt::MakeWriter;

mod common;
use common::{app, seeded, TRACE};

/// A subscriber writing into a buffer the test can read back.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }

    /// The captured lines that recorded an event at `level`.
    fn at(&self, level: Level) -> Vec<String> {
        let tag = format!(" {level} ");
        self.text()
            .lines()
            .filter(|l| l.contains(&tag))
            .map(str::to_string)
            .collect()
    }
}

struct Sink(Arc<Mutex<Vec<u8>>>);

impl io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Captured {
    type Writer = Sink;
    fn make_writer(&'a self) -> Self::Writer {
        Sink(self.0.clone())
    }
}

/// Capture everything this server logs while `body` runs.
///
/// `set_default` is thread-local and `#[tokio::test]` drives the future on
/// the calling thread, so nothing escapes the capture and nothing leaks
/// into another test.
async fn capture<F, Fut>(body: F) -> Captured
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(captured.clone())
        .with_max_level(Level::TRACE)
        .with_ansi(false)
        .with_target(true)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    body().await;
    captured
}

async fn post(app: &Router, headers: Vec<(&str, &str)>, body: Value) -> StatusCode {
    let mut req = Request::post("/mcp");
    for (k, v) in headers {
        req = req.header(k, v);
    }
    app.clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn a_served_tool_call_is_logged_with_its_name_and_arguments() {
    let logs = capture(|| async {
        let app = app().await;
        post(
            &app,
            vec![],
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"search_traces","arguments":{"service":"payments","errors_only":true}
            }}),
        )
        .await;
    })
    .await;

    let call = logs
        .at(Level::INFO)
        .into_iter()
        .find(|l| l.contains("mcp tool call"))
        .unwrap_or_else(|| panic!("no tool call was logged:\n{}", logs.text()));
    assert!(call.contains("tool=search_traces"), "{call}");
    // The arguments are what makes the line worth having: without them a
    // log of "search_traces" says nothing about why it found what it did.
    assert!(
        call.contains("payments") && call.contains("errors_only"),
        "{call}"
    );
    assert!(call.contains("elapsed_ms"), "{call}");
}

#[tokio::test]
async fn a_tool_that_answers_with_an_error_is_logged_as_a_warning() {
    let logs = capture(|| async {
        let app = app().await;
        post(
            &app,
            vec![],
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"search_traces","arguments":{"traceql":"{bad"}
            }}),
        )
        .await;
    })
    .await;

    let warnings = logs.at(Level::WARN);
    assert!(
        warnings.iter().any(|l| l.contains("mcp tool call failed")),
        "expected a warning, got:\n{}",
        logs.text()
    );
    // The query the model got wrong is recorded, at debug, so an operator
    // can see what it actually wrote.
    assert!(
        logs.text().contains("invalid TraceQL query"),
        "{}",
        logs.text()
    );
}

#[tokio::test]
async fn a_session_records_who_connected_and_on_which_protocol() {
    let logs = capture(|| async {
        let app = app().await;
        post(
            &app,
            vec![],
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-03-26",
                "clientInfo":{"name":"claude-desktop","version":"9.9"}
            }}),
        )
        .await;
    })
    .await;

    let line = logs
        .at(Level::INFO)
        .into_iter()
        .find(|l| l.contains("mcp client connected"))
        .unwrap_or_else(|| panic!("no connection was logged:\n{}", logs.text()));
    assert!(line.contains("claude-desktop"), "{line}");
    assert!(line.contains("9.9"), "{line}");
    // Both the version asked for and the one settled on, which is the only
    // way to diagnose a client that disagrees about the revision. The
    // leading space keeps `protocol=` from matching `requested_protocol=`.
    assert!(
        line.contains(r#"requested_protocol="2025-03-26""#),
        "{line}"
    );
    assert!(line.contains(r#" protocol="2025-03-26""#), "{line}");
}

#[tokio::test]
async fn refusals_are_logged_but_never_with_the_credential() {
    let logs = capture(|| async {
        let mcp = Mcp::new(Arc::new(otelview_mcp::Direct::new(
            seeded().await,
            Arc::new(otelview_config::Config::default()),
        )));
        let app = http::router(mcp, "/mcp", http::Auth::bearer("sekret"));
        let ping = json!({"jsonrpc":"2.0","id":1,"method":"ping"});

        assert_eq!(
            post(&app, vec![], ping.clone()).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            post(
                &app,
                vec![("authorization", "Bearer hunter2")],
                ping.clone()
            )
            .await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            post(
                &app,
                vec![
                    ("authorization", "Bearer sekret"),
                    ("origin", "https://evil.example")
                ],
                ping,
            )
            .await,
            StatusCode::FORBIDDEN
        );
    })
    .await;

    let text = logs.text();
    let warnings = logs.at(Level::WARN);
    assert_eq!(
        warnings
            .iter()
            .filter(|l| l.contains("missing or invalid token"))
            .count(),
        2,
        "both refusals should be logged:\n{text}"
    );
    assert!(
        warnings
            .iter()
            .any(|l| l.contains("foreign browser origin") && l.contains("evil.example")),
        "{text}"
    );
    // The point of the whole test: a log that leaks the token is worse
    // than no log at all, and a wrong guess must not be recorded either.
    assert!(
        !text.contains("sekret"),
        "the token reached the log:\n{text}"
    );
    assert!(
        !text.contains("hunter2"),
        "a guess reached the log:\n{text}"
    );
}

#[tokio::test]
async fn a_method_this_server_does_not_have_is_logged() {
    let logs = capture(|| async {
        let app = app().await;
        post(
            &app,
            vec![],
            json!({"jsonrpc":"2.0","id":1,"method":"sampling/createMessage"}),
        )
        .await;
    })
    .await;

    assert!(
        logs.at(Level::WARN)
            .iter()
            .any(|l| l.contains("mcp method not found") && l.contains("sampling/createMessage")),
        "{}",
        logs.text()
    );
}

/// Notifications and served requests are routine, so they stay below info:
/// an operator watching a busy server should see tool calls, not framing.
#[tokio::test]
async fn routine_traffic_stays_out_of_the_info_log() {
    let logs = capture(|| async {
        let app = app().await;
        post(
            &app,
            vec![],
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
        post(
            &app,
            vec![],
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        )
        .await;
    })
    .await;

    assert!(logs.at(Level::INFO).is_empty(), "{}", logs.text());
    assert!(
        logs.at(Level::DEBUG)
            .iter()
            .any(|l| l.contains("mcp notification")),
        "{}",
        logs.text()
    );
    assert!(
        logs.at(Level::DEBUG)
            .iter()
            .any(|l| l.contains("mcp request served") && l.contains("tools/list")),
        "{}",
        logs.text()
    );
}

/// stdio has no token and cannot have one: the pipe is the boundary. This
/// is the path `otelview mcp` takes, and it answers with no credential
/// anywhere in the picture.
#[tokio::test]
async fn the_stdio_path_answers_without_any_credentials() {
    let mcp = Mcp::new(Arc::new(otelview_mcp::Direct::new(
        seeded().await,
        Arc::new(otelview_config::Config::default()),
    )));
    let out = mcp
        .handle(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":"get_trace","arguments":{"trace_id":TRACE}}})
            .to_string(),
        )
        .await
        .expect("a request gets a response");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["result"]["isError"], false);
    assert!(v["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("POST /checkout"));
}
