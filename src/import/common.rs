//! Shared by the song file readers: the report a reader returns, the label lines of the app's lyric format, and
//! how a line of writers is split.
use crate::{model::Song, slides::section_label};

#[derive(Debug, Default)]
pub struct ImportReport {
    pub songs: Vec<Song>,
    /// (title, reason) for every row that did not become a song.
    pub skipped: Vec<(String, String)>,
}

// ---------- lyric format ----------

/// Label line in the app's lyric format: as-is when `section_label` knows it, else `[bracketed]`.
/// Cut to fit `section_label`'s 40-byte limit, or the label would read back as a lyric line.
pub(crate) fn label_line(label: &str) -> String {
    let mut l = String::new();
    for c in label.chars() { if l.len() + c.len_utf8() > 38 { break; } l.push(c); }
    let l = l.trim_end();
    if section_label(l).is_some() { l.to_string() } else { format!("[{l}]") }
}

/// "Chorus (2)", "Chorus (3)", …: the first numbered form of `base` whose section name isn't `taken`. The
/// base is shortened so the number always survives `label_line`'s cut (a long name would otherwise give the
/// same section name for every number, and the search would never end). Bounded; falls back to `base`.
pub(crate) fn numbered_label(base: &str, taken: impl Fn(&str) -> bool) -> String {
    for n in 2..=100_000usize {
        let suffix = format!(" ({n})");
        let mut cut = String::new();
        for c in base.chars() { if cut.len() + c.len_utf8() + suffix.len() > 38 { break; } cut.push(c); }
        let label = format!("{}{suffix}", cut.trim_end());
        if !taken(&section_name(Some(&label))) { return label; }
    }
    base.to_string()
}

/// A lyric line as written into the app's format: one `section_label` would read as a label ("Instrumental",
/// "Tag", "[Repeat]", common placeholder slides) is put in parentheses so it stays words on the slide.
pub(crate) fn lyric_line(line: &str) -> String {
    if section_label(line).is_none() { return line.to_string(); }
    let t = line.trim();
    let inner = if t.starts_with('[') && t.ends_with(']') { t.trim_matches(['[', ']']).trim() } else { t };
    format!("({inner})")
}

/// The name `parse_sections` gives a label, which is what `arrangement` must list.
pub(crate) fn section_name(label: Option<&str>) -> String {
    label.map(|l| section_label(&label_line(l)).unwrap_or_default()).unwrap_or_else(|| "Part 1".into())
}

// ---------- writers ----------

/// "A / B", "A; B", "A, B, and C" → "A | B | C" (the SongSelect writers style).
/// Try `/`, then `;`, then `,`; the first that gives two or more names wins.
pub fn split_writers(author: &str) -> String {
    let t = author.trim();
    let mut parts = vec![t];
    for sep in ['/', ';', ','] {
        let p: Vec<&str> = t.split(sep).map(str::trim).filter(|s| !s.is_empty()).collect();
        if p.len() >= 2 { parts = p; break; }
    }
    parts.into_iter().map(|p| {
        let lower = p.to_ascii_lowercase();
        if lower.starts_with("and ") { p[4..].trim() } else if lower.starts_with("& ") { p[2..].trim() } else { p }
    }).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writers_are_split_at_slashes_semicolons_or_commas() {
        assert_eq!(split_writers("Ann Lee / Bob Ray"), "Ann Lee | Bob Ray");
        assert_eq!(split_writers("Ann Lee; Bob Ray"), "Ann Lee | Bob Ray");
        assert_eq!(split_writers("Ann Lee, Bob Ray, and Cy Day"), "Ann Lee | Bob Ray | Cy Day");
        assert_eq!(split_writers("Ann Lee & Bob Ray"), "Ann Lee & Bob Ray");
        assert_eq!(split_writers("  "), "");
    }
}
