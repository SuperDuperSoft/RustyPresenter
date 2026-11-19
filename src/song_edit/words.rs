//! A slide's words without their chords, for editing, and the chords put back afterwards (each over the same letter
//! it was over), so a volunteer fixing a typo never has to see or type a `[G]`. Also the chords drawn over the words
//! in plain text, for the slide editor to show what the band sees.
use crate::slides::is_chord;

/// Characters looked at for the `]` that closes a chord.
const MAX_CHORD: usize = 64;

/// One line taken apart: its words (spaces tidied, no chords) and each chord with the character it sits before.
#[derive(Clone, Debug, PartialEq)]
pub struct Parted { pub words: String, pub chords: Vec<(usize, String)> }

/// `[D]  Sometimes the [A/D]night` → words "Sometimes the night", chords (0, "[D]"), (14, "[A/D]"). Notes for the
/// band (`[*Softly]`) count as chords. Other brackets (`[x2]`) are words.
pub fn part(line: &str) -> Parted {
    let mut words = String::new();
    let mut n = 0usize;
    let mut chords = vec![];
    let mut rest = line;
    let push = |c: char, words: &mut String, n: &mut usize| {
        if c.is_whitespace() {
            if *n > 0 && !words.ends_with(' ') { words.push(' '); *n += 1 }
        } else { words.push(c); *n += 1 }
    };
    while let Some(i) = rest.find('[') {
        for c in rest[..i].chars() { push(c, &mut words, &mut n) }
        let tail = &rest[i + 1..];
        match tail.char_indices().take(MAX_CHORD).find(|&(_, c)| c == ']' || c == '[') {
            Some((e, ']')) if is_chord(tail[..e].trim()) || tail[..e].starts_with('*') => {
                chords.push((n, format!("[{}]", tail[..e].trim())));
                rest = &tail[e + 1..];
            }
            _ => { push('[', &mut words, &mut n); rest = tail; }
        }
    }
    for c in rest.chars() { push(c, &mut words, &mut n) }
    if words.ends_with(' ') { words.pop(); n -= 1; }
    for c in &mut chords { c.0 = c.0.min(n) }
    Parted { words, chords }
}

/// Whether a line has chords.
pub fn has_chords(line: &str) -> bool { !part(line).chords.is_empty() }

/// The words a slide's lines show for editing: each line's words; lines of chords alone are left out.
pub fn words(lines: &[String]) -> String {
    lines.iter().map(|l| part(l).words).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("\n")
}

/// The longest common subsequence of `a` and `b` as pairs of indexes.
fn lcs<T: PartialEq>(a: &[T], b: &[T]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] { dp[i + 1][j + 1] + 1 } else { dp[i + 1][j].max(dp[i][j + 1]) };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, vec![]);
    while i < n && j < m {
        if a[i] == b[j] { out.push((i, j)); i += 1; j += 1 } else if dp[i + 1][j] >= dp[i][j + 1] { i += 1 } else { j += 1 }
    }
    out
}

/// `new` words with the chords of `old` (a line with chords) put back over the same letters: where the letter a
/// chord was over is still there, the chord stays over it; where it was typed over, over what took its place.
pub fn rechord(old: &str, new: &str) -> String {
    let p = part(old);
    if p.chords.is_empty() { return new.to_string() }
    let a: Vec<char> = p.words.chars().collect();
    let b: Vec<char> = new.chars().collect();
    let pairs = lcs(&a, &b);
    // Where each old character went: its matched place; one typed over sits as far before the next matched
    // character as it did ("Amazing" → "amazing": the chord over A goes over a), never before the one before it.
    let mut map = vec![0usize; a.len() + 1];
    let mut k = 0;
    let mut prev: Option<(usize, usize)> = None;
    for (i, slot) in map.iter_mut().enumerate().take(a.len()) {
        while k < pairs.len() && pairs[k].0 < i { prev = Some(pairs[k]); k += 1 }
        let low = prev.map_or(0, |(_, pj)| pj + 1);
        *slot = match pairs.get(k) {
            Some(&(pi, pj)) if pi == i => pj,
            Some(&(pi, pj)) => pj.saturating_sub(pi - i).max(low),
            None => (low + prev.map_or(i, |(pi, _)| i - pi - 1)).min(b.len()),
        };
    }
    map[a.len()] = b.len();
    let mut out = String::new();
    let mut chords = p.chords.iter().map(|(at, c)| (map[(*at).min(a.len())], c)).peekable();
    for (j, ch) in b.iter().enumerate() {
        while let Some((at, c)) = chords.peek() { if *at <= j { out.push_str(c); chords.next(); } else { break } }
        out.push(*ch);
    }
    for (_, c) in chords { out.push_str(c) }
    out
}

/// The slide's lines after its words were edited to `edited` (one line each; a blank line starts another slide):
/// lines whose words did not change stay exactly as they were, chords and all; a changed line gets the chords of
/// the line it replaced; lines of chords alone stay where they were; new lines have no chords.
pub fn merge_words(old: &[String], edited: &str) -> Vec<String> {
    let new: Vec<&str> = edited.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    // The old lines with words, and where each is in `old`.
    let worded: Vec<(usize, String)> = old.iter().enumerate().map(|(i, l)| (i, part(l).words)).filter(|(_, w)| !w.is_empty()).collect();
    let a: Vec<String> = worded.iter().map(|(_, w)| w.trim().to_string()).collect();
    let b: Vec<String> = new.iter().map(|l| part(l).words).collect();
    let pairs = lcs(&a, &b);
    let mut out: Vec<String> = vec![];
    // Lines of chords alone before old line `upto` (in `old`) not yet written.
    let mut next_old = 0usize;
    let flush = |upto: usize, out: &mut Vec<String>, next_old: &mut usize| {
        while *next_old < upto {
            if part(&old[*next_old]).words.is_empty() && !old[*next_old].trim().is_empty() { out.push(old[*next_old].trim().to_string()) }
            *next_old += 1;
        }
    };
    let (mut i, mut j) = (0usize, 0usize);
    for &(pi, pj) in pairs.iter().chain(std::iter::once(&(a.len(), b.len()))) {
        // Old lines i..pi were changed into new lines j..pj: paired off in turn, the rest added or dropped.
        let gap_old = pi - i;
        for (k, nl) in new[j..pj].iter().enumerate() {
            if nl.trim().is_empty() { out.push(String::new()); continue }
            if k < gap_old {
                let oi = worded[i + k].0;
                flush(oi, &mut out, &mut next_old);
                out.push(rechord(old[oi].trim(), nl.trim()));
                next_old = oi + 1;
            } else {
                out.push(nl.trim().to_string());
            }
        }
        if pi < a.len() {
            let oi = worded[pi].0;
            flush(oi, &mut out, &mut next_old);
            out.push(old[oi].trim().to_string());
            next_old = oi + 1;
        }
        i = pi + 1;
        j = pj + 1;
    }
    flush(old.len(), &mut out, &mut next_old);
    out
}

/// The chords drawn over the words in plain text, a line of chords over each line of words (for a fixed-width
/// font), as the band reads them on the stage screen.
pub fn chord_view(lines: &[String]) -> String {
    let mut out: Vec<String> = vec![];
    for l in lines {
        let p = part(l);
        if p.chords.is_empty() { out.push(p.words); continue }
        let mut row = String::new();
        let mut len = 0usize;
        for (at, c) in &p.chords {
            let name = c.trim_start_matches('[').trim_end_matches(']').trim_start_matches('*');
            let pad = if len == 0 { *at } else { (*at).max(len + 1) };
            while len < pad { row.push(' '); len += 1 }
            row.push_str(name);
            len += name.chars().count();
        }
        out.push(row.trim_end().to_string());
        if !p.words.is_empty() { out.push(p.words) }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> { s.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn a_line_comes_apart_into_words_and_chords() {
        let p = part("[D]  Sometimes the [A/D]night");
        assert_eq!(p.words, "Sometimes the night");
        assert_eq!(p.chords, vec![(0, "[D]".into()), (14, "[A/D]".into())]);
        assert_eq!(part("my [Dbsus]de[Db]fense").chords, vec![(3, "[Dbsus]".into()), (5, "[Db]".into())]);
        assert_eq!(part("Sing [x2] [*Softly][G]now").words, "Sing [x2] now");
        assert_eq!(part("[G] [D]").words, "", "chords alone have no words");
    }

    #[test]
    fn a_typo_fixed_keeps_each_chord_over_its_letter() {
        assert_eq!(rechord("A[G]mazing [G7]grace! how [C]sweet the [G]sound", "Amazing grace, how sweet the sound"),
            "A[G]mazing [G7]grace, how [C]sweet the [G]sound");
        assert_eq!(rechord("[G]Amazing [C]grace", "Oh amazing grace"), "Oh [G]amazing [C]grace", "words added before");
        assert_eq!(rechord("[G]Amazing [C]grace", "Amazing"), "[G]Amazing[C]", "a chord whose words went stays at the end");
        assert_eq!(rechord("no chords here", "no chords there"), "no chords there");
    }

    #[test]
    fn merging_keeps_untouched_lines_and_chord_lines_exactly() {
        let old = v(&["[D]  Sometimes the [A/D]night is beautiful ", "[G] [D]", "You could [A7]touch it"]);
        assert_eq!(words(&old), "Sometimes the night is beautiful\nYou could touch it");
        // Only the second line of words changed.
        assert_eq!(merge_words(&old, "Sometimes the night is beautiful\nYou could touch it, Lord"),
            v(&["[D]  Sometimes the [A/D]night is beautiful", "[G] [D]", "You could [A7]touch it, Lord"]));
        // A line added at the end, and a blank line to start a new slide.
        assert_eq!(merge_words(&old, "Sometimes the night is beautiful\n\nYou could touch it\nNew line"),
            v(&["[D]  Sometimes the [A/D]night is beautiful", "", "[G] [D]", "You could [A7]touch it", "New line"]));
        // A line taken out.
        assert_eq!(merge_words(&old, "You could touch it"), v(&["[G] [D]", "You could [A7]touch it"]));
    }

    #[test]
    fn the_chord_view_puts_chords_over_their_words() {
        assert_eq!(chord_view(&v(&["[G]Amazing [C]grace", "how sweet", "[G] [D]"])), "G       C\nAmazing grace\nhow sweet\nG D");
        assert_eq!(chord_view(&v(&["[Asus][A]praise"])), "Asus A\npraise", "chords close together never run together");
    }
}
