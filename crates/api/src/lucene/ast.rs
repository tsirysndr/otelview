//! The shape a parsed query takes.
//!
//! Deliberately close to Lucene's own vocabulary — `Occur`, `Clause`, the
//! range `Bound` — so anyone who knows the syntax can read the tree.

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
