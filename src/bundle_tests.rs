use super::*;
use crate::{model::*, store::test_dir};
use std::{fs, io::Write, path::PathBuf};

fn store() -> Store { Store::open(test_dir()).unwrap() }

fn asset(s: &Store, bytes: &[u8], ext: &str) -> AssetId {
    let p = s.tmp().join(Uuid::new_v4().to_string());
    fs::write(&p, bytes).unwrap();
    s.import_file(&p, ext).unwrap()
}

fn item(kind: ItemKind) -> ServiceItem { ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None } }

fn song(title: &str, ccli: &str, lyrics: &str) -> Song { Song { id: Uuid::new_v4(), title: title.into(), ccli: ccli.into(), lyrics: lyrics.into(), ..Song::default() } }

fn media(name: &str, a: &str) -> MediaFile { MediaFile { id: Uuid::new_v4(), name: name.into(), kind: MediaKind::Video, asset: a.into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() } }

/// A service using a song, a video, a deck and an image background; plus things it does not use.
fn sample(s: &Store) -> Data {
    let mut d = Data::default();
    let (v, slide, bg, other) = (asset(s, b"video", "mp4"), asset(s, b"slide", "png"), asset(s, b"bg", "jpg"), asset(s, b"not used", "png"));
    let (sg, m, deck) = (song("Amazing Grace", "22025", "Verse 1\nAmazing grace"), media("clip", &v), Deck { id: Uuid::new_v4(), title: "Notices".into(), slides: vec![slide] });
    let mut first = item(ItemKind::Song { song_id: sg.id, arrangement: None });
    first.background = Some(Background::Image { asset: bg, dim: 0 });
    first.notes = "Key of G".into();
    let svc = d.service_mut().unwrap();
    svc.name = "Easter".into();
    svc.items = vec![
        first,
        item(ItemKind::Media { media_id: m.id, autoplay: true, end: EndAction::Stop, looped: false }),
        item(ItemKind::Deck { deck_id: deck.id }),
        item(ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses: vec![VerseText::new("John 3:16", "For God")], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() }),
    ];
    d.songs = vec![sg, song("Unused", "1", "x")];
    d.media = vec![m, media("unused", &other)];
    d.decks = vec![deck];
    d
}

fn current(d: &Data) -> Uuid { d.service().unwrap().id }

fn saved(s: &Store, d: &Data) -> PathBuf {
    let (c, a) = gather(d, current(d), &|id| s.asset_path(id).exists()).unwrap();
    let dest = s.root.join("Easter.rpservice");
    write(s, &c, &a, &dest, &CancellationToken::new(), &|_| {}).unwrap();
    dest
}

fn open_in(to: &Store, path: &Path) -> Result<Contents, String> { read(to, path, &CancellationToken::new(), &|_| {}) }

fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
    let mut w = zip::ZipWriter::new(fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap();
}

/// A bundle with one video item whose JSON is `edit`ed, carrying `entries` beside `bundle.json`.
fn crafted(s: &Store, a: &str, edit: impl FnOnce(&mut Contents), entries: &[(&str, &[u8])]) -> PathBuf {
    let m = media("clip", a);
    let mut svc = Service::new("Crafted");
    svc.items = vec![item(ItemKind::Media { media_id: m.id, autoplay: true, end: EndAction::Stop, looped: false })];
    let mut c = Contents { format: FORMAT, service: svc, songs: vec![], media: vec![m], decks: vec![], fonts: vec![] };
    edit(&mut c);
    let json = serde_json::to_vec(&c).unwrap();
    let path = s.root.join("crafted.rpservice");
    let mut all: Vec<(&str, &[u8])> = vec![("bundle.json", &json)];
    all.extend_from_slice(entries);
    zip_of(&path, &all);
    path
}

const HEX: &str = "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae";

#[test]
fn gather_takes_only_what_the_service_uses() {
    let s = store();
    let d = sample(&s);
    let (c, assets) = gather(&d, current(&d), &|_| true).unwrap();
    assert_eq!(c.songs.len(), 1);
    assert_eq!(c.media.len(), 1);
    assert_eq!(c.decks.len(), 1);
    assert_eq!(assets.len(), 3, "the video, the slide and the background: {assets:?}");
    assert!(!assets.contains(&d.media[1].asset));
}

#[test]
fn gather_keeps_a_thumbnail_only_when_it_is_there() {
    let s = store();
    let mut d = sample(&s);
    let thumb = asset(&s, b"thumb", "jpg");
    d.media[0].thumb = Some(thumb.clone());
    d.media[0].original_asset = Some("gone.mp4".into());
    let (c, assets) = gather(&d, current(&d), &|id| s.asset_path(id).exists()).unwrap();
    assert!(assets.contains(&thumb));
    assert_eq!(c.media[0].original_asset, None, "an extra the service can do without is dropped, not a failure");
}

#[test]
fn gather_names_everything_missing_and_saves_nothing() {
    let s = store();
    let mut d = sample(&s);
    d.songs.clear();
    let video = d.media[0].asset.clone();
    let e = gather(&d, current(&d), &|id| id != video).unwrap_err();
    assert_eq!(e.len(), 2, "{e:?}");
    assert!(e[0].contains("song"), "{e:?}");
    assert!(e[1].contains("clip"), "{e:?}");
}

#[test]
fn gather_of_an_unknown_service_says_so() {
    let d = Data::default();
    assert_eq!(gather(&d, Uuid::new_v4(), &|_| true).unwrap_err(), vec![strings::service_gone()]);
}

#[test]
fn moves_a_service_to_an_empty_computer() {
    let (a, b) = (store(), store());
    let d = sample(&a);
    let file = saved(&a, &d);
    let mut there = Data::default();
    let c = open_in(&b, &file).unwrap();
    let opened = merge(&mut there, c);
    assert_eq!(opened.name, "Easter");
    assert_eq!((opened.songs_added, opened.media_added, opened.decks_added), (1, 1, 1));
    let svc = there.service().unwrap();
    assert_eq!(svc.id, opened.service, "the new service is the open one");
    assert_eq!(svc.items.len(), 4);
    assert_eq!(svc.items[0].notes, "Key of G");
    assert!(matches!(&svc.items[3].kind, ItemKind::Scripture { verses, .. } if verses[0].text == "For God"));
    assert_eq!(there.songs[0].lyrics, "Verse 1\nAmazing grace");
    assert_eq!(fs::read(b.asset_path(&there.media[0].asset)).unwrap(), b"video");
    assert_eq!(fs::read(b.asset_path(&there.decks[0].slides[0])).unwrap(), b"slide");
    let Some(Background::Image { asset: bg, .. }) = &svc.items[0].background else { panic!() };
    assert_eq!(fs::read(b.asset_path(bg)).unwrap(), b"bg");
}

#[test]
fn opening_twice_adds_a_second_service_and_nothing_else() {
    let (a, b) = (store(), store());
    let file = saved(&a, &sample(&a));
    let mut there = Data::default();
    merge(&mut there, open_in(&b, &file).unwrap());
    let second = merge(&mut there, open_in(&b, &file).unwrap());
    assert_eq!((second.songs_added, second.media_added, second.decks_added), (0, 0, 0));
    assert_eq!((there.songs.len(), there.media.len(), there.decks.len()), (1, 1, 1));
    assert_eq!(there.services.len(), 3);
    assert_eq!(second.name, "Easter (2)", "two services do not share a name");
    let ids: std::collections::BTreeSet<Uuid> = there.services[1..].iter().flat_map(|s| s.items.iter().map(|i| i.id)).collect();
    assert_eq!(ids.len(), 8, "item ids are not shared between services");
}

#[test]
fn the_same_song_under_another_id_is_reused() {
    let mut there = Data::default();
    let local = song("Grace", "22025", "words");
    let local_id = local.id;
    there.songs.push(local);
    let incoming = song("Amazing Grace", "22025", "words");
    let mut svc = Service::new("S");
    svc.items = vec![item(ItemKind::Song { song_id: incoming.id, arrangement: None })];
    let opened = merge(&mut there, Contents { format: FORMAT, service: svc, songs: vec![incoming], media: vec![], decks: vec![], fonts: vec![] });
    assert_eq!(opened.songs_added, 0);
    assert_eq!(there.songs.len(), 1);
    assert!(matches!(there.service().unwrap().items[0].kind, ItemKind::Song { song_id, .. } if song_id == local_id));
}

#[test]
fn a_song_with_other_words_comes_in_beside_the_library_one() {
    let mut there = Data::default();
    let local = song("Grace", "22025", "the words here");
    let (local_id, same_id) = (local.id, local.id);
    there.songs.push(local);
    let mut incoming = song("Grace", "22025", "the words as rehearsed");
    incoming.id = same_id;
    let mut svc = Service::new("S");
    svc.items = vec![item(ItemKind::Song { song_id: same_id, arrangement: None })];
    let opened = merge(&mut there, Contents { format: FORMAT, service: svc, songs: vec![incoming], media: vec![], decks: vec![], fonts: vec![] });
    assert_eq!(opened.songs_differ, vec!["Grace".to_string()]);
    assert_eq!(there.songs.len(), 2);
    assert_eq!(there.songs[0].lyrics, "the words here", "the library's own song is untouched");
    let ItemKind::Song { song_id, .. } = there.service().unwrap().items[0].kind else { panic!() };
    assert_ne!(song_id, local_id, "the service shows the words it was saved with");
    assert_eq!(there.songs.iter().find(|s| s.id == song_id).unwrap().lyrics, "the words as rehearsed");
}

#[test]
fn an_arrangement_change_counts_as_different() {
    let mut there = Data::default();
    there.songs.push(song("Grace", "22025", "words"));
    let mut incoming = song("Grace", "22025", "words");
    incoming.arrangement = vec!["Chorus".into()];
    let svc = Service::new("S");
    assert_eq!(merge(&mut there, Contents { format: FORMAT, service: svc, songs: vec![incoming], media: vec![], decks: vec![], fonts: vec![] }).songs_differ.len(), 1);
}

#[test]
fn two_titles_without_ccli_are_only_the_same_with_the_same_words() {
    let mut there = Data::default();
    there.songs.push(song("Hymn", "", "one"));
    let svc = Service::new("S");
    let same = song("Hymn", "", "one");
    assert_eq!(merge(&mut there, Contents { format: FORMAT, service: svc.clone(), songs: vec![same], media: vec![], decks: vec![], fonts: vec![] }).songs_added, 0);
    let other = song("Hymn", "", "two");
    assert_eq!(merge(&mut there, Contents { format: FORMAT, service: svc, songs: vec![other], media: vec![], decks: vec![], fonts: vec![] }).songs_differ.len(), 1);
}

#[test]
fn media_and_decks_match_by_their_files() {
    let mut there = Data::default();
    let local = media("Local name", &format!("{HEX}.mp4"));
    let local_deck = Deck { id: Uuid::new_v4(), title: "Local".into(), slides: vec![format!("{HEX}.png")] };
    let (mid, did) = (local.id, local_deck.id);
    there.media.push(local);
    there.decks.push(local_deck);
    let m = media("Other name", &format!("{HEX}.mp4"));
    let deck = Deck { id: Uuid::new_v4(), title: "Other".into(), slides: vec![format!("{HEX}.png")] };
    let mut svc = Service::new("S");
    svc.items = vec![item(ItemKind::Media { media_id: m.id, autoplay: false, end: EndAction::Loop, looped: false }), item(ItemKind::Deck { deck_id: deck.id })];
    let opened = merge(&mut there, Contents { format: FORMAT, service: svc, songs: vec![], media: vec![m], decks: vec![deck], fonts: vec![] });
    assert_eq!((opened.media_added, opened.decks_added), (0, 0));
    let items = &there.service().unwrap().items;
    assert!(matches!(items[0].kind, ItemKind::Media { media_id, autoplay: false, end: EndAction::Loop, .. } if media_id == mid));
    assert!(matches!(items[1].kind, ItemKind::Deck { deck_id } if deck_id == did));
}

#[test]
fn an_id_already_used_by_other_media_gets_a_new_one() {
    let mut there = Data::default();
    let local = media("Local", &format!("{HEX}.mp4"));
    let id = local.id;
    there.media.push(local);
    let mut m = media("Other", &format!("{HEX}.mov"));
    m.id = id;
    let mut svc = Service::new("S");
    svc.items = vec![item(ItemKind::Media { media_id: id, autoplay: true, end: EndAction::Stop, looped: false })];
    merge(&mut there, Contents { format: FORMAT, service: svc, songs: vec![], media: vec![m], decks: vec![], fonts: vec![] });
    assert_eq!(there.media.len(), 2);
    let ItemKind::Media { media_id, .. } = there.service().unwrap().items[0].kind else { panic!() };
    assert_ne!(media_id, id);
    assert_eq!(there.media.iter().find(|m| m.id == media_id).unwrap().name, "Other");
}

#[test]
fn a_file_that_is_not_a_zip_is_refused() {
    let s = store();
    let p = s.root.join("x.rpservice");
    fs::write(&p, b"hello").unwrap();
    assert_eq!(open_in(&s, &p).unwrap_err(), strings::not_a_service_file());
}

#[test]
fn a_zip_without_the_contents_is_refused() {
    let s = store();
    let p = s.root.join("x.rpservice");
    zip_of(&p, &[("other.txt", b"hi")]);
    assert_eq!(open_in(&s, &p).unwrap_err(), strings::not_a_service_file());
}

#[test]
fn a_newer_format_is_refused() {
    let s = store();
    let a = format!("{}.mp4", crate::sha256::digest_hex(b"v"));
    let p = crafted(&s, &a, |c| c.format = FORMAT + 1, &[(&format!("assets/{a}"), b"v")]);
    assert_eq!(open_in(&s, &p).unwrap_err(), strings::newer_file());
}

#[test]
fn a_changed_file_is_refused_and_nothing_stays() {
    let s = store();
    let a = format!("{}.mp4", crate::sha256::digest_hex(b"v"));
    let p = crafted(&s, &a, |_| {}, &[(&format!("assets/{a}"), b"changed")]);
    assert_eq!(open_in(&s, &p).unwrap_err(), strings::damaged());
    assert!(!s.asset_path(&a).exists());
    assert_eq!(fs::read_dir(s.tmp()).unwrap().count(), 0, "the stage is cleaned up");
    assert_eq!(fs::read_dir(s.assets()).unwrap().count(), 0, "one bad file publishes none of them");
}

#[test]
fn a_bad_file_after_a_good_one_publishes_neither() {
    let s = store();
    let good = format!("{}.png", crate::sha256::digest_hex(b"g"));
    let a = format!("{}.mp4", crate::sha256::digest_hex(b"v"));
    let g2 = good.clone();
    let p = crafted(&s, &a, move |c| c.media.push(media("g", &g2)), &[(&format!("assets/{good}"), b"g"), (&format!("assets/{a}"), b"changed")]);
    assert_eq!(open_in(&s, &p).unwrap_err(), strings::damaged());
    assert_eq!(fs::read_dir(s.assets()).unwrap().count(), 0);
}

#[test]
fn a_file_the_contents_name_but_the_zip_lacks_is_refused() {
    let s = store();
    let a = format!("{}.mp4", crate::sha256::digest_hex(b"v"));
    assert_eq!(open_in(&s, &crafted(&s, &a, |_| {}, &[])).unwrap_err(), strings::damaged());
}

#[test]
fn asset_names_that_leave_the_store_are_refused() {
    let s = store();
    for bad in ["../../evil.mp4", "..\\evil.mp4", "C:\\evil.mp4", "/etc/passwd", "short.mp4", "", "nodot"] {
        let p = crafted(&s, bad, |_| {}, &[]);
        assert_eq!(open_in(&s, &p).unwrap_err(), strings::damaged(), "{bad}");
    }
    let hexdot = format!("{}.m/p", HEX);
    assert_eq!(open_in(&s, &crafted(&s, &hexdot, |_| {}, &[])).unwrap_err(), strings::damaged());
}

#[test]
fn every_asset_field_is_checked() {
    let s = store();
    let a = format!("{}.mp4", crate::sha256::digest_hex(b"v"));
    let entry = format!("assets/{a}");
    let cases: Vec<Box<dyn FnOnce(&mut Contents)>> = vec![
        Box::new(|c| c.media[0].thumb = Some("../x.jpg".into())),
        Box::new(|c| c.media[0].original_asset = Some("../x.mp4".into())),
        Box::new(|c| c.decks.push(Deck { id: Uuid::new_v4(), title: "d".into(), slides: vec!["../x.png".into()] })),
        Box::new(|c| c.service.items[0].background = Some(Background::Video { asset: "../x.mp4".into(), dim: 0 })),
    ];
    for edit in cases {
        let p = crafted(&s, &a, edit, &[(&entry, b"v")]);
        assert_eq!(open_in(&s, &p).unwrap_err(), strings::damaged());
    }
}

#[test]
fn cancelling_a_save_leaves_no_file() {
    let s = store();
    let d = sample(&s);
    let (c, a) = gather(&d, current(&d), &|_| true).unwrap();
    let dest = s.root.join("out.rpservice");
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(write(&s, &c, &a, &dest, &token, &|_| {}).unwrap_err(), crate::import::CANCELLED);
    assert!(!dest.exists());
    assert_eq!(fs::read_dir(&s.root).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("out.")).count(), 0);
}

#[test]
fn a_failed_save_keeps_the_file_that_was_there() {
    let s = store();
    let d = sample(&s);
    let (c, a) = gather(&d, current(&d), &|_| true).unwrap();
    let dest = s.root.join("out.rpservice");
    fs::write(&dest, b"last week's").unwrap();
    fs::remove_file(s.asset_path(&a[0])).unwrap();
    assert!(write(&s, &c, &a, &dest, &CancellationToken::new(), &|_| {}).is_err());
    assert_eq!(fs::read(&dest).unwrap(), b"last week's");
}

#[test]
fn cancelling_an_open_adds_nothing() {
    let (a, b) = (store(), store());
    let file = saved(&a, &sample(&a));
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(read(&b, &file, &token, &|_| {}).unwrap_err(), crate::import::CANCELLED);
    assert_eq!(fs::read_dir(b.assets()).unwrap().count(), 0);
    assert_eq!(fs::read_dir(b.tmp()).unwrap().count(), 0);
}

#[test]
fn progress_reaches_100() {
    let s = store();
    let d = sample(&s);
    let (c, a) = gather(&d, current(&d), &|_| true).unwrap();
    let seen = std::cell::RefCell::new(vec![]);
    write(&s, &c, &a, &s.root.join("p.rpservice"), &CancellationToken::new(), &|p| seen.borrow_mut().push(p)).unwrap();
    let seen = seen.into_inner();
    assert_eq!(seen.last(), Some(&100));
    assert!(seen.is_sorted(), "{seen:?}");
}

#[test]
fn a_blocked_passage_stays_out_of_an_opened_service() {
    let (a, b) = (store(), store());
    let mut d = sample(&a);
    let scripture = ItemKind::Scripture { reference: "John 3:16".into(), translation: "TPT".into(), verses: vec![], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() };
    d.service_mut().unwrap().items.push(item(scripture));
    let file = saved(&a, &d);
    let mut there = Data::default();
    let c = open_in(&b, &file).unwrap();
    let opened = merge(&mut there, c);
    assert_eq!(opened.passages_removed, 1);
    assert_eq!(there.service().unwrap().items.len(), 4, "the rest of the service came in");
    assert!(strings::opened_report(&opened).contains("One passage used a translation that can't be used in RustyPresenter"));
}

#[test]
fn a_service_file_leaves_online_text_out() {
    let s = store();
    let mut d = sample(&s);
    if let ItemKind::Scripture { also, .. } = &mut d.service_mut().unwrap().items[3].kind {
        also.push(crate::scripture::AlsoShow { translation: "NIV".into(), verses: vec![VerseText::new("John 3:16", "NIV words")] });
    }
    let c = open_in(&store(), &saved(&s, &d)).unwrap();
    let ItemKind::Scripture { verses, also, .. } = &c.service.items[3].kind else { panic!() };
    assert_eq!((verses.len(), also[0].translation.as_str(), also[0].verses.len()), (1, "NIV", 0), "KJV kept, NIV's text left out");
}

#[test]
fn blanks_headers_and_the_font_travel_in_a_bundle() {
    let (a, b) = (store(), store());
    let mut d = sample(&a);
    d.settings.theme.font = "Montserrat".into();
    d.service_mut().unwrap().items.push(item(ItemKind::Blank { label: "Song".into(), want: crate::model::BlankKind::Song }));
    d.service_mut().unwrap().items.push(item(ItemKind::Header { title: "Worship".into(), color: "#4aa3ff".into() }));
    let file = saved(&a, &d);
    let c = open_in(&b, &file).unwrap();
    assert_eq!(c.fonts, ["Montserrat"]);
    let missing = missing_fonts(&c.fonts, &|_| false);
    assert_eq!(missing, ["Montserrat"]);
    assert!(missing_fonts(&c.fonts, &|f| f == "Montserrat").is_empty());
    assert!(missing_fonts(&["Segoe UI".to_string()], &|_| false).is_empty(), "the default font always has a stand-in");
    let mut there = Data::default();
    let opened = Opened { fonts_missing: missing, ..merge(&mut there, c) };
    let items = &there.service().unwrap().items;
    assert!(items.iter().any(|i| matches!(&i.kind, ItemKind::Blank { label, .. } if label == "Song")));
    assert!(items.iter().any(|i| matches!(&i.kind, ItemKind::Header { title, .. } if title == "Worship")));
    assert!(strings::opened_report(&opened).contains("made with a font this computer does not have, so its words show in another font: Montserrat"));
}
