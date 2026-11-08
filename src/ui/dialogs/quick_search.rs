//! N14 Quick search (Ctrl+F, Edit → Quick search…, `ui/dialogs/quick-search.slint`): one box that finds songs, media,
//! slide sets, this service's own items and other services. Enter opens the chosen result: a song or file already in
//! the service is selected there, another song opens in the song editor, a file is chosen in the media library, a
//! service is opened. Ctrl+Enter adds a song, file or slide set to the end of the service.
use super::{DialogKind, add::song_subtitle, song_editor::open_song_editor};
use crate::{
    model::{Data, ItemKind, MediaKind},
    tr,
    ui::{QuickHit, QuickSearchDlg, operator::{add_item::add_item_then, list_filter::matches, view::ItemRow, window::Operator}},
};
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};
use uuid::Uuid;

/// At most this many results are listed.
const MAX_HITS: usize = 60;

pub fn title() -> String { tr!("Quick search") }
fn type_hint() -> String { tr!("Type a song title, words from the lyrics, a file name or a service.") }
fn nothing_matches() -> String { tr!("Nothing matches.") }
fn tag_song() -> String { tr!("Song") }
fn tag_slides() -> String { tr!("Slides") }
fn tag_service() -> String { tr!("Service") }
fn tag_in_service() -> String { tr!("In this service") }
/// "Song · In this service".
fn tag_both(kind: &str) -> String { tr!("{kind} · In this service", kind = kind) }

/// What a result is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target { Song(Uuid), Media(Uuid), Deck(Uuid), Item(Uuid), Service(Uuid) }

/// One result: what it is, the item that already holds it in the open service, and what the row shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit { pub target: Target, pub item: Option<Uuid>, pub icon: &'static str, pub title: String, pub sub: String, pub tag: String }

impl Hit {
    /// Songs, files and slide sets can be added; this service's own items and other services can't.
    pub fn can_add(&self) -> bool { matches!(self.target, Target::Song(_) | Target::Media(_) | Target::Deck(_)) }
}

/// The results for `query`, best first: those whose title holds every word, then those that hold them elsewhere (a
/// song's lyrics, writers or CCLI number); A to Z within each. Nothing for an empty query.
pub fn search(data: &Data, items: &[ItemRow], query: &str) -> Vec<Hit> {
    if query.trim().is_empty() { return Vec::new() }
    let in_service = |want: &dyn Fn(&ItemKind) -> bool| items.iter().find(|i| want(&i.item.kind)).map(|i| i.id);
    // (rank, hit): 0 the title matches, 1 something else does.
    let mut found: Vec<(u8, Hit)> = Vec::new();
    let rank = |title: &str, more: &str| if matches(title, query) { Some(0) } else if matches(&format!("{title} {more}"), query) { Some(1) } else { None };
    for s in &data.songs {
        let Some(r) = rank(&s.title, &format!("{} {} {}", s.writers, s.ccli, s.lyrics)) else { continue };
        let item = in_service(&|k| matches!(k, ItemKind::Song { song_id, .. } if *song_id == s.id));
        let tag = if item.is_some() { tag_both(&tag_song()) } else { tag_song() };
        found.push((r, Hit { target: Target::Song(s.id), item, icon: "song", title: s.title.clone(), sub: song_subtitle(s), tag }));
    }
    for m in &data.media {
        let Some(r) = rank(&m.name, &m.folder) else { continue };
        let item = in_service(&|k| matches!(k, ItemKind::Media { media_id, .. } if *media_id == m.id));
        let (icon, kind) = match m.kind {
            MediaKind::Video => ("video", crate::ui::operator::strings::media_video()),
            MediaKind::Image => ("image", crate::ui::operator::strings::media_picture()),
            MediaKind::Audio => ("audio", crate::ui::operator::strings::media_audio()),
        };
        let tag = if item.is_some() { tag_both(&kind) } else { kind };
        found.push((r, Hit { target: Target::Media(m.id), item, icon, title: m.name.clone(), sub: m.folder.clone(), tag }));
    }
    for d in &data.decks {
        let Some(r) = rank(&d.title, "") else { continue };
        let item = in_service(&|k| matches!(k, ItemKind::Deck { deck_id } if *deck_id == d.id));
        let tag = if item.is_some() { tag_both(&tag_slides()) } else { tag_slides() };
        found.push((r, Hit { target: Target::Deck(d.id), item, icon: "deck", title: d.title.clone(), sub: String::new(), tag }));
    }
    // This service's passages and announcements (songs, files and slide sets are listed above).
    for it in items.iter().filter(|i| matches!(i.item.kind, ItemKind::Scripture { .. } | ItemKind::Text { .. })) {
        let Some(r) = rank(&it.title, &it.subtitle) else { continue };
        let icon = crate::ui::operator::service::kind_icon(&it.kind);
        found.push((r, Hit { target: Target::Item(it.id), item: Some(it.id), icon, title: it.title.clone(), sub: it.subtitle.clone(), tag: tag_in_service() }));
    }
    let order = crate::planning::date_order(&data.settings);
    for s in data.services.iter().filter(|s| Some(s.id) != data.current_service) {
        let date = s.date.map(|d| crate::planning::format_day_date(d, order)).unwrap_or_default();
        let Some(r) = rank(&s.name, &date) else { continue };
        found.push((r, Hit { target: Target::Service(s.id), item: None, icon: "book", title: s.name.clone(), sub: date, tag: tag_service() }));
    }
    found.sort_by_cached_key(|(r, h)| (*r, h.title.to_lowercase()));
    found.into_iter().take(MAX_HITS).map(|(_, h)| h).collect()
}

thread_local! {
    /// The open dialog's results, in the order they are listed.
    static HITS: RefCell<Vec<Hit>> = const { RefCell::new(Vec::new()) };
}

/// Opens the dialog with an empty box. Nothing over another dialog or before the first read.
pub fn open(op: &Rc<Operator>) {
    if op.covered() || op.state.borrow().snap.is_none() { return }
    op.open_dialog(DialogKind::QuickSearch);
    let g = op.ui.global::<QuickSearchDlg>();
    g.set_query("".into());
    show(op, "");
    let me = Rc::downgrade(op);
    g.on_edited(move |q| if let Some(op) = me.upgrade() { show(&op, &q) });
    let me = Rc::downgrade(op);
    g.on_open(move |i| if let Some(op) = me.upgrade() { pick(&op, i, false) });
    let me = Rc::downgrade(op);
    g.on_add(move |i| if let Some(op) = me.upgrade() { pick(&op, i, true) });
    op.on_dialog_close(|_| HITS.with(|h| h.borrow_mut().clear()));
}

/// Lists the results for `query`, the first one chosen.
fn show(op: &Operator, query: &str) {
    let hits = {
        let st = op.state.borrow();
        let Some(snap) = st.snap.as_ref() else { return };
        search(&snap.data, &snap.items, query)
    };
    let g = op.ui.global::<QuickSearchDlg>();
    let rows: Vec<QuickHit> = hits.iter().map(|h| QuickHit {
        icon: h.icon.into(), title: h.title.as_str().into(), sub: h.sub.as_str().into(), tag: h.tag.as_str().into(), can_add: h.can_add(),
    }).collect();
    g.set_note(if query.trim().is_empty() { type_hint() } else if hits.is_empty() { nothing_matches() } else { String::new() }.into());
    g.set_current(if hits.is_empty() { -1 } else { 0 });
    g.set_hits(ModelRc::new(VecModel::from(rows)));
    HITS.with(|h| *h.borrow_mut() = hits);
}

/// Enter (`add` false) or Ctrl+Enter (`add` true) on result `i`.
fn pick(op: &Rc<Operator>, i: i32, add: bool) {
    let Some(hit) = usize::try_from(i).ok().and_then(|i| HITS.with(|h| h.borrow().get(i).cloned())) else { return };
    let item: Option<Value> = match hit.target {
        Target::Song(id) => Some(json!({ "kind": "song", "song_id": id })),
        Target::Media(id) => Some(json!({ "kind": "media", "media_id": id, "autoplay": true, "end": "stop", "looped": false })),
        Target::Deck(id) => Some(json!({ "kind": "deck", "deck_id": id })),
        Target::Item(_) | Target::Service(_) => None,
    };
    if add && let Some(item) = item {
        return add_item_then(op, item, false, |op, _| op.close_dialog());
    }
    op.close_dialog();
    if let Some(id) = hit.item { return op.ui.invoke_service_select(id.to_string().into()) }
    match hit.target {
        Target::Song(id) => open_song_editor(op, Some(id), false),
        Target::Media(id) => op.ui.invoke_bin_pick(id.to_string().into()),
        // A slide set has nowhere else to open: it goes into the service.
        Target::Deck(id) => add_item_then(op, json!({ "kind": "deck", "deck_id": id }), false, |_, _| {}),
        Target::Service(id) => {
            let me = Rc::downgrade(op);
            op.ops().call::<Value>("service/select", json!({ "id": id }), move |r| if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true) });
        }
        Target::Item(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::{Deck, MediaFile, MediaStatus, Service, ServiceItem, Song},
        ui::operator::{view::OperatorSnapshot, window::testing::{find, find_all, open as open_window, refresh}},
    };
    use slint::{Model, platform::{Key, WindowEvent}};

    fn song(app: &App, title: &str, lyrics: &str) -> Uuid {
        let s = Song { id: Uuid::new_v4(), title: title.into(), lyrics: lyrics.into(), ..Default::default() };
        app.data.lock().unwrap().songs.push(s.clone());
        s.id
    }

    fn item(app: &App, kind: ItemKind) -> Uuid {
        let it = ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(it.clone());
        it.id
    }

    fn titles(hits: &[Hit]) -> Vec<&str> { hits.iter().map(|h| h.title.as_str()).collect() }

    fn press(op: &Operator, text: impl Into<slint::SharedString>) {
        let text = text.into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text });
    }

    fn ctrl(op: &Operator, text: impl Into<slint::SharedString>) {
        let control: slint::SharedString = char::from(Key::Control).into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: control.clone() });
        press(op, text);
        op.window().dispatch_event(WindowEvent::KeyReleased { text: control });
    }

    fn typed(op: &Operator, q: &str) {
        let field = find(op, "QuickSearchDialog::search").expect("the search box");
        field.set_accessible_value(q);
    }

    #[test]
    fn titles_first_then_lyrics_and_every_kind() {
        let app = test_app();
        song(&app, "Amazing Grace", "how sweet the sound");
        song(&app, "Be Thou My Vision", "amazing love");
        song(&app, "Holy Holy Holy", "x");
        let video = MediaFile { id: Uuid::new_v4(), name: "Amazing sunrise.mp4".into(), kind: MediaKind::Video, asset: "a.mp4".into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        let deck = Deck { id: Uuid::new_v4(), title: "Amazing announcements".into(), slides: Vec::new(), notes: vec![] };
        {
            let mut d = app.data.lock().unwrap();
            d.media.push(video.clone());
            d.decks.push(deck);
            d.services.push(Service::new("Amazing Easter"));
        }
        item(&app, ItemKind::Media { media_id: video.id, autoplay: true, end: Default::default(), looped: false });
        let snap = OperatorSnapshot::read(&app);
        let hits = search(&snap.data, &snap.items, "AMAZING");
        assert_eq!(titles(&hits), ["Amazing announcements", "Amazing Easter", "Amazing Grace", "Amazing sunrise.mp4", "Be Thou My Vision"],
            "title matches A–Z, then the lyrics match");
        let video_hit = hits.iter().find(|h| h.target == Target::Media(video.id)).unwrap();
        assert_eq!(video_hit.tag, "Video · In this service");
        assert!(video_hit.item.is_some());
        assert!(hits.iter().find(|h| h.title == "Amazing Easter").is_some_and(|h| !h.can_add()), "a service is opened, not added");
        assert!(search(&snap.data, &snap.items, "  ").is_empty());
        assert_eq!(titles(&search(&snap.data, &snap.items, "sweet grace")), ["Amazing Grace"], "words from the title and the lyrics together");
    }

    #[test]
    fn ctrl_f_opens_it_and_ctrl_enter_adds_the_song() {
        let app = test_app();
        let id = song(&app, "Amazing Grace", "a");
        song(&app, "Holy Holy Holy", "b");
        let op = open_window(&app);
        refresh(&op);
        ctrl(&op, "f");
        assert_eq!(op.dialog(), Some(DialogKind::QuickSearch));
        let g = op.ui.global::<QuickSearchDlg>();
        assert_eq!(g.get_note(), "Type a song title, words from the lyrics, a file name or a service.");
        typed(&op, "grace");
        assert_eq!(g.get_hits().row_count(), 1);
        assert_eq!(g.get_current(), 0);
        assert_eq!(find_all(&op, "QuickSearchDialog::hit").len(), 1);
        ctrl(&op, "\n");
        let items = app.data.lock().unwrap().services[0].items.clone();
        assert!(matches!(items.as_slice(), [ServiceItem { kind: ItemKind::Song { song_id, .. }, .. }] if *song_id == id));
        assert_eq!(op.dialog(), None, "adding closes it");
        assert_eq!(op.selected_item(), Some(items[0].id));
        assert!(op.toast_log().iter().any(|(t, _)| t == "Added to the service"));
    }

    #[test]
    fn enter_selects_what_the_service_has_or_opens_the_song() {
        let app = test_app();
        let in_service = song(&app, "Amazing Grace", "a");
        let other = song(&app, "Amazing Love", "b");
        let it = item(&app, ItemKind::Song { song_id: in_service, arrangement: None });
        item(&app, ItemKind::Text { title: "Welcome".into(), body: "hi".into(), layout: Default::default() });
        let op = open_window(&app);
        refresh(&op);
        op.ui.invoke_menu("find".into());
        typed(&op, "amazing");
        let g = op.ui.global::<QuickSearchDlg>();
        assert_eq!(g.get_hits().row_count(), 2);
        // The first is "Amazing Grace", already in the service: Enter selects it.
        op.ui.invoke_service_select(Uuid::nil().to_string().into());
        press(&op, "\n");
        assert_eq!(op.dialog(), None);
        assert_eq!(op.selected_item(), Some(it));
        // Down to "Amazing Love": not in the service, so Enter opens the song editor.
        op.ui.invoke_menu("find".into());
        typed(&op, "amazing");
        press(&op, char::from(Key::DownArrow));
        assert_eq!(g.get_current(), 1);
        press(&op, "\n");
        assert_eq!(op.dialog(), Some(DialogKind::SongEditor { song_id: Some(other), add_after: false }));
        assert_eq!(app.data.lock().unwrap().services[0].items.len(), 2, "Enter never adds");
    }

    #[test]
    fn enter_opens_another_service_and_nothing_matches_says_so() {
        let app = test_app();
        let s = Service::new("Easter Sunday");
        let easter = s.id;
        app.data.lock().unwrap().services.push(s);
        let op = open_window(&app);
        refresh(&op);
        op.ui.invoke_menu("find".into());
        typed(&op, "zzz");
        assert_eq!(op.ui.global::<QuickSearchDlg>().get_note(), "Nothing matches.");
        typed(&op, "easter");
        press(&op, "\n");
        assert_eq!(app.data.lock().unwrap().current_service, Some(easter));
        assert_eq!(op.dialog(), None);
    }
}
