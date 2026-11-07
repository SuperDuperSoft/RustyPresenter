//! The operator window: opening it, the top bar (`operator.js:93-107`) and the follower that turns core updates
//! into snapshots for the Slint thread (`operator.js:292-302`). The service column is in `service`, the slides
//! column in `slides`, the live column in `live`, the keyboard in `keys`, the Bible box in `bible`, toasts and jobs
//! in `overlays`; the dialogs come in later tasks.
use super::{bible, checks, files::{self, FilesState}, keys, live::{self, LiveState}, media_bin::{self, BinState}, menu, overlays::{self, Overlays}, service, slides::{self, SlidesState}, strings as tr, text_size, view::{self, OperatorSnapshot, RoomStatus}};
use crate::{
    app::{App, LiveUpdate, Update},
    ui::{BinFolder, BinRow, OperatorWindow, ServiceRow, SlideRowCells, ops::Ops, preview::Preview, thumbs::Thumbs},
    video::{DrawMode, FrameGrabber},
};
use slint::{ComponentHandle, VecModel};
use std::{cell::RefCell, rc::{Rc, Weak}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};
use tokio::sync::{Notify, broadcast::{Receiver, error::{RecvError, TryRecvError}}};
use uuid::Uuid;

/// How video is drawn and which libmpv loaded, for the missing-player note and Settings → About.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoInfo { pub draw_mode: DrawMode, pub mpv: Option<MpvInfo> }

/// The libmpv that loaded: its client API version, the file it came from, and its `mpv-version` and
/// `ffmpeg-version` (empty when they could not be read).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MpvInfo { pub api: (u32, u32), pub path: String, pub mpv_version: String, pub ffmpeg_version: String, pub configuration: String }

impl VideoInfo {
    /// No libmpv: video and audio items say "The video player is missing" (native spec §5).
    pub fn player_missing(&self) -> bool { self.mpv.is_none() }
}

/// What the operator needs from the windowing layer. `NativeScreens` implements it; tests pass a fake, so no
/// room window or mpv is needed.
pub trait OperatorScreens {
    fn video_info(&self) -> VideoInfo;
    /// The grabber of the video the room is drawing, for the live preview; `None` when the room is closed or draws
    /// no video. Called on the Slint thread.
    fn preview_grabber(&self) -> Option<FrameGrabber>;
    /// N5: a picture of the stage window as it is now, at most `max_w` wide, for the preview's Stage view; `None`
    /// while the stage window is closed. Called on the Slint thread.
    fn stage_snapshot(&self, max_w: u32) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> { let _ = max_w; None }
}

/// `NativeScreens` lives in an `Arc`: `Rc::new(screens.clone())` is then an `Rc<dyn OperatorScreens>`.
impl<T: OperatorScreens + ?Sized> OperatorScreens for Arc<T> {
    fn video_info(&self) -> VideoInfo { (**self).video_info() }
    fn preview_grabber(&self) -> Option<FrameGrabber> { (**self).preview_grabber() }
    fn stage_snapshot(&self, max_w: u32) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> { (**self).stage_snapshot(max_w) }
}

pub use crate::ui::dialogs::DialogKind;

/// The window's state between updates (the web's `S`, `L`, `selectedId` and `bibleArmed`, `operator.js:62-68`).
#[derive(Default)]
pub(crate) struct OpState {
    pub(crate) snap: Option<Arc<OperatorSnapshot>>,
    pub(super) live: Option<LiveUpdate>,
    pub(crate) selected: Option<Uuid>,
    pub(super) armed: Option<Uuid>,
    /// Verses per slide picked in the Bible box or Add → Bible this session; `None` follows Settings.
    pub(crate) bible_per_slide: Option<u8>,
    /// The open dialog, and how many confirm/prompt overlays are up; either one blocks the operator keys.
    pub(crate) dialog: Option<DialogKind>,
    pub(crate) asks: usize,
    /// The language the window's Rust-made text was last drawn in (`crate::i18n::current`).
    pub(super) lang: Option<String>,
    /// The blank the open Add dialog fills (FEAT-6a): its add buttons replace that blank instead of adding.
    pub(crate) fill: Option<Uuid>,
    /// The open dialog's hooks and the questions waiting (see `ui::dialogs`).
    pub(crate) dialogs: crate::ui::dialogs::DialogHooks,
    /// A service row is being dragged: new snapshots wait in `held_snapshot` (a rebuild would end the drag).
    pub(super) dragging: bool,
    pub(super) held_snapshot: Option<Arc<OperatorSnapshot>>,
    /// View → Zoom: steps from 100 % (`menu::ZOOMS`).
    pub(super) zoom_step: i8,
    /// Every toast shown, for tests that check a message without finding it on screen.
    #[cfg(test)]
    pub(super) toasts: Vec<(String, bool)>,
}

pub struct Operator {
    pub(crate) ui: OperatorWindow,
    app: Arc<App>,
    pub(super) ops: Ops,
    screens: Rc<dyn OperatorScreens>,
    pub(crate) state: RefCell<OpState>,
    /// The service list's rows; one model for the window's life (see `service`).
    pub(super) service_rows: Rc<VecModel<ServiceRow>>,
    /// The slide grid's rows; one model for the window's life (see `slides`).
    pub(super) slide_rows: Rc<VecModel<SlideRowCells>>,
    pub(super) slides: RefCell<SlidesState>,
    pub(super) thumbs: Thumbs,
    /// The live column between updates (see `live`), and its monitor's drawing.
    pub(super) live: RefCell<LiveState>,
    pub(super) preview: Preview,
    /// The media library (see `media_bin`). Its thumbnails have their own worker, so a slide grid redraw never cancels them.
    pub(super) bin: RefCell<BinState>,
    pub(super) bin_rows: Rc<VecModel<BinRow>>,
    pub(super) bin_folders: Rc<VecModel<BinFolder>>,
    pub(super) bin_thumbs: Thumbs,
    /// The toasts and the jobs box (see `overlays`).
    pub(super) overlays: Overlays,
    /// Adding files: PowerPoint imports waiting for their deck, the desktop drop, the window's event filter (see `files`).
    pub(super) files: FilesState,
    /// N2: re-runs the service check for the status pill.
    pub(super) check_timer: slint::Timer,
    /// N5, N30, N33: the preview's screen picker and the performance line (see `outputs`).
    pub(super) outputs_ui: RefCell<super::outputs::OutputsUi>,
    /// This window, for callbacks that outlive a call (thumbnails arrive later).
    pub(crate) me: Weak<Operator>,
    /// The follower's mailbox; `None` on the testing backend, which has no event loop to deliver into.
    feed: Option<Arc<Feed>>,
    /// How many times `raise` ran, for the second-launch test.
    #[cfg(test)]
    pub(crate) raises: std::cell::Cell<usize>,
}

impl Operator {
    /// Opens the window. The first state read happens on the follower thread (the web `onopen`); until it arrives
    /// the columns are empty, the service is called "Service" and there is no Welcome panel.
    pub fn open(app: Arc<App>, ops: Ops, screens: Rc<dyn OperatorScreens>) -> Result<Rc<Operator>, String> {
        let ui = OperatorWindow::new().map_err(tr::open_failed)?;
        // Slint picks the computer's language for the first window; show the app's.
        crate::i18n::sync_slint();
        // Its frames never wait for the monitor's refresh, so the room's are not held up behind them.
        crate::ui::pacing::attach(ui.window(), "operator", false);
        let feed = Feed::new(app.clone(), screens.video_info().player_missing());
        // Subscribe before the first read so no update falls in between.
        let rx = app.subscribe();
        let op = Rc::new_cyclic(|me: &Weak<Operator>| {
            let listening = listen(me.clone(), feed.clone());
            // Tests (inline actions, no event loop) draw thumbnails and the live monitor inline too.
            let (thumbs, preview) = if ops.is_inline() { (Thumbs::inline(app.store.root.clone()), Preview::inline(app.store.root.clone())) }
                else { (Thumbs::new(app.store.root.clone()), Preview::new(app.store.root.clone())) };
            let bin_thumbs = media_bin::new_thumbs(app.store.root.clone(), ops.is_inline());
            Operator {
                ui, app, ops, screens, state: RefCell::default(), service_rows: Rc::default(), slide_rows: Rc::default(),
                bin: RefCell::default(), bin_rows: Rc::default(), bin_folders: Rc::default(), bin_thumbs,
                slides: RefCell::default(), thumbs, live: RefCell::default(), preview, overlays: Overlays::default(), files: FilesState::default(), check_timer: slint::Timer::default(), outputs_ui: RefCell::default(), me: me.clone(), feed: listening.then(|| feed.clone()),
                #[cfg(test)]
                raises: Default::default(),
            }
        });
        service::wire(&op);
        slides::wire(&op);
        text_size::wire(&op);
        live::wire(&op);
        media_bin::wire(&op);
        keys::wire(&op);
        bible::wire(&op);
        overlays::wire(&op);
        super::overlay_strip::wire(&op);
        super::service_theme::wire(&op);
        crate::ui::dialogs::wire(&op);
        files::wire(&op);
        super::automation::wire(&op);
        checks::wire(&op);
        checks::start_watch(&op);
        menu::wire(&op);
        super::outputs::wire(&op);
        op.ui.show().map_err(tr::open_failed)?;
        if op.feed.is_some() { follow(feed, rx); }
        Ok(op)
    }

    pub fn window(&self) -> &slint::Window { self.ui.window() }

    pub fn app(&self) -> &Arc<App> { &self.app }

    pub fn ops(&self) -> &Ops { &self.ops }

    pub fn screens(&self) -> &Rc<dyn OperatorScreens> { &self.screens }

    /// Brings the window forward: shown, not minimised, focused (a second launch asks for this).
    pub fn raise(&self) {
        #[cfg(test)]
        self.raises.set(self.raises.get() + 1);
        let _ = self.ui.show();
        self.ui.window().set_minimized(false);
        use slint::winit_030::WinitWindowAccessor;
        self.ui.window().with_winit_window(|w| w.focus_window());
    }

    /// Whether this is the focused (foreground) window; never on the testing backend.
    pub fn focused(&self) -> bool {
        use slint::winit_030::WinitWindowAccessor;
        self.ui.window().with_winit_window(|w| w.has_focus()).unwrap_or(false)
    }

    /// Takes focus back from a room or stage window that just opened (winit activates every new window), so the
    /// arrow keys, Space, Enter and F1–F5 keep working without a click.
    pub fn refocus(&self) {
        use slint::winit_030::WinitWindowAccessor;
        self.ui.window().with_winit_window(|w| w.focus_window());
    }

    /// Asks the follower for a fresh full read (monitors included), e.g. when the Screens dialog opens.
    /// Without a follower (tests on the testing backend, `Ops::inline`) the read is done and applied at once.
    pub fn request_refresh(&self) {
        let Some(feed) = self.feed.clone() else {
            if self.ops.is_inline() {
                let player_missing = self.screens.video_info().player_missing();
                self.apply_snapshot(Arc::new(OperatorSnapshot::read(&self.app).with_player_missing(player_missing)));
            }
            return
        };
        let _ = std::thread::Builder::new().name("operator-refresh".into()).spawn(move || feed.read_all());
    }

    /// A new state read (`refresh()` in `operator.js:69-74`). Held while a service row is being dragged, and
    /// applied when the drag ends.
    pub fn apply_snapshot(&self, s: Arc<OperatorSnapshot>) {
        let live_item = self.live_item();
        let mut st = self.state.borrow_mut();
        if st.dragging { st.held_snapshot = Some(s); return; }
        st.selected = view::reselect(st.selected, &s.items, live_item);
        // Only a new read or a conversion changes `data`; a job's progress patch shares it.
        let data_changed = st.snap.as_ref().is_none_or(|old| !Arc::ptr_eq(&old.data, &s.data));
        // Settings → Language switched the app (maybe on another thread): this thread's windows follow, and every
        // Rust-made text below is drawn again in the new language.
        let lang = crate::i18n::current();
        let lang_changed = st.lang.replace(lang.clone()).is_some_and(|old| old != lang);
        if lang_changed { crate::i18n::sync_slint() }
        st.snap = Some(s.clone());
        drop(st);
        self.render_top(&s);
        self.render_service();
        self.render_slides();
        self.render_jobs();
        crate::ui::dialogs::settings_bible::jobs_changed(self);
        if data_changed { self.render_media(); self.render_overlay_strip(); } else { self.mark_media(); }
        self.mark_room_off();
        self.ui.set_item_count(s.items.len() as i32);
        self.ui.set_loaded(true);
        if data_changed || lang_changed { self.dialog_refresh(); }
        if lang_changed && self.state.borrow().live.is_some() { self.render_live(); }
        super::automation::render(self);
        self.watch_decks();
    }

    /// A new live view. An older one (a full read that raced a newer `Live`) is ignored.
    pub fn apply_live(&self, v: LiveUpdate) {
        {
            let mut st = self.state.borrow_mut();
            if st.live.as_ref().is_some_and(|l| l.view().version > v.view().version) { return; }
            st.live = Some(v);
        }
        self.mark_service();
        self.mark_slides();
        self.mark_media();
        self.render_live();
        super::automation::render(self);
    }

    /// A new timer readout (FEAT-2): the strip and the Automation dialog's readouts follow it.
    pub fn apply_timers(&self) {
        super::automation::render(self);
        crate::ui::dialogs::automation::readouts(self);
    }

    /// The item the live cursor is on (`L.cursor.item_id`), if any.
    pub(super) fn live_item(&self) -> Option<Uuid> {
        self.state.borrow().live.as_ref().and_then(|l| l.view().cursor.as_ref().map(|c| c.item_id))
    }

    /// `renderTop()` (`operator.js:93-107`).
    fn render_top(&self, s: &OperatorSnapshot) {
        let ui = &self.ui;
        ui.set_service_name(s.service_name.as_str().into());
        super::bible::render_chips(ui, s);
        ui.set_bible_per_slide(i32::from(self.state.borrow().bible_per_slide.unwrap_or(s.settings.bible_per_slide).min(4)));
        let (on, tip) = match s.room {
            RoomStatus::Showing => (true, tr::room_showing()),
            RoomStatus::Starting => (false, tr::room_starting()),
            RoomStatus::Off => (false, tr::room_off()),
        };
        ui.set_room_on(on);
        ui.set_room_state(if on { "on" } else { "warn" }.into());
        ui.set_room_tip(tip.into());
        let stage = view::stage_pill(s);
        ui.set_stage_shown(stage.is_some());
        ui.set_stage_on(stage.unwrap_or(false));
        ui.set_phones_label(view::phones_label(s.phones).unwrap_or_default().into());
        ui.set_has_service(s.service_id.is_some());
        super::menu::render(self, s);
        super::outputs::render(self, s);
    }
}

impl Drop for Operator {
    fn drop(&mut self) {
        // The follower thread sits in `blocking_recv`, so it sees `closed` and exits (dropping its `Arc<App>`) at the
        // next core update after this, not at once. Harmless: the window lives as long as the app.
        if let Some(feed) = &self.feed { feed.close(); }
        let _ = self.ui.hide();
    }
}

/// What the follower has read and the Slint thread has not applied yet: only the newest of each.
#[derive(Default)]
struct Pending { snap: Option<Arc<OperatorSnapshot>>, live: Option<LiveUpdate>, timers: bool }

/// The follower's side: reads and patches snapshots off the Slint thread and posts them to `listen`.
struct Feed {
    app: Arc<App>,
    /// The core cannot tell whether libmpv loaded, so every read is told (`OperatorSnapshot::with_player_missing`).
    player_missing: bool,
    /// The newest snapshot read or patched. Held for a whole read, so reads and patches are posted in order.
    current: Mutex<Option<Arc<OperatorSnapshot>>>,
    pending: Mutex<Pending>,
    wake: Notify,
    closed: AtomicBool,
}

impl Feed {
    fn new(app: Arc<App>, player_missing: bool) -> Arc<Feed> {
        Arc::new(Feed { app, player_missing, current: Mutex::default(), pending: Mutex::default(), wake: Notify::new(), closed: AtomicBool::new(false) })
    }

    /// A full read of the state and the live view (`refresh()`). Never on the Slint thread (see `OperatorSnapshot::read`).
    fn read_all(&self) {
        let mut current = self.current.lock().unwrap();
        let snap = Arc::new(OperatorSnapshot::read(&self.app).with_player_missing(self.player_missing));
        *current = Some(snap.clone());
        self.post(Some(snap), Some(LiveUpdate::new(self.app.live_view())));
    }

    /// Changes the newest snapshot without a full read. Before the first read there is nothing to change: that
    /// read carries the progress anyway.
    fn patch(&self, change: impl FnOnce(&mut OperatorSnapshot) -> bool) {
        let mut current = self.current.lock().unwrap();
        let Some(s) = current.as_ref() else { return };
        let mut s = (**s).clone();
        if !change(&mut s) { return }
        let s = Arc::new(s);
        *current = Some(s.clone());
        self.post(Some(s), None);
    }

    /// A batch of updates that arrived together (`None` is a lag). `Data` or a lag means one full read, like
    /// `refreshSoon`; that read is newer than any job, conversion or live update in the batch. Otherwise job and
    /// conversion progress patch the snapshot (`operator.js:299-300`) and only the newest live view is posted.
    fn handle(&self, batch: Vec<Option<Update>>) {
        if batch.iter().any(|u| matches!(u, None | Some(Update::Data))) { return self.read_all() }
        let mut live = None;
        for u in batch {
            match u {
                Some(Update::Live(v)) => live = Some(v),
                // FEAT-2: the strip reads the newest readout itself (`App::timers`).
                Some(Update::Timers(_)) => { self.pending.lock().unwrap().timers = true; self.wake.notify_one(); }
                Some(Update::Job { id, percent }) => self.patch(|s| match s.jobs.iter_mut().find(|j| j.id == id) {
                    Some(j) => { j.percent = percent; true }
                    None => false,
                }),
                _ => {}
            }
        }
        if live.is_some() { self.post(None, live) }
    }

    fn post(&self, snap: Option<Arc<OperatorSnapshot>>, live: Option<LiveUpdate>) {
        let mut p = self.pending.lock().unwrap();
        if snap.is_some() { p.snap = snap; }
        // A refresh read can finish before an older `Live` from the queue is posted: keep the newer view.
        if let Some(v) = live && p.live.as_ref().is_none_or(|old| old.view().version <= v.view().version) { p.live = Some(v); }
        drop(p);
        self.wake.notify_one();
    }

    fn take(&self) -> Pending { std::mem::take(&mut *self.pending.lock().unwrap()) }

    fn close(&self) { self.closed.store(true, Ordering::Relaxed); self.wake.notify_one(); }

    fn is_closed(&self) -> bool { self.closed.load(Ordering::Relaxed) }
}

/// Applies what the follower posts, on the Slint thread. False without an event loop (the testing backend), where
/// tests call `apply_snapshot` and `apply_live` themselves.
fn listen(me: Weak<Operator>, feed: Arc<Feed>) -> bool {
    slint::spawn_local(async move {
        loop {
            feed.wake.notified().await;
            if feed.is_closed() { return }
            let Some(op) = me.upgrade() else { return };
            let Pending { snap, live, timers } = feed.take();
            // Live first, so the selection can fall back to the live item (`operator.js:73`).
            if let Some(v) = live { op.apply_live(v); }
            if let Some(s) = snap { op.apply_snapshot(s); }
            if timers { op.apply_timers(); }
        }
    }).is_ok()
}

/// Follows the core's updates on a plain thread (like `ui::stage::follow`), starting with the first full read.
fn follow(feed: Arc<Feed>, mut rx: Receiver<Update>) {
    let _ = std::thread::Builder::new().name("operator-updates".into()).spawn(move || {
        feed.read_all();
        loop {
            let first = match rx.blocking_recv() {
                Ok(u) => Some(u),
                Err(RecvError::Lagged(_)) => None,
                Err(RecvError::Closed) => return,
            };
            if feed.is_closed() { return }
            let mut batch = vec![first];
            loop {
                match rx.try_recv() {
                    Ok(u) => batch.push(Some(u)),
                    Err(TryRecvError::Lagged(_)) => batch.push(None),
                    Err(_) => break,
                }
            }
            feed.handle(batch);
        }
    });
}

/// What the operator's tests share: a window on the testing backend with a fake `OperatorScreens`.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::ui::ui_test_init;
    use i_slint_backend_testing::ElementHandle;

    pub use crate::ui::test_support::FakeScreens;

    pub fn open(app: &Arc<App>) -> Rc<Operator> {
        ui_test_init();
        Operator::open(app.clone(), Ops::inline(app.clone()), Rc::new(FakeScreens { missing: false })).unwrap()
    }

    /// The follower's read, done here because the testing backend has no event loop to deliver it.
    pub fn refresh(op: &Operator) { op.apply_snapshot(Arc::new(OperatorSnapshot::read(&op.app))); }

    pub fn find(op: &Operator, id: &str) -> Option<ElementHandle> { ElementHandle::find_by_element_id(&op.ui, id).next() }

    pub fn find_all(op: &Operator, id: &str) -> Vec<ElementHandle> { ElementHandle::find_by_element_id(&op.ui, id).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::testing::{open, refresh, find};
    use crate::{
        app::test_app,
        model::{EndAction, ItemKind, MediaFile, MediaKind, MediaStatus, ServiceItem},
        ui::ui_test_init,
    };

    /// A video item in the current service; returns its media id.
    fn add_video(app: &App) -> Uuid {
        let media = MediaFile { id: Uuid::new_v4(), name: "clip.mp4".into(), kind: MediaKind::Video, asset: "clip.mp4".into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id: media.id, autoplay: true, end: EndAction::Stop, looped: false }, background: None, notes: String::new(), theme: None };
        let mut data = app.data.lock().unwrap();
        data.media.push(media.clone());
        data.services[0].items.push(item);
        media.id
    }

    #[test]
    fn top_bar_shows_service_name_or_service() {
        let app = test_app();
        let op = open(&app);
        assert_eq!(op.ui.get_service_name(), "Service", "before the first read");
        refresh(&op);
        assert_eq!(op.ui.get_service_name(), "Sunday Service");
        { let mut d = app.data.lock().unwrap(); d.services.clear(); d.current_service = None; }
        refresh(&op);
        assert_eq!(op.ui.get_service_name(), "Service");
    }

    #[test]
    fn room_pill_warns_when_room_not_connected() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        let pill = find(&op, "TopBar::status-room").expect("room pill");
        assert_eq!(pill.accessible_value().unwrap_or_default(), "warn");
        assert_eq!(pill.accessible_description().unwrap_or_default(), "Room screen is not open — click Screens");
        assert_eq!(pill.accessible_label().unwrap_or_default(), "Room");
    }

    #[test]
    fn room_pill_on_with_screen_guard() {
        let app = test_app();
        let op = open(&app);
        let _room = app.screen_guard("room");
        refresh(&op);
        let pill = find(&op, "TopBar::status-room").expect("room pill");
        assert_eq!(pill.accessible_value().unwrap_or_default(), "on");
        assert_eq!(pill.accessible_description().unwrap_or_default(), "Room screen is showing");
    }

    #[test]
    fn stage_pill_hidden_unless_configured_or_connected() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        assert!(find(&op, "TopBar::status-stage").is_none());
        app.data.lock().unwrap().settings.screens.assign("stage", crate::model::ScreenTarget::Monitor(r"\\.\DISPLAY2".into()));
        refresh(&op);
        let pill = find(&op, "TopBar::status-stage").expect("stage pill once configured");
        assert_eq!(pill.accessible_value().unwrap_or_default(), "off");
        assert_eq!(pill.accessible_description().unwrap_or_default(), "Stage display");
        let _stage = app.screen_guard("stage");
        refresh(&op);
        assert_eq!(find(&op, "TopBar::status-stage").unwrap().accessible_value().unwrap_or_default(), "on");
    }

    #[test]
    fn phones_pill_label() {
        let app = test_app();
        let op = open(&app);
        let label = |n: usize| {
            app.remote_conns.store(n, Ordering::Relaxed);
            refresh(&op);
            find(&op, "TopBar::status-phones").map(|p| p.accessible_label().unwrap_or_default().to_string())
        };
        assert_eq!(label(1).as_deref(), Some("1 phone"));
        assert_eq!(label(2).as_deref(), Some("2 phones"));
        assert_eq!(label(0), None);
    }

    #[test]
    fn narrow_hides_brand_word() {
        let app = test_app();
        let op = open(&app);
        // `narrow` follows the width from a change handler, which runs at the top of each event loop turn.
        let resize = |w: f32| {
            op.window().set_size(slint::LogicalSize::new(w, 800.0));
            slint::platform::update_timers_and_animations();
        };
        resize(1400.0);
        assert!(!op.ui.get_narrow());
        assert!(find(&op, "TopBar::brand-word").is_some());
        resize(1200.0);
        assert!(op.ui.get_narrow());
        assert!(find(&op, "TopBar::brand-word").is_none());
    }

    #[test]
    fn narrow_matches_the_first_width() {
        // `changed width` fires only when the width changes, so a window whose first width is already narrow and
        // never changes would keep `narrow` false (the wide top bar). The testing backend's window starts 800 px
        // wide, and nothing resizes it before `show`.
        ui_test_init();
        let ui = OperatorWindow::new().unwrap();
        let width = ui.window().size().to_logical(ui.window().scale_factor()).width;
        assert!(width <= 1250.0, "the test needs a narrow first width, got {width}");
        slint::platform::update_timers_and_animations();
        assert!(ui.get_narrow());
    }

    #[test]
    fn first_start_shows_welcome_and_no_dialog() {
        // Empty data: the Welcome panel is the guide; no dialog opens until the operator asks for one.
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        op.apply_live(crate::app::LiveUpdate::new(app.live_view()));
        slint::platform::update_timers_and_animations();
        assert_eq!(op.dialog(), None);
        assert!(!op.ui.get_dialog_open());
        assert!(find(&op, "DialogShell::card").is_none());
        assert!(find(&op, "SlidesPanel::welcome").is_some());
    }

    #[test]
    fn before_first_snapshot_shows_no_welcome() {
        let app = test_app();
        let op = open(&app);
        assert!(find(&op, "SlidesPanel::welcome").is_none());
        refresh(&op);
        assert!(find(&op, "SlidesPanel::welcome").is_some(), "an empty service shows the Welcome panel");
        add_video(&app);
        refresh(&op);
        assert!(find(&op, "SlidesPanel::welcome").is_none());
    }

    #[test]
    fn translation_chips_start_with_the_chosen_then_the_ones_that_work() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        let chips: Vec<crate::ui::TranslationChip> = slint::Model::iter(&op.ui.get_translations()).collect();
        let names: Vec<&str> = chips.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["KJV", "WEB", "BSB"], "a new install: the chosen one, then the Bibles that work now");
        assert!(chips[0].on && !chips[0].off);
        assert_eq!(chips.iter().filter(|c| c.on).count(), 1);
        assert_eq!(slint::Model::row_count(&op.ui.get_quick_chips()), 2, "the chips after the chosen one");
        assert_eq!(op.ui.get_translation(), "KJV");
    }

    #[test]
    fn follower_read_says_player_missing_on_video_row() {
        let app = test_app();
        add_video(&app);
        let feed = Feed::new(app.clone(), true);
        feed.read_all();
        let Pending { snap, live, .. } = feed.take();
        assert!(live.is_some(), "a full read carries the live view too");
        let op = open(&app);
        op.apply_snapshot(snap.unwrap());
        let st = op.state.borrow();
        assert_eq!(st.snap.as_ref().unwrap().items[0].subtitle, "Video · The video player is missing — reinstall RustyPresenter");
    }

    #[test]
    fn follower_patches_job_progress() {
        let app = test_app();
        let job = Uuid::new_v4();
        app.jobs.lock().unwrap().push(crate::app::Job { id: job, label: "Importing slides".into(), ..Default::default() });
        let feed = Feed::new(app.clone(), false);
        feed.read_all();
        feed.take();
        feed.handle(vec![Some(Update::Job { id: job, percent: 55 })]);
        let s = feed.take().snap.expect("patched snapshot");
        assert_eq!(s.jobs[0].percent, 55);
        // A `Data` in the batch means one full read instead, which shows the core's own state.
        feed.handle(vec![Some(Update::Job { id: job, percent: 60 }), Some(Update::Data)]);
        assert_eq!(feed.take().snap.unwrap().jobs[0].percent, 0);
    }

    #[test]
    fn older_live_view_does_not_replace_newer() {
        let app = test_app();
        let feed = Feed::new(app.clone(), false);
        let mut newer = app.live_view();
        newer.version = 5;
        let mut older = newer.clone();
        older.version = 4;
        feed.post(None, Some(LiveUpdate::new(newer.clone())));
        feed.post(None, Some(LiveUpdate::new(older.clone())));
        assert_eq!(feed.take().live.unwrap().view().version, 5);
        let op = open(&app);
        op.apply_live(LiveUpdate::new(newer));
        op.apply_live(LiveUpdate::new(older));
        assert_eq!(op.state.borrow().live.as_ref().unwrap().view().version, 5);
    }
}
