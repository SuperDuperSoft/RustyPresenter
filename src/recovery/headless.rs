//! A stand-in for the room in recovery tests: it commits the newest version it sees, and a restored one
//! before it, with no Slint and no mpv. Used only when `RUSTYPRESENTER_HEADLESS=1`, but compiled in every build.
use crate::{app::{App, Monitor, Screens}, live::{Live, RoomEvent, SlideLayer}};
use std::{io::Write, sync::Arc, thread::JoinHandle};
use tokio::sync::broadcast::error::RecvError;

/// No windows. Asking for the operator succeeds, so a second launch finds a running copy.
pub struct NoScreens;

impl Screens for NoScreens {
    fn open(&self, _role: &str, _monitor: Option<&str>) -> Result<(), String> { Ok(()) }
    fn close(&self, _role: &str) {}
    fn open_roles(&self) -> Vec<String> { vec![] }
    fn monitors(&self) -> Vec<Monitor> { vec![] }
}

/// Follows the live state on a thread of its own. Subscribes before the thread starts and looks at
/// the state before the first wait, so a restore made before this call is still seen.
pub fn run(app: Arc<App>, mut log: impl Write + Send + 'static) -> JoinHandle<()> {
    let mut updates = app.subscribe();
    let body = move || {
        // It stands in for the room, so a panic here must restart the app as one there would.
        crate::recovery::panic::mark_critical();
        let mut logged = false;
        loop {
            // The restored frame goes on air first, even with a newer one waiting, so the log names it.
            let (version, restored) = { let live = app.live.lock().unwrap(); (live.version, live.restored_version) };
            if let Some(r) = restored { commit(&app, r); }
            if !logged && let Some(line) = restored_line(&app.live.lock().unwrap()) {
                let _ = writeln!(log, "{line}").and_then(|()| log.flush());
                logged = true;
            }
            commit(&app, version);
            match updates.blocking_recv() {
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            }
        }
    };
    std::thread::Builder::new().name("headless-room".into()).spawn(body).expect("the headless room thread")
}

fn commit(app: &App, version: u64) {
    if version > app.live.lock().unwrap().committed_version { app.room_event(RoomEvent::Committed { version }); }
}

/// The line for a restore that has reached the screen, or `restore superseded` if something newer went on air
/// without it.
fn restored_line(live: &Live) -> Option<String> {
    let restored = live.restored_version?;
    if live.committed_version < restored { return None }
    if live.committed_version > restored { return Some(format!("restore superseded version={restored} committed={}", live.committed_version)) }
    let (frame, ..) = live.committed()?;
    let text = match &frame.slide { Some(SlideLayer::Text { text, .. }) => Some(text), _ => None };
    Some(format!("restored version={} slide={} media={}", live.committed_version, serde_json::to_string(&text).ok()?, frame.plays()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, live::*, model::*, recovery::{panic, state::LiveState}};
    use std::{env, io, process::{Command as Process, Output}, sync::Mutex, time::{Duration, Instant}};
    use uuid::Uuid;

    #[derive(Clone, Default)]
    struct Log(Arc<Mutex<Vec<u8>>>);

    impl Log {
        fn text(&self) -> String { String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned() }
    }

    impl Write for Log {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> { self.0.lock().unwrap().extend_from_slice(b); Ok(b.len()) }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    fn until(what: &str, done: impl Fn() -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < end, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn committed(app: &App) -> u64 { app.live.lock().unwrap().committed_version }

    fn text_item(app: &App, text: &str) -> Uuid {
        let id = Uuid::new_v4();
        let kind = ItemKind::Text { title: "t".into(), body: text.into(), layout: Default::default() };
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id, kind, background: None, notes: String::new(), theme: None });
        id
    }

    fn show(app: &App, item_id: Uuid) -> u64 {
        let out = app.command(Envelope::new(Command::Show { item_id, slide: 0 }));
        assert!(out.ok, "{out:?}");
        out.version
    }

    fn saved(frame: Frame) -> LiveState {
        LiveState { v: 1, seq: 1, saved_at: "now".into(), frame, cursor: None, item: None, positions: vec![], windows: vec![], clear_epoch: 0, next_inst: 0 }
    }

    fn slide(text: &str) -> Option<SlideLayer> {
        Some(SlideLayer::Text { text: text.into(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: None })
    }

    fn audio() -> Option<Inst<AudioLayer>> {
        Some(Inst { inst: 4, v: AudioLayer { asset: "a.mp3".into(), title: "a".into(), looped: false, paused: false } })
    }

    #[test]
    fn headless_commits_each_version() {
        let app = test_app();
        run(app.clone(), Log::default());
        let (a, b) = (text_item(&app, "Alpha"), text_item(&app, "Bravo"));
        let first = show(&app, a);
        until("the first commit", || committed(&app) == first);
        let second = show(&app, b);
        assert!(second > first);
        until("the second commit", || committed(&app) == second);
    }

    #[test]
    fn headless_logs_nothing_without_a_restore() {
        let app = test_app();
        let log = Log::default();
        run(app.clone(), log.clone());
        let version = show(&app, text_item(&app, "Alpha"));
        until("the commit", || committed(&app) == version);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(log.text(), "");
    }

    #[test]
    fn headless_logs_restored_version_and_slide() {
        let app = test_app();
        let log = Log::default();
        run(app.clone(), log.clone());
        let frame = Frame { slide: slide("Grace \"and\" peace"), ..Frame::default() };
        let version = { let mut live = app.live.lock().unwrap(); live.restore(&saved(frame), true); live.version };
        app.broadcast_live();
        until("the line", || log.text().contains('\n'));
        assert_eq!(log.text(), format!("restored version={version} slide=\"Grace \\\"and\\\" peace\" media=false\n"));
    }

    /// The real start-up order: the restore is done before the room is running.
    #[test]
    fn headless_finds_a_restore_made_before_it_started() {
        let app = test_app();
        let frame = Frame { slide: slide("Alpha"), audio: audio(), ..Frame::default() };
        let version = { let mut live = app.live.lock().unwrap(); live.restore(&saved(frame), true); live.version };
        let log = Log::default();
        run(app.clone(), log.clone());
        until("the line", || log.text().contains('\n'));
        assert_eq!(log.text(), format!("restored version={version} slide=\"Alpha\" media=true\n"));
    }

    #[test]
    fn headless_logs_a_restore_once() {
        let app = test_app();
        let log = Log::default();
        app.live.lock().unwrap().restore(&saved(Frame { slide: slide("Alpha"), ..Frame::default() }), true);
        run(app.clone(), log.clone());
        until("the line", || log.text().contains('\n'));
        let version = show(&app, text_item(&app, "Bravo"));
        until("the commit", || committed(&app) == version);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(log.text().lines().count(), 1, "{}", log.text());
    }

    #[test]
    fn a_picture_slide_logs_null() {
        let app = test_app();
        let log = Log::default();
        let frame = Frame { slide: Some(SlideLayer::Image { asset: "s.png".into(), transition: None }), ..Frame::default() };
        app.live.lock().unwrap().restore(&saved(frame), true);
        run(app.clone(), log.clone());
        until("the line", || log.text().contains('\n'));
        assert!(log.text().contains(" slide=null media=false\n"), "{}", log.text());
    }

    #[test]
    fn headless_logs_the_restored_frame_even_when_a_newer_one_waits() {
        let app = test_app();
        let restored = { let mut live = app.live.lock().unwrap(); live.restore(&saved(Frame { slide: slide("Alpha"), ..Frame::default() }), true); live.version };
        let newer = show(&app, text_item(&app, "Bravo"));
        let log = Log::default();
        run(app.clone(), log.clone());
        until("the newer commit", || committed(&app) == newer);
        until("the line", || log.text().contains('\n'));
        assert_eq!(log.text(), format!("restored version={restored} slide=\"Alpha\" media=false\n"));
    }

    #[test]
    fn a_restore_that_never_reached_the_screen_says_so() {
        let mut live = Live::new();
        live.restore(&saved(Frame { slide: slide("Alpha"), ..Frame::default() }), true);
        let restored = live.version;
        live.committed_version = restored + 1;
        assert_eq!(restored_line(&live).unwrap(), format!("restore superseded version={restored} committed={}", restored + 1));
    }

    #[test]
    fn crash_helper() {
        let Some(mode) = env::var("RP_HEADLESS_HELPER").ok() else { return };
        let log = std::path::PathBuf::from(env::var_os("RP_HEADLESS_LOG").unwrap());
        panic::install(log.clone());
        if mode == "poisoned" {
            // The process exits from the hook, so its data folder sits by the log for `crash_run` to remove.
            let store = crate::store::Store::open(log.with_extension("data")).unwrap();
            let data = store.load().unwrap();
            let app = App::new(store, data, 0, None);
            // Another thread dies holding the live state, so the room's next lock panics.
            let poisoner = app.clone();
            assert!(std::thread::spawn(move || { let _l = poisoner.live.lock().unwrap(); panic!("poison") }).join().is_err());
            let _ = run(app, Log::default()).join();
            return;
        }
        panic::mark_critical();
        let frame = Frame { slide: slide("Alpha"), audio: audio(), ..Frame::default() };
        Live::new().restore(&saved(frame), mode == "with");
    }

    /// Runs `crash_helper` in a process of its own, as a panic hook lasts for the whole process. Also returns its
    /// panic log.
    fn crash_run(mode: &str) -> (Output, String) {
        let log = env::temp_dir().join(format!("rp-headless-{}.log", Uuid::new_v4()));
        let out = Process::new(env::current_exe().unwrap()).args(["--exact", "recovery::headless::tests::crash_helper", "--nocapture"])
            .env("RP_HEADLESS_HELPER", mode).env("RP_HEADLESS_LOG", &log)
            .env("RUSTYPRESENTER_TEST_CRASH", "panic").env("RUSTYPRESENTER_TEST_CRASH_AT", "restore-media")
            .output().unwrap();
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_dir_all(log.with_extension("data"));
        (out, text)
    }

    /// The harness also exits 101 for a failed test; only the hook exits before it reports.
    fn left_early(out: &Output) -> bool { !String::from_utf8_lossy(&out.stdout).contains("test result:") }

    #[test]
    fn restore_media_crash_point_only_with_media() {
        let (without, log) = crash_run("without");
        assert_eq!(without.status.code(), Some(0), "{}", String::from_utf8_lossy(&without.stderr));
        assert!(log.is_empty(), "{log}");
        let (with, log) = crash_run("with");
        assert_eq!(with.status.code(), Some(crate::recovery::exitcode::PANIC));
        assert!(left_early(&with), "the hook exits before the harness reports");
        assert!(log.contains("test crash at restore-media"), "{log}");
    }

    #[test]
    fn a_panic_in_the_headless_room_restarts_the_app() {
        let (out, log) = crash_run("poisoned");
        assert_eq!(out.status.code(), Some(crate::recovery::exitcode::PANIC), "{out:?}");
        assert!(left_early(&out), "the hook exits before the harness reports");
        assert!(log.contains("headless-room"), "{log}");
    }
}
