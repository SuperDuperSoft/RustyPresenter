//! Crash recovery: the committed frame kept on disk, the supervisor that restarts a crashed app,
//! and the panic log.
pub mod exitcode;
pub mod headless;
pub mod panic;
pub mod state;
pub mod supervisor;

#[path = "../strings/recovery.rs"]
pub mod strings;

use std::path::{Path, PathBuf};

/// `<data>/logs/native-YYYYMMDD.log` for `date`.
pub fn log_path(root: &Path, date: chrono::NaiveDate) -> PathBuf {
    root.join("logs").join(format!("native-{}.log", date.format("%Y%m%d")))
}

/// Start-up's part of crash recovery. On `--restore` the saved frame goes back on air: with its video and
/// audio on the first restart, without them on the second (`attempt` 2), in case they are what crashed;
/// that saved state is then moved aside. A normal start forgets any saved state, so it never restores.
/// Returns the window roles that were open, to reopen.
pub fn restore_on_start(app: &crate::app::App, restore: bool, attempt: u32) -> Vec<String> {
    let root = &app.store.root;
    if !restore {
        state::clear(root);
        return vec![];
    }
    let Some(saved) = state::load(root) else { return vec![] };
    let with_media = attempt < 2;
    if !with_media { state::quarantine(root) }
    app.live.lock().unwrap().restore(&saved, with_media);
    if with_media { app.set_resume(&saved.positions) }
    app.set_notice(if with_media { strings::RESTARTED } else { strings::RESTARTED_NO_MEDIA });
    saved.windows
}

/// A video or song is playing, so its position is worth saving now and then.
pub fn playing(app: &crate::app::App) -> bool {
    let live = app.live.lock().unwrap();
    live.media_status.is_some() || live.audio_status.is_some()
}

/// Saves the frame every 5 s while something plays, so a restart resumes near where it was.
pub async fn save_while_playing(app: std::sync::Arc<crate::app::App>) {
    let mut every = tokio::time::interval(std::time::Duration::from_secs(5));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        every.tick().await;
        if playing(&app) { app.save_live_state() }
    }
}

/// Today's log in the data folder. The folder comes from the environment only: the supervisor
/// reads no saved data.
pub fn todays_log() -> PathBuf {
    let root = crate::server::data_dir().unwrap_or_else(|| PathBuf::from("."));
    log_path(&root, chrono::Local::now().date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_file_is_per_day_in_the_logs_folder() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert_eq!(log_path(Path::new("data"), d), Path::new("data").join("logs").join("native-20261101.log"));
    }

    use crate::{app::{App, test_app}, live::*, model::*};
    use state::LiveState;
    use std::sync::Arc;

    /// A saved state showing "Amazing grace" with a song playing, in the room and stage.
    fn saved() -> LiveState {
        let item_id = uuid::Uuid::new_v4();
        let slide = Slide { label: "Verse 1".into(), content: SlideContent::Text { text: "Amazing grace".into(), footer: None, reference: None }, notes: String::new() };
        let frame = Frame {
            slide: Some(SlideLayer::Text { text: "Amazing grace".into(), footer: None, reference: None, scripture: false }),
            audio: Some(Inst { inst: 7, v: AudioLayer { asset: "a.mp3".into(), title: "Hymn".into(), looped: false, paused: false } }),
            ..Frame::default()
        };
        LiveState {
            v: 1, seq: 3, saved_at: "now".into(), frame,
            cursor: Some(Cursor { item_id, item_index: 0, slide: 0 }),
            item: Some(LiveItem { item_id, title: "Amazing Grace".into(), kind: "song".into(), slides: vec![slide], background: Background::Color { color: "#000".into() } }),
            positions: vec![(7, 12.0)], windows: vec!["room".into(), "stage".into()],
        }
    }

    fn with_saved() -> Arc<App> {
        let app = test_app();
        state::write_with_retry(&state::path(&app.store.root), &serde_json::to_vec(&saved()).unwrap()).unwrap();
        app
    }

    #[test]
    fn restore_puts_the_frame_back_with_media_and_says_so() {
        let app = with_saved();
        assert_eq!(restore_on_start(&app, true, 1), ["room", "stage"]);
        let live = app.live.lock().unwrap();
        assert_eq!(live.frame, saved().frame);
        assert_eq!(live.restored, Some(live.version));
        assert_eq!(live.notice.as_deref(), Some(strings::RESTARTED));
        drop(live);
        assert_eq!(app.resume_position(7), Some(12.0));
        assert!(state::path(&app.store.root).exists(), "kept until the room commits a newer one");
    }

    #[test]
    fn second_attempt_moves_the_state_aside_and_leaves_media_out() {
        let app = with_saved();
        assert_eq!(restore_on_start(&app, true, 2), ["room", "stage"]);
        let live = app.live.lock().unwrap();
        assert_eq!(live.frame.slide, saved().frame.slide);
        assert_eq!(live.frame.audio, None);
        assert_eq!(live.notice.as_deref(), Some(strings::RESTARTED_NO_MEDIA));
        drop(live);
        assert_eq!(app.resume_position(7), None);
        assert!(!state::path(&app.store.root).exists());
        assert!(app.store.root.join("live-state.crashed.json").exists());
    }

    #[test]
    fn normal_start_forgets_the_saved_state() {
        let app = with_saved();
        assert!(restore_on_start(&app, false, 0).is_empty());
        assert!(!state::path(&app.store.root).exists());
        let live = app.live.lock().unwrap();
        assert_eq!((live.version, live.restored, live.notice.clone()), (0, None, None));
    }

    #[test]
    fn restore_with_nothing_saved_changes_nothing() {
        let app = test_app();
        assert!(restore_on_start(&app, true, 1).is_empty());
        let live = app.live.lock().unwrap();
        assert_eq!((live.version, live.restored, live.notice.clone()), (0, None, None));
    }

    #[test]
    fn playing_means_a_video_or_song_reports() {
        let app = test_app();
        assert!(!playing(&app));
        app.live.lock().unwrap().audio_status = Some(PlayStatus { inst: 7, state: PlayState::Playing, position: 1.0, duration: 9.0 });
        assert!(playing(&app));
    }
}
