//! The Bible box in the top bar (`operator.js:93-102, 324-340`): type a reference and press Enter to add the
//! passage after the live item and arm it, so the next Enter outside the box shows it. The Bible chips (the chosen
//! one, then favourites and recent ones) and the picker behind the chosen chip pick the translation, saved in
//! `Settings.translation` (product decision 3; Bible library spec §7.2). The Verses chip picks how many verses
//! go on each slide of the next passage: Settings' choice until the operator picks, then theirs for the session
//! (shared with Add → Bible).
use super::{strings as tr, view, window::Operator};
use serde::Deserialize;
use serde_json::{Value, json};
use crate::ui::TranslationChip;
use slint::{Model, ModelRc, SharedString, VecModel};
use std::{rc::{Rc, Weak}, sync::Arc};
use uuid::Uuid;

/// `bible/lookup`'s answer. The verses pass through to `service/add` untouched.
#[derive(Deserialize)]
pub(crate) struct Found { label: String, verses: Value, translation: String }

/// `service/add`'s answer: the new id, and what to say when an Also Bible was left out.
#[derive(Deserialize)]
struct Added { id: Uuid, #[serde(default)] toast: Option<String> }

/// Enter in the Bible box (`operator.js:325-336`): look the passage up, add it after the live item (or at the end),
/// clear the box, give the keys back, and select and arm the new item. A failure is an error toast with the core's
/// text, and the box keeps what was typed. Nothing happens while a dialog or question is open over the box. Words
/// that are not a reference open Find a verse instead.
pub fn submit(op: &Rc<Operator>, reference: String) {
    let reference = reference.trim().to_string();
    if reference.is_empty() || op.covered() { return }
    // Words, not a reference: find the verse by its words (FEAT-5 §3.1).
    if crate::scripture::input::classify_bible_input(&reference) == crate::scripture::input::BibleInput::Words {
        return crate::ui::dialogs::find_verse::open(op, &reference);
    }
    let translation = op.state.borrow().snap.as_ref().map(|s| s.translation.clone()).unwrap_or_else(|| "KJV".into());
    let me = Rc::downgrade(op);
    op.ops.call::<Found>("bible/lookup", json!({ "reference": reference, "translation": translation }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(found) => add(&op, found),
            Err(e) => op.toast(&e, true),
        }
    });
}

/// The second half of `submit`: the passage goes in with the translation the core returned, not the chip state
/// (`operator.js:330`).
pub(crate) fn add(op: &Rc<Operator>, found: Found) {
    let live_item = op.live_item();
    let position = op.state.borrow().snap.as_ref().and_then(|s| view::insert_position(&s.items, live_item));
    let Found { label, verses, translation } = found;
    let item = json!({ "kind": "scripture", "reference": label, "translation": translation, "verses": verses, "per_slide": op.bible_per_slide() });
    let me = Rc::downgrade(op);
    op.ops.call::<Added>("service/add", json!({ "position": position, "item": item }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(Added { id, toast }) => {
                op.ui.set_bible_text("".into());
                // Only the box gives up the keyboard (the web's `input.blur()`): a dialog or question opened while
                // the lookup ran keeps it.
                if !op.covered() { op.ui.invoke_focus_keys() }
                {
                    let mut st = op.state.borrow_mut();
                    st.selected = Some(id);
                    st.armed = Some(id);
                }
                // The core's `Data` update may already have been applied with the old selection.
                op.mark_service();
                op.render_slides();
                op.toast(&tr::bible_added(&label), false);
                super::bible_extras::remember(&op, &label);
                if let Some(t) = toast { op.toast(&t, true) }
            }
            Err(e) => op.toast(&e, true),
        }
    });
}

/// The chip at `index` (0 the chosen Bible, then the favourite and recent chips), or Alt+1…5 in the box.
pub fn pick(op: &Rc<Operator>, index: usize) {
    let Some(chip) = op.ui.get_translations().row_data(index) else { return };
    pick_id(op, chip.id.to_string());
}

/// Chooses the Bible `id` (a chip or a picker row). The chips change at once, with it first and recent
/// updated as the core will, and the choice is saved through `settings/translation` (product decision 3). A Bible
/// that is not connected is still picked, as in the web; its lookup then fails with the core's message. The
/// chip's click refocuses the box in `topbar.slint`.
pub fn pick_id(op: &Rc<Operator>, id: String) {
    {
        let mut st = op.state.borrow_mut();
        let Some(snap) = st.snap.as_mut() else { return };
        if !snap.translations.contains(&id) { return }
        let snap = Arc::make_mut(snap);
        snap.settings.used_translation(&id);
        snap.translation = id.clone();
    }
    rerender(op);
    let me = Rc::downgrade(op);
    op.ops.call::<Value>("settings/translation", json!({ "translation": id }), move |r| {
        if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(&e, true) }
    });
}

/// The star in the picker: stars or unstars `id`, at once on screen, then through `settings/favourite`.
pub fn toggle_favourite(op: &Rc<Operator>, id: String) {
    let (on, label) = {
        let mut st = op.state.borrow_mut();
        let Some(snap) = st.snap.as_mut() else { return };
        let snap = Arc::make_mut(snap);
        let on = !snap.settings.favourites.iter().any(|f| crate::bible::id::same_id(f, &id));
        snap.settings.set_favourite(&id, on);
        (on, view::bible_labels(snap).get(&id).cloned().unwrap_or_else(|| id.clone()))
    };
    rerender(op);
    let me = Rc::downgrade(op);
    op.ops.call::<Value>("settings/favourite", json!({ "translation": id, "on": on }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => op.toast(&if on { tr::favourite_added(&label) } else { tr::favourite_removed(&label) }, false),
            Err(e) => op.toast(&e, true),
        }
    });
}

/// Redraws the chips and the picker from the newest snapshot.
fn rerender(op: &Operator) {
    let snap = op.state.borrow().snap.clone();
    if let Some(s) = snap { render_chips(&op.ui, &s) }
}

fn chip(c: &view::BibleChoice, on: bool) -> TranslationChip {
    TranslationChip {
        id: c.id.as_str().into(), name: c.label.as_str().into(), full_name: c.name.as_str().into(),
        on, off: c.off, favourite: c.favourite, heading: SharedString::new(),
    }
}

/// The Bible box's chips and the picker's lines (spec §7.2) from `s`; the picker keeps its search.
pub(super) fn render_chips(ui: &crate::ui::OperatorWindow, s: &view::OperatorSnapshot) {
    let chips: Vec<TranslationChip> = view::bible_chips(s).iter().enumerate().map(|(i, c)| chip(c, i == 0)).collect();
    ui.set_translation(chips.first().map(|c| c.name.clone()).unwrap_or_default());
    ui.set_quick_chips(ModelRc::new(VecModel::from(chips.iter().skip(1).cloned().collect::<Vec<_>>())));
    ui.set_translations(ModelRc::new(VecModel::from(chips)));
    ui.set_also_label(also_label(s).into());
    fill_picker(ui, s, &ui.get_picker_search());
}

/// A Bible picker's lines for `query`, with `chosen` marked, and the note shown when nothing matches. The Bible box's
/// picker and Add → Bible's share this.
pub(crate) fn picker_rows(s: &view::OperatorSnapshot, query: &str, chosen: &str) -> (ModelRc<TranslationChip>, SharedString) {
    let rows: Vec<TranslationChip> = view::picker_lines(s, query).iter().map(|l| match l {
        view::PickerLine::Heading(h) => TranslationChip { heading: h.as_str().into(), ..Default::default() },
        view::PickerLine::Bible(c) => chip(c, c.id == chosen),
    }).collect();
    let note = if rows.is_empty() { tr::picker_no_match().into() } else { SharedString::new() };
    (ModelRc::new(VecModel::from(rows)), note)
}

/// The first Bible a picker lists (Enter in its search).
pub(crate) fn first_picked(rows: &ModelRc<TranslationChip>) -> Option<String> {
    rows.iter().find(|r| r.heading.is_empty()).map(|r| r.id.to_string())
}

/// The Bible box's picker lines for `query`.
fn fill_picker(ui: &crate::ui::OperatorWindow, s: &view::OperatorSnapshot, query: &str) {
    let (rows, note) = picker_rows(s, query, &s.translation);
    ui.set_picker_note(note);
    ui.set_picker_rows(rows);
}

/// The picker's search changed (or the picker is opening, with an empty search).
pub fn search_picker(op: &Operator, query: &str) {
    let snap = op.state.borrow().snap.clone();
    if let Some(s) = snap { fill_picker(&op.ui, &s, query) }
}

/// Enter in the picker's search: the first Bible listed.
pub fn accept_picker(op: &Rc<Operator>) {
    if let Some(id) = first_picked(&op.ui.get_picker_rows()) { pick_id(op, id) }
}

impl Operator {
    /// Verses per slide for the next passage: picked this session, else Settings' (0 = automatic).
    pub(crate) fn bible_per_slide(&self) -> u8 {
        let st = self.state.borrow();
        st.bible_per_slide.or(st.snap.as_ref().map(|s| s.settings.bible_per_slide)).unwrap_or(0).min(4)
    }

    /// The operator picked verses per slide (the Verses chip or Add → Bible): kept for the session.
    pub(crate) fn set_bible_per_slide(&self, n: u8) {
        let n = n.min(4);
        self.state.borrow_mut().bible_per_slide = Some(n);
        self.ui.set_bible_per_slide(i32::from(n));
    }
}

/// The Side by side chip's words: the short names of the Bibles every new passage is also shown in ("NIV · RVR1909"),
/// empty with none.
pub fn also_label(s: &view::OperatorSnapshot) -> String {
    s.settings.also_show.iter().map(|id| {
        s.bible_sources.iter().find(|b| crate::bible::id::same_id(&b.translation, id)).map_or_else(|| id.clone(), |b| b.label().to_string())
    }).collect::<Vec<_>>().join(" · ")
}

/// The Side by side chip: Settings → Bible, at the Bibles shown with every new passage.
pub fn open_also(op: &Rc<Operator>) {
    use crate::ui::dialogs::{SettingsTab, settings};
    settings::open(op, SettingsTab::Bible);
    settings::go(op, SettingsTab::Bible, "also-show");
}

/// The Verses chip: Automatic → 1 → 2 → 3 → 4 → Automatic.
pub fn cycle_per_slide(op: &Operator) { op.set_bible_per_slide((op.bible_per_slide() + 1) % 5) }

/// Connects the box and its chips. The callbacks hold a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_bible_submit(move |text| if let Some(op) = me.upgrade() { submit(&op, text.into()) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_pick_translation(move |i| if let (Some(op), Ok(i)) = (me.upgrade(), usize::try_from(i)) { pick(&op, i) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_pick_bible(move |id| if let Some(op) = me.upgrade() { pick_id(&op, id.into()) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_toggle_favourite(move |id| if let Some(op) = me.upgrade() { toggle_favourite(&op, id.into()) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_picker_opening(move || if let Some(op) = me.upgrade() { search_picker(&op, "") });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_picker_search_changed(move |q| if let Some(op) = me.upgrade() {
        // Kept here for every window: the picker may be in a window of its own.
        op.ui.set_picker_search(q.clone());
        search_picker(&op, &q)
    });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_picker_accept(move || if let Some(op) = me.upgrade() { accept_picker(&op) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_find_more_bibles(move |q| if let Some(op) = me.upgrade() { crate::ui::dialogs::settings::open_bible_search(&op, &q) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_cycle_per_slide(move || if let Some(op) = me.upgrade() { cycle_per_slide(&op) });
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.on_open_also(move || if let Some(op) = me.upgrade() { open_also(&op) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, LiveUpdate, test_app},
        live::Command,
        model::{ItemKind, ServiceItem},
        ui::operator::window::testing::{find, find_all, open, refresh},
    };
    use slint::{SharedString, platform::{Key, PointerEventButton, WindowEvent}};
    use std::sync::Arc;
    use uuid::Uuid;

    fn add_text(app: &App, title: &str) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: "x".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        item.id
    }

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1400.0, 900.0));
        // The window's `changed width` runs here, not in the middle of the first click (it moves the Bible box).
        slint::platform::update_timers_and_animations();
        refresh(&op);
        live(&op);
        op
    }

    fn live(op: &Operator) { op.apply_live(LiveUpdate::new(op.app().live_view())); }

    fn show(op: &Operator, item_id: Uuid) {
        op.ops().cmd(Command::Show { item_id, slide: 0 }, None, |o| assert!(o.ok, "{o:?}"));
        live(op);
    }

    fn ids(app: &App) -> Vec<Uuid> { app.data.lock().unwrap().services[0].items.iter().map(|i| i.id).collect() }

    fn cursor(app: &App) -> Option<(Uuid, usize)> { app.live_view().cursor.map(|c| (c.item_id, c.slide)) }

    fn press(op: &Operator, text: impl Into<SharedString>) {
        let text = text.into();
        op.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text });
    }

    /// Clicks into the box, types `reference` and presses Enter, as the operator would.
    fn type_and_enter(op: &Operator, reference: &str) {
        find(op, "BibleBox::bible-input").expect("Bible input").mock_single_click(PointerEventButton::Left);
        for c in reference.chars() { press(op, c) }
        press(op, char::from(Key::Return));
    }

    fn last_toast(op: &Operator) -> Option<(String, bool)> { op.state.borrow().toasts.last().cloned() }

    fn chord(op: &Operator, modifier: Key, text: &str) {
        op.window().dispatch_event(WindowEvent::KeyPressed { text: char::from(modifier).into() });
        press(op, text);
        op.window().dispatch_event(WindowEvent::KeyReleased { text: char::from(modifier).into() });
    }

    fn chip_names(op: &Operator) -> Vec<String> { op.ui.get_translations().iter().map(|c| c.name.to_string()).collect() }

    fn picker(op: &Operator) -> Vec<String> {
        op.ui.get_picker_rows().iter().map(|r| if r.heading.is_empty() { format!("{}{}", r.name, if r.favourite { " *" } else { "" }) } else { format!("# {}", r.heading) }).collect()
    }

    #[test]
    fn bible_enter_inserts_after_live_item_and_arms_it() {
        let app = test_app();
        let (a, b) = (add_text(&app, "A"), add_text(&app, "B"));
        let op = opened(&app);
        show(&op, a);
        type_and_enter(&op, "John 3:16");
        let now = ids(&app);
        assert_eq!(now.len(), 3);
        assert_eq!((now[0], now[2]), (a, b), "the passage goes right after the live item");
        let new = now[1];
        let st = op.state.borrow();
        assert_eq!((st.selected, st.armed), (Some(new), Some(new)));
        drop(st);
        assert_eq!(op.ui.get_bible_text(), "", "the box is cleared");
        assert_eq!(last_toast(&op), Some(("John 3:16 added — press Enter to show it".into(), false)));
    }

    #[test]
    fn bible_enter_appends_when_nothing_live() {
        let app = test_app();
        let a = add_text(&app, "A");
        let op = opened(&app);
        type_and_enter(&op, "John 3:16");
        let now = ids(&app);
        assert_eq!(now.len(), 2);
        assert_eq!(now[0], a);
        assert_eq!(op.state.borrow().armed, Some(now[1]));
    }

    #[test]
    fn bible_enter_says_which_also_bible_was_left_out() {
        let app = test_app();
        app.data.lock().unwrap().settings.also_show = vec!["WEB".into(), "NIV".into()];
        let op = opened(&app);
        type_and_enter(&op, "John 3:16");
        assert_eq!(ids(&app).len(), 1, "the passage went in");
        let toasts = op.state.borrow().toasts.clone();
        assert!(toasts.contains(&("NIV isn't available right now, so only KJV · WEB was added.".to_string(), true)), "{toasts:?}");
    }

    #[test]
    fn bible_item_uses_translation_from_lookup() {
        let app = test_app();
        let op = opened(&app);
        pick_id(&op, "WEB".into());
        type_and_enter(&op, "jn 3:16");
        let data = app.data.lock().unwrap();
        match &data.services[0].items[0].kind {
            ItemKind::Scripture { reference, translation, verses, .. } => {
                assert_eq!(translation, "WEB");
                assert_eq!(reference, "John 3:16", "the item carries the label the lookup returned");
                assert_eq!(verses.len(), 1);
            }
            other => panic!("not scripture: {other:?}"),
        }
    }

    #[test]
    fn second_enter_takes_armed_passage_live() {
        let app = test_app();
        let a = add_text(&app, "A");
        let op = opened(&app);
        show(&op, a);
        type_and_enter(&op, "Psalm 23:1-2");
        let new = ids(&app)[1];
        refresh(&op);
        assert_eq!(cursor(&app), Some((a, 0)), "the first Enter only adds and arms");
        press(&op, char::from(Key::Return));
        assert_eq!(cursor(&app), Some((new, 0)));
        assert_eq!(op.state.borrow().armed, None);
    }

    #[test]
    fn next_key_clears_the_arm() {
        let app = test_app();
        let op = opened(&app);
        type_and_enter(&op, "John 3:16");
        refresh(&op);
        assert!(op.state.borrow().armed.is_some());
        press(&op, char::from(Key::RightArrow));
        assert_eq!(op.state.borrow().armed, None);
    }

    #[test]
    fn unknown_book_shows_core_error_and_keeps_text() {
        let app = test_app();
        let op = opened(&app);
        type_and_enter(&op, "Hezekiah 1:1");
        assert_eq!(last_toast(&op), Some(("I don't recognise the book \"Hezekiah\".".into(), true)));
        assert_eq!(op.ui.get_bible_text(), "Hezekiah 1:1");
        assert!(ids(&app).is_empty());
        assert_eq!(op.state.borrow().armed, None);
    }

    #[test]
    fn unavailable_translation_errors_with_core_message() {
        let app = test_app();
        let op = opened(&app);
        let core = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
            .block_on(app.do_action("bible/lookup", br#"{"reference":"John 3:16","translation":"NIV"}"#)).unwrap_err().message;
        pick_id(&op, "NIV".into());
        type_and_enter(&op, "John 3:16");
        assert_eq!(last_toast(&op), Some((core, true)));
        assert_eq!(op.ui.get_bible_text(), "John 3:16");
        assert!(ids(&app).is_empty());
    }

    #[test]
    fn chip_tooltip_is_its_name_or_how_to_connect_it() {
        let app = test_app();
        let op = opened(&app);
        toggle_favourite(&op, "NIV".into());
        refresh(&op);
        op.window().set_size(slint::LogicalSize::new(1440.0, 900.0));
        slint::platform::update_timers_and_animations();
        let chips = find_all(&op, "BibleBox::translation-chip");
        let tip = |name: &str| chips.iter().find(|c| c.accessible_label().unwrap_or_default() == name)
            .and_then(|c| c.accessible_description()).unwrap_or_default().to_string();
        assert_eq!(tip("WEB"), "World English Bible");
        assert_eq!(tip("NIV"), "NIV isn't connected yet (Settings → Bible). You can still paste its verses with Add → Bible.");
    }

    /// The Side by side chip names the Bibles shown with every passage, and opens the place to choose them: any
    /// Bible in the list, one that is not connected yet too.
    #[test]
    fn the_side_by_side_chip_names_the_bibles_and_opens_their_setting() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        assert_eq!(op.ui.get_also_label(), "", "none: the chip says Off");
        app.data.lock().unwrap().settings.also_show = vec!["WEB".into(), "KJV".into()];
        refresh(&op);
        assert_eq!(op.ui.get_also_label(), "WEB · KJV");
        use slint::ComponentHandle as _;
        op.ui.invoke_open_also();
        let g = crate::ui::dialogs::settings_window::win(&op).global::<crate::ui::SettingsDlg>();
        assert_eq!(g.get_tab(), crate::ui::dialogs::settings::tab_index(crate::ui::dialogs::SettingsTab::Bible));
        assert_eq!(g.get_highlight(), "also-show");
        let snap = op.state.borrow().snap.clone().unwrap();
        let chips: Vec<String> = crate::ui::dialogs::settings_bible::also_chips(&snap).into_iter().map(|c| c.0).collect();
        for id in ["ESV", "NIV", "KJV", "WEB"].into_iter().filter(|id| *id != snap.translation) { assert!(chips.iter().any(|c| c == id), "{id} can be picked: {chips:?}") }
        assert!(!chips.iter().any(|c| *c == snap.translation), "but not the Bible it already is");
    }

    #[test]
    fn favourites_then_recent_follow_the_chosen_chip() {
        let app = test_app();
        let op = opened(&app);
        assert_eq!(chip_names(&op), ["KJV", "WEB", "BSB"]);
        toggle_favourite(&op, "ESV".into());
        assert_eq!(chip_names(&op), ["KJV", "ESV", "WEB", "BSB"], "a star shows at once, before the Bibles that only fill in");
        assert_eq!(app.data.lock().unwrap().settings.favourites, ["ESV"], "and is saved");
        pick_id(&op, "NIV".into());
        pick_id(&op, "BSB".into());
        assert_eq!(chip_names(&op), ["BSB", "ESV", "NIV", "KJV", "WEB"], "the chosen one, the favourite, then newest first");
        refresh(&op);
        assert_eq!(chip_names(&op), ["BSB", "ESV", "NIV", "KJV", "WEB"], "the core agrees");
        assert_eq!(app.data.lock().unwrap().settings.recent, ["BSB", "NIV"]);
        pick_id(&op, "WEB".into());
        assert_eq!(chip_names(&op).len(), 5, "at most four after the chosen one");
        assert_eq!(chip_names(&op), ["WEB", "ESV", "BSB", "NIV", "KJV"]);
        // Alt+2 picks the second chip.
        find(&op, "BibleBox::bible-input").unwrap().mock_single_click(PointerEventButton::Left);
        chord(&op, Key::Alt, "2");
        assert_eq!(app.data.lock().unwrap().settings.translation, "ESV");
    }

    #[test]
    fn picker_lists_favourites_recent_then_languages_and_searches() {
        let app = test_app();
        let op = opened(&app);
        toggle_favourite(&op, "WEB".into());
        pick_id(&op, "NIV".into());
        pick_id(&op, "KJV".into());
        refresh(&op);
        op.open_bible_picker();
        let all = picker(&op);
        assert_eq!(&all[..5], ["# Favourites", "WEB *", "# Recent", "KJV", "NIV"]);
        assert_eq!(all[5], "# English");
        assert!(all.contains(&"BSB".to_string()) && all.iter().filter(|l| *l == "KJV").count() == 1, "each Bible once: {all:?}");
        search_picker(&op, "world");
        assert_eq!(picker(&op), ["# Favourites", "WEB *"], "by name");
        search_picker(&op, "zzz");
        assert!(picker(&op).is_empty());
        assert_eq!(op.ui.get_picker_note(), tr::picker_no_match());
        search_picker(&op, "bsb");
        accept_picker(&op);
        assert_eq!(app.data.lock().unwrap().settings.translation, "BSB", "Enter picks the first Bible listed");
    }

    #[test]
    fn picker_star_and_row_click() {
        let app = test_app();
        let op = opened(&app);
        op.window().dispatch_event(WindowEvent::PointerMoved { position: slint::LogicalPosition::new(0.0, 0.0) });
        find(&op, "BibleBox::translation-pick").expect("the chosen chip").mock_single_click(PointerEventButton::Left);
        slint::platform::update_timers_and_animations();
        let stars = find_all(&op, "BiblePickerList::star");
        assert!(!stars.is_empty(), "the picker is open");
        let rows = picker(&op).into_iter().filter(|l| !l.starts_with('#')).collect::<Vec<_>>();
        let esv = rows.iter().position(|r| r == "ESV").expect("ESV listed");
        stars[esv].mock_single_click(PointerEventButton::Left);
        assert_eq!(app.data.lock().unwrap().settings.favourites, ["ESV"]);
        assert_eq!(last_toast(&op), Some(("ESV added to favourites".into(), false)));
    }

    #[test]
    fn find_more_opens_the_library_with_the_search() {
        let app = test_app();
        let op = opened(&app);
        op.open_bible_picker();
        op.ui.set_picker_search("Reina".into());
        op.ui.invoke_find_more_bibles("Reina".into());
        assert!(crate::ui::dialogs::settings::is_open(&op), "Settings opened");
        let g = slint::ComponentHandle::global::<crate::ui::SettingsDlg>(crate::ui::dialogs::settings_window::win(&op));
        assert_eq!(g.get_tab(), crate::ui::dialogs::settings::tab_index(crate::ui::dialogs::SettingsTab::Bible));
        assert_eq!(g.get_catalog_search(), "Reina");
    }

    #[test]
    fn translation_choice_survives_restart() {
        let app = test_app();
        let op = opened(&app);
        pick_id(&op, "NIV".into());
        let on = |op: &Operator| slint::Model::iter(&op.ui.get_translations()).filter(|c| c.on).map(|c| c.name.to_string()).collect::<Vec<_>>();
        assert_eq!(on(&op), ["NIV"], "the chip turns on at once");
        assert_eq!(app.data.lock().unwrap().settings.translation, "NIV");
        drop(op);
        let again = opened(&app);
        assert_eq!(on(&again), ["NIV"]);
    }

    #[test]
    fn chip_click_picks_and_refocuses_the_box() {
        let app = test_app();
        let op = opened(&app);
        // The first pointer event lays the chips out; a position read before it can be stale.
        op.window().dispatch_event(WindowEvent::PointerMoved { position: slint::LogicalPosition::new(0.0, 0.0) });
        let chips = find_all(&op, "BibleBox::translation-chip");
        let web = chips.iter().find(|c| c.accessible_label().unwrap_or_default() == "WEB").expect("WEB chip");
        web.mock_single_click(PointerEventButton::Left);
        assert_eq!(app.data.lock().unwrap().settings.translation, "WEB");
        press(&op, "J");
        assert_eq!(op.ui.get_bible_text(), "J", "typing goes to the Bible box");
    }

    fn per_slide_of(app: &App, i: usize) -> u8 {
        match &app.data.lock().unwrap().services[0].items[i].kind { ItemKind::Scripture { per_slide, .. } => *per_slide, k => panic!("{k:?}") }
    }

    #[test]
    fn new_passages_use_the_settings_verses_per_slide() {
        let app = test_app();
        app.data.lock().unwrap().settings.bible_per_slide = 2;
        let op = opened(&app);
        assert_eq!(op.ui.get_bible_per_slide(), 2, "the chip shows the Settings choice");
        type_and_enter(&op, "Psalm 23:1-4");
        assert_eq!(per_slide_of(&app, 0), 2);
    }

    #[test]
    fn verses_chip_picks_per_slide_for_the_session() {
        let app = test_app();
        let op = opened(&app);
        assert_eq!(op.ui.get_bible_per_slide(), 0, "Automatic by default");
        let chip = find(&op, "BibleBox::per-slide-chip").expect("Verses chip");
        chip.mock_single_click(PointerEventButton::Left);
        chip.mock_single_click(PointerEventButton::Left);
        assert_eq!(op.ui.get_bible_per_slide(), 2);
        type_and_enter(&op, "Psalm 23:1-4");
        assert_eq!(per_slide_of(&app, 0), 2);
        refresh(&op);
        assert_eq!(op.ui.get_bible_per_slide(), 2, "remembered across updates");
        for _ in 0..3 { cycle_per_slide(&op) }
        assert_eq!(op.ui.get_bible_per_slide(), 0, "round to Automatic again");
        assert_eq!(app.data.lock().unwrap().settings.bible_per_slide, 0, "the session pick does not change Settings");
    }

    #[test]
    fn empty_input_does_nothing() {
        let app = test_app();
        let op = opened(&app);
        submit(&op, "   ".into());
        assert!(ids(&app).is_empty());
        assert!(op.state.borrow().toasts.is_empty());
    }
}
