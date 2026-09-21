//! Parser and evaluator behaviour over realistic records.
//!
//! Covers the syntax end to end, against one log and one span. Tests for
//! individual helpers live beside them, in the module they belong to.

use otelview_model::{LogRecord, SpanRecord};
use serde_json::json;

use super::ast::Query;
use super::{eval, parse};

fn log() -> LogRecord {
    LogRecord {
        time_unix_nano: 1,
        observed_time_unix_nano: 1,
        severity_number: 17,
        severity_text: "ERROR".into(),
        body: json!("payment declined for order o-42"),
        attributes: json!({
            "http.method": "POST",
            "http.status_code": 402,
            "order": {"id": "o-42", "items": 3}
        }),
        resource_attributes: json!({"service.name": "payments", "host.name": "app-2"}),
        service_name: "payments".into(),
        trace_id: "abc123".into(),
        span_id: "def".into(),
        scope_name: "billing".into(),
    }
}

fn span() -> SpanRecord {
    SpanRecord {
        trace_id: "abc123".into(),
        span_id: "s1".into(),
        parent_span_id: String::new(),
        name: "POST /checkout".into(),
        service_name: "gateway".into(),
        kind: "server".into(),
        start_time_unix_nano: 1_000,
        end_time_unix_nano: 1_000 + 250_000_000,
        status_code: 2,
        status_message: "upstream timeout".into(),
        attributes: json!({"http.method": "POST", "http.status_code": 502}),
        resource_attributes: json!({"service.name": "gateway"}),
        events: json!([]),
        links: json!([]),
        scope_name: String::new(),
        scope_version: String::new(),
    }
}

fn m(q: &str) -> bool {
    eval(&parse(q).unwrap().unwrap(), &log())
}
fn ms(q: &str) -> bool {
    eval(&parse(q).unwrap().unwrap(), &span())
}

#[test]
fn terms_and_fields() {
    assert!(m("declined"));
    assert!(m("http.method:POST"));
    assert!(m("service:payments"));
    assert!(m("level:ERROR"));
    assert!(!m("http.method:GET"));
    assert!(!m("nonsense"));
    // Field matching is case-insensitive on the value, like the term case.
    assert!(m("http.method:post"));
}

#[test]
fn nested_attribute_paths() {
    assert!(m("order.id:o-42"));
    assert!(m("order.items:3"));
    assert!(!m("order.id:o-99"));
}

#[test]
fn phrases() {
    assert!(m(r#""payment declined""#));
    assert!(!m(r#""declined payment""#));
    assert!(m(r#"body:"payment declined for order""#));
    // Slop lets the words drift apart.
    assert!(!m(r#""payment order""#));
    assert!(m(r#""payment order"~3"#));
}

#[test]
fn wildcards() {
    assert!(m("http.method:PO*"));
    assert!(m("http.method:P?ST"));
    assert!(!m("http.method:P?T"));
    assert!(m("service:pay*"));
    assert!(!m("service:gate*"));
    // A bare term with no wildcard is a substring match.
    assert!(m("service:ayment"));
}

#[test]
fn fuzzy() {
    assert!(m("paymnet~")); // one transposition-ish edit
    assert!(m("declimed~1"));
    assert!(!m("declimed~0")); // a plain integer is the distance itself
    assert!(m("declined~0")); // ...so ~0 is an exact match
    assert!(!m("completely~1"));
    // The retired similarity spelling still parses.
    assert!(m("declimed~0.9"));
    assert!(!m("completely~0.5"));
    // Lucene tops out at two edits; asking for more does not widen it.
    assert!(!m("dexlimwd~5"));
}

#[test]
fn ranges() {
    assert!(m("http.status_code:[400 TO 500]"));
    assert!(!m("http.status_code:[200 TO 400]"));
    assert!(m("http.status_code:{401 TO 403}"));
    assert!(!m("http.status_code:{402 TO 403}"));
    assert!(m("http.status_code:[400 TO *]"));
    assert!(m("http.status_code:[* TO 500]"));
    // Mixed bounds.
    assert!(m("http.status_code:[402 TO 500}"));
    assert!(!m("http.status_code:{402 TO 500]"));
    // Lexicographic when the values are not numbers.
    assert!(m("service:[pa TO pz]"));
}

#[test]
fn boolean_operators() {
    assert!(m("declined AND payments"));
    assert!(!m("declined AND nonsense"));
    assert!(m("nonsense OR declined"));
    assert!(m("declined NOT nonsense"));
    assert!(!m("declined NOT payments"));
    assert!(m("+declined +payments"));
    assert!(!m("+declined +nonsense"));
    assert!(m("declined -nonsense"));
    assert!(!m("declined -payments"));
    // && || ! spellings.
    assert!(m("declined && payments"));
    assert!(m("nonsense || declined"));
}

#[test]
fn default_operator_is_or() {
    // Lucene's default: either term is enough.
    assert!(m("declined nonsense"));
    assert!(!m("nothing nonsense"));
}

#[test]
fn grouping_and_field_grouping() {
    assert!(m("(nonsense OR declined) AND payments"));
    assert!(!m("(nonsense OR nothing) AND payments"));
    assert!(m("http.status_code:(200 OR 402)"));
    assert!(!m("http.status_code:(200 OR 404)"));
    assert!(m("http.method:(POST OR GET)"));
}

#[test]
fn boost_is_parsed_and_ignored() {
    assert!(m("declined^4"));
    assert!(m("http.method:POST^2 AND payments^0.5"));
    assert_eq!(parse("declined^4").unwrap(), parse("declined").unwrap());
}

#[test]
fn escaping() {
    // An escaped colon is part of the term, not a field separator.
    let q = parse(r"weird\:value").unwrap().unwrap();
    assert_eq!(
        q,
        Query::Term {
            field: None,
            text: "weird:value".into()
        }
    );
    // Dashes inside words are not the prohibit operator.
    assert!(m("order.id:o-42"));
}

#[test]
fn works_over_spans_too() {
    assert!(ms("name:checkout"));
    assert!(ms("kind:server"));
    assert!(ms("status:2"));
    assert!(ms("status_message:timeout"));
    assert!(ms("http.status_code:[500 TO 599]"));
    assert!(ms(r#""POST /checkout""#));
    assert!(ms("service:gateway AND name:POST*"));
    assert!(!ms("service:payments"));
}

#[test]
fn parse_errors_and_empty() {
    assert!(parse("").unwrap().is_none());
    assert!(parse("   ").unwrap().is_none());
    // The message names what is missing, not whatever the inner parse hit.
    assert_eq!(parse("(unclosed").unwrap_err(), "expected ')'");
    assert_eq!(parse("bad(").unwrap_err(), "expected ')'");
    assert_eq!(parse("a ()").unwrap_err(), "empty group '()'");
    // Errors quote the query's own text, not lexer variant names.
    assert_eq!(parse("a)").unwrap_err(), "unexpected ')'");
    assert!(parse(r#""unterminated"#).is_err());
    assert!(parse("a)").is_err());
    assert!(parse("field:[1 TO").is_err());
    assert!(parse("field:[1 2]").is_err());
    assert!(parse("AND").is_err());
}
