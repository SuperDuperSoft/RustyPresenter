//! The printed document, built from a service with no layout in it (spec §6.1–6.3).
use super::strings as tr;
use crate::{
    model::{Data, DateOrder, ItemKind, MediaKind, PrintSettings, PrintWhat, Service, Song, VerseText},
    planning,
    model::ServiceItem,
    slides::{item_order, order_of, parse_sections},
};

#[derive(Clone, Debug, PartialEq)]
pub struct PrintDoc { pub title: String, pub subtitle: String, pub blocks: Vec<Block> }

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// A part heading ("Service order", "Lyrics") or a song's or reading's title.
    Heading(String),
    /// A coloured header of the service list, in the order.
    OrderHeader(String),
    Credit(String),
    OrderRow { n: usize, kind: String, title: String, detail: String, notes: String, blank: bool },
    /// "Order: Verse 1 · Chorus".
    Order(String),
    Section { label: String, lines: Vec<String> },
    Para(String),
    PageBreak,
}

pub fn build(data: &Data, service: &Service, options: &PrintSettings, order: DateOrder) -> PrintDoc {
    let mut subtitle = service.date.map(|d| planning::format_date_long(d, order)).unwrap_or_default();
    let ccli = data.settings.church_ccli.trim();
    if !ccli.is_empty() {
        if !subtitle.is_empty() { subtitle += " · " }
        subtitle += &tr::ccli_licence(ccli);
    }
    let mut blocks = vec![];
    if options.what != PrintWhat::Lyrics { order_blocks(data, service, options, &mut blocks) }
    if options.what == PrintWhat::Both { blocks.push(Block::PageBreak) }
    if options.what != PrintWhat::Order { lyric_blocks(data, service, options, &mut blocks) }
    PrintDoc { title: service.name.clone(), subtitle, blocks }
}

fn song<'a>(data: &'a Data, id: &uuid::Uuid) -> Option<&'a Song> { data.songs.iter().find(|s| s.id == *id) }

/// The song's sections in performance order (this service's own order, if it has one), by label.
fn order_labels(item: &ServiceItem, s: &Song) -> Vec<String> {
    let sections = parse_sections(&s.lyrics);
    order_of(item_order(item, s), &sections).into_iter().map(|i| sections[i].label.clone()).collect()
}

fn first_line(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.chars().count() > 80 { format!("{}…", line.chars().take(79).collect::<String>()) } else { line.into() }
}

fn order_blocks(data: &Data, service: &Service, options: &PrintSettings, out: &mut Vec<Block>) {
    out.push(Block::Heading(tr::order_heading().into()));
    if service.items.is_empty() { out.push(Block::Para(tr::empty_service().into())); return }
    let mut n = 0;
    for item in &service.items {
        let (kind, title, detail, blank) = match &item.kind {
            ItemKind::Header { title, .. } => { out.push(Block::OrderHeader(title.clone())); continue }
            ItemKind::Song { song_id, .. } => match song(data, song_id) {
                Some(s) => {
                    let labels = order_labels(item, s);
                    let mut detail = s.writers.trim().to_string();
                    if !item_order(item, s).is_empty() && !labels.is_empty() {
                        if !detail.is_empty() { detail += " · " }
                        detail += &tr::order_line(&labels.join(" "));
                    }
                    ("song", s.title.clone(), detail, false)
                }
                None => ("song", tr::missing_song().into(), String::new(), false),
            },
            ItemKind::Scripture { reference, translation, also, .. } => {
                let mut code = translation.clone();
                for a in also { code += &format!(" + {}", a.translation) }
                ("scripture", reference.clone(), code, false)
            }
            ItemKind::Media { media_id, end, looped, .. } => match data.media.iter().find(|m| m.id == *media_id) {
                Some(m) => {
                    let what = match m.kind { MediaKind::Video => tr::video(), MediaKind::Image => tr::picture(), MediaKind::Audio => tr::audio() };
                    let loops = *looped || *end == crate::model::EndAction::Loop;
                    ("media", m.name.clone(), format!("{what}{}", if loops { tr::loops() } else { String::new() }), false)
                }
                None => ("media", tr::missing_media().into(), String::new(), false),
            },
            ItemKind::Deck { deck_id } => match data.decks.iter().find(|d| d.id == *deck_id) {
                Some(d) => ("deck", d.title.clone(), tr::slides(d.slides.len()), false),
                None => ("deck", tr::missing_slides().into(), String::new(), false),
            },
            ItemKind::Text { title, body, .. } => ("text", title.clone(), first_line(body), false),
            ItemKind::Blank { label, want } => {
                let asked;
                let label = if label.trim().is_empty() { asked = planning::strings::blank_label(*want); &asked } else { label.trim() };
                ("blank", tr::blank_title(label), tr::not_chosen().into(), true)
            }
        };
        n += 1;
        let notes = if options.notes { item.notes.trim().to_string() } else { String::new() };
        out.push(Block::OrderRow { n, kind: kind.into(), title, detail, notes, blank });
    }
}

/// "16 For God so loved…  17 For God sent…".
fn verses_para(verses: &[VerseText]) -> String {
    verses.iter().map(|v| {
        let n = v.reference.rsplit(':').next().unwrap_or("").trim();
        if n.is_empty() || n == v.reference { v.text.trim().to_string() } else { format!("{n} {}", v.text.trim()) }
    }).collect::<Vec<_>>().join(" ")
}

fn lyric_blocks(data: &Data, service: &Service, options: &PrintSettings, out: &mut Vec<Block>) {
    out.push(Block::Heading(tr::lyrics_heading().into()));
    let mut songs = 0;
    let mut any = false;
    for item in &service.items {
        match &item.kind {
            ItemKind::Song { song_id, .. } => {
                let Some(s) = song(data, song_id) else { continue };
                if options.song_per_page && songs > 0 { out.push(Block::PageBreak) }
                songs += 1;
                any = true;
                out.push(Block::Heading(s.title.clone()));
                if !s.writers.trim().is_empty() { out.push(Block::Credit(s.writers.trim().into())) }
                if !s.copyright.trim().is_empty() { out.push(Block::Credit(tr::copyright(&s.copyright))) }
                if !s.ccli.trim().is_empty() { out.push(Block::Credit(tr::ccli_song(s.ccli.trim()))) }
                let sections = parse_sections(&s.lyrics);
                if sections.is_empty() { out.push(Block::Para(tr::no_lyrics().into())); continue }
                let order = order_of(item_order(item, s), &sections);
                out.push(Block::Order(tr::order_line(&order.iter().map(|i| sections[*i].label.as_str()).collect::<Vec<_>>().join(" · "))));
                let mut seen = vec![];
                for i in order.into_iter().chain(0..sections.len()) {
                    if seen.contains(&i) { continue }
                    seen.push(i);
                    let sec = &sections[i];
                    let mut lines = vec![];
                    for (g, group) in sec.groups.iter().enumerate() {
                        if g > 0 { lines.push(String::new()) }
                        lines.extend(group.iter().filter(|l| crate::sections::label_line(l).is_none()).cloned());
                    }
                    out.push(Block::Section { label: sec.label.clone(), lines });
                }
            }
            ItemKind::Scripture { reference, translation, verses, also, .. } if options.scripture => {
                any = true;
                out.push(Block::Heading(format!("{reference} ({translation})")));
                out.push(Block::Para(if verses.is_empty() { tr::no_verse_text().into() } else { verses_para(verses) }));
                for a in also.iter().filter(|a| !a.verses.is_empty()) {
                    out.push(Block::Credit(a.translation.clone()));
                    out.push(Block::Para(verses_para(&a.verses)));
                }
                if let Some(c) = data.bible_credit(translation).filter(|c| !c.line.is_empty()) { out.push(Block::Credit(c.line.clone())) }
            }
            _ => {}
        }
    }
    if !any { out.push(Block::Para(tr::nothing_to_print().into())) }
}
