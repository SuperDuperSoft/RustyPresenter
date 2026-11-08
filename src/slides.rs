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
    let short = |w: &str| w.len() <= 3 && w.chars().all(|c| c.is_ascii_alphanumeric());
    let rest_ok = inner.split_whitespace().skip(if lower.starts_with("pre chorus") || lower.starts_with("post chorus") { 2 } else { 1 }).all(short);
    if known && rest_ok { return Some(inner.to_string()) }
    // A heading word of the language the app is shown in ("Coro", "Verso 1"), with the same short suffix.
    let lower = inner.to_lowercase();
    let (_, rest) = crate::sections::translated_head(&lower)?;
    rest.split_whitespace().all(short).then(|| inner.to_string())
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

/// A slide group's label and pages (N19): a `{Label}` first line is the label, not words, and a group that is only
/// a label is one empty slide.
pub fn group_pages(group: &[String], lines_per_slide: usize) -> (Option<String>, Vec<String>) {
    let (tag, lines) = match group.first().and_then(|l| crate::sections::label_line(l)) {
        Some(t) => (Some(t.to_string()), &group[1..]),
        None => (None, group),
    };
    let mut pages = chunk_lines(lines, lines_per_slide);
    if pages.is_empty() { pages.push(String::new()) }
    (tag, pages)
}

pub fn song_order(song: &Song, sections: &[Section]) -> Vec<usize> { order_of(&song.arrangement, sections) }

/// The written sections `order` plays (an empty or unmatched order plays them as written).
pub fn order_of(order: &[String], sections: &[Section]) -> Vec<usize> {
    let labels: Vec<String> = sections.iter().map(|s| s.label.clone()).collect();
    crate::sections::resolve_order(order, &labels)
}

/// The order a service item plays its song in: the item's own order for this service, else the song's usual one.
pub fn item_order<'a>(item: &'a ServiceItem, song: &'a Song) -> &'a [String] {
    match &item.kind {
        ItemKind::Song { arrangement: Some(own), .. } => own,
        _ => &song.arrangement,
    }
}

pub fn song_slides(song: &Song, settings: &Settings) -> Vec<Slide> { song_slides_in(song, &song.arrangement, settings) }

/// `song_slides` in `order` instead of the song's usual order.
pub fn song_slides_in(song: &Song, order: &[String], settings: &Settings) -> Vec<Slide> {
    let sections = parse_sections(&song.lyrics);
    let lps = settings.lines_per_slide as usize;
    let mut slides = Vec::new();
    let credit = credit_line(song, settings);
    let order = order_of(order, &sections);
    for &idx in &order {
        let s = &sections[idx];
        for g in &s.groups {
            let (tag, pages) = group_pages(g, lps);
            let n = pages.len();
            for (p, text) in pages.into_iter().enumerate() {
                let label = if n > 1 { format!("{} ({}/{})", s.label, p + 1, n) } else { s.label.clone() };
                let mut slide = Slide::new(label, SlideContent::Text { text, footer: None, reference: None, size: 0, parallel: None });
                if p == 0 { slide.tag = tag.clone() }
                slides.push(slide);
            }
        }
    }
    // Each slide's section (FEAT-4), counted across the section's blank-line groups.
    let counts: Vec<(&str, usize)> = sections.iter().map(|s| (s.label.as_str(), s.groups.iter().map(|g| group_pages(g, lps).1.len()).sum())).collect();
    for (slide, r) in slides.iter_mut().zip(crate::sections::song_refs(&counts, &order)) { slide.section = Some(r) }
    if let Some(credit) = credit {
        let last = slides.len().checked_sub(1);
        let at: &[Option<usize>] = match settings.credits.as_str() { "first" => &[Some(0)], "last" => &[last], "both" => &[Some(0), last], _ => &[] };
        for p in at.iter().flatten() {
            if let Some(SlideContent::Text { footer, .. }) = slides.get_mut(*p).map(|s| &mut s.content) { *footer = Some(credit.clone()); }
        }
    }
    slides
}

/// A passage's credit goes on its first or last slide, as Settings says for songs (Bible library spec §10). With
/// credits off it still shows, on the first slide, for a Bible that is not public domain.
fn put_scripture_credit(slides: &mut [Slide], credit: &BibleCredit, settings: &Settings) {
    if credit.line.trim().is_empty() { return }
    let pos = match settings.credits.as_str() {
        "last" => slides.len().checked_sub(1),
        "none" if !credit.required => None,
        _ => Some(0),
    };
    if let Some(SlideContent::Text { footer, .. }) = pos.and_then(|p| slides.get_mut(p)).map(|s| &mut s.content) { *footer = Some(credit.line.clone()) }
}

/// The credit line when Settings → Songs leaves the template empty: the line songs have always had.
pub const DEFAULT_CREDIT_TEMPLATE: &str = "{title} · {author} · {copyright} · CCLI Song #{ccli} · CCLI License #{license}";

/// The words each template gap stands for (N36), in the order Settings lists them.
pub const CREDIT_GAPS: [&str; 8] = ["title", "artist", "author", "publisher", "year", "copyright", "ccli", "license"];

fn credit_line(song: &Song, settings: &Settings) -> Option<String> {
    if settings.credits == "none" { return None; }
    let template = if settings.credit_template.trim().is_empty() { DEFAULT_CREDIT_TEMPLATE } else { settings.credit_template.as_str() };
    let line = fill_credit(template, song, &settings.church_ccli);
    (!line.is_empty()).then_some(line)
}

/// A song's copyright: as written, else "© year publisher" from its parts.
pub fn song_copyright(song: &Song) -> String {
    let written = song.copyright.trim();
    if !written.is_empty() { return written.to_string() }
    let parts: Vec<&str> = [song.year.trim(), song.publisher.trim()].into_iter().filter(|p| !p.is_empty()).collect();
    if parts.is_empty() { String::new() } else { format!("© {}", parts.join(" ")) }
}

/// Fills a credit template. It is read in parts split by " · " (or " | "): a part whose gaps are all empty is left
/// out with its separator, so "CCLI Song #{ccli}" goes away for a song with no number. A gap the app does not know
/// stays as written.
pub fn fill_credit(template: &str, song: &Song, license: &str) -> String {
    let value = |gap: &str| -> Option<String> {
        Some(match gap {
            "title" => song.title.trim().into(),
            "artist" => song.artist.trim().into(),
            "author" | "writers" => song.writers.trim().into(),
            "publisher" => song.publisher.trim().into(),
            "year" => song.year.trim().into(),
            "copyright" => song_copyright(song),
            "ccli" => song.ccli.trim().into(),
            "license" | "licence" => license.trim().into(),
            _ => return None,
        })
    };
    let sep = [" · ", " | "].into_iter().find(|s| template.contains(s));
    let pieces: Vec<&str> = match sep { Some(s) => template.split(s).collect(), None => vec![template] };
    let parts: Vec<String> = pieces.into_iter().filter_map(|part| {
        let (mut out, mut gaps, mut filled) = (String::new(), 0, 0);
        let mut rest = part;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            match after.find('}').map(|close| (&after[..close], close)) {
                Some((gap, close)) => match value(gap.trim().to_lowercase().as_str()) {
                    Some(v) => { gaps += 1; if !v.is_empty() { filled += 1 } out.push_str(&v); rest = &after[close + 1..] }
                    None => { out.push_str(&rest[open..open + 1 + close + 1]); rest = &after[close + 1..] }
                },
                None => { out.push_str(&rest[open..]); rest = "" }
            }
        }
        out.push_str(rest);
        let out = out.trim().to_string();
        (!out.is_empty() && (gaps == 0 || filled > 0)).then_some(out)
    }).collect();
    parts.join(sep.unwrap_or(""))
}

/// One credit for a passage in several Bibles: their lines in column order, joined with " · ". It must show when any
/// of them must.
fn joined_credit(credits: &[&BibleCredit]) -> Option<BibleCredit> {
    let first = credits.first()?;
    let lines: Vec<&str> = credits.iter().map(|c| c.line.trim()).filter(|l| !l.is_empty()).collect();
    Some(BibleCredit { line: lines.join(" · "), required: credits.iter().any(|c| c.required), ..(*first).clone() })
}

/// The theme facts the scripture planner needs.
#[derive(Clone, Hash)]
struct PlanFacts { size: u32, lower: bool }

impl crate::scripture::planning::PlanTheme for PlanFacts {
    fn scripture_size(&self) -> u32 { self.size }
    fn is_lower_third(&self) -> bool { self.lower }
}

/// Slides of a passage in the main Bible and the Also Bibles (`also`: code and verses, in column order), planned
/// verse by verse so every Bible's slide shows the same verses (Nov 5 spec §3), with the operator's `breaks` (FEAT-5
/// §5.4). A slide too long even at the smallest size is split evenly inside its verses. Sized by the character
/// estimate until the measured planner takes over.
/// Also returns the planner's notes: one per Bible per verse it lacks.
pub fn parallel_slides(verses: &[VerseText], abbr: &str, also: &[(String, &[VerseText])], per_slide: u8, layout: crate::scripture::ParallelLayout, theme: &crate::model::Theme) -> (Vec<Slide>, Vec<String>) {
    planned_slides(verses, abbr, also, per_slide, layout, &Default::default(), theme)
}

/// The planner's input for one passage, as `planned_slides` and the slide breaks dialog both see it.
pub fn scripture_input(verses: &[VerseText], abbr: &str, also: &[(String, &[VerseText])], per_slide: u8, layout: crate::scripture::ParallelLayout, breaks: &crate::scripture::VerseBreaks, theme: &crate::model::Theme) -> crate::scripture::planning::ScriptureInput<impl crate::scripture::planning::PlanTheme + use<>> {
    use crate::scripture::planning::{AlsoInput, ScriptureInput};
    ScriptureInput {
        main: verses.to_vec(),
        main_code: abbr.to_string(),
        main_notice: None,
        also: also.iter().map(|(code, vs)| AlsoInput { code: code.clone(), verses: vs.to_vec(), notice: None }).collect(),
        per_slide,
        layout,
        breaks: breaks.clone(),
        theme: PlanFacts { size: theme.scripture_size, lower: theme.position == "lower" },
        canvas: (1920, 1080),
    }
}

/// `parallel_slides` with the operator's slide breaks. One Bible gets no `parallel` columns.
pub fn planned_slides(verses: &[VerseText], abbr: &str, also: &[(String, &[VerseText])], per_slide: u8, layout: crate::scripture::ParallelLayout, breaks: &crate::scripture::VerseBreaks, theme: &crate::model::Theme) -> (Vec<Slide>, Vec<String>) {
    use crate::scripture::planning::{CharMeasure, plan_scripture};
    let input = scripture_input(verses, abbr, also, per_slide, layout, breaks, theme);
    let planned = plan_scripture(&input, &mut CharMeasure::for_input(&input.theme, input.canvas));
    let slides = planned.slides.into_iter().map(|p| Slide::new(p.label, SlideContent::Text {
        text: p.text, footer: None, reference: Some(p.reference), size: 0,
        parallel: (!also.is_empty()).then(|| crate::scripture::Parallel { main_code: abbr.to_string(), also: p.also, layout }),
    })).collect();
    (slides, planned.notes)
}

/// One Bible's slides with the default theme. `per_slide` 0 = automatic (by length).
pub fn scripture_slides(verses: &[VerseText], per_slide: u8, translation: &str) -> Vec<Slide> {
    planned_slides(verses, translation, &[], per_slide, Default::default(), &Default::default(), &crate::model::Theme::default()).0
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
        slides.push(Slide::new(format!("{title} {}", i + 1), SlideContent::Text { text: block.to_string(), footer: None, reference: None, size: 0, parallel: None }));
    }
    if slides.is_empty() { slides.push(Slide::new(title.to_string(), SlideContent::Text { text: title.to_string(), footer: None, reference: None, size: 0, parallel: None })); }
    slides
}

/// A passage's slides and what the operator should know about them (Nov 5 spec §3): each verse a Bible lacks, and
/// each Bible left out of the slides. The note shows under the slides grid.
pub struct ScriptureBuild { pub slides: Vec<Slide>, pub notes: Vec<String> }

/// `build_item`'s slides of a passage, with its notes. Empty notes for any other item.
pub fn build_scripture(data: &Data, item: &ServiceItem) -> ScriptureBuild {
    let (_, _, slides, notes) = build_item_with_notes(data, item);
    ScriptureBuild { slides, notes }
}

/// Title, kind and slides for a service item.
pub fn build_item(data: &Data, item: &ServiceItem) -> (String, &'static str, Vec<Slide>) {
    let (title, kind, slides, _) = build_item_with_notes(data, item);
    (title, kind, slides)
}

/// `build_item` and the item's notes for the operator (`build_scripture`).
pub fn build_item_with_notes(data: &Data, item: &ServiceItem) -> (String, &'static str, Vec<Slide>, Vec<String>) {
    let s = &data.settings;
    let mut notes = Vec::new();
    let (title, kind, mut slides) = match &item.kind {
        ItemKind::Song { song_id, .. } => match data.songs.iter().find(|x| x.id == *song_id) {
            Some(song) => (song.title.clone(), "song", song_slides_in(song, item_order(item, song), s)),
            None => (crate::tr!("Missing song"), "song", vec![]),
        },
        ItemKind::Scripture { reference, translation, verses, per_slide, text_size, also, layout, breaks } => {
            let abbr_of = |t: &str| data.bible_credit(t).map(|c| c.abbr.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| t.to_string());
            let abbr = abbr_of(translation);
            // Also Bibles with no text here (an online one not fetched yet) are left out of the slides.
            let shown: Vec<(String, &[VerseText])> = also.iter().filter(|a| !a.verses.is_empty()).map(|a| (abbr_of(&a.translation), a.verses.as_slice())).collect();
            if verses.is_empty() && crate::bible::sources::is_online(translation) { notes.push(crate::scripture::strings::text_unreachable(&abbr)) }
            // An online Bible may simply need the internet; any other has left the library.
            notes.extend(also.iter().filter(|a| a.verses.is_empty()).map(|a| {
                if crate::bible::sources::is_online(&a.translation) { crate::scripture::strings::needs_internet(&abbr_of(&a.translation)) }
                else { crate::scripture::strings::bible_removed() }
            }));
            let (mut slides, missing) = planned_slides(verses, &abbr, &shown, *per_slide, *layout, breaks, &s.theme);
            notes.extend(missing);
            if s.theme.position == "lower" && let Some((third, _)) = shown.get(1) { notes.push(crate::scripture::strings::lower_third_left_out(third)) }
            for sl in &mut slides { if let SlideContent::Text { size, .. } = &mut sl.content { *size = *text_size } }
            let credits: Vec<&BibleCredit> = std::iter::once(translation.as_str()).chain(also.iter().filter(|a| !a.verses.is_empty()).map(|a| a.translation.as_str()))
                .filter_map(|t| data.bible_credit(t)).collect();
            if let Some(credit) = joined_credit(&credits) { put_scripture_credit(&mut slides, &credit, s) }
            let codes: Vec<&str> = std::iter::once(abbr.as_str()).chain(shown.iter().map(|(a, _)| a.as_str())).collect();
            (format!("{reference} ({})", codes.join(" · ")), "scripture", slides)
        }
        ItemKind::Media { media_id, autoplay, end, looped } => match data.media.iter().find(|m| m.id == *media_id) {
            Some(m) => {
                let content = match m.kind {
                    MediaKind::Video => SlideContent::Video { asset: m.asset.clone(), autoplay: *autoplay, end: *end },
                    MediaKind::Image => SlideContent::Still { asset: m.asset.clone() },
                    MediaKind::Audio => SlideContent::Audio { asset: m.asset.clone(), looped: *looped },
                };
                let kind = match m.kind { MediaKind::Video => "video", MediaKind::Image => "image", MediaKind::Audio => "audio" };
                (m.name.clone(), kind, vec![Slide::new(m.name.clone(), content)])
            }
            None => (crate::tr!("Missing media"), "video", vec![]),
        },
        ItemKind::Deck { deck_id } => match data.decks.iter().find(|d| d.id == *deck_id) {
            Some(d) => (d.title.clone(), "deck", d.slides.iter().enumerate().map(|(i, a)| Slide {
                notes: d.notes.get(i).cloned().unwrap_or_default(),
                ..Slide::new(crate::tr!("Slide {n}", n = i + 1), SlideContent::Image { asset: a.clone() })
            }).collect()),
            None => (crate::tr!("Missing slides"), "deck", vec![]),
        },
        ItemKind::Text { title, body, .. } => (title.clone(), "text", text_slides(title, body)),
        ItemKind::Blank { label, want } => (crate::planning::blank_title(label, *want), "blank", vec![]),
        ItemKind::Header { title, .. } => (title.clone(), "header", vec![]),
    };
    if !item.notes.is_empty() { for sl in &mut slides { if sl.notes.is_empty() { sl.notes = item.notes.clone(); } } }
    (title, kind, slides, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn spanish_heading_words_are_sections_in_spanish() {
        let lyrics = "Verso 1\nSublime gracia\nCoro\nYa libre soy\nCoro de ángeles canta\nVerse 2\nb";
        crate::i18n::testing::with_catalog(crate::sections::kind_tests_es(), || {
            assert_eq!(section_label("Coro").as_deref(), Some("Coro"));
            assert_eq!(section_label("Verso 2:").as_deref(), Some("Verso 2"));
            assert_eq!(section_label("Coro de ángeles canta"), None, "a lyric that starts with the word");
            let secs = parse_sections(lyrics);
            assert_eq!(secs.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Verso 1", "Coro", "Verse 2"]);
            let song = Song { lyrics: lyrics.into(), ..Default::default() };
            let slides = song_slides(&song, &Settings { credits: "none".into(), ..Default::default() });
            let refs: Vec<_> = slides.iter().map(|s| s.section.as_ref().map(|r| (r.kind, r.number)).unwrap()).collect();
            use crate::sections::SectionKind::*;
            assert_eq!(refs, [(Verse, Some(1)), (Chorus, None), (Verse, Some(2))]);
        });
        assert_eq!(section_label("Coro"), None, "English only, outside Spanish");
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

    fn song_item(song: uuid::Uuid, own: Option<&[&str]>) -> ServiceItem {
        let arrangement = own.map(|o| o.iter().map(|s| s.to_string()).collect());
        ServiceItem { id: uuid::Uuid::new_v4(), kind: ItemKind::Song { song_id: song, arrangement }, background: None, notes: String::new(), theme: None }
    }

    fn usual_song() -> Song {
        Song { id: uuid::Uuid::new_v4(), title: "T".into(), lyrics: "Verse 1\na\nChorus\nx\nVerse 2\nb".into(), arrangement: vec!["Verse 1".into(), "Chorus".into(), "Verse 2".into(), "Chorus".into()], ..Default::default() }
    }

    #[test]
    fn item_order_prefers_item_override() {
        let song = usual_song();
        assert_eq!(item_order(&song_item(song.id, None), &song), song.arrangement.as_slice());
        assert_eq!(item_order(&song_item(song.id, Some(&["Chorus"])), &song), ["Chorus"]);
    }

    fn built_labels(data: &Data, item: &ServiceItem) -> Vec<String> { build_item(data, item).2.into_iter().map(|s| s.label).collect() }

    #[test]
    fn build_item_uses_item_override() {
        let song = usual_song();
        let data = Data { songs: vec![song.clone()], ..Default::default() };
        assert_eq!(built_labels(&data, &song_item(song.id, None)), ["Verse 1", "Chorus", "Verse 2", "Chorus"]);
        assert_eq!(built_labels(&data, &song_item(song.id, Some(&["Chorus", "Verse 2"]))), ["Chorus", "Verse 2"]);
    }

    #[test]
    fn empty_override_list_means_written_order() {
        let song = usual_song();
        let data = Data { songs: vec![song.clone()], ..Default::default() };
        assert_eq!(built_labels(&data, &song_item(song.id, Some(&[]))), ["Verse 1", "Chorus", "Verse 2"]);
    }

    #[test]
    fn credits_on_first_slide() {
        let song = Song { title: "T".into(), writers: "W".into(), ccli: "123".into(), lyrics: "Verse 1\na".into(), ..Default::default() };
        let settings = Settings { church_ccli: "999".into(), ..Default::default() };
        let slides = song_slides(&song, &settings);
        match &slides[0].content { SlideContent::Text { footer: Some(f), .. } => { assert!(f.contains("CCLI Song #123")); assert!(f.contains("License #999")); } c => panic!("{c:?}") }
    }

    #[test]
    fn credits_show_on_first_and_last_slide_by_default() {
        let song = Song { title: "T".into(), ccli: "123".into(), lyrics: "Verse 1\na\n\nb\n\nc".into(), ..Default::default() };
        let line = Some("T · CCLI Song #123");
        assert_eq!(footers(&song_slides(&song, &Settings::default())), [line.clone(), None, line.clone()]);
        let one = Song { lyrics: "Verse 1\na".into(), ..song.clone() };
        assert_eq!(footers(&song_slides(&one, &Settings::default())), [line.clone()], "one slide: once");
        let first = Settings { credits: "first".into(), ..Default::default() };
        assert_eq!(footers(&song_slides(&song, &first)), [line, None, None]);
        let off = Settings { credits: "none".into(), ..Default::default() };
        assert_eq!(footers(&song_slides(&song, &off)), [None, None, None]);
    }

    #[test]
    fn credit_template_fills_parts_and_drops_empty_ones() {
        let song = Song { title: "Grace".into(), artist: "Chris Tomlin".into(), writers: "John Newton".into(), publisher: "sixsteps Music".into(),
            year: "2006".into(), ccli: "4768151".into(), lyrics: "Verse 1\na".into(), ..Default::default() };
        assert_eq!(fill_credit(DEFAULT_CREDIT_TEMPLATE, &song, "999"), "Grace · John Newton · © 2006 sixsteps Music · CCLI Song #4768151 · CCLI License #999");
        assert_eq!(fill_credit("“{title}” as sung by {artist} | Words: {author} | {weird}", &song, ""), "“Grace” as sung by Chris Tomlin | Words: John Newton | {weird}");
        let bare = Song { title: "Grace".into(), ..Default::default() };
        assert_eq!(fill_credit(DEFAULT_CREDIT_TEMPLATE, &bare, ""), "Grace", "parts with nothing in them go");
        assert_eq!(fill_credit("{title} ({year})", &bare, ""), "Grace ()", "one part: kept as written");
        let written = Song { copyright: "© 2006 Hillsong".into(), year: "1999".into(), ..bare };
        assert_eq!(song_copyright(&written), "© 2006 Hillsong", "the written one wins");
        let custom = Settings { credit_template: "{title} — {artist}".into(), ..Default::default() };
        assert_eq!(footers(&song_slides(&song, &custom))[0], Some("Grace — Chris Tomlin"));
    }

    #[test]
    fn slide_labels_come_from_brace_lines() {
        let song = Song { lyrics: "Verse 1\n{Instrumental}\n\n{Spoken Word}\nSay this\nand this\nand that\nChorus\nx".into(), ..Default::default() };
        let slides = song_slides(&song, &Settings { credits: "none".into(), ..Default::default() });
        let got: Vec<(&str, Option<&str>, String)> = slides.iter().map(|s| (s.label.as_str(), s.tag.as_deref(), match &s.content { SlideContent::Text { text, .. } => text.clone(), _ => String::new() })).collect();
        assert_eq!(got, [
            ("Verse 1", Some("Instrumental"), String::new()),
            ("Verse 1 (1/2)", Some("Spoken Word"), "Say this\nand this".into()),
            ("Verse 1 (2/2)", None, "and that".into()),
            ("Chorus", None, "x".into()),
        ]);
        assert_eq!(slides[2].section.as_ref().map(|r| (r.page, r.pages)), Some((2, 3)), "the label line is not counted as words");
    }

    #[test]
    fn exact_discontiguous_and_cross_chapter_labels() {
        let v = |r: &str| VerseText::new(r, "t");
        let a = [v("Psalm 23:1"), v("Psalm 23:4"), v("Psalm 23:5"), v("Psalm 23:6")];
        assert_eq!(join_refs(&a.iter().collect::<Vec<_>>()), "Psalm 23:1, 4–6");
        let b = [v("John 1:51"), v("John 2:1"), v("John 2:2")];
        assert_eq!(join_refs(&b.iter().collect::<Vec<_>>()), "John 1:51–2:2");
    }

    fn verses(n: usize) -> Vec<VerseText> {
        (1..=n).map(|i| VerseText::new(format!("Psalm 23:{i}"), format!("verse {i} words"))).collect()
    }

    #[test]
    fn scripture_groups_by_per_slide_and_automatic_by_length() {
        assert_eq!(scripture_slides(&verses(5), 2, "KJV").len(), 3);
        assert_eq!(scripture_slides(&verses(5), 1, "KJV").len(), 5);
        assert_eq!(scripture_slides(&verses(5), 0, "KJV").len(), 1, "short verses share one slide when automatic");
    }

    #[test]
    fn passage_text_size_reaches_every_slide() {
        let mut data = Data::default();
        let kind = ItemKind::Scripture { reference: "Psalm 23".into(), translation: "KJV".into(), verses: verses(4), per_slide: 2, text_size: 96, also: vec![], layout: Default::default(), breaks: Default::default() };
        let item = ServiceItem { id: uuid::Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None };
        data.services[0].items.push(item.clone());
        let (_, kind, slides) = build_item(&data, &item);
        assert_eq!((kind, slides.len()), ("scripture", 2));
        assert!(slides.iter().all(|s| matches!(s.content, SlideContent::Text { size: 96, .. })));
    }

    fn passage(verses: Vec<VerseText>, breaks: crate::scripture::VerseBreaks) -> Vec<Slide> {
        let data = Data::default();
        let kind = ItemKind::Scripture { reference: "Psalm 23".into(), translation: "KJV".into(), verses, per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks };
        let item = ServiceItem { id: uuid::Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None };
        build_item(&data, &item).2
    }

    #[test]
    fn chosen_breaks_split_and_keep_a_single_bible_passage() {
        use crate::scripture::{VerseBreaks, VerseId};
        let labels = |s: &[Slide]| s.iter().map(|s| s.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&passage(verses(4), VerseBreaks::default())), ["Psalm 23:1–4"], "automatic: short verses share a slide");
        let before = VerseBreaks { before: [VerseId(18, 23, 3)].into(), join: Default::default() };
        assert_eq!(labels(&passage(verses(4), before)), ["Psalm 23:1–2", "Psalm 23:3–4"]);
        let slides = passage(verses(4), VerseBreaks { before: [VerseId(18, 23, 3)].into(), join: Default::default() });
        assert!(matches!(&slides[1].content, SlideContent::Text { reference: Some(r), parallel: None, .. } if r == "Psalm 23:3–4 (KJV)"), "{:?}", slides[1].content);
    }

    #[test]
    fn a_verse_too_long_for_one_slide_is_split_inside_itself() {
        let long = VerseText::new("Psalm 23:1", "word ".repeat(400).trim());
        let slides = passage(vec![long], Default::default());
        assert!(slides.len() >= 2, "{}", slides.len());
        assert_eq!(slides[1].label, "Psalm 23:1 (cont.)");
        let words: usize = slides.iter().map(|s| match &s.content { SlideContent::Text { text, .. } => text.split_whitespace().filter(|w| w.contains("word")).count(), _ => 0 }).sum();
        assert_eq!(words, 400, "no word is lost");
    }

    /// A four-verse passage, two verses a slide, in `translation`, with `credits` set as Settings has it.
    fn credited(translation: &str, credits: &str, credit: Option<BibleCredit>) -> (String, Vec<Slide>) {
        let mut data = Data::default();
        data.settings.credits = credits.into();
        if let Some(c) = credit { data.bible_credits.insert(translation.into(), c); }
        let kind = ItemKind::Scripture { reference: "Psalm 23".into(), translation: translation.into(), verses: verses(4), per_slide: 2, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() };
        let item = ServiceItem { id: uuid::Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None };
        let (title, _, slides) = build_item(&data, &item);
        (title, slides)
    }

    fn footers(slides: &[Slide]) -> Vec<Option<&str>> {
        slides.iter().map(|s| match &s.content { SlideContent::Text { footer, .. } => footer.as_deref(), _ => None }).collect()
    }

    fn rv(required: bool) -> BibleCredit {
        BibleCredit { abbr: "RV1909".into(), line: "RV1909: © Sociedad (CC BY-SA 4.0)".into(), required }
    }

    #[test]
    fn scripture_credit_follows_the_credits_setting() {
        let line = Some("RV1909: © Sociedad (CC BY-SA 4.0)");
        assert_eq!(footers(&credited("ebible:spaRV1909", "first", Some(rv(false))).1), [line, None]);
        assert_eq!(footers(&credited("ebible:spaRV1909", "last", Some(rv(false))).1), [None, line]);
        assert_eq!(footers(&credited("ebible:spaRV1909", "none", Some(rv(false))).1), [None, None], "public domain: off is off");
    }

    #[test]
    fn a_credit_the_bible_requires_shows_once_even_with_credits_off() {
        assert_eq!(footers(&credited("ebible:spaRV1909", "none", Some(rv(true))).1), [Some("RV1909: © Sociedad (CC BY-SA 4.0)"), None]);
    }

    #[test]
    fn scripture_slides_name_the_bible_by_its_abbreviation_not_its_id() {
        let (title, slides) = credited("ebible:spaRV1909", "first", Some(rv(false)));
        assert_eq!(title, "Psalm 23 (RV1909)");
        assert!(matches!(&slides[0].content, SlideContent::Text { reference: Some(r), .. } if r == "Psalm 23:1–2 (RV1909)"), "{:?}", slides[0].content);
    }

    #[test]
    fn a_bible_with_no_credit_line_gets_no_footer_and_keeps_its_id() {
        let blank = BibleCredit { abbr: "KJV".into(), line: String::new(), required: false };
        let (title, slides) = credited("KJV", "first", Some(blank));
        assert_eq!(footers(&slides), [None, None]);
        assert_eq!(title, "Psalm 23 (KJV)");
        let (_, slides) = credited("WEB", "first", None);
        assert_eq!(footers(&slides), [None, None], "a Bible with no credit known");
    }

    #[test]
    fn credits_match_the_id_in_any_letter_case_for_a_bare_id() {
        let mut data = Data::default();
        data.bible_credits.insert("ESV".into(), BibleCredit { abbr: "ESV".into(), line: "ESV notice".into(), required: true });
        assert_eq!(data.bible_credit("esv").map(|c| c.line.as_str()), Some("ESV notice"));
        assert_eq!(data.bible_credit("NIV"), None);
    }

    #[test]
    fn long_verse_split_keeps_every_word_in_order() {
        let text = "And it came to pass, when the king sat in his house, and the LORD had given him rest round about from all his enemies; That the king said unto Nathan the prophet, See now, I dwell in an house of cedar, but the ark of God dwelleth within curtains. And more words here to make it long enough to split twice over for the test.";
        let parts = split_long(text, 120);
        assert!(parts.len() >= 2);
        assert_eq!(parts.join(" ").split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>());
    }

    fn three_bible_item(per_slide: u8) -> ServiceItem {
        let vs = |t: &str| vec![VerseText::new("John 3:16", format!("{t} 16")), VerseText::new("John 3:17", format!("{t} 17"))];
        let also = vec![crate::scripture::AlsoShow { translation: "WEB".into(), verses: vs("web") }, crate::scripture::AlsoShow { translation: "BSB".into(), verses: vs("bsb") }];
        ServiceItem { id: uuid::Uuid::new_v4(), kind: ItemKind::Scripture { reference: "John 3:16-17".into(), translation: "KJV".into(), verses: vs("kjv"), per_slide, text_size: 0, also, layout: Default::default(), breaks: Default::default() }, background: None, notes: String::new(), theme: None }
    }

    #[test]
    fn three_bible_item_builds_parallel_slides() {
        let (title, kind, slides) = build_item(&Data::default(), &three_bible_item(0));
        assert_eq!((title.as_str(), kind), ("John 3:16-17 (KJV · WEB · BSB)", "scripture"));
        assert_eq!(slides.len(), 1);
        let SlideContent::Text { text, reference, parallel: Some(p), .. } = &slides[0].content else { panic!("{:?}", slides[0].content) };
        assert!(text.contains("kjv 16") && text.contains("kjv 17"));
        assert_eq!(reference.as_deref(), Some("John 3:16–17 (KJV · WEB · BSB)"));
        let codes: Vec<&str> = std::iter::once(p.main_code.as_str()).chain(p.also.iter().map(|a| a.code.as_str())).collect();
        assert_eq!(codes, ["KJV", "WEB", "BSB"]);
        assert!(p.also[1].text.contains("bsb 17"));
    }

    #[test]
    fn three_bibles_one_verse_per_slide() {
        let (_, _, slides) = build_item(&Data::default(), &three_bible_item(1));
        assert_eq!(slides.len(), 2);
        assert!(slides.iter().all(|s| matches!(&s.content, SlideContent::Text { parallel: Some(p), .. } if p.also.len() == 2)));
    }

    #[test]
    fn an_also_bible_without_text_is_left_out() {
        let mut item = three_bible_item(0);
        if let ItemKind::Scripture { also, .. } = &mut item.kind { also[1].verses.clear() }
        let (title, _, slides) = build_item(&Data::default(), &item);
        assert_eq!(title, "John 3:16-17 (KJV · WEB)");
        assert!(matches!(&slides[0].content, SlideContent::Text { parallel: Some(p), .. } if p.also.len() == 1));
    }

    fn notes_of(data: &Data, item: &ServiceItem) -> Vec<String> { build_scripture(data, item).notes }

    #[test]
    fn missing_verse_noted_once_per_bible() {
        let mut item = three_bible_item(0);
        if let ItemKind::Scripture { also, .. } = &mut item.kind { also[0].verses.truncate(1); also[1].verses.truncate(1) }
        assert_eq!(notes_of(&Data::default(), &item), ["WEB doesn't include John 3:17.", "BSB doesn't include John 3:17."]);
        assert!(notes_of(&Data::default(), &three_bible_item(0)).is_empty(), "nothing to say when every Bible has every verse");
    }

    #[test]
    fn lower_third_names_the_bible_it_leaves_out() {
        let mut data = Data::default();
        data.settings.theme.position = "lower".into();
        assert_eq!(notes_of(&data, &three_bible_item(0)), ["Lower-third themes show two Bibles. BSB is left out."]);
    }

    #[test]
    fn uncached_online_also_is_dropped_with_note() {
        let mut item = three_bible_item(0);
        if let ItemKind::Scripture { also, .. } = &mut item.kind { also[1] = crate::scripture::AlsoShow { translation: "NIV".into(), verses: vec![] } }
        let b = build_scripture(&Data::default(), &item);
        assert!(b.slides.iter().all(|s| matches!(&s.content, SlideContent::Text { parallel: Some(p), .. } if p.also.len() == 1)));
        assert_eq!(b.notes, ["NIV needs the internet for this passage."]);
    }

    #[test]
    fn removed_also_bible_drops_column_with_note() {
        let mut item = three_bible_item(0);
        if let ItemKind::Scripture { also, .. } = &mut item.kind { also.truncate(1); also[0] = crate::scripture::AlsoShow { translation: "ebible:spaRV1909".into(), verses: vec![] } }
        let b = build_scripture(&Data::default(), &item);
        assert!(b.slides.iter().all(|s| matches!(&s.content, SlideContent::Text { parallel: None, .. })), "only the main Bible is left");
        assert_eq!(b.notes, ["That Bible was removed. Add it again in Settings → Bibles."]);
    }

    #[test]
    fn build_item_and_build_scripture_agree() {
        let (data, item) = (Data::default(), three_bible_item(1));
        assert_eq!(build_item(&data, &item).2, build_scripture(&data, &item).slides);
        assert!(build_scripture(&data, &ServiceItem { kind: ItemKind::Text { title: "A".into(), body: "a".into(), layout: Default::default() }, ..item }).notes.is_empty());
    }

    #[test]
    fn online_main_without_text_says_so() {
        let mut item = three_bible_item(0);
        if let ItemKind::Scripture { translation, verses, also, .. } = &mut item.kind { *translation = "NIV".into(); verses.clear(); also.clear() }
        assert_eq!(notes_of(&Data::default(), &item), ["Couldn't get NIV text — check the internet."]);
    }
}
