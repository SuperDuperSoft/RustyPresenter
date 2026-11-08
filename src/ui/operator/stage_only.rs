//! N29: a slide on the stage screen only, so the band sees a note the room doesn't. "Stage only" in the slides
//! column (or Window → Next slide click: stage only) arms the next slide click to go to the stage instead of the
//! room; a bar says what is up there, with Take it down. The phone does the same from its slide list. The App keeps
//! the slide (`App::set_stage_only`), like the stage message.
use super::{strings as tr, window::Operator};
use crate::live::Command;
use crate::ui::PanelExtras;
use slint::ComponentHandle;
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// What the bar names: the stage-only slide's label, else "slide 3".
pub(super) fn render(op: &Operator) {
    let label = op.state.borrow().live.as_ref().and_then(|l| l.view().stage_only.clone())
        .map(|o| if o.slide.label.trim().is_empty() { tr::stage_only_slide(o.index + 1) } else { o.slide.label.clone() })
        .unwrap_or_default();
    op.ui.global::<PanelExtras>().set_stage_only(label.into());
}

/// A slide click while armed: it goes to the stage only, and the arm is used up. False when not armed.
pub(super) fn take_click(op: &Rc<Operator>, item_id: Uuid, slide: usize) -> bool {
    let g = op.ui.global::<PanelExtras>();
    if !g.get_stage_mode() { return false }
    g.set_stage_mode(false);
    send(op, Some(item_id), slide);
    true
}

fn send(op: &Rc<Operator>, item_id: Option<Uuid>, slide: usize) {
    let me = Rc::downgrade(op);
    op.ops.cmd(Command::StageOnly { item_id, slide }, None, move |o| {
        let Some(op) = me.upgrade() else { return };
        match (o.ok, o.error) {
            (true, _) if item_id.is_some() => op.toast(&tr::on_stage_only(), false),
            (false, Some(e)) => op.toast(&e, true),
            _ => {}
        }
    });
}

/// Arms (or disarms) the next slide click for the stage only.
pub fn toggle(op: &Operator) {
    let g = op.ui.global::<PanelExtras>();
    g.set_stage_mode(!g.get_stage_mode());
}

/// Take it down: the stage shows the room's slide again.
pub fn take_down(op: &Rc<Operator>) { send(op, None, 0) }

pub(super) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<PanelExtras>();
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_toggle_stage_mode(move || if let Some(op) = me.upgrade() { toggle(&op) });
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_stage_down(move || if let Some(op) = me.upgrade() { take_down(&op) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, LiveUpdate, test_app},
        model::{ItemKind, ServiceItem},
        ui::operator::window::testing::{find, find_all, open, refresh},
    };
    use slint::platform::PointerEventButton;
    use std::sync::Arc;

    fn add_text(app: &App, title: &str, body: &str) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: body.into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
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

    fn live(op: &Operator) { op.apply_live(LiveUpdate::new(op.app().live_view())); }

    #[test]
    fn stage_only_sends_the_next_click_to_the_stage_and_the_room_stays() {
        let app = test_app();
        let id = add_text(&app, "Band", "Watch me\n\nTwo more times\n\nEnd");
        let op = opened(&app);
        let g = op.ui.global::<PanelExtras>();
        find(&op, "SlidesPanel::stage-mode").expect("the Stage only button").mock_single_click(PointerEventButton::Left);
        assert!(g.get_stage_mode());
        assert!(find(&op, "SlidesPanel::stage-bar").is_some(), "the bar says what the next click does");
        find_all(&op, "SlidesPanel::thumb").swap_remove(1).mock_single_click(PointerEventButton::Left);
        live(&op);
        let v = app.live_view();
        assert_eq!(v.stage_only.as_ref().map(|o| (o.item_id, o.index)), Some((id, 1)));
        assert!(v.cursor.is_none() && v.frame.slide.is_none(), "the room did not change");
        assert!(!g.get_stage_mode(), "one click only");
        assert_eq!(g.get_stage_only(), "Band 2");
        assert_eq!(op.state.borrow().toasts.last().map(|t| t.0.clone()), Some(tr::on_stage_only()));
        // The next click goes to the room again.
        find_all(&op, "SlidesPanel::thumb").swap_remove(0).mock_single_click(PointerEventButton::Left);
        assert_eq!(app.live_view().cursor.map(|c| c.slide), Some(0));
        assert!(app.live_view().stage_only.is_some(), "the stage note stays up meanwhile");
        live(&op);
        find(&op, "SlidesPanel::stage-down").expect("Take it down").mock_single_click(PointerEventButton::Left);
        live(&op);
        assert!(app.live_view().stage_only.is_none());
        assert_eq!(g.get_stage_only(), "");
        assert!(find(&op, "SlidesPanel::stage-bar").is_none());
    }

    #[test]
    fn the_menu_arms_it_too() {
        let app = test_app();
        add_text(&app, "Band", "Watch me");
        let op = opened(&app);
        op.ui.global::<PanelExtras>().invoke_toggle_stage_mode();
        assert!(op.ui.global::<PanelExtras>().get_stage_mode());
        toggle(&op);
        assert!(!op.ui.global::<PanelExtras>().get_stage_mode(), "again: cancelled");
    }
}
