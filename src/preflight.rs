//! Preflight (spec §13): before the service, is everything the current service needs really there? It reads the
//! plan and the disk and changes nothing. Scripture items carry their own verse text, so a passage in the service
//! needs no internet and no translation check; only an item without text is flagged.
use crate::{app::Monitor, model::{Background, Data, ItemKind, MediaFile, MediaStatus}, slides::build_item};
#[path = "strings/preflight.rs"]
pub mod strings;

/// `report` lists at most this many problems, so the box still fits the window.
const SHOWN: usize = 8;

/// What is wrong, one line each: the screens, then the settings' files, then the service in order. Empty when all
/// is well. `audio` is the sound outputs found, empty when that cannot be told. `has_asset` says whether a file of
/// the library is really there.
pub fn check(data: &Data, monitors: &[Monitor], audio: &[String], has_asset: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out = vec![];
    let connected = |id: &str| monitors.iter().any(|m| m.id == id);
    let displays = &data.settings.displays;
    match displays.room.as_deref() {
        None => out.push(strings::NO_ROOM.to_string()),
        Some(room) if !connected(room) => out.push(strings::ROOM_GONE.to_string()),
        Some(_) => {}
    }
    if displays.stage.as_deref().is_some_and(|s| !connected(s)) { out.push(strings::STAGE_GONE.into()); }
    let sound = data.settings.audio_device.as_str();
    if !sound.is_empty() && !audio.is_empty() && !audio.iter().any(|a| a == sound) { out.push(strings::AUDIO_GONE.into()); }
    if data.settings.logo.as_deref().is_some_and(|l| !has_asset(l)) { out.push(strings::LOGO_MISSING.into()); }
    if !background_ok(&data.settings.background, has_asset) { out.push(strings::BACKGROUND_MISSING.into()); }
    let Some(service) = data.service() else { return out };
    if service.items.is_empty() { out.push(strings::EMPTY_SERVICE.into()); }
    for (i, item) in service.items.iter().enumerate() {
        let (title, _, slides) = build_item(data, item);
        let mut problems = content_problems(data, &item.kind, slides.is_empty(), has_asset);
        if item.background.as_ref().is_some_and(|b| !background_ok(b, has_asset)) { problems.push(strings::ITEM_BACKGROUND_MISSING.into()); }
        out.extend(problems.iter().map(|p| strings::item(i + 1, &title, p)));
    }
    out
}

fn background_ok(b: &Background, has_asset: &dyn Fn(&str) -> bool) -> bool {
    match b {
        Background::Color { .. } => true,
        Background::Image { asset } | Background::Video { asset } => has_asset(asset),
    }
}

fn content_problems(data: &Data, kind: &ItemKind, no_slides: bool, has_asset: &dyn Fn(&str) -> bool) -> Vec<String> {
    let one = |s: &str| vec![s.to_string()];
    match kind {
        ItemKind::Song { song_id } if !data.songs.iter().any(|s| s.id == *song_id) => one(strings::SONG_MISSING),
        ItemKind::Song { .. } if no_slides => one(strings::SONG_EMPTY),
        ItemKind::Scripture { verses, .. } if verses.is_empty() => one(strings::NO_VERSES),
        ItemKind::Media { media_id, .. } => match data.media.iter().find(|m| m.id == *media_id) {
            None => one(strings::MEDIA_GONE),
            Some(MediaFile { status: MediaStatus::Failed { reason }, .. }) => vec![strings::cant_play(reason)],
            Some(m) if !has_asset(&m.asset) => one(strings::FILE_MISSING),
            Some(_) => vec![],
        },
        ItemKind::Deck { deck_id } => match data.decks.iter().find(|d| d.id == *deck_id) {
            None => one(strings::DECK_GONE),
            Some(d) => match d.slides.iter().filter(|a| !has_asset(a)).count() {
                0 => vec![],
                missing => vec![strings::slides_missing(missing, d.slides.len())],
            },
        },
        _ => vec![],
    }
}

/// The text for the operator.
pub fn report(problems: &[String]) -> String {
    if problems.is_empty() { return strings::ALL_CLEAR.into(); }
    let mut text = format!("{}\n", strings::found(problems.len()));
    for p in problems.iter().take(SHOWN) { text.push('\n'); text.push_str(p); }
    if problems.len() > SHOWN { text.push('\n'); text.push_str(&strings::more(problems.len() - SHOWN)); }
    text
}

#[cfg(test)]
mod tests {
    use super::{strings as s, *};
    use crate::model::{Deck, EndAction, MediaKind, Service, ServiceItem, Song, VerseText};
    use uuid::Uuid;

    fn monitor(id: &str) -> Monitor { Monitor { id: id.into(), label: format!("Screen {id}"), x: 0, y: 0, width: 1920, height: 1080, primary: false } }

    /// A service with these items, the room on a connected screen.
    fn data(items: Vec<ItemKind>) -> Data {
        let mut d = Data::default();
        d.settings.displays.room = Some("room".into());
        let mut service = Service::new("Sunday");
        service.items = items.into_iter().map(item).collect();
        d.current_service = Some(service.id);
        d.services = vec![service];
        d
    }
    fn item(kind: ItemKind) -> ServiceItem { ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new() } }
    fn text() -> ItemKind { ItemKind::Text { title: "Welcome".into(), body: "Hello".into() } }
    fn all(_: &str) -> bool { true }
    fn none(_: &str) -> bool { false }
    fn run(d: &Data, has: &dyn Fn(&str) -> bool) -> Vec<String> { check(d, &[monitor("room")], &[], has) }

    fn media(status: MediaStatus) -> MediaFile {
        MediaFile { id: Uuid::new_v4(), name: "Intro".into(), kind: MediaKind::Video, asset: "a.mp4".into(), status, original_asset: None, duration: None, thumb: None, size: None }
    }
    fn media_item(m: &MediaFile) -> ItemKind { ItemKind::Media { media_id: m.id, autoplay: true, end: EndAction::Stop, looped: false } }

    #[test]
    fn a_ready_service_has_nothing_to_fix() {
        assert_eq!(run(&data(vec![text()]), &all), Vec::<String>::new());
    }

    #[test]
    fn an_empty_service_is_flagged() {
        assert_eq!(run(&data(vec![]), &all), [s::EMPTY_SERVICE]);
    }

    #[test]
    fn no_room_screen_chosen() {
        let mut d = data(vec![text()]);
        d.settings.displays.room = None;
        assert_eq!(run(&d, &all), [s::NO_ROOM]);
    }

    #[test]
    fn room_screen_not_connected_comes_first() {
        let mut d = data(vec![ItemKind::Song { song_id: Uuid::new_v4() }]);
        d.settings.displays.room = Some(r"\\.\DISPLAY2".into());
        let got = run(&d, &all);
        assert_eq!(got[0], s::ROOM_GONE);
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn a_room_screen_is_found_by_its_id_among_several() {
        let d = data(vec![text()]);
        assert_eq!(check(&d, &[monitor("other")], &[], &all), [s::ROOM_GONE]);
        assert_eq!(check(&d, &[monitor("other"), monitor("room")], &[], &all), Vec::<String>::new());
    }

    #[test]
    fn stage_screen_gone_is_flagged_but_none_chosen_is_fine() {
        let mut d = data(vec![text()]);
        assert_eq!(run(&d, &all), Vec::<String>::new());
        d.settings.displays.stage = Some("stage".into());
        assert_eq!(run(&d, &all), [s::STAGE_GONE]);
        assert_eq!(check(&d, &[monitor("room"), monitor("stage")], &[], &all), Vec::<String>::new());
    }

    #[test]
    fn a_chosen_sound_output_that_is_gone_is_flagged() {
        let mut d = data(vec![text()]);
        let rooms = [monitor("room")];
        d.settings.audio_device = "wasapi/{board}".into();
        assert_eq!(check(&d, &rooms, &["wasapi/{laptop}".to_string()], &all), [s::AUDIO_GONE]);
        assert_eq!(check(&d, &rooms, &["wasapi/{board}".to_string()], &all), Vec::<String>::new());
        assert_eq!(check(&d, &rooms, &[], &all), Vec::<String>::new(), "no list: it cannot tell");
        d.settings.audio_device.clear();
        assert_eq!(check(&d, &rooms, &["wasapi/{laptop}".to_string()], &all), Vec::<String>::new(), "the default is always there");
    }

    #[test]
    fn missing_logo_and_default_background_files() {
        let mut d = data(vec![text()]);
        d.settings.logo = Some("logo.png".into());
        d.settings.background = Background::Image { asset: "bg.png".into() };
        assert_eq!(run(&d, &none), [s::LOGO_MISSING, s::BACKGROUND_MISSING]);
        assert_eq!(run(&d, &|a| a == "logo.png"), [s::BACKGROUND_MISSING]);
    }

    #[test]
    fn a_song_gone_from_the_library_names_its_place() {
        let d = data(vec![text(), ItemKind::Song { song_id: Uuid::new_v4() }]);
        assert_eq!(run(&d, &all), [s::item(2, "Missing song", s::SONG_MISSING)]);
    }

    #[test]
    fn a_song_without_words() {
        let song = Song { id: Uuid::new_v4(), title: "Silent".into(), ..Song::default() };
        let mut d = data(vec![ItemKind::Song { song_id: song.id }]);
        d.songs.push(song);
        assert_eq!(run(&d, &all), [s::item(1, "Silent", s::SONG_EMPTY)]);
    }

    #[test]
    fn scripture_with_no_verses_is_flagged_and_with_verses_needs_nothing_else() {
        let empty = ItemKind::Scripture { reference: "John 3:16".into(), translation: "NIV".into(), verses: vec![], per_slide: 0, text_size: 0 };
        let full = ItemKind::Scripture { reference: "John 3:17".into(), translation: "NIV".into(), verses: vec![VerseText { reference: "John 3:17".into(), text: "For God".into() }], per_slide: 0, text_size: 0 };
        let got = run(&data(vec![empty, full]), &all);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].starts_with("1. ") && got[0].ends_with(s::NO_VERSES), "{got:?}");
    }

    #[test]
    fn media_gone_from_the_library() {
        let m = media(MediaStatus::Ready);
        let d = data(vec![media_item(&m)]);
        let got = run(&d, &all);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].ends_with(s::MEDIA_GONE), "{got:?}");
    }

    #[test]
    fn media_file_missing_on_disk() {
        let m = media(MediaStatus::Ready);
        let mut d = data(vec![media_item(&m)]);
        d.media.push(m);
        assert_eq!(run(&d, &all), Vec::<String>::new());
        assert_eq!(run(&d, &none), [s::item(1, "Intro", s::FILE_MISSING)]);
    }

    #[test]
    fn media_that_failed_says_why() {
        let m = media(MediaStatus::Failed { reason: "not a video".into() });
        let mut d = data(vec![media_item(&m)]);
        d.media.push(m);
        assert_eq!(run(&d, &all), [s::item(1, "Intro", &s::cant_play("not a video"))]);
    }

    #[test]
    fn deck_counts_its_missing_slides() {
        let deck = Deck { id: Uuid::new_v4(), title: "Sermon".into(), slides: vec!["1.png".into(), "2.png".into(), "3.png".into()] };
        let mut d = data(vec![ItemKind::Deck { deck_id: deck.id }]);
        d.decks.push(deck);
        assert_eq!(run(&d, &|a| a == "2.png"), [s::item(1, "Sermon", &s::slides_missing(2, 3))]);
        assert_eq!(run(&d, &all), Vec::<String>::new());
    }

    #[test]
    fn deck_gone_from_the_library() {
        let got = run(&data(vec![ItemKind::Deck { deck_id: Uuid::new_v4() }]), &all);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].ends_with(s::DECK_GONE), "{got:?}");
    }

    #[test]
    fn an_items_own_background_file() {
        let mut d = data(vec![text()]);
        d.services[0].items[0].background = Some(Background::Video { asset: "loop.mp4".into() });
        assert_eq!(run(&d, &none), [s::item(1, "Welcome", s::ITEM_BACKGROUND_MISSING)]);
        assert_eq!(run(&d, &all), Vec::<String>::new());
    }

    #[test]
    fn only_the_current_service_is_checked() {
        let mut d = data(vec![text()]);
        let mut other = Service::new("Last week");
        other.items.push(item(ItemKind::Song { song_id: Uuid::new_v4() }));
        d.services.push(other);
        assert_eq!(run(&d, &all), Vec::<String>::new());
    }

    #[test]
    fn report_all_clear() {
        assert_eq!(report(&[]), s::ALL_CLEAR);
    }

    #[test]
    fn report_lists_each_line_under_a_count() {
        assert_eq!(report(&["one".to_string()]), format!("{}\n\none", s::found(1)));
        assert_eq!(report(&["one".to_string(), "two".to_string()]), format!("{}\n\none\ntwo", s::found(2)));
    }

    #[test]
    fn a_long_report_stops_and_says_how_many_are_left() {
        let lines: Vec<String> = (0..12).map(|i| format!("line {i}")).collect();
        let got = report(&lines);
        assert!(got.starts_with(&s::found(12)), "{got}");
        assert!(got.contains("line 7") && !got.contains("line 8"), "{got}");
        assert!(got.ends_with(&s::more(4)), "{got}");
    }
}
