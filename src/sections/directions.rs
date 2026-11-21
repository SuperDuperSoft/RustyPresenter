//! Directions for the band written among a song's words: "(1st Ending)", "(To Verse 2)", "(Repeat and fade)",
//! "(Instrumental)". SongSelect's chord charts and many hand-typed songs have them. They are for the musicians,
//! not for the room (Jake, November 20, 2026: "That stuff is in the file but why would the audience need to see
//! that?"): the slides show each line without them, and auto-follow does not wait for them to be sung. The song's
//! own text keeps them, and so does the stage screen's chord sheet.
//!
//! Only what is surely a direction goes. Words in brackets that are sung stay ("(Oh how He loves us)", "(him)"), and
//! so do the marks that tell the room something: who sings ("(Men)", "(All)") and how often ("(x2)", "(Repeat)").

/// One line of a slide as the room sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomLine {
    /// The line without its directions (chords, if any, are still in it).
    pub text: String,
    /// The line shows nothing because of what was taken out: it was all directions, or it is a second ending
    /// with the same words as the first.
    pub gone: bool,
}

/// Words a direction is made of besides its telling word.
const FILLER: &[&str] = &["to", "the", "and", "then", "back", "go", "goes", "on", "of", "at", "in", "a", "an", "only", "time", "times", "last",
    "first", "second", "third", "fourth", "final", "x", "twice", "once", "again", "out", "al", "until", "from", "as", "desired", "needed", "ad",
    "lib", "with", "no", "into", "next", "after", "before", "each", "every", "optional", "top", "end", "straight", "full", "up", "down",
    "half", "whole", "step", "steps", "change", "changes", "key", "d", "s", "c", "pre", "post", "misc", "band", "guitar", "piano", "keys",
    "drums", "bass", "organ", "pads", "strings", "vocals", "voices", "now", "here", "slow", "slowly", "slower", "faster", "big", "quiet", "fine"];
/// The parts of a song a direction sends the band to.
const PARTS: &[&str] = &["verse", "verses", "chorus", "choruses", "bridge", "prechorus", "refrain", "tag", "ending", "endings", "intro", "outro",
    "interlude", "instrumental", "coda", "turnaround", "vamp", "hook", "channel", "break", "solo", "top"];
/// A word that makes brackets a direction when all the other words in them are direction words too.
const TELLING: &[&str] = &["ending", "endings", "fade", "fades", "fading", "coda", "segno", "ds", "dc", "instrumental", "interlude", "turnaround",
    "vamp", "tacet", "rit", "ritard", "ritardando", "modulate", "modulation", "intro", "outro", "cappella", "capella", "acapella", "acappella",
    "solo", "spoken", "softly", "build", "builds", "band", "tag"];
const REPEAT: &[&str] = &["repeat", "repeats", "repeated", "rpt"];

fn number(w: &str) -> bool {
    let digits = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) && matches!(&w[digits.len()..], "" | "st" | "nd" | "rd" | "th" | "x")
        || w.strip_prefix('x').is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
}

fn known(w: &str) -> bool { number(w) || FILLER.contains(&w) || PARTS.contains(&w) || TELLING.contains(&w) || REPEAT.contains(&w) }

/// The words of `inner` in lowercase ("D.S." is "ds").
fn words(inner: &str) -> Vec<String> {
    inner.to_lowercase().replace("d.s.", "ds").replace("d.c.", "dc").replace("pre-chorus", "prechorus").replace("pre chorus", "prechorus")
        .split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// What stands in brackets (or alone on a line) is a direction for the band: every word is a direction word, and
/// one of them tells so. "(Repeat)" and "(x2)" are not: they tell the room to sing it again.
fn is_direction(inner: &str) -> bool {
    let w = words(inner);
    if w.is_empty() || w.len() > 8 || !w.iter().all(|x| known(x)) { return false }
    let has = |list: &[&str]| w.iter().any(|x| list.contains(&x.as_str()));
    // "to verse 2", "back to the chorus", "go to bridge".
    let sent = w.iter().enumerate().any(|(i, x)| x == "to" && w[i + 1..].iter().find(|y| y.as_str() != "the").is_some_and(|y| PARTS.contains(&y.as_str())));
    // "repeat chorus", "repeat and fade" (but not "repeat", "repeat 2x").
    let repeat_part = has(REPEAT) && (has(PARTS) || has(TELLING));
    // "last time", "2nd time only" (but not "3 times": that is how often).
    let ordinal = |x: &str| number(x) && ["st", "nd", "rd", "th"].iter().any(|s| x.ends_with(s));
    let which_time = w.windows(2).any(|p| (p[1] == "time" || p[1] == "times")
        && (["last", "first", "second", "third", "fourth", "final"].contains(&p[0].as_str()) || ordinal(&p[0])));
    let key_change = has(&["key"]) && has(&["change", "changes", "up", "down"]);
    sent || repeat_part || which_time || key_change || has(TELLING)
}

/// Which ending a direction names: "1st Ending" 1, "second ending" 2, "last ending" 99.
fn ending(inner: &str) -> Option<u8> {
    let w = words(inner);
    if !w.iter().any(|x| x == "ending" || x == "endings") { return None }
    w.iter().find_map(|x| match x.as_str() {
        "first" => Some(1), "second" => Some(2), "third" => Some(3), "fourth" => Some(4), "last" | "final" => Some(99),
        n => n.trim_end_matches(|c: char| c.is_ascii_alphabetic()).parse::<u8>().ok().filter(|_| number(n)),
    })
}

/// `line` without its directions, and the ending it is marked as (if any). A line that is nothing but a direction
/// with no brackets ("Repeat chorus", "Instrumental") comes back empty.
fn strip(line: &str) -> (String, Option<u8>, bool) {
    let mut out = String::new();
    let mut rest = line;
    let mut which = None;
    let mut took = false;
    while let Some(i) = rest.find(['(', '（']) {
        let open = rest[i..].chars().next().map_or(1, char::len_utf8);
        let tail = &rest[i + open..];
        match tail.find([')', '）']) {
            Some(e) if is_direction(&tail[..e]) => {
                out.push_str(&rest[..i]);
                which = which.or(ending(&tail[..e]));
                took = true;
                rest = &tail[e + tail[e..].chars().next().map_or(1, char::len_utf8)..];
            }
            _ => { out.push_str(&rest[..i + open]); rest = tail; }
        }
    }
    out.push_str(rest);
    if !took {
        // No brackets: only a whole line that starts as a direction does ("Repeat chorus x2", "Instrumental").
        let plain = crate::slides::strip_chords(line).unwrap_or_else(|| line.to_string());
        let w = words(&plain);
        let starts = w.first().is_some_and(|x| REPEAT.contains(&x.as_str()) || ["instrumental", "interlude", "turnaround"].contains(&x.as_str()));
        if starts && is_direction(&plain) {
            // The chords of a break stay for the stage; its name goes.
            let chords: String = line.split_inclusive(']').filter_map(|p| p.rfind('[').map(|i| &p[i..])).collect::<Vec<_>>().join(" ");
            return (chords, None, true);
        }
        return (line.to_string(), None, false);
    }
    // What a direction leaves behind: doubled spaces, and a colon or dash that led up to the words.
    let tidy = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let tidy = tidy.trim_start_matches([':', '-', '–', '—', ' ']).trim_end_matches(['-', '–', '—', ' ']).to_string();
    (tidy, which, true)
}

/// The words alone, for telling two endings with the same words apart from two with different ones.
fn sung(line: &str) -> String {
    let plain = crate::slides::strip_chords(line).unwrap_or_else(|| line.to_string());
    plain.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// A slide group's lines as the room sees them, one for each line given. A second (third, last) ending with the
/// same words as the ending before it shows nothing: only one of them is sung each time round.
pub fn room_lines(lines: &[String]) -> Vec<RoomLine> {
    let mut out: Vec<RoomLine> = Vec::with_capacity(lines.len());
    // The words of the last line marked as an ending.
    let mut before: Option<String> = None;
    for line in lines {
        let (text, which, took) = strip(line);
        let words = sung(&text);
        let again = which.is_some_and(|n| n >= 2) && !words.is_empty() && before.as_deref() == Some(words.as_str());
        if which.is_some() { before = Some(words.clone()) }
        out.push(RoomLine { gone: took && (words.is_empty() || again), text: if again { String::new() } else { text } });
    }
    out
}

/// `text` (a slide's lines) without directions, for places that have no slide groups (a pasted line, a search).
pub fn without(text: &str) -> String {
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    room_lines(&lines).into_iter().filter(|l| !l.gone).map(|l| l.text).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(lines: &[&str]) -> Vec<String> {
        room_lines(&lines.iter().map(|l| l.to_string()).collect::<Vec<_>>()).into_iter().map(|l| if l.gone { "<gone>".into() } else { l.text }).collect()
    }

    #[test]
    fn directions_for_the_band_go() {
        for (line, want) in [
            ("(1st Ending) And I will follow You all of my days (To Verse 2)", "And I will follow You all of my days"),
            ("And I will follow You all of my days (Repeat and fade)", "And I will follow You all of my days"),
            ("Holy, holy, holy (To Chorus)", "Holy, holy, holy"),
            ("(Back to the Bridge) We sing", "We sing"),
            ("Worthy is the Lamb (Last time)", "Worthy is the Lamb"),
            ("Worthy is the Lamb (2nd time only)", "Worthy is the Lamb"),
            ("Forever (Key change) forever", "Forever forever"),
            ("(Repeat Chorus): Our God reigns", "Our God reigns"),
            ("Amen (D.S. al Coda)", "Amen"),
            ("Amen (D.C. al Fine)", "Amen"),
            ("Amen (Tag 2x)", "Amen"),
            ("Amen (A cappella)", "Amen"),
            ("Amen (Guitar solo)", "Amen"),
            ("Amen (Full band)", "Amen"),
            ("Amen （To Verse 3）", "Amen"),
        ] {
            assert_eq!(room(&[line]), [want], "{line}");
        }
    }

    #[test]
    fn sung_words_and_marks_for_the_room_stay() {
        for line in [
            "Oh how He loves us (Oh how He loves us)",
            "How one star he saw had been lit for me (him)",
            "(Men) Holy is the Lord",
            "(All) Holy is the Lord",
            "Holy is the Lord (x2)",
            "Holy is the Lord (2x)",
            "Holy is the Lord (Repeat)",
            "Holy is the Lord (Repeat 2x)",
            "(Repeat the sounding joy)",
            "Repeat the sounding joy",
            "(Again and again)",
            "(To the end of the age)",
            "It is fine (fine)",
            "(One more time)",
            "(Hallelujah)",
            "Back to the start",
            "To the King",
        ] {
            assert_eq!(room(&[line]), [line], "{line}");
        }
    }

    #[test]
    fn a_line_of_nothing_but_a_direction_shows_nothing() {
        assert_eq!(room(&["(Repeat Chorus)", "(Instrumental)", "Repeat chorus x2", "Instrumental", "(To Verse 2)"]), ["<gone>"; 5]);
        // A break's chords stay for the stage.
        let lines = room_lines(&["[G] [D] (Instrumental)".to_string(), "Instrumental [Em] [C]".to_string()]);
        assert_eq!((lines[0].text.as_str(), lines[0].gone, lines[1].text.as_str(), lines[1].gone), ("[G] [D]", true, "[Em] [C]", true));
    }

    #[test]
    fn a_second_ending_with_the_same_words_is_not_shown_twice() {
        let chorus = ["And [D]step by step You'll [A/C#]lead me [A]",
            "(1st Ending)    And I will [G/B]follow You [Asus]all of [A]my  [D]days      [G/D]    [A/D]     (To Verse 2)",
            "(2nd Ending)   And I will [G/B]follow You [Asus]all of [A]my  [D]days      (To Ending)"];
        let out = room(&chorus);
        assert_eq!(out[0], chorus[0]);
        assert_eq!(crate::slides::strip_chords(&out[1]).unwrap(), "And I will follow You all of my days");
        assert_eq!(out[2], "<gone>");
        // Different words: both endings show.
        assert_eq!(room(&["(1st Ending) All of my days", "(2nd Ending) All of my life", "(Last Ending) All of my life"]),
            ["All of my days", "All of my life", "<gone>"]);
        // Not an ending: the same line twice is sung twice.
        assert_eq!(room(&["All of my days", "All of my days"]), ["All of my days", "All of my days"]);
    }

    #[test]
    fn without_takes_them_from_a_slides_text() {
        assert_eq!(without("Amen (To Verse 2)\n(Instrumental)\nAmen"), "Amen\nAmen");
    }
}
