//! Kibana-style query language (KQL) for log search.
//!
//! Supported syntax:
//! - `field:value` — case-insensitive equality on a field
//! - `field:"quoted phrase"` — phrases with spaces
//! - `field:val*` — wildcards anywhere in the value
//! - `field:>100`, `>=`, `<`, `<=` — numeric comparisons
//! - `term` — bare terms full-text match body/attributes/service/severity
//! - `and`, `or`, `not`, parentheses; bare adjacency means `and`
//!
//! Fields resolve against: `service`/`service.name`, `level`/`severity`,
//! `severity_number`, `body`/`message`, `trace_id`, `span_id`, `scope`, and
//! any (dotted) attribute or resource-attribute key.

use otelview_model::{severity_level, LogRecord};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Match {
        field: Option<String>,
        op: Op,
        value: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    LParen,
    RParen,
    And,
    Or,
    Not,
    /// A `field:value` pair or a bare term.
    Term {
        field: Option<String>,
        value: String,
    },
}

pub fn parse(input: &str) -> Result<Option<Expr>, String> {
    let tokens = lex(input)?;
    if tokens.is_empty() {
        return Ok(None);
    }
    let mut p = Parser { tokens, pos: 0 };
    let expr = p.or_expr()?;
    if p.pos != p.tokens.len() {
        return Err(format!("unexpected token at position {}", p.pos));
    }
    Ok(Some(expr))
}

fn lex(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' => i += 1,
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '"' => {
                let (s, next) = read_quoted(&chars, i)?;
                tokens.push(Token::Term {
                    field: None,
                    value: s,
                });
                i = next;
            }
            _ => {
                // Read a word up to whitespace/paren; a ':' splits field:value,
                // where the value may itself be quoted.
                let start = i;
                let mut field: Option<String> = None;
                let mut value = String::new();
                while i < chars.len() && !" \t\n()".contains(chars[i]) {
                    if chars[i] == ':' && field.is_none() {
                        field = Some(chars[start..i].iter().collect());
                        value.clear();
                        i += 1;
                        if i < chars.len() && chars[i] == '"' {
                            let (s, next) = read_quoted(&chars, i)?;
                            value = s;
                            i = next;
                        }
                        continue;
                    }
                    value.push(chars[i]);
                    i += 1;
                }
                if field.is_none() {
                    value = chars[start..i].iter().collect();
                }
                match (field.as_deref(), value.to_lowercase().as_str()) {
                    (None, "and") => tokens.push(Token::And),
                    (None, "or") => tokens.push(Token::Or),
                    (None, "not") => tokens.push(Token::Not),
                    _ => tokens.push(Token::Term { field, value }),
                }
            }
        }
    }
    Ok(tokens)
}

fn read_quoted(chars: &[char], start: usize) -> Result<(String, usize), String> {
    let mut s = String::new();
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '"' => return Ok((s, i + 1)),
            '\\' if i + 1 < chars.len() => {
                s.push(chars[i + 1]);
                i += 2;
            }
            c => {
                s.push(c);
                i += 1;
            }
        }
    }
    Err("unterminated quoted string".into())
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn or_expr(&mut self) -> Result<Expr, String> {
        let mut parts = vec![self.and_expr()?];
        while matches!(self.peek(), Some(Token::Or)) {
            self.pos += 1;
            parts.push(self.and_expr()?);
        }
        Ok(if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            Expr::Or(parts)
        })
    }

    fn and_expr(&mut self) -> Result<Expr, String> {
        let mut parts = vec![self.unary()?];
        loop {
            match self.peek() {
                Some(Token::And) => {
                    self.pos += 1;
                    parts.push(self.unary()?);
                }
                // Bare adjacency (a space between terms) means "and".
                Some(Token::Not) | Some(Token::LParen) | Some(Token::Term { .. }) => {
                    parts.push(self.unary()?);
                }
                _ => break,
            }
        }
        Ok(if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            Expr::And(parts)
        })
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Token::Not) => {
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Some(Token::LParen) => {
                self.pos += 1;
                let e = self.or_expr()?;
                match self.peek() {
                    Some(Token::RParen) => {
                        self.pos += 1;
                        Ok(e)
                    }
                    _ => Err("expected ')'".into()),
                }
            }
            Some(Token::Term { .. }) => {
                let Token::Term { field, value } = self.tokens[self.pos].clone() else {
                    unreachable!()
                };
                self.pos += 1;
                let (op, value) = split_op(&value);
                Ok(Expr::Match { field, op, value })
            }
            other => Err(format!("unexpected token {other:?}")),
        }
    }
}

fn split_op(value: &str) -> (Op, String) {
    for (prefix, op) in [
        (">=", Op::Gte),
        ("<=", Op::Lte),
        (">", Op::Gt),
        ("<", Op::Lt),
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            return (op, rest.to_string());
        }
    }
    (Op::Eq, value.to_string())
}

/// Case-insensitive glob match; `*` matches any run of characters.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let text = text.to_lowercase();
    if !pattern.contains('*') {
        return pattern == text;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut rest = text.as_str();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match rest.find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 {
                    return false;
                }
                rest = &rest[idx + part.len()..];
            }
            None => return false,
        }
    }
    if let Some(last) = parts.last() {
        if !last.is_empty() && !text.ends_with(last.to_lowercase().as_str()) {
            return false;
        }
    }
    true
}

pub(crate) fn json_lookup<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    if let Some(v) = root.get(path) {
        return Some(v);
    }
    // Dotted path into nested objects.
    let mut cur = root;
    for part in path.split('.') {
        cur = cur.get(part)?;
    }
    Some(cur)
}

fn field_values(log: &LogRecord, field: &str) -> Vec<Value> {
    match field.to_lowercase().as_str() {
        "service" | "service.name" | "service_name" => {
            vec![Value::String(log.service_name.clone())]
        }
        "level" | "severity" => vec![
            Value::String(severity_level(log.severity_number).to_string()),
            Value::String(log.severity_text.clone()),
        ],
        "severity_number" => vec![Value::from(log.severity_number)],
        "body" | "message" => vec![log.body.clone()],
        "trace_id" => vec![Value::String(log.trace_id.clone())],
        "span_id" => vec![Value::String(log.span_id.clone())],
        "scope" => vec![Value::String(log.scope_name.clone())],
        _ => {
            let mut out = Vec::new();
            if let Some(v) = json_lookup(&log.attributes, field) {
                out.push(v.clone());
            }
            if let Some(v) = json_lookup(&log.resource_attributes, field) {
                out.push(v.clone());
            }
            out
        }
    }
}

pub(crate) fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn matches_value(op: Op, expected: &str, actual: &Value) -> bool {
    match op {
        Op::Eq => {
            if actual.is_object() || actual.is_array() {
                return glob_match(&format!("*{expected}*"), &value_to_string(actual));
            }
            glob_match(expected, &value_to_string(actual))
        }
        _ => {
            let (Some(exp), Some(act)) = (
                expected.parse::<f64>().ok(),
                actual
                    .as_f64()
                    .or_else(|| value_to_string(actual).parse::<f64>().ok()),
            ) else {
                return false;
            };
            match op {
                Op::Gt => act > exp,
                Op::Gte => act >= exp,
                Op::Lt => act < exp,
                Op::Lte => act <= exp,
                Op::Eq => unreachable!(),
            }
        }
    }
}

pub fn eval(expr: &Expr, log: &LogRecord) -> bool {
    match expr {
        Expr::And(parts) => parts.iter().all(|e| eval(e, log)),
        Expr::Or(parts) => parts.iter().any(|e| eval(e, log)),
        Expr::Not(inner) => !eval(inner, log),
        Expr::Match {
            field: Some(field),
            op,
            value,
        } => {
            let values = field_values(log, field);
            !values.is_empty() && values.iter().any(|v| matches_value(*op, value, v))
        }
        Expr::Match {
            field: None,
            op: Op::Eq,
            value,
        } => {
            // Bare term: substring over the whole record.
            let hay = format!(
                "{} {} {} {} {}",
                value_to_string(&log.body),
                log.attributes,
                log.resource_attributes,
                log.severity_text,
                log.service_name
            )
            .to_lowercase();
            if value.contains('*') {
                glob_match(&format!("*{value}*"), &hay)
            } else {
                hay.contains(&value.to_lowercase())
            }
        }
        Expr::Match { field: None, .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    fn matches(q: &str) -> bool {
        eval(&parse(q).unwrap().unwrap(), &log())
    }

    #[test]
    fn field_equality_and_case() {
        assert!(matches("http.method:POST"));
        assert!(matches("http.method:post"));
        assert!(!matches("http.method:GET"));
        assert!(matches("service:payments"));
        assert!(matches("level:error"));
        assert!(matches("host.name:app-2"));
    }

    #[test]
    fn numeric_comparisons() {
        assert!(matches("http.status_code:>=400"));
        assert!(matches("http.status_code:>401"));
        assert!(!matches("http.status_code:<400"));
        assert!(matches("severity_number:>=17"));
        assert!(matches("order.items:<=3"));
    }

    #[test]
    fn wildcards_phrases_and_bare_terms() {
        assert!(matches("http.method:P*T"));
        assert!(matches("body:\"payment declined*\""));
        assert!(matches("declined"));
        assert!(matches("o-42"));
        assert!(!matches("refunded"));
    }

    #[test]
    fn boolean_operators_and_parens() {
        assert!(matches("http.method:POST and level:error"));
        assert!(matches("http.method:GET or level:error"));
        assert!(!matches("http.method:GET and level:error"));
        assert!(matches("not http.method:GET"));
        assert!(matches(
            "(http.method:GET or http.method:POST) and service:payments"
        ));
        // adjacency = and
        assert!(matches("declined service:payments"));
        assert!(!matches("declined service:checkout"));
    }

    #[test]
    fn nested_json_lookup() {
        assert!(matches("order.id:o-42"));
    }

    #[test]
    fn parse_errors_and_empty() {
        assert!(parse("").unwrap().is_none());
        assert!(parse("   ").unwrap().is_none());
        assert!(parse("(a or b").is_err());
        assert!(parse("field:\"unterminated").is_err());
    }
}
