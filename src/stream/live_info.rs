//! What the livestream's scene choice needs from the App (`stream::scene::LiveInfo`), and the operator's choice of
//! scene (a scene button, a slide action, a macro), kept on the App so all of them share it.
use super::scene::{Choice, LiveInfo, Scene, SlideKind};
use crate::app::App;
use crate::automation::preservice::PreRun;
use crate::live::{PlayState, SlideLayer};
use crate::timers::PRE_SERVICE_TIMER_ID;

impl App {
    /// What is live, the operator's scene choice, and the pre-service countdown as text while it runs.
    pub fn stream_info(&self) -> (LiveInfo, Choice, Option<String>) {
        let (info, pre) = {
            let data = self.data.lock().unwrap();
            let live = self.live.lock().unwrap();
            let pre: Option<PreRun> = self.automation.lock().unwrap().pre.clone();
            let cursor_item = live.cursor.as_ref().map(|c| c.item_id);
            // The last item that shows anything: headers and unfilled blanks after it do not count.
            let last_item = cursor_item.is_some_and(|id| data.service().is_some_and(|s| {
                s.items.iter().rev().find(|i| !matches!(i.kind, crate::model::ItemKind::Header { .. } | crate::model::ItemKind::Blank { .. })).is_some_and(|i| i.id == id)
            }));
            let media_playing = live.frame.media.is_some()
                && !live.media_status.as_ref().is_some_and(|s| matches!(s.state, PlayState::Ended | PlayState::Error));
            let slide = match &live.frame.slide {
                Some(SlideLayer::Text { text, .. }) if !text.trim().is_empty() => SlideKind::Words,
                Some(SlideLayer::Image { .. } | SlideLayer::Design { .. }) => SlideKind::Picture,
                _ => SlideKind::None,
            };
            // Brought back by a restart and not touched by hand since.
            let restored = live.restored_version.is_some_and(|r| live.manual_version <= r);
            (LiveInfo { pre_service: pre.is_some(), cursor_item, restored, last_item, media_playing, slide }, pre)
        };
        let countdown = pre.and_then(|_| self.timers().timers.iter().find(|t| t.id == PRE_SERVICE_TIMER_ID).map(|t| crate::timers::text(t.value_ms)));
        (info, *self.stream_choice.lock().unwrap(), countdown)
    }

    /// The operator picks `scene` (`None`: Automatic) for the live item.
    pub fn set_stream_scene(&self, scene: Option<Scene>) {
        let item = self.live.lock().unwrap().cursor.as_ref().map(|c| c.item_id);
        *self.stream_choice.lock().unwrap() = Choice::pick(scene, item);
    }

    /// A new livestream starts on Automatic.
    pub fn clear_stream_choice(&self) { *self.stream_choice.lock().unwrap() = Choice::default() }
}

#[cfg(test)]
mod tests {
    use crate::app::{App, test_app};
    use crate::live::{Command, Envelope};
    use crate::stream::scene::{Choice, Scene, SlideKind};
    use std::sync::Arc;
    use uuid::Uuid;

    fn block_on<F: std::future::Future>(f: F) -> F::Output { tokio::runtime::Runtime::new().unwrap().block_on(f) }
    fn text_item(app: &Arc<App>, title: &str) -> Uuid {
        let item = serde_json::json!({ "kind": "text", "title": title, "body": format!("{title} words") });
        let r = block_on(app.do_action("service/add", serde_json::json!({ "item": item }).to_string().as_bytes())).unwrap();
        serde_json::from_value(r["id"].clone()).unwrap()
    }
    fn cmd(app: &App, cmd: Command) {
        let base = Some(app.live.lock().unwrap().version);
        assert!(app.command(Envelope { id: Uuid::new_v4().to_string(), base, cmd }).ok);
    }

    #[test]
    fn stream_info_reads_the_live_slide() {
        let app = test_app();
        assert_eq!(app.stream_info().0.cursor_item, None, "nothing live yet");
        let a = text_item(&app, "Welcome");
        cmd(&app, Command::Show { item_id: a, slide: 0 });
        let (info, choice, countdown) = app.stream_info();
        assert_eq!((info.cursor_item, info.slide, info.media_playing, info.pre_service), (Some(a), SlideKind::Words, false, false));
        assert_eq!((choice, countdown), (Choice::default(), None));
    }

    #[test]
    fn a_manual_scene_is_kept_for_the_live_item() {
        let app = test_app();
        let a = text_item(&app, "Sermon");
        cmd(&app, Command::Show { item_id: a, slide: 0 });
        app.set_stream_scene(Some(Scene::Camera));
        assert_eq!(app.stream_info().1, Choice { scene: Some(Scene::Camera), item: Some(a) });
        app.clear_stream_choice();
        assert_eq!(app.stream_info().1, Choice::default());
    }

    #[test]
    fn a_restored_slide_counts_as_not_live_until_touched() {
        let app = test_app();
        let (a, b) = (text_item(&app, "One"), text_item(&app, "Two"));
        cmd(&app, Command::Show { item_id: a, slide: 0 });
        { let mut live = app.live.lock().unwrap(); live.restored_version = Some(live.version); }
        assert!(app.stream_info().0.restored, "brought back by a restart");
        cmd(&app, Command::Show { item_id: b, slide: 0 });
        assert!(!app.stream_info().0.restored, "touched since");
    }

    #[test]
    fn headers_and_blanks_after_the_last_item_do_not_count() {
        let app = test_app();
        let a = text_item(&app, "Closing song");
        for item in [serde_json::json!({ "kind": "header", "title": "Post-service" }), serde_json::json!({ "kind": "blank", "label": "Maybe a video" })] {
            block_on(app.do_action("service/add", serde_json::json!({ "item": item }).to_string().as_bytes())).unwrap();
        }
        cmd(&app, Command::Show { item_id: a, slide: 0 });
        assert!(app.stream_info().0.last_item, "only a header and a blank follow it");
    }

    #[test]
    fn the_last_item_is_known() {
        let app = test_app();
        let (a, b) = (text_item(&app, "One"), text_item(&app, "Two"));
        cmd(&app, Command::Show { item_id: a, slide: 0 });
        assert!(!app.stream_info().0.last_item);
        cmd(&app, Command::Show { item_id: b, slide: 0 });
        cmd(&app, Command::ClearSlide);
        let info = app.stream_info().0;
        assert_eq!((info.last_item, info.slide), (true, SlideKind::None));
    }
}
