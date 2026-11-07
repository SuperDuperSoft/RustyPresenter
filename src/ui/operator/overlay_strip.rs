//! The Overlays strip in the Live column (FEAT-1 spec §3.6; `ui/operator/overlay-strip.slint`): a chip per saved
//! overlay, lit while it is on the room screen. A click turns it on or off with `Command::Overlay`, sending the newest
//! `clear_epoch` this window has seen, so a chip clicked just after a clear from the phone is refused with "The
//! screen was cleared — try again." F6 and the strip's Clear button take every overlay down.
use super::window::Operator;
use crate::{
    live::Command,
    theme::overlay::{OverlayKind, slots::{self, Chip}},
    ui::{OverlayChipRow, OverlayStrip},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::{Rc, Weak};

pub fn kind_name(k: OverlayKind) -> &'static str {
    match k {
        OverlayKind::Text => "text",
        OverlayKind::Picture => "picture",
        OverlayKind::Clock => "clock",
        OverlayKind::Other => "other",
    }
}

impl Operator {
    /// The chips as the newest data and live view have them.
    pub(crate) fn overlay_chips(&self) -> Vec<Chip> {
        let st = self.state.borrow();
        let Some(snap) = st.snap.as_ref() else { return vec![] };
        let up = st.live.as_ref().map(|l| l.view().frame.overlays.clone()).unwrap_or_default();
        slots::chips(&snap.data.overlays, &up, snap.data.settings.logo.as_ref())
    }

    pub(super) fn render_overlay_strip(&self) {
        let rows: Vec<OverlayChipRow> = self.overlay_chips().into_iter().map(|c| OverlayChipRow {
            name: c.name.as_str().into(), kind: kind_name(c.kind).into(), on: c.on, changed: c.changed, deleted: c.deleted,
        }).collect();
        self.ui.global::<OverlayStrip>().set_chips(ModelRc::new(VecModel::from(rows)));
    }

    /// A chip's click: on when it is off, off when it is on.
    pub(crate) fn toggle_overlay(self: &Rc<Self>, i: usize) {
        let Some(chip) = self.overlay_chips().into_iter().nth(i) else { return };
        let seen_clear = self.state.borrow().live.as_ref().map(|l| l.view().clear_epoch);
        self.command(Command::Overlay { id: chip.id, on: !chip.on, seen_clear: (!chip.on).then_some(seen_clear.unwrap_or(0)) }, None);
    }
}

/// Connects the strip's callbacks. Each holds a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<OverlayStrip>();
    let me: Weak<Operator> = Rc::downgrade(op);
    g.on_toggle(move |i| if let Some(op) = me.upgrade() {
        op.ui.invoke_focus_keys();
        op.toggle_overlay(i.max(0) as usize);
    });
    let me = Rc::downgrade(op);
    g.on_clear(move || if let Some(op) = me.upgrade() {
        op.ui.invoke_focus_keys();
        op.command(Command::ClearOverlays, None);
    });
    let me = Rc::downgrade(op);
    g.on_manage(move || if let Some(op) = me.upgrade() { crate::ui::dialogs::overlays::open(&op) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, LiveUpdate, test_app},
        theme::overlay::{Overlay, OverlayLook, OverlaySource, Placement},
        ui::operator::window::testing::{find, find_all, open, refresh},
    };
    use slint::{Model, platform::{Key, PointerEventButton, WindowEvent}};
    use std::sync::Arc;
    use uuid::Uuid;

    fn sign(app: &App, name: &str) -> Uuid {
        let o = Overlay { id: Uuid::new_v4(), name: name.into(), source: OverlaySource::Text { text: name.into(), styles: vec![], look: OverlayLook::default() },
            place: Placement::default_for(OverlayKind::Text) };
        app.data.lock().unwrap().overlays.push(o.clone());
        o.id
    }

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1366.0, 768.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        live(&op);
        op
    }

    fn live(op: &Operator) { op.apply_live(LiveUpdate::new(op.app().live_view())); }

    fn chips(op: &Operator) -> Vec<OverlayChipRow> { op.ui.global::<OverlayStrip>().get_chips().iter().collect() }

    fn up(app: &App) -> Vec<String> { app.live_view().frame.overlays.iter().map(|s| s.name.clone()).collect() }

    fn settle() { slint::platform::update_timers_and_animations(); }

    #[test]
    fn overlays_strip_collapses_when_empty() {
        let app = test_app();
        let op = opened(&app);
        settle();
        assert!(chips(&op).is_empty());
        assert!(find(&op, "OverlaysStrip::overlays-first-btn").is_some(), "just “+ Overlays”");
        assert!(find(&op, "OverlaysStrip::overlays-clear-btn").is_none());
    }

    #[test]
    fn overlay_chip_toggles_and_lights() {
        let app = test_app();
        sign(&app, "Welcome");
        sign(&app, "Wi-Fi: GraceGuest");
        let op = opened(&app);
        settle();
        assert_eq!(chips(&op).iter().map(|c| c.name.to_string()).collect::<Vec<_>>(), ["Welcome", "Wi-Fi: GraceGuest"]);
        let chip = || find_all(&op, "OverlaysStrip::overlay-chip");
        assert_eq!(chip().len(), 2);
        chip()[1].mock_single_click(PointerEventButton::Left);
        live(&op);
        settle();
        assert_eq!(up(&app), ["Wi-Fi: GraceGuest"]);
        assert!(chips(&op)[1].on && !chips(&op)[0].on, "the chip lights");
        chip()[1].mock_single_click(PointerEventButton::Left);
        live(&op);
        assert!(up(&app).is_empty(), "a second click takes it down");
        assert!(!chips(&op)[1].on);
    }

    #[test]
    fn clear_button_and_f6_take_every_overlay_down() {
        let app = test_app();
        sign(&app, "A");
        sign(&app, "B");
        let op = opened(&app);
        op.toggle_overlay(0);
        live(&op);
        op.toggle_overlay(1);
        live(&op);
        assert_eq!(up(&app).len(), 2);
        settle();
        find(&op, "OverlaysStrip::overlays-clear-btn").unwrap().mock_single_click(PointerEventButton::Left);
        live(&op);
        assert!(up(&app).is_empty());
        op.toggle_overlay(0);
        live(&op);
        assert_eq!(up(&app), ["A"]);
        let f6 = slint::SharedString::from(char::from(Key::F6));
        op.window().dispatch_event(WindowEvent::KeyPressed { text: f6.clone() });
        op.window().dispatch_event(WindowEvent::KeyReleased { text: f6 });
        live(&op);
        assert!(up(&app).is_empty(), "F6 clears the overlays");
    }

    #[test]
    fn a_chip_clicked_after_an_unseen_clear_is_refused() {
        let app = test_app();
        sign(&app, "A");
        let op = opened(&app);
        // A clear from the phone that this window has not seen yet.
        app.command(crate::live::Envelope { id: "phone".into(), base: None, cmd: Command::ClearAll });
        op.toggle_overlay(0);
        assert!(up(&app).is_empty());
        let toasts: Vec<String> = find_all(&op, "ToastStack::toast").iter().map(|t| t.accessible_label().unwrap_or_default().to_string()).collect();
        settle();
        assert!(toasts.contains(&"The screen was cleared — try again.".to_string()), "{toasts:?}");
    }

    #[test]
    fn overlay_edit_while_on_shows_changed_dot_and_deleted_name() {
        let app = test_app();
        let id = sign(&app, "Welcome");
        let op = opened(&app);
        op.toggle_overlay(0);
        live(&op);
        if let OverlaySource::Text { text, .. } = &mut app.data.lock().unwrap().overlays[0].source { *text = "Welcome home".into() }
        refresh(&op);
        assert!(chips(&op)[0].changed);
        app.data.lock().unwrap().overlays.retain(|o| o.id != id);
        refresh(&op);
        assert!(chips(&op)[0].deleted && chips(&op)[0].on, "still up, struck through");
    }
}
