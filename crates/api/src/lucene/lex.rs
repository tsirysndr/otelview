//! Turning query text into tokens.
//!
//! Lucene's punctuation is context-sensitive — `-` negates at the start of
//! a clause but is an ordinary character inside `o-42`, and `~` is a fuzzy
//! distance after a word but a proximity slop after a phrase — so the lexer
//! stays deliberately dumb and leaves those calls to the parser.

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Tok {
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
pub(super) const MAX_EDITS: u32 = 2;

fn is_word_char(c: char) -> bool {
    !c.is_whitespace() && !"()[]{}:\"^~+-".contains(c)
}

pub(super) fn lex(input: &str) -> Result<Vec<Tok>, String> {
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

/// How a token reads back to whoever typed it — parse errors quote the
/// query's own text rather than the lexer's variant names.
pub(super) fn describe(t: &Tok) -> String {
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
