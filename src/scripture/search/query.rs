//! Search queries, the tokenizer shared by verses and queries, and result snippets (spec §3.1, §5.5).
use crate::scripture::strings::EMPTY_SEARCH;

/// Most terms a query uses; later ones are ignored.
pub const MAX_TERMS: usize = 12;
/// Longest snippet body, in chars (without the `…` marks).
pub const SNIPPET_CHARS: usize = 160;
/// A snippet starts at most this many chars before the first match.
pub const SNIPPET_LEAD: usize = 40;

/// Words split on whitespace and on the dashes `- – —`.
fn is_separator(c: char) -> bool { c.is_whitespace() || matches!(c, '-' | '–' | '—') }

/// One raw token as a search word: lowercase, no apostrophes, only letters and digits.
fn normalize(raw: &str) -> String {
    raw.chars().filter(|c| !matches!(c, '’' | '\'')).flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric()).collect()
}

/// The words of a verse or a query; `LORD's` → `lords`, `know—that` → `know`, `that`.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(is_separator).map(normalize).filter(|w| !w.is_empty()).collect()
}

/// One query term: a single word, or a quoted phrase of two or more words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Term {
    Word(String),
    Phrase(Vec<String>),
}

/// A parsed keyword search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    pub terms: Vec<Term>,
}

impl SearchQuery {
    /// `love "be still" God` → word, phrase, word. An unclosed quote runs to the end;
    /// a phrase that normalises to one word is a `Word`. At most 12 terms count.
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut terms = Vec::new();
        for (i, part) in s.split('"').enumerate() {
            let words = tokenize(part);
            if i % 2 == 1 && words.len() > 1 {
                terms.push(Term::Phrase(words));
            } else {
                terms.extend(words.into_iter().map(Term::Word));
            }
        }
        terms.truncate(MAX_TERMS);
        if terms.is_empty() { return Err(EMPTY_SEARCH.to_string()); }
        Ok(Self { terms })
    }

    /// Every query word in order, phrases flattened.
    pub fn words(&self) -> Vec<&str> {
        self.terms.iter().flat_map(|t| match t {
            Term::Word(w) => std::slice::from_ref(w),
            Term::Phrase(ws) => ws.as_slice(),
        }).map(String::as_str).collect()
    }
}

/// Terms of three or more letters match as a prefix (`love` finds `loveth`); shorter ones exactly.
pub fn word_matches(term: &str, word: &str) -> bool {
    if term.chars().count() >= 3 { word.starts_with(term) } else { word == term }
}

/// Char offset of the first word in `text` that matches any query word (0 when none does).
pub fn first_match(text: &str, q: &SearchQuery) -> usize {
    let words = q.words();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if is_separator(chars[i]) { i += 1; continue; }
        let end = (i..chars.len()).find(|&j| is_separator(chars[j])).unwrap_or(chars.len());
        let raw: String = chars[i..end].iter().collect();
        let word = normalize(&raw);
        if !word.is_empty() && words.iter().any(|t| word_matches(t, &word)) {
            // Point at the word itself, not at leading punctuation such as a quote mark.
            return (i..end).find(|&j| chars[j].is_alphanumeric()).unwrap_or(i);
        }
        i = end;
    }
    0
}

/// At most 160 chars of verse text around the first match, starting at a word boundary no more
/// than 40 chars before it, with `…` where text was cut. Text of 160 chars or less is returned whole.
pub fn snippet(text: &str, first_match_char: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= SNIPPET_CHARS { return text.to_string(); }
    let at = first_match_char.min(chars.len());
    let word_start = |p: usize| p == 0 || (chars[p - 1].is_whitespace() && !chars[p].is_whitespace());
    let start = (at.saturating_sub(SNIPPET_LEAD)..at).find(|&p| word_start(p)).unwrap_or(at);
    let mut end = (start + SNIPPET_CHARS).min(chars.len());
    if end < chars.len() && !chars[end].is_whitespace() {
        // Don't cut a word in half when an earlier break exists.
        if let Some(p) = (start + 1..end).rev().find(|&p| chars[p].is_whitespace()) { end = p; }
    }
    let body: String = chars[start..end].iter().collect();
    let body = body.trim();
    let mut out = String::new();
    if start > 0 { out.push('…'); }
    out.push_str(body);
    if end < chars.len() { out.push('…'); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parses_terms_and_quoted_phrases() {
        let q = SearchQuery::parse(r#"love "be still" God"#).unwrap();
        assert_eq!(q.terms, vec![Term::Word("love".into()), Term::Phrase(vec!["be".into(), "still".into()]), Term::Word("god".into())]);
    }
    #[test]
    fn query_normalises_case_and_apostrophes() {
        assert_eq!(SearchQuery::parse("LORD’s Don't").unwrap().words(), vec!["lords", "dont"]);
    }
    #[test]
    fn empty_query_is_an_error() {
        for s in ["", "   ", "!!", "\"\""] { assert_eq!(SearchQuery::parse(s).unwrap_err(), "Type at least one word to search for."); }
    }
    #[test]
    fn only_first_12_terms_count() { assert_eq!(SearchQuery::parse(&"a ".repeat(20)).unwrap().terms.len(), 12); }
    #[test]
    fn tokenizer_splits_dashes_and_drops_punctuation() {
        assert_eq!(tokenize("know—that I am God: the LORD's"), vec!["know", "that", "i", "am", "god", "the", "lords"]);
    }
    #[test]
    fn prefix_rule_needs_three_letters() {
        assert!(word_matches("love", "loveth") && word_matches("an", "an") && !word_matches("an", "and"));
    }
    #[test]
    fn snippet_trims_around_first_match() {
        let text = format!("{}needle {}", "word ".repeat(30), "tail ".repeat(40));
        let s = snippet(&text, text.find("needle").unwrap());
        assert!(s.starts_with('…') && s.ends_with('…') && s.contains("needle"));
        let body = s.trim_matches('…');
        assert!(body.chars().count() <= 160 && !body.starts_with(' '));
        assert!(body.find("needle").unwrap() <= 40);
    }
    #[test]
    fn short_verse_snippet_is_whole() { assert_eq!(snippet("Be still, and know", 0), "Be still, and know"); }
}
