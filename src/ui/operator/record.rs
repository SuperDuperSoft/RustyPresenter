//! N22: the Record button in the top bar and File → Start/Stop recording. Starting finds FFmpeg (offering to get
//! it when there is none), starts `record::Recording` and connects the room window to it; the button then shows a
//! red dot and the time. Stopping saves the MP4 in the background and says where it is.
use super::window::Operator;
use crate::{
    platform::tools,
    record::{self, Ended, Plan, Recording, run},
    stream,
    ui::dialogs::confirm,
    youtube::fetch::Tool,
};
use slint::{Timer, TimerMode};
use std::{cell::{Cell, RefCell}, path::PathBuf, rc::Rc, time::Duration};
use tokio_util::sync::CancellationToken;

/// The recording, while there is one, and what the button shows.
#[derive(Default)]
pub struct RecordUi {
    rec: RefCell<Option<Recording>>,
    /// Looking for FFmpeg or starting; a second click waits.
    starting: Cell<bool>,
    /// The recording under way is also a livestream.
    live: Cell<bool>,
    /// Go Live: each destination's name and whether it was connected at the last look, in the order of
    /// `Plan::stream`. The ticker says when one drops or comes back.
    links: RefCell<Vec<(String, bool)>>,
    /// Updates the time on the button once a second.
    ticker: Timer,
    /// Tests: how the recording ended, waited for by `testing::wait_ended` (no event loop to deliver it).
    #[cfg(test)]
    ended: RefCell<Option<tokio::sync::oneshot::Receiver<Ended>>>,
}

/// Runs `work` off the UI thread and `done` with its answer back on it (at once in tests, which have no event loop).
fn off_ui<T: Send + 'static>(op: &Operator, work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static) {
    if op.ops().is_inline() { return done(work()) }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawned = std::thread::Builder::new().name("record-start".into()).spawn(move || { let _ = tx.send(work()); });
    if spawned.is_err() { return }
    if let Err(e) = slint::spawn_local(async move { if let Ok(v) = rx.await { done(v) } }) { eprintln!("recording: {e}") }
}

impl Operator {
    /// The Record button and File → Start/Stop recording.
    pub fn toggle_recording(self: &Rc<Self>) { self.record_or_stop(false) }

    /// The Go Live button: a recording that is also sent to the destinations that are on; again stops both.
    pub fn go_live(self: &Rc<Self>) {
        if self.is_recording() && !self.record.live.get() { return self.toast(stream::tr::stop_recording_first(), true) }
        self.record_or_stop(true)
    }

    fn record_or_stop(self: &Rc<Self>, live: bool) {
        if self.record.starting.get() { return }
        if let Some(rec) = self.record.rec.borrow().as_ref() {
            rec.stop();
            self.show_recording(false);
            self.toast(record::tr::finishing(), false);
            return;
        }
        if !self.screens().record_to(None) { return self.toast(record::tr::no_room(), true) }
        let (prefs, app_output) = self.state.borrow().snap.as_ref().map(|s| (s.settings.record.clone(), s.settings.audio_device.clone())).unwrap_or_default();
        let data = self.app().store.root.clone();
        let me = Rc::downgrade(self);
        // Go Live: the destinations that are on, with their keys from the computer's store. None ready: no
        // recording starts either (the operator asked for a stream).
        let mut names = String::new();
        let mut addresses = vec![];
        if live {
            let mut all = prefs.stream.clone();
            stream::keys::fill(stream::keys::app_store().as_ref(), &mut all);
            let (good, bad) = stream::ready(&all);
            for (name, why) in &bad { self.toast(stream::tr::not_ready(name, &why.words()), true) }
            if good.is_empty() { return self.toast(stream::tr::nothing_ready(), true) }
            let titles: Vec<String> = all.iter().filter(|d| d.on && d.address().is_ok()).map(|d| d.title().to_string()).collect();
            names = titles.join(", ");
            *self.record.links.borrow_mut() = titles.into_iter().map(|t| (t, false)).collect();
            addresses = good;
        }
        self.record.starting.set(true);
        let prefs_pc = prefs.pc_sound(&app_output);
        off_ui(self, move || Plan::make(&data, &prefs), move |plan| {
            let Some(op) = me.upgrade() else { return };
            op.record.starting.set(false);
            match plan {
                Ok(mut plan) => {
                    if live { plan.pc_sound = prefs_pc }
                    plan.stream = addresses;
                    op.record.live.set(live);
                    op.start_recording(plan);
                    if live && op.is_recording() {
                        op.ui.set_live(true);
                        op.toast(stream::tr::live_on(&names), false);
                    }
                }
                Err(Some(why)) => op.toast(why, true),
                Err(None) => op.offer_ffmpeg(),
            }
        });
    }

    fn start_recording(self: &Rc<Self>, plan: Plan) {
        let (ffmpeg, folder) = (plan.ffmpeg.clone(), plan.out.parent().map(PathBuf::from).unwrap_or_default());
        let (tx, rx) = tokio::sync::oneshot::channel::<Ended>();
        let rec = match Recording::start(plan, move |e| { let _ = tx.send(e); }) {
            Ok(r) => r,
            Err(e) => return self.toast(e, true),
        };
        if !self.screens().record_to(Some(rec.sink().clone())) {
            drop(rec);
            return self.toast(record::tr::no_room(), true);
        }
        let current = run::partial_of(&rec.out);
        *self.record.rec.borrow_mut() = Some(rec);
        self.show_recording(true);
        self.toast(record::tr::recording(), false);
        #[cfg(test)]
        if self.ops().is_inline() { *self.record.ended.borrow_mut() = Some(rx) }
        #[cfg(not(test))]
        {
            let me = Rc::downgrade(self);
            let waiting = async move {
                let Ok(ended) = rx.await else { return };
                if let Some(op) = me.upgrade() { op.recording_ended(ended) }
            };
            if let Err(e) = slint::spawn_local(waiting) { eprintln!("recording: {e}") }
        }
        // A part an earlier recording left (the app or the computer stopped) is finished now.
        let me = Rc::downgrade(self);
        off_ui(self, move || run::finish_leftovers(&ffmpeg, &folder, &current), move |done| {
            let Some(op) = me.upgrade() else { return };
            for e in done { let (text, err) = e.toast(); op.toast(text, err) }
        });
    }

    fn recording_ended(&self, ended: Ended) {
        self.screens().record_to(None);
        // The writer has finished; dropping it does not wait.
        drop(self.record.rec.borrow_mut().take());
        self.show_recording(false);
        let (text, err) = ended.toast();
        self.toast(text, err);
    }

    /// Go Live: says which destination has just connected, and which has lost its connection (it is tried
    /// again by itself: `stream::send`).
    pub(crate) fn say_links(&self, now: &[bool]) {
        if !self.record.live.get() { return }
        let mut said = vec![];
        for ((name, was), up) in self.record.links.borrow_mut().iter_mut().zip(now) {
            if *was != *up {
                *was = *up;
                said.push(if *up { (stream::tr::connected(name), false) } else { (stream::tr::lost(name), true) });
            }
        }
        for (text, err) in said { self.toast(text, err) }
        // The Live button: how many are connected now, of how many.
        self.ui.set_live_total(now.len() as i32);
        self.ui.set_live_up(now.iter().filter(|u| **u).count() as i32);
    }

    /// Whether a recording is under way.
    pub fn is_recording(&self) -> bool { self.record.rec.borrow().is_some() }

    fn show_recording(&self, on: bool) {
        self.ui.set_recording(on);
        if !on {
            self.record.live.set(false);
            self.ui.set_live(false);
        }
        self.ui.set_record_time(record::clock(0).into());
        if !on { return self.record.ticker.stop() }
        let me = self.me.clone();
        self.record.ticker.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(op) = me.upgrade() else { return };
            let secs = op.record.rec.borrow().as_ref().map(Recording::secs);
            if let Some(s) = secs { op.ui.set_record_time(record::clock(s).into()) }
            let links = op.record.rec.borrow().as_ref().map(Recording::links).unwrap_or_default();
            op.say_links(&links);
        });
    }

    /// No FFmpeg: says how to get it, and gets it on Get FFmpeg (the same download YouTube videos use).
    fn offer_ffmpeg(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        let text = record::tr::no_ffmpeg(&tools::ffmpeg_hint(tools::OS));
        confirm(self, &text, &tr::get_ffmpeg(), &tr::not_now(), false, move |yes| {
            let Some(op) = me.upgrade().filter(|_| yes) else { return };
            let job = op.client_job(tr::getting_ffmpeg());
            let progress = job.progress();
            let app = op.app().clone();
            let me = Rc::downgrade(&op);
            off_ui(&op, move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
                rt.block_on(app.youtube.fetch.ensure(Tool::Ffmpeg, false, &progress, &CancellationToken::new())).map(|_| ()).map_err(|e| format!("{e:?}"))
            }, move |r| {
                drop(job);
                let Some(op) = me.upgrade() else { return };
                match r {
                    Ok(()) => op.toast(tr::ffmpeg_ready(), false),
                    Err(e) => {
                        eprintln!("FFmpeg could not be fetched for recording: {e}");
                        op.toast(tr::ffmpeg_failed(tools::ffmpeg_install_command(tools::OS)), true)
                    }
                }
            });
        });
    }
}

pub(super) fn wire(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ui.on_record(move || if let Some(op) = me.upgrade() { op.toggle_recording() });
    let me = Rc::downgrade(op);
    op.ui.on_go_live(move || if let Some(op) = me.upgrade() { op.go_live() });
}

mod tr {
    use crate::tr;
    pub fn get_ffmpeg() -> String { tr!("Get FFmpeg") }
    pub fn not_now() -> String { tr!("Not now") }
    pub fn getting_ffmpeg() -> String { tr!("Getting FFmpeg for recording") }
    pub fn ffmpeg_ready() -> String { tr!("FFmpeg is ready. Press Record to start recording.") }
    pub fn ffmpeg_failed(command: &str) -> String { tr!("FFmpeg could not be downloaded. Check the internet connection, or install it yourself: {command}", command = command) }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Waits for the recording to be saved and lets the window hear it, as the event loop would.
    pub fn wait_ended(op: &Operator) -> Ended {
        let rx = op.record.ended.borrow_mut().take().expect("a recording was started");
        let ended = rx.blocking_recv().expect("the writer reports");
        op.recording_ended(ended.clone());
        ended
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
