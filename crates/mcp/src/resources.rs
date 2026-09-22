//! Resources: the things worth having in context before asking anything.
//!
//! The catalog of services and metrics is what a query has to be written
//! against, and the query-language references are what it has to be written
//! in. A client that pins these reads the instance correctly on its first
//! attempt instead of its third.

use serde_json::json;

use crate::backend::Otel;
use crate::protocol::{Resource, ResourceContents};
use crate::syntax;

const JSON: &str = "application/json";
const MARKDOWN: &str = "text/markdown";

pub fn catalog() -> Vec<Resource> {
    let mut out = vec![
        Resource {
            uri: "otelview://services".into(),
            name: "services",
            title: "Services",
            description: "Every service that has reported telemetry.",
            mime_type: JSON,
        },
        Resource {
            uri: "otelview://metrics".into(),
            name: "metrics",
            title: "Metric catalog",
            description: "Every metric name, with type, unit and reporting services.",
            mime_type: JSON,
        },
        Resource {
            uri: "otelview://stats".into(),
            name: "stats",
            title: "Storage stats",
            description: "How much telemetry is stored, and in which backend.",
            mime_type: JSON,
        },
        Resource {
            uri: "otelview://config".into(),
            name: "config",
            title: "Configuration",
            description: "The running configuration, with secrets redacted.",
            mime_type: JSON,
        },
    ];
    out.extend(syntax::LANGUAGES.iter().map(|lang| Resource {
        uri: format!("otelview://syntax/{lang}"),
        name: match *lang {
            "kql" => "kql-syntax",
            "traceql" => "traceql-syntax",
            _ => "lucene-syntax",
        },
        title: match *lang {
            "kql" => "KQL reference",
            "traceql" => "TraceQL reference",
            _ => "Lucene reference",
        },
        description: match *lang {
            "kql" => "Grammar and examples for the log query language.",
            "traceql" => "Grammar and examples for the trace query language.",
            _ => "Grammar and examples for the language both signals accept.",
        },
        mime_type: MARKDOWN,
    }));
    out
}

/// `None` when the URI names no resource this server has.
pub async fn read(otel: &dyn Otel, uri: &str) -> Option<anyhow::Result<ResourceContents>> {
    if let Some(lang) = uri.strip_prefix("otelview://syntax/") {
        return syntax::sheet(lang).map(|sheet| {
            Ok(ResourceContents {
                uri: uri.to_string(),
                mime_type: MARKDOWN,
                text: sheet.to_string(),
            })
        });
    }
    let json = match uri {
        "otelview://services" => otel.services().await.map(|v| json!(v)),
        "otelview://metrics" => otel.metrics().await.map(|v| json!(v)),
        "otelview://stats" => otel.stats().await.map(|v| json!(v)),
        "otelview://config" => otel.config().await,
        _ => return None,
    };
    Some(
        json.map_err(|e| anyhow::anyhow!("{e}"))
            .and_then(|v| Ok(serde_json::to_string_pretty(&v)?))
            .map(|text| ResourceContents {
                uri: uri.to_string(),
                mime_type: JSON,
                text,
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_syntax_resource_has_a_sheet_behind_it() {
        for r in catalog() {
            if let Some(lang) = r.uri.strip_prefix("otelview://syntax/") {
                assert!(syntax::sheet(lang).is_some(), "{} is dead", r.uri);
            }
        }
    }

    #[test]
    fn resource_uris_are_unique() {
        let mut uris: Vec<String> = catalog().into_iter().map(|r| r.uri).collect();
        let before = uris.len();
        uris.sort();
        uris.dedup();
        assert_eq!(before, uris.len());
    }
}
