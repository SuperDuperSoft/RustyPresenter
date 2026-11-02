//! Screens dialog logic that needs no window (`openScreens`, `operator.js:816-845`).
use super::{Tone, strings as tr};
use crate::{app::Monitor, ui::operator::view::RoomStatus};

/// The monitor a card starts on: the saved one if it is still connected, else the first that is not the primary
/// screen, else none, which disables "Show on this screen".
pub fn default_pick(monitors: &[Monitor], saved: Option<&str>) -> Option<String> {
    let saved = monitors.iter().find(|m| Some(m.id.as_str()) == saved);
    saved.or_else(|| monitors.iter().find(|m| !m.primary)).map(|m| m.id.clone())
}

pub fn status(connected: bool, open: bool) -> RoomStatus {
    if connected { RoomStatus::Showing } else if open { RoomStatus::Starting } else { RoomStatus::Off }
}

pub fn status_chip(status: RoomStatus) -> (Tone, &'static str) {
    match status {
        RoomStatus::Showing => (Tone::Ok, tr::SCREEN_SHOWING),
        RoomStatus::Starting => (Tone::Warn, tr::SCREEN_STARTING),
        RoomStatus::Off => (Tone::Plain, tr::SCREEN_OFF),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(id: &str, primary: bool) -> Monitor {
        Monitor { id: id.into(), label: id.into(), x: 0, y: 0, width: 1920, height: 1080, primary }
    }

    #[test]
    fn default_pick_prefers_saved_then_non_primary() {
        let all = [monitor("laptop", true), monitor("tv", false), monitor("stage", false)];
        assert_eq!(default_pick(&all, Some("stage")).as_deref(), Some("stage"));
        assert_eq!(default_pick(&all, None).as_deref(), Some("tv"));
    }

    #[test]
    fn a_saved_monitor_that_is_gone_falls_back() {
        let all = [monitor("laptop", true), monitor("tv", false)];
        assert_eq!(default_pick(&all, Some("unplugged")).as_deref(), Some("tv"));
    }

    #[test]
    fn the_saved_primary_screen_is_still_chosen() {
        let all = [monitor("laptop", true), monitor("tv", false)];
        assert_eq!(default_pick(&all, Some("laptop")).as_deref(), Some("laptop"));
    }

    #[test]
    fn one_monitor_nothing_saved_disables_show() {
        assert_eq!(default_pick(&[monitor("laptop", true)], None), None);
        assert_eq!(default_pick(&[monitor("laptop", true)], Some("unplugged")), None);
        assert_eq!(default_pick(&[], None), None);
    }

    #[test]
    fn screens_status_chips() {
        assert_eq!(status_chip(status(true, false)), (Tone::Ok, "Showing"));
        assert_eq!(status_chip(status(true, true)), (Tone::Ok, "Showing"));
        assert_eq!(status_chip(status(false, true)), (Tone::Warn, "Starting…"));
        assert_eq!(status_chip(status(false, false)), (Tone::Plain, "Off"));
    }
}
