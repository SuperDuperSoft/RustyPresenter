//! The operator window: opening it, the top bar (`operator.js:93-107`) and the follower that turns core updates
//! into snapshots for the Slint thread (`operator.js:292-302`). The columns, keys and dialogs come in later tasks.
use super::{strings as tr, view::{self, OperatorSnapshot, RoomStatus}};
use crate::{
    app::{App, LiveUpdate, Update},
    model::MediaStatus,
    ui::{OperatorWindow, TranslationChip, ops::Ops},
    video::DrawMode,
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, rc::{Rc, Weak}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};
use tokio::sync::{Notify, broadcast::{Receiver, error::{RecvError, TryRecvError}}};
use uuid::Uuid;

/// How video is drawn and which libmpv loaded, for the missing-player note and Settings → About.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoInfo { pub draw_mode: DrawMode, pub mpv: Option<MpvInfo> }

/// The libmpv that loaded: its client API version and the file it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct MpvInfo { pub api: (u32, u32), pub path: String }

impl VideoInfo {
    /// No libmpv: video and audio items say "The video player is missing" (native spec §5).
    pub fn player_missing(&self) -> bool { self.mpv.is_none() }
}

/// What the operator needs from the windowing layer. `NativeScreens` implements it; tests pass a fake, so no
/// room window or mpv is needed. (The live preview's frame grabber joins it with Task 8.)
pub trait OperatorScreens {
    fn video_info(&self) -> VideoInfo;
}

/// `NativeScreens` lives in an `Arc`: `Rc::new(screens.clone())` is then an `Rc<dyn OperatorScreens>`.
impl<T: OperatorScreens + ?Sized> OperatorScreens for Arc<T> {
    fn video_info(&self) -> VideoInfo { (**self).video_info() }
}

/// The dialogs of the web operator (`operator.js:353-1000`); one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind { Add, ItemEdit, Services, Screens, Phone, Settings }

/// The window's state between updates (the web's `S`, `L`, `selectedId` and `bibleArmed`, `operator.js:62-68`).
#[derive(Default)]
#[allow(dead_code)] // `armed`, `dialog`, `asks` and `dragging` are used by the columns, keys and dialogs (Tasks 6–13).
struct OpState {
    snap: Option<Arc<OperatorSnapshot>>,
    live: Option<LiveUpdate>,
    selected: Option<Uuid>,
    armed: Option<Uuid>,
    dialog: Option<DialogKind>,
    asks: usize,
    dragging: bool,
    held_snapshot: Option<Arc<OperatorSnapshot>>,
}

pub struct Operator {
    ui: OperatorWindow,
    app: Arc<App>,
    ops: Ops,
    screens: Rc<dyn OperatorScreens>,
    state: RefCell<OpState>,
    /// The follower's mailbox; `None` on the testing backend, which has no event loop to deliver into.
    feed: Option<Arc<Feed>>,
}

impl Operator {
    /// Opens the window. The first state read happens on the follower thread (the web `onopen`); until it arrives
    /// the columns are empty, the service is called "Service" and there is no Welcome panel.
    pub fn open(app: Arc<App>, ops: Ops, screens: Rc<dyn OperatorScreens>) -> Result<Rc<Operator>, String> {
        let ui = OperatorWindow::new().map_err(tr::open_failed)?;
        let feed = Feed::new(app.clone(), screens.video_info().player_missing());
        // Subscribe before the first read so no update falls in between.
        let rx = app.subscribe();
        let op = Rc::new_cyclic(|me: &Weak<Operator>| {
            let listening = listen(me.clone(), feed.clone());
            Operator { ui, app, ops, screens, state: RefCell::default(), feed: listening.then(|| feed.clone()) }
        });
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
        let _ = self.ui.show();
        self.ui.window().set_minimized(false);
        use slint::winit_030::WinitWindowAccessor;
        self.ui.window().with_winit_window(|w| w.focus_window());
    }

    /// Asks the follower for a fresh full read (monitors included), e.g. when the Screens dialog opens.
    pub fn request_refresh(&self) {
        let Some(feed) = self.feed.clone() else { return };
        let _ = std::thread::Builder::new().name("operator-refresh".into()).spawn(move || feed.read_all());
    }

    /// A new state read (`refresh()` in `operator.js:69-74`). Held while a service row is being dragged (Task 6).
    pub fn apply_snapshot(&self, s: Arc<OperatorSnapshot>) {
        let mut st = self.state.borrow_mut();
        if st.dragging { st.held_snapshot = Some(s); return; }
        let live_item = st.live.as_ref().and_then(|l| l.view().cursor.as_ref().map(|c| c.item_id));
        st.selected = view::reselect(st.selected, &s.items, live_item);
        st.snap = Some(s.clone());
        drop(st);
        self.render_top(&s);
        self.ui.set_item_count(s.items.len() as i32);
        self.ui.set_loaded(true);
    }

    /// A new live view. An older one (a full read that raced a newer `Live`) is ignored.
    pub fn apply_live(&self, v: LiveUpdate) {
        let mut st = self.state.borrow_mut();
        if st.live.as_ref().is_some_and(|l| l.view().version > v.view().version) { return; }
        st.live = Some(v);
    }

    /// `renderTop()` (`operator.js:93-107`).
    fn render_top(&self, s: &OperatorSnapshot) {
        let ui = &self.ui;
        ui.set_service_name(s.service_name.as_str().into());
        let chips: Vec<TranslationChip> = s.translations.iter().map(|t| TranslationChip {
            name: t.as_str().into(), on: *t == s.translation, off: !s.available.contains(t),
        }).collect();
        ui.set_translations(ModelRc::new(VecModel::from(chips)));
        let (on, tip) = match s.room {
            RoomStatus::Showing => (true, tr::ROOM_SHOWING),
            RoomStatus::Starting => (false, tr::ROOM_STARTING),
            RoomStatus::Off => (false, tr::ROOM_OFF),
        };
        ui.set_room_on(on);
        ui.set_room_state(if on { "on" } else { "warn" }.into());
        ui.set_room_tip(tip.into());
        let stage = view::stage_pill(s);
        ui.set_stage_shown(stage.is_some());
        ui.set_stage_on(stage.unwrap_or(false));
        ui.set_phones_label(view::phones_label(s.phones).unwrap_or_default().into());
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
struct Pending { snap: Option<Arc<OperatorSnapshot>>, live: Option<LiveUpdate> }

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
                Some(Update::Job { id, percent }) => self.patch(|s| match s.jobs.iter_mut().find(|j| j.id == id) {
                    Some(j) => { j.percent = percent; true }
                    None => false,
                }),
                Some(Update::Convert { id, percent }) => self.patch(|s| {
                    let Some(m) = Arc::make_mut(&mut s.data).media.iter_mut().find(|m| m.id == id) else { return false };
                    m.status = MediaStatus::Converting { percent };
                    s.refresh_subtitles();
                    true
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
            let Pending { snap, live } = feed.take();
            // Live first, so the selection can fall back to the live item (`operator.js:73`).
            if let Some(v) = live { op.apply_live(v); }
            if let Some(s) = snap { op.apply_snapshot(s); }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::test_app,
        model::{EndAction, ItemKind, MediaFile, MediaKind, ServiceItem},
        ui::ui_test_init,
    };
    use i_slint_backend_testing::ElementHandle;

    struct FakeScreens { missing: bool }

    impl OperatorScreens for FakeScreens {
        fn video_info(&self) -> VideoInfo {
            VideoInfo { draw_mode: DrawMode::Software, mpv: (!self.missing).then(|| MpvInfo { api: (2, 3), path: "libmpv-2.dll".into() }) }
        }
    }

    fn open(app: &Arc<App>) -> Rc<Operator> {
        ui_test_init();
        Operator::open(app.clone(), Ops::inline(app.clone()), Rc::new(FakeScreens { missing: false })).unwrap()
    }

    /// The follower's read, done here because the testing backend has no event loop to deliver it.
    fn refresh(op: &Operator) { op.apply_snapshot(Arc::new(OperatorSnapshot::read(&op.app))); }

    fn find(op: &Operator, id: &str) -> Option<ElementHandle> { ElementHandle::find_by_element_id(&op.ui, id).next() }

    /// A video item in the current service; returns its media id.
    fn add_video(app: &App) -> Uuid {
        let media = MediaFile { id: Uuid::new_v4(), name: "clip.mp4".into(), kind: MediaKind::Video, asset: "clip.mp4".into(), status: MediaStatus::Ready, original_asset: None };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id: media.id, autoplay: true, end: EndAction::Stop, looped: false }, background: None, notes: String::new() };
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
        app.data.lock().unwrap().settings.displays.stage = Some(r"\\.\DISPLAY2".into());
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
    fn before_first_snapshot_shows_no_welcome() {
        let app = test_app();
        let op = open(&app);
        assert!(find(&op, "OperatorWindow::welcome").is_none());
        refresh(&op);
        assert!(find(&op, "OperatorWindow::welcome").is_some(), "an empty service shows the Welcome panel");
        add_video(&app);
        refresh(&op);
        assert!(find(&op, "OperatorWindow::welcome").is_none());
    }

    #[test]
    fn translation_chips_mark_the_chosen_and_unconnected() {
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        let chips: Vec<TranslationChip> = slint::Model::iter(&op.ui.get_translations()).collect();
        let snap = op.state.borrow().snap.clone().unwrap();
        assert_eq!(chips.len(), snap.translations.len());
        assert!(chips.iter().any(|c| c.name == "KJV" && c.on && !c.off));
        assert_eq!(chips.iter().filter(|c| c.on).count(), 1);
        assert!(chips.iter().any(|c| c.off), "a translation that needs a key is shown faded");
    }

    #[test]
    fn follower_read_says_player_missing_on_video_row() {
        let app = test_app();
        add_video(&app);
        let feed = Feed::new(app.clone(), true);
        feed.read_all();
        let Pending { snap, live } = feed.take();
        assert!(live.is_some(), "a full read carries the live view too");
        let op = open(&app);
        op.apply_snapshot(snap.unwrap());
        let st = op.state.borrow();
        assert_eq!(st.snap.as_ref().unwrap().items[0].subtitle, "Video · The video player is missing — reinstall RustyPresenter");
    }

    #[test]
    fn follower_patches_conversion_and_job_progress() {
        let app = test_app();
        let media = add_video(&app);
        let job = Uuid::new_v4();
        app.jobs.lock().unwrap().push(crate::app::Job { id: job, label: "Importing slides".into(), percent: 0, error: None });
        let feed = Feed::new(app.clone(), false);
        feed.read_all();
        feed.take();
        feed.handle(vec![Some(Update::Convert { id: media, percent: 40 }), Some(Update::Job { id: job, percent: 55 })]);
        let s = feed.take().snap.expect("patched snapshot");
        assert_eq!(s.items[0].subtitle, "Video · converting 40%");
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
