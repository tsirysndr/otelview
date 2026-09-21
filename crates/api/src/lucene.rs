//! Lucene query syntax, for logs and traces.
//!
//! Like [`crate::kql`] and [`crate::traceql`] this parses to an AST and
//! evaluates as a Rust predicate rather than lowering to SQL, so it works
//! across every storage backend.
//!
//! Supported:
//! - `term`, `"a phrase"`, `field:term`, `field:"a phrase"`
//! - wildcards `te?t` (one character) and `te*t` (any run)
//! - fuzzy `roam~` (edit distance 2) and `roam~1`
//! - proximity `"jakarta apache"~10`
//! - ranges `[1 TO 5]` inclusive, `{1 TO 5}` exclusive, mixed `[1 TO 5}`,
//!   and `*` for an open end
//! - `AND` `OR` `NOT`, their `&&` `||` `!` spellings, and `+must` `-must_not`
//! - grouping `(a OR b) AND c`, and field grouping `field:(a OR b)`
//! - escaping with a backslash: `path:\/api\/traces`
//!
//! Boosts (`term^4`) parse and are then ignored: this decides whether a
//! record matches, and there is no ranking for a weight to affect.
//!
//! As in Lucene the default operator is OR, so `a b` matches either.

use otelview_model::{severity_level, LogRecord, SpanRecord};
use serde_json::Value;

use crate::kql::{json_lookup, value_to_string};

/* ------------------------------------------------------------ documents -- */

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

/* ------------------------------------------------------------------ AST -- */

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Occur {
    /// Optional: at least one must match when there are no `Must` clauses.
    Should,
    Must,
    MustNot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Clause {
    pub occur: Occur,
    pub query: Query,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Bound {
    /// `*` — this end is open.
    Open,
    Inclusive(String),
    Exclusive(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    Bool(Vec<Clause>),
    /// A single word, possibly with wildcards.
    Term {
        field: Option<String>,
        text: String,
    },
    /// `word~` / `word~1`: matches within an edit distance.
    Fuzzy {
        field: Option<String>,
        text: String,
        distance: u32,
    },
    /// A quoted phrase. `slop` > 0 allows the words to be that far apart.
    Phrase {
        field: Option<String>,
        words: Vec<String>,
        slop: usize,
    },
    Range {
        field: Option<String>,
        lo: Bound,
        hi: Bound,
    },
}

/* ---------------------------------------------------------------- lexer -- */

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    LParen,
    RParen,
    And,
    Or,
    Not,
    Plus,
    Minus,
    Colon,
    /// A bare word. Wildcards stay in the text; escapes are already resolved.
    Word(String),
    /// A quoted phrase, escapes resolved.
    Quoted(String),
    /// `~n` — fuzzy distance or phrase slop. `~` alone is `None`.
    Tilde(Option<u32>),
    /// `^n`, kept only so it can be skipped.
    Boost,
    RangeOpen(bool),  // true = inclusive `[`
    RangeClose(bool), // true = inclusive `]`
    To,
}

/// Lucene's fuzzy automaton is built for at most two edits, and a larger
/// distance on short words degenerates into matching everything.
const MAX_EDITS: u32 = 2;

fn is_word_char(c: char) -> bool {
    !c.is_whitespace() && !"()[]{}:\"^~+-".contains(c)
}

fn lex(input: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '[' => {
                out.push(Tok::RangeOpen(true));
                i += 1;
            }
            ']' => {
                out.push(Tok::RangeClose(true));
                i += 1;
            }
            '{' => {
                out.push(Tok::RangeOpen(false));
                i += 1;
            }
            '}' => {
                out.push(Tok::RangeClose(false));
                i += 1;
            }
            ':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            '+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            // A minus only negates when it starts a clause; mid-word it is
            // part of the word (service-name), which the word branch handles.
            '-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '^' => {
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                out.push(Tok::Boost);
            }
            '~' => {
                i += 1;
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let n: String = chars[start..i].iter().collect();
                out.push(Tok::Tilde(if n.is_empty() {
                    None
                } else if n.contains('.') {
                    // The old 0.0-1.0 similarity spelling. Lucene dropped it,
                    // but queries in the wild still carry it, so map it onto
                    // an edit distance rather than failing: the less similar
                    // the request, the more edits it is willing to make.
                    n.parse::<f64>()
                        .ok()
                        .map(|f| if f >= 0.8 { 1 } else { MAX_EDITS })
                } else {
                    // A plain integer is the edit distance itself, so `~0`
                    // means exactly this word. Lucene's automaton tops out
                    // at two edits and so do we.
                    n.parse::<u32>().ok().map(|d| d.min(MAX_EDITS))
                }));
            }
            '"' => {
                let (s, next) = read_quoted(&chars, i)?;
                out.push(Tok::Quoted(s));
                i = next;
            }
            _ => {
                let mut word = String::new();
                while i < chars.len() {
                    let ch = chars[i];
                    if ch == '\\' && i + 1 < chars.len() {
                        word.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    // A dash inside a word is part of it, not an operator.
                    if ch == '-' && !word.is_empty() {
                        word.push(ch);
                        i += 1;
                        continue;
                    }
                    if !is_word_char(ch) {
                        break;
                    }
                    word.push(ch);
                    i += 1;
                }
                if word.is_empty() {
                    return Err(format!("unexpected character {c:?}"));
                }
                out.push(match word.as_str() {
                    "AND" => Tok::And,
                    "OR" => Tok::Or,
                    "NOT" => Tok::Not,
                    "TO" => Tok::To,
                    _ => Tok::Word(word),
                });
            }
        }
    }
    Ok(out)
}

fn read_quoted(chars: &[char], start: usize) -> Result<(String, usize), String> {
    let mut s = String::new();
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                s.push(chars[i + 1]);
                i += 2;
            }
            '"' => return Ok((s, i + 1)),
            c => {
                s.push(c);
                i += 1;
            }
        }
    }
    Err("unterminated quoted string".into())
}

/* --------------------------------------------------------------- parser -- */

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

    /// A run of clauses joined by the default OR, with AND/NOT binding the
    /// neighbouring clauses as Lucene does.
    fn query(&mut self) -> Result<Query, String> {
        let mut clauses: Vec<Clause> = Vec::new();
        while let Some(tok) = self.peek() {
            if *tok == Tok::RParen {
                break;
            }
            match tok {
                Tok::And => {
                    self.pos += 1;
                    // `a AND b` promotes both sides.
                    if let Some(last) = clauses.last_mut() {
                        last.occur = Occur::Must;
                    }
                    let q = self.clause_body()?;
                    clauses.push(Clause {
                        occur: Occur::Must,
                        query: q,
                    });
                }
                Tok::Or => {
                    self.pos += 1;
                    let q = self.clause_body()?;
                    clauses.push(Clause {
                        occur: Occur::Should,
                        query: q,
                    });
                }
                Tok::Not => {
                    self.pos += 1;
                    let q = self.clause_body()?;
                    clauses.push(Clause {
                        occur: Occur::MustNot,
                        query: q,
                    });
                }
                Tok::Plus => {
                    self.pos += 1;
                    let q = self.clause_body()?;
                    clauses.push(Clause {
                        occur: Occur::Must,
                        query: q,
                    });
                }
                Tok::Minus => {
                    self.pos += 1;
                    let q = self.clause_body()?;
                    clauses.push(Clause {
                        occur: Occur::MustNot,
                        query: q,
                    });
                }
                _ => {
                    let q = self.clause_body()?;
                    clauses.push(Clause {
                        occur: Occur::Should,
                        query: q,
                    });
                }
            }
        }
        if clauses.is_empty() {
            return Err(if matches!(self.peek(), Some(Tok::RParen)) {
                "empty group '()'".into()
            } else {
                "empty query".into()
            });
        }
        if clauses.len() == 1 && clauses[0].occur == Occur::Should {
            return Ok(clauses.pop().unwrap().query);
        }
        Ok(Query::Bool(clauses))
    }

    /// One clause: an optional `field:`, then a group, range, phrase or term.
    fn clause_body(&mut self) -> Result<Query, String> {
        let field = self.try_field();
        if self.eat(&Tok::LParen) {
            // A group that runs off the end of the input is a missing ')',
            // which says more than whatever the inner parse tripped over.
            let inner = self.query().map_err(|e| {
                if self.peek().is_none() {
                    "expected ')'".into()
                } else {
                    e
                }
            })?;
            if !self.eat(&Tok::RParen) {
                return Err("expected ')'".into());
            }
            self.skip_boost();
            // `field:(a OR b)` pushes the field down into the group.
            return Ok(match field {
                Some(f) => with_field(inner, &f),
                None => inner,
            });
        }
        if let Some(Tok::RangeOpen(lo_inc)) = self.peek().cloned() {
            self.pos += 1;
            return self.range(field, lo_inc);
        }
        match self.peek().cloned() {
            Some(Tok::Quoted(text)) => {
                self.pos += 1;
                let slop = self.try_tilde().unwrap_or(0) as usize;
                self.skip_boost();
                Ok(Query::Phrase {
                    field,
                    words: text.split_whitespace().map(str::to_string).collect(),
                    slop,
                })
            }
            Some(Tok::Word(text)) => {
                self.pos += 1;
                let fuzzy = self.try_tilde();
                self.skip_boost();
                Ok(match fuzzy {
                    // `~` with no number is Lucene's default of 2.
                    Some(d) => Query::Fuzzy {
                        field,
                        text,
                        distance: d,
                    },
                    None => Query::Term { field, text },
                })
            }
            Some(t) => Err(format!("unexpected {t:?}")),
            None => Err("unexpected end of query".into()),
        }
    }

    /// `field:` — only when a colon actually follows the word.
    fn try_field(&mut self) -> Option<String> {
        if let (Some(Tok::Word(w)), Some(Tok::Colon)) =
            (self.toks.get(self.pos), self.toks.get(self.pos + 1))
        {
            let name = w.clone();
            self.pos += 2;
            return Some(name);
        }
        None
    }

    fn try_tilde(&mut self) -> Option<u32> {
        if let Some(Tok::Tilde(n)) = self.peek().cloned() {
            self.pos += 1;
            return Some(n.unwrap_or(MAX_EDITS));
        }
        None
    }

    fn skip_boost(&mut self) {
        while self.eat(&Tok::Boost) {}
    }

    fn range(&mut self, field: Option<String>, lo_inc: bool) -> Result<Query, String> {
        let lo = self.range_end()?;
        if !self.eat(&Tok::To) {
            return Err("expected TO in a range".into());
        }
        let hi = self.range_end()?;
        let hi_inc = match self.peek().cloned() {
            Some(Tok::RangeClose(inc)) => {
                self.pos += 1;
                inc
            }
            _ => return Err("unterminated range".into()),
        };
        self.skip_boost();
        Ok(Query::Range {
            field,
            lo: bound(lo, lo_inc),
            hi: bound(hi, hi_inc),
        })
    }

    fn range_end(&mut self) -> Result<String, String> {
        match self.peek().cloned() {
            Some(Tok::Word(w)) => {
                self.pos += 1;
                Ok(w)
            }
            Some(Tok::Quoted(w)) => {
                self.pos += 1;
                Ok(w)
            }
            Some(t) => Err(format!("expected a range bound but found {t:?}")),
            None => Err("unterminated range".into()),
        }
    }
}

fn bound(text: String, inclusive: bool) -> Bound {
    if text == "*" {
        Bound::Open
    } else if inclusive {
        Bound::Inclusive(text)
    } else {
        Bound::Exclusive(text)
    }
}

/// Push a field name into every leaf that does not already carry one, so
/// `status:(404 OR 500)` means what it looks like.
fn with_field(q: Query, f: &str) -> Query {
    let set = |field: Option<String>| Some(field.unwrap_or_else(|| f.to_string()));
    match q {
        Query::Bool(cs) => Query::Bool(
            cs.into_iter()
                .map(|c| Clause {
                    occur: c.occur,
                    query: with_field(c.query, f),
                })
                .collect(),
        ),
        Query::Term { field, text } => Query::Term {
            field: set(field),
            text,
        },
        Query::Fuzzy {
            field,
            text,
            distance,
        } => Query::Fuzzy {
            field: set(field),
            text,
            distance,
        },
        Query::Phrase { field, words, slop } => Query::Phrase {
            field: set(field),
            words,
            slop,
        },
        Query::Range { field, lo, hi } => Query::Range {
            field: set(field),
            lo,
            hi,
        },
    }
}

/// How a token reads back to whoever typed it — parse errors quote the
/// query's own text rather than the lexer's variant names.
fn describe(t: &Tok) -> String {
    match t {
        Tok::LParen => "'('".into(),
        Tok::RParen => "')'".into(),
        Tok::And => "'AND'".into(),
        Tok::Or => "'OR'".into(),
        Tok::Not => "'NOT'".into(),
        Tok::Plus => "'+'".into(),
        Tok::Minus => "'-'".into(),
        Tok::Colon => "':'".into(),
        Tok::Word(w) => format!("'{w}'"),
        Tok::Quoted(q) => format!("'\"{q}\"'"),
        Tok::Tilde(_) => "'~'".into(),
        Tok::Boost => "'^'".into(),
        Tok::RangeOpen(inc) => if *inc { "'['" } else { "'{'" }.into(),
        Tok::RangeClose(inc) => if *inc { "']'" } else { "'}'" }.into(),
        Tok::To => "'TO'".into(),
    }
}

/// Parse a Lucene query. `Ok(None)` means "empty query, no filter".
pub fn parse(input: &str) -> Result<Option<Query>, String> {
    let toks = lex(input)?;
    if toks.is_empty() {
        return Ok(None);
    }
    let mut p = P { toks, pos: 0 };
    let q = p.query()?;
    if p.pos != p.toks.len() {
        return match p.peek() {
            Some(t) => Err(format!("unexpected {}", describe(t))),
            None => Err("unexpected trailing input".into()),
        };
    }
    Ok(Some(q))
}

/* ----------------------------------------------------------- evaluation -- */

pub fn eval<D: Doc>(q: &Query, doc: &D) -> bool {
    match q {
        Query::Bool(clauses) => {
            let has_must = clauses.iter().any(|c| c.occur == Occur::Must);
            let mut any_should = false;
            let mut saw_should = false;
            for c in clauses {
                let hit = eval(&c.query, doc);
                match c.occur {
                    Occur::Must => {
                        if !hit {
                            return false;
                        }
                    }
                    Occur::MustNot => {
                        if hit {
                            return false;
                        }
                    }
                    Occur::Should => {
                        saw_should = true;
                        any_should |= hit;
                    }
                }
            }
            // Lucene: optional clauses only become a requirement when there
            // is nothing mandatory to satisfy.
            if has_must || !saw_should {
                true
            } else {
                any_should
            }
        }
        Query::Term { field, text } => haystacks(field.as_deref(), doc)
            .iter()
            .any(|h| wildcard(text, h)),
        Query::Fuzzy {
            field,
            text,
            distance,
        } => haystacks(field.as_deref(), doc).iter().any(|h| {
            h.split_whitespace().any(|w| {
                edit_distance(&w.to_lowercase(), &text.to_lowercase()) <= *distance as usize
            })
        }),
        Query::Phrase { field, words, slop } => haystacks(field.as_deref(), doc)
            .iter()
            .any(|h| phrase_match(h, words, *slop)),
        Query::Range { field, lo, hi } => haystacks(field.as_deref(), doc)
            .iter()
            .any(|h| in_range(h, lo, hi)),
    }
}

/// The strings a clause is tested against: one field's values, or the whole
/// record when the clause named no field.
fn haystacks<D: Doc>(field: Option<&str>, doc: &D) -> Vec<String> {
    match field {
        Some(f) => doc.field(f).iter().map(value_to_string).collect(),
        None => vec![doc.text()],
    }
}

/// Case-insensitive glob with Lucene's `?` (one character) and `*` (any run).
/// A term with no wildcard matches as a substring, which is what a filter
/// over unanalyzed JSON needs to be useful.
fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    if !pattern.contains(['*', '?']) {
        return text.to_lowercase().contains(&pattern.to_lowercase());
    }
    // Anchored glob, iterative with backtracking on `*`.
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Words in order, each within `slop` positions of the next. `slop` 0 means
/// strictly adjacent.
fn phrase_match(text: &str, words: &[String], slop: usize) -> bool {
    if words.is_empty() {
        return true;
    }
    let toks: Vec<String> = text.to_lowercase().split_whitespace().map(clean).collect();
    let wanted: Vec<String> = words.iter().map(|w| clean(&w.to_lowercase())).collect();
    'start: for i in 0..toks.len() {
        if !wildcard(&wanted[0], &toks[i]) {
            continue;
        }
        let mut at = i;
        for w in &wanted[1..] {
            let limit = (at + 1 + slop).min(toks.len().saturating_sub(1));
            let mut found = None;
            for (j, tok) in toks.iter().enumerate().take(limit + 1).skip(at + 1) {
                if wildcard(w, tok) {
                    found = Some(j);
                    break;
                }
            }
            match found {
                Some(j) => at = j,
                None => continue 'start,
            }
        }
        return true;
    }
    false
}

/// Trim the punctuation that JSON rendering leaves around values, so a
/// phrase can match inside a serialized attribute map.
fn clean(s: &str) -> String {
    s.trim_matches(|c: char| "\",{}[]:".contains(c)).to_string()
}

fn in_range(text: &str, lo: &Bound, hi: &Bound) -> bool {
    let num = text.trim().parse::<f64>().ok();
    let cmp_lo = match lo {
        Bound::Open => true,
        Bound::Inclusive(v) => compare(text, v, num).is_ge(),
        Bound::Exclusive(v) => compare(text, v, num).is_gt(),
    };
    let cmp_hi = match hi {
        Bound::Open => true,
        Bound::Inclusive(v) => compare(text, v, num).is_le(),
        Bound::Exclusive(v) => compare(text, v, num).is_lt(),
    };
    cmp_lo && cmp_hi
}

/// Numeric when both sides are numbers, lexicographic otherwise — matching
/// how Lucene ranges read for dates and versions.
fn compare(text: &str, bound: &str, text_num: Option<f64>) -> std::cmp::Ordering {
    match (text_num, bound.trim().parse::<f64>().ok()) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
        _ => text.cmp(bound),
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/* --------------------------------------------------------------- tests -- */

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

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("same", "same"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn wildcard_anchoring() {
        // Globs are anchored; plain terms are substrings.
        assert!(wildcard("po*", "post"));
        assert!(!wildcard("o*", "post"));
        assert!(wildcard("*os*", "post"));
        assert!(wildcard("p?st", "post"));
        assert!(!wildcard("p?st", "poost"));
        assert!(wildcard("ost", "post"));
    }
}
