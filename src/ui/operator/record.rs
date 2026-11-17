//! N22: the Record button in the top bar and File → Start/Stop recording. Starting finds FFmpeg (offering to get
//! it when there is none), starts `record::Recording` and connects the room window to it; the button then shows a
//! red dot and the time. Stopping saves the MP4 in the background and says where it is.
use super::window::Operator;
use crate::{
    platform::tools,
    record::{self, Ended, Plan, Recording, run},
    stream::{self, scene::Scene},
    ui::dialogs::confirm,
    youtube::fetch::Tool,
};
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::{cell::{Cell, RefCell}, path::PathBuf, rc::Rc, sync::Arc, time::Duration};
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
    /// Signed in, while live: the viewers' chat, and how many messages the operator has seen.
    chat: stream::chat::Board,
    chat_seen: Cell<u64>,
    /// Stops the chat readers when the stream stops.
    chat_stop: RefCell<Arc<std::sync::atomic::AtomicBool>>,
    /// Go Live: each destination's drops so far, whether it ever connected (the summary at the end), and the
    /// lighter stream it was last said to get (0: the full one).
    history: RefCell<Vec<(u32, bool, u32)>>,
    /// The destinations the operator was told the internet is too slow for (once each).
    slow_told: RefCell<Vec<String>>,
    /// The destinations the operator was told have not connected after `NEVER_TRIES` (once each).
    never_told: RefCell<Vec<String>>,
    /// The camera in the stream: whether its pictures came at the last look (`None`: not yet seen at all).
    camera_was: Cell<Option<bool>>,
    /// Seconds the camera has been waited for since Go Live, before its first picture.
    camera_wait: Cell<u32>,
    /// The sound input being recorded (empty: none), and whether it was silent at the last look.
    input: RefCell<String>,
    silent_was: Cell<bool>,
    /// The clips marked in the recording under way (`record::clips`).
    marks: RefCell<record::clips::Marks>,
    /// Whether the computer keeps up with the recording, from the writer's lag each second.
    slow: RefCell<record::slow::Slow>,
    /// Updates the time on the button once a second.
    ticker: Timer,
    /// Go Live: the cameras' names, in `Plan::cameras` order.
    cameras: RefCell<Vec<String>>,
    /// Go Live: each camera's PTZ control (`stream::ptz`), in the same order; `None`: it cannot be moved.
    camera_controls: RefCell<Vec<Option<stream::ptz::Control>>>,
    /// While live: the scene button, four times a second.
    scene_ticker: Timer,
    /// Go Live with two cameras or more: the camera chosen to go out next (`Plan::cameras` order).
    next: Cell<Option<usize>>,
    /// The small Live and Next pictures shown now, so the same ones are not made into images again.
    peek_shown: RefCell<record::peek::Peek>,
    /// Go Live, signed in: the broadcasts made for it, with their destinations' names (ended, and their replay
    /// links said, when the stream stops).
    broadcasts: RefCell<Vec<(String, stream::accounts::Live)>>,
    /// Seconds since the viewers were last asked about.
    viewers_age: Cell<u32>,
    /// Go Live: the church's own scenes, listed after the built-in ones on the scene button.
    own_scenes: RefCell<Vec<stream::own_scenes::OwnScene>>,
    /// The video encoder of the recording under way, and whether Settings left the choice to the app.
    encoder: RefCell<String>,
    encoder_auto: Cell<bool>,
    /// Seconds the computer was behind while live, and the most viewers at once (the report after a stream).
    slow_secs: Cell<u32>,
    viewers_most: Cell<i32>,
    /// More settings: the minutes "Save last" keeps (0: no button).
    replay_minutes: Cell<u32>,
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

/// How often (seconds) a signed-in stream's viewers are asked about.
/// Go Live: tries in a row without connecting before the operator is told a destination does not connect.
pub(crate) const NEVER_TRIES: u32 = 3;

/// Go Live with a camera: seconds it may take to send its first picture before the operator is told it does not.
pub(crate) const CAMERA_START_SECS: u32 = 15;

const VIEWERS_EVERY: u32 = 30;

/// Signed in: a broadcast made at each service for Go Live, by the destination's name.
fn make_broadcasts(ids: &stream::accounts::Ids, ends: &stream::accounts::Ends, signed: Vec<(stream::Destination, String, stream::accounts::Details)>)
    -> Vec<(String, Result<stream::accounts::Live, String>)> {
    if signed.is_empty() { return vec![] }
    let fail = |e: String| signed.iter().map(|(d, _, _)| (d.title().to_string(), Err(e.clone()))).collect::<Vec<_>>();
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() { Ok(r) => r, Err(e) => return fail(e.to_string()) };
    let client = match crate::bible::sources::client() { Ok(c) => c, Err(e) => return fail(format!("{e:?}")) };
    rt.block_on(async {
        let mut out = vec![];
        let store = stream::keys::app_store();
        for (d, token, details) in &signed {
            let made = stream::accounts::prepare(client, ends, ids, d, token, details).await;
            // A sign-in that works once (Twitch's) is kept again, renewed, for next time.
            if let Ok(Some(renewed)) = made.as_ref().map(|l| l.renewed.clone()) {
                let name = stream::accounts::kept(store.as_ref(), d.id).map(|(n, _)| n).unwrap_or_default();
                if let Err(e) = stream::accounts::keep(store.as_ref(), d.id, &name, &renewed) { eprintln!("stream: {}: the sign-in could not be kept: {e}", d.title()) }
            }
            out.push((d.title().to_string(), made));
        }
        out
    })
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
            self.end_broadcasts(true);
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
        let mut titles = vec![];
        let mut addresses = vec![];
        // Which of `addresses` get the tall picture, and each one's own quality, in the same order.
        let mut talls: Vec<bool> = vec![];
        let mut qualities: Vec<stream::Quality> = vec![];
        // The services the stream goes to (HEVC only when all of them take it).
        let mut services: Vec<stream::Service> = vec![];
        // Signed in: the broadcast is made, and its address got, as the recording is got ready.
        let mut signed: Vec<(stream::Destination, String)> = vec![];
        // NDI out alone is enough to go live (a church streaming from OBS takes the app's picture over NDI).
        let ndi_only = live && prefs.ndi().any() && stream::ndi::runtime().is_some();
        if live {
            let store = stream::keys::app_store();
            let mut all = prefs.stream.clone();
            stream::keys::fill(store.as_ref(), &mut all);
            let (with_sign_in, keyed): (Vec<_>, Vec<_>) = all.into_iter().partition(|d| d.on && stream::accounts::kept(store.as_ref(), d.id).is_some());
            signed = with_sign_in.into_iter().filter_map(|d| stream::accounts::kept(store.as_ref(), d.id).map(|(_, token)| (d, token))).collect();
            let (good, bad) = stream::ready(&keyed);
            for (name, why) in &bad { self.toast(stream::tr::not_ready(name, &why.words()), true) }
            if good.is_empty() && signed.is_empty() && !ndi_only { return self.toast(stream::tr::nothing_ready(), true) }
            titles = keyed.iter().filter(|d| d.on && d.address().is_ok()).map(|d| d.title().to_string()).collect();
            talls = keyed.iter().filter(|d| d.on && d.address().is_ok()).map(|d| d.tall).collect();
            qualities = keyed.iter().filter(|d| d.on && d.address().is_ok()).map(|d| d.quality).collect();
            services = keyed.iter().filter(|d| d.on && d.address().is_ok()).map(|d| d.service).chain(signed.iter().map(|(d, _)| d.service)).collect();
            self.record.slow_told.borrow_mut().clear();
            self.record.never_told.borrow_mut().clear();
            self.record.camera_was.set(None);
            self.record.camera_wait.set(0);
            addresses = good;
        }
        let today = chrono::Local::now().date_naive();
        let details = |service| stream::accounts::Details {
            title: stream::accounts::title_for(&prefs.stream_title, today, service),
            description: prefs.stream_description.clone(),
            privacy: prefs.stream_privacy,
            thumbnail: prefs.stream_thumbnail.clone(),
            scheduled: if service == stream::Service::YouTube { prefs.stream_scheduled.clone() } else { String::new() },
        };
        let signed_tall: Vec<(String, bool, stream::Quality)> = signed.iter().map(|(d, _)| (d.title().to_string(), d.tall, d.quality)).collect();
        let signed: Vec<(stream::Destination, String, stream::accounts::Details)> = signed.into_iter().map(|(d, t)| { let x = details(d.service); (d, t, x) }).collect();
        self.record.starting.set(true);
        let prefs_pc = prefs.pc_sound(&app_output);
        let sound_fx = prefs.sound_fx();
        let delay_secs = stream::delay::checked(prefs.stream_delay_secs);
        let lighter = prefs.stream_lighter.unwrap_or(true);
        let ndi = prefs.ndi();
        // The cameras (up to four, in Settings' order) and their names for the Cameras box and the scene button.
        let chosen = prefs.cameras();
        let camera_names: Vec<String> = chosen.iter().map(|(c, _)| c.clone()).collect();
        let cameras: Vec<Vec<String>> = camera_names.iter().map(|c| record::args::camera_input(crate::platform::tools::OS, c)).collect();
        if live {
            *self.record.cameras.borrow_mut() = camera_names;
            *self.record.camera_controls.borrow_mut() = chosen.iter().map(|(cam, ctl)| stream::ptz::Control::of(cam, ctl)).collect();
        }
        // The scenes: what is live now (read ten times a second by `stream::pictures`), the words over the camera,
        // and the church's cards and choices. A new stream starts on Automatic.
        let canvas = prefs.size.size();
        let (app, app2) = (self.app().clone(), self.app().clone());
        if live { self.app().clear_stream_choice() }
        let theme = self.app().data.lock().unwrap().settings.theme.clone();
        let words_top = prefs.stream_layout.words_top;
        let look = stream::pictures::Look {
            info: Arc::new(move || app.stream_info()),
            words: Arc::new(move || {
                let live = app2.live.lock().unwrap();
                let theme = live.item.as_ref().map(|i| i.theme.clone()).unwrap_or_default();
                stream::lyrics::key_for_at(live.frame.slide.as_ref()?, &theme, canvas, words_top)
            }),
            auto: prefs.auto(),
            cards: prefs.cards.clone(),
            theme,
            layout: prefs.stream_layout,
            own: prefs.own_scenes.clone(),
        };
        if live { *self.record.own_scenes.borrow_mut() = prefs.own_scenes.clone() }
        let (ids, ends) = (stream::accounts::app_ids(), stream::accounts::app_ends());
        let scheduled_id = prefs.stream_scheduled.clone();
        // The file's `{title}`: the service's name.
        let title = self.app().data.lock().unwrap().service().map(|s| s.name.clone()).unwrap_or_default();
        let making = record::run::Making { title, services, live };
        // Go Live: are the cameras and the sound input there and sending? Looked at while the rest is got ready.
        let wanted = live.then(|| stream::preflight::Wanted {
            cameras: prefs.cameras().into_iter().map(|(c, _)| c).collect(),
            sound: match prefs.sound() { record::Sound::Input(name) => Some(name), _ => None },
        });
        let auto_encoder = prefs.encoder.is_empty();
        let replay = prefs.more.keep.replay_minutes;
        off_ui(self, move || {
            let missing = std::thread::scope(|s| {
                let check = wanted.as_ref().map(|w| s.spawn(|| run::find_ffmpeg(&data).map(|ff| stream::preflight::check(&ff, w)).unwrap_or_default()));
                let made = (Plan::make_with(&data, &prefs, &making), make_broadcasts(&ids, &ends, signed));
                (made, check.map(|c| c.join().unwrap_or_default()).unwrap_or_default())
            });
            (missing.0.0, missing.0.1, missing.1)
        }, move |(plan, made, missing)| {
            let Some(op) = me.upgrade() else { return };
            op.record.starting.set(false);
            let mut broadcasts = vec![];
            for (title, r) in made {
                match r {
                    Ok(b) => {
                        addresses.push(b.address.clone());
                        talls.push(signed_tall.iter().any(|(t, tall, _)| *t == title && *tall));
                        qualities.push(signed_tall.iter().find(|(t, _, _)| *t == title).map(|(_, _, q)| *q).unwrap_or_default());
                        titles.push(title.clone());
                        broadcasts.push((title, b))
                    }
                    Err(e) => { eprintln!("stream: {title}: {e}"); op.toast(stream::accounts::tr::prepare_failed(&title, &e), true) }
                }
            }
            let used_ahead = broadcasts.iter().any(|(_, b)| b.service == stream::Service::YouTube && !scheduled_id.is_empty() && b.id == scheduled_id);
            if used_ahead { let _ = op.app().edit(|d| { d.settings.record.stream_scheduled.clear(); d.settings.record.stream_scheduled_link.clear(); Ok(()) }); }
            *op.record.broadcasts.borrow_mut() = broadcasts;
            if live && addresses.is_empty() && !ndi_only && plan.is_ok() { return op.toast(stream::tr::nothing_ready(), true) }
            if live {
                *op.record.links.borrow_mut() = titles.iter().map(|t| (t.clone(), false)).collect();
                op.record.history.borrow_mut().clear();
            }
            let names = titles.join(", ");
            // What is not there or not sending, said once each (the stream still goes out).
            if plan.is_ok() { for m in &missing { op.toast(m.words(), true) } }
            match plan {
                Ok(mut plan) => {
                    // Each destination that differs from the main stream gets its own picture from its sender.
                    let full = if plan.kbps > 0 { plan.kbps } else { plan.preset.max_kbps() };
                    plan.ways = qualities.iter().zip(talls.iter().chain(std::iter::repeat(&false)))
                        .map(|(q, tall)| stream::send::Way { own: q.own(*tall, plan.preset, full, plan.encoder, plan.more.fps()), ..Default::default() }).collect();
                    *op.record.encoder.borrow_mut() = plan.encoder.to_string();
                    op.record.encoder_auto.set(auto_encoder);
                    op.record.replay_minutes.set(replay);
                    if live {
                        plan.pc_sound = prefs_pc;
                        plan.cameras = cameras;
                        plan.look = Some(look);
                        plan.sound_fx = sound_fx;
                        plan.delay_secs = delay_secs;
                        plan.lighter = lighter;
                        plan.ndi = ndi;
                        if ndi.any() && stream::ndi::runtime().is_none() { op.toast(stream::tr::ndi_missing(), true) }
                    }
                    plan.stream = addresses;
                    plan.tall = talls;
                    op.record.live.set(live);
                    op.start_recording(plan);
                    if live && op.is_recording() {
                        op.record.viewers_age.set(0);
                        op.ui.set_live_watching(-1);
                        op.ui.set_live(true);
                        op.toast(if names.is_empty() { stream::tr::live_ndi_only() } else { stream::tr::live_on(&names) }, false);
                        op.start_chat();
                        op.note_stream_licence();
                    }
                }
                Err(Some(why)) => op.toast(why, true),
                Err(None) => op.offer_ffmpeg(),
            }
            // Broadcasts made for a stream that did not start are ended at once, unsaid.
            if !(live && op.is_recording()) { op.end_broadcasts(false) }
        });
    }

    fn start_recording(self: &Rc<Self>, mut plan: Plan) {
        // A chapter at each item taken live, named as the service list names it.
        let app = self.app().clone();
        plan.chapters = Some(record::chapters::Source(Arc::new(move || app.live.lock().ok()?.item.as_ref().map(|i| i.title.clone()))));
        let (ffmpeg, folder) = (plan.ffmpeg.clone(), plan.out.parent().map(PathBuf::from).unwrap_or_default());
        *self.record.input.borrow_mut() = match &plan.sound { record::Sound::Input(name) => name.clone(), _ => String::new() };
        self.record.silent_was.set(false);
        *self.record.slow.borrow_mut() = Default::default();
        self.record.slow_secs.set(0);
        self.record.viewers_most.set(-1);
        *self.record.marks.borrow_mut() = Default::default();
        self.ui.set_clipping(false);
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
        record::unfinished::mark(&self.app().store.root, &record::unfinished::Unfinished { file: current.clone(), live: self.record.live.get() });
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

    /// Signed in: each broadcast is ended at its service (off the UI thread) and, with `say`, its replay link said.
    fn end_broadcasts(&self, say: bool) {
        let all = std::mem::take(&mut *self.record.broadcasts.borrow_mut());
        if all.is_empty() { return }
        if say { for (title, b) in &all { self.toast(stream::accounts::tr::replay(title, &b.watch), false) } }
        if self.ops().is_inline() { return }
        let ends = stream::accounts::app_ends();
        let _ = std::thread::Builder::new().name("stream-end".into()).spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            let Ok(client) = crate::bible::sources::client() else { return };
            rt.block_on(async { for (_, b) in &all { stream::accounts::finish(client, &ends, b).await } });
        });
    }

    /// Signed in: how many are watching, asked every `VIEWERS_EVERY` seconds off the UI thread.
    fn ask_viewers(&self) {
        let age = self.record.viewers_age.get() + 1;
        self.record.viewers_age.set(age);
        if age < VIEWERS_EVERY || self.ops().is_inline() { return }
        self.record.viewers_age.set(0);
        let all: Vec<stream::accounts::Live> = self.record.broadcasts.borrow().iter().map(|(_, b)| b.clone()).collect();
        if all.is_empty() { return }
        let (ui, ends, ids) = (self.ui.as_weak(), stream::accounts::app_ends(), stream::accounts::app_ids());
        let _ = std::thread::Builder::new().name("stream-viewers".into()).spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            let Ok(client) = crate::bible::sources::client() else { return };
            let counts: Vec<Option<u64>> = rt.block_on(async {
                let mut v = vec![];
                for b in &all { v.push(stream::accounts::viewers(client, &ends, &ids, b).await) }
                v
            });
            if counts.iter().all(Option::is_none) { return }
            let total: u64 = counts.into_iter().flatten().sum();
            let _ = ui.upgrade_in_event_loop(move |ui| if ui.get_live() { ui.set_live_watching(total.min(i32::MAX as u64) as i32) });
        });
    }

    /// Signed in, live: starts reading the broadcasts' chat into the board (YouTube and Facebook every 5 s, Twitch
    /// as it is written).
    fn start_chat(&self) {
        let all: Vec<stream::accounts::Live> = self.record.broadcasts.borrow().iter().map(|(_, b)| b.clone()).collect();
        *self.record.chat.lock().unwrap_or_else(|e| e.into_inner()) = Default::default();
        self.record.chat_seen.set(0);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        *self.record.chat_stop.borrow_mut() = stop.clone();
        self.ui.set_chat_on(!all.is_empty());
        self.show_chat();
        if all.is_empty() || self.ops().is_inline() { return }
        let (ends, ids) = (stream::accounts::app_ends(), stream::accounts::app_ids());
        for live in all.iter().filter(|l| l.service == stream::Service::Twitch) {
            let Some(login) = live.login() else { continue };
            let (l, e, i) = (live.clone(), ends.clone(), ids.clone());
            let access = Arc::new(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
                let client = crate::bible::sources::client().ok()?;
                rt.block_on(l.access_now(client, &e, &i)).ok()
            });
            stream::chat::twitch_reader(stream::accounts::twitch::CHAT.into(), login, access, self.record.chat.clone(), stop.clone());
        }
        let polled: Vec<stream::accounts::Live> = all.into_iter().filter(|l| l.service != stream::Service::Twitch).collect();
        if polled.is_empty() { return }
        let board = self.record.chat.clone();
        let _ = std::thread::Builder::new().name("stream-chat".into()).spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            let Ok(client) = crate::bible::sources::client() else { return };
            let mut cursors: Vec<stream::chat::Cursor> = polled.iter().map(|_| Default::default()).collect();
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                for (live, cursor) in polled.iter().zip(cursors.iter_mut()) {
                    match rt.block_on(stream::chat::next(client, &ends, &ids, live, cursor)) {
                        Ok(new) => stream::chat::add(&board, new),
                        Err(e) => eprintln!("stream chat: {e}"),
                    }
                }
                for _ in 0..50 { if stop.load(std::sync::atomic::Ordering::Relaxed) { break } std::thread::sleep(Duration::from_millis(100)) }
            }
        });
    }

    /// The Chat button: how many new messages since the operator last looked, and the list.
    pub(crate) fn show_chat(&self) {
        let chat = self.record.chat.lock().unwrap_or_else(|e| e.into_inner());
        let new = chat.total.saturating_sub(self.record.chat_seen.get());
        self.ui.set_chat_new(new.min(999) as i32);
        let lines: Vec<slint::SharedString> = chat.list.iter().map(|m| stream::tr::chat_line(&m.name, m.from.name(), &m.text).into()).collect();
        self.ui.set_chat_lines(slint::ModelRc::new(slint::VecModel::from(lines)));
    }

    /// The chat list was opened: everything in it is seen.
    pub(crate) fn chat_opened(&self) {
        let total = self.record.chat.lock().unwrap_or_else(|e| e.into_inner()).total;
        self.record.chat_seen.set(total);
        self.show_chat();
    }

    /// The stream stopped: the readers stop.
    fn stop_chat(&self) {
        self.record.chat_stop.borrow().store(true, std::sync::atomic::Ordering::Relaxed);
        self.ui.set_chat_on(false);
    }

    /// After a stream: how long it ran and how each destination did, then (when there is more to say) what the
    /// computer left out, which destinations got a lighter stream, and the most viewers. All of it is kept in the
    /// logs (`stream::report`).
    pub(crate) fn say_stream_summary(&self, secs: u64) {
        let report = self.stream_report(secs);
        if report.rows.is_empty() { return }
        let trouble = report.rows.iter().any(|r| r.drops > 0 || !r.ever);
        self.toast(report.summary(), trouble);
        if let Some(more) = report.more() { self.toast(more, report.trouble()) }
        stream::report::keep(&self.app().store.root.join("logs"), &report, chrono::Local::now().naive_local());
    }

    /// The report of the stream under way (`secs` long).
    fn stream_report(&self, secs: u64) -> stream::report::Report {
        let history = self.record.history.borrow();
        let (links, dropped) = self.record.rec.borrow().as_ref().map(|r| (r.links(), r.dropped())).unwrap_or_default();
        let rows = self.record.links.borrow().iter().enumerate().map(|(i, (name, _))| {
            let (drops, ever, _) = history.get(i).copied().unwrap_or_default();
            let link = links.get(i).copied().unwrap_or_default();
            stream::report::Row { name: name.clone(), drops, ever, lowest_kbps: link.lowest_kbps, skips: link.skips }
        }).collect();
        stream::report::Report {
            secs, rows, dropped, slow_secs: self.record.slow_secs.get(),
            viewers: u64::try_from(self.record.viewers_most.get()).ok().filter(|_| !self.record.broadcasts.borrow().is_empty()),
            encoder: self.record.encoder.borrow().clone(),
        }
    }

    fn recording_ended(&self, ended: Ended) {
        record::unfinished::clear(&self.app().store.root);
        if self.record.live.get() && let Some(secs) = self.record.rec.borrow().as_ref().map(Recording::secs) { self.say_stream_summary(secs) }
        self.end_broadcasts(true);
        self.screens().record_to(None);
        // The writer has finished; dropping it does not wait.
        drop(self.record.rec.borrow_mut().take());
        self.show_recording(false);
        let (text, err) = ended.toast();
        self.toast(text, err);
        // The clips marked, each copied out of the finished file.
        let clips = std::mem::take(&mut *self.record.marks.borrow_mut()).finish();
        self.ui.set_clipping(false);
        if let (false, Some(file)) = (clips.is_empty(), ended.file.clone()) {
            let root = self.app().store.root.clone();
            let me = self.me.clone();
            off_ui(self, move || run::find_ffmpeg(&root).map(|ff| record::clips::cut(&ff, &file, &clips)).unwrap_or_default(), move |made| {
                let Some(op) = me.upgrade() else { return };
                for m in made {
                    match m { Ok(p) => op.toast(record::tr::clip_saved(&p.display().to_string()), false), Err(p) => op.toast(record::tr::clip_failed(&p), true) }
                }
            });
        }
    }

    /// Clip: starts a clip of the recording under way, or ends the open one. Nothing while not recording.
    pub fn toggle_clip(&self) {
        let Some(at) = self.record.rec.borrow().as_ref().map(Recording::elapsed) else { return };
        let open = self.record.marks.borrow_mut().toggle(at);
        self.ui.set_clipping(open);
        self.toast(if open { record::tr::clip_started() } else { record::tr::clip_marked() }, false);
    }

    /// "Save last N min": the last minutes of the recording so far, as a video of its own (More settings).
    pub fn save_last_minutes(&self) {
        let minutes = self.record.replay_minutes.get();
        let Some(job) = self.record.rec.borrow().as_ref().filter(|_| minutes > 0).map(|r| r.save_last(minutes)) else { return };
        self.toast(record::tr::saving_last(minutes), false);
        let me = self.me.clone();
        off_ui(self, job, move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(file) => op.toast(record::tr::saved_last(&file.display().to_string()), false),
                Err(file) => op.toast(record::tr::save_last_failed(&file), true),
            }
        });
    }

    /// A slide action or macro asked for a clip to start or end (`App::clip_request`).
    pub(crate) fn take_clip_request(&self) {
        let Some(start) = self.app().clip_request.lock().unwrap().take() else { return };
        if self.is_recording() && self.record.marks.borrow().is_open() != start { self.toggle_clip() }
    }

    /// Go Live: says which destination has just connected, and which has lost its connection (it is tried
    /// again by itself: `stream::send`).
    pub(crate) fn say_links(&self, now: &[stream::send::Link]) {
        if !self.record.live.get() { return }
        let mut said = vec![];
        let mut history = self.record.history.borrow_mut();
        for (i, ((name, was), link)) in self.record.links.borrow_mut().iter_mut().zip(now).enumerate() {
            if history.len() <= i { history.resize(i + 1, (0, false, 0)) }
            if *was != link.up {
                if link.up { history[i].1 = true } else { history[i].0 += 1 }
                *was = link.up;
                said.push(if link.up { (stream::tr::connected(name), false) } else { (stream::tr::lost(name), true) });
            }
            // Never connected after a few tries: a wrong key, or the service or the internet refusing. Said once.
            if !link.up && link.failed >= NEVER_TRIES && !self.record.never_told.borrow().contains(name) {
                self.record.never_told.borrow_mut().push(name.clone());
                said.push((stream::tr::never_connected(name), true));
            }
            // Made lighter (or full again) because of its internet: said as it changes.
            if history[i].2 != link.kbps {
                said.push(if link.kbps > 0 { (stream::tr::lighter(name, link.kbps), false) } else { (stream::tr::full_again(name), false) });
                history[i].2 = link.kbps;
            }
            // The upload cannot keep up even with the lightest stream: said once, with what to do.
            if link.lightest && link.skips >= SLOW_SKIPS && !self.record.slow_told.borrow().contains(name) {
                self.record.slow_told.borrow_mut().push(name.clone());
                said.push((stream::tr::too_slow(name), true));
            }
        }
        for (text, err) in said { self.toast(text, err) }
        // The Live button: how many are connected now, of how many.
        self.ui.set_live_total(now.len() as i32);
        self.ui.set_live_up(now.iter().filter(|l| l.up).count() as i32);
    }

    /// The first time a stream goes out from this computer: a note that streaming songs and videos needs a
    /// licence. Shown once; the choice is kept with the settings.
    pub(crate) fn note_stream_licence(self: &Rc<Self>) {
        let noted = self.state.borrow().snap.as_ref().is_some_and(|s| s.settings.record.licence_noted);
        if noted { return }
        super::super::dialogs::ask::notice(self, &stream::tr::licence_note());
        if let Err(e) = self.app().edit_untracked(|d| { d.settings.record.licence_noted = true; Ok(()) }) { eprintln!("go live: could not keep the licence note's mark: {e}") }
    }

    /// Go Live with a camera: says when its pictures stop (the stream shows the slides meanwhile) and when they
    /// come back; and when it has sent none `CAMERA_START_SECS` after Go Live (before that it is still starting).
    pub(crate) fn say_camera(&self, seen: Option<bool>) {
        let Some(now) = seen else { return };
        if !self.record.live.get() { return }
        // Never seen for `CAMERA_START_SECS`: it is not starting (unplugged, or another program has it).
        if self.record.camera_was.get().is_none() && !now {
            self.record.camera_wait.set(self.record.camera_wait.get() + 1);
            if self.record.camera_wait.get() >= CAMERA_START_SECS {
                self.record.camera_was.set(Some(false));
                self.toast(stream::tr::camera_never(), true);
            }
            return;
        }
        match (self.record.camera_was.get(), now) {
            (Some(true), false) => self.toast(stream::tr::camera_lost(), true),
            (Some(false), true) => self.toast(stream::tr::camera_back(), false),
            _ => {}
        }
        // Not seen yet and still not seen: still starting, nothing to say.
        if self.record.camera_was.get().is_some() || now { self.record.camera_was.set(Some(now)) }
    }

    /// At start: a recording the app never ended (it stopped in the middle, `record::unfinished`). Its part is
    /// finished into a video, and after a livestream the operator is asked whether to go live again.
    pub fn after_unfinished_recording(self: &Rc<Self>) {
        let root = self.app().store.root.clone();
        let Some(note) = record::unfinished::take(&root) else { return };
        // The part, finished into a video (off the UI thread; FFmpeg is needed for it).
        let folder = note.file.parent().map(PathBuf::from).unwrap_or_default();
        let me = Rc::downgrade(self);
        off_ui(self, move || run::find_ffmpeg(&root).map(|ff| run::finish_leftovers(&ff, &folder, std::path::Path::new(""))).unwrap_or_default(), move |done| {
            let Some(op) = me.upgrade() else { return };
            for e in done { let (text, err) = e.toast(); op.toast(text, err) }
        });
        if note.live {
            let me = Rc::downgrade(self);
            confirm(self, &record::tr::was_live(), &record::tr::go_live_again(), &tr::not_now(), false, move |yes| {
                if yes && let Some(op) = me.upgrade() { op.go_live() }
            });
        }
    }

    /// Recording: says when the computer cannot keep up (the picture is jerky), and when it does again, from how
    /// far behind the writer was in the last second (`None`: not recording, and the next recording starts afresh).
    pub(crate) fn say_slow(&self, lag: Option<Duration>) {
        let Some(lag) = lag else { *self.record.slow.borrow_mut() = Default::default(); return };
        if lag > record::slow::BEHIND { self.record.slow_secs.set(self.record.slow_secs.get() + 1) }
        let said = self.record.slow.borrow_mut().note(lag);
        match said {
            // The processor's encoder, chosen by the app: next time the graphics card's encoder is tried first.
            Some(record::slow::Say::TooSlow) if self.record.encoder_auto.get() && *self.record.encoder.borrow() == "libx264" => {
                self.toast(format!("{} {}", record::tr::too_slow(), record::tr::too_slow_next_time()), true);
                if let Err(e) = self.app().edit_untracked(|d| { d.settings.record.x264_too_slow = true; Ok(()) }) { eprintln!("recording: could not keep that x264 is too slow here: {e}") }
            }
            Some(record::slow::Say::TooSlow) => self.toast(record::tr::too_slow(), true),
            Some(record::slow::Say::Smooth) => self.toast(record::tr::smooth_again(), false),
            None => {}
        }
    }

    /// Recording or live with a sound input: says when it has gone silent for `args::SILENCE` (a muted send, a
    /// pulled cable), and when sound comes again. Once each time.
    pub(crate) fn say_silence(&self, silent: Option<bool>) {
        let Some(now) = silent else { return };
        if now == self.record.silent_was.replace(now) { return }
        let name = self.record.input.borrow().clone();
        if now { self.toast(record::tr::no_sound(&name), true) } else { self.toast(record::tr::sound_back(&name), false) }
    }

    /// The Freeze button: the stream (and the recording) keeps its picture as it is now; again, the live picture.
    pub fn hold_stream(&self) {
        let on = !self.ui.get_live_held();
        if self.while_live(|r| r.hold(on)) { self.ui.set_live_held(on) }
    }

    /// The Mute button: silence goes out in place of the sound; again, the sound.
    pub fn mute_stream(&self) {
        let on = !self.ui.get_live_muted();
        if self.while_live(|r| r.mute(on)) { self.ui.set_live_muted(on) }
    }

    /// Every scene on the scene button: the built-in ones, then the church's own.
    fn scenes(&self) -> Vec<Scene> { stream::own_scenes::all(&self.record.own_scenes.borrow()) }

    /// The scene button's list: `i` is a scene (`scenes`), -1 Automatic. Holds until another item goes live.
    pub fn pick_scene(&self, i: i32) {
        let scene = usize::try_from(i).ok().and_then(|i| self.scenes().get(i).copied());
        self.app().set_stream_scene(scene);
        self.show_scene();
    }

    /// While live: the scene button offers the positions of the camera in the picture, when it can be moved.
    pub fn show_positions(&self) {
        let n = if self.record.live.get() && self.active_control().is_some() { i32::from(stream::ptz::POSITIONS) } else { 0 };
        self.ui.set_camera_positions(n);
    }

    /// The PTZ control of the camera in the picture, if it has one and it can move now.
    fn active_control(&self) -> Option<stream::ptz::Control> {
        let i = usize::try_from(self.ui.get_camera_active()).unwrap_or(0);
        self.record.camera_controls.borrow().get(i).cloned().flatten().filter(stream::ptz::Control::ready)
    }

    /// Sends the camera in the picture to position `i` (from 0). The packet is sent off the UI thread (a camera
    /// named by host name is looked up first); a camera that does not answer changes nothing in the stream.
    pub fn pick_position(&self, i: i32) {
        let (Ok(i), Some(control)) = (u8::try_from(i), self.active_control()) else { return };
        if !self.record.live.get() || i >= stream::ptz::POSITIONS { return }
        let send = move || if let Err(e) = stream::ptz::recall(&control, i + 1) { eprintln!("camera control: {e}") };
        if self.ops().is_inline() { return send() }
        let _ = std::thread::Builder::new().name("camera-control".into()).spawn(send);
    }

    /// The scene button's camera rows: camera `i` goes into the picture at once (the one that was out becomes next
    /// if `i` was next).
    pub fn pick_camera(&self, i: i32) {
        if let Ok(i) = usize::try_from(i) { self.put_camera_out(i, false) }
    }

    /// The Cameras box: camera `i` is chosen to go out next. Not the one going out now.
    pub fn choose_next(&self, i: i32) {
        let Ok(i) = usize::try_from(i) else { return };
        let out = usize::try_from(self.ui.get_camera_active()).unwrap_or(0);
        if !self.record.live.get() || i >= self.record.cameras.borrow().len() || i == out { return }
        self.record.next.set(Some(i));
        if let Some(r) = self.record.rec.borrow().as_ref() { r.set_next(i) }
        self.show_cameras();
    }

    /// Switch (`fade` false) or Fade: the next camera goes out, and the one that was out becomes next.
    pub fn switch_to_next(&self, fade: bool) {
        if let Some(n) = self.record.next.get() { self.put_camera_out(n, fade) }
    }

    /// Camera `i` goes out (with the short fade when `fade`). Chosen as next, the one that was out takes its place.
    fn put_camera_out(&self, i: usize, fade: bool) {
        let was = usize::try_from(self.ui.get_camera_active()).unwrap_or(0);
        if !self.record.live.get() || i >= self.record.cameras.borrow().len() || i == was { return }
        if self.record.next.get() == Some(i) { self.record.next.set(Some(was)) }
        if let Some(r) = self.record.rec.borrow().as_ref() {
            r.switch_camera(i, fade);
            if let Some(n) = self.record.next.get() { r.set_next(n) }
        }
        self.ui.set_camera_active(i as i32);
        self.show_positions();
        self.show_cameras();
    }

    /// Sends the next camera to position `i` (from 0), to get its shot ready before it goes out.
    pub fn move_next(&self, i: i32) {
        let control = self.record.next.get().and_then(|n| self.record.camera_controls.borrow().get(n).cloned().flatten());
        let (Ok(i), Some(control)) = (u8::try_from(i), control) else { return };
        if !self.record.live.get() || i >= stream::ptz::POSITIONS { return }
        let send = move || if let Err(e) = stream::ptz::recall(&control, i + 1) { eprintln!("camera control: {e}") };
        if self.ops().is_inline() { return send() }
        let _ = std::thread::Builder::new().name("camera-control".into()).spawn(send);
    }

    /// The Cameras box: shown while live with a camera; which camera is out and which is next, and their names.
    pub(crate) fn show_cameras(&self) {
        let g = self.ui.global::<crate::ui::CameraSwitch>();
        let names: Vec<String> = self.record.cameras.borrow().iter().map(|c| record::args::camera_label(c)).collect();
        let shown = self.record.live.get() && !names.is_empty();
        g.set_shown(shown);
        if !shown { return }
        let out = usize::try_from(self.ui.get_camera_active()).unwrap_or(0).min(names.len() - 1);
        let next = self.record.next.get().filter(|n| *n < names.len() && *n != out);
        let named = |i: usize| format!("{} {}", i + 1, names[i]);
        let in_picture = self.record.rec.borrow().as_ref().and_then(Recording::scene).is_none_or(Scene::needs_camera);
        g.set_live(out as i32);
        g.set_next(next.map_or(-1, |n| n as i32));
        g.set_live_caption(if in_picture { named(out) } else { tr::not_in_scene(&named(out)) }.into());
        g.set_next_caption(next.map(named).unwrap_or_default().into());
        let movable = next.is_some_and(|n| self.record.camera_controls.borrow().get(n).is_some_and(Option::is_some));
        g.set_next_positions(if movable { i32::from(stream::ptz::POSITIONS) } else { 0 });
        let listed: Vec<slint::SharedString> = names.into_iter().map(Into::into).collect();
        if g.get_names().row_count() != listed.len() || g.get_names().iter().zip(&listed).any(|(a, b)| a != *b) {
            g.set_names(slint::ModelRc::new(slint::VecModel::from(listed)));
        }
    }

    /// The small Live and Next pictures the writer made last (`record::peek`), made into images when they changed.
    pub(crate) fn show_peek(&self, peek: record::peek::Peek) {
        use record::peek::Small;
        let same = |a: &Option<Arc<Small>>, b: &Option<Arc<Small>>| match (a, b) { (Some(a), Some(b)) => Arc::ptr_eq(a, b), (None, None) => true, _ => false };
        let image = |s: &Small| slint::Image::from_rgba8(slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&s.rgba, s.width, s.height));
        let g = self.ui.global::<crate::ui::CameraSwitch>();
        let mut shown = self.record.peek_shown.borrow_mut();
        if !same(&shown.live, &peek.live) {
            g.set_live_has_picture(peek.live.is_some());
            g.set_live_picture(peek.live.as_deref().map(image).unwrap_or_default());
        }
        if !same(&shown.next, &peek.next) {
            g.set_next_has_picture(peek.next.is_some());
            g.set_next_picture(peek.next.as_deref().map(image).unwrap_or_default());
        }
        *shown = peek;
    }

    /// The scene button: the scene going out, and the one chosen by hand while it holds.
    pub fn show_scene(&self) {
        let (info, choice, _) = self.app().stream_info();
        let all = self.scenes();
        let index = |s: Scene| all.iter().position(|x| *x == s).map_or(-1, |i| i as i32);
        self.ui.set_scene_chosen(stream::scene::lit(choice, &info).map_or(-1, index));
        let out = self.record.rec.borrow().as_ref().and_then(Recording::scene);
        if let Some(s) = out { self.ui.set_scene_out(index(s)) }
        // The phones' tally lights: red for the camera out, green for the one chosen as next.
        let camera = usize::try_from(self.ui.get_camera_active()).ok().filter(|_| !self.record.cameras.borrow().is_empty());
        let tally = stream::tally::Tally::of(self.record.live.get() && self.is_recording(), camera, self.record.next.get(), out.is_some_and(|s| stream::own_scenes::shows_camera(s, camera.unwrap_or(0), &self.record.own_scenes.borrow())));
        *self.app().tally.lock().unwrap() = tally;
        // NDI cameras: their own tally lights, and their positions once they say they can move.
        stream::ndi::show_tally(&self.record.cameras.borrow(), &tally);
        self.show_positions();
        // The sound meter: the loudest since the last look (-1 hides it: no sound going out to measure).
        let level = self.record.rec.borrow().as_ref().and_then(Recording::level);
        self.ui.set_live_level(level.map_or(-1, stream::level::bars));
    }

    /// Runs `f` on the recording when it is live. True when it was.
    fn while_live(&self, f: impl FnOnce(&Recording)) -> bool {
        match self.record.rec.borrow().as_ref() {
            Some(r) if self.record.live.get() => { f(r); true }
            _ => false,
        }
    }

    /// Whether a recording is under way.
    pub fn is_recording(&self) -> bool { self.record.rec.borrow().is_some() }

    fn show_recording(&self, on: bool) {
        self.ui.set_recording(on);
        self.ui.set_replay_minutes(if on { self.record.replay_minutes.get().min(i32::MAX as u32) as i32 } else { 0 });
        if !on {
            self.record.live.set(false);
            self.ui.set_live(false);
            *self.app().tally.lock().unwrap() = Default::default();
            self.record.next.set(None);
            self.show_cameras();
            self.show_peek(Default::default());
            self.stop_chat();
            self.ui.set_camera_positions(0);
            self.ui.set_live_held(false);
            self.ui.set_live_muted(false);
            self.ui.set_live_watching(-1);
        }
        self.ui.set_record_time(record::clock(0).into());
        if !on {
            self.ui.set_live_level(-1);
            self.record.scene_ticker.stop();
            return self.record.ticker.stop();
        }
        // The scene button, while live: its names, and what goes out looked at four times a second.
        if self.record.live.get() {
            let own = self.record.own_scenes.borrow().clone();
            let labels: Vec<slint::SharedString> = self.scenes().into_iter().map(|s| stream::own_scenes::name(s, &own).into()).collect();
            self.ui.set_scene_labels(slint::ModelRc::new(slint::VecModel::from(labels)));
            // With two cameras the list offers each; the first is in the picture at the start.
            let cams = self.record.cameras.borrow();
            let cams: Vec<slint::SharedString> = if cams.len() > 1 { cams.iter().map(|c| record::args::camera_label(c).into()).collect() } else { vec![] };
            self.ui.set_camera_labels(slint::ModelRc::new(slint::VecModel::from(cams)));
            self.show_positions();
            self.ui.set_camera_active(0);
            // Two cameras or more: the second is next at the start.
            let next = (self.record.cameras.borrow().len() > 1).then_some(1);
            self.record.next.set(next);
            if let (Some(n), Some(r)) = (next, self.record.rec.borrow().as_ref()) { r.set_next(n) }
            self.show_scene();
            self.show_cameras();
            let me = self.me.clone();
            self.record.scene_ticker.start(TimerMode::Repeated, Duration::from_millis(250), move || {
                let Some(op) = me.upgrade() else { return };
                op.show_scene();
                op.show_cameras();
                let peek = op.record.rec.borrow().as_ref().map(Recording::peek).unwrap_or_default();
                op.show_peek(peek);
            });
        }
        let me = self.me.clone();
        self.record.ticker.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(op) = me.upgrade() else { return };
            let secs = op.record.rec.borrow().as_ref().map(Recording::secs);
            if let Some(s) = secs { op.ui.set_record_time(record::clock(s).into()) }
            let links = op.record.rec.borrow().as_ref().map(Recording::links).unwrap_or_default();
            op.say_links(&links);
            let camera = op.record.rec.borrow().as_ref().and_then(Recording::camera_seen);
            op.say_camera(camera);
            let silent = op.record.rec.borrow().as_ref().and_then(Recording::silent);
            op.say_silence(silent);
            op.take_clip_request();
            let lag = op.record.rec.borrow().as_ref().map(Recording::lag);
            op.say_slow(lag);
            op.ask_viewers();
            op.record.viewers_most.set(op.record.viewers_most.get().max(op.ui.get_live_watching()));
            if op.ui.get_chat_on() { op.show_chat() }
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

/// A sender that skipped this often is not a passing hiccup: the internet cannot keep up with that destination.
const SLOW_SKIPS: u32 = 3;

pub(super) fn wire(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ui.on_record(move || if let Some(op) = me.upgrade() { op.toggle_recording() });
    let me = Rc::downgrade(op);
    op.ui.on_toggle_clip(move || if let Some(op) = me.upgrade() { op.toggle_clip() });
    let me = Rc::downgrade(op);
    op.ui.on_go_live(move || if let Some(op) = me.upgrade() { op.go_live() });
    let me = Rc::downgrade(op);
    op.ui.on_save_last(move || if let Some(op) = me.upgrade() { op.save_last_minutes() });
    let me = Rc::downgrade(op);
    op.ui.on_hold_stream(move || if let Some(op) = me.upgrade() { op.hold_stream() });
    let me = Rc::downgrade(op);
    op.ui.on_mute_stream(move || if let Some(op) = me.upgrade() { op.mute_stream() });
    let me = Rc::downgrade(op);
    op.ui.on_pick_scene(move |i| if let Some(op) = me.upgrade() { op.pick_scene(i) });
    let me = Rc::downgrade(op);
    op.ui.on_pick_camera(move |i| if let Some(op) = me.upgrade() { op.pick_camera(i) });
    let me = Rc::downgrade(op);
    op.ui.on_chat_opened(move || if let Some(op) = me.upgrade() { op.chat_opened() });
    let me = Rc::downgrade(op);
    op.ui.on_pick_position(move |i| if let Some(op) = me.upgrade() { op.pick_position(i) });
    let cams = op.ui.global::<crate::ui::CameraSwitch>();
    let me = Rc::downgrade(op);
    cams.on_choose_next(move |i| if let Some(op) = me.upgrade() { op.choose_next(i) });
    let me = Rc::downgrade(op);
    cams.on_switch(move || if let Some(op) = me.upgrade() { op.switch_to_next(false) });
    let me = Rc::downgrade(op);
    cams.on_fade(move || if let Some(op) = me.upgrade() { op.switch_to_next(true) });
    let me = Rc::downgrade(op);
    cams.on_move_next(move |i| if let Some(op) = me.upgrade() { op.move_next(i) });
}

mod tr {
    use crate::tr;
    pub fn get_ffmpeg() -> String { tr!("Get FFmpeg") }
    pub fn not_now() -> String { tr!("Not now") }
    pub fn getting_ffmpeg() -> String { tr!("Getting FFmpeg for recording") }
    pub fn ffmpeg_ready() -> String { tr!("FFmpeg is ready. Press Record to start recording.") }
    pub fn ffmpeg_failed(command: &str) -> String { tr!("FFmpeg could not be downloaded. Check the internet connection, or install it yourself: {command}", command = command) }
    /// Under the Live picture when the scene going out shows no camera: the camera that comes with a camera scene.
    pub fn not_in_scene(camera: &str) -> String { tr!("{camera} (not in this scene)", camera = camera) }
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
