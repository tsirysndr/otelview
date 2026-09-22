//! A small instance with one of everything, for the tests.

use std::sync::Arc;

use otelview_config::{Config, MemoryConfig};
use otelview_model::{LogRecord, MetricPoint, MetricType, SpanRecord};
use otelview_storage::memory::MemoryStorage;
use otelview_storage::DynStorage;
use serde_json::json;

use crate::backend::direct::Direct;
use crate::backend::Otel;

/// Nanos in the middle of 2023 — a fixed instant, so rendered timestamps
/// are the same on every run.
pub const T0: u64 = 1_688_000_000_000_000_000;

pub fn span(
    trace: &str,
    id: &str,
    parent: &str,
    service: &str,
    dur: u64,
    error: bool,
) -> SpanRecord {
    SpanRecord {
        trace_id: trace.into(),
        span_id: id.into(),
        parent_span_id: parent.into(),
        name: format!("GET /{service}"),
        service_name: service.into(),
        kind: "server".into(),
        start_time_unix_nano: T0,
        end_time_unix_nano: T0 + dur,
        status_code: if error { 2 } else { 0 },
        status_message: if error { "boom".into() } else { String::new() },
        attributes: json!({"http": {"method": "GET", "status_code": if error { 500 } else { 200 }}}),
        resource_attributes: json!({"host": {"name": "node-1"}}),
        events: json!([]),
        links: json!([]),
        scope_name: String::new(),
        scope_version: String::new(),
    }
}

pub fn log(service: &str, trace: &str, severity: i32, body: &str) -> LogRecord {
    LogRecord {
        time_unix_nano: T0,
        observed_time_unix_nano: T0,
        severity_number: severity,
        severity_text: String::new(),
        body: json!(body),
        attributes: json!({"http": {"method": "GET"}}),
        resource_attributes: json!({"host": {"name": "node-1"}}),
        service_name: service.into(),
        trace_id: trace.into(),
        span_id: String::new(),
        scope_name: String::new(),
    }
}

pub fn metric(name: &str, service: &str, value: f64) -> MetricPoint {
    MetricPoint {
        name: name.into(),
        description: "requests served".into(),
        unit: "1".into(),
        metric_type: MetricType::Sum,
        service_name: service.into(),
        time_unix_nano: T0,
        value,
        count: 0,
        attributes: json!({"route": "/checkout"}),
        resource_attributes: json!({}),
        extra: json!({"exemplars": [{"trace_id": "aaaa", "span_id": "a1", "time_unix_nano": T0, "value": value}]}),
    }
}

/// Storage holding two traces (one failing), a few logs and one metric.
pub async fn sample_storage() -> DynStorage {
    let storage: DynStorage = Arc::new(MemoryStorage::new(&MemoryConfig::default()));
    storage
        .insert_spans(vec![
            span("aaaa", "a1", "", "svc-a", 10_000_000, false),
            span("aaaa", "a2", "a1", "svc-b", 4_000_000, true),
            span("bbbb", "b1", "", "svc-a", 2_000_000, false),
        ])
        .await
        .unwrap();
    storage
        .insert_logs(vec![
            log("svc-a", "aaaa", 9, "handling checkout"),
            log("svc-b", "aaaa", 17, "connection refused"),
            log("svc-a", "", 13, "retrying"),
        ])
        .await
        .unwrap();
    storage
        .insert_metrics(vec![metric("http.server.requests", "svc-a", 42.0)])
        .await
        .unwrap();
    storage
}

pub async fn sample_otel() -> Arc<dyn Otel> {
    Arc::new(Direct::new(
        sample_storage().await,
        Arc::new(Config::default()),
    ))
}
