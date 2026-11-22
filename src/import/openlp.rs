//! Verses → lyrics in the app's format, for the song files written the OpenLP way: OpenLyrics (`openlyrics`) and
//! OpenSong (`opensong`) both read their file into `Verse`s and build the song here. A verse has a type code
//! ("v", "c" …) and a label ("1", "2a"); a verse order ("v1 c1 v2 c1") lists verse tags in the order they are
//! sung and becomes the arrangement. OpenLP's formatting tags, chords and slide splits are cleaned out of the
//! text.
use super::common::{label_line, section_name};
use crate::tr;
use std::collections::{HashMap, HashSet};

/// One `<verse>` of the lyrics XML: type code ("v", "c" …), label ("1", "2a"), text. `name` is a section name of
/// its own (OpenSong's `[Coda]`, an OpenLyrics `name="Outro"`) used instead of one made from kind and label;
/// OpenLP's own verses have none.
#[derive(Clone, Debug, PartialEq)]
pub struct Verse { pub kind: String, pub label: String, pub text: String, pub name: String }

/// Lyrics bigger than this are not a song (a stray document pasted in); skipped instead of parsed.
const MAX_LYRICS: usize = 1_000_000;
/// More verses than this is not a song either (and would make labelling slow).
const MAX_VERSES: usize = 500;
/// `verse_order` tags read, and sections put in the arrangement, at most.
const MAX_ORDER_TAGS: usize = 1000;
const MAX_ORDER: usize = 2000;

// ---------- lyrics XML → lyrics ----------

fn is_bom_or_space(c: char) -> bool { c.is_whitespace() || c == '\u{feff}' }

/// The text of one `<verse>`. OpenLP's own format is a CDATA block with real newlines. OpenLyrics-style verses
/// hold `<lines>` (each one a slide) whose line breaks are `<br/>` (or one `<line>` per line), and whose
/// whitespace, newlines included, is only spacing; `<comment>`s are not lyrics. Only a new `<lines>` starts a
/// new slide: a line left empty inside one (a comment or chord alone, `<br/><br/>`, an empty `<line/>`) is
/// dropped. `repeat` and `part` are not shown: the words are projected once, without the singers' part.
/// Walked without recursion, so deep nesting can't overflow the stack, and each element is entered and left once.
pub(crate) fn verse_text(verse: roxmltree::Node) -> String {
    /// `out` is the verse so far; `block` the `<lines>` being read, kept apart until it ends.
    struct Walk { out: String, block: String, in_lines: usize, in_comment: usize }
    impl Walk {
        fn dst(&mut self) -> &mut String { if self.in_lines > 0 { &mut self.block } else { &mut self.out } }
        /// Ends the slide being read: its non-empty lines, with blank lines around them (a slide break).
        fn flush(&mut self) {
            let lines: Vec<&str> = self.block.split('\n').filter(|l| !l.trim().is_empty()).collect();
            if !lines.is_empty() {
                let text = lines.join("\n");
                self.out.push_str("\n\n");
                self.out.push_str(&text);
                self.out.push_str("\n\n");
            }
            self.block.clear();
        }
        fn enter(&mut self, name: &str) {
            match name {
                "br" => self.dst().push('\n'),
                "lines" => { self.flush(); self.in_lines += 1; }
                "comment" => self.in_comment += 1,
                _ => {}
            }
        }
        fn leave(&mut self, name: &str) {
            match name {
                "line" => self.dst().push('\n'),
                "lines" => { self.flush(); self.in_lines = self.in_lines.saturating_sub(1); }
                "comment" => self.in_comment = self.in_comment.saturating_sub(1),
                _ => {}
            }
        }
    }
    let name = |n: roxmltree::Node| n.tag_name().name().to_ascii_lowercase();
    let mut w = Walk { out: String::new(), block: String::new(), in_lines: 0, in_comment: 0 };
    // Open elements, innermost last: the ancestors of the node being visited (descendants() is document order).
    let mut open: Vec<roxmltree::Node> = Vec::new();
    for n in verse.descendants().skip(1) {
        while open.last().is_some_and(|o| Some(*o) != n.parent()) {
            if let Some(o) = open.pop() { w.leave(&name(o)); }
        }
        if n.is_text() {
            if w.in_comment > 0 { continue; }
            let t = n.text().unwrap_or("");
            if w.in_lines > 0 { w.block.extend(t.chars().map(|c| if c.is_whitespace() { ' ' } else { c })); } else { w.out.push_str(t); }
        } else if n.is_element() {
            w.enter(&name(n));
            open.push(n);
        }
    }
    while let Some(o) = open.pop() { w.leave(&name(o)); }
    w.flush();
    w.out
}

/// OpenLP's verse types by their one-letter code (also how `verse_order` tags start). Anything else is "Other".
fn letter_code(c: Option<char>) -> char {
    match c.map(|c| c.to_ascii_lowercase()) {
        Some(c @ ('v' | 'c' | 'b' | 'p' | 'i' | 'e' | 't')) => c,
        _ => 'o',
    }
}

/// A verse's `type`. Databases from OpenLP 1.9 and early 2.0 may hold whole, sometimes translated, names
/// ("Strophe", "Refrain"): those are matched as words, like OpenLP's own loose matching; otherwise the first
/// letter is the code.
fn kind_code(kind: &str) -> char {
    let k = kind.trim().to_lowercase();
    match k.as_str() {
        "verse" | "vers" | "strophe" | "estrofa" | "couplet" | "verso" => 'v',
        "chorus" | "refrain" | "coro" | "estribillo" | "ritornello" => 'c',
        "bridge" | "brücke" | "puente" | "pont" | "ponte" => 'b',
        "pre-chorus" | "prechorus" | "pre chorus" | "pre-refrain" | "vorrefrain" | "pre-coro" => 'p',
        "intro" | "introduction" | "einleitung" | "introducción" => 'i',
        "ending" | "end" | "outro" | "coda" | "schluss" | "final" => 'e',
        "tag" => 't',
        "other" | "andere" | "otro" => 'o',
        _ => letter_code(k.chars().next()),
    }
}

fn kind_name(code: char) -> &'static str {
    match code { 'v' => "Verse", 'c' => "Chorus", 'b' => "Bridge", 'p' => "Pre-Chorus", 'i' => "Intro", 'e' => "Ending", 't' => "Tag", _ => "Other" }
}

/// `[G]`, `[D/F#]`, `[Am7]`, `[Csus4]`, `[B♭]`, `[H7]`, `[Fis]`, `[Do]`, `[em]`: an inline chord (OpenLP 3), not
/// lyrics. OpenLP accepts English, German (H, -is/-es) and neo-Latin (Do Re Mi …) notation; lowercase is minor
/// in German. `[Ending]` is not one.
fn is_chord(s: &str) -> bool {
    const ROOTS: &[&str] = &["Sol", "Do", "Re", "Mi", "Fa", "La", "Si", "A", "B", "C", "D", "E", "F", "G", "H",
        "a", "b", "c", "d", "e", "f", "g", "h"];
    fn suffix_ok(mut rest: &str) -> bool {
        'next: while !rest.is_empty() {
            for t in ["maj", "min", "sus", "dim", "aug", "add", "m", "M", "+", "-", "#", "b", "♯", "♭", "(", ")", "°", "ø"] {
                if let Some(r) = rest.strip_prefix(t) { rest = r; continue 'next; }
            }
            let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            if digits == 0 { return false; }
            rest = &rest[digits..];
        }
        true
    }
    fn chord_part(s: &str) -> bool {
        // "Esus4" must not read as "Es" + "us4", so try with and without an accidental.
        ROOTS.iter().filter_map(|r| s.strip_prefix(r)).any(|rest| suffix_ok(rest)
            || ["is", "es", "s", "#", "b", "♯", "♭", "x"].iter().filter_map(|a| rest.strip_prefix(a)).any(suffix_ok))
    }
    let mut parts = s.splitn(2, '/');
    s.len() <= 16 && parts.next().is_some_and(chord_part) && parts.next().is_none_or(chord_part)
}

/// OpenLP's built-in formatting tags. Tags a user defined in OpenLP's settings are not in the song database,
/// so any other `{…}` is kept as text: it may be something the user typed, like `{Repeat}`.
const OPENLP_TAGS: &[&str] = &["r", "b", "y", "o", "g", "pk", "bl", "pp", "w", "st", "it", "u", "br", "p", "su", "sb", "sup", "sub"];

/// Characters looked at for the end of a `{tag}` or `[chord]`, so an unclosed bracket costs little.
const MAX_MARKUP: usize = 21;

/// Removes `{r}…{/r}` formatting tags (`{br}` is a line break) and inline chords.
fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['{', '[']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let close = if tail.starts_with('{') { '}' } else { ']' };
        match tail[1..].char_indices().take(MAX_MARKUP).find(|&(_, c)| c == close).map(|(e, _)| e) {
            Some(e) => {
                let inner = &tail[1..1 + e];
                let tag = inner.strip_prefix('/').unwrap_or(inner);
                let is_tag = close == '}' && OPENLP_TAGS.iter().any(|t| t.eq_ignore_ascii_case(tag));
                if is_tag {
                    if inner.eq_ignore_ascii_case("br") { out.push('\n'); }
                } else if !(close == ']' && is_chord(inner)) {
                    out.push_str(&tail[..e + 2]);
                }
                rest = &tail[e + 2..];
            }
            None => { out.push_str(&tail[..1]); rest = &tail[1..]; }
        }
    }
    out.push_str(rest);
    out
}

/// Verse text → slides (groups of lines). `[---]` (optional split) and blank lines start a new slide. A line
/// left empty only by removing markup (a chord-only line) is dropped, not a split.
fn verse_groups(text: &str) -> Vec<Vec<String>> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n").replace("[--}{--]", "\n\n").replace("[---]", "\n\n");
    let mut groups: Vec<Vec<String>> = vec![vec![]];
    for raw in text.split('\n') {
        if raw.trim_matches(is_bom_or_space).is_empty() {
            if groups.last().is_some_and(|g| !g.is_empty()) { groups.push(vec![]); }
            continue;
        }
        for line in strip_markup(raw).split('\n') {
            // Removing a chord mid-line leaves two spaces; tabs are spacing too.
            let t = line.split_whitespace().collect::<Vec<_>>().join(" ");
            if let Some(g) = groups.last_mut().filter(|_| !t.is_empty()) { g.push(t); }
        }
    }
    groups.retain(|g| !g.is_empty());
    groups
}

fn join_groups(groups: &[Vec<String>]) -> String {
    groups.iter().map(|g| g.join("\n")).collect::<Vec<_>>().join("\n\n")
}

/// Verses (from OpenLyrics or OpenSong) plus a verse order ("v1 c1 v2 c1") → lyrics in the app's format (label
/// lines, blank line between slides) and the arrangement: section names in verse-order order, empty when that is
/// just the written order. Err is why the lyrics can't be used.
pub(crate) fn lyrics_from_verses(verses: Vec<Verse>, verse_order: &str) -> Result<(String, Vec<String>), String> {
    if verses.len() > MAX_VERSES { return Err(tr!("the words have too many verses (more than {max})", max = MAX_VERSES)); }
    if verses.iter().map(|v| v.text.len()).sum::<usize>() > MAX_LYRICS { return Err(tr!("the words are too long to be a song")); }
    let verses: Vec<(Verse, Vec<Vec<String>>)> = verses.into_iter()
        .map(|v| { let g = verse_groups(&v.text); (v, g) }).filter(|(_, g)| !g.is_empty()).collect();
    if verses.is_empty() { return Err(tr!("the words have no verses")); }
    let codes: Vec<char> = verses.iter().map(|(v, _)| kind_code(&v.kind)).collect();
    let mut count: HashMap<char, usize> = HashMap::new();
    for c in &codes { *count.entry(*c).or_default() += 1; }

    // Section labels. A lone "c1" is just "Chorus"; verses keep their number ("Verse 1").
    let key = |l: &str| section_name(Some(l)).to_lowercase();
    let (mut names, mut written): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let mut taken: HashSet<String> = HashSet::new();
    let mut next_number: HashMap<String, usize> = HashMap::new();
    for ((v, _), &code) in verses.iter().zip(&codes) {
        let base = kind_name(code);
        // Kept short so a "(2)" suffix still fits on the label line and keeps names apart.
        let short: String = v.label.chars().take(20).collect();
        let own: String = v.name.trim().chars().take(30).collect();
        let label = if !own.trim().is_empty() { own.trim().to_string() }
            else if short.is_empty() || (code != 'v' && short == "1" && count[&code] == 1) { base.to_string() }
            else { format!("{base} {short}") };
        // The app finds sections by name, so the same tag twice gets "(2)".
        let mut name = label.clone();
        if taken.contains(&key(&name)) {
            let n = next_number.entry(key(&label)).or_insert(2);
            for _ in 0..=MAX_VERSES {
                name = format!("{label} ({n})");
                *n += 1;
                if !taken.contains(&key(&name)) { break; }
            }
        }
        taken.insert(key(&name));
        written.push(section_name(Some(&name)));
        names.push(name);
    }

    let mut out: Vec<String> = Vec::new();
    for ((_, groups), name) in verses.iter().zip(&names) {
        if !out.is_empty() { out.push(String::new()); }
        out.push(label_line(name));
        out.push(join_groups(groups));
    }

    // verse_order: "v1 c1 v2 c1". A section with a name of its own ("Outro") is found by that name; otherwise a
    // tag is a kind letter and number, and one without a number ("c") is the first verse of that type. Unknown
    // tags are dropped. Verses sharing a tag are all played where the tag is, so none becomes unreachable.
    let tag_of = |i: usize| (codes[i], verses[i].0.label.to_lowercase(), verses[i].0.name.trim().to_lowercase());
    let mut by_tag: HashMap<(char, String, String), Vec<usize>> = HashMap::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let (mut first_of_kind, mut first_unnamed): (HashMap<char, usize>, HashMap<char, usize>) = (HashMap::new(), HashMap::new());
    for i in 0..verses.len() {
        let t = tag_of(i);
        if t.2.is_empty() { first_unnamed.entry(t.0).or_insert(i); } else { by_name.entry(t.2.clone()).or_insert(i); }
        first_of_kind.entry(t.0).or_insert(i);
        by_tag.entry(t).or_default().push(i);
    }
    let mut order: Vec<String> = Vec::new();
    for tag in verse_order.split_whitespace().take(MAX_ORDER_TAGS) {
        let code = letter_code(tag.chars().next());
        let num = tag.char_indices().nth(1).map(|(i, _)| &tag[i..]).unwrap_or("").trim().to_lowercase();
        let unnamed = |label: &str| by_tag.get(&(code, label.to_string(), String::new())).map(|v| v[0]);
        let found = by_name.get(&tag.to_lowercase()).copied().or_else(|| if num.is_empty() {
            first_unnamed.get(&code).or_else(|| first_of_kind.get(&code)).copied()
        } else {
            unnamed(&num).or_else(|| if num == "1" { unnamed("") } else { None })
        });
        let Some(i) = found else { continue };
        for &j in &by_tag[&tag_of(i)] {
            if order.len() < MAX_ORDER { order.push(written[j].clone()); }
        }
    }
    if order == written { order.clear(); }
    Ok((out.join("\n"), order))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Song;
    use crate::slides::{parse_sections, song_order};

    /// The `<verse>` elements of OpenLP-style lyrics XML, in document order. DTDs are refused (no entity tricks).
    fn parse_verses(xml: &str) -> Result<Vec<Verse>, String> {
        let doc = roxmltree::Document::parse(xml.trim_start_matches(is_bom_or_space)).map_err(|e| tr!("the words are not valid XML ({error})", error = e))?;
        let verses: Vec<_> = doc.descendants().filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("verse")).collect();
        if verses.len() > MAX_VERSES { return Err(tr!("the words have too many verses (more than {max})", max = MAX_VERSES)); }
        Ok(verses.into_iter().map(|n| {
            let (mut kind, mut label) = (n.attribute("type").unwrap_or("").trim().to_string(), n.attribute("label").unwrap_or("").trim().to_string());
            // OpenLyrics style `name="v1"` when there is no type.
            if kind.is_empty() {
                let name = n.attribute("name").unwrap_or("").trim();
                if let Some(c) = name.chars().next() { kind = c.to_string(); label = name[c.len_utf8()..].trim().to_string(); }
            }
            Verse { kind, label, text: verse_text(n), name: String::new() }
        }).collect())
    }

    /// Lyrics XML (or plain text) plus a verse order → lyrics and arrangement: the tests' way in to
    /// `lyrics_from_verses`.
    fn lyrics_from_xml(xml: &str, verse_order: &str) -> Result<(String, Vec<String>), String> {
        let trimmed = xml.trim_matches(is_bom_or_space);
        if trimmed.len() > MAX_LYRICS { return Err(tr!("the words are too long to be a song")); }
        if !trimmed.starts_with('<') {
            let out = join_groups(&verse_groups(trimmed));
            return if out.is_empty() { Err(tr!("the words are empty")) } else { Ok((out, vec![])) };
        }
        lyrics_from_verses(parse_verses(trimmed)?, verse_order)
    }

    // Amazing Grace (John Newton, 1779): public domain.
    const GRACE: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<song version="1.0"><lyrics language="en"><verse type="v" label="1"><![CDATA[Amazing grace! how sweet the sound
That saved a wretch like me!
I once was lost, but now am found,
Was blind, but now I see.]]></verse><verse type="v" label="2"><![CDATA['Twas grace that taught my heart to fear,
And grace my fears relieved;
How precious did that grace appear
The hour I first believed!]]></verse></lyrics></song>"#;

    // Invented text, exercising chorus, bridge, optional splits, formatting tags and chords.
    const INVENTED: &str = r#"<song version="1.0"><lyrics><verse type="v" label="1"><![CDATA[{r}Morning{/r} light on the hill
Rivers run & the fields are still]]></verse><verse type="c" label="1"><![CDATA[Sing [G]out, sing [D/F#]out
[---]
Sing it all the day]]></verse><verse type="v" label="2"><![CDATA[Evening comes{br}the lamps are lit]]></verse><verse type="b" label="1"><![CDATA[Over and over]]></verse></lyrics></song>"#;

    #[test]
    fn verses_come_out_of_the_xml_in_order() {
        let v = parse_verses(GRACE).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].kind.as_str(), v[0].label.as_str()), ("v", "1"));
        assert!(v[0].text.starts_with("Amazing grace! how sweet the sound\nThat saved"));
        assert_eq!(v[1].label, "2");
    }

    #[test]
    fn hymn_becomes_labelled_sections() {
        let (lyrics, order) = lyrics_from_xml(GRACE, "").unwrap();
        assert!(lyrics.starts_with("Verse 1\nAmazing grace! how sweet the sound\n"));
        assert!(lyrics.contains("Was blind, but now I see.\n\nVerse 2\n'Twas grace"));
        assert!(order.is_empty());
        let labels: Vec<_> = parse_sections(&lyrics).into_iter().map(|s| s.label).collect();
        assert_eq!(labels, ["Verse 1", "Verse 2"]);
    }

    #[test]
    fn tags_chords_and_splits_are_cleaned() {
        let (lyrics, _) = lyrics_from_xml(INVENTED, "").unwrap();
        assert_eq!(lyrics, "Verse 1\nMorning light on the hill\nRivers run & the fields are still\n\n\
            Chorus\nSing out, sing out\n\nSing it all the day\n\nVerse 2\nEvening comes\nthe lamps are lit\n\nBridge\nOver and over");
        let secs = parse_sections(&lyrics);
        assert_eq!(secs[1].groups, vec![vec!["Sing out, sing out".to_string()], vec!["Sing it all the day".to_string()]]);
    }

    #[test]
    fn verse_order_becomes_the_arrangement() {
        let (lyrics, order) = lyrics_from_xml(INVENTED, "v1 c1 v2 C1 b1 c x9").unwrap();
        assert_eq!(order, ["Verse 1", "Chorus", "Verse 2", "Chorus", "Bridge", "Chorus"]);
        let song = Song { lyrics, arrangement: order.clone(), ..Default::default() };
        let secs = parse_sections(&song.lyrics);
        let got: Vec<_> = song_order(&song, &secs).into_iter().map(|i| secs[i].label.clone()).collect();
        assert_eq!(got, order);
        // Order that is just the written order adds nothing.
        assert!(lyrics_from_xml(INVENTED, "v1 c1 v2 b1").unwrap().1.is_empty());
    }

    #[test]
    fn every_verse_type_has_a_label_the_app_reads() {
        let xml = r#"<song><lyrics><verse type="p" label="1">a</verse><verse type="i" label="1">b</verse><verse type="e" label="1">c</verse><verse type="o" label="1">d</verse><verse type="o" label="2">e</verse><verse type="c" label="1">f</verse><verse type="c" label="2">g</verse><verse type="Verse" label="3">h</verse><verse type="z">i</verse></lyrics></song>"#;
        let (lyrics, _) = lyrics_from_xml(xml, "").unwrap();
        let labels: Vec<_> = parse_sections(&lyrics).into_iter().map(|s| s.label).collect();
        assert_eq!(labels, ["Pre-Chorus", "Intro", "Ending", "Other 1", "Other 2", "Chorus 1", "Chorus 2", "Verse 3", "Other"]);
    }

    #[test]
    fn same_tag_twice_keeps_both_verses() {
        let xml = r#"<song><lyrics><verse type="v" label="1">first</verse><verse type="v" label="1">second</verse></lyrics></song>"#;
        let (lyrics, _) = lyrics_from_xml(xml, "").unwrap();
        let secs = parse_sections(&lyrics);
        assert_eq!(secs.len(), 2);
        assert_ne!(secs[0].label, secs[1].label);
    }

    #[test]
    fn plain_text_lyrics_are_kept() {
        let (lyrics, _) = lyrics_from_xml("Line one\r\nLine two\r\n\r\n\r\nLine three", "").unwrap();
        assert_eq!(lyrics, "Line one\nLine two\n\nLine three");
    }

    #[test]
    fn malformed_xml_is_an_error_not_a_panic() {
        for bad in ["<song><lyrics><verse type=\"v\">x", "<song><lyrics></lyrics></song>", "<song>", "<?xml version='1.0'?>",
            "<!DOCTYPE x [<!ENTITY a \"aaaa\">]><song><lyrics><verse>&a;</verse></lyrics></song>", "<song><lyrics><verse type=\"v\" label=\"1\">   </verse></lyrics></song>"] {
            assert!(lyrics_from_xml(bad, "v1").is_err(), "{bad}");
        }
    }

    fn labels(lyrics: &str) -> Vec<String> { parse_sections(lyrics).into_iter().map(|s| s.label).collect() }
    fn wrap(verses: &str) -> String { format!("<song version=\"1.0\"><lyrics>{verses}</lyrics></song>") }

    #[test]
    fn a_bom_before_or_after_whitespace_still_reads_as_xml() {
        for xml in [format!("\u{feff}{GRACE}"), format!(" \u{feff}{GRACE}"), format!("\u{feff} \n{GRACE}")] {
            let (lyrics, _) = lyrics_from_xml(&xml, "").unwrap();
            assert!(lyrics.starts_with("Verse 1\nAmazing grace!"), "{lyrics}");
        }
    }

    #[test]
    fn openlyrics_lines_keep_their_line_breaks() {
        let xml = r#"<song xmlns="http://openlyrics.info/namespace/2009/song"><lyrics><verse name="v1">
            <lines>
              Amazing grace! how sweet the sound<br/>
              That saved a wretch like me!
            </lines>
            <lines><comment>quiet</comment>I once was lost, but now am found,<br/>Was blind, but now I see.</lines>
          </verse><verse name="c"><lines><line>Line one</line><line>Line two</line></lines></verse></lyrics></song>"#;
        let (lyrics, _) = lyrics_from_xml(xml, "").unwrap();
        assert_eq!(lyrics, "Verse 1\nAmazing grace! how sweet the sound\nThat saved a wretch like me!\n\n\
            I once was lost, but now am found,\nWas blind, but now I see.\n\nChorus\nLine one\nLine two");
    }

    #[test]
    fn pretty_printed_xml_around_cdata_is_fine() {
        let xml = "<song>\n  <lyrics>\n    <verse type=\"v\" label=\"1\">\n      <![CDATA[One\nTwo]]>\n    </verse>\n  </lyrics>\n</song>";
        assert_eq!(lyrics_from_xml(xml, "").unwrap().0, "Verse 1\nOne\nTwo");
    }

    #[test]
    fn chord_lines_and_mid_line_chords_leave_clean_text() {
        let xml = wrap("<verse type=\"v\" label=\"1\"><![CDATA[[G]  [D/F#]\nSing [G] out loud\nand [Em]  sing again[--}{--]Second [B♭]half [F♯m]here\n[H7]Ger[Hm]man [Fis]notes [Do]and [em]low\n[C]\n[Am7]]]></verse>");
        let (lyrics, _) = lyrics_from_xml(&xml, "").unwrap();
        assert_eq!(lyrics, "Verse 1\nSing out loud\nand sing again\n\nSecond half here\nGerman notes and low");
        assert_eq!(parse_sections(&lyrics)[0].groups.len(), 2);
    }

    #[test]
    fn text_that_only_looks_like_markup_is_kept() {
        let xml = wrap("<verse type=\"v\" label=\"1\"><![CDATA[{Repeat} sing it {x2}\n[Ending] [Bridge] {st}bold{/st} {it}lean{/it}{/r}]]></verse>");
        let (lyrics, _) = lyrics_from_xml(&xml, "").unwrap();
        assert_eq!(lyrics, "Verse 1\n{Repeat} sing it {x2}\n[Ending] [Bridge] bold lean");
    }

    #[test]
    fn multibyte_lyrics_and_labels_survive() {
        let xml = wrap("<verse type=\"v\" label=\"2ä\"><![CDATA[Grâce — ünïcödé [G]wörld {ü} [日本]\n日本語の歌]]></verse>");
        let (lyrics, _) = lyrics_from_xml(&xml, "").unwrap();
        assert_eq!(lyrics, "[Verse 2ä]\nGrâce — ünïcödé wörld {ü} [日本]\n日本語の歌");
        assert_eq!(labels(&lyrics), ["Verse 2ä"]);
    }

    #[test]
    fn old_full_verse_type_names_are_understood() {
        let xml = wrap(r#"<verse type="Strophe" label="1">a</verse><verse type="Refrain" label="1">b</verse><verse type="Coro" label="2">c</verse><verse type="Vers" label="2">d</verse><verse type="Bridge" label="1">e</verse>"#);
        assert_eq!(labels(&lyrics_from_xml(&xml, "").unwrap().0), ["Verse 1", "Chorus 1", "Chorus 2", "Verse 2", "Bridge"]);
    }

    #[test]
    fn a_duplicated_tag_keeps_both_verses_in_the_arrangement() {
        let xml = wrap(r#"<verse type="v" label="1">first</verse><verse type="v" label="1">second</verse><verse type="c" label="1">sing along</verse>"#);
        let (lyrics, order) = lyrics_from_xml(&xml, "v1 c1 v1").unwrap();
        assert_eq!(labels(&lyrics), ["Verse 1", "Verse 1 (2)", "Chorus"]);
        assert_eq!(order, ["Verse 1", "Verse 1 (2)", "Chorus", "Verse 1", "Verse 1 (2)"]);
    }

    #[test]
    fn a_numbered_tag_finds_an_unlabelled_verse() {
        let xml = wrap(r#"<verse type="v" label="1">verse</verse><verse type="c">chorus</verse>"#);
        assert_eq!(lyrics_from_xml(&xml, "c1 v1").unwrap().1, ["Chorus", "Verse 1"]);
    }

    #[test]
    fn hostile_lyrics_finish_quickly() {
        let start = std::time::Instant::now();
        // ~27k copies of the same tag, just under the size cap: refused, not a hang.
        let many = wrap(&"<verse type=\"v\" label=\"1\">x</verse>".repeat(27_000));
        assert!(many.len() < MAX_LYRICS);
        assert!(lyrics_from_xml(&many, "v1 ".repeat(5000).as_str()).unwrap_err().contains("too many"));
        // The most verses allowed, all with the same tag, and a long verse_order.
        let dup = wrap(&"<verse type=\"v\" label=\"1\">x</verse>".repeat(MAX_VERSES));
        let (lyrics, order) = lyrics_from_xml(&dup, &"v1 ".repeat(100_000)).unwrap();
        let l = labels(&lyrics);
        assert_eq!(l.len(), MAX_VERSES);
        assert_eq!(l.iter().collect::<std::collections::HashSet<_>>().len(), MAX_VERSES);
        assert!(!order.is_empty());
        // Labels too long to number still finish.
        let long = wrap(&format!("<verse type=\"v\" label=\"{0}\">x</verse>", "a".repeat(60)).repeat(3));
        assert_eq!(labels(&lyrics_from_xml(&long, "").unwrap().0).len(), 3);
        // Unclosed brackets everywhere.
        for c in ["[", "{"] { assert!(lyrics_from_xml(&c.repeat(900_000), "").is_ok()); }
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "{:?}", start.elapsed());
        assert!(lyrics_from_xml(&"a".repeat(MAX_LYRICS + 1), "").unwrap_err().contains("too long"));
    }
}
