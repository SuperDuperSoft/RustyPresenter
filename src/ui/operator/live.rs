//! The live column (`renderLive()` and `renderTransport()`, `operator.js:212-261`, buttons at `:345-347`): what is
//! live, the monitor, the play/pause rows, the notice, Back/Next, the clears and Up next. The monitor's layers are
//! drawn by `Preview`; while a video is live its frame comes from the room's player 4 times a second.
use super::{view::{self, RoomStatus, TransportKind, TransportRow}, window::Operator};
use crate::{
    app::LiveView,
    live::Command,
    room::Kind,
    ui::{Layer, OperatorWindow, Side, TransportCell, ops::outcome_toast, preview::{self, Drawn}, room::put, thumbs},
};
use slint::{ComponentHandle, Image, ModelRc, Timer, TimerMode, VecModel};
use std::{rc::{Rc, Weak}, time::Duration};

/// How often the preview asks the room's player for a frame (4 Hz).
const GRAB_EVERY: Duration = Duration::from_millis(250);

/// The column between updates.
#[derive(Default)]
pub(super) struct LiveState {
    /// The transport rows shown (a click names one by index).
    rows: Vec<TransportRow>,
    /// The instance of the video the preview follows, with a count bumped whenever it changes, so a frame grabbed
    /// for an older video is dropped.
    video: Option<u64>,
    video_gen: u64,
    grab_timer: Timer,
}

impl Operator {
    /// `renderLive()`: everything in the column that follows the live view.
    pub(super) fn render_live(&self) {
        let Some(live) = self.state.borrow().live.clone() else { return };
        let v = live.view();
        let ui = &self.ui;
        ui.set_live_pending(v.pending);
        ui.set_live_what(view::live_what(v).into());
        ui.set_live_notice(v.notice.clone().unwrap_or_default().into());
        ui.set_logo_on(v.frame.logo);
        let (title, body) = view::next_up(v);
        ui.set_up_next_title(title.into());
        ui.set_up_next_body(body.into());
        let rows = view::transport(v);
        let cells: Vec<TransportCell> = rows.iter().map(|r| TransportCell {
            video: r.kind == TransportKind::Video, label: r.label.as_str().into(), paused: r.paused, percent: r.percent, time: r.time.as_str().into(),
        }).collect();
        ui.set_transport(ModelRc::new(VecModel::from(cells)));
        self.live.borrow_mut().rows = rows;
        self.render_monitor(v);
    }

    /// The monitor: its layers (only those that changed are drawn again), and the video under them.
    fn render_monitor(&self, v: &LiveView) {
        self.preview.set_size(thumbs::thumb_size(self.ui.get_monitor_width(), self.ui.window().scale_factor()));
        let me = self.me.clone();
        self.preview.show(&v.frame, &v.theme, v.logo.as_deref(), move |layers| if let Some(op) = me.upgrade() { op.monitor_drawn(layers) });
        self.ui.set_monitor_bg_visible(preview::background_visible(&v.frame));
        self.follow_video(preview::drawn_video(&v.frame));
    }

    fn monitor_drawn(&self, layers: Vec<(Kind, Drawn)>) {
        for (kind, drawn) in layers {
            let (get, set): (fn(&OperatorWindow) -> Layer, fn(&OperatorWindow, Layer)) = match kind {
                Kind::Background => (OperatorWindow::get_monitor_bg, OperatorWindow::set_monitor_bg),
                Kind::Media => (OperatorWindow::get_monitor_media, OperatorWindow::set_monitor_media),
                Kind::Slide => (OperatorWindow::get_monitor_slide, OperatorWindow::set_monitor_slide),
                Kind::Logo => (OperatorWindow::get_monitor_logo, OperatorWindow::set_monitor_logo),
                Kind::Audio => continue,
            };
            let mut l = get(&self.ui);
            match drawn {
                Drawn::Pic(px, color) => put(&mut l, Side { image: px.map(|p| p.image()).unwrap_or_default(), color }),
                Drawn::Hidden => l.shown = false,
            }
            set(&self.ui, l);
        }
    }

    /// Starts or stops the 4 Hz grab when the drawn video changes. With the room closed there is nothing to grab:
    /// the monitor stays black and says so.
    fn follow_video(&self, video: Option<u64>) {
        self.ui.set_monitor_video(video.is_some());
        self.mark_room_off();
        let mut st = self.live.borrow_mut();
        if st.video == video { return }
        st.video = video;
        st.video_gen += 1;
        self.ui.set_monitor_video_frame(Image::default());
        if video.is_none() { return st.grab_timer.stop() }
        let me = self.me.clone();
        st.grab_timer.start(TimerMode::Repeated, GRAB_EVERY, move || if let Some(op) = me.upgrade() { op.grab_tick() });
    }

    /// "Room screen is off" over a live video while the room window is not open (a room that is opening, or
    /// preparing the video, shows black without it).
    pub(super) fn mark_room_off(&self) {
        let closed = self.state.borrow().snap.as_ref().is_none_or(|s| s.room == RoomStatus::Off);
        self.ui.set_monitor_room_off(closed && self.ui.get_monitor_video());
    }

    /// One 250 ms tick: ask the room's player for its frame, unless the last grab is still on its way.
    fn grab_tick(&self) {
        let Some(grabber) = self.screens().preview_grabber() else { return self.ui.set_monitor_video_frame(Image::default()) };
        let generation = self.live.borrow().video_gen;
        let me = self.me.clone();
        self.preview.grab(grabber, self.preview.size().0, move |px| {
            let Some(op) = me.upgrade() else { return };
            if op.live.borrow().video_gen != generation { return }
            if let Some(px) = px { op.ui.set_monitor_video_frame(Image::from_rgb8(px)) }
        });
    }

    /// Back and Next send the live version as `base` (a double press is refused as stale) and leave the Bible arm
    /// alone; only the keys clear it (`operator.js:343-344`).
    pub(super) fn step(self: &Rc<Self>, next: bool) {
        let base = self.state.borrow().live.as_ref().map(|l| l.view().version);
        self.command(if next { Command::Next } else { Command::Prev }, base);
    }

    /// A command from this column; a refusal becomes a toast (`cmd()`, `operator.js:80-86`).
    pub(super) fn command(self: &Rc<Self>, cmd: Command, base: Option<u64>) {
        let me = Rc::downgrade(self);
        self.ops.cmd(cmd, base, move |o| {
            if let (Some(op), Some((msg, err))) = (me.upgrade(), outcome_toast(&o)) { op.toast(&msg, err) }
        });
    }

    /// Play/pause on a transport row: the opposite of what it shows, for the instance it shows.
    fn toggle_transport(self: &Rc<Self>, i: usize) {
        let Some(row) = self.live.borrow().rows.get(i).cloned() else { return };
        let cmd = match row.kind {
            TransportKind::Video => Command::MediaPause { inst: row.inst, paused: !row.paused },
            TransportKind::Audio => Command::AudioPause { inst: row.inst, paused: !row.paused },
        };
        self.command(cmd, None);
    }

    /// The monitor was resized: draw its layers again at the new size.
    fn monitor_resized(&self) {
        let v = self.state.borrow().live.clone();
        if let Some(v) = v { self.render_monitor(v.view()) }
    }
}

/// The web's `data-cmd` of a clear button.
fn clear_command(name: &str) -> Option<Command> {
    Some(match name {
        "clear_all" => Command::ClearAll,
        "clear_slide" => Command::ClearSlide,
        "clear_media" => Command::ClearMedia,
        "clear_audio" => Command::ClearAudio,
        "logo" => Command::Logo,
        _ => return None,
    })
}

/// Connects the column's callbacks. Each holds a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let ui = &op.ui;
    let me: Weak<Operator> = Rc::downgrade(op);
    ui.on_live_next(move || if let Some(op) = me.upgrade() { op.step(true) });
    let me = Rc::downgrade(op);
    ui.on_live_prev(move || if let Some(op) = me.upgrade() { op.step(false) });
    // The clears send no base.
    let me = Rc::downgrade(op);
    ui.on_live_clear(move |name| if let (Some(op), Some(cmd)) = (me.upgrade(), clear_command(&name)) { op.command(cmd, None) });
    let me = Rc::downgrade(op);
    ui.on_transport_toggle(move |i| if let (Some(op), Ok(i)) = (me.upgrade(), usize::try_from(i)) { op.toggle_transport(i) });
    let me = Rc::downgrade(op);
    ui.on_monitor_resized(move |_| if let Some(op) = me.upgrade() { op.monitor_resized() });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, LiveUpdate, test_app},
        live::Command,
        model::{EndAction, ItemKind, MediaFile, MediaKind, MediaStatus, ServiceItem},
        render::Renderer,
        ui::{operator::{view, window::testing::{find, open, refresh}}, screens::NativeScreens, ops::Ops, ui_test_init},
        platform::monitors::MonitorId,
    };
    use slint::platform::PointerEventButton;
    use std::sync::Arc;
    use uuid::Uuid;

    fn add_text(app: &App, title: &str, body: &str) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: body.into() }, background: None, notes: String::new() };
        app.data.lock().unwrap().services[0].items.push(item.clone());
        item.id
    }

    fn add_video(app: &App) -> Uuid {
        let media = MediaFile { id: Uuid::new_v4(), name: "clip.mp4".into(), kind: MediaKind::Video, asset: "clip.mp4".into(), status: MediaStatus::Ready, original_asset: None };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id: media.id, autoplay: true, end: EndAction::Stop, looped: false }, background: None, notes: String::new() };
        let mut d = app.data.lock().unwrap();
        d.media.push(media);
        d.services[0].items.push(item.clone());
        item.id
    }

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1400.0, 900.0));
        refresh(&op);
        op
    }

    /// Runs `cmd` the way a phone would and hands the operator the new live view (no follower in tests).
    fn run(op: &Operator, cmd: Command) {
        op.ops().cmd(cmd, None, |o| assert!(o.ok, "{o:?}"));
        live(op);
    }

    fn live(op: &Operator) { op.apply_live(LiveUpdate::new(op.app().live_view())); }

    fn click(op: &Operator, id: &str) {
        find(op, id).unwrap_or_else(|| panic!("{id}")).mock_single_click(PointerEventButton::Left);
        live(op);
    }

    fn cursor(app: &App) -> Option<(Uuid, usize)> { app.live_view().cursor.map(|c| (c.item_id, c.slide)) }

    fn label(op: &Operator, id: &str) -> String { find(op, id).and_then(|e| e.accessible_label()).unwrap_or_default().to_string() }

    #[test]
    fn transport_rows_from_view() {
        let app = test_app();
        let id = add_video(&app);
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        let want = view::transport(&app.live_view());
        let rows: Vec<_> = slint::Model::iter(&op.ui.get_transport()).collect();
        assert_eq!(rows.len(), want.len());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].video);
        assert_eq!((rows[0].label.as_str(), rows[0].time.as_str(), rows[0].paused), ("Video", "—", want[0].paused));
    }

    #[test]
    fn next_button_advances_live() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2\n\na3");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        click(&op, "LiveColumn::next-btn");
        assert_eq!(cursor(&app), Some((id, 1)));
        click(&op, "LiveColumn::next-btn");
        assert_eq!(cursor(&app), Some((id, 2)));
    }

    #[test]
    fn next_button_keeps_the_arm() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        op.state.borrow_mut().armed = Some(id);
        click(&op, "LiveColumn::next-btn");
        assert_eq!(cursor(&app), Some((id, 1)));
        assert_eq!(op.state.borrow().armed, Some(id), "only the keys clear the arm (operator.js:343-344)");
        click(&op, "LiveColumn::prev-btn");
        assert_eq!(op.state.borrow().armed, Some(id));
    }

    #[test]
    fn prev_button_goes_back() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2\n\na3");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 2 });
        click(&op, "LiveColumn::prev-btn");
        assert_eq!(cursor(&app), Some((id, 1)));
    }

    #[test]
    fn next_button_sends_the_live_version_as_base() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2\n\na3");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        // Something else moved the screen; this window has not heard yet, so its Next is stale and silent.
        app.command(crate::live::Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id: id, slide: 2 } });
        find(&op, "LiveColumn::next-btn").unwrap().mock_single_click(PointerEventButton::Left);
        assert_eq!(cursor(&app), Some((id, 2)), "a stale Next does nothing");
    }

    #[test]
    fn clear_all_button_clears_frame() {
        let app = test_app();
        let id = add_text(&app, "A", "a1");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        assert!(app.live_view().frame.slide.is_some());
        assert_eq!(label(&op, "LiveColumn::live-what"), "A · A 1");
        click(&op, "LiveColumn::clear-all");
        assert!(app.live_view().frame.slide.is_none());
        assert_eq!(label(&op, "LiveColumn::live-what"), "Clear");
        run(&op, Command::Show { item_id: id, slide: 0 });
        click(&op, "LiveColumn::clear-text");
        assert!(app.live_view().frame.slide.is_none());
    }

    #[test]
    fn logo_button_marked_on_while_logo_shown() {
        let app = test_app();
        let op = opened(&app);
        let on = || find(&op, "LiveColumn::clear-logo").unwrap().accessible_checked();
        assert_eq!(on(), Some(false));
        click(&op, "LiveColumn::clear-logo");
        assert!(app.live_view().frame.logo);
        assert_eq!(on(), Some(true));
        assert_eq!(label(&op, "LiveColumn::live-what"), "Logo");
        // Logo is not a toggle (`live.rs`): another clear takes it off.
        click(&op, "LiveColumn::clear-all");
        assert_eq!(on(), Some(false));
    }

    #[test]
    fn pending_marker_while_requested_not_committed() {
        let app = test_app();
        let id = add_text(&app, "A", "a1");
        let op = opened(&app);
        assert!(find(&op, "LiveColumn::pending").is_none());
        run(&op, Command::Show { item_id: id, slide: 0 });
        assert!(app.live_view().pending, "no room committed it");
        assert_eq!(label(&op, "LiveColumn::pending"), "updating…");
        let mut v = app.live_view();
        v.pending = false;
        v.version += 1;
        op.apply_live(LiveUpdate::new(v));
        assert!(find(&op, "LiveColumn::pending").is_none());
    }

    #[test]
    fn notice_shown_from_live_view() {
        let app = test_app();
        let op = opened(&app);
        assert!(find(&op, "LiveColumn::notice").is_none());
        let mut v = app.live_view();
        v.notice = Some("The room screen restarted.".into());
        v.version += 1;
        op.apply_live(LiveUpdate::new(v));
        assert_eq!(label(&op, "LiveColumn::notice"), "The room screen restarted.");
    }

    #[test]
    fn room_monitor_gone_shows_notice() {
        let app = test_app();
        ui_test_init();
        let screens = NativeScreens::new(&app, Renderer::new(), None);
        let op = crate::ui::operator::window::Operator::open(app.clone(), Ops::inline(app.clone()), Rc::new(screens.clone())).unwrap();
        let mon = |name: &str, x: i32| MonitorId { name: name.into(), size: (1920, 1080), position: (x, 0) };
        let (laptop, projector) = (mon(r"\\.\DISPLAY1", 0), mon(r"\\.\DISPLAY2", 1920));
        screens.look_at(vec![laptop.clone(), projector.clone()], Some(laptop.clone()));
        crate::app::Screens::open(&*screens, "room", Some(&projector.key())).unwrap();
        // The projector is unplugged: the Plan 2 watcher closes the room and leaves a notice for the operator.
        screens.look_at(vec![laptop.clone()], Some(laptop));
        live(&op);
        assert_eq!(label(&op, "LiveColumn::notice"), crate::ui::screens::gone_notice("room"));
        screens.shutdown();
    }

    #[test]
    fn up_next_end_of_service() {
        let app = test_app();
        let op = opened(&app);
        assert_eq!(label(&op, "LiveColumn::up-next"), "—");
        let id = add_text(&app, "A", "a1\n\na2");
        refresh(&op);
        run(&op, Command::Show { item_id: id, slide: 0 });
        let up = find(&op, "LiveColumn::up-next").unwrap();
        assert_eq!((up.accessible_description().unwrap().as_str(), up.accessible_label().unwrap().as_str()), ("A · A 2", "a2"));
        run(&op, Command::Show { item_id: id, slide: 1 });
        assert_eq!(label(&op, "LiveColumn::up-next"), "End of service");
    }

    #[test]
    fn transport_button_tooltip_play_or_pause() {
        let app = test_app();
        let id = add_video(&app);
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        let tip = || find(&op, "LiveColumn::transport-video").and_then(|b| b.accessible_description()).unwrap_or_default().to_string();
        let paused = || matches!(app.live_view().frame.media.map(|m| m.v), Some(crate::live::MediaLayer::Video { paused: true, .. }));
        assert!(!paused());
        assert_eq!(tip(), "Pause");
        click(&op, "LiveColumn::transport-video");
        assert!(paused(), "the button sends media_pause for the instance on air");
        assert_eq!(tip(), "Play");
        click(&op, "LiveColumn::transport-video");
        assert!(!paused());
        assert_eq!(tip(), "Pause");
    }

    #[test]
    fn video_preview_without_room_says_room_screen_is_off() {
        let app = test_app();
        let id = add_video(&app);
        let txt = add_text(&app, "A", "a1");
        let op = opened(&app);
        assert!(find(&op, "LiveMonitor::room-off").is_none());
        run(&op, Command::Show { item_id: id, slide: 0 });
        assert!(op.ui.get_monitor_video());
        assert_eq!(label(&op, "LiveMonitor::room-off"), "Room screen is off");
        assert_eq!(op.ui.get_monitor_video_frame().size().width, 0, "no second player: the frame stays empty");
        // Once the room is open (and drawing nothing yet) the caption goes; a slide has no caption at all.
        let _room = app.screen_guard("room");
        refresh(&op);
        assert!(find(&op, "LiveMonitor::room-off").is_none());
        run(&op, Command::Show { item_id: txt, slide: 0 });
        assert!(!op.ui.get_monitor_video());
    }

    #[test]
    fn monitor_shows_the_live_slide() {
        let app = test_app();
        let id = add_text(&app, "A", "Amazing grace");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        let slide = op.ui.get_monitor_slide();
        assert!(slide.shown);
        let face = if slide.b_top { slide.b } else { slide.a };
        assert!(face.image.size().width > 0, "the slide text is drawn");
        assert!(op.ui.get_monitor_bg().shown, "the settings background is drawn");
        click(&op, "LiveColumn::clear-all");
        assert!(!op.ui.get_monitor_slide().shown);
    }
}
