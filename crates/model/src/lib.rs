//! Internal data model shared by receivers, storage backends and the query API.
//!
//! OTLP payloads are flattened into these records at ingest time so every
//! storage backend deals with one simple shape per signal.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single span, flattened from OTLP `ResourceSpans`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanRecord {
    /// Hex-encoded 16-byte trace id.
    pub trace_id: String,
    /// Hex-encoded 8-byte span id.
    pub span_id: String,
    /// Hex-encoded parent span id, empty string for root spans.
    #[serde(default)]
    pub parent_span_id: String,
    pub name: String,
    /// `resource.attributes["service.name"]`, or "unknown_service".
    pub service_name: String,
    /// OTLP SpanKind as a lowercase string: internal|server|client|producer|consumer|unspecified.
    pub kind: String,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    /// 0 = Unset, 1 = Ok, 2 = Error.
    pub status_code: i32,
    #[serde(default)]
    pub status_message: String,
    /// Span attributes as a JSON object.
    pub attributes: Value,
    /// Resource attributes as a JSON object.
    pub resource_attributes: Value,
    /// Span events: `[{name, time_unix_nano, attributes}]`.
    pub events: Value,
    /// Span links: `[{trace_id, span_id, attributes}]`.
    pub links: Value,
    #[serde(default)]
    pub scope_name: String,
    #[serde(default)]
    pub scope_version: String,
}

impl SpanRecord {
    pub fn duration_nanos(&self) -> u64 {
        self.end_time_unix_nano
            .saturating_sub(self.start_time_unix_nano)
    }
    pub fn is_error(&self) -> bool {
        self.status_code == 2
    }
    pub fn is_root(&self) -> bool {
        self.parent_span_id.is_empty()
    }
}

/// A single log record, flattened from OTLP `ResourceLogs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    pub time_unix_nano: u64,
    pub observed_time_unix_nano: u64,
    /// OTLP severity number (1..=24), 0 if unset.
    pub severity_number: i32,
    #[serde(default)]
    pub severity_text: String,
    /// Log body rendered as JSON (string bodies stay plain strings).
    pub body: Value,
    pub attributes: Value,
    pub resource_attributes: Value,
    pub service_name: String,
    #[serde(default)]
    pub trace_id: String,
    #[serde(default)]
    pub span_id: String,
    #[serde(default)]
    pub scope_name: String,
}

/// The kind of a metric data point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricType {
    Gauge,
    Sum,
    Histogram,
    ExponentialHistogram,
    Summary,
}

impl MetricType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricType::Gauge => "gauge",
            MetricType::Sum => "sum",
            MetricType::Histogram => "histogram",
            MetricType::ExponentialHistogram => "exponential_histogram",
            MetricType::Summary => "summary",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gauge" => Some(MetricType::Gauge),
            "sum" => Some(MetricType::Sum),
            "histogram" => Some(MetricType::Histogram),
            "exponential_histogram" => Some(MetricType::ExponentialHistogram),
            "summary" => Some(MetricType::Summary),
            _ => None,
        }
    }
}

/// One metric data point, flattened from OTLP `ResourceMetrics`.
///
/// Histogram points carry `value` = sum, plus `count` and bucket data in
/// `extra`; gauge/sum points carry the point value and `count` = 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub unit: String,
    pub metric_type: MetricType,
    pub service_name: String,
    pub time_unix_nano: u64,
    pub value: f64,
    #[serde(default)]
    pub count: u64,
    /// Data-point attributes (the series key).
    pub attributes: Value,
    pub resource_attributes: Value,
    /// Type-specific extras: histogram buckets, quantiles, monotonic/temporality flags.
    pub extra: Value,
}

/// Summary of one trace for list views.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    pub trace_id: String,
    /// Name of the root span (or earliest span if the root wasn't received).
    pub root_name: String,
    pub root_service: String,
    pub start_time_unix_nano: u64,
    pub duration_nanos: u64,
    pub span_count: u64,
    pub error_count: u64,
    /// Distinct service names participating in the trace.
    pub services: Vec<String>,
}

/// Query for the trace search endpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceQuery {
    pub service: Option<String>,
    pub operation: Option<String>,
    /// Substring match against span attribute values.
    pub attribute_query: Option<String>,
    pub min_duration_nanos: Option<u64>,
    pub max_duration_nanos: Option<u64>,
    pub start_time_min_unix_nano: Option<u64>,
    pub start_time_max_unix_nano: Option<u64>,
    pub errors_only: bool,
    pub limit: usize,
}

/// Query for the log search endpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogQuery {
    pub service: Option<String>,
    /// Minimum severity number (inclusive).
    pub min_severity: Option<i32>,
    /// Substring match against body and attributes.
    pub search: Option<String>,
    pub trace_id: Option<String>,
    pub time_min_unix_nano: Option<u64>,
    pub time_max_unix_nano: Option<u64>,
    pub limit: usize,
}

/// Description of one metric (deduplicated by name).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInfo {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub metric_type: MetricType,
    pub services: Vec<String>,
}

/// Query for metric series.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricQuery {
    pub name: String,
    pub service: Option<String>,
    pub time_min_unix_nano: Option<u64>,
    pub time_max_unix_nano: Option<u64>,
    /// Maximum number of points per series (downsampled if exceeded).
    pub max_points: usize,
}

/// One series of a metric: a distinct (service, attributes) combination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSeries {
    pub service_name: String,
    pub attributes: Value,
    pub points: Vec<SeriesPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesPoint {
    pub time_unix_nano: u64,
    pub value: f64,
}

/// Counters shown in the status line.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageStats {
    pub spans: u64,
    pub logs: u64,
    pub metric_points: u64,
    pub services: u64,
    pub backend: String,
}

/// Map OTLP severity number to a coarse level name.
pub fn severity_level(severity_number: i32) -> &'static str {
    match severity_number {
        1..=4 => "trace",
        5..=8 => "debug",
        9..=12 => "info",
        13..=16 => "warn",
        17..=20 => "error",
        21..=24 => "fatal",
        _ => "unset",
    }
}
