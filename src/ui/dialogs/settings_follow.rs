//! Settings → Auto-follow (1.3): the sound input with a level meter and Test, the songs' usual mode, the lead
//! time, when the words go off, and how many words a jump needs. The inputs are asked for off the UI thread.
use crate::{
    follow::{Mode, Settings, audio, rules::{LEAD_MAX, LEAD_MIN}},
    ui::{SettingsDlg, operator::window::Operator},
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc};

/// The jump strictness Settings allows (words in a row).
pub const JUMP_WORDS: std::ops::RangeInclusive<u8> = 2..=12;
/// The blank-after times Settings allows (seconds).
pub const BLANK_SECS: std::ops::RangeInclusive<u32> = 1..=60;

thread_local! {
    /// Test's listening (and what it hears), until the dialog closes.
    static TEST: RefCell<Option<(audio::Capture, std::sync::mpsc::Receiver<crate::follow::FollowEvent>)>> = const { RefCell::new(None) };
}

fn usual_input() -> String { crate::tr!("The computer's usual input") }
fn not_connected(name: &str) -> String { crate::tr!("{name} (not connected)", name = name) }

/// The input list's lines and the device each stands for, with the chosen one. A saved input that is not plugged
/// in stays chosen.
pub fn device_choices(devices: &[String], saved: &str) -> (Vec<String>, Vec<String>, usize) {
    let mut labels = vec![usual_input()];
    let mut names = vec![String::new()];
    labels.extend(devices.iter().cloned());
    names.extend(devices.iter().cloned());
    if !saved.is_empty() && !devices.iter().any(|d| d == saved) {
        labels.push(not_connected(saved));
        names.push(saved.into());
    }
    let index = names.iter().position(|n| n == saved).unwrap_or(0);
    (labels, names, index)
}

fn strings(v: Vec<String>) -> ModelRc<SharedString> { ModelRc::new(VecModel::from(v.into_iter().map(Into::into).collect::<Vec<SharedString>>())) }

fn fill_devices(g: &SettingsDlg, devices: &[String], saved: &str) {
    let (labels, names, index) = device_choices(devices, saved);
    g.set_follow_device_labels(strings(labels));
    g.set_follow_device_names(strings(names));
    g.set_follow_device_index(index as i32);
}

fn read_device(g: &SettingsDlg) -> String {
    g.get_follow_device_names().row_data(g.get_follow_device_index().max(0) as usize).unwrap_or_default().into()
}

/// Seconds as the form shows them ("3", "2.5").
fn secs_text(s: f32) -> String { if s.fract() == 0.0 { format!("{s:.0}") } else { format!("{s:.1}") } }

pub(super) fn fill_form(g: &SettingsDlg, s: &Settings, devices: &[String]) {
    fill_devices(g, devices, &s.device);
    g.set_follow_mode(s.mode.index() as i32);
    g.set_follow_lead((s.lead() * 10.0).round() as i32);
    g.set_follow_blank_on(s.blank_after_s.is_some());
    g.set_follow_blank_secs(secs_text(s.blank_after_s.unwrap_or(3.0)).into());
    g.set_follow_talk_on(s.talk_blank_after_s.is_some());
    g.set_follow_talk_secs(secs_text(s.talk_blank_after_s.unwrap_or(8.0)).into());
    g.set_follow_jump(s.jump().to_string().into());
    g.set_follow_level(0.0);
    g.set_follow_test_result(SharedString::new());
}

/// Reads the form back, every number kept to its range; a number that can't be read leaves the setting as it was.
pub(super) fn read_form(g: &SettingsDlg, s: &mut Settings) {
    s.device = read_device(g);
    s.mode = Mode::from_index(g.get_follow_mode().max(0) as usize);
    s.lead_s = (g.get_follow_lead() as f32 / 10.0).clamp(LEAD_MIN, LEAD_MAX);
    let secs = |on: bool, text: SharedString, old: Option<f32>| -> Option<f32> {
        if !on { return None }
        let (lo, hi) = (*BLANK_SECS.start() as f32, *BLANK_SECS.end() as f32);
        match text.trim().replace(',', ".").parse::<f32>() {
            Ok(n) if n.is_finite() => Some(n.clamp(lo, hi)),
            _ => Some(old.unwrap_or(3.0)),
        }
    };
    s.blank_after_s = secs(g.get_follow_blank_on(), g.get_follow_blank_secs(), s.blank_after_s);
    s.talk_blank_after_s = secs(g.get_follow_talk_on(), g.get_follow_talk_secs(), s.talk_blank_after_s.or(Some(8.0)));
    if let Ok(n) = g.get_follow_jump().trim().parse::<u32>() {
        s.jump_words = n.clamp(u32::from(*JUMP_WORDS.start()), u32::from(*JUMP_WORDS.end())) as u8;
    }
}

/// Lists the inputs (off the UI thread) and connects Test.
pub(super) fn load(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    // Tests never ask the computer for its inputs (`device_choices` is tested on its own).
    if !op.ops().is_inline() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = std::thread::Builder::new().name("follow-inputs".into()).spawn(move || { let _ = tx.send(audio::input_devices()); });
        let _ = slint::spawn_local(async move {
            let (Ok(devices), Some(op)) = (rx.await, me.upgrade()) else { return };
            let g = op.ui.global::<SettingsDlg>();
            let chosen = read_device(&g);
            fill_devices(&g, &devices, &chosen);
        });
    }
    let me = Rc::downgrade(op);
    op.ui.global::<SettingsDlg>().on_follow_test(move || if let Some(op) = me.upgrade() { test(&op) });
}

/// Test: listens to the chosen input until the dialog closes, or says why it can't.
fn test(op: &Rc<Operator>) {
    let g = op.ui.global::<SettingsDlg>();
    stop();
    let (tx, rx) = std::sync::mpsc::channel();
    let said = match audio::Capture::start(&read_device(&g), tx) {
        Ok(c) => {
            TEST.with(|t| *t.borrow_mut() = Some((c, rx)));
            crate::tr!("Listening… sing or say a line.")
        }
        Err(e) => crate::tr!("Can't listen to this input: {error}", error = e),
    };
    g.set_follow_test_result(said.into());
}

/// Stops Test's listening (the dialog closed).
pub(super) fn stop() { TEST.with(|t| t.borrow_mut().take()); }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, ui::{dialogs::{SettingsTab, settings}, operator::window::testing::{open, refresh}}};

    #[test]
    fn settings_lead_slider_clamps() {
        crate::ui::ui_test_init();
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        settings::open(&op, SettingsTab::AutoFollow);
        let g = op.ui.global::<SettingsDlg>();
        assert_eq!(g.get_tab(), 4, "Auto-follow comes right after Songs");
        // The defaults: 2 s lead, words off after 3 s of no singing and 8 s of talking, 4 words to jump.
        assert_eq!((g.get_follow_lead(), g.get_follow_blank_secs().as_str(), g.get_follow_talk_secs().as_str(), g.get_follow_jump().as_str()), (20, "3", "8", "4"));
        assert!(g.get_follow_blank_on() && g.get_follow_talk_on());
        let mut s = Settings::default();
        for (tenths, secs) in [(99, 4.0), (1, 0.5), (-5, 0.5), (35, 3.5)] {
            g.set_follow_lead(tenths);
            read_form(&g, &mut s);
            assert_eq!(s.lead_s, secs, "{tenths}");
        }
        // Off, and numbers kept to their ranges.
        g.set_follow_blank_on(false);
        g.set_follow_talk_secs("500".into());
        g.set_follow_jump("1".into());
        g.set_follow_mode(2);
        read_form(&g, &mut s);
        assert_eq!((s.blank_after_s, s.talk_blank_after_s, s.jump_words, s.mode), (None, Some(60.0), 2, Mode::Auto));
        // A saved lead out of range shows clamped.
        let wild = Settings { lead_s: 9.0, ..Settings::default() };
        fill_form(&g, &wild, &[]);
        assert_eq!(g.get_follow_lead(), 40);
        // Saving keeps it.
        g.set_follow_lead(25);
        g.invoke_save();
        assert_eq!(app.data.lock().unwrap().settings.follow.lead_s, 2.5);
    }

    #[test]
    fn a_saved_input_that_is_unplugged_stays_chosen() {
        let (labels, names, index) = device_choices(&["Desk mix".into()], "");
        assert_eq!((labels.len(), names, index), (2, vec![String::new(), "Desk mix".into()], 0));
        let (labels, _, index) = device_choices(&[], "USB Mic");
        assert_eq!((labels[1].as_str(), index), ("USB Mic (not connected)", 1));
    }
}
