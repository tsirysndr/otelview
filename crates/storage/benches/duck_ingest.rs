//! Rough ingest timing for the DuckDB backend.
//!
//! Not a criterion benchmark — a single timed run, printed. It exists to make
//! the cost of the write path observable when changing it:
//!
//!     cargo run --release -p otelview-storage --bench duck_ingest

use std::time::Instant;

use otelview_model::SpanRecord;
use otelview_storage::{duck::DuckdbStorage, Storage};
use serde_json::json;

#[tokio::main]
async fn main() {
    let rows: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(50_000);

    let store = DuckdbStorage::open(":memory:").expect("open");

    let spans: Vec<SpanRecord> = (0..rows)
        .map(|i| SpanRecord {
            trace_id: format!("t{}", i / 10),
            span_id: format!("s{i}"),
            parent_span_id: String::new(),
            name: "op".into(),
            service_name: "svc".into(),
            kind: "server".into(),
            start_time_unix_nano: 1_000 + i as u64,
            end_time_unix_nano: 1_500 + i as u64,
            status_code: 0,
            status_message: String::new(),
            attributes: json!({"http.route": "/x"}),
            resource_attributes: json!({"service.name": "svc"}),
            events: json!([]),
            links: json!([]),
            scope_name: String::new(),
            scope_version: String::new(),
        })
        .collect();

    let started = Instant::now();
    store.insert_spans(spans).await.expect("insert");
    let elapsed = started.elapsed();

    let per_row = elapsed.as_secs_f64() / rows as f64;
    println!(
        "inserted {rows} spans in {elapsed:?} ({:.1} rows/sec, {:.1}µs/row)",
        rows as f64 / elapsed.as_secs_f64(),
        per_row * 1e6,
    );

    // And a read straight after, to show the pool answering.
    let started = Instant::now();
    let services = store.list_services().await.expect("read");
    println!("list_services -> {services:?} in {:?}", started.elapsed());
}
