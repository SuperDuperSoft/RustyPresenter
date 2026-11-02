//! Add dialog logic for the Media tab that needs no window.
use super::{Tone, strings as tr};
use crate::model::MediaStatus;

/// A media row's status chip: its colour, text and tooltip (`statusChip`, `operator.js:464-470`).
pub fn status_chip(status: &MediaStatus) -> (Tone, String, String) {
    match status {
        MediaStatus::Ready => (Tone::Ok, tr::MEDIA_READY.into(), String::new()),
        MediaStatus::Converting { percent } => (Tone::Warn, tr::media_converting(*percent), String::new()),
        MediaStatus::Unconverted { reason } => (Tone::Warn, tr::MEDIA_NEEDS_CONVERTER.into(), reason.clone()),
        MediaStatus::Failed { reason } => (Tone::Bad, tr::MEDIA_FAILED.into(), reason.clone()),
    }
}

/// Whether a media row offers **Convert** (`operator.js:481`).
pub fn can_convert(status: &MediaStatus) -> bool {
    matches!(status, MediaStatus::Unconverted { .. } | MediaStatus::Failed { .. })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_chips_match_the_web() {
        let chip = |s: &MediaStatus| status_chip(s);
        assert_eq!(chip(&MediaStatus::Ready), (Tone::Ok, "Ready".into(), String::new()));
        assert_eq!(chip(&MediaStatus::Converting { percent: 40 }), (Tone::Warn, "Converting 40%".into(), String::new()));
        assert_eq!(chip(&MediaStatus::Unconverted { reason: "no ffmpeg".into() }), (Tone::Warn, "Needs converter".into(), "no ffmpeg".into()));
        assert_eq!(chip(&MediaStatus::Failed { reason: "bad file".into() }), (Tone::Bad, "Failed".into(), "bad file".into()));
    }

    #[test]
    fn convert_button_only_for_unconverted_or_failed() {
        assert!(!can_convert(&MediaStatus::Ready));
        assert!(!can_convert(&MediaStatus::Converting { percent: 1 }));
        assert!(can_convert(&MediaStatus::Unconverted { reason: String::new() }));
        assert!(can_convert(&MediaStatus::Failed { reason: String::new() }));
    }
}
