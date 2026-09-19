//! Embedded DuckDB storage (file-backed or in-memory).
//!
//! DuckDB is effectively single-writer, so one connection is shared behind a
//! mutex and every call hops onto the blocking pool. Filters run in SQL;
//! trace summarization reuses the shared Rust helper for identical semantics
//! across backends.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use async_trait::async_trait;
use duckdb::types::Value as DbValue;
use duckdb::{params_from_iter, Connection};
use otelview_config::MemoryConfig;
use otelview_model::{
    LogQuery, LogRecord, MetricInfo, MetricPoint, MetricQuery, MetricSeries, MetricType,
    SpanRecord, StorageStats, TraceQuery, TraceSummary,
};

use crate::memory::group_series;
use crate::summary::build_trace_summaries;
use crate::Storage;

pub struct DuckdbStorage {
    conn: Arc<Mutex<Connection>>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS spans (
    trace_id TEXT NOT NULL,
    span_id TEXT NOT NULL,
    parent_span_id TEXT NOT NULL DEFAULT '',
    name TEXT NOT NULL,
    service_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    start_time_unix_nano BIGINT NOT NULL,
    end_time_unix_nano BIGINT NOT NULL,
    duration_nanos BIGINT NOT NULL,
    status_code INTEGER NOT NULL DEFAULT 0,
    status_message TEXT NOT NULL DEFAULT '',
    attributes TEXT NOT NULL DEFAULT '{}',
    resource_attributes TEXT NOT NULL DEFAULT '{}',
    events TEXT NOT NULL DEFAULT '[]',
    links TEXT NOT NULL DEFAULT '[]',
    scope_name TEXT NOT NULL DEFAULT '',
    scope_version TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_spans_trace ON spans (trace_id);
CREATE INDEX IF NOT EXISTS idx_spans_start ON spans (start_time_unix_nano);

CREATE TABLE IF NOT EXISTS logs (
    time_unix_nano BIGINT NOT NULL,
    observed_time_unix_nano BIGINT NOT NULL,
    severity_number INTEGER NOT NULL DEFAULT 0,
    severity_text TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT 'null',
    attributes TEXT NOT NULL DEFAULT '{}',
    resource_attributes TEXT NOT NULL DEFAULT '{}',
    service_name TEXT NOT NULL,
    trace_id TEXT NOT NULL DEFAULT '',
    span_id TEXT NOT NULL DEFAULT '',
    scope_name TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_logs_time ON logs (time_unix_nano);

CREATE TABLE IF NOT EXISTS metric_points (
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    unit TEXT NOT NULL DEFAULT '',
    metric_type TEXT NOT NULL,
    service_name TEXT NOT NULL,
    time_unix_nano BIGINT NOT NULL,
    value DOUBLE NOT NULL DEFAULT 0,
    count BIGINT NOT NULL DEFAULT 0,
    attributes TEXT NOT NULL DEFAULT '{}',
    resource_attributes TEXT NOT NULL DEFAULT '{}',
    extra TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_metrics_name_time ON metric_points (name, time_unix_nano);
"#;

impl DuckdbStorage {
    pub fn open(path: &str) -> Result<Self> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory().context("opening in-memory DuckDB")?
        } else {
            Connection::open(path).with_context(|| format!("opening DuckDB at {path}"))?
        };
        conn.execute_batch(SCHEMA).context("creating DuckDB schema")?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    async fn with_conn<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            f(&conn)
        })
        .await
        .context("DuckDB task panicked")?
    }
}

fn row_to_span(row: &duckdb::Row<'_>) -> duckdb::Result<SpanRecord> {
    Ok(SpanRecord {
        trace_id: row.get(0)?,
        span_id: row.get(1)?,
        parent_span_id: row.get(2)?,
        name: row.get(3)?,
        service_name: row.get(4)?,
        kind: row.get(5)?,
        start_time_unix_nano: row.get::<_, i64>(6)? as u64,
        end_time_unix_nano: row.get::<_, i64>(7)? as u64,
        status_code: row.get(8)?,
        status_message: row.get(9)?,
        attributes: parse_json(row.get::<_, String>(10)?),
        resource_attributes: parse_json(row.get::<_, String>(11)?),
        events: parse_json(row.get::<_, String>(12)?),
        links: parse_json(row.get::<_, String>(13)?),
        scope_name: row.get(14)?,
        scope_version: row.get(15)?,
    })
}

const SPAN_COLS: &str = "trace_id, span_id, parent_span_id, name, service_name, kind, \
     start_time_unix_nano, end_time_unix_nano, status_code, status_message, attributes, \
     resource_attributes, events, links, scope_name, scope_version";

fn parse_json(s: String) -> serde_json::Value {
    serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)
}

#[async_trait]
impl Storage for DuckdbStorage {
    async fn insert_spans(&self, spans: Vec<SpanRecord>) -> Result<()> {
        self.with_conn(move |conn| {
            let mut stmt = conn.prepare_cached(
                "INSERT INTO spans VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )?;
            for s in spans {
                stmt.execute(params_from_iter(vec![
                    DbValue::Text(s.trace_id),
                    DbValue::Text(s.span_id),
                    DbValue::Text(s.parent_span_id),
                    DbValue::Text(s.name),
                    DbValue::Text(s.service_name),
                    DbValue::Text(s.kind),
                    DbValue::BigInt(s.start_time_unix_nano as i64),
                    DbValue::BigInt(s.end_time_unix_nano as i64),
                    DbValue::BigInt(s.end_time_unix_nano.saturating_sub(s.start_time_unix_nano)
                        as i64),
                    DbValue::Int(s.status_code),
                    DbValue::Text(s.status_message),
                    DbValue::Text(s.attributes.to_string()),
                    DbValue::Text(s.resource_attributes.to_string()),
                    DbValue::Text(s.events.to_string()),
                    DbValue::Text(s.links.to_string()),
                    DbValue::Text(s.scope_name),
                    DbValue::Text(s.scope_version),
                ]))?;
            }
            Ok(())
        })
        .await
    }

    async fn insert_logs(&self, logs: Vec<LogRecord>) -> Result<()> {
        self.with_conn(move |conn| {
            let mut stmt = conn.prepare_cached(
                "INSERT INTO logs VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )?;
            for l in logs {
                stmt.execute(params_from_iter(vec![
                    DbValue::BigInt(l.time_unix_nano as i64),
                    DbValue::BigInt(l.observed_time_unix_nano as i64),
                    DbValue::Int(l.severity_number),
                    DbValue::Text(l.severity_text),
                    DbValue::Text(l.body.to_string()),
                    DbValue::Text(l.attributes.to_string()),
                    DbValue::Text(l.resource_attributes.to_string()),
                    DbValue::Text(l.service_name),
                    DbValue::Text(l.trace_id),
                    DbValue::Text(l.span_id),
                    DbValue::Text(l.scope_name),
                ]))?;
            }
            Ok(())
        })
        .await
    }

    async fn insert_metrics(&self, points: Vec<MetricPoint>) -> Result<()> {
        self.with_conn(move |conn| {
            let mut stmt = conn.prepare_cached(
                "INSERT INTO metric_points VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )?;
            for p in points {
                stmt.execute(params_from_iter(vec![
                    DbValue::Text(p.name),
                    DbValue::Text(p.description),
                    DbValue::Text(p.unit),
                    DbValue::Text(p.metric_type.as_str().to_string()),
                    DbValue::Text(p.service_name),
                    DbValue::BigInt(p.time_unix_nano as i64),
                    DbValue::Double(p.value),
                    DbValue::BigInt(p.count as i64),
                    DbValue::Text(p.attributes.to_string()),
                    DbValue::Text(p.resource_attributes.to_string()),
                    DbValue::Text(p.extra.to_string()),
                ]))?;
            }
            Ok(())
        })
        .await
    }

    async fn list_services(&self) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT service_name FROM (
                    SELECT service_name FROM spans
                    UNION SELECT service_name FROM logs
                    UNION SELECT service_name FROM metric_points
                 ) ORDER BY service_name",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
        .await
    }

    async fn list_operations(&self, service: &str) -> Result<Vec<String>> {
        let service = service.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT name FROM spans WHERE (? = '' OR service_name = ?) ORDER BY name",
            )?;
            let rows =
                stmt.query_map([&service, &service], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<duckdb::Result<Vec<_>>>()?)
        })
        .await
    }

    async fn find_traces(&self, q: TraceQuery) -> Result<Vec<TraceSummary>> {
        self.with_conn(move |conn| {
            let limit = if q.limit == 0 { 20 } else { q.limit };
            let mut sql = String::from(
                "SELECT trace_id, max(start_time_unix_nano) AS latest FROM spans WHERE 1=1",
            );
            let mut params: Vec<DbValue> = Vec::new();
            if let Some(s) = q.service.as_deref().filter(|s| !s.is_empty()) {
                sql.push_str(" AND service_name = ?");
                params.push(DbValue::Text(s.to_string()));
            }
            if let Some(op) = q.operation.as_deref().filter(|s| !s.is_empty()) {
                sql.push_str(" AND name = ?");
                params.push(DbValue::Text(op.to_string()));
            }
            if q.errors_only {
                sql.push_str(" AND status_code = 2");
            }
            if let Some(min) = q.min_duration_nanos {
                sql.push_str(" AND duration_nanos >= ?");
                params.push(DbValue::BigInt(min as i64));
            }
            if let Some(max) = q.max_duration_nanos {
                sql.push_str(" AND duration_nanos <= ?");
                params.push(DbValue::BigInt(max as i64));
            }
            if let Some(min) = q.start_time_min_unix_nano {
                sql.push_str(" AND start_time_unix_nano >= ?");
                params.push(DbValue::BigInt(min as i64));
            }
            if let Some(max) = q.start_time_max_unix_nano {
                sql.push_str(" AND start_time_unix_nano <= ?");
                params.push(DbValue::BigInt(max as i64));
            }
            if let Some(attr_q) = q.attribute_query.as_deref().filter(|s| !s.is_empty()) {
                match attr_q.split_once('=') {
                    Some((k, v)) => {
                        sql.push_str(
                            " AND (json_extract_string(attributes, ?) = ? \
                              OR json_extract_string(resource_attributes, ?) = ?)",
                        );
                        let key = format!("$.\"{}\"", k.trim());
                        params.push(DbValue::Text(key.clone()));
                        params.push(DbValue::Text(v.trim().to_string()));
                        params.push(DbValue::Text(key));
                        params.push(DbValue::Text(v.trim().to_string()));
                    }
                    None => {
                        sql.push_str(
                            " AND (attributes LIKE '%' || ? || '%' \
                              OR resource_attributes LIKE '%' || ? || '%')",
                        );
                        params.push(DbValue::Text(attr_q.to_string()));
                        params.push(DbValue::Text(attr_q.to_string()));
                    }
                }
            }
            sql.push_str(" GROUP BY trace_id ORDER BY latest DESC LIMIT ?");
            params.push(DbValue::BigInt(limit as i64));

            let mut stmt = conn.prepare(&sql)?;
            let ids: Vec<String> = stmt
                .query_map(params_from_iter(params), |r| r.get::<_, String>(0))?
                .collect::<duckdb::Result<_>>()?;
            if ids.is_empty() {
                return Ok(Vec::new());
            }

            let placeholders = vec!["?"; ids.len()].join(", ");
            let mut stmt = conn.prepare(&format!(
                "SELECT {SPAN_COLS} FROM spans WHERE trace_id IN ({placeholders})"
            ))?;
            let spans: Vec<SpanRecord> = stmt
                .query_map(params_from_iter(ids.iter().map(|s| s.as_str())), row_to_span)?
                .collect::<duckdb::Result<_>>()?;
            Ok(build_trace_summaries(&spans))
        })
        .await
    }

    async fn get_trace(&self, trace_id: &str) -> Result<Vec<SpanRecord>> {
        let trace_id = trace_id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {SPAN_COLS} FROM spans WHERE trace_id = ? ORDER BY start_time_unix_nano"
            ))?;
            let spans: Vec<SpanRecord> = stmt
                .query_map([&trace_id], row_to_span)?
                .collect::<duckdb::Result<_>>()?;
            Ok(spans)
        })
        .await
    }

    async fn query_logs(&self, q: LogQuery) -> Result<Vec<LogRecord>> {
        self.with_conn(move |conn| {
            let limit = if q.limit == 0 { 200 } else { q.limit };
            let mut sql = String::from(
                "SELECT time_unix_nano, observed_time_unix_nano, severity_number, severity_text, \
                 body, attributes, resource_attributes, service_name, trace_id, span_id, \
                 scope_name FROM logs WHERE 1=1",
            );
            let mut params: Vec<DbValue> = Vec::new();
            if let Some(s) = q.service.as_deref().filter(|s| !s.is_empty()) {
                sql.push_str(" AND service_name = ?");
                params.push(DbValue::Text(s.to_string()));
            }
            if let Some(min) = q.min_severity {
                sql.push_str(" AND severity_number >= ?");
                params.push(DbValue::Int(min));
            }
            if let Some(t) = q.trace_id.as_deref().filter(|s| !s.is_empty()) {
                sql.push_str(" AND trace_id = ?");
                params.push(DbValue::Text(t.to_string()));
            }
            if let Some(min) = q.time_min_unix_nano {
                sql.push_str(" AND time_unix_nano >= ?");
                params.push(DbValue::BigInt(min as i64));
            }
            if let Some(max) = q.time_max_unix_nano {
                sql.push_str(" AND time_unix_nano <= ?");
                params.push(DbValue::BigInt(max as i64));
            }
            if let Some(s) = q.search.as_deref().filter(|s| !s.is_empty()) {
                sql.push_str(
                    " AND (lower(body) LIKE '%' || lower(?) || '%' \
                      OR lower(attributes) LIKE '%' || lower(?) || '%' \
                      OR lower(severity_text) LIKE '%' || lower(?) || '%')",
                );
                for _ in 0..3 {
                    params.push(DbValue::Text(s.to_string()));
                }
            }
            sql.push_str(" ORDER BY time_unix_nano DESC LIMIT ?");
            params.push(DbValue::BigInt(limit as i64));

            let mut stmt = conn.prepare(&sql)?;
            let logs: Vec<LogRecord> = stmt
                .query_map(params_from_iter(params), |row| {
                    Ok(LogRecord {
                        time_unix_nano: row.get::<_, i64>(0)? as u64,
                        observed_time_unix_nano: row.get::<_, i64>(1)? as u64,
                        severity_number: row.get(2)?,
                        severity_text: row.get(3)?,
                        body: parse_json(row.get::<_, String>(4)?),
                        attributes: parse_json(row.get::<_, String>(5)?),
                        resource_attributes: parse_json(row.get::<_, String>(6)?),
                        service_name: row.get(7)?,
                        trace_id: row.get(8)?,
                        span_id: row.get(9)?,
                        scope_name: row.get(10)?,
                    })
                })?
                .collect::<duckdb::Result<_>>()?;
            Ok(logs)
        })
        .await
    }

    async fn list_metrics(&self) -> Result<Vec<MetricInfo>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT name, any_value(description), any_value(unit), any_value(metric_type), \
                 list(DISTINCT service_name) FROM metric_points GROUP BY name ORDER BY name",
            )?;
            let infos: Vec<MetricInfo> = stmt
                .query_map([], |row| {
                    let services: duckdb::types::Value = row.get(4)?;
                    let services = match services {
                        duckdb::types::Value::List(items) => items
                            .into_iter()
                            .filter_map(|v| match v {
                                duckdb::types::Value::Text(s) => Some(s),
                                _ => None,
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    Ok(MetricInfo {
                        name: row.get(0)?,
                        description: row.get(1)?,
                        unit: row.get(2)?,
                        metric_type: MetricType::parse(&row.get::<_, String>(3)?)
                            .unwrap_or(MetricType::Gauge),
                        services,
                    })
                })?
                .collect::<duckdb::Result<_>>()?;
            Ok(infos)
        })
        .await
    }

    async fn query_metric_series(&self, q: MetricQuery) -> Result<Vec<MetricSeries>> {
        self.with_conn(move |conn| {
            let mut sql = String::from(
                "SELECT service_name, attributes, time_unix_nano, value FROM metric_points \
                 WHERE name = ?",
            );
            let mut params: Vec<DbValue> = vec![DbValue::Text(q.name.clone())];
            if let Some(s) = q.service.as_deref().filter(|s| !s.is_empty()) {
                sql.push_str(" AND service_name = ?");
                params.push(DbValue::Text(s.to_string()));
            }
            if let Some(min) = q.time_min_unix_nano {
                sql.push_str(" AND time_unix_nano >= ?");
                params.push(DbValue::BigInt(min as i64));
            }
            if let Some(max) = q.time_max_unix_nano {
                sql.push_str(" AND time_unix_nano <= ?");
                params.push(DbValue::BigInt(max as i64));
            }
            sql.push_str(" ORDER BY time_unix_nano");

            let mut stmt = conn.prepare(&sql)?;
            let raw: Vec<MetricPoint> = stmt
                .query_map(params_from_iter(params), |row| {
                    Ok(MetricPoint {
                        name: q.name.clone(),
                        description: String::new(),
                        unit: String::new(),
                        metric_type: MetricType::Gauge,
                        service_name: row.get(0)?,
                        attributes: parse_json(row.get::<_, String>(1)?),
                        time_unix_nano: row.get::<_, i64>(2)? as u64,
                        value: row.get(3)?,
                        count: 0,
                        resource_attributes: serde_json::Value::Null,
                        extra: serde_json::Value::Null,
                    })
                })?
                .collect::<duckdb::Result<_>>()?;
            Ok(group_series(raw.iter().collect(), q.max_points))
        })
        .await
    }

    async fn stats(&self) -> Result<StorageStats> {
        self.with_conn(|conn| {
            let count = |sql: &str| -> Result<u64> {
                Ok(conn.query_row(sql, [], |r| r.get::<_, i64>(0))? as u64)
            };
            Ok(StorageStats {
                spans: count("SELECT count(*) FROM spans")?,
                logs: count("SELECT count(*) FROM logs")?,
                metric_points: count("SELECT count(*) FROM metric_points")?,
                services: count(
                    "SELECT count(DISTINCT service_name) FROM (
                        SELECT service_name FROM spans
                        UNION SELECT service_name FROM logs
                        UNION SELECT service_name FROM metric_points)",
                )?,
                backend: "duckdb".into(),
            })
        })
        .await
    }
}

/// The memory limits config is unused here but kept for signature parity.
#[allow(dead_code)]
fn _unused(_c: &MemoryConfig) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn span(trace: &str, id: &str, svc: &str, start: u64, status: i32) -> SpanRecord {
        SpanRecord {
            trace_id: trace.into(),
            span_id: id.into(),
            parent_span_id: String::new(),
            name: format!("op-{id}"),
            service_name: svc.into(),
            kind: "server".into(),
            start_time_unix_nano: start,
            end_time_unix_nano: start + 500,
            status_code: status,
            status_message: String::new(),
            attributes: json!({"http.route": "/x"}),
            resource_attributes: json!({"service.name": svc}),
            events: json!([]),
            links: json!([]),
            scope_name: String::new(),
            scope_version: String::new(),
        }
    }

    #[tokio::test]
    async fn duckdb_roundtrip() {
        let store = DuckdbStorage::open(":memory:").unwrap();
        store
            .insert_spans(vec![
                span("t1", "a", "svc-a", 100, 0),
                span("t1", "b", "svc-b", 200, 2),
                span("t2", "c", "svc-a", 900, 0),
            ])
            .await
            .unwrap();

        assert_eq!(store.list_services().await.unwrap(), vec!["svc-a", "svc-b"]);
        assert_eq!(store.list_operations("svc-b").await.unwrap(), vec!["op-b"]);

        let all = store.find_traces(TraceQuery { limit: 10, ..Default::default() }).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].trace_id, "t2");
        assert_eq!(all[1].span_count, 2);
        assert_eq!(all[1].error_count, 1);

        let errors = store
            .find_traces(TraceQuery { errors_only: true, limit: 10, ..Default::default() })
            .await
            .unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].trace_id, "t1");

        let by_attr = store
            .find_traces(TraceQuery {
                attribute_query: Some("http.route=/x".into()),
                limit: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(by_attr.len(), 2);

        let trace = store.get_trace("t1").await.unwrap();
        assert_eq!(trace.len(), 2);
        assert_eq!(trace[0].span_id, "a");
        assert_eq!(trace[0].attributes["http.route"], "/x");

        let stats = store.stats().await.unwrap();
        assert_eq!(stats.spans, 3);
        assert_eq!(stats.backend, "duckdb");
    }

    #[tokio::test]
    async fn duckdb_logs_and_metrics() {
        let store = DuckdbStorage::open(":memory:").unwrap();
        store
            .insert_logs(vec![LogRecord {
                time_unix_nano: 5,
                observed_time_unix_nano: 5,
                severity_number: 17,
                severity_text: "ERROR".into(),
                body: json!("exploded"),
                attributes: json!({"k": "v"}),
                resource_attributes: json!({}),
                service_name: "svc".into(),
                trace_id: "t1".into(),
                span_id: "a".into(),
                scope_name: String::new(),
            }])
            .await
            .unwrap();
        let logs = store
            .query_logs(LogQuery { search: Some("EXPLO".into()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, json!("exploded"));

        store
            .insert_metrics(vec![MetricPoint {
                name: "cpu".into(),
                description: "cpu usage".into(),
                unit: "%".into(),
                metric_type: MetricType::Gauge,
                service_name: "svc".into(),
                time_unix_nano: 10,
                value: 0.5,
                count: 0,
                attributes: json!({"core": 0}),
                resource_attributes: json!({}),
                extra: json!({}),
            }])
            .await
            .unwrap();
        let infos = store.list_metrics().await.unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].services, vec!["svc"]);
        let series = store
            .query_metric_series(MetricQuery { name: "cpu".into(), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].points[0].value, 0.5);
    }
}
