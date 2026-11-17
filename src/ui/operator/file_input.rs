//! How files reach the operator: the desktop drop batch (Task 15) and the file picker's filters. Pure, so it is
//! tested without a window; the winit filter and `rfd` call these.
use super::{files::all_images, strings as tr};
use crate::import::media;
use std::path::PathBuf;

/// What one window event means for the drop, with the winit types left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropEvent { Hovered, Cancelled, Dropped(PathBuf) }

/// What the window must do after an event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DropStep {
    /// `Some` when the "Drop to add" hint must change.
    pub hint: Option<bool>,
    /// True on the first file of a batch only: start one zero timer, then call `take` from it.
    pub start_timer: bool,
}

/// winit sends one `DroppedFile` per file. This collects the files of one turn so they are handled together.
#[derive(Debug, Default)]
pub struct DropBatch { paths: Vec<PathBuf>, hint: bool }

impl DropBatch {
    pub fn on_event(&mut self, event: DropEvent) -> DropStep {
        match event {
            DropEvent::Hovered => DropStep { hint: self.set_hint(true), start_timer: false },
            DropEvent::Cancelled => DropStep { hint: self.set_hint(false), start_timer: false },
            DropEvent::Dropped(path) => {
                // A drop ends the hover and winit sends no `HoveredFileCancelled` after it.
                let hint = self.set_hint(false);
                self.paths.push(path);
                DropStep { hint, start_timer: self.paths.len() == 1 }
            }
        }
    }

    pub fn take(&mut self) -> Vec<PathBuf> { std::mem::take(&mut self.paths) }

    fn set_hint(&mut self, on: bool) -> Option<bool> {
        let changed = self.hint != on;
        self.hint = on;
        changed.then_some(on)
    }
}

/// What to do with a finished batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropPlan {
    /// Confirm "Make N pictures into one set of slides?", then `images_to_deck` or `add_files`.
    AskOneDeck(Vec<PathBuf>),
    AddFiles(Vec<PathBuf>),
}

pub fn drop_plan(paths: Vec<PathBuf>) -> Option<DropPlan> {
    match paths.len() {
        0 => None,
        n if n > 1 && all_images(&paths) => Some(DropPlan::AskOneDeck(paths)),
        _ => Some(DropPlan::AddFiles(paths)),
    }
}

/// The web `accept` lists (`operator.js:413,476,491,492,726,900,948`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter { Media, PowerPoint, Lyrics, Pictures, PicturesAndVideos, Bibles, Services, Csv, WebPage, Translation, Text, Playlist, ThemePack, Pptx, Recording, Videos }

impl Filter {
    pub fn name(self) -> String {
        match self {
            Filter::Media => tr::filter_media(),
            Filter::PowerPoint => tr::filter_powerpoint(),
            Filter::Lyrics => tr::filter_lyrics(),
            Filter::Pictures => tr::filter_pictures(),
            Filter::PicturesAndVideos => tr::filter_pictures_and_videos(),
            Filter::Bibles => tr::filter_bibles(),
            Filter::Services => tr::filter_services(),
            Filter::Csv => crate::usage::strings::filter_csv(),
            Filter::WebPage => crate::print::strings::filter_page(),
            Filter::Translation => crate::tr!("Translation file (.po)"),
            Filter::Text => crate::tr!("Text file (.txt)"),
            Filter::Playlist => crate::tr!("Playlist (.m3u8)"),
            Filter::ThemePack => crate::tr!("Theme pack (.rpthemes)"),
            Filter::Pptx => crate::tr!("PowerPoint (.pptx)"),
            Filter::Recording => crate::tr!("Recording (video or sound)"),
            Filter::Videos => crate::tr!("Video"),
        }
    }

    pub fn extensions(self) -> Vec<&'static str> {
        match self {
            Filter::Media => [media::VIDEO, media::AUDIO, media::IMAGE].concat(),
            Filter::PowerPoint => crate::import::deck::DECK_EXT.to_vec(),
            Filter::Lyrics => crate::songselect::inbox::EXTENSIONS.to_vec(),
            Filter::Pictures => media::IMAGE.to_vec(),
            Filter::PicturesAndVideos => [media::IMAGE, media::VIDEO].concat(),
            Filter::Bibles => vec!["xml", "usx", "osis", "zef"],
            Filter::Services => vec![crate::bundle::EXT],
            Filter::Csv => vec!["csv"],
            Filter::WebPage => vec!["html"],
            Filter::Translation => vec!["po"],
            Filter::Text => vec!["txt"],
            Filter::Playlist => vec![crate::transfer::PLAYLIST_EXT],
            Filter::ThemePack => vec![crate::transfer::THEME_EXT],
            Filter::Pptx => crate::import::pptx_text::EXT.to_vec(),
            Filter::Recording => [media::VIDEO, media::AUDIO].concat(),
            Filter::Videos => media::VIDEO.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> { names.iter().map(PathBuf::from).collect() }

    fn dropped(name: &str) -> DropEvent { DropEvent::Dropped(PathBuf::from(name)) }

    #[test]
    fn drop_batch_collects_files_from_one_turn() {
        let mut b = DropBatch::default();
        let starts: Vec<bool> = ["a.mp4", "b.png", "c.txt"].map(|f| b.on_event(dropped(f)).start_timer).into();
        assert_eq!(starts, [true, false, false], "one timer for the batch");
        assert_eq!(b.take(), paths(&["a.mp4", "b.png", "c.txt"]));
    }

    #[test]
    fn the_next_drop_starts_a_new_batch() {
        let mut b = DropBatch::default();
        b.on_event(dropped("a.mp4"));
        b.take();
        assert!(b.take().is_empty(), "a batch is handed over once");
        assert!(b.on_event(dropped("b.mp4")).start_timer);
        assert_eq!(b.take(), paths(&["b.mp4"]));
    }

    #[test]
    fn hovering_shows_the_hint_once() {
        let mut b = DropBatch::default();
        assert_eq!(b.on_event(DropEvent::Hovered), DropStep { hint: Some(true), start_timer: false });
        assert_eq!(b.on_event(DropEvent::Hovered).hint, None, "winit sends one per file");
    }

    #[test]
    fn cancelling_hides_the_hint() {
        let mut b = DropBatch::default();
        b.on_event(DropEvent::Hovered);
        assert_eq!(b.on_event(DropEvent::Cancelled).hint, Some(false));
        assert_eq!(b.on_event(DropEvent::Cancelled).hint, None);
    }

    #[test]
    fn dropping_hides_the_hint_because_no_cancel_follows() {
        let mut b = DropBatch::default();
        b.on_event(DropEvent::Hovered);
        assert_eq!(b.on_event(dropped("a.mp4")), DropStep { hint: Some(false), start_timer: true });
        assert_eq!(b.on_event(dropped("b.mp4")), DropStep { hint: None, start_timer: false });
    }

    #[test]
    fn cancelling_keeps_files_already_dropped() {
        let mut b = DropBatch::default();
        b.on_event(dropped("a.mp4"));
        b.on_event(DropEvent::Cancelled);
        assert_eq!(b.take(), paths(&["a.mp4"]));
    }

    #[test]
    fn many_pictures_dropped_ask_once() {
        let files = paths(&["1.png", "2.jpg", "3.webp"]);
        assert_eq!(drop_plan(files.clone()), Some(DropPlan::AskOneDeck(files)));
    }

    #[test]
    fn one_picture_is_just_added() {
        let files = paths(&["1.png"]);
        assert_eq!(drop_plan(files.clone()), Some(DropPlan::AddFiles(files)));
    }

    #[test]
    fn a_mixed_drop_is_just_added() {
        let files = paths(&["1.png", "2.mp4"]);
        assert_eq!(drop_plan(files.clone()), Some(DropPlan::AddFiles(files)));
    }

    #[test]
    fn an_empty_batch_does_nothing() { assert_eq!(drop_plan(Vec::new()), None); }

    #[test]
    fn deck_question_uses_the_web_wording() {
        assert_eq!(tr::pictures_to_deck(3), "Make 3 pictures into one set of slides?");
    }

    #[test]
    fn media_filter_has_the_web_extras() {
        let e = Filter::Media.extensions();
        for x in ["mkv", "mov", "wmv", "avi", "mts", "m2ts", "mpg", "mpeg", "flv", "vob", "wma", "flac", "aiff"] {
            assert!(e.contains(&x), "{x}");
        }
    }

    #[test]
    fn media_filter_covers_video_audio_and_pictures_once() {
        let e = Filter::Media.extensions();
        for x in media::VIDEO.iter().chain(media::AUDIO).chain(media::IMAGE) { assert!(e.contains(x), "{x}"); }
        let mut unique = e.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), e.len(), "no extension twice");
    }

    #[test]
    fn exact_filters_match_the_web() {
        assert_eq!(Filter::PowerPoint.extensions(), crate::import::deck::DECK_EXT);
        assert_eq!(Filter::Lyrics.extensions(), ["txt"]);
        assert_eq!(Filter::Pictures.extensions(), media::IMAGE);
        assert_eq!(Filter::Bibles.extensions(), ["xml", "usx", "osis", "zef"]);
    }

    #[test]
    fn pictures_and_videos_filter_is_both() {
        assert_eq!(Filter::PicturesAndVideos.extensions(), [media::IMAGE, media::VIDEO].concat());
    }

    #[test]
    fn every_filter_has_a_name() {
        for f in [Filter::Media, Filter::PowerPoint, Filter::Lyrics, Filter::Pictures, Filter::PicturesAndVideos, Filter::Bibles] {
            assert!(!f.name().is_empty(), "{f:?}");
        }
    }
}
