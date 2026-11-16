//! Livestream scenes (1.3b, design `docs/superpowers/specs/2026-11-15-livestream-scenes-design.md`): what the
//! stream shows, and the automatic choice of it from what is live. Pure, so all tested here.
use crate::tr;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What the stream shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scene {
    /// The camera fills the picture.
    Camera,
    /// The camera, with the live slide's words as a lower third.
    CameraWords,
    /// What the room shows.
    #[default]
    Screen,
    /// The room screen, with the camera small in the bottom right corner.
    ScreenCamera,
    /// A card before the service, with the pre-service countdown.
    StartingSoon,
    /// A card after the service.
    Ending,
    /// A card for a break ("We'll be right back").
    Hold,
}

impl Scene {
    /// In the order of the buttons.
    pub const ALL: [Scene; 7] = [Scene::Camera, Scene::CameraWords, Scene::Screen, Scene::ScreenCamera, Scene::StartingSoon, Scene::Ending, Scene::Hold];

    pub fn label(self) -> String {
        match self {
            Scene::Camera => tr!("Camera"),
            Scene::CameraWords => tr!("Camera + words"),
            Scene::Screen => tr!("Full screen"),
            Scene::ScreenCamera => tr!("Screen + camera"),
            Scene::StartingSoon => tr!("Starting soon"),
            Scene::Ending => tr!("Ending"),
            Scene::Hold => tr!("Hold"),
        }
    }

    /// Whether the scene shows the camera.
    pub fn needs_camera(self) -> bool { matches!(self, Scene::Camera | Scene::CameraWords | Scene::ScreenCamera) }

    /// Whether the scene is a card (a picture or words of its own).
    pub fn is_card(self) -> bool { matches!(self, Scene::StartingSoon | Scene::Ending | Scene::Hold) }
}

/// What the live slide is, as far as the stream cares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlideKind {
    /// No slide, or one with no words.
    #[default]
    None,
    /// A text slide with words (a song, a verse, notes).
    Words,
    /// A picture or a designed slide (presentations, announcement pictures).
    Picture,
}

/// What `choose` needs to know about what is live.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveInfo {
    /// The pre-service countdown is running.
    pub pre_service: bool,
    /// The service item live now; `None` before anything was taken live.
    pub cursor_item: Option<Uuid>,
    /// The live item is the service's last.
    pub last_item: bool,
    /// A video is playing on the media layer.
    pub media_playing: bool,
    pub slide: SlideKind,
}

/// Settings → Livestream: the automatic choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoPrefs {
    pub on: bool,
    /// Presentations show the room screen with the camera in the corner, not the room screen alone.
    pub presentation_camera: bool,
}

impl Default for AutoPrefs {
    fn default() -> Self { AutoPrefs { on: true, presentation_camera: false } }
}

/// The operator's choice (a scene button, a slide action): `scene` `None` is Automatic. It holds while `item` is
/// the live item.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub scene: Option<Scene>,
    pub item: Option<Uuid>,
}

impl Choice {
    pub fn pick(scene: Option<Scene>, item: Option<Uuid>) -> Choice { Choice { scene, item } }

    /// The chosen scene, while it still holds for what is live.
    fn holding(self, info: &LiveInfo) -> Option<Scene> { self.scene.filter(|_| self.item == info.cursor_item) }
}

/// The scene for what is live now. `last`: the scene going out (kept when automatic scenes are off).
pub fn choose(info: &LiveInfo, prefs: AutoPrefs, choice: Choice, last: Scene) -> Scene {
    if let Some(s) = choice.holding(info) { return s }
    if !prefs.on { return last }
    if info.pre_service || info.cursor_item.is_none() { return Scene::StartingSoon }
    if info.media_playing { return Scene::Screen }
    match info.slide {
        SlideKind::Words => Scene::CameraWords,
        SlideKind::Picture if prefs.presentation_camera => Scene::ScreenCamera,
        SlideKind::Picture => Scene::Screen,
        SlideKind::None if info.last_item => Scene::Ending,
        SlideKind::None => Scene::Camera,
    }
}

/// `scene` as it can go out: a camera scene with no camera picture shows the room screen instead.
pub fn effective(scene: Scene, camera: bool) -> Scene {
    if scene.needs_camera() && !camera { Scene::Screen } else { scene }
}

/// The scene button to light: the operator's choice while it holds, `None` for Automatic.
pub fn lit(choice: Choice, info: &LiveInfo) -> Option<Scene> { choice.holding(info) }

#[cfg(test)]
mod tests {
    use super::*;

    fn at(slide: SlideKind) -> LiveInfo { LiveInfo { cursor_item: Some(Uuid::from_u128(1)), slide, ..LiveInfo::default() } }
    fn auto(info: &LiveInfo) -> Scene { choose(info, AutoPrefs::default(), Choice::default(), Scene::Screen) }

    #[test]
    fn starting_soon_before_anything_and_while_the_countdown_runs() {
        assert_eq!(auto(&LiveInfo::default()), Scene::StartingSoon);
        assert_eq!(auto(&LiveInfo { pre_service: true, ..at(SlideKind::Words) }), Scene::StartingSoon);
    }

    #[test]
    fn words_get_the_camera_and_words() {
        assert_eq!(auto(&at(SlideKind::Words)), Scene::CameraWords);
    }

    #[test]
    fn a_video_goes_out_full() {
        assert_eq!(auto(&LiveInfo { media_playing: true, ..at(SlideKind::Words) }), Scene::Screen);
    }

    #[test]
    fn pictures_follow_the_presentation_setting() {
        assert_eq!(auto(&at(SlideKind::Picture)), Scene::Screen);
        let corner = AutoPrefs { presentation_camera: true, ..AutoPrefs::default() };
        assert_eq!(choose(&at(SlideKind::Picture), corner, Choice::default(), Scene::Screen), Scene::ScreenCamera);
    }

    #[test]
    fn nothing_on_the_slide_is_the_camera() {
        assert_eq!(auto(&at(SlideKind::None)), Scene::Camera);
        assert_eq!(auto(&LiveInfo { last_item: true, ..at(SlideKind::None) }), Scene::Ending);
    }

    #[test]
    fn a_click_holds_until_the_next_item() {
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let held = Choice::pick(Some(Scene::Hold), Some(a));
        let on_a = at(SlideKind::Words);
        assert_eq!(choose(&on_a, AutoPrefs::default(), held, Scene::Screen), Scene::Hold);
        assert_eq!(lit(held, &on_a), Some(Scene::Hold));
        let on_b = LiveInfo { cursor_item: Some(b), ..on_a };
        assert_eq!(choose(&on_b, AutoPrefs::default(), held, Scene::Hold), Scene::CameraWords);
        assert_eq!(lit(held, &on_b), None);
    }

    #[test]
    fn auto_off_keeps_the_last_scene() {
        let off = AutoPrefs { on: false, ..AutoPrefs::default() };
        assert_eq!(choose(&at(SlideKind::Words), off, Choice::default(), Scene::Camera), Scene::Camera);
    }

    #[test]
    fn no_camera_falls_back_to_the_room() {
        assert_eq!(effective(Scene::CameraWords, false), Scene::Screen);
        assert_eq!(effective(Scene::StartingSoon, false), Scene::StartingSoon);
        assert_eq!(effective(Scene::Camera, true), Scene::Camera);
    }

    #[test]
    fn scene_names_are_kept_by_name() {
        assert_eq!(serde_json::to_value(Scene::CameraWords).unwrap(), serde_json::json!("camera_words"));
        assert_eq!(serde_json::from_value::<Scene>(serde_json::json!("starting_soon")).unwrap(), Scene::StartingSoon);
    }
}
