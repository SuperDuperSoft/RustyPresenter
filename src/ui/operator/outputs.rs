//! The live preview's screen picker (N5): Room shows the room as drawn here (`live`), Stage shows a picture of the
//! stage window taken once a second, so the operator can check it without walking to it. The performance line under
//! the preview (N33) and Window → Audience screen in a window (N30) live here too.
use super::{view::OperatorSnapshot, window::Operator};
use crate::{model::ScreenTarget, ui::{OutputsState, perf}};
use slint::{ComponentHandle, Image, Timer, TimerMode};
use std::{rc::Rc, time::Duration};
use uuid::Uuid;

/// How often the Stage view takes a new picture of the stage window. Each one draws that window again on the UI
/// thread, so not more often than its clock changes.
const STAGE_EVERY: Duration = Duration::from_secs(1);
/// How often the performance line is written again.
const PERF_EVERY: Duration = Duration::from_secs(1);
/// What the preview shows (`OutputsState.view`).
pub const ROOM: i32 = 0;
pub const STAGE: i32 = 1;

#[derive(Default)]
pub(crate) struct OutputsUi {
    stage_timer: Timer,
    perf_timer: Timer,
    /// The room's first screen before Audience screen in a window moved it there: its id, output and switch, put back
    /// when the window is closed again from the menu.
    before_window: Option<(Uuid, ScreenTarget, bool)>,
}

impl Operator {
    fn outputs(&self) -> OutputsState<'_> { self.ui.global::<OutputsState>() }

    /// The picker under the preview: Room or Stage.
    pub(crate) fn set_preview_view(&self, view: i32) {
        let g = self.outputs();
        g.set_view(view);
        let st = self.outputs_ui.borrow();
        if view != STAGE {
            st.stage_timer.stop();
            g.set_stage_frame(Image::default());
            return
        }
        let me = self.me.clone();
        st.stage_timer.start(TimerMode::Repeated, STAGE_EVERY, move || if let Some(op) = me.upgrade() { op.stage_tick() });
        drop(st);
        self.stage_tick();
    }

    /// One picture of the stage window, or "Stage screen is off".
    pub(crate) fn stage_tick(&self) {
        let w = match self.preview.size().0 { 0 => 640, w => w };
        let g = self.outputs();
        match self.screens().stage_snapshot(w) {
            Some(px) => { g.set_stage_frame(Image::from_rgba8(px)); g.set_stage_off(false) }
            None => { g.set_stage_frame(Image::default()); g.set_stage_off(true) }
        }
    }

    /// Writes the performance line again.
    pub(crate) fn perf_tick(&self) {
        let (room, stage) = self.state.borrow().snap.as_ref()
            .map_or((false, false), |s| (s.windows.iter().any(|w| w == "room"), s.windows.iter().any(|w| w == "stage")));
        let r = perf::Reading::now(room, stage);
        let g = self.outputs();
        g.set_perf(r.line().into());
        g.set_perf_warn(r.slow());
    }

    /// View → Show performance (also Settings → Video).
    fn toggle_perf(self: &Rc<Self>) {
        let on = !self.outputs().get_show_perf();
        let me = self.me.clone();
        self.ops.call::<serde_json::Value>("settings", serde_json::json!({ "show_performance": on }), move |r| {
            if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(&e, true) }
        });
    }

    /// Window → Audience screen in a window: the room shows in a plain window (for trying things out with no projector,
    /// or to capture it). Again: the window closes and the room goes back to where it was set to show.
    fn toggle_room_window(self: &Rc<Self>) {
        let Some(snap) = self.state.borrow().snap.clone() else { return };
        let me = self.me.clone();
        let done = move |r: Result<serde_json::Value, String>| if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(&e, true) };
        if !room_windowed(&snap) {
            let first = snap.settings.screens.audience.screens.first();
            self.outputs_ui.borrow_mut().before_window = first.map(|s| (s.id, s.output().target, snap.settings.screens.audience.on));
            return self.ops.call("display/open", serde_json::json!({ "role": "room" }), done);
        }
        let before = self.outputs_ui.borrow_mut().before_window.take();
        match before {
            Some((id, target, on)) if target != ScreenTarget::Window => {
                let me = self.me.clone();
                self.ops.call::<serde_json::Value>("screens/update", serde_json::json!({ "id": id, "target": target }), move |r| {
                    let Some(op) = me.upgrade() else { return };
                    if let Err(e) = r { return op.toast(&e, true) }
                    op.ops.call("screens/power", serde_json::json!({ "group": "audience", "on": on }), done);
                });
            }
            _ => self.ops.call("screens/power", serde_json::json!({ "group": "audience", "on": false }), done),
        }
    }
}

/// The room's first screen shows in a window, and is switched on.
pub fn room_windowed(s: &OperatorSnapshot) -> bool {
    let a = &s.settings.screens.audience;
    a.on && a.screens.first().is_some_and(|x| x.output().target == ScreenTarget::Window)
}

/// After each state read: the menu ticks, and the performance line on or off.
pub(super) fn render(op: &Operator, s: &OperatorSnapshot) {
    let g = op.outputs();
    g.set_room_windowed(room_windowed(s));
    let on = s.settings.show_performance;
    perf::set_wanted(on);
    g.set_show_perf(on);
    let st = op.outputs_ui.borrow();
    if on && !st.perf_timer.running() {
        let me = op.me.clone();
        st.perf_timer.start(TimerMode::Repeated, PERF_EVERY, move || if let Some(op) = me.upgrade() { op.perf_tick() });
    } else if !on {
        st.perf_timer.stop();
        g.set_perf("".into());
        g.set_perf_warn(false);
    }
    drop(st);
    if on { op.perf_tick() }
    if g.get_view() == STAGE { op.stage_tick() }
}

pub(super) fn wire(op: &Rc<Operator>) {
    let g = op.outputs();
    let me = op.me.clone();
    g.on_view_changed(move |v| if let Some(op) = me.upgrade() { op.set_preview_view(v) });
    let me = op.me.clone();
    g.on_toggle_perf(move || if let Some(op) = me.upgrade() { op.toggle_perf() });
    let me = op.me.clone();
    g.on_toggle_room_window(move || if let Some(op) = me.upgrade() { op.toggle_room_window() });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::test_app,
        ui::{operator::window::{OperatorScreens, VideoInfo, testing::refresh}, ops::Ops},
        video::{DrawMode, FrameGrabber},
    };
    use slint::{Model, Rgba8Pixel, SharedPixelBuffer};
    use std::cell::Cell;

    /// A stage window that is open while `open` says.
    struct StageScreens { open: Rc<Cell<bool>> }

    impl OperatorScreens for StageScreens {
        fn video_info(&self) -> VideoInfo { VideoInfo { draw_mode: DrawMode::Software, mpv: None } }
        fn preview_grabber(&self) -> Option<FrameGrabber> { None }
        fn stage_snapshot(&self, max_w: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
            self.open.get().then(|| SharedPixelBuffer::new(max_w.min(64), 36))
        }
    }

    #[test]
    fn preview_picks_the_stage_screen() {
        crate::ui::ui_test_init();
        let app = test_app();
        let open = Rc::new(Cell::new(true));
        let op = Operator::open(app.clone(), Ops::inline(app.clone()), Rc::new(StageScreens { open: open.clone() })).unwrap();
        refresh(&op);
        let g = op.ui.global::<OutputsState>();
        assert_eq!(g.get_view(), ROOM, "the room by default");
        assert_eq!(g.get_view_names().row_count(), 2);
        g.invoke_view_changed(STAGE);
        assert_eq!(g.get_view(), STAGE);
        assert!(g.get_stage_frame().size().width > 0, "a picture of the stage window");
        assert!(!g.get_stage_off());
        open.set(false);
        op.stage_tick();
        assert!(g.get_stage_off(), "closed: the preview says the stage screen is off");
        assert_eq!(g.get_stage_frame().size().width, 0);
        g.invoke_view_changed(ROOM);
        assert_eq!(g.get_view(), ROOM);
        assert!(!op.outputs_ui.borrow().stage_timer.running(), "no pictures taken while the room is shown");
    }

    #[test]
    fn show_performance_from_the_menu() {
        crate::ui::ui_test_init();
        let app = test_app();
        let op = crate::ui::test_support::operator_for_tests(app.clone());
        refresh(&op);
        let g = op.ui.global::<OutputsState>();
        assert!(!g.get_show_perf());
        assert_eq!(g.get_perf(), "", "off by default");
        g.invoke_toggle_perf();
        assert!(app.data.lock().unwrap().settings.show_performance, "saved");
        refresh(&op);
        assert!(g.get_show_perf());
        assert!(g.get_perf().contains("Audience screen is off"), "{}", g.get_perf());
        assert!(op.outputs_ui.borrow().perf_timer.running());
        g.invoke_toggle_perf();
        refresh(&op);
        assert_eq!(g.get_perf(), "");
        assert!(!op.outputs_ui.borrow().perf_timer.running());
    }

    /// Windows that open without a desktop.
    struct NoDesk;

    impl crate::app::Screens for NoDesk {
        fn open(&self, _role: &str, _monitor: Option<&str>) -> Result<(), String> { Ok(()) }
        fn close(&self, _role: &str) {}
        fn open_roles(&self) -> Vec<String> { vec![] }
        fn monitors(&self) -> Vec<crate::app::Monitor> { vec![] }
    }

    #[test]
    fn room_screen_in_a_window_and_back() {
        crate::ui::ui_test_init();
        let app = test_app();
        app.set_screens(std::sync::Arc::new(NoDesk));
        let projector = ScreenTarget::Monitor("DISPLAY2".into());
        app.edit(|d| { d.settings.screens.assign("audience", projector.clone()); d.settings.screens.audience.on = false; Ok(()) }).unwrap();
        let op = crate::ui::test_support::operator_for_tests(app.clone());
        refresh(&op);
        let g = op.ui.global::<OutputsState>();
        assert!(!g.get_room_windowed());
        g.invoke_toggle_room_window();
        refresh(&op);
        let a = app.data.lock().unwrap().settings.screens.audience.clone();
        assert!(a.on && a.screens[0].output().target == ScreenTarget::Window, "the room shows in a window");
        assert!(g.get_room_windowed());
        g.invoke_toggle_room_window();
        refresh(&op);
        let a = app.data.lock().unwrap().settings.screens.audience.clone();
        assert_eq!((a.on, a.screens[0].output().target.clone()), (false, projector), "back to the projector, switched off as before");
        assert!(!g.get_room_windowed());
    }
}
