//! A room with no window and no player, for the recovery tests (`RUSTYPRESENTER_HEADLESS=1`). It commits
//! the newest version it is sent, and a restored one before it, and logs what a restore put back, so a test
//! can read it from the log. `Screens` stands in for the windows.
use crate::{app::{App, Update}, live::{MediaLayer, RoomEvent}, model::{Background, SlideContent}};
use std::{io::Write, sync::{Arc, Weak}, thread::JoinHandle};
use tokio::sync::broadcast::error::RecvError;

/// Follows the live updates on a thread of its own until the app is gone.
pub fn run(app: Arc<App>, mut log: impl Write + Send + 'static) -> JoinHandle<()> {
    // Subscribe before reading the view so no update falls in between.
    let mut rx = app.subscribe();
    let weak = Arc::downgrade(&app);
    drop(app);
    std::thread::Builder::new().name("headless-room".into()).spawn(move || {
        // It stands in for the room, so a panic here must restart the app as one there would.
        crate::recovery::panic::mark_critical();
        let mut logged = false;
        // A restore made before the room started is already waiting.
        if !commit_latest(&weak, &mut logged, &mut log) { return; }
        loop {
            match rx.blocking_recv() {
                Ok(Update::Live(_)) | Err(RecvError::Lagged(_)) => if !commit_latest(&weak, &mut logged, &mut log) { return },
                Ok(_) => {}
                Err(RecvError::Closed) => return,
            }
        }
    }).expect("the headless room thread starts")
}

/// Commits the newest version, as a room would once it is on screen. False once the app is gone.
fn commit_latest(app: &Weak<App>, logged: &mut bool, log: &mut impl Write) -> bool {
    let Some(app) = app.upgrade() else { return false };
    let (version, restored) = { let l = app.live.lock().unwrap(); (l.version, l.restored) };
    // The restored frame goes on air first, even when a newer one is already waiting, so the log names it.
    if let Some(r) = restored { commit(&app, r); }
    if !*logged && let Some(line) = restored_line(&app) {
        *logged = true;
        let _ = writeln!(log, "{line}").and_then(|_| log.flush());
    }
    commit(&app, version);
    true
}

fn commit(app: &App, version: u64) {
    if version > app.live.lock().unwrap().committed_version { app.room_event(RoomEvent::Committed { version }); }
}

/// The windowing layer with no windows. It answers a second launch's request for the operator, so that
/// launch exits as it would with a real operator window, and keeps the roles it was asked to open.
#[derive(Default)]
pub struct Screens { roles: std::sync::Mutex<Vec<String>> }

impl crate::app::Screens for Screens {
    fn open(&self, role: &str, _monitor: Option<&str>) -> Result<(), String> {
        match role {
            "operator" => Ok(()),
            "room" | "stage" => {
                let mut roles = self.roles.lock().unwrap();
                if !roles.iter().any(|r| r == role) { roles.push(role.into()) }
                Ok(())
            }
            _ => Err("unknown window".into()),
        }
    }

    fn close(&self, role: &str) { self.roles.lock().unwrap().retain(|r| r != role) }

    fn open_roles(&self) -> Vec<String> { self.roles.lock().unwrap().clone() }

    fn monitors(&self) -> Vec<crate::app::Monitor> { vec![] }
}

/// `restored version={v} slide={json} media={bool}` once the restored frame is committed, or
/// `restore superseded version={v} committed={c}` if something newer went on air without it.
fn restored_line(app: &App) -> Option<String> {
    let live = app.live.lock().unwrap();
    let r = live.restored?;
    if live.committed_version < r { return None; }
    if live.committed_version > r { return Some(format!("restore superseded version={r} committed={}", live.committed_version)); }
    let (frame, cursor, item) = live.committed()?;
    let slide = cursor.zip(item).and_then(|(c, i)| match i.slides.get(c.slide)?.content.clone() {
        SlideContent::Text { text, .. } => Some(text),
        _ => None,
    });
    let media = frame.media.as_ref().is_some_and(|m| matches!(m.v, MediaLayer::Video { .. }))
        || frame.background.as_ref().is_some_and(|b| matches!(b.v, Background::Video { .. }))
        || frame.audio.is_some();
    Some(format!("restored version={r} slide={} media={media}", serde_json::to_string(&slide).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, live::*, model::*, recovery::{panic, state::LiveState}};
    use std::{env, process::{Command as Process, Output}, sync::Mutex, time::{Duration, Instant}};
    use uuid::Uuid;

    /// The log, readable while the room writes to it.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(b); Ok(b.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    impl Sink {
        fn text(&self) -> String { String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned() }
    }

    /// A test app whose data folder goes when the test ends.
    struct Fresh(Arc<App>);

    impl std::ops::Deref for Fresh {
        type Target = Arc<App>;
        fn deref(&self) -> &Arc<App> { &self.0 }
    }

    impl Drop for Fresh {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0.store.root); }
    }

    fn fresh() -> Fresh { Fresh(test_app()) }

    fn wait_until(done: impl Fn() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if done() { return true; }
            std::thread::sleep(Duration::from_millis(10));
        }
        done()
    }

    fn add_text(app: &App, body: &str) -> Uuid {
        let id = Uuid::new_v4();
        let kind = ItemKind::Text { title: "Welcome".into(), body: body.into() };
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id, kind, background: None, notes: String::new() });
        id
    }

    fn show(app: &App, item_id: Uuid) -> u64 {
        let out = app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id, slide: 0 } });
        assert!(out.ok, "{out:?}");
        out.version
    }

    /// A saved state showing "Amazing grace", with an audio layer when `audio`.
    fn saved(audio: bool) -> LiveState {
        let item_id = Uuid::new_v4();
        let slide = Slide { label: "Verse 1".into(), content: SlideContent::Text { text: "Amazing grace".into(), footer: None, reference: None, size: 0 }, notes: String::new() };
        let frame = Frame {
            slide: Some(SlideLayer::Text { text: "Amazing grace".into(), footer: None, reference: None, scripture: false, passage: vec![], size: 0 }),
            audio: audio.then(|| Inst { inst: 7, v: AudioLayer { asset: "a.mp3".into(), title: "Hymn".into(), looped: false, paused: false } }),
            ..Frame::default()
        };
        LiveState {
            v: 1, seq: 3, saved_at: "now".into(), frame,
            cursor: Some(Cursor { item_id, item_index: 0, slide: 0 }),
            item: Some(LiveItem { item_id, title: "Amazing Grace".into(), kind: "song".into(), slides: vec![slide], background: Background::Color { color: "#000".into() } }),
            positions: vec![(7, 12.0)], windows: vec!["room".into()],
        }
    }

    #[test]
    fn headless_commits_each_version() {
        let app = fresh();
        let log = Sink::default();
        let _room = run(app.clone(), log.clone());
        let (a, b) = (add_text(&app, "Hello"), add_text(&app, "Again"));
        let first = show(&app, a);
        assert!(wait_until(|| app.live.lock().unwrap().committed_version == first), "first show was not committed");
        let second = show(&app, b);
        assert!(wait_until(|| app.live.lock().unwrap().committed_version == second), "second show was not committed");
        assert!(!log.text().contains("restored"), "nothing was restored: {}", log.text());
    }

    #[test]
    fn headless_commits_what_was_waiting_before_it_started() {
        let app = fresh();
        let version = show(&app, add_text(&app, "Hello"));
        let _room = run(app.clone(), Sink::default());
        assert!(wait_until(|| app.live.lock().unwrap().committed_version == version));
    }

    #[test]
    fn headless_logs_restored_version_and_slide() {
        let app = fresh();
        app.live.lock().unwrap().restore(&saved(false), false);
        let version = app.live.lock().unwrap().version;
        let log = Sink::default();
        let _room = run(app.clone(), log.clone());
        assert!(wait_until(|| log.text().contains("restored")), "no restore logged: {}", log.text());
        assert_eq!(log.text().trim_end(), format!(r#"restored version={version} slide="Amazing grace" media=false"#));
        // Later shows are ordinary commits, and the restore is logged once.
        let next = show(&app, add_text(&app, "Hello"));
        assert!(wait_until(|| app.live.lock().unwrap().committed_version == next));
        assert_eq!(log.text().matches("restored").count(), 1, "{}", log.text());
    }

    #[test]
    fn headless_logs_the_restored_frame_even_when_a_newer_one_waits() {
        let app = fresh();
        app.live.lock().unwrap().restore(&saved(false), false);
        let restored = app.live.lock().unwrap().version;
        // A show lands before the room starts: the restored frame still goes on air first.
        let newer = show(&app, add_text(&app, "Hello"));
        let log = Sink::default();
        let _room = run(app.clone(), log.clone());
        assert!(wait_until(|| app.live.lock().unwrap().committed_version == newer), "the newer show was not committed");
        assert!(wait_until(|| log.text().contains("restored")), "no restore logged: {}", log.text());
        assert_eq!(log.text().trim_end(), format!(r#"restored version={restored} slide="Amazing grace" media=false"#));
    }

    #[test]
    fn headless_logs_a_restore_made_while_it_runs_with_its_media() {
        let app = fresh();
        let log = Sink::default();
        let _room = run(app.clone(), log.clone());
        app.live.lock().unwrap().restore(&saved(true), true);
        app.broadcast_live();
        assert!(wait_until(|| log.text().contains("restored")), "no restore logged: {}", log.text());
        assert!(log.text().contains(r#"slide="Amazing grace" media=true"#), "{}", log.text());
    }

    const HELPER: &str = "recovery::headless::tests::crash_helper";

    /// Runs `crash_helper` in a process of its own, with the restore crash point armed. A panic hook
    /// lasts for the whole process. Returns what it printed and its panic log.
    fn helper(mode: &str) -> (Output, String) {
        let log = env::temp_dir().join(format!("rp-headless-{}.log", Uuid::new_v4()));
        let out = Process::new(env::current_exe().unwrap()).args(["--exact", HELPER, "--nocapture"])
            .env("RP_HEADLESS_HELPER", &log).env("RP_HEADLESS_MODE", mode)
            .env("RUSTYPRESENTER_TEST_CRASH", "panic").env("RUSTYPRESENTER_TEST_CRASH_AT", "restore-media").output().unwrap();
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_dir_all(log.with_extension("data"));
        (out, text)
    }

    /// The harness also exits 101 for a failed test; only the hook exits before it reports.
    fn left_early(out: &Output) -> bool { !String::from_utf8_lossy(&out.stdout).contains("test result:") }

    #[test]
    fn crash_helper() {
        let Some(log) = env::var_os("RP_HEADLESS_HELPER").map(std::path::PathBuf::from) else { return };
        panic::install(log.clone());
        match env::var("RP_HEADLESS_MODE").unwrap().as_str() {
            "restore-plain" | "restore-media" => {
                panic::mark_critical();
                Live::new().restore(&saved(true), env::var("RP_HEADLESS_MODE").unwrap() == "restore-media");
            }
            "poisoned" => {
                // The process exits from the hook, so its data folder is next to the log for `helper` to remove.
                let store = crate::store::Store::open(log.with_extension("data")).unwrap();
                let data = store.load().unwrap();
                let app = App::new(store, data, 0, None);
                // Another thread dies holding the live state, so the room's next lock panics.
                let poisoner = app.clone();
                assert!(std::thread::spawn(move || { let _l = poisoner.live.lock().unwrap(); panic!("poison") }).join().is_err());
                let _ = run(app.clone(), Sink::default()).join();
            }
            mode => unreachable!("{mode}"),
        }
    }

    #[test]
    fn restore_media_crash_point_only_with_media() {
        let (without, log) = helper("restore-plain");
        assert!(without.status.success(), "{without:?}");
        assert!(log.is_empty(), "{log}");
        let (with, log) = helper("restore-media");
        assert_eq!(with.status.code(), Some(101), "{with:?}");
        assert!(left_early(&with), "the hook exits before the harness reports");
        assert!(log.contains("PANIC thread=") && log.contains("test crash at restore-media"), "{log}");
    }

    #[test]
    fn headless_screens_answer_a_second_launch_and_keep_roles() {
        use crate::app::Screens as _;
        let s = Screens::default();
        assert_eq!(s.open("operator", None), Ok(()), "a second launch's focus request succeeds");
        s.open("stage", None).unwrap();
        s.open("room", Some("x")).unwrap();
        s.open("room", None).unwrap();
        assert_eq!(s.open_roles(), ["stage", "room"]);
        s.close("stage");
        assert_eq!(s.open_roles(), ["room"]);
        assert!(s.open("projector", None).is_err());
        assert!(s.monitors().is_empty());
    }

    #[test]
    fn a_panic_in_the_headless_room_exits_101() {
        let (out, log) = helper("poisoned");
        assert_eq!(out.status.code(), Some(101), "{out:?}");
        assert!(left_early(&out), "the hook exits before the harness reports");
        assert!(log.contains("PANIC thread=headless-room"), "{log}");
    }
}
