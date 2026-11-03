//! Add to service (`openAdd()` and its tabs, `operator.js:378-511`): five tiles above the current tab. The Song,
//! Bible and Announcement tabs are here; Video & media and Slides are in `add_media`. The dialog's fields and lists
//! live in the `AddState` global (`ui/dialogs/add-list.slint`).
use super::{AddTab, DialogKind, add_strings as tr, song_editor::open_song_editor};
use crate::{
    model::Song,
    slides::section_label,
    ui::{AddRow, AddState, operator::{add_item::add_item_then, file_input::Filter, files, window::Operator}, thumbs::Thumbs},
};
use serde::Deserialize;
use serde_json::{Value, json};
use slint::{ComponentHandle, FilterModel, Model, ModelRc, VecModel};
use std::{cell::{OnceCell, RefCell}, rc::{Rc, Weak}};
use uuid::Uuid;

type Rows = FilterModel<Rc<VecModel<AddRow>>, Box<dyn Fn(&AddRow) -> bool>>;

/// The open Add dialog's list: every row of the current tab, and the rows the search lets through.
pub(crate) struct AddCtx {
    pub(crate) op: Weak<Operator>,
    pub(crate) all: Rc<VecModel<AddRow>>,
    term: Rc<RefCell<String>>,
    shown: Rc<Rows>,
    /// The Media and Slides rows' pictures, made on first use.
    thumbs: OnceCell<Thumbs>,
}

impl AddCtx {
    fn new(op: &Rc<Operator>) -> Rc<AddCtx> {
        let all: Rc<VecModel<AddRow>> = Rc::default();
        let term: Rc<RefCell<String>> = Rc::default();
        let t = term.clone();
        // `JSON.stringify(x).toLowerCase().includes(term)` (product decision 5): `key` is the row's JSON, lowercased.
        let filter: Box<dyn Fn(&AddRow) -> bool> = Box::new(move |r| { let t = t.borrow(); t.is_empty() || r.key.contains(t.as_str()) });
        let shown = Rc::new(FilterModel::new(all.clone(), filter));
        Rc::new(AddCtx { op: Rc::downgrade(op), all, term, shown, thumbs: OnceCell::new() })
    }

    /// Draws the rows' pictures on a worker thread, or at once in tests.
    pub(crate) fn thumbs(&self, op: &Operator) -> &Thumbs {
        self.thumbs.get_or_init(|| {
            let root = op.app().store.root.clone();
            if op.ops().is_inline() { Thumbs::inline(root) } else { Thumbs::new(root) }
        })
    }
}

fn tab_index(tab: AddTab) -> i32 {
    match tab { AddTab::Song => 0, AddTab::Scripture => 1, AddTab::Media => 2, AddTab::Deck => 3, AddTab::Text => 4 }
}

fn tab_at(i: i32) -> Option<AddTab> {
    Some(match i { 0 => AddTab::Song, 1 => AddTab::Scripture, 2 => AddTab::Media, 3 => AddTab::Deck, 4 => AddTab::Text, _ => return None })
}

/// `openAdd(tab)`: opens the dialog on `tab` with empty fields.
pub fn open_add(op: &Rc<Operator>, tab: AddTab) {
    op.open_dialog(DialogKind::Add(tab));
    let st = op.ui.global::<AddState>();
    for clear in [AddState::set_search, AddState::set_reference, AddState::set_pasted, AddState::set_text_title, AddState::set_text_body] {
        clear(&st, "".into());
    }
    st.set_paste_open(false);
    st.set_per_slide(i32::from(op.bible_per_slide()));
    let ctx = AddCtx::new(op);
    st.set_rows(ModelRc::from(ctx.shown.clone()));
    wire(op, &ctx);
    show_tab(op, &ctx, tab);
    let redraw = ctx.clone();
    op.on_dialog_refresh(move |op| draw_list(op, &redraw));
}

/// A tile: the tab's content is made new, so its search starts empty and its first field takes the keyboard.
fn show_tab(op: &Rc<Operator>, ctx: &AddCtx, tab: AddTab) {
    op.state.borrow_mut().dialog = Some(DialogKind::Add(tab));
    let st = op.ui.global::<AddState>();
    st.set_search("".into());
    ctx.term.borrow_mut().clear();
    st.set_tab(tab_index(tab));
    draw_list(op, ctx);
}

/// The current tab's rows from the newest snapshot (`dialogRefresh`, `operator.js:388`). Only the list is rebuilt:
/// what the user typed in the search and the fields stays.
fn draw_list(op: &Rc<Operator>, ctx: &AddCtx) {
    let Some(DialogKind::Add(tab)) = op.dialog() else { return };
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let st = op.ui.global::<AddState>();
    st.set_translation(snap.translation.as_str().into());
    st.set_can_paste(!snap.available.contains(&snap.translation));
    let (rows, placeholder, pictures) = match tab {
        AddTab::Song => {
            let mut songs: Vec<&Song> = snap.data.songs.iter().collect();
            songs.sort_by_cached_key(|s| s.title.to_lowercase());
            (songs.into_iter().map(song_row).collect(), tr::search_songs(snap.data.songs.len()), Vec::new())
        }
        AddTab::Media | AddTab::Deck => super::add_media::rows(&snap, tab),
        AddTab::Scripture | AddTab::Text => (Vec::new(), String::new(), Vec::new()),
    };
    ctx.all.set_vec(rows);
    st.set_placeholder(placeholder.into());
    show_empty(op, ctx);
    // Once the rows are in the model, so a picture drawn at once finds its row.
    super::add_media::request_pictures(op, ctx, pictures);
}

/// "Nothing matches." while searching, "Nothing here yet." otherwise, when no row is shown.
fn show_empty(op: &Operator, ctx: &AddCtx) {
    let empty = if ctx.shown.row_count() > 0 { "" } else if ctx.term.borrow().is_empty() { tr::NOTHING_YET } else { tr::NOTHING_MATCHES };
    op.ui.global::<AddState>().set_empty(empty.into());
}

/// The search field changed: `q.value.trim().toLowerCase()`.
fn search(op: &Operator, ctx: &AddCtx, text: &str) {
    *ctx.term.borrow_mut() = text.trim().to_lowercase();
    ctx.shown.reset();
    show_empty(op, ctx);
}

/// A library entry's search key: its JSON, lowercased.
pub(crate) fn search_key(x: &impl serde::Serialize) -> slint::SharedString {
    serde_json::to_string(x).unwrap_or_default().to_lowercase().into()
}

fn song_row(s: &Song) -> AddRow {
    AddRow {
        id: s.id.to_string().into(), title: s.title.as_str().into(), subtitle: song_subtitle(s).into(), icon: "song".into(),
        can_edit: true, key: search_key(s), ..Default::default()
    }
}

/// `[writers, ccli && "CCLI #n"].filter(Boolean).join(' · ') || firstLyric(s)` (`operator.js:421`).
pub fn song_subtitle(s: &Song) -> String {
    let ccli = (!s.ccli.is_empty()).then(|| tr::ccli(&s.ccli));
    let parts: Vec<&str> = [Some(s.writers.as_str()), ccli.as_deref()].into_iter().flatten().filter(|p| !p.is_empty()).collect();
    if parts.is_empty() { first_lyric(&s.lyrics) } else { parts.join(" · ") }
}

/// `firstLyric()` (`operator.js:430`): the first line that is not blank and not a section name, at most 80
/// characters.
pub fn first_lyric(lyrics: &str) -> String {
    lyrics.split('\n').map(|l| l.trim_end_matches('\r'))
        .find(|l| !l.trim().is_empty() && section_label(l).is_none())
        .map(|l| l.chars().take(80).collect())
        .unwrap_or_default()
}

/// The library row's id, as the core's `Uuid`.
fn row_id(id: &str) -> Option<Uuid> { Uuid::parse_str(id).ok() }

/// `bible/lookup`'s and `bible/paste`'s answer. The verses pass through to `service/add` untouched.
#[derive(Deserialize)]
struct Found { label: String, verses: Value, translation: String }

/// Add verses (Enter in the reference) and Add pasted verses: the passage goes at the end, quietly, then the toast
/// "`<label>` added" and the dialog closes (`operator.js:432-445`).
fn add_verses(op: &Rc<Operator>, pasted: bool) {
    let st = op.ui.global::<AddState>();
    let translation = op.state.borrow().snap.as_ref().map(|s| s.translation.clone()).unwrap_or_else(|| "KJV".into());
    let (action, body) = if pasted {
        ("bible/paste", json!({ "reference": st.get_reference().as_str(), "translation": translation, "text": st.get_pasted().as_str() }))
    } else {
        ("bible/lookup", json!({ "reference": st.get_reference().as_str(), "translation": translation }))
    };
    let me = Rc::downgrade(op);
    op.ops().call::<Found>(action, body, move |r| {
        let Some(op) = me.upgrade() else { return };
        let Found { label, verses, translation } = match r { Ok(f) => f, Err(e) => return op.toast(e, true) };
        let item = json!({ "kind": "scripture", "reference": label, "translation": translation, "verses": verses, "per_slide": op.bible_per_slide() });
        add_item_then(&op, item, true, move |op, _| {
            op.toast(tr::bible_added(&label), false);
            op.close_dialog();
        });
    });
}

/// Add announcement (`operator.js:504-509`).
fn add_text(op: &Rc<Operator>) {
    let st = op.ui.global::<AddState>();
    let title = st.get_text_title().trim().to_string();
    if title.is_empty() { return op.toast(tr::GIVE_IT_A_TITLE, true) }
    let item = json!({ "kind": "text", "title": title, "body": st.get_text_body().as_str() });
    add_item_then(op, item, false, |op, _| op.close_dialog());
}

/// A row's Add: the item goes at the end with the toast "Added to the service", and the dialog closes.
fn add_row(op: &Rc<Operator>, id: Uuid) {
    let item = match op.dialog() {
        Some(DialogKind::Add(AddTab::Song)) => json!({ "kind": "song", "song_id": id }),
        Some(DialogKind::Add(AddTab::Media)) => json!({ "kind": "media", "media_id": id, "autoplay": true, "end": "stop", "looped": false }),
        Some(DialogKind::Add(AddTab::Deck)) => json!({ "kind": "deck", "deck_id": id }),
        _ => return,
    };
    add_item_then(op, item, false, |op, _| op.close_dialog());
}

/// Points `AddState`'s callbacks at this dialog. Set on each open; they hold the context and a `Weak` window.
fn wire(op: &Rc<Operator>, ctx: &Rc<AddCtx>) {
    let st = op.ui.global::<AddState>();
    let with = |f: fn(&Rc<Operator>, &Rc<AddCtx>)| {
        let ctx = ctx.clone();
        move || if let Some(op) = ctx.op.upgrade() { f(&op, &ctx) }
    };
    let with_id = |f: fn(&Rc<Operator>, &Rc<AddCtx>, Uuid)| {
        let ctx = ctx.clone();
        move |id: slint::SharedString| if let (Some(op), Some(id)) = (ctx.op.upgrade(), row_id(&id)) { f(&op, &ctx, id) }
    };
    let c = ctx.clone();
    st.on_pick_tab(move |i| if let (Some(op), Some(tab)) = (c.op.upgrade(), tab_at(i)) { show_tab(&op, &c, tab) });
    let c = ctx.clone();
    st.on_search_edited(move |t| if let Some(op) = c.op.upgrade() { search(&op, &c, &t) });
    st.on_new_song(with(|op, _| open_song_editor(op, None, true)));
    st.on_import_lyrics(with(|op, _| {
        let me = Rc::downgrade(op);
        files::pick(op, Filter::Lyrics, true, move |paths| if let Some(op) = me.upgrade() { files::add_files(&op, paths) });
    }));
    st.on_edit_song(with_id(|op, _, id| open_song_editor(op, Some(id), false)));
    st.on_add_row(with_id(|op, _, id| add_row(op, id)));
    st.on_bible_add(with(|op, _| add_verses(op, false)));
    st.on_bible_paste(with(|op, _| add_verses(op, true)));
    let c = ctx.clone();
    st.on_per_slide_picked(move |i| if let Some(op) = c.op.upgrade() { op.set_bible_per_slide(u8::try_from(i).unwrap_or(0)) });
    st.on_text_add(with(|op, _| add_text(op)));
    super::add_media::wire(op, ctx);
}

/// The service column's "Add to service" opens the dialog on the Song tab (`operator.js:348`).
pub(crate) fn wire_add_button(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ui.on_service_add(move || if let Some(op) = me.upgrade() { open_add(&op, AddTab::Song) });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::{ItemKind, Song},
        ui::operator::window::testing::{find, find_all, open, refresh},
    };
    use slint::{SharedString, platform::{Key, PointerEventButton, WindowEvent}};
    use std::sync::Arc;

    pub(crate) fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1400.0, 900.0));
        refresh(&op);
        op
    }

    /// Runs what a frame would: change handlers, and making the dialog's parts (whose `init` takes the keyboard).
    pub(crate) fn settle(op: &Operator) {
        slint::platform::update_timers_and_animations();
        let _ = find(op, "DialogShell::close-btn");
        let _ = find_all(op, "AddListRow::add-btn");
        slint::platform::update_timers_and_animations();
    }

    pub(crate) fn press(op: &Operator, text: impl Into<SharedString>) {
        let text = text.into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text });
    }

    pub(crate) fn type_text(op: &Operator, text: &str) { for c in text.chars() { press(op, c) } }

    pub(crate) fn click(op: &Operator, id: &str) {
        find(op, id).unwrap_or_else(|| panic!("{id} on screen")).mock_single_click(PointerEventButton::Left);
        settle(op);
    }

    pub(crate) fn song(app: &App, title: &str, writers: &str, ccli: &str, lyrics: &str) -> Uuid {
        let s = Song { id: Uuid::new_v4(), title: title.into(), writers: writers.into(), ccli: ccli.into(), lyrics: lyrics.into(), ..Default::default() };
        app.data.lock().unwrap().songs.push(s.clone());
        s.id
    }

    pub(crate) fn items(app: &App) -> Vec<ItemKind> { app.data.lock().unwrap().services[0].items.iter().map(|i| i.kind.clone()).collect() }

    pub(crate) fn shown(op: &Operator) -> Vec<AddRow> { op.ui.global::<AddState>().get_rows().iter().collect() }

    pub(crate) fn titles(op: &Operator) -> Vec<String> { shown(op).iter().map(|r| r.title.to_string()).collect() }

    pub(crate) fn last_toast(op: &Operator) -> Option<(String, bool)> { op.toast_log().last().cloned() }

    const TILES: [&str; 5] = ["AddDialog::song-tile", "AddDialog::bible-tile", "AddDialog::media-tile", "AddDialog::deck-tile", "AddDialog::text-tile"];

    #[test]
    fn add_dialog_opens_on_song_tab_from_add_button() {
        let app = test_app();
        song(&app, "Amazing Grace", "", "", "Verse 1\nAmazing grace");
        let op = opened(&app);
        click(&op, "ServiceColumn::add-btn");
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Song)));
        assert_eq!(op.ui.get_dialog_title(), "Add to service");
        assert!(!op.ui.get_dialog_wide());
        let st = op.ui.global::<AddState>();
        assert_eq!(st.get_tab(), 0);
        assert_eq!(titles(&op), ["Amazing Grace"]);
        let tiles: Vec<(String, bool)> = TILES.iter().map(|id| find(&op, id).unwrap())
            .map(|t| (t.accessible_label().unwrap_or_default().to_string(), t.accessible_checked().unwrap_or(false))).collect();
        assert_eq!(tiles.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["Song", "Bible", "Video & media", "Slides", "Announcement"]);
        assert_eq!(tiles.iter().filter(|t| t.1).map(|t| t.0.as_str()).collect::<Vec<_>>(), ["Song"]);
        // The search field has the keyboard: typing goes into it.
        type_text(&op, "zz");
        settle(&op);
        assert_eq!(st.get_search(), "zz");
    }

    #[test]
    fn song_tab_search_filters_and_empty_messages() {
        let app = test_app();
        let op = opened(&app);
        open_add(&op, AddTab::Song);
        settle(&op);
        let st = op.ui.global::<AddState>();
        assert_eq!(st.get_placeholder(), "Search 0 songs…");
        assert_eq!(st.get_empty(), "Nothing here yet.");
        song(&app, "Amazing Grace", "John Newton", "", "Verse 1\nAmazing grace");
        song(&app, "Be Thou My Vision", "", "", "Verse 1\nBe thou my vision");
        song(&app, "abide with me", "", "22222", "Verse 1\nAbide with me");
        refresh(&op);
        assert_eq!(titles(&op), ["abide with me", "Amazing Grace", "Be Thou My Vision"], "sorted by title, any case");
        assert_eq!(st.get_placeholder(), "Search 3 songs…");
        assert_eq!(st.get_empty(), "");
        // Any field matches, case-insensitively, typed into the field.
        type_text(&op, " NEWTON ");
        settle(&op);
        assert_eq!(titles(&op), ["Amazing Grace"]);
        type_text(&op, "x");
        settle(&op);
        assert!(titles(&op).is_empty());
        assert_eq!(st.get_empty(), "Nothing matches.");
        // A refresh keeps the search and its filter.
        refresh(&op);
        assert_eq!(st.get_search(), " NEWTON x");
        assert_eq!(st.get_empty(), "Nothing matches.");
        song(&app, "Only One", "", "", "");
        refresh(&op);
        assert_eq!(st.get_placeholder(), "Search 4 songs…");
        let one = op.ui.global::<AddState>();
        one.invoke_search_edited("22222".into());
        assert_eq!(titles(&op), ["abide with me"], "the CCLI number is a field too");
        assert_eq!(search_key(&app.data.lock().unwrap().songs[0]).contains("newton"), true);
    }

    #[test]
    fn song_row_subtitle_falls_back_to_first_lyric() {
        let s = |writers: &str, ccli: &str, lyrics: &str| Song { writers: writers.into(), ccli: ccli.into(), lyrics: lyrics.into(), ..Default::default() };
        assert_eq!(song_subtitle(&s("John Newton", "4768151", "")), "John Newton · CCLI #4768151");
        assert_eq!(song_subtitle(&s("", "4768151", "")), "CCLI #4768151");
        assert_eq!(song_subtitle(&s("John Newton", "", "")), "John Newton");
        assert_eq!(song_subtitle(&s("", "", "Verse 1\r\n\r\n  Amazing grace how sweet\r\nThat saved")), "  Amazing grace how sweet");
        assert_eq!(song_subtitle(&s("", "", "[Chorus]\nChorus\n")), "");
        let long = "a".repeat(100);
        assert_eq!(first_lyric(&long).chars().count(), 80);
        assert_eq!(first_lyric("Verse 1\né".repeat(1).as_str()), "é");
    }

    #[test]
    fn add_song_adds_and_closes() {
        let app = test_app();
        let id = song(&app, "Amazing Grace", "", "", "Verse 1\nAmazing grace");
        let op = opened(&app);
        open_add(&op, AddTab::Song);
        settle(&op);
        click(&op, "AddListRow::add-btn");
        assert!(matches!(items(&app)[..], [ItemKind::Song { song_id }] if song_id == id));
        assert_eq!(op.dialog(), None);
        assert_eq!(last_toast(&op), Some(("Added to the service".into(), false)));
        let new_id = app.data.lock().unwrap().services[0].items[0].id;
        assert_eq!(op.selected_item(), Some(new_id), "the new item is selected");
    }

    #[test]
    fn add_failure_keeps_dialog_open() {
        let app = test_app();
        let id = song(&app, "Amazing Grace", "", "", "Verse 1\nAmazing grace");
        let op = opened(&app);
        open_add(&op, AddTab::Song);
        { let mut d = app.data.lock().unwrap(); d.services.clear(); d.current_service = None; }
        op.ui.global::<AddState>().invoke_add_row(id.to_string().into());
        assert_eq!(last_toast(&op), Some(("No service is open.".into(), true)));
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Song)));
    }

    #[test]
    fn bible_tab_appends_at_end_and_toasts_label() {
        let app = test_app();
        let text = |title: &str| {
            let item = crate::model::ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: "x".into() }, background: None, notes: String::new() };
            app.data.lock().unwrap().services[0].items.push(item.clone());
            item.id
        };
        let first = text("Welcome");
        text("Offering");
        let op = opened(&app);
        op.ops().cmd(crate::live::Command::Show { item_id: first, slide: 0 }, None, |o| assert!(o.ok));
        op.apply_live(crate::app::LiveUpdate::new(app.live_view()));
        open_add(&op, AddTab::Song);
        settle(&op);
        click(&op, "AddDialog::bible-tile");
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Scripture)));
        let st = op.ui.global::<AddState>();
        let translation = op.state.borrow().snap.as_ref().unwrap().translation.clone();
        assert_eq!(st.get_translation(), translation.as_str());
        // The reference field has the keyboard; Enter adds.
        type_text(&op, "Gen 1:1");
        press(&op, char::from(Key::Return));
        let it = items(&app);
        assert_eq!(it.len(), 3);
        assert!(matches!(&it[2], ItemKind::Scripture { reference, .. } if reference == "Genesis 1:1"), "at the end, not after the live item: {:?}", it[2]);
        assert_eq!(last_toast(&op), Some(("Genesis 1:1 added".into(), false)), "quiet add, then the label");
        assert_eq!(op.dialog(), None);
    }

    #[test]
    fn bible_tab_adds_with_the_chosen_verses_per_slide() {
        let app = test_app();
        app.data.lock().unwrap().settings.bible_per_slide = 3;
        let op = opened(&app);
        open_add(&op, AddTab::Scripture);
        settle(&op);
        let st = op.ui.global::<AddState>();
        assert_eq!(st.get_per_slide(), 3, "starts at the Settings choice");
        assert!(find(&op, "AddBibleTab::per-slide").is_some());
        st.set_per_slide(1);
        st.invoke_per_slide_picked(1);
        st.set_reference("Psalm 23:1-3".into());
        click(&op, "AddBibleTab::add");
        assert!(matches!(&items(&app)[0], ItemKind::Scripture { per_slide: 1, .. }), "{:?}", items(&app)[0]);
        assert_eq!(op.ui.get_bible_per_slide(), 1, "the Bible box remembers the choice too");
        open_add(&op, AddTab::Scripture);
        assert_eq!(op.ui.global::<AddState>().get_per_slide(), 1, "remembered for the session");
    }

    #[test]
    fn bible_lookup_error_keeps_dialog_open() {
        let app = test_app();
        let op = opened(&app);
        open_add(&op, AddTab::Scripture);
        settle(&op);
        op.ui.global::<AddState>().set_reference("Nowhere 99:99".into());
        click(&op, "AddBibleTab::add");
        assert!(last_toast(&op).is_some_and(|t| t.1), "an error toast");
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Scripture)));
        assert!(items(&app).is_empty());
    }

    #[test]
    fn announcement_needs_a_title() {
        let app = test_app();
        let op = opened(&app);
        open_add(&op, AddTab::Text);
        settle(&op);
        // The title field has the keyboard.
        type_text(&op, "   ");
        click(&op, "AddTextTab::add");
        assert_eq!(last_toast(&op), Some(("Give it a title.".into(), true)));
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Text)));
        let st = op.ui.global::<AddState>();
        st.set_text_title("  Welcome ".into());
        st.set_text_body("Hello\n\nAgain".into());
        click(&op, "AddTextTab::add");
        assert!(matches!(&items(&app)[..], [ItemKind::Text { title, body }] if title == "Welcome" && body == "Hello\n\nAgain"));
        assert_eq!(last_toast(&op), Some(("Added to the service".into(), false)));
        assert_eq!(op.dialog(), None);
    }

    #[test]
    fn opening_again_starts_with_empty_fields() {
        let app = test_app();
        let op = opened(&app);
        open_add(&op, AddTab::Text);
        op.ui.global::<AddState>().set_text_title("Half typed".into());
        op.close_dialog();
        open_add(&op, AddTab::Text);
        assert_eq!(op.ui.global::<AddState>().get_text_title(), "");
    }

    /// Tab and Shift+Tab, `n` times each way, with the given dialog or question open.
    fn tab_around(op: &Operator, n: usize) {
        for k in [Key::Tab, Key::Backtab] {
            for _ in 0..n { press(op, k); settle(op) }
        }
    }

    #[test]
    fn tab_stays_inside_an_open_dialog() {
        let app = test_app();
        let op = opened(&app);
        open_add(&op, AddTab::Text);
        settle(&op);
        for _ in 0..12 { press(&op, Key::Tab); settle(&op) }
        type_text(&op, "John 3:16");
        press(&op, Key::Return);
        settle(&op);
        assert_eq!(op.ui.get_bible_text(), "", "Tab never reached the Bible box under the scrim");
        assert!(items(&app).is_empty(), "nothing was added behind the dialog");
        tab_around(&op, 7);
        press(&op, Key::Escape);
        settle(&op);
        assert!(op.state.borrow().dialog.is_none(), "Esc still closes the dialog");
    }

    #[test]
    fn tab_stays_on_a_question() {
        let app = test_app();
        let op = opened(&app);
        let answer = Rc::new(std::cell::Cell::new(None));
        let a = answer.clone();
        super::super::ask::confirm(&op, "Delete?", "Delete", "Cancel", true, move |ok| a.set(Some(ok)));
        let _ = find(&op, "AskOverlay::ok");
        settle(&op);
        tab_around(&op, 5);
        type_text(&op, "John 3:16");
        assert_eq!(op.ui.get_bible_text(), "");
        press(&op, Key::Escape);
        settle(&op);
        assert_eq!(answer.get(), Some(false), "Esc still answers the question");
    }

    #[test]
    fn a_bible_submit_under_a_dialog_does_nothing() {
        let app = test_app();
        let op = opened(&app);
        open_add(&op, AddTab::Song);
        crate::ui::operator::bible::submit(&op, "John 3:16".into());
        assert!(items(&app).is_empty());
    }
}
