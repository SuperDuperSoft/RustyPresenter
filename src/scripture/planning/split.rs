//! Splitting a too-long scripture slide into even parts (spec §5.4 step 4). Splitting runs on
//! the composed slide text, so a verse superscript stays attached to its first word.
use super::measure::{Fit, Measure};
use crate::scripture::ParallelText;

/// Splits `text` into `n` parts (clamped to `1..=word count`) at word boundaries (ASCII spaces).
/// Cut `k` sits nearest `k × len / n` chars, preferring a sentence end (`. ? !`) within ±25 %
/// of one share, else a clause end (`; : ,`) within ±25 %, else the nearest word boundary.
/// Never drops or reorders words and never returns an empty part.
pub fn split_into(text: &str, n: usize) -> Vec<String> {
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    let n = n.clamp(1, words.len().max(1));
    if n == 1 {
        return vec![text.to_string()];
    }
    // ends[j]: char position just after word j, i.e. the boundary between words j and j + 1.
    let mut ends = Vec::with_capacity(words.len());
    let mut pos = 0;
    for (j, w) in words.iter().enumerate() {
        pos += w.chars().count() + usize::from(j > 0);
        ends.push(pos);
    }
    let len = pos as f64;
    let share = len / n as f64;
    let tol = 0.25 * share;
    let mut cuts = Vec::with_capacity(n - 1);
    let mut lo = 0;
    for k in 1..n {
        // Boundaries are 0..=W-2; leave one for each later cut.
        let hi = words.len() + k - n - 1;
        let target = k as f64 * share;
        let dist = |j: usize| (ends[j] as f64 - target).abs();
        let nearest = |ok: &dyn Fn(usize) -> bool| (lo..=hi).filter(|&j| ok(j)).min_by(|&a, &b| dist(a).total_cmp(&dist(b)));
        let ends_with = |j: usize, marks: &[char]| words[j].trim_end_matches(CLOSERS).ends_with(marks);
        let cut = nearest(&|j| dist(j) <= tol && ends_with(j, &['.', '?', '!']))
            .or_else(|| nearest(&|j| dist(j) <= tol && ends_with(j, &[';', ':', ','])))
            .or_else(|| nearest(&|_| true))
            .expect("lo <= hi while n <= word count");
        cuts.push(cut);
        lo = cut + 1;
    }
    let mut parts = Vec::with_capacity(n);
    let mut start = 0;
    for end in cuts.into_iter().chain([words.len() - 1]) {
        parts.push(words[start..=end].join(" "));
        start = end + 1;
    }
    parts
}

/// Closing quotes and brackets that may follow a sentence or clause mark.
const CLOSERS: &[char] = &['"', '\'', '\u{201D}', '\u{2019}', ')', ']'];

fn word_count(text: &str) -> usize { text.split(' ').filter(|w| !w.is_empty()).count() }

/// Splits a `TooLong` slide (main text plus optional second column) into the same number of
/// parts in both columns: the fewest parts that are all `Comfortable`, else all at most
/// `Shrunk`, else as many parts as there are words (`W`, the smaller count of the non-empty
/// columns). With `W < 2` there is nothing to split and the slide comes back as one part.
pub(super) fn split_slide(main: &str, second: Option<&ParallelText>, m: &mut dyn Measure) -> Vec<(String, Option<ParallelText>)> {
    let counts = [Some(word_count(main)), second.map(|s| word_count(&s.text))];
    let w = counts.into_iter().flatten().filter(|&c| c > 0).min().unwrap_or(0);
    let column = |text: &str, n: usize| if word_count(text) == 0 { vec![text.to_string(); n] } else { split_into(text, n) };
    let parts = |n: usize| -> Vec<(String, Option<ParallelText>)> {
        let seconds: Vec<Option<ParallelText>> = match second {
            Some(s) => column(&s.text, n).into_iter().map(|text| Some(ParallelText { text, ..s.clone() })).collect(),
            None => vec![None; n],
        };
        column(main, n).into_iter().zip(seconds).collect()
    };
    if w < 2 {
        return vec![(main.to_string(), second.cloned())];
    }
    let mut shrunk = None;
    for n in 2..=w {
        let p = parts(n);
        let fits: Vec<Fit> = p.iter().map(|(t, s)| m.fits(t, s.as_ref())).collect();
        if fits.iter().all(|&f| f == Fit::Comfortable) {
            return p;
        }
        if shrunk.is_none() && fits.iter().all(|&f| f != Fit::TooLong) {
            shrunk = Some(p);
        }
    }
    shrunk.unwrap_or_else(|| parts(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: char = '\u{2009}';

    #[test]
    fn split_into_keeps_every_word_and_prefers_sentences() {
        let t = "One two three. Four five six; seven eight nine ten eleven.";
        let p = split_into(t, 2);
        assert_eq!(p.join(" "), t);
        assert!(p[0].ends_with('.') || p[0].ends_with(';'));
    }

    #[test]
    fn split_into_clamps_to_word_count() {
        assert_eq!(split_into("one two", 3), ["one", "two"]);
        assert_eq!(split_into("solo", 4), ["solo"]);
        assert_eq!(split_into(&format!("¹⁷{T}word next"), 2), [format!("¹⁷{T}word"), "next".to_string()]); // superscript stays attached
    }
}
