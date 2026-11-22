//! N6, the Bible extras: the Bible box's history (its last references, one click to add again), Prev and Next
//! verse on the selected passage (the verse just before or after it goes in after the live item, armed, as from the
//! box), Save to library (the passage as a song the church can edit and use again), and About this Bible (where a
//! translation comes from and its licence). The whole passage already shows at once as the numbered slide grid.
use super::{bible, view, window::Operator};
use crate::model::{ItemKind, Song, VerseText};
use crate::scripture::strings as ss;
use serde_json::{Value, json};
use slint::{ModelRc, SharedString, VecModel};
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// The selected passage: its reference, Bible and verses.
struct Passage { reference: String, translation: String, verses: Vec<VerseText> }

fn selected_passage(op: &Operator) -> Option<(Passage, view::ItemRow)> {
    let st = op.state.borrow();
    let row = st.selected.and_then(|id| st.snap.as_ref()?.items.iter().find(|i| i.id == id))?.clone();
    match &row.item.kind {
        ItemKind::Scripture { reference, translation, verses, .. } =>
            Some((Passage { reference: reference.clone(), translation: translation.clone(), verses: verses.clone() }, row)),
        _ => None,
    }
}

/// The history and whether a passage is selected, from the newest snapshot and selection.
pub(super) fn render(op: &Operator) {
    let history: Vec<SharedString> = op.state.borrow().snap.as_ref()
        .map(|s| s.settings.bible_history.iter().map(|r| r.as_str().into()).collect()).unwrap_or_default();
    let g = op.ui.panel_extras_all();
    g.set_history(ModelRc::new(VecModel::from(history)));
    g.set_passage(selected_passage(op).is_some());
}

/// A reference was added from the Bible box (or Prev/Next verse): first in the history at once, then saved.
pub(super) fn remember(op: &Rc<Operator>, reference: &str) {
    {
        let mut st = op.state.borrow_mut();
        if let Some(snap) = st.snap.as_mut() { std::sync::Arc::make_mut(snap).settings.used_reference(reference) }
    }
    render(op);
    let me = Rc::downgrade(op);
    op.ops.call::<Value>("settings/bible-history", json!({ "reference": reference }), move |r| {
        if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(&e, true) }
    });
}

/// A history line: back in the box, and added as if typed.
pub fn pick_history(op: &Rc<Operator>, reference: String) {
    op.ui.set_bible_text(reference.as_str().into());
    bible::submit(op, reference);
}

/// The reference of the verse just before (`forward` false) or after `verses` (KJV versification); `None` at either
/// end of the Bible or when the passage's verses are not known.
pub fn neighbour_reference(verses: &[VerseText], forward: bool) -> Option<String> {
    let ids: Vec<_> = verses.iter().filter_map(crate::scripture::input::verse_id).collect();
    let edge = if forward { ids.iter().max() } else { ids.iter().min() }?;
    crate::bible::kjv().neighbour(*edge, forward).map(crate::scripture::input::label_of)
}

/// ‹ Verse / Verse ›: the verse before or after the selected passage, in its Bible, added after the live item and
/// armed (the next Enter shows it).
pub fn step(op: &Rc<Operator>, forward: bool) {
    let Some((p, _)) = selected_passage(op) else { return op.toast(&ss::pick_a_passage(), true) };
    let Some(reference) = neighbour_reference(&p.verses, forward) else {
        return op.toast(&if forward { ss::no_verse_after() } else { ss::no_verse_before() }, true);
    };
    let me = Rc::downgrade(op);
    op.ops.call::<bible::Found>("bible/lookup", json!({ "reference": reference, "translation": p.translation }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r { Ok(found) => bible::add(&op, found), Err(e) => op.toast(&e, true) }
    });
}

/// The song Save to library makes from a passage's slides: each slide a section named by its label.
pub fn passage_song(title: &str, copyright: &str, slides: &[crate::model::Slide]) -> Song {
    let lyrics = slides.iter().filter_map(|s| match &s.content {
        crate::model::SlideContent::Text { text, parallel, .. } => {
            let words = crate::model::all_bibles_text(text, parallel.as_ref());
            Some(if s.label.trim().is_empty() { words } else { format!("[{}]\n{words}", s.label.trim()) })
        }
        _ => None,
    }).collect::<Vec<_>>().join("\n\n");
    Song { id: Uuid::nil(), title: title.into(), writers: String::new(), ccli: String::new(), copyright: copyright.into(), lyrics, arrangement: vec![], artist: String::new(), publisher: String::new(), year: String::new(), lines_per_slide: 0, text_size: 0, ..Default::default() }
}

/// Save to library: the selected passage as a new song, its slides as sections, with the Bible's credit.
pub fn save_passage(op: &Rc<Operator>) {
    let Some((p, row)) = selected_passage(op) else { return op.toast(&ss::pick_a_passage(), true) };
    let (abbr, credit) = {
        let st = op.state.borrow();
        let c = st.snap.as_ref().and_then(|s| s.data.bible_credit(&p.translation).cloned());
        (c.as_ref().map_or_else(|| p.translation.clone(), |c| c.abbr.clone()), c.map(|c| c.line).unwrap_or_default())
    };
    let title = ss::saved_passage_title(&p.reference, &abbr);
    let song = passage_song(&title, &credit, &row.slides);
    let me = Rc::downgrade(op);
    op.ops.call::<Value>("song/save", serde_json::to_value(&song).unwrap_or_default(), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r { Ok(_) => op.toast(&ss::passage_saved(&title), false), Err(e) => op.toast(&e, true) }
    });
}

/// About this Bible: what `id` is, where it comes from and its licence, from the rows Settings → Bible shows.
pub fn info_text(s: &view::OperatorSnapshot, id: &str) -> String {
    let src = s.bible_sources.iter().find(|b| crate::bible::id::same_id(&b.translation, id));
    let label = view::bible_labels(s).get(id).cloned().unwrap_or_else(|| id.to_string());
    let Some(src) = src else { return format!("{label}\n\n{}", ss::info_unavailable()) };
    let mut lines = vec![if src.name.is_empty() || src.name == label { label } else { format!("{} ({label})", src.name) }];
    if !src.language.is_empty() { lines.push(ss::info_language(&src.language)) }
    if let Some(p) = src.provider.as_deref().filter(|p| !p.is_empty()) { lines.push(ss::info_from(p)) }
    let notice = src.copyright.as_deref().map(str::trim).filter(|c| !c.is_empty());
    if let Some(c) = notice { lines.push(ss::info_copyright(c)) }
    if !src.licence.is_empty() {
        let l = if src.licence_url.is_empty() { src.licence.clone() } else { format!("{} — {}", src.licence, src.licence_url) };
        lines.push(ss::info_licence(&l));
    }
    let public = crate::bible::BUILT_IN.iter().any(|b| b.0 == src.translation);
    if notice.is_none() && src.licence.is_empty() && !public { lines.push(ss::info_no_notice()) }
    match src.status.as_str() {
        "needs_key" => lines.push(ss::info_needs_key()),
        "unavailable" => lines.push(ss::info_unavailable()),
        _ => {}
    }
    lines.join("\n")
}

/// The ⓘ on a Bible (picker row, Edit → About this Bible…).
pub fn info(op: &Rc<Operator>, id: &str) {
    let text = op.state.borrow().snap.as_ref().map(|s| info_text(s, id));
    if let Some(t) = text { crate::ui::dialogs::ask::notice(op, &t) }
}

/// Connects the global's callbacks. They hold a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let g = op.ui.panel_extras_all();
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_history_pick(move |r| if let Some(op) = me.upgrade() { pick_history(&op, r.into()) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_prev_verse(move || if let Some(op) = me.upgrade() { step(&op, false) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_next_verse(move || if let Some(op) = me.upgrade() { step(&op, true) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_save_passage(move || if let Some(op) = me.upgrade() { save_passage(&op) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_bible_info(move |id| if let Some(op) = me.upgrade() { info(&op, &id) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        ui::operator::window::testing::{find, find_all, open, refresh},
    };
    use slint::Model;
    use std::sync::Arc;

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op
    }

    fn items(app: &App) -> Vec<(Uuid, String)> {
        app.data().services[0].items.iter().map(|i| (i.id, match &i.kind { ItemKind::Scripture { reference, .. } => reference.clone(), _ => String::new() })).collect()
    }

    fn history(op: &Operator) -> Vec<String> { op.ui.panel_extras_all().get_history().iter().map(|s| s.to_string()).collect() }

    #[test]
    fn the_box_keeps_the_last_references_and_one_click_adds_one_again() {
        let app = test_app();
        let op = opened(&app);
        assert!(history(&op).is_empty());
        assert!(find(&op, "BibleBox::history-btn").is_none(), "no history, no button");
        bible::submit(&op, "jn 3:16".into());
        bible::submit(&op, "Psalm 23:1-2".into());
        assert_eq!(history(&op), ["Psalm 23:1–2", "John 3:16"], "newest first, as the lookup names it");
        assert_eq!(app.data().settings.bible_history, ["Psalm 23:1–2", "John 3:16"], "and saved");
        refresh(&op);
        assert!(find(&op, "BibleBox::history-btn").is_some());
        pick_history(&op, "John 3:16".into());
        let all = items(&app);
        assert_eq!(all.len(), 3);
        assert_eq!(all[2].1, "John 3:16", "added again at the end");
        assert_eq!(op.state.borrow().armed, Some(all[2].0), "and armed, as from the box");
        assert_eq!(history(&op), ["John 3:16", "Psalm 23:1–2"], "used again: first again, once");
    }

    #[test]
    fn a_passage_shows_every_slide_at_once_numbered() {
        let app = test_app();
        let op = opened(&app);
        bible::submit(&op, "Ruth 4:5-22".into());
        refresh(&op);
        let id = op.state.borrow().selected.expect("the new passage is selected");
        let row = op.state.borrow().snap.as_ref().unwrap().items.iter().find(|i| i.id == id).cloned().unwrap();
        assert!(row.slides.len() > 3, "{}", row.slides.len());
        assert_eq!(op.ui.get_slide_count() as usize, row.slides.len());
        let idx: Vec<i32> = op.ui.get_slide_rows().iter().flat_map(|r| r.cells.iter().map(|c| c.idx).collect::<Vec<_>>()).collect();
        assert_eq!(idx, (0..row.slides.len() as i32).collect::<Vec<_>>(), "one numbered cell per slide");
        assert!(op.ui.panel_extras_all().get_passage());
        assert!(find(&op, "SlidesPanel::next-verse").is_some() && find(&op, "SlidesPanel::save-passage").is_some());
    }

    #[test]
    fn prev_and_next_verse_add_the_neighbour_in_the_passages_bible() {
        let app = test_app();
        let op = opened(&app);
        crate::ui::operator::bible::pick_id(&op, "WEB".into());
        bible::submit(&op, "John 3:16-17".into());
        refresh(&op);
        crate::ui::operator::bible::pick_id(&op, "KJV".into());
        find(&op, "SlidesPanel::next-verse").expect("Verse ›").mock_single_click(slint::platform::PointerEventButton::Left);
        let after = items(&app);
        assert_eq!(after.iter().map(|(_, r)| r.as_str()).collect::<Vec<_>>(), ["John 3:16–17", "John 3:18"]);
        let data = app.data();
        assert!(matches!(&data.services[0].items[1].kind, ItemKind::Scripture { translation, .. } if translation == "WEB"), "the passage's Bible, not the chip's");
        drop(data);
        assert_eq!(op.state.borrow().armed, Some(after[1].0), "armed: Enter shows it");
        // The new one is selected now: the verse before it is John 3:17.
        refresh(&op);
        step(&op, false);
        assert_eq!(items(&app).last().unwrap().1, "John 3:17");
        let vs = |r: &str| vec![VerseText { id: crate::scripture::input::verse_id_of(r), ..VerseText::new(r, "x") }];
        assert_eq!(neighbour_reference(&vs("Genesis 1:1"), false), None);
        assert_eq!(neighbour_reference(&vs("John 3:36"), true).as_deref(), Some("John 4:1"));
    }

    #[test]
    fn save_to_library_makes_an_editable_song_with_the_credit() {
        let app = test_app();
        let op = opened(&app);
        bible::submit(&op, "John 3:16-17".into());
        refresh(&op);
        save_passage(&op);
        let data = app.data();
        let song = data.songs.iter().find(|s| s.title == "John 3:16–17 (KJV)").expect("saved as a song");
        assert!(song.lyrics.contains("For God so loved the world"), "{}", song.lyrics);
        assert!(crate::slides::parse_sections(&song.lyrics).len() >= 1);
        drop(data);
        assert_eq!(op.state.borrow().toasts.last().cloned(), Some(("Saved “John 3:16–17 (KJV)” to the song library.".into(), false)));
    }

    #[test]
    fn about_a_bible_says_where_it_comes_from_and_its_licence() {
        let app = test_app();
        let op = opened(&app);
        let snap = op.state.borrow().snap.clone().unwrap();
        let kjv = info_text(&snap, "KJV");
        assert!(kjv.contains("public domain"), "{kjv}");
        assert!(!kjv.contains(&ss::info_no_notice()), "a built-in Bible needs no warning");
        let niv = info_text(&snap, "NIV");
        assert!(niv.contains(&ss::info_needs_key()) || niv.contains(&ss::info_unavailable()), "{niv}");
        info(&op, "KJV");
        assert_eq!(op.state.borrow().asks, 1, "shown as a notice");
        assert_eq!(crate::ui::operator::menu::action("bible-info"), Some(crate::ui::operator::menu::MenuAction::BibleInfo));
        // Every picker row has its ⓘ.
        op.open_bible_picker();
        slint::platform::update_timers_and_animations();
        assert!(!find_all(&op, "BiblePickerList::info").is_empty());
    }
}
