//! Safe mode: when two starts in a row fail or crash within their first 20 seconds while drawing with the graphics
//! card, the next one draws in software, sets Settings → General → Screens and graphics → Graphics to Software and says why.
//!
//! The supervisor's crash count lives only as long as the supervisor, and a start that fails (exit code 2) stops it,
//! so the count is kept on disk: `<data>/graphics-start.json` is written as each start begins and removed once the
//! app has run 20 seconds or quit normally. Finding it at the next start means that start failed.
use crate::{app::App, model::Graphics, video::graphics::{self, Chosen}};
use serde::{Deserialize, Serialize};
use std::{path::{Path, PathBuf}, time::Duration};

/// How long a start must run before it counts as having worked.
pub const SETTLE: Duration = Duration::from_secs(20);
/// Failed starts in a row that turn safe mode on.
pub const FAILS: u32 = 2;

/// What a start leaves on disk until it has worked.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Trial {
    pub graphics: Graphics,
    /// Starts just before this one that failed with graphics other than Software.
    pub fails: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Why { Setting, Flag, SafeMode }

/// What this start draws with, why, and its `Trial::fails`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan { pub graphics: Graphics, pub why: Why, pub fails: u32 }

/// `last` is what the previous start left, which is there only when it failed. `flag` is `--safe-mode`.
pub fn plan(setting: Graphics, flag: bool, last: Option<Trial>) -> Plan {
    let fails = match last {
        Some(t) if t.graphics != Graphics::Software => t.fails + 1,
        _ => 0,
    };
    if flag { return Plan { graphics: Graphics::Software, why: Why::Flag, fails: 0 } }
    if fails >= FAILS && setting != Graphics::Software { return Plan { graphics: Graphics::Software, why: Why::SafeMode, fails: 0 } }
    Plan { graphics: setting, why: Why::Setting, fails }
}

fn file(root: &Path) -> PathBuf { root.join("graphics-start.json") }

/// The previous start's record; `None` when it worked (or the file can't be read).
pub fn load(root: &Path) -> Option<Trial> {
    serde_json::from_slice(&std::fs::read(file(root)).ok()?).ok()
}

pub fn begin(root: &Path, t: Trial) {
    let json = serde_json::to_vec(&t).unwrap_or_default();
    if let Err(e) = std::fs::write(file(root), json) { eprintln!("Could not write {}: {e}", file(root).display()) }
}

/// This start worked: it ran 20 seconds or quit normally.
pub fn passed(root: &Path) {
    let _ = std::fs::remove_file(file(root));
}

/// What to tell the operator once the window is open.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// A message that stays until OK is clicked.
    Dialog(String),
    Toast(String),
}

pub struct Start { pub chosen: Chosen, pub notices: Vec<Notice> }

/// Chooses the graphics for this start (`video::choose_backend`) and records the start. Call once, before any window.
/// What Automatic tries first: the graphics test's `winner`, unless the last start failed with it.
pub fn auto_first(winner: Option<Graphics>, last: Option<Trial>) -> Option<Graphics> {
    winner.filter(|w| last.is_none_or(|t| t.graphics != *w))
}

pub fn start(app: &App, root: &Path, flag: bool) -> Start {
    let setting = app.data.lock().unwrap().settings.graphics;
    let last = load(root);
    let p = plan(setting, flag, last);
    let auto = auto_first(crate::video::bench::auto_pick(crate::video::bench::load(root).as_ref()), last);
    let mut notices = vec![];
    if p.why == Why::SafeMode {
        eprintln!("Graphics failed {} starts in a row: starting in safe mode (software).", FAILS);
        if let Err(e) = app.edit(|d| { d.settings.graphics = Graphics::Software; Ok(()) }) { eprintln!("Could not save the graphics setting: {e}") }
        notices.push(Notice::Dialog(graphics::safe_mode_notice()));
    }
    // Recorded before choosing too: a graphics driver can crash while it is chosen.
    let trying = if p.graphics == Graphics::Auto { auto.unwrap_or(Graphics::Auto) } else { p.graphics };
    begin(root, Trial { graphics: trying, fails: p.fails });
    let chosen = graphics::choose_backend_auto(p.graphics, auto);
    if chosen.graphics != trying { begin(root, Trial { graphics: chosen.graphics, fails: p.fails }) }
    if let Some(wanted) = chosen.fell_back_from { notices.push(Notice::Toast(graphics::fell_back(wanted, chosen.graphics))) }
    Start { chosen, notices }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Graphics::*;

    fn failed(graphics: Graphics, fails: u32) -> Option<Trial> { Some(Trial { graphics, fails }) }

    #[test]
    fn after_a_start_that_worked_the_setting_is_used() {
        assert_eq!(plan(OpenGl, false, None), Plan { graphics: OpenGl, why: Why::Setting, fails: 0 });
    }

    #[test]
    fn two_failed_starts_in_a_row_turn_safe_mode_on() {
        let first = plan(Auto, false, None);
        assert_eq!(first.fails, 0);
        let second = plan(Auto, false, failed(Auto, first.fails));
        assert_eq!(second, Plan { graphics: Auto, why: Why::Setting, fails: 1 }, "one failure is not enough");
        let third = plan(Auto, false, failed(Auto, second.fails));
        assert_eq!(third, Plan { graphics: Software, why: Why::SafeMode, fails: 0 });
    }

    #[test]
    fn failures_in_software_do_not_count() {
        assert_eq!(plan(Auto, false, failed(Software, 1)), Plan { graphics: Auto, why: Why::Setting, fails: 0 });
    }

    #[test]
    fn a_church_already_on_software_is_not_told_again() {
        assert_eq!(plan(Software, false, failed(Vulkan, 1)).why, Why::Setting);
    }

    #[test]
    fn the_flag_forces_software_without_a_notice() {
        assert_eq!(plan(Direct3D, true, None), Plan { graphics: Software, why: Why::Flag, fails: 0 });
        assert_eq!(plan(Direct3D, true, failed(Direct3D, 5)).why, Why::Flag);
    }

    #[test]
    fn automatic_skips_a_test_winner_that_just_failed_to_start() {
        assert_eq!(auto_first(Some(Vulkan), None), Some(Vulkan));
        assert_eq!(auto_first(Some(Vulkan), failed(Vulkan, 0)), None);
        assert_eq!(auto_first(Some(Vulkan), failed(OpenGl, 0)), Some(Vulkan));
        assert_eq!(auto_first(None, None), None);
    }

    #[test]
    fn the_record_is_kept_until_the_start_has_worked() {
        let root = std::env::temp_dir().join(format!("rp-safe-mode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(load(&root), None);
        begin(&root, Trial { graphics: Vulkan, fails: 1 });
        assert_eq!(load(&root), failed(Vulkan, 1));
        passed(&root);
        assert_eq!(load(&root), None);
        std::fs::write(file(&root), "not json").unwrap();
        assert_eq!(load(&root), None, "a broken record is a start that worked");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
