//! What a query can be evaluated against.
//!
//! Both record shapes implement [`Doc`], which is what lets one grammar
//! serve logs and traces: a field lookup resolves the record's own columns
//! first, then falls through to its attributes.

use otelview_model::{severity_level, LogRecord, SpanRecord};
use serde_json::Value;

use crate::kql::{json_lookup, value_to_string};

/// What a query can be evaluated against. Implemented for both records so
/// one grammar serves logs and traces.
pub trait Doc {
    /// Values held under a field name, which may be an attribute path.
    fn field(&self, name: &str) -> Vec<Value>;
    /// Everything searchable, for terms written without a field.
    fn text(&self) -> String;
}

impl Doc for LogRecord {
    fn field(&self, name: &str) -> Vec<Value> {
        match name.to_lowercase().as_str() {
            "service" | "service.name" | "service_name" => {
                vec![Value::String(self.service_name.clone())]
            }
            "level" | "severity" => vec![
                Value::String(severity_level(self.severity_number).to_string()),
                Value::String(self.severity_text.clone()),
            ],
            "severity_number" => vec![Value::from(self.severity_number)],
            "body" | "message" => vec![self.body.clone()],
            "trace_id" => vec![Value::String(self.trace_id.clone())],
            "span_id" => vec![Value::String(self.span_id.clone())],
            "scope" => vec![Value::String(self.scope_name.clone())],
            _ => attr_lookup(&[&self.attributes, &self.resource_attributes], name),
        }
    }

    fn text(&self) -> String {
        format!(
            "{} {} {} {} {}",
            value_to_string(&self.body),
            self.attributes,
            self.resource_attributes,
            self.severity_text,
            self.service_name
        )
    }
}

impl Doc for SpanRecord {
    fn field(&self, name: &str) -> Vec<Value> {
        match name.to_lowercase().as_str() {
            "service" | "service.name" | "service_name" => {
                vec![Value::String(self.service_name.clone())]
            }
            "name" | "operation" => vec![Value::String(self.name.clone())],
            "kind" => vec![Value::String(self.kind.clone())],
            "status" | "status_code" => vec![Value::from(self.status_code)],
            "status_message" => vec![Value::String(self.status_message.clone())],
            "duration" | "duration_nanos" => vec![Value::from(self.duration_nanos())],
            "trace_id" => vec![Value::String(self.trace_id.clone())],
            "span_id" => vec![Value::String(self.span_id.clone())],
            "parent_span_id" => vec![Value::String(self.parent_span_id.clone())],
            "scope" => vec![Value::String(self.scope_name.clone())],
            _ => attr_lookup(&[&self.attributes, &self.resource_attributes], name),
        }
    }

    fn text(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.name, self.service_name, self.kind, self.attributes, self.resource_attributes
        )
    }
}

fn attr_lookup(sources: &[&Value], name: &str) -> Vec<Value> {
    sources
        .iter()
        .filter_map(|src| json_lookup(src, name).cloned())
        .collect()
}
