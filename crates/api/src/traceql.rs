//! TraceQL — Tempo-style query language for trace search.
//!
//! Like [`crate::kql`], this parses to an AST and evaluates as a predicate in
//! Rust rather than lowering to SQL, so it works identically across every
//! storage backend. The unit of evaluation is a whole trace (all of its
//! spans), because TraceQL selects traces by what their spans look like.
//!
//! Supported syntax:
//! - `{ span.http.method = "GET" }` — span attribute
//! - `{ resource.service.name = "checkout" }` — resource attribute
//! - `{ .http.status_code >= 500 }` — unscoped: span attributes, then resource
//! - `{ span.foo }` — attribute exists
//! - intrinsics: `name`, `duration`, `status`, `kind`, `rootName`,
//!   `rootServiceName`, `traceDuration`
//! - operators: `=` `!=` `>` `>=` `<` `<=` `=~` `!~` (the last two are regex)
//! - durations: `10ns`, `500us`, `100ms`, `1.5s`, `2m`, `1h`
//! - enums: `status = error|ok|unset`, `kind = server|client|internal|…`
//! - logic inside a spanset: `&&`, `||`, `!`, parentheses
//! - logic between spansets: `{…} && {…}`, `{…} || {…}`
//! - aggregates: `{…} | count() > 2`, `{…} | avg(duration) > 100ms`
//!   (also `sum`/`min`/`max`, over `duration` or any numeric attribute)
//!
//! Not supported yet — these produce a clear error rather than being silently
//! ignored: structural operators (`>>`, `>`, `~`, `<<`), `select()`, `by()`.

use otelview_model::SpanRecord;
use regex::Regex;
use serde_json::Value;

use crate::kql::{json_lookup, value_to_string};

/* ---------------------------------------------------------------- AST -- */

/// A spanset expression: one or more `{…}` selectors combined with `&&`/`||`.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Set(Spanset),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

/// A single `{ … }` selector plus any `| count() > n` style filters.
#[derive(Debug, Clone, PartialEq)]
pub struct Spanset {
    /// `None` for the bare `{}`, which selects every span.
    pub filter: Option<Cond>,
    pub aggregates: Vec<AggFilter>,
}

/// A condition inside `{ … }`, evaluated against one span at a time.
#[derive(Debug, Clone, PartialEq)]
pub enum Cond {
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    Not(Box<Cond>),
    Cmp {
        field: Field,
        op: Op,
        value: Val,
    },
    /// A bare field reference: true when the attribute is present.
    Exists(Field),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Intrinsic(Intrinsic),
    /// `span.foo` — span attributes only.
    Span(String),
    /// `resource.foo` — resource attributes only.
    Resource(String),
    /// `.foo` — span attributes, falling back to resource attributes.
    Unscoped(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intrinsic {
    Name,
    Duration,
    Status,
    Kind,
    RootName,
    RootServiceName,
    TraceDuration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Re,
    Nre,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Str(String),
    Num(f64),
    /// A duration literal, in nanoseconds.
    Dur(u64),
    Bool(bool),
    /// Compiled at parse time so a bad pattern is a parse error, and so the
    /// regex is not recompiled once per span.
    Re(Pattern),
}

/// A `Regex` that can sit in a `PartialEq` AST.
#[derive(Debug, Clone)]
pub struct Pattern(Regex);

impl Pattern {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AggFilter {
    pub agg: Agg,
    pub op: Op,
    pub value: Val,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Agg {
    Count,
    Avg(Field),
    Sum(Field),
    Min(Field),
    Max(Field),
}

/* -------------------------------------------------------------- lexer -- */

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    LBrace,
    RBrace,
    LParen,
    RParen,
    Pipe,
    AndAnd,
    OrOr,
    Bang,
    Op(Op),
    Ident(String),
    Str(String),
    Num(f64),
    Dur(u64),
    /// Recognised but unimplemented spanset operators, kept as a token so the
    /// parser can name them in the error instead of failing cryptically.
    Structural(&'static str),
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '/' | '@' | ':')
}

fn lex(input: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '{' => {
                out.push(Tok::LBrace);
                i += 1;
            }
            '}' => {
                out.push(Tok::RBrace);
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '&' => {
                if chars.get(i + 1) == Some(&'&') {
                    out.push(Tok::AndAnd);
                    i += 2;
                } else {
                    return Err("expected '&&'".into());
                }
            }
            '|' => {
                if chars.get(i + 1) == Some(&'|') {
                    out.push(Tok::OrOr);
                    i += 2;
                } else {
                    out.push(Tok::Pipe);
                    i += 1;
                }
            }
            '!' => match chars.get(i + 1) {
                Some('=') => {
                    out.push(Tok::Op(Op::Ne));
                    i += 2;
                }
                Some('~') => {
                    out.push(Tok::Op(Op::Nre));
                    i += 2;
                }
                _ => {
                    out.push(Tok::Bang);
                    i += 1;
                }
            },
            '=' => match chars.get(i + 1) {
                Some('~') => {
                    out.push(Tok::Op(Op::Re));
                    i += 2;
                }
                // `==` is accepted as a synonym for `=`.
                Some('=') => {
                    out.push(Tok::Op(Op::Eq));
                    i += 2;
                }
                _ => {
                    out.push(Tok::Op(Op::Eq));
                    i += 1;
                }
            },
            '>' => match chars.get(i + 1) {
                Some('>') => {
                    out.push(Tok::Structural(">>"));
                    i += 2;
                }
                Some('=') => {
                    out.push(Tok::Op(Op::Gte));
                    i += 2;
                }
                _ => {
                    out.push(Tok::Op(Op::Gt));
                    i += 1;
                }
            },
            '<' => match chars.get(i + 1) {
                Some('<') => {
                    out.push(Tok::Structural("<<"));
                    i += 2;
                }
                Some('=') => {
                    out.push(Tok::Op(Op::Lte));
                    i += 2;
                }
                _ => {
                    out.push(Tok::Op(Op::Lt));
                    i += 1;
                }
            },
            '~' => {
                out.push(Tok::Structural("~"));
                i += 1;
            }
            '"' | '\'' => {
                let (s, next) = read_quoted(&chars, i)?;
                out.push(Tok::Str(s));
                i = next;
            }
            c if c.is_ascii_digit() => {
                let (tok, next) = read_number(&chars, i)?;
                out.push(tok);
                i = next;
            }
            c if is_ident_char(c) => {
                let start = i;
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect()));
            }
            other => return Err(format!("unexpected character {other:?}")),
        }
    }
    Ok(out)
}

fn read_quoted(chars: &[char], start: usize) -> Result<(String, usize), String> {
    let quote = chars[start];
    let mut s = String::new();
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                s.push(chars[i + 1]);
                i += 2;
            }
            c if c == quote => return Ok((s, i + 1)),
            c => {
                s.push(c);
                i += 1;
            }
        }
    }
    Err("unterminated quoted string".into())
}

/// A number, optionally carrying a duration unit (`100ms`, `1.5s`).
fn read_number(chars: &[char], start: usize) -> Result<(Tok, usize), String> {
    let mut i = start;
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
        i += 1;
    }
    let num: String = chars[start..i].iter().collect();
    let n: f64 = num.parse().map_err(|_| format!("invalid number {num:?}"))?;

    let unit_start = i;
    while i < chars.len() && chars[i].is_alphabetic() {
        i += 1;
    }
    let unit: String = chars[unit_start..i].iter().collect();
    if unit.is_empty() {
        return Ok((Tok::Num(n), i));
    }
    let scale = match unit.as_str() {
        "ns" => 1.0,
        "us" | "µs" => 1_000.0,
        "ms" => 1_000_000.0,
        "s" => 1_000_000_000.0,
        "m" => 60.0 * 1_000_000_000.0,
        "h" => 3_600.0 * 1_000_000_000.0,
        other => return Err(format!("unknown duration unit {other:?}")),
    };
    Ok((Tok::Dur((n * scale) as u64), i))
}

/* ------------------------------------------------------------- parser -- */

struct P {
    toks: Vec<Tok>,
    pos: usize,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// spanset_or := spanset_and ( "||" spanset_and )*
    fn spanset_or(&mut self) -> Result<Expr, String> {
        let mut left = self.spanset_and()?;
        while self.eat(&Tok::OrOr) {
            let right = self.spanset_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// spanset_and := spanset_primary ( "&&" spanset_primary )*
    fn spanset_and(&mut self) -> Result<Expr, String> {
        let mut left = self.spanset_primary()?;
        while self.eat(&Tok::AndAnd) {
            let right = self.spanset_primary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn spanset_primary(&mut self) -> Result<Expr, String> {
        if let Some(Tok::Structural(op)) = self.peek() {
            return Err(unsupported_structural(op));
        }
        if self.eat(&Tok::LParen) {
            let inner = self.spanset_or()?;
            if !self.eat(&Tok::RParen) {
                return Err("expected ')'".into());
            }
            return Ok(inner);
        }
        if !self.eat(&Tok::LBrace) {
            return match self.peek() {
                Some(t) => Err(format!("expected '{{' but found {t:?}")),
                None => Err("expected '{'".into()),
            };
        }
        let filter = if self.peek() == Some(&Tok::RBrace) {
            None
        } else {
            Some(self.cond_or()?)
        };
        if !self.eat(&Tok::RBrace) {
            return match self.peek() {
                Some(t) => Err(format!("expected '}}' but found {t:?}")),
                None => Err("expected '}'".into()),
            };
        }
        let mut aggregates = Vec::new();
        while self.eat(&Tok::Pipe) {
            aggregates.push(self.agg_filter()?);
        }
        Ok(Expr::Set(Spanset { filter, aggregates }))
    }

    /// `count() > 2`, `avg(duration) >= 100ms`
    fn agg_filter(&mut self) -> Result<AggFilter, String> {
        let name = match self.peek().cloned() {
            Some(Tok::Ident(n)) => {
                self.pos += 1;
                n
            }
            Some(t) => return Err(format!("expected an aggregate but found {t:?}")),
            None => return Err("expected an aggregate after '|'".into()),
        };
        if !self.eat(&Tok::LParen) {
            return Err(format!("expected '(' after {name}"));
        }
        let agg = if name == "count" {
            if !self.eat(&Tok::RParen) {
                return Err("count() takes no argument".into());
            }
            Agg::Count
        } else {
            let field = self.field()?;
            if !self.eat(&Tok::RParen) {
                return Err(format!("expected ')' to close {name}("));
            }
            match name.as_str() {
                "avg" => Agg::Avg(field),
                "sum" => Agg::Sum(field),
                "min" => Agg::Min(field),
                "max" => Agg::Max(field),
                "by" | "select" => {
                    return Err(format!("'{name}()' is not supported yet"));
                }
                other => return Err(format!("unknown aggregate '{other}'")),
            }
        };
        let op = match self.peek() {
            Some(Tok::Op(o)) => {
                let o = *o;
                self.pos += 1;
                o
            }
            _ => return Err("expected a comparison after the aggregate".into()),
        };
        let value = self.value()?;
        Ok(AggFilter { agg, op, value })
    }

    /// cond_or := cond_and ( "||" cond_and )*
    fn cond_or(&mut self) -> Result<Cond, String> {
        let mut left = self.cond_and()?;
        while self.eat(&Tok::OrOr) {
            let right = self.cond_and()?;
            left = Cond::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// cond_and := cond_unary ( "&&" cond_unary )*
    fn cond_and(&mut self) -> Result<Cond, String> {
        let mut left = self.cond_unary()?;
        while self.eat(&Tok::AndAnd) {
            let right = self.cond_unary()?;
            left = Cond::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn cond_unary(&mut self) -> Result<Cond, String> {
        if self.eat(&Tok::Bang) {
            return Ok(Cond::Not(Box::new(self.cond_unary()?)));
        }
        if self.eat(&Tok::LParen) {
            let inner = self.cond_or()?;
            if !self.eat(&Tok::RParen) {
                return Err("expected ')'".into());
            }
            return Ok(inner);
        }
        let field = self.field()?;
        let op = match self.peek() {
            Some(Tok::Op(o)) => {
                let o = *o;
                self.pos += 1;
                o
            }
            // A bare field is an existence check.
            _ => return Ok(Cond::Exists(field)),
        };
        let value = self.value()?;
        if matches!(op, Op::Re | Op::Nre) {
            let pat = match &value {
                Val::Str(s) => s.clone(),
                other => return Err(format!("regex operand must be a string, got {other:?}")),
            };
            let re = Regex::new(&pat).map_err(|e| format!("invalid regex {pat:?}: {e}"))?;
            return Ok(Cond::Cmp {
                field,
                op,
                value: Val::Re(Pattern(re)),
            });
        }
        Ok(Cond::Cmp { field, op, value })
    }

    fn field(&mut self) -> Result<Field, String> {
        let raw = match self.peek().cloned() {
            Some(Tok::Ident(n)) => {
                self.pos += 1;
                n
            }
            Some(t) => return Err(format!("expected a field but found {t:?}")),
            None => return Err("expected a field".into()),
        };
        Ok(parse_field(&raw))
    }

    fn value(&mut self) -> Result<Val, String> {
        match self.peek().cloned() {
            Some(Tok::Str(s)) => {
                self.pos += 1;
                Ok(Val::Str(s))
            }
            Some(Tok::Num(n)) => {
                self.pos += 1;
                Ok(Val::Num(n))
            }
            Some(Tok::Dur(d)) => {
                self.pos += 1;
                Ok(Val::Dur(d))
            }
            // Bare words in value position are enum/bool literals (`error`,
            // `server`, `true`) or an unquoted string.
            Some(Tok::Ident(w)) => {
                self.pos += 1;
                Ok(match w.as_str() {
                    "true" => Val::Bool(true),
                    "false" => Val::Bool(false),
                    _ => Val::Str(w),
                })
            }
            Some(t) => Err(format!("expected a value but found {t:?}")),
            None => Err("expected a value".into()),
        }
    }
}

fn unsupported_structural(op: &str) -> String {
    format!(
        "structural operator '{op}' is not supported yet — use '&&' to require \
         both spansets in the same trace"
    )
}

fn parse_field(raw: &str) -> Field {
    if let Some(rest) = raw.strip_prefix("span.") {
        return Field::Span(rest.to_string());
    }
    if let Some(rest) = raw.strip_prefix("resource.") {
        return Field::Resource(rest.to_string());
    }
    if let Some(rest) = raw.strip_prefix('.') {
        return Field::Unscoped(rest.to_string());
    }
    match raw {
        "name" => Field::Intrinsic(Intrinsic::Name),
        "duration" => Field::Intrinsic(Intrinsic::Duration),
        "status" => Field::Intrinsic(Intrinsic::Status),
        "kind" => Field::Intrinsic(Intrinsic::Kind),
        "rootName" => Field::Intrinsic(Intrinsic::RootName),
        "rootServiceName" => Field::Intrinsic(Intrinsic::RootServiceName),
        "traceDuration" => Field::Intrinsic(Intrinsic::TraceDuration),
        // Anything else unscoped behaves like `.foo`, which is what users
        // reaching for `http.method` inside braces expect.
        other => Field::Unscoped(other.to_string()),
    }
}

/// Parse a TraceQL query. `Ok(None)` means "empty query, no filter".
pub fn parse(input: &str) -> Result<Option<Expr>, String> {
    let toks = lex(input)?;
    if toks.is_empty() {
        return Ok(None);
    }
    let mut p = P { toks, pos: 0 };
    let expr = p.spanset_or()?;
    if p.pos != p.toks.len() {
        return match p.peek() {
            Some(Tok::Structural(op)) => Err(unsupported_structural(op)),
            // Between spansets, `>` and `<` are the child/parent structural
            // operators rather than comparisons.
            Some(Tok::Op(Op::Gt)) => Err(unsupported_structural(">")),
            Some(Tok::Op(Op::Lt)) => Err(unsupported_structural("<")),
            Some(t) => Err(format!("unexpected trailing {t:?}")),
            None => Err("unexpected trailing input".into()),
        };
    }
    Ok(Some(expr))
}

/* ---------------------------------------------------------- evaluation -- */

/// Trace-level facts that the `root*`/`traceDuration` intrinsics need.
struct Ctx {
    root_name: String,
    root_service: String,
    trace_duration_nanos: u64,
}

impl Ctx {
    fn build(spans: &[SpanRecord]) -> Ctx {
        let root = spans
            .iter()
            .find(|s| s.is_root())
            .or_else(|| spans.iter().min_by_key(|s| s.start_time_unix_nano));
        let start = spans.iter().map(|s| s.start_time_unix_nano).min();
        let end = spans.iter().map(|s| s.end_time_unix_nano).max();
        Ctx {
            root_name: root.map(|s| s.name.clone()).unwrap_or_default(),
            root_service: root.map(|s| s.service_name.clone()).unwrap_or_default(),
            trace_duration_nanos: match (start, end) {
                (Some(a), Some(b)) => b.saturating_sub(a),
                _ => 0,
            },
        }
    }
}

/// Does this trace (given all of its spans) match the query?
pub fn eval(expr: &Expr, spans: &[SpanRecord]) -> bool {
    let ctx = Ctx::build(spans);
    eval_expr(expr, spans, &ctx)
}

fn eval_expr(expr: &Expr, spans: &[SpanRecord], ctx: &Ctx) -> bool {
    match expr {
        Expr::And(a, b) => eval_expr(a, spans, ctx) && eval_expr(b, spans, ctx),
        Expr::Or(a, b) => eval_expr(a, spans, ctx) || eval_expr(b, spans, ctx),
        Expr::Set(set) => {
            let matched: Vec<&SpanRecord> = match &set.filter {
                None => spans.iter().collect(),
                Some(c) => spans.iter().filter(|s| eval_cond(c, s, ctx)).collect(),
            };
            if matched.is_empty() {
                return false;
            }
            set.aggregates.iter().all(|a| eval_agg(a, &matched, ctx))
        }
    }
}

fn eval_agg(f: &AggFilter, spans: &[&SpanRecord], ctx: &Ctx) -> bool {
    let actual = match &f.agg {
        Agg::Count => Some(spans.len() as f64),
        Agg::Avg(field) | Agg::Sum(field) | Agg::Min(field) | Agg::Max(field) => {
            let nums: Vec<f64> = spans
                .iter()
                .filter_map(|s| field_value(field, s, ctx))
                .filter_map(|v| as_number(&v))
                .collect();
            if nums.is_empty() {
                None
            } else {
                Some(match &f.agg {
                    Agg::Avg(_) => nums.iter().sum::<f64>() / nums.len() as f64,
                    Agg::Sum(_) => nums.iter().sum(),
                    Agg::Min(_) => nums.iter().copied().fold(f64::INFINITY, f64::min),
                    Agg::Max(_) => nums.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    Agg::Count => unreachable!(),
                })
            }
        }
    };
    let Some(actual) = actual else { return false };
    let Some(expected) = val_as_number(&f.value) else {
        return false;
    };
    compare_numbers(f.op, actual, expected)
}

fn eval_cond(c: &Cond, s: &SpanRecord, ctx: &Ctx) -> bool {
    match c {
        Cond::And(a, b) => eval_cond(a, s, ctx) && eval_cond(b, s, ctx),
        Cond::Or(a, b) => eval_cond(a, s, ctx) || eval_cond(b, s, ctx),
        Cond::Not(inner) => !eval_cond(inner, s, ctx),
        Cond::Exists(field) => field_value(field, s, ctx).is_some(),
        Cond::Cmp { field, op, value } => match field_value(field, s, ctx) {
            Some(actual) => matches_value(*op, value, &actual),
            None => false,
        },
    }
}

fn field_value(field: &Field, s: &SpanRecord, ctx: &Ctx) -> Option<Value> {
    match field {
        Field::Intrinsic(i) => Some(match i {
            Intrinsic::Name => Value::String(s.name.clone()),
            Intrinsic::Duration => Value::from(s.duration_nanos()),
            Intrinsic::Status => Value::from(s.status_code),
            Intrinsic::Kind => Value::String(s.kind.clone()),
            Intrinsic::RootName => Value::String(ctx.root_name.clone()),
            Intrinsic::RootServiceName => Value::String(ctx.root_service.clone()),
            Intrinsic::TraceDuration => Value::from(ctx.trace_duration_nanos),
        }),
        Field::Span(k) => json_lookup(&s.attributes, k).cloned(),
        Field::Resource(k) => json_lookup(&s.resource_attributes, k).cloned(),
        Field::Unscoped(k) => {
            // `service.name` is promoted to a column, so honour it here too —
            // it is the key users reach for most and it is not always present
            // in the resource attribute map.
            if k == "service.name" {
                return Some(Value::String(s.service_name.clone()));
            }
            json_lookup(&s.attributes, k)
                .or_else(|| json_lookup(&s.resource_attributes, k))
                .cloned()
        }
    }
}

fn as_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn val_as_number(v: &Val) -> Option<f64> {
    match v {
        Val::Num(n) => Some(*n),
        Val::Dur(d) => Some(*d as f64),
        Val::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Val::Str(s) => status_code_of(s).map(f64::from).or_else(|| s.parse().ok()),
        Val::Re(_) => None,
    }
}

/// `status = error` compares against the OTLP status code.
fn status_code_of(word: &str) -> Option<i32> {
    match word {
        "unset" => Some(0),
        "ok" => Some(1),
        "error" => Some(2),
        _ => None,
    }
}

fn compare_numbers(op: Op, actual: f64, expected: f64) -> bool {
    match op {
        Op::Eq => actual == expected,
        Op::Ne => actual != expected,
        Op::Gt => actual > expected,
        Op::Gte => actual >= expected,
        Op::Lt => actual < expected,
        Op::Lte => actual <= expected,
        Op::Re | Op::Nre => false,
    }
}

fn matches_value(op: Op, expected: &Val, actual: &Value) -> bool {
    // Regex works on the string rendering of whatever the field holds.
    if let Val::Re(p) = expected {
        let hit = p.0.is_match(&value_to_string(actual));
        return if op == Op::Nre { !hit } else { hit };
    }
    // Numeric comparison whenever both sides look numeric — this is what
    // makes `duration > 100ms` and `.http.status_code >= 500` work.
    if let (Some(a), Some(b)) = (as_number(actual), val_as_number(expected)) {
        return compare_numbers(op, a, b);
    }
    let a = value_to_string(actual);
    let b = match expected {
        Val::Str(s) => s.clone(),
        Val::Num(n) => n.to_string(),
        Val::Dur(d) => d.to_string(),
        Val::Bool(v) => v.to_string(),
        Val::Re(_) => unreachable!("handled above"),
    };
    match op {
        Op::Eq => a == b,
        Op::Ne => a != b,
        // Ordering comparisons are meaningless for non-numeric values.
        _ => false,
    }
}

/* --------------------------------------------------------------- tests -- */

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const T0: u64 = 1_000_000_000;
    const MS: u64 = 1_000_000;

    #[allow(clippy::too_many_arguments)]
    fn span(
        name: &str,
        service: &str,
        start_ms: u64,
        dur_ms: u64,
        status: i32,
        kind: &str,
        attrs: Value,
    ) -> SpanRecord {
        SpanRecord {
            trace_id: "t1".into(),
            span_id: name.into(),
            parent_span_id: String::new(),
            name: name.into(),
            service_name: service.into(),
            kind: kind.into(),
            start_time_unix_nano: T0 + start_ms * MS,
            end_time_unix_nano: T0 + (start_ms + dur_ms) * MS,
            status_code: status,
            status_message: String::new(),
            attributes: attrs,
            resource_attributes: json!({"service.name": service, "host.name": "a1"}),
            events: json!([]),
            links: json!([]),
            scope_name: String::new(),
            scope_version: String::new(),
        }
    }

    /// A two-span trace spanning 400ms:
    ///   root  `GET /`  gateway  0..400ms  ok     (400ms)
    ///   child `charge` payments 50..350ms error  (300ms)
    fn trace() -> Vec<SpanRecord> {
        let root = span(
            "GET /",
            "gateway",
            0,
            400,
            0,
            "server",
            json!({"http.method": "GET", "http.status_code": 200}),
        );
        let mut child = span(
            "charge",
            "payments",
            50,
            300,
            2,
            "client",
            json!({"http.method": "POST", "http.status_code": 502}),
        );
        child.parent_span_id = "GET /".into();
        vec![root, child]
    }

    fn matches(q: &str) -> bool {
        eval(&parse(q).unwrap().unwrap(), &trace())
    }

    #[test]
    fn attribute_scopes() {
        assert!(matches(r#"{ span.http.method = "POST" }"#));
        assert!(matches(r#"{ resource.service.name = "payments" }"#));
        assert!(matches(r#"{ .http.method = "GET" }"#));
        // A scope that does not hold the key must not fall back.
        assert!(!matches(r#"{ resource.http.method = "GET" }"#));
        assert!(!matches(r#"{ span.http.method = "DELETE" }"#));
    }

    #[test]
    fn intrinsics() {
        assert!(matches(r#"{ name = "charge" }"#));
        assert!(matches("{ status = error }"));
        assert!(matches("{ kind = client }"));
        assert!(matches(r#"{ rootName = "GET /" }"#));
        assert!(matches(r#"{ rootServiceName = "gateway" }"#));
        assert!(!matches("{ status = ok }"));
        assert!(matches("{ status != ok }"));
    }

    #[test]
    fn durations() {
        assert!(matches("{ duration > 100ms }"));
        assert!(matches("{ duration >= 400ms }")); // the root
        assert!(matches("{ duration < 350ms }")); // the child
        assert!(!matches("{ duration > 1s }"));
        assert!(!matches("{ duration < 10ms }"));
        assert!(matches("{ traceDuration = 400ms }"));
        // Unit handling: 1.5s and 1500ms are the same bound.
        assert!(!matches("{ duration > 1.5s }"));
        assert!(!matches("{ duration > 1500ms }"));
        // …and the small units line up too.
        assert!(matches("{ duration = 400000000ns }"));
        assert!(matches("{ duration = 400000us }"));
    }

    #[test]
    fn numeric_and_regex_operators() {
        assert!(matches("{ .http.status_code >= 500 }"));
        assert!(!matches("{ .http.status_code > 502 }"));
        assert!(matches(r#"{ name =~ "^GET" }"#));
        assert!(matches(r#"{ name !~ "^POST" }"#));
        assert!(!matches(r#"{ name =~ "^nope" }"#));
    }

    #[test]
    fn boolean_logic_within_a_spanset() {
        // Both conditions must hold on the *same* span.
        assert!(matches(r#"{ name = "charge" && status = error }"#));
        assert!(!matches(r#"{ name = "GET /" && status = error }"#));
        assert!(matches(r#"{ name = "GET /" || status = error }"#));
        assert!(matches(r#"{ !(name = "charge") }"#));
        assert!(matches(
            r#"{ (name = "charge" || name = "GET /") && duration > 10ms }"#
        ));
    }

    #[test]
    fn spanset_operators_span_the_whole_trace() {
        // Separate spansets may be satisfied by different spans.
        assert!(matches(r#"{ name = "GET /" } && { status = error }"#));
        assert!(matches(r#"{ name = "nope" } || { status = error }"#));
        assert!(!matches(r#"{ name = "nope" } && { status = error }"#));
        assert!(!matches(r#"{ name = "nope" } || { name = "nada" }"#));
    }

    #[test]
    fn existence_and_empty_selector() {
        assert!(matches("{ span.http.method }"));
        assert!(!matches("{ span.nope }"));
        assert!(matches("{}"));
        assert!(!eval(&parse("{}").unwrap().unwrap(), &[]));
    }

    #[test]
    fn aggregates() {
        assert!(matches("{} | count() > 1"));
        assert!(!matches("{} | count() > 2"));
        assert!(matches("{ status = error } | count() = 1"));
        // root 400ms, child 300ms.
        assert!(matches("{} | max(duration) = 400ms"));
        assert!(matches("{} | min(duration) = 300ms"));
        assert!(matches("{} | avg(duration) = 350ms"));
        assert!(matches("{} | sum(duration) = 700ms"));
        // The aggregate applies to the selected spans, not the whole trace.
        assert!(matches("{ status = error } | max(duration) = 300ms"));
        // Aggregates chain: both must hold.
        assert!(matches("{} | count() = 2 | max(duration) >= 400ms"));
        assert!(!matches("{} | count() = 2 | max(duration) > 1s"));
    }

    #[test]
    fn parse_errors_and_empty() {
        assert!(parse("").unwrap().is_none());
        assert!(parse("   ").unwrap().is_none());
        assert!(parse("{ name = ").is_err());
        assert!(parse("{ name = \"x\"").is_err());
        assert!(parse("name = \"x\" }").is_err());
        assert!(parse("{ name = \"unterminated }").is_err());
        assert!(parse(r#"{ name =~ "((" }"#).is_err());
        assert!(parse("{ duration > 5years }").is_err());
        assert!(parse("{} | nope() > 1").is_err());
        assert!(parse("{} | count(duration) > 1").is_err());
    }

    #[test]
    fn unsupported_syntax_is_named_not_ignored() {
        for q in [
            r#"{ name = "a" } >> { name = "b" }"#,
            r#"{ name = "a" } > { name = "b" }"#,
            r#"{ name = "a" } ~ { name = "b" }"#,
        ] {
            let err = parse(q).unwrap_err();
            assert!(
                err.contains("not supported yet"),
                "expected a clear unsupported-syntax error for {q:?}, got {err:?}"
            );
        }
        assert!(parse("{} | by(name) > 1")
            .unwrap_err()
            .contains("not supported"));
    }
}
