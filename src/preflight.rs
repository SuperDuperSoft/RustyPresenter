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
/// `cached(bible, reference)` says whether an online Bible's passage is in its capped cache (Nov 5 spec §5).
pub fn check(data: &Data, monitors: &[Monitor], audio: &[String], has_asset: &dyn Fn(&str) -> bool, cached: &dyn Fn(&str, &str) -> bool) -> Vec<String> {
    let mut out = vec![];
    let connected = |id: &str| monitors.iter().any(|m| m.id == id);
    let screens = &data.settings.screens;
    let room = screens.monitors("audience");
    if room.is_empty() { out.push(strings::no_room()) }
    else if !room.into_iter().all(connected) { out.push(strings::room_gone()) }
    if screens.stage.on && !screens.monitors("stage").into_iter().all(connected) { out.push(strings::stage_gone().into()); }
    let sound = data.settings.audio_device.as_str();
    if !sound.is_empty() && !audio.is_empty() && !audio.iter().any(|a| a == sound) { out.push(strings::audio_gone().into()); }
    if data.settings.logo.as_deref().is_some_and(|l| !has_asset(l)) { out.push(strings::logo_missing().into()); }
    if !background_ok(&data.settings.background, has_asset) { out.push(strings::background_missing().into()); }
    let Some(service) = data.service() else { return out };
    if service.items.is_empty() { out.push(strings::empty_service().into()); }
    for (i, item) in service.items.iter().enumerate() {
        let (title, _, slides) = build_item(data, item);
        let mut problems = content_problems(data, &item.kind, slides.is_empty(), has_asset, cached);
        if item.background.as_ref().is_some_and(|b| !background_ok(b, has_asset)) { problems.push(strings::item_background_missing().into()); }
        out.extend(problems.iter().map(|p| strings::item(i + 1, &title, p)));
    }
    out
}

fn background_ok(b: &Background, has_asset: &dyn Fn(&str) -> bool) -> bool {
    match b {
        Background::Color { .. } | Background::Gradient { .. } => true,
        Background::Image { asset, .. } | Background::Video { asset, .. } => has_asset(asset),
    }
}

fn content_problems(data: &Data, kind: &ItemKind, no_slides: bool, has_asset: &dyn Fn(&str) -> bool, cached: &dyn Fn(&str, &str) -> bool) -> Vec<String> {
    let one = |s: String| vec![s];
    match kind {
        ItemKind::Song { song_id } if !data.songs.iter().any(|s| s.id == *song_id) => one(strings::song_missing()),
        ItemKind::Song { .. } if no_slides => one(strings::song_empty()),
        ItemKind::Scripture { reference, translation, verses, also, .. } => {
            // Each online Bible whose passage would not show without internet; a missing main text says only that.
            let uncached: Vec<&str> = std::iter::once(translation.as_str()).chain(also.iter().map(|a| a.translation.as_str()))
                .filter(|t| crate::bible::sources::is_online(t) && !cached(t, reference)).collect();
            let abbr = |t: &str| data.bible_credit(t).map(|c| c.abbr.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| t.to_string());
            let mut out = if verses.is_empty() && !uncached.contains(&translation.as_str()) { one(strings::no_verses()) } else { vec![] };
            out.extend(uncached.into_iter().map(|t| strings::not_offline(&abbr(t), reference)));
            out
        }
        ItemKind::Media { media_id, .. } => match data.media.iter().find(|m| m.id == *media_id) {
            None => one(strings::media_gone()),
            Some(MediaFile { status: MediaStatus::Failed { reason }, .. }) => vec![strings::cant_play(reason)],
            Some(m) if !has_asset(&m.asset) => one(strings::file_missing()),
            Some(_) => vec![],
        },
        ItemKind::Deck { deck_id } => match data.decks.iter().find(|d| d.id == *deck_id) {
            None => one(strings::deck_gone()),
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
    if problems.is_empty() { return strings::all_clear().into(); }
    let mut text = format!("{}\n", strings::found(problems.len()));
    for p in problems.iter().take(SHOWN) { text.push('\n'); text.push_str(p); }
    if problems.len() > SHOWN { text.push('\n'); text.push_str(&strings::more(problems.len() - SHOWN)); }
    text
}

#[cfg(test)]
mod tests {
    use super::{strings as s, *};
    use crate::model::{Deck, EndAction, MediaKind, ScreenTarget, Service, ServiceItem, Song, VerseText};
    use uuid::Uuid;

    fn monitor(id: &str) -> Monitor { Monitor { id: id.into(), label: format!("Screen {id}"), x: 0, y: 0, width: 1920, height: 1080, primary: false } }

    /// A service with these items, the room on a connected screen.
    fn data(items: Vec<ItemKind>) -> Data {
        let mut d = Data::default();
        d.settings.screens.assign("audience", ScreenTarget::Monitor("room".into()));
        let mut service = Service::new("Sunday");
        service.items = items.into_iter().map(item).collect();
        d.current_service = Some(service.id);
        d.services = vec![service];
        d
    }
    fn item(kind: ItemKind) -> ServiceItem { ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None } }
    fn text() -> ItemKind { ItemKind::Text { title: "Welcome".into(), body: "Hello".into() } }
    fn all(_: &str) -> bool { true }
    fn none(_: &str) -> bool { false }
    fn run(d: &Data, has: &dyn Fn(&str) -> bool) -> Vec<String> { check(d, &[monitor("room")], &[], has, &|_, _| true) }

    fn media(status: MediaStatus) -> MediaFile {
        MediaFile { id: Uuid::new_v4(), name: "Intro".into(), kind: MediaKind::Video, asset: "a.mp4".into(), status, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() }
    }
    fn media_item(m: &MediaFile) -> ItemKind { ItemKind::Media { media_id: m.id, autoplay: true, end: EndAction::Stop, looped: false } }

    #[test]
    fn a_ready_service_has_nothing_to_fix() {
        assert_eq!(run(&data(vec![text()]), &all), Vec::<String>::new());
    }

    #[test]
    fn an_empty_service_is_flagged() {
        assert_eq!(run(&data(vec![]), &all), [s::empty_service()]);
    }

    #[test]
    fn no_room_screen_chosen() {
        let mut d = data(vec![text()]);
        d.settings.screens.assign("audience", ScreenTarget::None);
        assert_eq!(run(&d, &all), [s::no_room()]);
    }

    #[test]
    fn room_screen_not_connected_comes_first() {
        let mut d = data(vec![ItemKind::Song { song_id: Uuid::new_v4() }]);
        d.settings.screens.assign("audience", ScreenTarget::Monitor(r"\\.\DISPLAY2".into()));
        let got = run(&d, &all);
        assert_eq!(got[0], s::room_gone());
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn a_room_screen_is_found_by_its_id_among_several() {
        let d = data(vec![text()]);
        assert_eq!(check(&d, &[monitor("other")], &[], &all, &|_, _| true), [s::room_gone()]);
        assert_eq!(check(&d, &[monitor("other"), monitor("room")], &[], &all, &|_, _| true), Vec::<String>::new());
    }

    #[test]
    fn stage_screen_gone_is_flagged_but_none_chosen_is_fine() {
        let mut d = data(vec![text()]);
        assert_eq!(run(&d, &all), Vec::<String>::new());
        d.settings.screens.assign("stage", ScreenTarget::Monitor("stage".into()));
        assert_eq!(run(&d, &all), [s::stage_gone()]);
        assert_eq!(check(&d, &[monitor("room"), monitor("stage")], &[], &all, &|_, _| true), Vec::<String>::new());
    }

    #[test]
    fn a_chosen_sound_output_that_is_gone_is_flagged() {
        let mut d = data(vec![text()]);
        let rooms = [monitor("room")];
        d.settings.audio_device = "wasapi/{board}".into();
        assert_eq!(check(&d, &rooms, &["wasapi/{laptop}".to_string()], &all, &|_, _| true), [s::audio_gone()]);
        assert_eq!(check(&d, &rooms, &["wasapi/{board}".to_string()], &all, &|_, _| true), Vec::<String>::new());
        assert_eq!(check(&d, &rooms, &[], &all, &|_, _| true), Vec::<String>::new(), "no list: it cannot tell");
        d.settings.audio_device.clear();
        assert_eq!(check(&d, &rooms, &["wasapi/{laptop}".to_string()], &all, &|_, _| true), Vec::<String>::new(), "the default is always there");
    }

    #[test]
    fn missing_logo_and_default_background_files() {
        let mut d = data(vec![text()]);
        d.settings.logo = Some("logo.png".into());
        d.settings.background = Background::Image { asset: "bg.png".into(), dim: 0 };
        assert_eq!(run(&d, &none), [s::logo_missing(), s::background_missing()]);
        assert_eq!(run(&d, &|a| a == "logo.png"), [s::background_missing()]);
    }

    #[test]
    fn a_song_gone_from_the_library_names_its_place() {
        let d = data(vec![text(), ItemKind::Song { song_id: Uuid::new_v4() }]);
        assert_eq!(run(&d, &all), [s::item(2, "Missing song", &s::song_missing())]);
    }

    #[test]
    fn a_song_without_words() {
        let song = Song { id: Uuid::new_v4(), title: "Silent".into(), ..Song::default() };
        let mut d = data(vec![ItemKind::Song { song_id: song.id }]);
        d.songs.push(song);
        assert_eq!(run(&d, &all), [s::item(1, "Silent", &s::song_empty())]);
    }

    #[test]
    fn scripture_with_no_verses_is_flagged_and_with_verses_needs_nothing_else() {
        let empty = ItemKind::Scripture { reference: "John 3:16".into(), translation: "NIV".into(), verses: vec![], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() };
        let full = ItemKind::Scripture { reference: "John 3:17".into(), translation: "NIV".into(), verses: vec![VerseText::new("John 3:17", "For God")], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() };
        let got = run(&data(vec![empty, full]), &all);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].starts_with("1. ") && got[0].ends_with(&s::no_verses()), "{got:?}");
    }

    #[test]
    fn media_gone_from_the_library() {
        let m = media(MediaStatus::Ready);
        let d = data(vec![media_item(&m)]);
        let got = run(&d, &all);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].ends_with(&s::media_gone()), "{got:?}");
    }

    #[test]
    fn media_file_missing_on_disk() {
        let m = media(MediaStatus::Ready);
        let mut d = data(vec![media_item(&m)]);
        d.media.push(m);
        assert_eq!(run(&d, &all), Vec::<String>::new());
        assert_eq!(run(&d, &none), [s::item(1, "Intro", &s::file_missing())]);
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
        assert!(got[0].ends_with(&s::deck_gone()), "{got:?}");
    }

    #[test]
    fn an_items_own_background_file() {
        let mut d = data(vec![text()]);
        d.services[0].items[0].background = Some(Background::Video { asset: "loop.mp4".into(), dim: 0 });
        assert_eq!(run(&d, &none), [s::item(1, "Welcome", &s::item_background_missing())]);
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
        assert_eq!(report(&[]), s::all_clear());
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

    fn niv_item() -> ItemKind {
        ItemKind::Scripture { reference: "John 3:16".into(), translation: "KJV".into(), verses: vec![VerseText::new("John 3:16", "For God")], per_slide: 0, text_size: 0,
            also: vec![crate::scripture::AlsoShow { translation: "NIV".into(), verses: vec![] }], layout: Default::default(), breaks: Default::default() }
    }

    #[test]
    fn preflight_flags_uncached_online_passage() {
        let out = check(&data(vec![niv_item()]), &[monitor("room")], &[], &all, &|_, _| false);
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].ends_with("NIV isn't ready offline for John 3:16. Open this service once with internet."), "{out:?}");
        let mut online_main = niv_item();
        if let ItemKind::Scripture { translation, verses, also, .. } = &mut online_main { *translation = "ESV".into(); verses.clear(); also.clear() }
        let out = check(&data(vec![online_main]), &[monitor("room")], &[], &all, &|_, _| false);
        assert_eq!(out.len(), 1, "one line, not also \"no verse text\": {out:?}");
        assert!(out[0].ends_with("ESV isn't ready offline for John 3:16. Open this service once with internet."));
    }

    #[test]
    fn preflight_quiet_when_cached() {
        let asked = std::cell::RefCell::new(vec![]);
        let cached = |t: &str, r: &str| { asked.borrow_mut().push(format!("{t} {r}")); true };
        assert_eq!(check(&data(vec![niv_item()]), &[monitor("room")], &[], &all, &cached), Vec::<String>::new());
        assert_eq!(*asked.borrow(), ["NIV John 3:16"], "stored Bibles are not asked about");
    }
}
