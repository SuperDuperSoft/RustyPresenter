use super::doc::{Block, build};
use super::*;
use crate::model::*;
use uuid::Uuid;

fn item(kind: ItemKind) -> ServiceItem { ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None } }

const LYRICS: &str = "Verse 1\nAmazing grace\nHow sweet the sound\n\nChorus\nMy chains are gone\n\nVerse 2\nTwas grace";

/// A service with one of every kind, and the data it needs.
fn sample() -> (Data, Service) {
    let mut d = Data::default();
    let song = Song { id: Uuid::new_v4(), title: "Amazing Grace".into(), writers: "John Newton".into(), ccli: "4768151".into(),
        copyright: "Public Domain".into(), lyrics: LYRICS.into(), arrangement: vec!["Verse 1".into(), "Chorus".into(), "Verse 2".into(), "Chorus".into()], ..Default::default() };
    let media = MediaFile { id: Uuid::new_v4(), name: "Walk-in".into(), kind: MediaKind::Video, asset: "a.mp4".into(), status: MediaStatus::Ready,
        original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
    let deck = Deck { id: Uuid::new_v4(), title: "Sermon".into(), slides: vec!["1.png".into(), "2.png".into()], notes: vec![] };
    let mut s = Service::new("Sunday 11/8/2026");
    s.date = chrono::NaiveDate::from_ymd_opt(2026, 11, 8);
    s.items = vec![
        item(ItemKind::Header { title: "Worship".into(), color: String::new() }),
        item(ItemKind::Song { song_id: song.id, arrangement: None }),
        item(ItemKind::Scripture { reference: "John 3:16-17".into(), translation: "KJV".into(),
            verses: vec![VerseText::new("John 3:16", "For God so loved the world"), VerseText::new("John 3:17", "For God sent not")],
            per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() }),
        item(ItemKind::Media { media_id: media.id, autoplay: true, end: EndAction::Loop, looped: false }),
        item(ItemKind::Deck { deck_id: deck.id }),
        item(ItemKind::Text { title: "Welcome".into(), body: "\nWelcome to <church> & friends\nsecond".into(), layout: Default::default() }),
        item(ItemKind::Blank { label: "Song".into(), want: BlankKind::Song }),
    ];
    s.items[1].notes = "Key of G".into();
    d.songs.push(song);
    d.media.push(media);
    d.decks.push(deck);
    d.settings.church_ccli = "123456".into();
    d.settings.date_order = Some(DateOrder::Mdy);
    (d, s)
}

fn opts(what: PrintWhat) -> PrintSettings { PrintSettings { what, ..PrintSettings::default() } }

fn rows(blocks: &[Block]) -> Vec<(String, String, String, bool)> {
    blocks.iter().filter_map(|b| match b {
        Block::OrderRow { kind, title, detail, blank, .. } => Some((kind.clone(), title.clone(), detail.clone(), *blank)),
        _ => None,
    }).collect()
}

#[test]
fn order_rows_for_every_kind() {
    let (d, s) = sample();
    let doc = build(&d, &s, &opts(PrintWhat::Order), DateOrder::Mdy);
    assert_eq!(doc.title, "Sunday 11/8/2026");
    assert_eq!(doc.subtitle, "Sunday, November 8, 2026 · CCLI License # 123456");
    assert!(doc.blocks.contains(&Block::OrderHeader("Worship".into())));
    let r = rows(&doc.blocks);
    let s = |a: &str, b: &str, c: &str, bl| (a.to_string(), b.to_string(), c.to_string(), bl);
    assert_eq!(r, vec![
        s("song", "Amazing Grace", "John Newton · Order: Verse 1 Chorus Verse 2 Chorus", false),
        s("scripture", "John 3:16-17", "KJV", false),
        s("media", "Walk-in", "Video (loops)", false),
        s("deck", "Sermon", "2 slides", false),
        s("text", "Welcome", "Welcome to <church> & friends", false),
        s("blank", "Blank — Song", "(not chosen yet)", true),
    ]);
    assert!(!doc.blocks.iter().any(|b| matches!(b, Block::Heading(h) if h == "Lyrics")), "order only");
}

#[test]
fn print_follows_this_services_song_order() {
    let (d, mut s) = sample();
    if let ItemKind::Song { arrangement, .. } = &mut s.items[1].kind { *arrangement = Some(vec!["Chorus".into(), "Verse 2".into()]) }
    let doc = build(&d, &s, &opts(PrintWhat::Both), DateOrder::Mdy);
    assert_eq!(rows(&doc.blocks)[0].2, "John Newton · Order: Chorus Verse 2");
    assert!(doc.blocks.contains(&Block::Order("Order: Chorus · Verse 2".into())));
}

#[test]
fn order_includes_notes_only_when_asked() {
    let (d, s) = sample();
    let notes = |o: &PrintSettings| build(&d, &s, o, DateOrder::Mdy).blocks.iter().filter_map(|b| match b { Block::OrderRow { notes, .. } if !notes.is_empty() => Some(notes.clone()), _ => None }).collect::<Vec<_>>();
    assert_eq!(notes(&opts(PrintWhat::Order)), ["Key of G"]);
    assert!(notes(&PrintSettings { notes: false, ..opts(PrintWhat::Order) }).is_empty());
}

#[test]
fn lyrics_print_each_section_once_with_order_line() {
    let (d, s) = sample();
    let doc = build(&d, &s, &opts(PrintWhat::Lyrics), DateOrder::Mdy);
    assert!(doc.blocks.contains(&Block::Order("Order: Verse 1 · Chorus · Verse 2 · Chorus".into())));
    let sections: Vec<&str> = doc.blocks.iter().filter_map(|b| match b { Block::Section { label, .. } => Some(label.as_str()), _ => None }).collect();
    assert_eq!(sections, ["Verse 1", "Chorus", "Verse 2"]);
    assert!(doc.blocks.contains(&Block::Credit("© Public Domain".into())));
    assert!(doc.blocks.contains(&Block::Credit("CCLI Song # 4768151".into())));
    assert!(doc.blocks.contains(&Block::Para("16 For God so loved the world 17 For God sent not".into())));
    assert!(rows(&doc.blocks).is_empty(), "lyrics only");
}

#[test]
fn lyrics_scripture_optional_and_blanks_skipped() {
    let (d, s) = sample();
    let doc = build(&d, &s, &PrintSettings { scripture: false, ..opts(PrintWhat::Lyrics) }, DateOrder::Mdy);
    assert!(!doc.blocks.iter().any(|b| matches!(b, Block::Heading(h) if h.starts_with("John 3"))));
    assert!(!doc.blocks.iter().any(|b| matches!(b, Block::Heading(h) if h.contains("Blank"))));
}

#[test]
fn lyrics_song_per_page_inserts_breaks() {
    let (mut d, mut s) = sample();
    let mut second = d.songs[0].clone();
    second.id = Uuid::new_v4();
    second.title = "Second".into();
    s.items.push(item(ItemKind::Song { song_id: second.id, arrangement: None }));
    d.songs.push(second);
    let breaks = |o: &PrintSettings| build(&d, &s, o, DateOrder::Mdy).blocks.iter().filter(|b| **b == Block::PageBreak).count();
    assert_eq!(breaks(&opts(PrintWhat::Lyrics)), 0);
    assert_eq!(breaks(&PrintSettings { song_per_page: true, ..opts(PrintWhat::Lyrics) }), 1);
    assert_eq!(breaks(&opts(PrintWhat::Both)), 1, "Both: order, a break, lyrics");
}

#[test]
fn lyrics_empty_service_message() {
    let d = Data::default();
    let s = Service::new("Empty");
    let doc = build(&d, &s, &opts(PrintWhat::Both), DateOrder::Mdy);
    assert!(doc.blocks.contains(&Block::Para(strings::nothing_to_print().into())));
    assert!(doc.blocks.contains(&Block::Para(strings::empty_service().into())));
    assert_eq!(doc.subtitle, "", "no date, no licence");
}

#[test]
fn html_escapes_text_and_has_no_script_or_remote_urls() {
    let (d, mut s) = sample();
    s.name = "Easter <b>&</b>".into();
    let page = super::html(&d, &s, &opts(PrintWhat::Both), chrono::NaiveDate::from_ymd_opt(2026, 11, 6).unwrap());
    assert!(page.contains("Easter &lt;b&gt;&amp;&lt;/b&gt;"));
    assert!(page.contains("Welcome to &lt;church&gt; &amp; friends"));
    assert!(!page.contains("<b>&</b>"));
    assert!(!page.to_lowercase().contains("<script"));
    assert!(!page.contains("http://") && !page.contains("https://") && !page.contains("src="));
    assert!(page.contains("Printed from RustyPresenter · 11/6/2026"));
    assert!(page.contains("class=\"brk\""), "Both has a page break");
}

#[test]
fn html_page_size_from_paper() {
    let (mut d, s) = sample();
    d.settings.print.paper = Some(Paper::A4);
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 6).unwrap();
    assert!(super::html(&d, &s, &PrintSettings { paper: Some(Paper::Letter), ..opts(PrintWhat::Order) }, today).contains("size: letter"));
    assert!(super::html(&d, &s, &PrintSettings { paper: Some(Paper::A4), ..opts(PrintWhat::Order) }, today).contains("size: A4"));
}

#[test]
fn file_stems_say_what_is_in_them() {
    assert_eq!(file_stem("Sunday", PrintWhat::Both), "Sunday – order and lyrics");
    assert_eq!(file_stem("Sunday", PrintWhat::Order), "Sunday – order");
    assert_eq!(file_stem("Sunday", PrintWhat::Lyrics), "Sunday – lyrics");
}
