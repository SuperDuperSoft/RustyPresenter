//! `live-state.json`: the last committed frame and media positions (spec §8.3).
//! Written atomically off the UI thread; read once on `--restore`.
use crate::live::{Cursor, Frame, Live, LiveItem};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, atomic::{AtomicBool, Ordering}},
    thread::JoinHandle,
    time::Duration,
};

/// What was on air, as the room last committed it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LiveState {
    /// Format version: 1.
    pub v: u32,
    pub seq: u64,
    pub saved_at: String,
    pub frame: Frame,
    pub cursor: Option<Cursor>,
    pub item: Option<LiveItem>,
    /// Playback instance → seconds played, for the video and audio in `frame`.
    pub positions: Vec<(u64, f64)>,
    /// Open screens to reopen: "room", "stage".
    pub windows: Vec<String>,
}

pub fn path(root: &Path) -> PathBuf { root.join("live-state.json") }

/// The saved state, if there is a readable one in this format.
pub fn load(root: &Path) -> Option<LiveState> {
    let s: LiveState = serde_json::from_slice(&std::fs::read(path(root)).ok()?).ok()?;
    (s.v == 1).then_some(s)
}

/// Sets aside a state that crashed the app on restore, so it is never tried again.
pub fn quarantine(root: &Path) {
    if let Err(e) = std::fs::rename(path(root), root.join("live-state.crashed.json")) { eprintln!("Could not set aside the saved live state: {e}") }
}

pub fn clear(root: &Path) { let _ = std::fs::remove_file(path(root)); }

/// `store::write_atomic`, retried while something (antivirus, an indexer, OneDrive) holds the file open.
pub fn write_with_retry(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tries = 0;
    loop {
        match crate::store::write_atomic(path, bytes) {
            Err(e) if tries < 10 && (e.kind() == io::ErrorKind::PermissionDenied || (cfg!(windows) && e.raw_os_error() == Some(32))) => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(30));
            }
            r => return r,
        }
    }
}

/// Saves states on its own thread (`live-state`). Only the latest waiting state is written.
pub struct StateWriter {
    shared: Arc<(Mutex<Option<LiveState>>, Condvar)>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Test hook: the thread holds this while it writes, so a test can pause it.
    #[cfg(test)]
    gate: Arc<Mutex<()>>,
}

impl StateWriter {
    pub fn start(root: PathBuf) -> StateWriter {
        let shared = Arc::new((Mutex::new(None::<LiveState>), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        let gate = Arc::new(Mutex::new(()));
        let (s, halt) = (shared.clone(), stop.clone());
        #[cfg(test)]
        let g = gate.clone();
        let thread = std::thread::Builder::new().name("live-state".into()).spawn(move || loop {
            let next = {
                let mut slot = s.1.wait_while(s.0.lock().unwrap(), |v| v.is_none() && !halt.load(Ordering::Acquire)).unwrap();
                slot.take()
            };
            let Some(state) = next else { return }; // stopping, and nothing left to write
            #[cfg(test)]
            let _g = g.lock().unwrap();
            let written = serde_json::to_vec(&state).map_err(io::Error::other).and_then(|b| write_with_retry(&path(&root), &b));
            if let Err(e) = written { eprintln!("Could not save the live state: {e}") }
        }).expect("live-state thread");
        StateWriter { shared, stop, thread: Some(thread), #[cfg(test)] gate }
    }

    /// Replaces any state still waiting to be written.
    pub fn submit(&self, s: LiveState) {
        *self.shared.0.lock().unwrap() = Some(s);
        self.shared.1.notify_one();
    }

    /// Writes whatever is waiting, then ends the thread.
    pub fn flush_and_stop(mut self) {
        {
            let _slot = self.shared.0.lock().unwrap();
            self.stop.store(true, Ordering::Release);
            self.shared.1.notify_one();
        }
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}

/// The committed frame as a state to save, or `None` before the room has committed anything.
pub fn snapshot(live: &Live, windows: Vec<String>, seq: u64) -> Option<LiveState> {
    let (frame, cursor, item) = live.committed()?;
    let positions = [&live.media_status, &live.audio_status].into_iter().flatten()
        .filter(|p| frame.media.as_ref().is_some_and(|m| m.inst == p.inst) || frame.audio.as_ref().is_some_and(|a| a.inst == p.inst))
        .map(|p| (p.inst, p.position)).collect();
    Some(LiveState { v: 1, seq, saved_at: crate::store::now_stamp(), frame, cursor, item, positions, windows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::{App, test_app}, live::*, model::*};
    use std::sync::Arc;
    use uuid::Uuid;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rp-state-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn saved(frame: Frame, seq: u64) -> LiveState {
        LiveState { v: 1, seq, saved_at: crate::store::now_stamp(), frame, cursor: None, item: None, positions: vec![], windows: vec!["room".into()] }
    }
    fn add(app: &App, kind: ItemKind) -> Uuid {
        let id = Uuid::new_v4();
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id, kind, background: None, notes: String::new() });
        id
    }
    fn text_item(app: &App, body: &str) -> Uuid { add(app, ItemKind::Text { title: body.into(), body: body.into() }) }
    fn run(app: &App, cmd: Command) -> u64 {
        let out = app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd });
        assert!(out.ok, "{out:?}");
        out.version
    }
    fn show_commit(app: &App, item_id: Uuid) {
        let version = run(app, Command::Show { item_id, slide: 0 });
        app.room_event(RoomEvent::Committed { version });
    }
    fn slide_text(f: &Frame) -> String { match &f.slide { Some(SlideLayer::Text { text, .. }) => text.clone(), _ => String::new() } }

    #[test]
    fn snapshot_uses_committed_frame_not_requested() {
        let app = test_app();
        let (a, b) = (text_item(&app, "Alpha"), text_item(&app, "Bravo"));
        let media_id = Uuid::new_v4();
        app.data.lock().unwrap().media.push(MediaFile { id: media_id, name: "m".into(), kind: MediaKind::Audio, asset: "m.mp3".into(), status: MediaStatus::Ready, original_asset: None });
        let music = add(&app, ItemKind::Media { media_id, autoplay: true, end: EndAction::Stop, looped: false });
        assert!(snapshot(&app.live.lock().unwrap(), vec![], 1).is_none(), "nothing committed yet");
        show_commit(&app, a);
        show_commit(&app, music);
        let inst = app.live_view().frame.audio.unwrap().inst;
        app.room_event(RoomEvent::Media { inst, state: PlayState::Playing, position: 12.5, duration: 60.0, error: None });
        run(&app, Command::Show { item_id: b, slide: 0 }); // requested, not on air yet
        let s = snapshot(&app.live.lock().unwrap(), vec!["room".into()], 7).unwrap();
        assert!(slide_text(&s.frame).contains("Alpha"), "{:?}", s.frame);
        assert_eq!(s.cursor.as_ref().map(|c| c.item_id), Some(music));
        assert_eq!(s.item.as_ref().map(|i| i.item_id), Some(music));
        assert_eq!(s.positions, vec![(inst, 12.5)]);
        assert_eq!((s.v, s.seq, s.windows.clone()), (1, 7, vec!["room".to_string()]));
    }

    #[test]
    fn load_missing_garbage_and_wrong_version_are_none() {
        let root = dir();
        assert!(load(&root).is_none());
        std::fs::write(path(&root), b"{not json").unwrap();
        assert!(load(&root).is_none());
        let mut s = saved(Frame { logo: true, ..Frame::default() }, 3);
        s.v = 2;
        std::fs::write(path(&root), serde_json::to_vec(&s).unwrap()).unwrap();
        assert!(load(&root).is_none());
        s.v = 1;
        std::fs::write(path(&root), serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(load(&root), Some(s));
        clear(&root);
        assert!(!path(&root).exists());
    }

    #[test]
    fn restore_reissues_instances_above_saved() {
        let app = test_app();
        let a = text_item(&app, "Alpha");
        let frame = Frame {
            background: Some(Inst { inst: 40, v: Background::Color { color: "#123456".into() } }),
            audio: Some(Inst { inst: 41, v: AudioLayer { asset: "m.mp3".into(), title: "Music".into(), looped: false, paused: false } }),
            ..Frame::default()
        };
        let mut s = saved(frame.clone(), 1);
        s.cursor = Some(Cursor { item_id: a, item_index: 0, slide: 0 });
        {
            let mut live = app.live.lock().unwrap();
            live.restore(&s, true);
            assert_eq!(live.frame, frame);
            assert_eq!(live.cursor, s.cursor);
            assert!(live.pending(), "the room must commit the restored frame");
        }
        run(&app, Command::Show { item_id: a, slide: 0 });
        let bg = app.live_view().frame.background.unwrap().inst;
        assert!(bg > 41, "new instance {bg} must not reuse a saved one");
    }

    #[test]
    fn restore_without_media_keeps_slide_and_colour_background() {
        let slide = SlideLayer::Text { text: "Alpha".into(), footer: None, reference: None, scripture: false };
        let audio = Inst { inst: 3, v: AudioLayer { asset: "m.mp3".into(), title: "Music".into(), looped: true, paused: false } };
        let frame = Frame {
            background: Some(Inst { inst: 1, v: Background::Color { color: "#000".into() } }),
            media: Some(Inst { inst: 2, v: MediaLayer::Video { asset: "v.mp4".into(), end: EndAction::Stop, paused: false } }),
            slide: Some(slide.clone()),
            logo: false,
            audio: Some(audio),
        };
        let mut live = Live::new();
        live.restore(&saved(frame, 1), false);
        assert_eq!(live.frame.slide, Some(slide));
        assert!(matches!(live.frame.background.as_ref().map(|b| &b.v), Some(Background::Color { .. })));
        assert!(live.frame.media.is_none() && live.frame.audio.is_none());

        // A video background goes; a still and an image background stay.
        let still = Some(Inst { inst: 5, v: MediaLayer::Still { asset: "s.png".into() } });
        let mut live = Live::new();
        live.restore(&saved(Frame { background: Some(Inst { inst: 4, v: Background::Video { asset: "bg.mp4".into() } }), media: still.clone(), ..Frame::default() }, 1), false);
        assert!(live.frame.background.is_none());
        assert_eq!(live.frame.media, still);
        let mut live = Live::new();
        live.restore(&saved(Frame { background: Some(Inst { inst: 6, v: Background::Image { asset: "bg.png".into() } }), ..Frame::default() }, 1), false);
        assert!(live.frame.background.is_some());
    }

    #[test]
    fn writer_keeps_only_latest() {
        let root = dir();
        let w = StateWriter::start(root.clone());
        let gate = w.gate.lock().unwrap(); // the thread cannot write while this is held
        for seq in 1..=100 { w.submit(saved(Frame::default(), seq)); }
        assert_eq!(w.shared.0.lock().unwrap().as_ref().map(|s| s.seq), Some(100), "later states replace waiting ones");
        drop(gate);
        w.flush_and_stop();
        assert_eq!(load(&root).map(|s| s.seq), Some(100));
    }

    #[cfg(windows)]
    #[test]
    fn rename_retries_when_target_is_locked() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = dir();
        let p = path(&root);
        std::fs::write(&p, b"old").unwrap();
        // Shares read only (no FILE_SHARE_DELETE), as an indexer or antivirus scan would.
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(&p).unwrap();
        assert!(crate::store::write_atomic(&p, b"blocked").is_err(), "the open handle must block the rename");
        let t = std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_millis(100)); drop(held) });
        write_with_retry(&p, b"new").unwrap();
        t.join().unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
    }

    #[test]
    fn quarantine_renames() {
        let root = dir();
        std::fs::write(path(&root), serde_json::to_vec(&saved(Frame::default(), 1)).unwrap()).unwrap();
        quarantine(&root);
        assert!(!path(&root).exists());
        assert!(root.join("live-state.crashed.json").exists());
        assert!(load(&root).is_none());
    }

    #[test]
    fn app_writes_state_after_commit() {
        let app: Arc<App> = test_app();
        app.set_state_writer(StateWriter::start(app.store.root.clone()));
        let a = text_item(&app, "Alpha");
        let version = run(&app, Command::Show { item_id: a, slide: 0 });
        app.room_event(RoomEvent::Committed { version });
        app.stop_state_writer(); // writes what is waiting
        let s = load(&app.store.root).expect("live-state.json after the commit");
        assert!(slide_text(&s.frame).contains("Alpha"), "{:?}", s.frame);
    }

    #[test]
    fn app_saves_nothing_requested_or_after_stop() {
        let app: Arc<App> = test_app();
        app.set_state_writer(StateWriter::start(app.store.root.clone()));
        let (a, b) = (text_item(&app, "Alpha"), text_item(&app, "Bravo"));
        run(&app, Command::Show { item_id: a, slide: 0 });
        app.stop_state_writer();
        assert!(load(&app.store.root).is_none(), "a requested frame is not saved");
        // A clean quit stops the writer, then clears: a later commit must not bring the file back.
        clear(&app.store.root);
        let version = run(&app, Command::Show { item_id: b, slide: 0 });
        app.room_event(RoomEvent::Committed { version });
        app.stop_state_writer();
        assert!(!path(&app.store.root).exists());
    }

    #[test]
    fn resume_position_lasts_until_on_air() {
        let app = test_app();
        app.set_resume(&[(7, 3.5)]);
        assert_eq!(app.resume_position(8), None);
        assert_eq!(app.resume_position(7), Some(3.5));
        // A newer plan may prepare the same instance again before the first one went on air.
        assert_eq!(app.resume_position(7), Some(3.5));
        let frame = Frame { background: Some(Inst { inst: 7, v: Background::Video { asset: "bg.mp4".into() } }), ..Frame::default() };
        let version = { let mut live = app.live.lock().unwrap(); live.restore(&saved(frame, 1), true); live.version };
        app.room_event(RoomEvent::Committed { version });
        assert_eq!(app.resume_position(7), None, "a later replay starts from the beginning");
    }
}
