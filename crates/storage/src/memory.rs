//! Bounded in-memory storage: ring buffers per signal, linear-scan queries.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::RwLock;

use anyhow::Result;
use async_trait::async_trait;
use otelview_config::MemoryConfig;
use otelview_model::{
    LogQuery, LogRecord, MetricInfo, MetricPoint, MetricQuery, MetricSeries, SeriesPoint,
    SpanRecord, StorageStats, TraceQuery, TraceSummary,
};

use crate::summary::{build_trace_summaries, span_matches};
use crate::Storage;

pub struct MemoryStorage {
    inner: RwLock<Inner>,
    max_spans: usize,
    max_logs: usize,
    max_metric_points: usize,
}

#[derive(Default)]
struct Inner {
    spans: VecDeque<SpanRecord>,
    logs: VecDeque<LogRecord>,
    metrics: VecDeque<MetricPoint>,
}

impl MemoryStorage {
    pub fn new(cfg: &MemoryConfig) -> Self {
        Self {
            inner: RwLock::new(Inner::default()),
            max_spans: cfg.max_spans.max(1),
            max_logs: cfg.max_logs.max(1),
            max_metric_points: cfg.max_metric_points.max(1),
        }
    }
}

fn push_capped<T>(buf: &mut VecDeque<T>, items: Vec<T>, cap: usize) {
    for item in items {
        if buf.len() >= cap {
            buf.pop_front();
        }
        buf.push_back(item);
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn insert_spans(&self, spans: Vec<SpanRecord>) -> Result<()> {
        let mut inner = self.inner.write().unwrap();
        push_capped(&mut inner.spans, spans, self.max_spans);
        Ok(())
    }

    async fn insert_logs(&self, logs: Vec<LogRecord>) -> Result<()> {
        let mut inner = self.inner.write().unwrap();
        push_capped(&mut inner.logs, logs, self.max_logs);
        Ok(())
    }

    async fn insert_metrics(&self, points: Vec<MetricPoint>) -> Result<()> {
        let mut inner = self.inner.write().unwrap();
        push_capped(&mut inner.metrics, points, self.max_metric_points);
        Ok(())
    }

    async fn list_services(&self) -> Result<Vec<String>> {
        let inner = self.inner.read().unwrap();
        let mut set = BTreeSet::new();
        for s in &inner.spans {
            set.insert(s.service_name.clone());
        }
        for l in &inner.logs {
            set.insert(l.service_name.clone());
        }
        for m in &inner.metrics {
            set.insert(m.service_name.clone());
        }
        Ok(set.into_iter().collect())
    }

    async fn list_operations(&self, service: &str) -> Result<Vec<String>> {
        let inner = self.inner.read().unwrap();
        let mut set = BTreeSet::new();
        for s in &inner.spans {
            if service.is_empty() || s.service_name == service {
                set.insert(s.name.clone());
            }
        }
        Ok(set.into_iter().collect())
    }

    async fn find_traces(&self, q: TraceQuery) -> Result<Vec<TraceSummary>> {
        let inner = self.inner.read().unwrap();
        let limit = if q.limit == 0 { 20 } else { q.limit };
        // Newest-first scan for trace ids with at least one matching span.
        let mut matched: Vec<&str> = Vec::new();
        for s in inner.spans.iter().rev() {
            if span_matches(s, &q) && !matched.contains(&s.trace_id.as_str()) {
                matched.push(s.trace_id.as_str());
                if matched.len() >= limit {
                    break;
                }
            }
        }
        let ids: BTreeSet<&str> = matched.into_iter().collect();
        let spans: Vec<SpanRecord> = inner
            .spans
            .iter()
            .filter(|s| ids.contains(s.trace_id.as_str()))
            .cloned()
            .collect();
        Ok(build_trace_summaries(&spans))
    }

    async fn get_trace(&self, trace_id: &str) -> Result<Vec<SpanRecord>> {
        let inner = self.inner.read().unwrap();
        let mut spans: Vec<SpanRecord> =
            inner.spans.iter().filter(|s| s.trace_id == trace_id).cloned().collect();
        spans.sort_by_key(|s| s.start_time_unix_nano);
        Ok(spans)
    }

    async fn query_logs(&self, q: LogQuery) -> Result<Vec<LogRecord>> {
        let inner = self.inner.read().unwrap();
        let limit = if q.limit == 0 { 200 } else { q.limit };
        let mut out = Vec::new();
        for l in inner.logs.iter().rev() {
            if log_matches(l, &q) {
                out.push(l.clone());
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    async fn list_metrics(&self) -> Result<Vec<MetricInfo>> {
        let inner = self.inner.read().unwrap();
        let mut by_name: BTreeMap<&str, MetricInfo> = BTreeMap::new();
        for m in &inner.metrics {
            let entry = by_name.entry(m.name.as_str()).or_insert_with(|| MetricInfo {
                name: m.name.clone(),
                description: m.description.clone(),
                unit: m.unit.clone(),
                metric_type: m.metric_type,
                services: Vec::new(),
            });
            if !entry.services.contains(&m.service_name) {
                entry.services.push(m.service_name.clone());
            }
        }
        Ok(by_name.into_values().collect())
    }

    async fn query_metric_series(&self, q: MetricQuery) -> Result<Vec<MetricSeries>> {
        let inner = self.inner.read().unwrap();
        let points: Vec<&MetricPoint> = inner
            .metrics
            .iter()
            .filter(|m| m.name == q.name)
            .filter(|m| q.service.as_deref().map(|s| s.is_empty() || m.service_name == s).unwrap_or(true))
            .filter(|m| q.time_min_unix_nano.map(|t| m.time_unix_nano >= t).unwrap_or(true))
            .filter(|m| q.time_max_unix_nano.map(|t| m.time_unix_nano <= t).unwrap_or(true))
            .collect();
        Ok(group_series(points, q.max_points))
    }

    async fn stats(&self) -> Result<StorageStats> {
        let inner = self.inner.read().unwrap();
        let mut services = BTreeSet::new();
        for s in &inner.spans {
            services.insert(s.service_name.as_str());
        }
        for l in &inner.logs {
            services.insert(l.service_name.as_str());
        }
        for m in &inner.metrics {
            services.insert(m.service_name.as_str());
        }
        Ok(StorageStats {
            spans: inner.spans.len() as u64,
            logs: inner.logs.len() as u64,
            metric_points: inner.metrics.len() as u64,
            services: services.len() as u64,
            backend: "memory".into(),
        })
    }
}

pub(crate) fn log_matches(l: &LogRecord, q: &LogQuery) -> bool {
    if let Some(service) = &q.service {
        if !service.is_empty() && &l.service_name != service {
            return false;
        }
    }
    if let Some(min) = q.min_severity {
        if l.severity_number < min {
            return false;
        }
    }
    if let Some(trace_id) = &q.trace_id {
        if !trace_id.is_empty() && &l.trace_id != trace_id {
            return false;
        }
    }
    if let Some(min) = q.time_min_unix_nano {
        if l.time_unix_nano < min {
            return false;
        }
    }
    if let Some(max) = q.time_max_unix_nano {
        if l.time_unix_nano > max {
            return false;
        }
    }
    if let Some(search) = &q.search {
        if !search.is_empty() {
            let hay = format!("{} {} {}", l.body, l.attributes, l.severity_text);
            if !hay.to_lowercase().contains(&search.to_lowercase()) {
                return false;
            }
        }
    }
    true
}

/// Group points into one series per (service, attribute-set), sorted by time,
/// downsampled by striding when a series exceeds `max_points`.
pub(crate) fn group_series(points: Vec<&MetricPoint>, max_points: usize) -> Vec<MetricSeries> {
    let mut by_key: BTreeMap<String, MetricSeries> = BTreeMap::new();
    for p in points {
        let key = format!("{}|{}", p.service_name, p.attributes);
        let series = by_key.entry(key).or_insert_with(|| MetricSeries {
            service_name: p.service_name.clone(),
            attributes: p.attributes.clone(),
            points: Vec::new(),
        });
        series.points.push(SeriesPoint { time_unix_nano: p.time_unix_nano, value: p.value });
    }
    let max_points = if max_points == 0 { 500 } else { max_points };
    let mut out: Vec<MetricSeries> = by_key.into_values().collect();
    for series in &mut out {
        series.points.sort_by_key(|p| p.time_unix_nano);
        if series.points.len() > max_points {
            let stride = series.points.len().div_ceil(max_points);
            series.points = series
                .points
                .iter()
                .step_by(stride)
                .cloned()
                .collect();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelview_model::MetricType;
    use serde_json::json;

    fn span(trace: &str, id: &str, svc: &str, start: u64) -> SpanRecord {
        SpanRecord {
            trace_id: trace.into(),
            span_id: id.into(),
            parent_span_id: String::new(),
            name: "op".into(),
            service_name: svc.into(),
            kind: "server".into(),
            start_time_unix_nano: start,
            end_time_unix_nano: start + 100,
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

    #[tokio::test]
    async fn ring_buffer_caps_spans() {
        let store = MemoryStorage::new(&MemoryConfig { max_spans: 3, max_logs: 3, max_metric_points: 3 });
        let spans = (0..5).map(|i| span(&format!("t{i}"), "s", "svc", i)).collect();
        store.insert_spans(spans).await.unwrap();
        assert_eq!(store.stats().await.unwrap().spans, 3);
        // Oldest evicted: t0/t1 gone.
        assert!(store.get_trace("t0").await.unwrap().is_empty());
        assert_eq!(store.get_trace("t4").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn find_traces_respects_service_filter_and_limit() {
        let store = MemoryStorage::new(&MemoryConfig::default());
        let mut spans = Vec::new();
        for i in 0..10u64 {
            let svc = if i % 2 == 0 { "even" } else { "odd" };
            spans.push(span(&format!("t{i}"), "s", svc, i * 10));
        }
        store.insert_spans(spans).await.unwrap();
        let q = TraceQuery { service: Some("even".into()), limit: 3, ..Default::default() };
        let res = store.find_traces(q).await.unwrap();
        assert_eq!(res.len(), 3);
        assert!(res.iter().all(|t| t.root_service == "even"));
        // Newest first.
        assert_eq!(res[0].trace_id, "t8");
    }

    #[tokio::test]
    async fn logs_filter_by_severity_and_search() {
        let store = MemoryStorage::new(&MemoryConfig::default());
        let logs = vec![
            LogRecord {
                time_unix_nano: 1,
                observed_time_unix_nano: 1,
                severity_number: 9,
                severity_text: "INFO".into(),
                body: json!("hello world"),
                attributes: json!({}),
                resource_attributes: json!({}),
                service_name: "svc".into(),
                trace_id: String::new(),
                span_id: String::new(),
                scope_name: String::new(),
            },
            LogRecord {
                time_unix_nano: 2,
                observed_time_unix_nano: 2,
                severity_number: 17,
                severity_text: "ERROR".into(),
                body: json!("kaboom"),
                attributes: json!({}),
                resource_attributes: json!({}),
                service_name: "svc".into(),
                trace_id: String::new(),
                span_id: String::new(),
                scope_name: String::new(),
            },
        ];
        store.insert_logs(logs).await.unwrap();
        let q = LogQuery { min_severity: Some(13), ..Default::default() };
        let res = store.query_logs(q).await.unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].severity_text, "ERROR");
        let q = LogQuery { search: Some("HELLO".into()), ..Default::default() };
        assert_eq!(store.query_logs(q).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn metric_series_grouped_by_attributes() {
        let store = MemoryStorage::new(&MemoryConfig::default());
        let mut points = Vec::new();
        for i in 0..4u64 {
            for route in ["/a", "/b"] {
                points.push(MetricPoint {
                    name: "http.requests".into(),
                    description: String::new(),
                    unit: "1".into(),
                    metric_type: MetricType::Sum,
                    service_name: "svc".into(),
                    time_unix_nano: i * 1000,
                    value: i as f64,
                    count: 0,
                    attributes: json!({"route": route}),
                    resource_attributes: json!({}),
                    extra: json!({}),
                });
            }
        }
        store.insert_metrics(points).await.unwrap();
        let q = MetricQuery { name: "http.requests".into(), ..Default::default() };
        let series = store.query_metric_series(q).await.unwrap();
        assert_eq!(series.len(), 2);
        assert!(series.iter().all(|s| s.points.len() == 4));
        let infos = store.list_metrics().await.unwrap();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].name, "http.requests");
    }
}
