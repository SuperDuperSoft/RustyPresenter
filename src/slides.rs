//! Turns service items into slides.
use crate::model::*;

/// Is this line a section heading such as `Verse 1`, `[Chorus]`, `Pre-Chorus 2`, `Bridge`?
pub fn section_label(line: &str) -> Option<String> {
    let t = line.trim();
    if t.is_empty() || t.len() > 40 { return None; }
    let bracketed = t.starts_with('[') && t.ends_with(']');
    let inner = t.trim_matches(['[', ']']).trim().trim_end_matches(':').trim();
    if bracketed && !inner.is_empty() { return Some(inner.to_string()); }
    let lower = inner.to_ascii_lowercase();
    let head = lower.split_whitespace().next().unwrap_or("");
    const WORDS: &[&str] = &["verse", "chorus", "bridge", "intro", "outro", "ending", "end", "tag", "interlude", "refrain",
        "pre-chorus", "prechorus", "instrumental", "vamp", "turnaround", "coda", "misc", "post-chorus"];
    let known = WORDS.contains(&head) || lower.starts_with("pre chorus") || lower.starts_with("post chorus");
    // The rest must be a short number/letter suffix ("Verse 1", "Chorus 2a"), not a lyric like "Verse after verse".
    let rest_ok = inner.split_whitespace().skip(if lower.starts_with("pre chorus") || lower.starts_with("post chorus") { 2 } else { 1 })
        .all(|w| w.len() <= 3 && w.chars().all(|c| c.is_ascii_alphanumeric()));
    (known && rest_ok).then(|| inner.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub label: String,
    /// Manual slide groups (split by blank lines inside the section).
    pub groups: Vec<Vec<String>>,
}

pub fn parse_sections(lyrics: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;
    let mut group: Vec<String> = Vec::new();
    let close_group = |sec: &mut Option<Section>, group: &mut Vec<String>| {
        if !group.is_empty() {
            sec.get_or_insert_with(|| Section { label: String::new(), groups: vec![] }).groups.push(std::mem::take(group));
        }
    };
    for raw in lyrics.replace("\r\n", "\n").lines() {
        let line = raw.trim();
        if let Some(label) = section_label(line) {
            close_group(&mut current, &mut group);
            if let Some(s) = current.take() { if !s.groups.is_empty() { out.push(s); } }
            current = Some(Section { label, groups: vec![] });
        } else if line.is_empty() {
            close_group(&mut current, &mut group);
        } else {
            group.push(line.to_string());
        }
    }
    close_group(&mut current, &mut group);
    if let Some(s) = current { if !s.groups.is_empty() { out.push(s); } }
    // Unlabeled songs: number the blocks so they still navigate nicely.
    for (i, s) in out.iter_mut().enumerate() { if s.label.is_empty() { s.label = format!("Part {}", i + 1); } }
    out
}

/// Split lines into pages of at most `n` lines.
pub fn chunk_lines(lines: &[String], n: usize) -> Vec<String> {
    lines.chunks(n.max(1)).map(|c| c.join("\n")).collect()
}

pub fn song_order(song: &Song, sections: &[Section]) -> Vec<usize> {
    let find = |label: &str| sections.iter().position(|s| s.label.eq_ignore_ascii_case(label.trim()));
    let order: Vec<usize> = song.arrangement.iter().filter_map(|l| find(l)).collect();
    if order.is_empty() { (0..sections.len()).collect() } else { order }
}

pub fn song_slides(song: &Song, settings: &Settings) -> Vec<Slide> {
    let sections = parse_sections(&song.lyrics);
    let mut slides = Vec::new();
    let credit = credit_line(song, settings);
    for idx in song_order(song, &sections) {
        let s = &sections[idx];
        for g in &s.groups {
            let pages = chunk_lines(g, settings.lines_per_slide as usize);
            let n = pages.len();
            for (p, text) in pages.into_iter().enumerate() {
                let label = if n > 1 { format!("{} ({}/{})", s.label, p + 1, n) } else { s.label.clone() };
                slides.push(Slide { label, content: SlideContent::Text { text, footer: None, reference: None }, notes: String::new() });
            }
        }
    }
    if let Some(credit) = credit {
        let pos = match settings.credits.as_str() { "first" => Some(0), "last" => slides.len().checked_sub(1), _ => None };
        if let Some(SlideContent::Text { footer, .. }) = pos.and_then(|p| slides.get_mut(p)).map(|s| &mut s.content) { *footer = Some(credit); }
    }
    slides
}

fn credit_line(song: &Song, settings: &Settings) -> Option<String> {
    if settings.credits == "none" { return None; }
    let mut parts = vec![song.title.clone()];
    if !song.writers.trim().is_empty() { parts.push(song.writers.trim().to_string()); }
    if !song.copyright.trim().is_empty() { parts.push(song.copyright.trim().to_string()); }
    let mut line = parts.join(" · ");
    if !song.ccli.trim().is_empty() { line.push_str(&format!(" · CCLI Song #{}", song.ccli.trim())); }
    if !settings.church_ccli.trim().is_empty() { line.push_str(&format!(" · CCLI License #{}", settings.church_ccli.trim())); }
    Some(line)
}

/// Group verses into slides. `per_slide` 0 = automatic (by length).
pub fn scripture_slides(verses: &[VerseText], per_slide: u8, translation: &str) -> Vec<Slide> {
    const AUTO_CHARS: usize = 330;
    let mut slides = Vec::new();
    let mut group: Vec<&VerseText> = Vec::new();
    let flush = |group: &mut Vec<&VerseText>, slides: &mut Vec<Slide>| {
        if group.is_empty() { return; }
        let text = group.iter().map(|v| format!("{}{}", superscript(verse_num(&v.reference)), v.text)).collect::<Vec<_>>().join(" ");
        let reference = format!("{} ({translation})", join_refs(group));
        slides.push(Slide { label: join_refs(group), content: SlideContent::Text { text, footer: None, reference: Some(reference) }, notes: String::new() });
        group.clear();
    };
    for v in verses {
        if v.text.chars().count() > AUTO_CHARS {
            // One long verse: split at sentence or clause ends, never dropping words.
            flush(&mut group, &mut slides);
            let parts = split_long(&v.text, AUTO_CHARS);
            let n = parts.len();
            for (i, p) in parts.into_iter().enumerate() {
                let label = if i == 0 { v.reference.clone() } else { format!("{} (cont.)", v.reference) };
                let text = if i == 0 { format!("{}{}", superscript(verse_num(&v.reference)), p) } else { p };
                let reference = format!("{}{} ({translation})", v.reference, if i > 0 { " (cont.)" } else { "" });
                slides.push(Slide { label: if n > 1 { label } else { v.reference.clone() }, content: SlideContent::Text { text, footer: None, reference: Some(reference) }, notes: String::new() });
            }
            continue;
        }
        let len: usize = group.iter().map(|g| g.text.chars().count()).sum::<usize>() + v.text.chars().count();
        let full = if per_slide == 0 { len > AUTO_CHARS } else { group.len() >= per_slide as usize };
        if full { flush(&mut group, &mut slides); }
        group.push(v);
    }
    flush(&mut group, &mut slides);
    slides
}

fn verse_num(reference: &str) -> &str { reference.rsplit(':').next().unwrap_or("") }

fn superscript(n: &str) -> String {
    let s: String = n.chars().map(|c| match c { '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹', o => o }).collect();
    if s.is_empty() { s } else { format!("{s}\u{2009}") }
}

/// "John 3:16", "John 3:16–18", "Psalm 23:1, 4–6", "John 1:51–2:2".
pub fn join_refs(group: &[&VerseText]) -> String {
    let parsed: Vec<(String, u32, u32)> = group.iter().filter_map(|v| {
        let (book, cv) = v.reference.rsplit_once(' ')?;
        let (c, vs) = cv.split_once(':')?;
        Some((book.to_string(), c.parse().ok()?, vs.parse().ok()?))
    }).collect();
    if parsed.len() != group.len() || parsed.is_empty() { return group.iter().map(|v| v.reference.as_str()).collect::<Vec<_>>().join(", "); }
    let book = &parsed[0].0;
    // runs of consecutive verses
    let mut runs: Vec<((u32, u32), (u32, u32))> = vec![];
    for &(_, c, v) in &parsed {
        if let Some(last) = runs.last_mut() {
            let (lc, lv) = last.1;
            if (c == lc && v == lv + 1) || (c == lc + 1 && v == 1) { last.1 = (c, v); continue; }
        }
        runs.push(((c, v), (c, v)));
    }
    let mut out = String::new();
    let mut chapter = 0;
    for (i, (a, b)) in runs.iter().enumerate() {
        if i > 0 { out.push_str(", "); }
        if a.0 != chapter { out.push_str(&format!("{}:", a.0)); chapter = a.0; }
        out.push_str(&a.1.to_string());
        if a != b {
            if b.0 != a.0 { out.push_str(&format!("–{}:{}", b.0, b.1)); chapter = b.0; } else { out.push_str(&format!("–{}", b.1)); }
        }
    }
    format!("{book} {out}")
}

pub fn split_long(text: &str, max: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut len = 0;
    let mut last_break: Option<usize> = None; // index in cur after which a clause ends
    for w in words {
        cur.push(w);
        len += w.chars().count() + 1;
        if w.ends_with(['.', ';', ':', ',', '?', '!']) { last_break = Some(cur.len()); }
        if len > max {
            let cut = last_break.filter(|&b| b * 2 >= cur.len()).unwrap_or(cur.len() - 1).max(1);
            let rest = cur.split_off(cut);
            out.push(cur.join(" "));
            cur = rest;
            len = cur.iter().map(|w| w.chars().count() + 1).sum();
            last_break = None;
        }
    }
    if !cur.is_empty() { out.push(cur.join(" ")); }
    out
}

pub fn text_slides(title: &str, body: &str) -> Vec<Slide> {
    let mut slides = Vec::new();
    for (i, block) in body.replace("\r\n", "\n").split("\n\n").map(str::trim).filter(|b| !b.is_empty()).enumerate() {
        slides.push(Slide { label: format!("{title} {}", i + 1), content: SlideContent::Text { text: block.to_string(), footer: None, reference: None }, notes: String::new() });
    }
    if slides.is_empty() { slides.push(Slide { label: title.to_string(), content: SlideContent::Text { text: title.to_string(), footer: None, reference: None }, notes: String::new() }); }
    slides
}

/// Title, kind and slides for a service item.
pub fn build_item(data: &Data, item: &ServiceItem) -> (String, &'static str, Vec<Slide>) {
    let s = &data.settings;
    let (title, kind, mut slides) = match &item.kind {
        ItemKind::Song { song_id } => match data.songs.iter().find(|x| x.id == *song_id) {
            Some(song) => (song.title.clone(), "song", song_slides(song, s)),
            None => ("Missing song".into(), "song", vec![]),
        },
        ItemKind::Scripture { reference, translation, verses, per_slide, abbr } => {
            let label = if abbr.is_empty() { translation } else { abbr };
            // A saved passage in a blocked translation (Bible library design §4) shows the message, never its text.
            if crate::bible::blocklist::blocked_id(translation) || crate::bible::blocklist::blocked_id(label) {
                let v = [VerseText { reference: reference.clone(), text: crate::bible::strings::BLOCKED.into() }];
                (format!("{reference} ({label})"), "scripture", scripture_slides(&v, 1, label))
            } else {
                (format!("{reference} ({label})"), "scripture", scripture_slides(verses, *per_slide, label))
            }
        }
        ItemKind::Media { media_id, autoplay, end, looped } => match data.media.iter().find(|m| m.id == *media_id) {
            Some(m) => {
                let content = match m.kind {
                    MediaKind::Video => SlideContent::Video { asset: m.asset.clone(), autoplay: *autoplay, end: *end },
                    MediaKind::Image => SlideContent::Still { asset: m.asset.clone() },
                    MediaKind::Audio => SlideContent::Audio { asset: m.asset.clone(), looped: *looped },
                };
                let kind = match m.kind { MediaKind::Video => "video", MediaKind::Image => "image", MediaKind::Audio => "audio" };
                (m.name.clone(), kind, vec![Slide { label: m.name.clone(), content, notes: String::new() }])
            }
            None => ("Missing media".into(), "video", vec![]),
        },
        ItemKind::Deck { deck_id } => match data.decks.iter().find(|d| d.id == *deck_id) {
            Some(d) => (d.title.clone(), "deck", d.slides.iter().enumerate().map(|(i, a)| Slide { label: format!("Slide {}", i + 1), content: SlideContent::Image { asset: a.clone() }, notes: String::new() }).collect()),
            None => ("Missing slides".into(), "deck", vec![]),
        },
        ItemKind::Text { title, body } => (title.clone(), "text", text_slides(title, body)),
    };
    if !item.notes.is_empty() { for sl in &mut slides { if sl.notes.is_empty() { sl.notes = item.notes.clone(); } } }
    (title, kind, slides)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripture(translation: &str, abbr: &str) -> ServiceItem {
        let verses = vec![VerseText { reference: "John 1:1".into(), text: "In the beginning".into() }];
        ServiceItem { id: uuid::Uuid::new_v4(), kind: ItemKind::Scripture { reference: "John 1:1".into(), translation: translation.into(), verses, per_slide: 0, abbr: abbr.into() }, background: None, notes: String::new() }
    }

    fn texts(slides: &[Slide]) -> Vec<String> {
        slides.iter().map(|s| match &s.content { SlideContent::Text { text, .. } => text.clone(), _ => String::new() }).collect()
    }

    #[test]
    fn online_bible_items_show_their_abbreviation_not_their_id() {
        let (title, _, slides) = build_item(&Data::default(), &scripture("yv:149", "RVR1960"));
        assert_eq!(title, "John 1:1 (RVR1960)");
        assert!(matches!(&slides[0].content, SlideContent::Text { reference: Some(r), .. } if r.ends_with("(RVR1960)")), "{:?}", slides[0].content);
        let (title, _, _) = build_item(&Data::default(), &scripture("KJV", ""));
        assert_eq!(title, "John 1:1 (KJV)", "old items have no abbr");
    }

    #[test]
    fn a_saved_passage_in_a_blocked_translation_never_shows_its_text() {
        for (t, a) in [("NWT", ""), ("yv:1849", "TPT"), ("yv:5", "JST")] {
            let (_, _, slides) = build_item(&Data::default(), &scripture(t, a));
            assert_eq!(texts(&slides).len(), 1);
            assert!(texts(&slides)[0].contains(crate::bible::strings::BLOCKED), "{t}");
            assert!(!texts(&slides)[0].contains("In the beginning"));
        }
    }

    #[test]
    fn labels_are_recognized_but_lyrics_are_not() {
        assert_eq!(section_label("Verse 1").as_deref(), Some("Verse 1"));
        assert_eq!(section_label("[Chorus]").as_deref(), Some("Chorus"));
        assert_eq!(section_label("Pre-Chorus 2").as_deref(), Some("Pre-Chorus 2"));
        assert_eq!(section_label("Bridge:").as_deref(), Some("Bridge"));
        assert_eq!(section_label("Verse after verse I sing"), None);
        assert_eq!(section_label("Amazing grace how sweet"), None);
    }

    #[test]
    fn song_slides_follow_arrangement_and_blank_line_breaks() {
        let song = Song { title: "T".into(), lyrics: "Verse 1\na\nb\nc\n\nd\nChorus\nx\ny".into(), arrangement: vec!["Chorus".into(), "Verse 1".into(), "Chorus".into()], ..Default::default() };
        let settings = Settings { credits: "none".into(), ..Default::default() };
        let slides = song_slides(&song, &settings);
        let texts: Vec<_> = slides.iter().map(|s| match &s.content { SlideContent::Text { text, .. } => text.clone(), _ => unreachable!() }).collect();
        assert_eq!(texts, vec!["x\ny", "a\nb", "c", "d", "x\ny"]);
        assert_eq!(slides[1].label, "Verse 1 (1/2)");
    }

    #[test]
    fn credits_on_first_slide() {
        let song = Song { title: "T".into(), writers: "W".into(), ccli: "123".into(), lyrics: "Verse 1\na".into(), ..Default::default() };
        let settings = Settings { church_ccli: "999".into(), ..Default::default() };
        let slides = song_slides(&song, &settings);
        match &slides[0].content { SlideContent::Text { footer: Some(f), .. } => { assert!(f.contains("CCLI Song #123")); assert!(f.contains("License #999")); } c => panic!("{c:?}") }
    }

    #[test]
    fn exact_discontiguous_and_cross_chapter_labels() {
        let v = |r: &str| VerseText { reference: r.into(), text: "t".into() };
        let a = [v("Psalm 23:1"), v("Psalm 23:4"), v("Psalm 23:5"), v("Psalm 23:6")];
        assert_eq!(join_refs(&a.iter().collect::<Vec<_>>()), "Psalm 23:1, 4–6");
        let b = [v("John 1:51"), v("John 2:1"), v("John 2:2")];
        assert_eq!(join_refs(&b.iter().collect::<Vec<_>>()), "John 1:51–2:2");
    }

    #[test]
    fn long_verse_split_keeps_every_word_in_order() {
        let text = "And it came to pass, when the king sat in his house, and the LORD had given him rest round about from all his enemies; That the king said unto Nathan the prophet, See now, I dwell in an house of cedar, but the ark of God dwelleth within curtains. And more words here to make it long enough to split twice over for the test.";
        let parts = split_long(text, 120);
        assert!(parts.len() >= 2);
        assert_eq!(parts.join(" ").split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>());
    }
}
