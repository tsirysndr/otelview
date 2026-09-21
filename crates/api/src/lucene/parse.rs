//! Tokens to AST.
//!
//! Recursive descent over a flat token list. Lucene has no operator
//! precedence to speak of: a query is a list of clauses, each `Should`,
//! `Must` or `MustNot`, and `AND`/`OR`/`NOT` are sugar over that.

use super::ast::{Bound, Clause, Occur, Query};
use super::lex::{describe, lex, Tok, MAX_EDITS};

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
