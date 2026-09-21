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
//!
//! The pipeline is lex -> parse -> eval, over whatever implements [`Doc`]:
//!
//! - `lex`: query text to tokens
//! - `ast`: the shape a parsed query takes
//! - `parse`: tokens to AST
//! - `doc`: what a query can be evaluated against
//! - `eval`: does this one record match

mod ast;
mod doc;
mod eval;
mod lex;
mod parse;
#[cfg(test)]
mod tests;

pub use ast::{Bound, Clause, Occur, Query};
pub use doc::Doc;
pub use eval::eval;
pub use parse::parse;
