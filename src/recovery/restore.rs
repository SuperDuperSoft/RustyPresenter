//! What a starting app does with the saved frame: puts it back after a crash, forgets it otherwise.
use super::state;
use crate::app::App;

/// The notice after a restart with everything back.
pub fn restarted() -> String { crate::tr!("RustyPresenter restarted after a problem.") }
/// The notice when what plays was left out.
pub fn restarted_no_media() -> String {
    crate::tr!("RustyPresenter restarted after a problem. Video/audio stopped — click it again to replay.")
}

/// The second restart in a row, or later: what plays may be what crashed.
const WITHOUT_MEDIA_FROM: u32 = 2;

/// Returns the window roles to reopen, or `None` when settings decide as usual.
pub fn at_start(app: &App, restore: bool, attempt: u32) -> Option<Vec<String>> {
    let root = &app.store.root;
    if !restore {
        state::clear(root);
        return None;
    }
    let saved = state::load(root)?;
    let with_media = attempt < WITHOUT_MEDIA_FROM;
    if !with_media { state::quarantine(root); }
    app.live.lock().unwrap().restore(&saved, with_media);
    if with_media { app.set_resume(&saved.positions); }
    app.set_notice(&if with_media { restarted() } else { restarted_no_media() });
    Some(saved.windows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, live::*, model::*, recovery::state::{self, LiveState}};
    use uuid::Uuid;

    /// Puts a text slide on air and saves that as if the app had died there, with audio playing.
    fn saved_with_audio(app: &App) -> LiveState {
        let item_id = Uuid::new_v4();
        let kind = ItemKind::Text { title: "t".into(), body: "Amazing".into(), layout: Default::default() };
        app.data.lock().unwrap().services[0].items.push(ServiceItem { id: item_id, kind, background: None, notes: String::new(), theme: None });
        let out = app.command(Envelope { id: Uuid::new_v4().to_string(), base: None, cmd: Command::Show { item_id, slide: 0 } });
        let version = out.version;
        app.room_event(RoomEvent::Committed { version });
        let mut s = state::snapshot(&app.live.lock().unwrap(), vec!["room".into()], 1).unwrap();
        s.frame.audio = Some(Inst { inst: 40, v: AudioLayer { asset: "a.mp3".into(), title: "A".into(), looped: false, paused: false } });
        s.positions = vec![(40, 12.5)];
        state::write_with_retry(&state::path(&app.store.root), &serde_json::to_vec(&s).unwrap()).unwrap();
        s
    }

    fn fresh(app: &App) { *app.live.lock().unwrap() = Live::new(); }

    #[test]
    fn normal_start_forgets_the_saved_frame() {
        let app = test_app();
        saved_with_audio(&app);
        assert_eq!(at_start(&app, false, 0), None);
        assert!(!state::path(&app.store.root).exists());
    }

    #[test]
    fn first_restart_brings_everything_back() {
        let app = test_app();
        saved_with_audio(&app);
        fresh(&app);
        assert_eq!(at_start(&app, true, 0), Some(vec!["room".to_string()]));
        let live = app.live.lock().unwrap();
        assert!(live.frame.audio.is_some());
        assert_eq!(live.notice.as_deref(), Some(restarted().as_str()));
        drop(live);
        assert_eq!(app.resume_position(40), Some(12.5));
        assert!(state::path(&app.store.root).exists(), "kept: the next crash may need it");
    }

    #[test]
    fn second_restart_leaves_out_what_plays_and_sets_the_state_aside() {
        let app = test_app();
        saved_with_audio(&app);
        fresh(&app);
        assert_eq!(at_start(&app, true, 2), Some(vec!["room".to_string()]));
        let live = app.live.lock().unwrap();
        assert!(live.frame.audio.is_none());
        assert!(live.frame.slide.is_some());
        assert_eq!(live.notice.as_deref(), Some(restarted_no_media().as_str()));
        drop(live);
        assert_eq!(app.resume_position(40), None);
        assert!(!state::path(&app.store.root).exists());
        assert!(app.store.root.join("live-state.crashed.json").exists());
    }

    #[test]
    fn restart_with_nothing_saved_restores_nothing() {
        let app = test_app();
        assert_eq!(at_start(&app, true, 1), None);
        assert!(app.live.lock().unwrap().notice.is_none());
    }

    #[test]
    fn notice_wording_is_the_rooms_restart_wording() {
        assert!(restarted_no_media().ends_with("Video/audio stopped — click it again to replay."));
        assert!(restarted_no_media().starts_with(&restarted()));
    }
}
