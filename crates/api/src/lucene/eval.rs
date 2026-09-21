//! Deciding whether one record matches a parsed query.
//!
//! No index and no scoring: every predicate runs against the record in
//! hand, which is what keeps the language working over all four storage
//! backends without any of them knowing Lucene exists.

use crate::kql::value_to_string;

use super::ast::{Bound, Occur, Query};
use super::doc::Doc;

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

#[cfg(test)]
mod tests {
    use super::*;

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
