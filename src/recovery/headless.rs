//! A room with no window and no player, for the recovery tests (`RUSTYPRESENTER_HEADLESS=1`). It commits
//! every version it is sent and logs what a restore put back, so a test can read it from the log.
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
    let (version, committed) = { let l = app.live.lock().unwrap(); (l.version, l.committed_version) };
    if version > committed { app.room_event(RoomEvent::Committed { version }); }
    if !*logged && let Some(line) = restored_line(&app) {
        *logged = true;
        let _ = writeln!(log, "{line}").and_then(|_| log.flush());
    }
    true
}

/// `restored version={v} slide={json} media={bool}` once the restored frame, or a newer one, is committed.
fn restored_line(app: &App) -> Option<String> {
    let live = app.live.lock().unwrap();
    if live.restored.is_none_or(|r| live.committed_version < r) { return None; }
    let (frame, cursor, item) = live.committed()?;
    let slide = cursor.zip(item).and_then(|(c, i)| match i.slides.get(c.slide)?.content.clone() {
        SlideContent::Text { text, .. } => Some(text),
        _ => None,
    });
    let media = frame.media.as_ref().is_some_and(|m| matches!(m.v, MediaLayer::Video { .. }))
        || frame.background.as_ref().is_some_and(|b| matches!(b.v, Background::Video { .. }))
        || frame.audio.is_some();
    Some(format!("restored version={} slide={} media={media}", live.committed_version, serde_json::to_string(&slide).unwrap_or_default()))
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
        let slide = Slide { label: "Verse 1".into(), content: SlideContent::Text { text: "Amazing grace".into(), footer: None, reference: None }, notes: String::new() };
        let frame = Frame {
            slide: Some(SlideLayer::Text { text: "Amazing grace".into(), footer: None, reference: None, scripture: false }),
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
        let app = test_app();
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
        let app = test_app();
        let version = show(&app, add_text(&app, "Hello"));
        let _room = run(app.clone(), Sink::default());
        assert!(wait_until(|| app.live.lock().unwrap().committed_version == version));
    }

    #[test]
    fn headless_logs_restored_version_and_slide() {
        let app = test_app();
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
    fn headless_logs_a_restore_made_while_it_runs_with_its_media() {
        let app = test_app();
        let log = Sink::default();
        let _room = run(app.clone(), log.clone());
        app.live.lock().unwrap().restore(&saved(true), true);
        app.broadcast_live();
        assert!(wait_until(|| log.text().contains("restored")), "no restore logged: {}", log.text());
        assert!(log.text().contains(r#"slide="Amazing grace" media=true"#), "{}", log.text());
    }

    const HELPER: &str = "recovery::headless::tests::restore_helper";

    /// Restores in a process of its own, with the crash point armed. A panic hook lasts for the whole process.
    fn helper(media: bool) -> Output {
        let log = env::temp_dir().join(format!("rp-headless-{}.log", Uuid::new_v4()));
        Process::new(env::current_exe().unwrap()).args(["--exact", HELPER, "--nocapture"])
            .env("RP_RESTORE_HELPER", &log).env("RP_RESTORE_MEDIA", if media { "1" } else { "0" })
            .env("RUSTYPRESENTER_TEST_CRASH", "panic").env("RUSTYPRESENTER_TEST_CRASH_AT", "restore-media").output().unwrap()
    }

    #[test]
    fn restore_helper() {
        let Some(log) = env::var_os("RP_RESTORE_HELPER") else { return };
        panic::install(log.into());
        panic::mark_critical();
        Live::new().restore(&saved(true), env::var("RP_RESTORE_MEDIA").unwrap() == "1");
    }

    #[test]
    fn restore_media_crash_point_only_with_media() {
        let without = helper(false);
        assert!(without.status.success(), "{without:?}");
        let with = helper(true);
        assert_eq!(with.status.code(), Some(101), "{with:?}");
        // The harness also exits 101 for a failed test; only the hook exits before it reports.
        assert!(!String::from_utf8_lossy(&with.stdout).contains("test result:"), "the hook exits before the harness reports");
    }
}
