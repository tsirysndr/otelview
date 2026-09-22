//! The instance both test binaries run against: one failing request,
//! traced end to end, with the logs and the metric exemplar that belong
//! to it.
//!
//! Shared rather than copied so that a test asserting on "the seeded
//! trace" means the same trace everywhere.
#![allow(dead_code)]

use std::sync::Arc;

use axum::Router;
use otelview_config::{Config, MemoryConfig};
use otelview_mcp::{http, Direct, Mcp};
use otelview_model::{LogRecord, MetricPoint, MetricType, SpanRecord};
use otelview_storage::memory::MemoryStorage;
use otelview_storage::DynStorage;
use serde_json::json;

/// A fixed instant, so nothing in here depends on the clock.
pub const T0: u64 = 1_688_000_000_000_000_000;
pub const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";

/// gateway → payments (fails) → db, with logs on the failing span and a
/// metric whose exemplar points back at it.
pub async fn seeded() -> DynStorage {
    let storage: DynStorage = Arc::new(MemoryStorage::new(&MemoryConfig::default()));
    let span = |id: &str, parent: &str, svc: &str, name: &str, start, dur, err| SpanRecord {
        trace_id: TRACE.into(),
        span_id: id.into(),
        parent_span_id: parent.into(),
        name: name.into(),
        service_name: svc.into(),
        kind: "server".into(),
        start_time_unix_nano: start,
        end_time_unix_nano: start + dur,
        status_code: if err { 2 } else { 0 },
        status_message: if err {
            "upstream timeout".into()
        } else {
            String::new()
        },
        attributes: json!({"http": {"method": "POST", "status_code": if err { 500 } else { 200 }}}),
        resource_attributes: json!({"host": {"name": "node-1"}}),
        events: json!([]),
        links: json!([]),
        scope_name: String::new(),
        scope_version: String::new(),
    };
    storage
        .insert_spans(vec![
            span(
                "a1",
                "",
                "gateway",
                "POST /checkout",
                T0,
                800_000_000,
                false,
            ),
            span(
                "a2",
                "a1",
                "payments",
                "charge",
                T0 + 50_000_000,
                600_000_000,
                true,
            ),
            span(
                "a3",
                "a2",
                "db",
                "SELECT accounts",
                T0 + 100_000_000,
                120_000_000,
                false,
            ),
            // A second, healthy trace, so "errors only" has something to exclude.
            SpanRecord {
                trace_id: "bbbb".into(),
                span_id: "b1".into(),
                ..span("b1", "", "gateway", "GET /health", T0, 2_000_000, false)
            },
        ])
        .await
        .unwrap();

    let log = |svc: &str, trace: &str, sev: i32, body: &str| LogRecord {
        time_unix_nano: T0 + 300_000_000,
        observed_time_unix_nano: T0 + 300_000_000,
        severity_number: sev,
        severity_text: String::new(),
        body: json!(body),
        attributes: json!({"http": {"method": "POST"}}),
        resource_attributes: json!({"host": {"name": "node-1"}}),
        service_name: svc.into(),
        trace_id: trace.into(),
        span_id: String::new(),
        scope_name: String::new(),
    };
    storage
        .insert_logs(vec![
            log("payments", TRACE, 9, "charging card"),
            log(
                "payments",
                TRACE,
                17,
                "connection refused talking to acquirer",
            ),
            log("gateway", "", 13, "retrying checkout"),
        ])
        .await
        .unwrap();

    storage
        .insert_metrics(vec![MetricPoint {
            name: "http.server.requests".into(),
            description: "requests served".into(),
            unit: "1".into(),
            metric_type: MetricType::Sum,
            service_name: "gateway".into(),
            time_unix_nano: T0,
            value: 42.0,
            count: 0,
            attributes: json!({"route": "/checkout"}),
            resource_attributes: json!({}),
            extra: json!({"exemplars": [
                {"trace_id": TRACE, "span_id": "a2", "time_unix_nano": T0, "value": 42.0}
            ]}),
        }])
        .await
        .unwrap();
    storage
}

pub async fn app() -> Router {
    let mcp = Mcp::new(Arc::new(Direct::new(
        seeded().await,
        Arc::new(Config::default()),
    )));
    http::router(mcp, "/mcp", http::Auth::open())
}
