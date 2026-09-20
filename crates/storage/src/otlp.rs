//! Conversions between OTLP protobuf payloads and the internal model.

use opentelemetry_proto::tonic::common::v1::{
    any_value, AnyValue, ArrayValue, KeyValue, KeyValueList,
};
use opentelemetry_proto::tonic::logs::v1::ResourceLogs;
use opentelemetry_proto::tonic::metrics::v1::{metric, number_data_point, ResourceMetrics};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{
    span, status, ResourceSpans, ScopeSpans, Span, Status, TracesData,
};
use otelview_model::{LogRecord, MetricPoint, MetricType, SpanRecord};
use serde_json::{json, Map, Value};

pub const UNKNOWN_SERVICE: &str = "unknown_service";

pub fn any_value_to_json(v: &AnyValue) -> Value {
    match &v.value {
        Some(any_value::Value::StringValue(s)) => Value::String(s.clone()),
        Some(any_value::Value::BoolValue(b)) => Value::Bool(*b),
        Some(any_value::Value::IntValue(i)) => json!(i),
        Some(any_value::Value::DoubleValue(d)) => json!(d),
        Some(any_value::Value::ArrayValue(arr)) => {
            Value::Array(arr.values.iter().map(any_value_to_json).collect())
        }
        Some(any_value::Value::KvlistValue(kvs)) => kvs_to_json(&kvs.values),
        Some(any_value::Value::BytesValue(b)) => Value::String(hex::encode(b)),
        _ => Value::Null,
    }
}

pub fn kvs_to_json(kvs: &[KeyValue]) -> Value {
    let mut map = Map::new();
    for kv in kvs {
        let val = kv
            .value
            .as_ref()
            .map(any_value_to_json)
            .unwrap_or(Value::Null);
        map.insert(kv.key.clone(), val);
    }
    Value::Object(map)
}

pub fn json_to_any_value(v: &Value) -> AnyValue {
    let value = match v {
        Value::Null => None,
        Value::Bool(b) => Some(any_value::Value::BoolValue(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(any_value::Value::IntValue(i))
            } else {
                Some(any_value::Value::DoubleValue(n.as_f64().unwrap_or(0.0)))
            }
        }
        Value::String(s) => Some(any_value::Value::StringValue(s.clone())),
        Value::Array(items) => Some(any_value::Value::ArrayValue(ArrayValue {
            values: items.iter().map(json_to_any_value).collect(),
        })),
        Value::Object(_) => Some(any_value::Value::KvlistValue(KeyValueList {
            values: json_to_kvs(v),
        })),
    };
    #[allow(clippy::needless_update)]
    AnyValue {
        value,
        ..Default::default()
    }
}

pub fn json_to_kvs(v: &Value) -> Vec<KeyValue> {
    match v {
        Value::Object(map) => map
            .iter()
            .map(|(k, val)| KeyValue {
                key: k.clone(),
                value: Some(json_to_any_value(val)),
                ..Default::default()
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub fn resource_service_name(resource: Option<&Resource>) -> String {
    resource
        .map(|r| &r.attributes)
        .and_then(|attrs| attrs.iter().find(|kv| kv.key == "service.name"))
        .and_then(|kv| kv.value.as_ref())
        .and_then(|v| match &v.value {
            Some(any_value::Value::StringValue(s)) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| UNKNOWN_SERVICE.to_string())
}

fn span_kind_str(kind: i32) -> &'static str {
    match span::SpanKind::try_from(kind) {
        Ok(span::SpanKind::Internal) => "internal",
        Ok(span::SpanKind::Server) => "server",
        Ok(span::SpanKind::Client) => "client",
        Ok(span::SpanKind::Producer) => "producer",
        Ok(span::SpanKind::Consumer) => "consumer",
        _ => "unspecified",
    }
}

fn span_kind_from_str(kind: &str) -> span::SpanKind {
    match kind {
        "internal" => span::SpanKind::Internal,
        "server" => span::SpanKind::Server,
        "client" => span::SpanKind::Client,
        "producer" => span::SpanKind::Producer,
        "consumer" => span::SpanKind::Consumer,
        _ => span::SpanKind::Unspecified,
    }
}

pub fn spans_from_resource_spans(resource_spans: &[ResourceSpans]) -> Vec<SpanRecord> {
    let mut out = Vec::new();
    for rs in resource_spans {
        let service = resource_service_name(rs.resource.as_ref());
        let resource_attrs = rs
            .resource
            .as_ref()
            .map(|r| kvs_to_json(&r.attributes))
            .unwrap_or_else(|| json!({}));
        for ss in &rs.scope_spans {
            let (scope_name, scope_version) = ss
                .scope
                .as_ref()
                .map(|s| (s.name.clone(), s.version.clone()))
                .unwrap_or_default();
            for s in &ss.spans {
                let events: Vec<Value> = s
                    .events
                    .iter()
                    .map(|e| {
                        json!({
                            "name": e.name,
                            "time_unix_nano": e.time_unix_nano,
                            "attributes": kvs_to_json(&e.attributes),
                        })
                    })
                    .collect();
                let links: Vec<Value> = s
                    .links
                    .iter()
                    .map(|l| {
                        json!({
                            "trace_id": hex::encode(&l.trace_id),
                            "span_id": hex::encode(&l.span_id),
                            "attributes": kvs_to_json(&l.attributes),
                        })
                    })
                    .collect();
                let (status_code, status_message) = s
                    .status
                    .as_ref()
                    .map(|st| (st.code, st.message.clone()))
                    .unwrap_or((0, String::new()));
                out.push(SpanRecord {
                    trace_id: hex::encode(&s.trace_id),
                    span_id: hex::encode(&s.span_id),
                    parent_span_id: hex::encode(&s.parent_span_id),
                    name: s.name.clone(),
                    service_name: service.clone(),
                    kind: span_kind_str(s.kind).to_string(),
                    start_time_unix_nano: s.start_time_unix_nano,
                    end_time_unix_nano: s.end_time_unix_nano,
                    status_code,
                    status_message,
                    attributes: kvs_to_json(&s.attributes),
                    resource_attributes: resource_attrs.clone(),
                    events: Value::Array(events),
                    links: Value::Array(links),
                    scope_name: scope_name.clone(),
                    scope_version: scope_version.clone(),
                });
            }
        }
    }
    out
}

/// Rebuild OTLP `TracesData` from model spans (used to export to Jaeger
/// remote storage). Spans are grouped by their resource attributes.
pub fn spans_to_traces_data(spans: &[SpanRecord]) -> TracesData {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<String, (Value, Vec<&SpanRecord>)> = BTreeMap::new();
    for s in spans {
        let key = s.resource_attributes.to_string();
        groups
            .entry(key)
            .or_insert_with(|| (s.resource_attributes.clone(), Vec::new()))
            .1
            .push(s);
    }
    let mut resource_spans = Vec::new();
    for (_, (resource_attrs, group)) in groups {
        let pb_spans: Vec<Span> = group
            .iter()
            .map(|s| {
                let events = match &s.events {
                    Value::Array(evs) => evs
                        .iter()
                        .map(|e| span::Event {
                            time_unix_nano: e
                                .get("time_unix_nano")
                                .and_then(Value::as_u64)
                                .unwrap_or_default(),
                            name: e
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            attributes: e.get("attributes").map(json_to_kvs).unwrap_or_default(),
                            ..Default::default()
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                let links = match &s.links {
                    Value::Array(ls) => ls
                        .iter()
                        .map(|l| span::Link {
                            trace_id: l
                                .get("trace_id")
                                .and_then(Value::as_str)
                                .and_then(|h| hex::decode(h).ok())
                                .unwrap_or_default(),
                            span_id: l
                                .get("span_id")
                                .and_then(Value::as_str)
                                .and_then(|h| hex::decode(h).ok())
                                .unwrap_or_default(),
                            attributes: l.get("attributes").map(json_to_kvs).unwrap_or_default(),
                            ..Default::default()
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                Span {
                    trace_id: hex::decode(&s.trace_id).unwrap_or_default(),
                    span_id: hex::decode(&s.span_id).unwrap_or_default(),
                    parent_span_id: hex::decode(&s.parent_span_id).unwrap_or_default(),
                    name: s.name.clone(),
                    kind: span_kind_from_str(&s.kind) as i32,
                    start_time_unix_nano: s.start_time_unix_nano,
                    end_time_unix_nano: s.end_time_unix_nano,
                    attributes: json_to_kvs(&s.attributes),
                    events,
                    links,
                    status: Some(Status {
                        message: s.status_message.clone(),
                        code: status::StatusCode::try_from(s.status_code)
                            .unwrap_or(status::StatusCode::Unset)
                            as i32,
                    }),
                    ..Default::default()
                }
            })
            .collect();
        resource_spans.push(ResourceSpans {
            resource: Some(Resource {
                attributes: json_to_kvs(&resource_attrs),
                ..Default::default()
            }),
            scope_spans: vec![ScopeSpans {
                scope: None,
                spans: pb_spans,
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        });
    }
    TracesData { resource_spans }
}

pub fn logs_from_resource_logs(resource_logs: &[ResourceLogs]) -> Vec<LogRecord> {
    let mut out = Vec::new();
    for rl in resource_logs {
        let service = resource_service_name(rl.resource.as_ref());
        let resource_attrs = rl
            .resource
            .as_ref()
            .map(|r| kvs_to_json(&r.attributes))
            .unwrap_or_else(|| json!({}));
        for sl in &rl.scope_logs {
            let scope_name = sl
                .scope
                .as_ref()
                .map(|s| s.name.clone())
                .unwrap_or_default();
            for lr in &sl.log_records {
                let time = if lr.time_unix_nano != 0 {
                    lr.time_unix_nano
                } else {
                    lr.observed_time_unix_nano
                };
                out.push(LogRecord {
                    time_unix_nano: time,
                    observed_time_unix_nano: lr.observed_time_unix_nano,
                    severity_number: lr.severity_number,
                    severity_text: lr.severity_text.clone(),
                    body: lr
                        .body
                        .as_ref()
                        .map(any_value_to_json)
                        .unwrap_or(Value::Null),
                    attributes: kvs_to_json(&lr.attributes),
                    resource_attributes: resource_attrs.clone(),
                    service_name: service.clone(),
                    trace_id: hex::encode(&lr.trace_id),
                    span_id: hex::encode(&lr.span_id),
                    scope_name: scope_name.clone(),
                });
            }
        }
    }
    out
}

pub fn metrics_from_resource_metrics(resource_metrics: &[ResourceMetrics]) -> Vec<MetricPoint> {
    let mut out = Vec::new();
    for rm in resource_metrics {
        let service = resource_service_name(rm.resource.as_ref());
        let resource_attrs = rm
            .resource
            .as_ref()
            .map(|r| kvs_to_json(&r.attributes))
            .unwrap_or_else(|| json!({}));
        for sm in &rm.scope_metrics {
            for m in &sm.metrics {
                convert_metric(m, &service, &resource_attrs, &mut out);
            }
        }
    }
    out
}

/// Rebuild OTLP `LogsData` from model log records, grouped by resource.
pub fn logs_to_logs_data(logs: &[LogRecord]) -> opentelemetry_proto::tonic::logs::v1::LogsData {
    use opentelemetry_proto::tonic::logs::v1::{LogsData, ResourceLogs, ScopeLogs};
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<String, (Value, Vec<&LogRecord>)> = BTreeMap::new();
    for l in logs {
        groups
            .entry(l.resource_attributes.to_string())
            .or_insert_with(|| (l.resource_attributes.clone(), Vec::new()))
            .1
            .push(l);
    }
    let mut resource_logs = Vec::new();
    for (_, (resource_attrs, group)) in groups {
        let records = group
            .iter()
            .map(|l| opentelemetry_proto::tonic::logs::v1::LogRecord {
                time_unix_nano: l.time_unix_nano,
                observed_time_unix_nano: l.observed_time_unix_nano,
                severity_number: l.severity_number,
                severity_text: l.severity_text.clone(),
                body: Some(json_to_any_value(&l.body)),
                attributes: json_to_kvs(&l.attributes),
                trace_id: hex::decode(&l.trace_id).unwrap_or_default(),
                span_id: hex::decode(&l.span_id).unwrap_or_default(),
                ..Default::default()
            })
            .collect();
        resource_logs.push(ResourceLogs {
            resource: Some(Resource {
                attributes: json_to_kvs(&resource_attrs),
                ..Default::default()
            }),
            scope_logs: vec![ScopeLogs {
                scope: None,
                log_records: records,
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    LogsData { resource_logs }
}

/// Rebuild OTLP `MetricsData` from model metric points, grouped by resource
/// then metric name. Histogram buckets, sum flags and summary quantiles are
/// restored from the point's `extra` payload.
pub fn metric_points_to_metrics_data(
    points: &[MetricPoint],
) -> opentelemetry_proto::tonic::metrics::v1::MetricsData {
    use opentelemetry_proto::tonic::metrics::v1::{
        Gauge, Histogram, HistogramDataPoint, Metric, MetricsData, NumberDataPoint,
        ResourceMetrics, ScopeMetrics, Sum, Summary, SummaryDataPoint,
    };
    use std::collections::BTreeMap;

    // resource key → metric name → points
    let mut groups: BTreeMap<String, (Value, BTreeMap<String, Vec<&MetricPoint>>)> =
        BTreeMap::new();
    for p in points {
        let entry = groups
            .entry(p.resource_attributes.to_string())
            .or_insert_with(|| (p.resource_attributes.clone(), BTreeMap::new()));
        entry.1.entry(p.name.clone()).or_default().push(p);
    }

    let number_point = |p: &MetricPoint| NumberDataPoint {
        attributes: json_to_kvs(&p.attributes),
        time_unix_nano: p.time_unix_nano,
        value: Some(number_data_point::Value::AsDouble(p.value)),
        ..Default::default()
    };

    let mut resource_metrics = Vec::new();
    for (_, (resource_attrs, by_name)) in groups {
        let mut metrics = Vec::new();
        for (name, pts) in by_name {
            let first = pts[0];
            let data = match first.metric_type {
                MetricType::Gauge | MetricType::ExponentialHistogram => {
                    // Exponential histograms are re-emitted as gauges over the
                    // sum: bucket detail is not reconstructed.
                    Some(metric::Data::Gauge(Gauge {
                        data_points: pts.iter().map(|p| number_point(p)).collect(),
                    }))
                }
                MetricType::Sum => Some(metric::Data::Sum(Sum {
                    data_points: pts.iter().map(|p| number_point(p)).collect(),
                    aggregation_temporality: first
                        .extra
                        .get("temporality")
                        .and_then(Value::as_i64)
                        .unwrap_or(2) as i32,
                    is_monotonic: first
                        .extra
                        .get("is_monotonic")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })),
                MetricType::Histogram => Some(metric::Data::Histogram(Histogram {
                    data_points: pts
                        .iter()
                        .map(|p| HistogramDataPoint {
                            attributes: json_to_kvs(&p.attributes),
                            time_unix_nano: p.time_unix_nano,
                            count: p.count,
                            sum: Some(p.value),
                            bucket_counts: p
                                .extra
                                .get("bucket_counts")
                                .and_then(Value::as_array)
                                .map(|a| {
                                    a.iter().filter_map(Value::as_u64).collect()
                                })
                                .unwrap_or_default(),
                            explicit_bounds: p
                                .extra
                                .get("explicit_bounds")
                                .and_then(Value::as_array)
                                .map(|a| {
                                    a.iter().filter_map(Value::as_f64).collect()
                                })
                                .unwrap_or_default(),
                            min: p.extra.get("min").and_then(Value::as_f64),
                            max: p.extra.get("max").and_then(Value::as_f64),
                            ..Default::default()
                        })
                        .collect(),
                    aggregation_temporality: first
                        .extra
                        .get("temporality")
                        .and_then(Value::as_i64)
                        .unwrap_or(2) as i32,
                })),
                MetricType::Summary => Some(metric::Data::Summary(Summary {
                    data_points: pts
                        .iter()
                        .map(|p| SummaryDataPoint {
                            attributes: json_to_kvs(&p.attributes),
                            time_unix_nano: p.time_unix_nano,
                            count: p.count,
                            sum: p.value,
                            quantile_values: p
                                .extra
                                .get("quantiles")
                                .and_then(Value::as_array)
                                .map(|a| {
                                    a.iter()
                                        .map(|q| {
                                            opentelemetry_proto::tonic::metrics::v1::summary_data_point::ValueAtQuantile {
                                                quantile: q.get("quantile").and_then(Value::as_f64).unwrap_or(0.0),
                                                value: q.get("value").and_then(Value::as_f64).unwrap_or(0.0),
                                            }
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                            ..Default::default()
                        })
                        .collect(),
                })),
            };
            metrics.push(Metric {
                name,
                description: first.description.clone(),
                unit: first.unit.clone(),
                data,
                ..Default::default()
            });
        }
        resource_metrics.push(ResourceMetrics {
            resource: Some(Resource {
                attributes: json_to_kvs(&resource_attrs),
                ..Default::default()
            }),
            scope_metrics: vec![ScopeMetrics {
                scope: None,
                metrics,
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    MetricsData { resource_metrics }
}

fn number_value(v: Option<&number_data_point::Value>) -> f64 {
    match v {
        Some(number_data_point::Value::AsDouble(d)) => *d,
        Some(number_data_point::Value::AsInt(i)) => *i as f64,
        None => 0.0,
    }
}

/// OTLP exemplars for one data point, as the JSON we persist in `extra`.
/// Returns `None` when the producer sent none, so `extra` stays unchanged
/// for the overwhelmingly common case.
fn exemplars_json(
    exemplars: &[opentelemetry_proto::tonic::metrics::v1::Exemplar],
) -> Option<Value> {
    use opentelemetry_proto::tonic::metrics::v1::exemplar;
    let out: Vec<Value> = exemplars
        .iter()
        // An exemplar with no trace id cannot link anything, so it is not
        // worth storing.
        .filter(|e| !e.trace_id.is_empty())
        .map(|e| {
            json!({
                "trace_id": hex::encode(&e.trace_id),
                "span_id": hex::encode(&e.span_id),
                "time_unix_nano": e.time_unix_nano,
                "value": match e.value {
                    Some(exemplar::Value::AsDouble(d)) => d,
                    Some(exemplar::Value::AsInt(i)) => i as f64,
                    None => 0.0,
                },
            })
        })
        .collect();
    (!out.is_empty()).then(|| Value::Array(out))
}

/// Attach exemplars to a point's `extra`, preserving whatever the metric
/// type already put there.
fn attach_exemplars(
    p: &mut MetricPoint,
    exemplars: &[opentelemetry_proto::tonic::metrics::v1::Exemplar],
) {
    if let Some(ex) = exemplars_json(exemplars) {
        match p.extra.as_object_mut() {
            Some(obj) => {
                obj.insert("exemplars".into(), ex);
            }
            None => p.extra = json!({ "exemplars": ex }),
        }
    }
}

fn convert_metric(
    m: &opentelemetry_proto::tonic::metrics::v1::Metric,
    service: &str,
    resource_attrs: &Value,
    out: &mut Vec<MetricPoint>,
) {
    let base = |metric_type: MetricType, time: u64, attrs: &[KeyValue]| MetricPoint {
        name: m.name.clone(),
        description: m.description.clone(),
        unit: m.unit.clone(),
        metric_type,
        service_name: service.to_string(),
        time_unix_nano: time,
        value: 0.0,
        count: 0,
        attributes: kvs_to_json(attrs),
        resource_attributes: resource_attrs.clone(),
        extra: json!({}),
    };
    match &m.data {
        Some(metric::Data::Gauge(g)) => {
            for dp in &g.data_points {
                let mut p = base(MetricType::Gauge, dp.time_unix_nano, &dp.attributes);
                p.value = number_value(dp.value.as_ref());
                attach_exemplars(&mut p, &dp.exemplars);
                out.push(p);
            }
        }
        Some(metric::Data::Sum(s)) => {
            for dp in &s.data_points {
                let mut p = base(MetricType::Sum, dp.time_unix_nano, &dp.attributes);
                p.value = number_value(dp.value.as_ref());
                p.extra = json!({
                    "is_monotonic": s.is_monotonic,
                    "temporality": s.aggregation_temporality,
                });
                attach_exemplars(&mut p, &dp.exemplars);
                out.push(p);
            }
        }
        Some(metric::Data::Histogram(h)) => {
            for dp in &h.data_points {
                let mut p = base(MetricType::Histogram, dp.time_unix_nano, &dp.attributes);
                p.value = dp.sum.unwrap_or(0.0);
                p.count = dp.count;
                p.extra = json!({
                    "bucket_counts": dp.bucket_counts,
                    "explicit_bounds": dp.explicit_bounds,
                    "min": dp.min,
                    "max": dp.max,
                    "temporality": h.aggregation_temporality,
                });
                attach_exemplars(&mut p, &dp.exemplars);
                out.push(p);
            }
        }
        Some(metric::Data::ExponentialHistogram(h)) => {
            for dp in &h.data_points {
                let mut p = base(
                    MetricType::ExponentialHistogram,
                    dp.time_unix_nano,
                    &dp.attributes,
                );
                p.value = dp.sum.unwrap_or(0.0);
                p.count = dp.count;
                p.extra = json!({
                    "scale": dp.scale,
                    "zero_count": dp.zero_count,
                    "min": dp.min,
                    "max": dp.max,
                });
                attach_exemplars(&mut p, &dp.exemplars);
                out.push(p);
            }
        }
        Some(metric::Data::Summary(s)) => {
            for dp in &s.data_points {
                let mut p = base(MetricType::Summary, dp.time_unix_nano, &dp.attributes);
                p.value = dp.sum;
                p.count = dp.count;
                let quantiles: Vec<Value> = dp
                    .quantile_values
                    .iter()
                    .map(|q| json!({"quantile": q.quantile, "value": q.value}))
                    .collect();
                p.extra = json!({ "quantiles": quantiles });
                out.push(p);
            }
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn exemplars_survive_ingest_and_keep_type_extras() {
        use opentelemetry_proto::tonic::common::v1::InstrumentationScope;
        use opentelemetry_proto::tonic::metrics::v1::{
            exemplar, Exemplar, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics, Sum,
        };
        let ex = |trace: &str, span: &str| Exemplar {
            filtered_attributes: vec![],
            time_unix_nano: 7,
            span_id: hex::decode(span).unwrap(),
            trace_id: hex::decode(trace).unwrap(),
            value: Some(exemplar::Value::AsDouble(4.5)),
        };
        let rm = ResourceMetrics {
            resource: None,
            scope_metrics: vec![ScopeMetrics {
                scope: Some(InstrumentationScope::default()),
                metrics: vec![Metric {
                    name: "http.server.duration".into(),
                    description: String::new(),
                    unit: "ms".into(),
                    metadata: vec![],
                    data: Some(super::metric::Data::Sum(Sum {
                        is_monotonic: true,
                        aggregation_temporality: 2,
                        data_points: vec![NumberDataPoint {
                            attributes: vec![],
                            start_time_unix_nano: 0,
                            time_unix_nano: 7,
                            exemplars: vec![
                                ex("aabbccddeeff00112233445566778899", "0011223344556677"),
                                // no trace id: cannot link anything, dropped
                                Exemplar {
                                    filtered_attributes: vec![],
                                    time_unix_nano: 7,
                                    span_id: vec![],
                                    trace_id: vec![],
                                    value: None,
                                },
                            ],
                            flags: 0,
                            value: Some(super::number_data_point::Value::AsDouble(1.0)),
                        }],
                    })),
                }],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        };

        let points = super::metrics_from_resource_metrics(&[rm]);
        assert_eq!(points.len(), 1);
        let p = &points[0];

        // The Sum's own extras are still there alongside the exemplars.
        assert_eq!(p.extra["is_monotonic"], true);
        assert_eq!(p.extra["temporality"], 2);

        let exemplars = p.exemplars();
        assert_eq!(exemplars.len(), 1, "the trace-less exemplar is dropped");
        assert_eq!(exemplars[0].trace_id, "aabbccddeeff00112233445566778899");
        assert_eq!(exemplars[0].span_id, "0011223344556677");
        assert_eq!(exemplars[0].value, 4.5);

        assert!(p.links_to("aabbccddeeff00112233445566778899", None));
        assert!(p.links_to("AABBCCDDEEFF00112233445566778899", Some("0011223344556677")));
        assert!(!p.links_to("aabbccddeeff00112233445566778899", Some("nope")));
        assert!(!p.links_to("different", None));
    }

    use super::*;
    use opentelemetry_proto::tonic::common::v1::InstrumentationScope;

    fn make_resource(service: &str) -> Resource {
        Resource {
            attributes: vec![KeyValue {
                key: "service.name".into(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::StringValue(service.into())),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn make_span(trace_id: u8, span_id: u8, parent: Option<u8>, name: &str) -> Span {
        Span {
            trace_id: vec![trace_id; 16],
            span_id: vec![span_id; 8],
            parent_span_id: parent.map(|p| vec![p; 8]).unwrap_or_default(),
            name: name.into(),
            kind: span::SpanKind::Server as i32,
            start_time_unix_nano: 1_000,
            end_time_unix_nano: 5_000,
            status: Some(Status {
                code: status::StatusCode::Error as i32,
                message: "boom".into(),
            }),
            attributes: vec![KeyValue {
                key: "http.method".into(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::StringValue("GET".into())),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn converts_spans_roundtrip() {
        let rs = ResourceSpans {
            resource: Some(make_resource("svc-a")),
            scope_spans: vec![ScopeSpans {
                scope: Some(InstrumentationScope {
                    name: "lib".into(),
                    version: "1.0".into(),
                    ..Default::default()
                }),
                spans: vec![make_span(1, 2, None, "GET /")],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        };
        let records = spans_from_resource_spans(&[rs]);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.service_name, "svc-a");
        assert_eq!(r.kind, "server");
        assert_eq!(r.duration_nanos(), 4_000);
        assert!(r.is_error());
        assert!(r.is_root());
        assert_eq!(r.attributes["http.method"], "GET");

        // Model → OTLP → model roundtrip preserves the essentials.
        let td = spans_to_traces_data(&records);
        let back = spans_from_resource_spans(&td.resource_spans);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].trace_id, r.trace_id);
        assert_eq!(back[0].name, r.name);
        assert_eq!(back[0].service_name, "svc-a");
        assert_eq!(back[0].status_code, 2);
    }

    #[test]
    fn nested_any_value_to_json() {
        let v = AnyValue {
            value: Some(any_value::Value::KvlistValue(KeyValueList {
                values: vec![KeyValue {
                    key: "xs".into(),
                    value: Some(AnyValue {
                        value: Some(any_value::Value::ArrayValue(ArrayValue {
                            values: vec![
                                AnyValue {
                                    value: Some(any_value::Value::IntValue(1)),
                                    ..Default::default()
                                },
                                AnyValue {
                                    value: Some(any_value::Value::BoolValue(true)),
                                    ..Default::default()
                                },
                            ],
                        })),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
            })),
        };
        assert_eq!(any_value_to_json(&v), json!({"xs": [1, true]}));
    }
}
