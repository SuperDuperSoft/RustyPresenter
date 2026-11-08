//! N15: the action palette (`ui/operator/action-palette.slint`) and the badges on the thumbnails. Tiles come from
//! `automation::slide_actions::palette`; a tile carries its steps as JSON, so a drag or a click hands them back to
//! the `automation/slide-actions/*` actions. The selected item's slides are the ones it edits.
use super::window::Operator;
use crate::automation::{macros::MacroStep, slide_actions::{self, GROUPS}};
use crate::ui::{PaletteGroup, PaletteRow, PaletteTile, SlideActChip, SlideActChipRow, SlideActs};
use crate::ui::dialogs::automation::step_label;
use serde_json::{Value, json};
use slint::{ComponentHandle, DataTransfer, ModelRc, SharedString, VecModel};
use std::rc::{Rc, Weak};

/// A dragged tile: plain text `act:` and its steps as JSON.
const DRAG_PREFIX: &str = "act:";
/// Tiles to a row in the palette (`ActionPalette`).
const PER_ROW: usize = 4;
/// Chips to a row for the picked slide's actions.
const CHIPS_PER_ROW: usize = 4;

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> { ModelRc::new(VecModel::from(v)) }

/// The steps a tile's code or a drag payload carries, or `None` for anything else.
pub fn steps_of(code: &str) -> Option<Vec<MacroStep>> {
    serde_json::from_str::<Vec<MacroStep>>(code).ok().filter(|s| !s.is_empty())
}

fn drag_steps(d: &DataTransfer) -> Option<Vec<MacroStep>> {
    d.plain_text().ok().and_then(|t| t.strip_prefix(DRAG_PREFIX).and_then(steps_of))
}

/// The badge on each of the selected item's slides: how many actions, and their names one to a line.
pub(super) fn badges(op: &Operator, slides: usize) -> Vec<(i32, String)> {
    let st = op.state.borrow();
    let (Some(snap), Some(item)) = (st.snap.as_ref(), st.selected) else { return vec![(0, String::new()); slides] };
    let (d, a) = (&snap.data, &snap.data.automation);
    (0..slides).map(|i| {
        let list = slide_actions::of(a, item, i);
        let names: Vec<String> = list.iter().map(|s| step_label(s, a, d)).collect();
        (list.len() as i32, names.join("\n"))
    }).collect()
}

/// The palette's tiles, the slide list and the picked slide's actions. Only while it is open.
pub(super) fn render(op: &Operator) {
    let g = op.ui.global::<SlideActs>();
    if !g.get_open() { return }
    let st = op.state.borrow();
    let Some(snap) = st.snap.as_ref() else { return };
    let item = st.selected.and_then(|id| snap.items.iter().find(|i| i.id == id));
    let (d, a) = (&snap.data, &snap.data.automation);
    let tiles = slide_actions::palette(d, item.map_or(0, |i| i.slides.len()));
    let groups: Vec<PaletteGroup> = GROUPS.iter().filter_map(|grp| {
        let mine: Vec<PaletteTile> = tiles.iter().filter(|t| t.group == *grp).map(|t| PaletteTile {
            label: t.label.as_str().into(),
            code: if t.off.is_some() { SharedString::new() } else { serde_json::to_string(&t.steps).unwrap_or_default().into() },
            off: t.off.clone().unwrap_or_default().into(),
        }).collect();
        if mine.is_empty() { return None }
        let rows = mine.chunks(PER_ROW).map(|r| PaletteRow { tiles: model(r.to_vec()) }).collect();
        Some(PaletteGroup { name: grp.label().into(), rows: model(rows) })
    }).collect();
    g.set_groups(model(groups));
    let options: Vec<SharedString> = item.map(|it| it.slides.iter().enumerate().map(|(i, s)| {
        let n = slide_actions::run_name(i);
        SharedString::from(if s.label.trim().is_empty() { n } else { format!("{n} — {}", s.label.trim()) })
    }).collect()).unwrap_or_default();
    let picked = g.get_slide().clamp(0, (options.len() as i32 - 1).max(0));
    g.set_slide(picked);
    g.set_slide_options(model(options));
    let chips: Vec<SlideActChip> = item.map(|it| slide_actions::of(a, it.id, picked as usize).iter().enumerate()
        .map(|(i, s)| SlideActChip { index: i as i32, label: step_label(s, a, d).into() }).collect()).unwrap_or_default();
    g.set_current(model(chips.chunks(CHIPS_PER_ROW).map(|r| SlideActChipRow { chips: model(r.to_vec()) }).collect()));
}

/// Opens or closes the palette (the Actions button, Window → Action palette).
pub fn toggle(op: &Operator) {
    let g = op.ui.global::<SlideActs>();
    g.set_open(!g.get_open());
    render(op);
}

/// Adds `steps` to slide `slide` of the selected item.
fn add(op: &Rc<Operator>, slide: usize, steps: Vec<MacroStep>) {
    let Some(item) = op.state.borrow().selected else { return };
    send(op, "automation/slide-actions/add", json!({ "item": item, "slide": slide, "steps": steps }));
}

fn send(op: &Rc<Operator>, action: &'static str, body: Value) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>(action, body, move |r| {
        let Some(op) = me.upgrade() else { return };
        if let Err(e) = r { op.toast(&e, true) }
    });
}

/// A thumbnail's badge, or a tile dropped on a slide: the palette shows that slide.
fn pick(op: &Operator, slide: usize) {
    let g = op.ui.global::<SlideActs>();
    g.set_slide(slide as i32);
    if !g.get_open() { g.set_open(true) }
    render(op);
}

pub(super) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<SlideActs>();
    let w = |op: &Rc<Operator>| -> Weak<Operator> { Rc::downgrade(op) };
    let me = w(op);
    g.on_toggle(move || if let Some(op) = me.upgrade() { toggle(&op) });
    let me = w(op);
    g.on_slide_picked(move |_| if let Some(op) = me.upgrade() { render(&op) });
    let me = w(op);
    g.on_add(move |code| if let (Some(op), Some(steps)) = (me.upgrade(), steps_of(&code)) {
        let slide = op.ui.global::<SlideActs>().get_slide().max(0) as usize;
        add(&op, slide, steps);
    });
    let me = w(op);
    g.on_remove(move |i| if let Some(op) = me.upgrade() {
        let (Some(item), slide) = (op.state.borrow().selected, op.ui.global::<SlideActs>().get_slide().max(0)) else { return };
        send(&op, "automation/slide-actions/remove", json!({ "item": item, "slide": slide, "index": i.max(0) }));
    });
    let me = w(op);
    g.on_badge(move |i| if let Some(op) = me.upgrade() { pick(&op, i.max(0) as usize) });
    g.on_drag_data(|code| {
        let mut d = DataTransfer::default();
        if !code.is_empty() { d.set_plain_text(format!("{DRAG_PREFIX}{code}").into()); }
        d
    });
    g.on_can_drop(|d| drag_steps(&d).is_some());
    let me = w(op);
    g.on_dropped(move |i, d| if let (Some(op), Some(steps)) = (me.upgrade(), drag_steps(&d)) {
        let slide = i.max(0) as usize;
        pick(&op, slide);
        add(&op, slide, steps);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::{ItemKind, ServiceItem},
        timers::STOPWATCH_ID,
        ui::operator::window::testing::{find, find_all, open, refresh},
    };
    use slint::{Model, platform::PointerEventButton};
    use std::sync::Arc;
    use uuid::Uuid;

    fn add_text(app: &App, title: &str, body: &str) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: body.into() }, background: None, notes: String::new(), theme: None };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        item.id
    }

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op
    }

    fn tile_code(op: &Operator, label: &str) -> String {
        let g = op.ui.global::<SlideActs>();
        g.get_groups().iter().flat_map(|g| g.rows.iter().collect::<Vec<_>>()).flat_map(|r| r.tiles.iter().collect::<Vec<_>>())
            .find(|t| t.label == label).map(|t| t.code.to_string()).unwrap_or_default()
    }

    #[test]
    fn a_dropped_tile_goes_on_that_slide_and_shows_as_a_badge() {
        let app = test_app();
        let id = add_text(&app, "Welcome", "One\n\nTwo\n\nThree");
        let op = opened(&app);
        find(&op, "SlidesPanel::palette-btn").expect("the Actions button").mock_single_click(PointerEventButton::Left);
        let g = op.ui.global::<SlideActs>();
        assert!(g.get_open());
        assert!(find(&op, "SlidesPanel::action-palette").is_some(), "the palette shows");
        let code = tile_code(&op, "Start “Stopwatch”");
        assert!(!code.is_empty());
        let mut d = DataTransfer::default();
        d.set_plain_text(format!("{DRAG_PREFIX}{code}").into());
        assert!(g.invoke_can_drop(d.clone()));
        assert!(!g.invoke_can_drop(DataTransfer::default()), "a file or other text is not a tile");
        g.invoke_dropped(2, d);
        refresh(&op);
        assert_eq!(slide_actions::of(&app.data.lock().unwrap().automation, id, 2).len(), 2, "both steps of the tile");
        assert_eq!(g.get_slide(), 2, "the palette shows the slide it went on");
        assert_eq!(g.get_current().row_count(), 1);
        assert_eq!(badges(&op, 3)[2].0, 2);
        // The thumbnail's cell carries the count its badge shows. (The testing backend doesn't reach the badge
        // element itself by id, so this checks the model the badge is drawn from.)
        let acts: Vec<i32> = op.ui.get_slide_rows().iter().flat_map(|r| r.cells.iter().map(|c| c.actions).collect::<Vec<_>>()).collect();
        assert_eq!(acts, [0, 0, 2], "the thumbnail has a badge");
        assert_eq!(app.live_view().frame.overlays.len(), 0, "adding runs nothing");
        let _ = STOPWATCH_ID;
    }

    #[test]
    fn a_click_adds_to_the_picked_slide_and_the_x_takes_it_off() {
        let app = test_app();
        let id = add_text(&app, "Welcome", "One\n\nTwo");
        let op = opened(&app);
        toggle(&op);
        let g = op.ui.global::<SlideActs>();
        assert_eq!(g.get_slide_options().row_count(), 2);
        g.set_slide(1);
        g.invoke_add(tile_code(&op, "Clear audio").into());
        refresh(&op);
        assert_eq!(slide_actions::of(&app.data.lock().unwrap().automation, id, 1).len(), 1);
        g.invoke_remove(0);
        refresh(&op);
        assert!(app.data.lock().unwrap().automation.slide_actions.is_empty());
        // The badge opens the palette on its slide.
        toggle(&op);
        assert!(!g.get_open());
        g.invoke_badge(0);
        assert!(g.get_open() && g.get_slide() == 0);
        let _ = find_all(&op, "SlidesPanel::thumb");
    }

    #[test]
    fn the_menu_opens_it_and_runs_pre_service() {
        let app = test_app();
        add_text(&app, "Welcome", "One");
        let op = opened(&app);
        op.ui.invoke_menu("action-palette".into());
        assert!(op.ui.global::<SlideActs>().get_open());
        op.ui.invoke_menu("start-preservice".into());
        // No start time set: the toast says what to do, nothing goes up.
        assert!(op.state.borrow().toasts.last().is_some());
    }
}
