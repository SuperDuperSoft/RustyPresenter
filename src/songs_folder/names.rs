//! File names for songs: the title, made safe on Windows, macOS and Linux, and different from every other file in
//! the folder ("Amazing Grace.chordpro", "Amazing Grace (2).chordpro"). Windows and macOS do not tell capitals
//! apart, so neither do these names.

/// The longest name kept before the ending, in characters (a path on Windows has room for about 260).
const MAX_STEM: usize = 100;

/// Names Windows keeps for devices: a file may not be called these, whatever its ending.
const RESERVED: &[&str] = &["con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9"];

/// A song title as a file name without its ending: characters no system allows become spaces, the name is trimmed
/// of spaces and dots at its ends (Windows drops them; a leading dot hides a file on macOS and Linux) and cut to a
/// sensible length. An empty title is "Song".
pub fn stem(title: &str) -> String {
    let cleaned: String = title.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c }).collect();
    let mut s = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    s = s.chars().take(MAX_STEM).collect();
    let s = s.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    if s.is_empty() { return "Song".into() }
    let base = s.split('.').next().unwrap_or("").trim().to_lowercase();
    if RESERVED.contains(&base.as_str()) { format!("{s} (song)") } else { s }
}

/// The first of "stem.ext", "stem (2).ext", "stem (3).ext" … that `taken` does not hold (compared without capitals).
pub fn unique(stem: &str, ext: &str, taken: impl Fn(&str) -> bool) -> String {
    let first = format!("{stem}.{ext}");
    if !taken(&first.to_lowercase()) { return first }
    (2..).map(|n| format!("{stem} ({n}).{ext}")).find(|n| !taken(&n.to_lowercase())).unwrap_or(first)
}

/// Whether `file` is a name `unique` could give for `title` ("Grace.chordpro", "Grace (3).chordpro"), so a song
/// whose title did not change keeps its file.
pub fn fits(file: &str, title: &str, ext: &str) -> bool {
    let Some(s) = file.strip_suffix(&format!(".{ext}")) else { return false };
    let want = stem(title).to_lowercase();
    let s = s.to_lowercase();
    if s == want { return true }
    s.strip_prefix(&want).and_then(|r| r.strip_prefix(" (")).and_then(|r| r.strip_suffix(')')).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe_everywhere() {
        assert_eq!(stem("Amazing Grace"), "Amazing Grace");
        assert_eq!(stem("What A Beautiful Name (Live)"), "What A Beautiful Name (Live)");
        assert_eq!(stem("10,000 Reasons / Bless the Lord"), "10,000 Reasons Bless the Lord");
        assert_eq!(stem("Who? What: \"Why\" <me> | *you*\\"), "Who What Why me you");
        assert_eq!(stem("  ...Hidden.  "), "Hidden");
        assert_eq!(stem("Tab\there\nnew line"), "Tab here new line");
        assert_eq!(stem(""), "Song");
        assert_eq!(stem("???"), "Song");
        assert_eq!(stem("CON"), "CON (song)");
        assert_eq!(stem("nul.txt"), "nul.txt (song)");
        assert_eq!(stem("Console"), "Console");
        assert_eq!(stem("Él es Señor 主 Святый"), "Él es Señor 主 Святый");
        assert_eq!(stem(&"x".repeat(300)).chars().count(), 100);
    }

    #[test]
    fn names_never_clash() {
        let taken = ["amazing grace.chordpro", "amazing grace (2).chordpro"];
        assert_eq!(unique("Amazing Grace", "chordpro", |n| taken.contains(&n)), "Amazing Grace (3).chordpro");
        assert_eq!(unique("AMAZING GRACE", "chordpro", |n| taken.contains(&n)), "AMAZING GRACE (3).chordpro", "capitals do not count");
        assert_eq!(unique("Other", "chordpro", |n| taken.contains(&n)), "Other.chordpro");
        assert!(fits("Grace.chordpro", "Grace", "chordpro"));
        assert!(fits("grace (12).chordpro", "Grace", "chordpro"));
        assert!(fits("Who What.chordpro", "Who? What", "chordpro"));
        assert!(!fits("Grace Again.chordpro", "Grace", "chordpro"));
        assert!(!fits("Grace (x).chordpro", "Grace", "chordpro"));
        assert!(!fits("Grace.txt", "Grace", "chordpro"));
    }
}
