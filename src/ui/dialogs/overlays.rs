//! The Overlays dialog (FEAT-1 spec §3.6 steps 3–5; `ui/dialogs/overlays.slint`): the saved overlays in drawing order
//! with Edit, Duplicate, Delete and Up/Down, and the overlay editor. Saving runs `overlay/save` at once; an overlay
//! that is on the screen keeps showing its old copy until it is turned off and on (its chip shows a dot). The
//! preview draws the overlay over a sample lyric on the church default, as the room would.
use super::{DialogKind, background, confirm, settings_look, strings as dlg};
use crate::{
    app::{ImportPurpose, Imported},
    model::{AssetId, Data, Settings, Slide, SlideContent},
    render::Fonts,
    store::Store,
    theme::{self, overlay::{Anchor, BoxKind, ImageSource, Overlay, OverlayKind, OverlayLook, OverlaySource, Placement, slots}, strings as ts},
    ui::{OverlayListRow, OverlaysDlg, operator::{file_input::Filter, files, window::Operator}, thumbs::Pixels},
};
use slint::{ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::{Cell, RefCell}, path::PathBuf, rc::{Rc, Weak}, sync::Mutex};
use tokio::sync::oneshot;
use uuid::Uuid;

/// The nine places in the order of the editor's grid.
pub const ANCHORS: [Anchor; 9] = [Anchor::TopLeft, Anchor::Top, Anchor::TopRight, Anchor::Left, Anchor::Center, Anchor::Right, Anchor::BottomLeft, Anchor::Bottom, Anchor::BottomRight];

/// The open dialog: the ids of the rows, and the overlay being edited.
#[derive(Default)]
struct State {
    ids: Vec<Uuid>,
    editing: Option<Overlay>,
    timer: Timer,
    generation: Cell<u64>,
}

thread_local! {
    static OPEN: RefCell<Option<Rc<RefCell<State>>>> = const { RefCell::new(None) };
}

fn state() -> Option<Rc<RefCell<State>>> { OPEN.with(|o| o.borrow().clone()) }

fn g(op: &Operator) -> OverlaysDlg<'_> { op.ui.global::<OverlaysDlg>() }

fn snap_data(op: &Operator) -> Option<std::sync::Arc<Data>> { op.state.borrow().snap.as_ref().map(|s| s.data.clone()) }

pub fn kind_label(k: OverlayKind) -> String {
    match k {
        OverlayKind::Text => ts::kind_text(),
        OverlayKind::Picture => ts::kind_picture(),
        OverlayKind::Clock => ts::kind_clock(),
        OverlayKind::Other => String::new(),
    }
}

/// The line under an overlay's name: what it is and where it sits.
fn row_sub(o: &Overlay) -> String {
    match &o.source {
        OverlaySource::Text { text, .. } => format!("{} · {}", kind_label(OverlayKind::Text), text.lines().next().unwrap_or_default()),
        s => kind_label(s.kind()),
    }
}

/// Opens the dialog on the list (the strip's + and the Settings menu).
pub fn open(op: &Rc<Operator>) {
    if op.state.borrow().snap.is_none() { return }
    op.open_dialog(DialogKind::Overlays);
    let st = Rc::new(RefCell::new(State::default()));
    OPEN.with(|o| *o.borrow_mut() = Some(st));
    let d = g(op);
    d.set_editing(false);
    d.set_palette(ModelRc::new(VecModel::from(background::palette())));
    d.set_help(ts::text_overlay_help().into());
    fill(op);
    wire(op);
    op.on_dialog_close(|op| {
        if let Some(st) = OPEN.with(|o| o.borrow_mut().take()) { st.borrow().timer.stop() }
        g(op).set_preview(Image::default());
    });
}

fn fill(op: &Operator) {
    let (Some(st), Some(data)) = (state(), snap_data(op)) else { return };
    st.borrow_mut().ids = data.overlays.iter().map(|o| o.id).collect();
    let rows: Vec<OverlayListRow> = data.overlays.iter().map(|o| OverlayListRow { name: o.name.as_str().into(), sub: row_sub(o).into() }).collect();
    g(op).set_rows(ModelRc::new(VecModel::from(rows)));
}

fn refill_later(op: &Operator) { op.on_next_read(|op| fill(op)) }

fn wire(op: &Rc<Operator>) {
    let g = g(op);
    let with = |f: fn(&Rc<Operator>)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move || if let Some(op) = me.upgrade() { f(&op) }
    };
    let with_i = |f: fn(&Rc<Operator>, usize)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move |i: i32| if let Some(op) = me.upgrade() { f(&op, i.max(0) as usize) }
    };
    let with_color = |f: fn(&Rc<Operator>, String)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move |c: slint::Color| if let Some(op) = me.upgrade() { f(&op, background::to_hex(c)) }
    };
    let with_hex = |f: fn(&Rc<Operator>, String)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move |t: slint::SharedString| if let (Some(op), Some(hex)) = (me.upgrade(), background::parse_hex(&t)) { f(&op, hex) }
    };
    g.on_new_overlay(with_i(new_overlay));
    g.on_edit(with_i(edit));
    g.on_duplicate(with_i(duplicate));
    g.on_delete(with_i(delete));
    let me = Rc::downgrade(op);
    g.on_move(move |i, step| if let Some(op) = me.upgrade() { move_row(&op, i.max(0) as usize, step) });
    g.on_changed(with(changed));
    g.on_text_color(with_color(|op, c| set_color(op, false, c)));
    g.on_text_hex(with_hex(|op, c| set_color(op, false, c)));
    g.on_box_color(with_color(|op, c| set_color(op, true, c)));
    g.on_box_hex(with_hex(|op, c| set_color(op, true, c)));
    g.on_choose_picture(with(choose_picture));
    g.on_back(with(back));
    g.on_save(with(save));
}

fn overlay_at(op: &Operator, i: usize) -> Option<Overlay> {
    let id = *state()?.borrow().ids.get(i)?;
    snap_data(op)?.overlays.iter().find(|o| o.id == id).cloned()
}

/// A name no saved overlay has.
fn free_name(data: &Data, base: &str) -> String {
    let taken = |n: &str| data.overlays.iter().any(|o| o.name.eq_ignore_ascii_case(n));
    if !taken(base) { return base.to_string() }
    theme::copy_name(base, taken)
}

/// A new overlay of the kind the button names (0 Text, 1 Picture, 2 Clock), with the spec's defaults.
pub fn new_of(kind: usize, name: String) -> Overlay {
    let (source, k) = match kind {
        1 => (OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 }, OverlayKind::Picture),
        2 => (OverlaySource::Clock { look: OverlayLook::default(), seconds: false, h24: false }, OverlayKind::Clock),
        _ => (OverlaySource::Text { text: ts::overlay_text_sample(), styles: vec![], look: OverlayLook::default() }, OverlayKind::Text),
    };
    Overlay { id: Uuid::nil(), name, source, place: Placement::default_for(k) }
}

fn new_overlay(op: &Rc<Operator>, kind: usize) {
    let Some(data) = snap_data(op) else { return };
    open_editor(op, new_of(kind, free_name(&data, &ts::new_overlay_name())));
}

fn edit(op: &Rc<Operator>, i: usize) { if let Some(o) = overlay_at(op, i) { open_editor(op, o) } }

fn duplicate(op: &Rc<Operator>, i: usize) {
    let (Some(o), Some(data)) = (overlay_at(op, i), snap_data(op)) else { return };
    let name = theme::copy_name(&o.name, |n| data.overlays.iter().any(|x| x.name.eq_ignore_ascii_case(n)));
    open_editor(op, Overlay { id: Uuid::nil(), name, ..o });
}

fn delete(op: &Rc<Operator>, i: usize) {
    let Some(o) = overlay_at(op, i) else { return };
    let me = Rc::downgrade(op);
    confirm(op, &ts::delete_overlay_question(&o.name), &ts::delete(), &dlg::cancel(), true, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        call(&op, "overlay/delete", serde_json::json!({ "id": o.id }), |op| op.toast(ts::overlay_deleted(), false));
    });
}

fn move_row(op: &Rc<Operator>, i: usize, step: i32) {
    let Some(o) = overlay_at(op, i) else { return };
    let Some(to) = i.checked_add_signed(step as isize) else { return };
    call(op, "overlay/move", serde_json::json!({ "id": o.id, "to": to }), |_| {});
}

/// Runs an action; on success the list is drawn again and `done` runs, else an error toast.
fn call(op: &Rc<Operator>, action: &'static str, body: serde_json::Value, done: impl FnOnce(&Rc<Operator>) + 'static) {
    let me = Rc::downgrade(op);
    op.ops().call::<serde_json::Value>(action, body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                refill_later(&op);
                done(&op);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

fn look_of(o: &Overlay) -> Option<&OverlayLook> {
    match &o.source {
        OverlaySource::Text { look, .. } | OverlaySource::Clock { look, .. } => Some(look),
        _ => None,
    }
}

fn open_editor(op: &Rc<Operator>, o: Overlay) {
    let Some(st) = state() else { return };
    let g = g(op);
    g.set_name(o.name.as_str().into());
    g.set_kind(match o.source.kind() { OverlayKind::Picture => 1, OverlayKind::Clock => 2, _ => 0 });
    g.set_text("".into());
    g.set_picture_name("".into());
    match &o.source {
        OverlaySource::Text { text, .. } => g.set_text(text.as_str().into()),
        OverlaySource::Image { source, opacity } => {
            g.set_picture_source(i32::from(matches!(source, ImageSource::Asset { .. })));
            g.set_opacity(i32::from(*opacity));
            if let ImageSource::Asset { asset } = source { g.set_picture_name(picture_name(op, asset).into()) }
        }
        OverlaySource::Clock { seconds, h24, .. } => {
            g.set_seconds(*seconds);
            g.set_h24(*h24);
        }
        // FEAT-2 timers and messages are shown from Automation, never saved here.
        OverlaySource::Timer { .. } | OverlaySource::Message { .. } | OverlaySource::Unknown => {}
    }
    let look = look_of(&o).cloned().unwrap_or_default();
    g.set_size(look.size as i32);
    g.set_shadow(look.shadow);
    g.set_box(match look.box_kind { BoxKind::None => 0, BoxKind::Behind => 1, BoxKind::Bar => 2 });
    g.set_box_clear(i32::from(100 - look.box_opacity.min(100)).min(90));
    g.set_anchor(ANCHORS.iter().position(|a| *a == o.place.anchor).unwrap_or(8) as i32);
    g.set_width(i32::from(o.place.width));
    st.borrow_mut().editing = Some(o);
    show_swatches(op);
    g.set_editing(true);
    render_now(op);
}

fn picture_name(op: &Operator, asset: &AssetId) -> String {
    snap_data(op).and_then(|d| d.media.iter().find(|m| m.asset == *asset).map(|m| m.name.clone())).unwrap_or_else(|| asset.clone())
}

fn show_swatches(op: &Operator) {
    let Some(o) = state().and_then(|s| s.borrow().editing.clone()) else { return };
    let look = look_of(&o).cloned().unwrap_or_default();
    let g = g(op);
    g.set_color_swatch(settings_look::swatch(&look.color));
    g.set_box_swatch(settings_look::swatch(&look.box_color));
    let has_logo = snap_data(op).is_some_and(|d| d.settings.logo.is_some());
    g.set_logo_note(if has_logo { String::new() } else { ts::logo_first() }.into());
}

/// The edited overlay with what the form says now.
fn read_editor(op: &Operator) -> Option<Overlay> {
    let g = g(op);
    let mut o = state()?.borrow().editing.clone()?;
    o.name = g.get_name().to_string();
    let mut look = look_of(&o).cloned().unwrap_or_default();
    look.size = g.get_size().clamp(20, 120) as u32;
    look.shadow = g.get_shadow();
    look.box_kind = [BoxKind::None, BoxKind::Behind, BoxKind::Bar][g.get_box().clamp(0, 2) as usize];
    look.box_opacity = (100 - g.get_box_clear().clamp(0, 90)) as u8;
    o.source = match o.source {
        OverlaySource::Text { styles, .. } => OverlaySource::Text { text: g.get_text().to_string(), styles, look },
        OverlaySource::Image { source, .. } => {
            let source = if g.get_picture_source() == 0 { ImageSource::ChurchLogo } else { source };
            OverlaySource::Image { source, opacity: g.get_opacity().clamp(10, 100) as u8 }
        }
        OverlaySource::Clock { .. } => OverlaySource::Clock { look, seconds: g.get_seconds(), h24: g.get_h24() },
        other @ (OverlaySource::Timer { .. } | OverlaySource::Message { .. } | OverlaySource::Unknown) => other,
    };
    o.place.anchor = ANCHORS[g.get_anchor().clamp(0, 8) as usize];
    o.place.width = g.get_width().clamp(5, 100) as u8;
    Some(o)
}

fn changed(op: &Rc<Operator>) {
    let Some(st) = state() else { return };
    if let Some(o) = read_editor(op) { st.borrow_mut().editing = Some(o) }
    let me = Rc::downgrade(op);
    st.borrow().timer.start(TimerMode::SingleShot, settings_look::PREVIEW_DELAY, move || if let Some(op) = me.upgrade() { render_now(&op) });
}

fn set_color(op: &Rc<Operator>, boxed: bool, hex: String) {
    let Some(st) = state() else { return };
    if let Some(o) = st.borrow_mut().editing.as_mut() {
        match &mut o.source {
            OverlaySource::Text { look, .. } | OverlaySource::Clock { look, .. } => {
                if boxed { look.box_color = hex } else { look.color = hex }
            }
            _ => {}
        }
    }
    show_swatches(op);
    changed(op);
}

/// "Choose picture…": a picture of its own, imported as a bare file.
fn choose_picture(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    files::pick(op, Filter::Pictures, false, move |paths| {
        let (Some(op), Some(path)) = (me.upgrade(), paths.into_iter().next()) else { return };
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let me = Rc::downgrade(&op);
        op.ops().import(path, ImportPurpose::Asset, |_| {}, move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(Imported::Asset(asset)) => {
                    if let Some(st) = state() && let Some(o) = st.borrow_mut().editing.as_mut()
                        && let OverlaySource::Image { source, .. } = &mut o.source {
                        *source = ImageSource::Asset { asset };
                    }
                    g(&op).set_picture_name(name.into());
                    changed(&op);
                }
                Ok(_) => {}
                Err(e) => op.toast(e, true),
            }
        });
    });
}

/// Back to the list (editor), or close (list).
fn back(op: &Rc<Operator>) {
    let editing = g(op).get_editing();
    if !editing { return op.close_dialog() }
    if let Some(st) = state() { st.borrow_mut().editing = None; st.borrow().timer.stop() }
    g(op).set_editing(false);
    g(op).set_preview(Image::default());
}

fn save(op: &Rc<Operator>) {
    let Some(o) = read_editor(op) else { return };
    let me = Rc::downgrade(op);
    op.ops().call::<serde_json::Value>("overlay/save", serde_json::to_value(&o).unwrap_or_default(), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => {
                back(&op);
                refill_later(&op);
                op.toast(ts::overlay_saved(), false);
            }
            Err(e) => op.toast(e, true),
        }
    });
}

/// The overlay preview's own fonts (loading the system's takes a moment, so once).
static FONTS: Mutex<Option<Fonts>> = Mutex::new(None);

/// A sample lyric on the church default's background, with `o` drawn over it as the room would.
fn draw(settings: &Settings, data: &Data, o: &Overlay, root: PathBuf) -> Pixels {
    let mut s = settings.clone();
    let church = theme::church_default(data);
    s.theme = church.style.clone();
    if let Some(b) = church.background { s.background = b }
    let store = Store { root };
    let slide = Slide::new(String::new(), SlideContent::Text {
        text: super::settings::strings::sample_text(), footer: None, reference: None, size: 0, parallel: None,
    });
    let (mut px, _) = settings_look::draw_sample(&s, &slide, "song", &data.media, &store);
    let lib = [Overlay { id: Uuid::nil(), ..o.clone() }];
    let Ok(slot) = slots::saved_slot(Uuid::nil(), &lib, data.settings.logo.as_ref(), 0) else { return px };
    let (w, h) = (px.width(), px.height());
    let mut fonts = FONTS.lock().unwrap_or_else(|e| e.into_inner());
    let fonts = fonts.get_or_insert_with(Fonts::new);
    let (over, _) = crate::ui::room::prepare::overlays(fonts, &[slot], &church.style.font, (w, h), |a| store.asset_path(a));
    let crate::ui::room::prepare::Px::Premul(over) = over else { return px };
    for (d, s) in px.make_mut_bytes().chunks_exact_mut(4).zip(over.as_bytes().chunks_exact(4)) {
        let keep = 255 - u16::from(s[3]);
        for c in 0..3 { d[c] = (u16::from(s[c]) + (u16::from(d[c]) * keep + 127) / 255).min(255) as u8 }
    }
    px
}

/// Draws the preview now: on a worker thread in the app, at once in tests.
fn render_now(op: &Rc<Operator>) {
    let Some(st) = state() else { return };
    st.borrow().timer.stop();
    let Some(o) = st.borrow().editing.clone() else { return };
    let generation = st.borrow().generation.get() + 1;
    st.borrow().generation.set(generation);
    let data = op.app().data.lock().unwrap().clone();
    let settings = data.settings.clone();
    let root = op.app().store.root.clone();
    let me = Rc::downgrade(op);
    let apply = move |px: Pixels| {
        let (Some(op), Some(st)) = (me.upgrade(), state()) else { return };
        if st.borrow().generation.get() != generation { return }
        g(&op).set_preview(Image::from_rgba8(px));
    };
    if op.ops().is_inline() { return apply(draw(&settings, &data, &o, root)) }
    let (tx, rx) = oneshot::channel::<Pixels>();
    let started = std::thread::Builder::new().name("overlay-preview".into()).spawn(move || { let _ = tx.send(draw(&settings, &data, &o, root)); });
    if started.is_ok() {
        let _ = slint::spawn_local(async move { if let Ok(px) = rx.await { apply(px) } });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        ui::operator::window::testing::{find, find_all, open as open_window, refresh},
    };
    use i_slint_backend_testing::mock_elapsed_time;
    use slint::{Model, platform::PointerEventButton};
    use std::sync::Arc;

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open_window(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 1100.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op
    }

    fn settle() { slint::platform::update_timers_and_animations(); }

    fn click(op: &Operator, id: &str) {
        settle();
        find(op, id).unwrap_or_else(|| panic!("{id} not found")).mock_single_click(PointerEventButton::Left);
        settle();
    }

    fn toasts(op: &Operator) -> Vec<String> {
        find_all(op, "ToastStack::toast").iter().map(|t| t.accessible_label().unwrap_or_default().to_string()).collect()
    }

    fn names(op: &Operator) -> Vec<String> { g(op).get_rows().iter().map(|r| r.name.to_string()).collect() }

    fn pixels(img: &Image) -> Vec<u8> { img.to_rgba8().map(|p| p.as_bytes().to_vec()).unwrap_or_default() }

    #[test]
    fn strip_plus_opens_the_dialog_and_a_text_overlay_saves() {
        let app = test_app();
        let op = opened(&app);
        settle();
        click(&op, "OverlaysStrip::overlays-first-btn");
        assert_eq!(op.dialog(), Some(DialogKind::Overlays));
        assert_eq!(op.ui.get_dialog_title(), "Overlays");
        assert!(find(&op, "OverlayList::no-overlays").is_some());
        click(&op, "OverlayList::new-text");
        assert!(g(&op).get_editing());
        assert_eq!((g(&op).get_name().as_str(), g(&op).get_text().as_str()), ("New overlay", "Welcome!"));
        assert_eq!(g(&op).get_anchor(), 7, "text goes bottom-centre");
        assert!(find(&op, "OverlayEditor::overlay-text").is_some(), "the text box");
        assert_eq!(g(&op).get_help(), "For a sign that stays up.");
        g(&op).set_name("Wi-Fi".into());
        g(&op).set_text("Wi-Fi: GraceGuest".into());
        g(&op).set_anchor(1);
        g(&op).set_box(2);
        g(&op).invoke_changed();
        click(&op, "OverlaysDialog::save");
        let o = app.data.lock().unwrap().overlays[0].clone();
        assert_eq!(o.name, "Wi-Fi");
        assert_eq!(o.place.anchor, Anchor::Top);
        assert!(matches!(&o.source, OverlaySource::Text { text, look, .. } if text == "Wi-Fi: GraceGuest" && look.box_kind == BoxKind::Bar));
        assert!(toasts(&op).contains(&"Overlay saved".to_string()));
        assert!(!g(&op).get_editing());
        assert_eq!(names(&op), ["Wi-Fi"]);
        // The strip has its chip.
        refresh(&op);
        assert_eq!(op.ui.overlay_strip_all().get_chips().row_count(), 1);
    }

    #[test]
    fn a_blank_text_is_refused() {
        let app = test_app();
        let op = opened(&app);
        open(&op);
        click(&op, "OverlayList::new-text");
        g(&op).set_text("  ".into());
        click(&op, "OverlaysDialog::save");
        assert!(toasts(&op).contains(&"Type the overlay's text.".to_string()));
        assert!(g(&op).get_editing(), "the editor stays");
    }

    #[test]
    fn clock_and_picture_overlays() {
        let app = test_app();
        let op = opened(&app);
        open(&op);
        click(&op, "OverlayList::new-clock");
        assert_eq!(g(&op).get_kind(), 2);
        assert_eq!(g(&op).get_anchor(), 8, "clocks go bottom-right");
        g(&op).set_h24(true);
        g(&op).invoke_changed();
        click(&op, "OverlaysDialog::save");
        assert!(matches!(app.data.lock().unwrap().overlays[0].source, OverlaySource::Clock { h24: true, .. }));
        click(&op, "OverlayList::new-picture");
        assert_eq!(g(&op).get_logo_note(), "Choose a church logo first (Settings → Look → Text and logo).");
        assert_eq!(g(&op).get_name(), "New overlay copy", "a free name");
    }

    #[test]
    fn preview_draws_the_overlay_over_a_lyric() {
        let app = test_app();
        let op = opened(&app);
        open(&op);
        click(&op, "OverlayList::new-text");
        let first = pixels(&g(&op).get_preview());
        assert!(!first.is_empty());
        g(&op).set_box(2);
        g(&op).invoke_changed();
        mock_elapsed_time(std::time::Duration::from_millis(400));
        settle();
        assert_ne!(pixels(&g(&op).get_preview()), first, "the bar shows");
    }

    #[test]
    fn reorder_duplicate_and_delete() {
        let app = test_app();
        {
            let mut d = app.data.lock().unwrap();
            for n in ["A", "B"] {
                let mut o = new_of(0, n.into());
                o.id = Uuid::new_v4();
                d.overlays.push(o);
            }
        }
        let op = opened(&app);
        open(&op);
        assert_eq!(names(&op), ["A", "B"]);
        g(&op).invoke_move(1, -1);
        assert_eq!(names(&op), ["B", "A"]);
        g(&op).invoke_duplicate(0);
        assert_eq!(g(&op).get_name(), "B copy");
        click(&op, "OverlaysDialog::save");
        assert_eq!(names(&op), ["B", "A", "B copy"]);
        g(&op).invoke_delete(0);
        settle();
        click(&op, "AskOverlay::ok");
        assert_eq!(names(&op), ["A", "B copy"]);
        assert!(toasts(&op).contains(&"Overlay deleted".to_string()));
        click(&op, "OverlaysDialog::close");
        assert_eq!(op.dialog(), None);
    }
}
