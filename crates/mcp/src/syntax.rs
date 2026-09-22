//! Cheat sheets for the three query languages.
//!
//! A model that has to guess at syntax writes TraceQL that looks like
//! PromQL and gets a parse error back. These are served both as a tool
//! (`query_syntax`) and as resources, so a client can pin them into context
//! before the first query rather than learning from failures.

/// The languages this server can explain, in the order they are listed.
pub const LANGUAGES: &[&str] = &["kql", "traceql", "lucene"];

pub fn sheet(language: &str) -> Option<&'static str> {
    match language.trim().to_ascii_lowercase().as_str() {
        "kql" => Some(KQL),
        "traceql" => Some(TRACEQL),
        "lucene" => Some(LUCENE),
        _ => None,
    }
}

pub const KQL: &str = r#"# KQL — log search (`search_logs`, `log_histogram`)

Kibana-style. Case-insensitive.

    field:value              equality
    field:"quoted phrase"    phrases with spaces
    field:val*               wildcards anywhere in the value
    field:>100  >=  <  <=    numeric comparisons
    term                     bare term: full-text over body, attributes,
                             service and severity
    and  or  not  ( )        logic; bare adjacency means `and`

Fields resolve against `service` / `service.name`, `level` / `severity`,
`severity_number`, `body` / `message`, `trace_id`, `span_id`, `scope`, and
any dotted attribute or resource-attribute key.

Examples:

    http.method:POST and http.status_code:>=500
    level:error and not http.target:/health
    "connection refused" and service:payments
"#;

pub const TRACEQL: &str = r#"# TraceQL — trace search (`search_traces`)

Tempo-style. The unit of evaluation is a whole trace: a trace matches when
its spans satisfy the spanset expression.

    { span.http.method = "GET" }           span attribute
    { resource.service.name = "cart" }     resource attribute
    { .http.status_code >= 500 }           unscoped: span first, then resource
    { span.foo }                           attribute exists

Intrinsics: `name`, `duration`, `status`, `kind`, `rootName`,
`rootServiceName`, `traceDuration`.

    operators   =  !=  >  >=  <  <=  =~  !~      (last two are regex)
    durations   10ns 500us 100ms 1.5s 2m 1h
    enums       status = error|ok|unset
                kind = server|client|internal|producer|consumer
    logic       && || ! and parentheses, inside and between spansets
    aggregates  { … } | count() > 2
                { … } | avg(duration) > 100ms   (sum/min/max too)

Not supported — these are refused with a named error rather than silently
ignored: structural operators (`>>`, `>`, `~`, `<<`), `select()`, `by()`.

Examples:

    { status = error && duration > 100ms }
    { name = "charge" } && { .http.method = "GET" }
    { resource.service.name = "gateway" } | count() > 5
"#;

pub const LUCENE: &str = r#"# Lucene — logs *and* traces (`search_logs`, `search_traces`)

The one language both signals accept. A trace matches when any single span
of it matches. The default operator is OR, so `a b` matches either.

    term                     "a phrase"        field:term
    field:"a phrase"         te?t (one char)   te*t (any run)
    roam~ / roam~1           fuzzy (default edit distance 2)
    "jakarta apache"~10      proximity
    [1 TO 5]                 inclusive range
    {1 TO 5}                 exclusive range, mixed [1 TO 5} allowed
    [500 TO *]               open end
    AND OR NOT               also && || !
    +must  -must_not         required / excluded clauses
    (a OR b) AND c           grouping
    field:(a OR b)           field grouping
    path:\/api\/traces       backslash escaping

Boosts (`term^4`) parse and are ignored: this decides whether a record
matches, and there is no ranking for a weight to affect.

Examples:

    http.method:POST AND http.status_code:[500 TO *]
    level:ERROR AND "connection refused"
    service:gateway AND NOT http.target:/health
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_language_has_a_sheet() {
        for l in LANGUAGES {
            assert!(sheet(l).is_some(), "{l} has no cheat sheet");
        }
        assert!(sheet("promql").is_none());
        assert!(sheet("TraceQL").is_some(), "matching is case-insensitive");
    }
}
