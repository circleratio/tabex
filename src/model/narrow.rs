//! Narrowing condition (requirement 6.1): plain substring plus an optional migemo pattern.
//!
//! rustmigemo returns patterns in its default regex syntax, which only uses literals,
//! `\` escapes, `[...]` classes, `(...)` groups and `|`. That subset is parsed here into a
//! tree of alternatives so no regex engine is needed.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    #[error("unbalanced brackets")]
    Unbalanced,
    #[error("trailing escape")]
    TrailingEscape,
    #[error("empty character class")]
    EmptyClass,
}

#[derive(Debug)]
enum Node {
    Char(char),
    Class(Vec<char>),
    Group(Alt),
}

type Seq = Vec<Node>;

/// Alternatives, indexed by the character a branch must start with.
#[derive(Debug, Default)]
struct Alt {
    branches: Vec<Seq>,
    by_first: HashMap<char, Vec<u32>>,
    /// Branches that do not start with a fixed character (group first, or empty).
    other: Vec<u32>,
}

impl Alt {
    fn new(branches: Vec<Seq>) -> Alt {
        let mut alt = Alt {
            branches,
            ..Default::default()
        };
        for (i, seq) in alt.branches.iter().enumerate() {
            let i = i as u32;
            match seq.first() {
                Some(Node::Char(c)) => alt.by_first.entry(*c).or_default().push(i),
                Some(Node::Class(cs)) => {
                    for c in cs {
                        let v = alt.by_first.entry(*c).or_default();
                        if v.last() != Some(&i) {
                            v.push(i);
                        }
                    }
                }
                Some(Node::Group(_)) | None => alt.other.push(i),
            }
        }
        alt
    }

    /// Positions where a match of `self` starting at `at` can end.
    fn ends(&self, s: &[char], at: usize, out: &mut Vec<usize>) {
        let keyed = s.get(at).and_then(|c| self.by_first.get(c));
        for &b in keyed.into_iter().flatten().chain(&self.other) {
            seq_ends(&self.branches[b as usize], s, at, out);
        }
    }
}

fn seq_ends(seq: &[Node], s: &[char], at: usize, out: &mut Vec<usize>) {
    let mut cur = vec![at];
    for node in seq {
        let mut next = Vec::new();
        for &p in &cur {
            match node {
                Node::Char(c) => {
                    if s.get(p) == Some(c) {
                        next.push(p + 1);
                    }
                }
                Node::Class(cs) => {
                    if s.get(p).is_some_and(|c| cs.contains(c)) {
                        next.push(p + 1);
                    }
                }
                Node::Group(alt) => alt.ends(s, p, &mut next),
            }
        }
        next.sort_unstable();
        next.dedup();
        if next.is_empty() {
            return;
        }
        cur = next;
    }
    out.extend(cur);
}

/// Lowercase one character the same way for patterns and names.
pub fn fold(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Parsed migemo pattern.
#[derive(Debug)]
pub struct Pattern {
    root: Alt,
}

impl Pattern {
    /// Parse a pattern in rustmigemo's default syntax; literals are lowercased.
    pub fn parse(src: &str) -> Result<Pattern, PatternError> {
        let mut it = src.chars().peekable();
        let root = parse_alt(&mut it)?;
        if it.next().is_some() {
            // Only a stray ')' stops the top-level alternative early.
            return Err(PatternError::Unbalanced);
        }
        Ok(Pattern { root })
    }

    /// Whether the pattern occurs anywhere in `lower_name` (already folded with [`fold`]).
    pub fn is_match(&self, lower_name: &[char]) -> bool {
        let mut out = Vec::new();
        (0..lower_name.len()).any(|i| {
            out.clear();
            self.root.ends(lower_name, i, &mut out);
            !out.is_empty()
        })
    }
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

fn parse_alt(it: &mut Chars) -> Result<Alt, PatternError> {
    let mut branches = vec![parse_seq(it)?];
    while it.next_if_eq(&'|').is_some() {
        branches.push(parse_seq(it)?);
    }
    Ok(Alt::new(branches))
}

fn parse_seq(it: &mut Chars) -> Result<Seq, PatternError> {
    let mut seq = Vec::new();
    while let Some(&c) = it.peek() {
        match c {
            '|' | ')' => break,
            '(' => {
                it.next();
                let alt = parse_alt(it)?;
                if it.next() != Some(')') {
                    return Err(PatternError::Unbalanced);
                }
                seq.push(Node::Group(alt));
            }
            '[' => {
                it.next();
                let mut cs = Vec::new();
                loop {
                    match it.next() {
                        None => return Err(PatternError::Unbalanced),
                        Some(']') => break,
                        Some('\\') => cs.push(fold(it.next().ok_or(PatternError::TrailingEscape)?)),
                        Some(c) => cs.push(fold(c)),
                    }
                }
                if cs.is_empty() {
                    return Err(PatternError::EmptyClass);
                }
                seq.push(Node::Class(cs));
            }
            '\\' => {
                it.next();
                seq.push(Node::Char(fold(it.next().ok_or(PatternError::TrailingEscape)?)));
            }
            ']' => return Err(PatternError::Unbalanced),
            c => {
                it.next();
                seq.push(Node::Char(fold(c)));
            }
        }
    }
    Ok(seq)
}

/// Narrowing condition of one tab, shared by its listing and pending listing.
#[derive(Debug)]
pub struct Query {
    /// Lowercase narrowing text.
    text: String,
    pattern: Option<Pattern>,
}

impl Query {
    pub fn new(text: &str, pattern: Option<Pattern>) -> Query {
        Query {
            text: text.to_lowercase(),
            pattern,
        }
    }

    /// Substring match only.
    pub fn plain(text: &str) -> Query {
        Query::new(text, None)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn uses_migemo(&self) -> bool {
        self.pattern.is_some()
    }

    /// Case-insensitive substring of the whole name, or a match of the migemo pattern.
    pub fn matches(&self, name: &[u16]) -> bool {
        if super::listing::name_matches(name, &self.text) {
            return true;
        }
        self.pattern.as_ref().is_some_and(|p| {
            let chars: Vec<char> = String::from_utf16_lossy(name).chars().map(fold).collect();
            p.is_match(&chars)
        })
    }
}

/// The pattern is derived from the text, so its presence is all that matters.
impl PartialEq for Query {
    fn eq(&self, other: &Query) -> bool {
        self.text == other.text && self.uses_migemo() == other.uses_migemo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What rustmigemo (with migemo-compact-dict v0.2) returns for "kaisha".
    const KAISHA: &str = "(kaisha|かいしゃ|カイシャ|介錯|会社|會社|膾炙|解[釈釋]|開写像|ｋａｉｓｈａ|ｶｲｼｬ)";

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn hit(pattern: &str, name: &str) -> bool {
        let chars: Vec<char> = name.chars().map(fold).collect();
        Pattern::parse(pattern).unwrap().is_match(&chars)
    }

    #[test]
    fn literals_and_escapes() {
        assert!(hit("abc", "xxabcxx"));
        assert!(!hit("abc", "ab"));
        assert!(hit(r"a\.b", "a.b"));
        assert!(!hit(r"a\.b", "axb"));
        assert!(hit(r#"\(concat "and\\057or"\)"#, r#"(concat "and\057or")"#));
    }

    #[test]
    fn classes() {
        assert!(hit("解[釈釋]", "解釈.txt"));
        assert!(hit("解[釈釋]", "解釋"));
        assert!(!hit("解[釈釋]", "解説"));
        assert!(hit(r"x[\]\.]y", "x]y"));
        assert!(hit(r"x[\]\.]y", "x.y"));
        assert!(!hit(r"x[\]\.]y", "x-y"));
    }

    #[test]
    fn groups_nested_and_concatenated() {
        let p = "a(b|c(d|e))f";
        assert!(hit(p, "abf"));
        assert!(hit(p, "acef"));
        assert!(!hit(p, "acf"));
        let two = "(kensaku|けんさく)(kekka|結[果核])";
        assert!(hit(two, "けんさく結果.txt"));
        assert!(hit(two, "kensakukekka"));
        assert!(!hit(two, "けんさく.txt"));
    }

    #[test]
    fn case_insensitive() {
        assert!(hit("A(I|Cアダプタ)", "acアダプタ"));
        assert!(hit("kaisha", "KAISHA.md"));
        assert!(hit("ＡＢ", "ａｂ"));
    }

    #[test]
    fn kaisha() {
        for name in [
            "会社案内.pdf",
            "かいしゃ.txt",
            "カイシャ.txt",
            "kaisha.log",
            "KAISHA.md",
            "ｶｲｼｬ",
        ] {
            assert!(hit(KAISHA, name), "{name}");
        }
        assert!(!hit(KAISHA, "会議.txt"));
    }

    #[test]
    fn first_character_index_keeps_every_branch() {
        // Several branches sharing a first character, one starting with a class, one with a group.
        let p = "(ab|ac|[ax]d|(y|z)e|)";
        for name in ["ab", "ac", "ad", "xd", "ye", "ze"] {
            assert!(hit(p, name), "{name}");
        }
        // The empty branch matches anything.
        assert!(hit(p, "q"));
        assert!(hit("(ab|ac|[ax]d)", "zzac"));
        assert!(!hit("(ab|ac|[ax]d)", "ae"));
    }

    #[test]
    fn errors() {
        assert_eq!(Pattern::parse("(ab").unwrap_err(), PatternError::Unbalanced);
        assert_eq!(Pattern::parse("ab)").unwrap_err(), PatternError::Unbalanced);
        assert_eq!(Pattern::parse("[ab").unwrap_err(), PatternError::Unbalanced);
        assert_eq!(Pattern::parse("ab]").unwrap_err(), PatternError::Unbalanced);
        assert_eq!(Pattern::parse(r"ab\").unwrap_err(), PatternError::TrailingEscape);
        assert_eq!(Pattern::parse("a[]").unwrap_err(), PatternError::EmptyClass);
        assert!(Pattern::parse("").is_ok());
    }

    #[test]
    fn query_is_substring_or_pattern() {
        let q = Query::new("kaisha", Some(Pattern::parse(KAISHA).unwrap()));
        assert!(q.uses_migemo());
        assert!(q.matches(&w("会社案内.pdf")));
        assert!(q.matches(&w("my-KAISHA-notes")));
        assert!(!q.matches(&w("会議.txt")));
        // A pattern that misses a substring hit must not hide it.
        let odd = Query::new("f1", Some(Pattern::parse("zzz").unwrap()));
        assert!(odd.matches(&w("F10.txt")));
        let plain = Query::plain("kaisha");
        assert!(!plain.uses_migemo());
        assert!(!plain.matches(&w("会社案内.pdf")));
        assert!(plain.matches(&w("KAISHA.md")));
        assert!(Query::plain("").matches(&w("anything")));
    }

    #[test]
    fn query_equality() {
        assert_eq!(Query::plain("F1"), Query::plain("f1"));
        assert_ne!(Query::plain("f1"), Query::plain("f2"));
        assert_ne!(
            Query::plain("f1"),
            Query::new("f1", Some(Pattern::parse("f1").unwrap()))
        );
    }
}
