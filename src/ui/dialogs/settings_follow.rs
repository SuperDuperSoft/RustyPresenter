//! Settings → Auto-follow (1.3): the sound input with a level meter and Test, the songs' usual mode, the lead
//! time, when the words go off, and how many words a jump needs. The inputs are asked for off the UI thread.
use crate::{
    follow::{LogEntry, Mode, Settings, audio, rules::{LEAD_MAX, LEAD_MIN}},
    model::{Data, ItemKind, ServiceItem, Slide},
    ui::{SettingsDlg, operator::window::Operator},
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use uuid::Uuid;

/// The jump strictness Settings allows (words heard on the other slide).
pub const JUMP_WORDS: std::ops::RangeInclusive<u8> = 2..=12;
/// The blank-after times Settings allows (seconds).
pub const BLANK_SECS: std::ops::RangeInclusive<u32> = 1..=60;
/// The seconds offered when a words-off timer is switched on (both are off as Settings come: songs have long
/// pauses, and the next words wait on the screen through them).
const BLANK_OFFER: f32 = 15.0;
const TALK_OFFER: f32 = 30.0;

thread_local! {
    /// Test's listening (and what it hears), until the dialog closes, and the timer that moves the meter.
    static TEST: RefCell<Option<(audio::Capture, std::sync::mpsc::Receiver<crate::follow::FollowEvent>)>> = const { RefCell::new(None) };
    static METER: slint::Timer = slint::Timer::default();
    /// Practice: the songs offered (as service items) and the recording chosen.
    static PRACTICE: RefCell<(Vec<ServiceItem>, Option<PathBuf>)> = const { RefCell::new((Vec::new(), None)) };
}

/// Runs Practice: `follow::practice::practice`, or a stand-in in tests.
type Runner = fn(&std::path::Path, Vec<Slide>, Settings) -> Result<Vec<LogEntry>, String>;

#[cfg(test)]
thread_local! {
    pub(crate) static FAKE_PRACTICE: std::cell::Cell<Option<Runner>> = const { std::cell::Cell::new(None) };
}

fn runner() -> Runner {
    #[cfg(test)]
    if let Some(f) = FAKE_PRACTICE.with(|f| f.get()) { return f }
    crate::follow::practice::practice
}

fn usual_input() -> String { crate::tr!("The first one found") }
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

/// The chosen input; `None`: the first one found.
fn read_device(g: &SettingsDlg) -> Option<String> {
    let name: String = g.get_follow_device_names().row_data(g.get_follow_device_index().max(0) as usize).unwrap_or_default().into();
    (!name.is_empty()).then_some(name)
}

/// Seconds as the form shows them ("3", "2.5").
fn secs_text(s: f32) -> String { if s.fract() == 0.0 { format!("{s:.0}") } else { format!("{s:.1}") } }

pub(super) fn fill_form(g: &SettingsDlg, s: &Settings, devices: &[String]) {
    fill_devices(g, devices, s.input.as_deref().unwrap_or(""));
    g.set_follow_mode(s.mode.index() as i32);
    g.set_follow_lead((s.lead() * 10.0).round() as i32);
    g.set_follow_blank_on(s.blank_after_s.is_some());
    g.set_follow_blank_secs(secs_text(s.blank_after_s.unwrap_or(BLANK_OFFER)).into());
    g.set_follow_talk_on(s.talk_blank_after_s.is_some());
    g.set_follow_talk_secs(secs_text(s.talk_blank_after_s.unwrap_or(TALK_OFFER)).into());
    g.set_follow_jump(s.jump().to_string().into());
    g.set_follow_level(0.0);
    g.set_follow_test_result(SharedString::new());
}

/// Reads the form back, every number kept to its range; a number that can't be read leaves the setting as it was.
pub(super) fn read_form(g: &SettingsDlg, s: &mut Settings) {
    s.input = read_device(g);
    s.mode = Mode::from_index(g.get_follow_mode().max(0) as usize);
    s.lead_s = (g.get_follow_lead() as f32 / 10.0).clamp(LEAD_MIN, LEAD_MAX);
    let secs = |on: bool, text: SharedString, old: Option<f32>| -> Option<f32> {
        if !on { return None }
        let (lo, hi) = (*BLANK_SECS.start() as f32, *BLANK_SECS.end() as f32);
        match text.trim().replace(',', ".").parse::<f32>() {
            Ok(n) if n.is_finite() => Some(n.clamp(lo, hi)),
            _ => Some(old.unwrap_or(BLANK_OFFER)),
        }
    };
    s.blank_after_s = secs(g.get_follow_blank_on(), g.get_follow_blank_secs(), s.blank_after_s);
    s.talk_blank_after_s = secs(g.get_follow_talk_on(), g.get_follow_talk_secs(), s.talk_blank_after_s.or(Some(TALK_OFFER)));
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
            fill_devices(&g, &devices, chosen.as_deref().unwrap_or(""));
        });
    }
    let me = Rc::downgrade(op);
    op.ui.global::<SettingsDlg>().on_follow_test(move || if let Some(op) = me.upgrade() { test(&op) });
    let g = op.ui.global::<SettingsDlg>();
    let me = Rc::downgrade(op);
    g.on_practice_choose(move || if let Some(op) = me.upgrade() { choose_recording(&op) });
    let me = Rc::downgrade(op);
    g.on_practice_run(move || if let Some(op) = me.upgrade() { run_practice(&op) });
    let me = Rc::downgrade(op);
    g.on_review_copy(move || if let Some(op) = me.upgrade() {
        let text = review_lines(&op).join("\n");
        super::settings::copy(&op, &text, &crate::tr!("Review log copied"));
    });
    g.set_follow_page(0);
    fill_practice(op);
    g.set_review_lines(strings(review_lines(op)));
}

/// Settings → Auto-follow on `page` (1 Practice, 2 Review log): the Auto chip's menu.
pub fn open_page(op: &Rc<Operator>, page: i32) {
    super::settings::open(op, super::SettingsTab::AutoFollow);
    op.ui.global::<SettingsDlg>().set_follow_page(page);
}

/// The songs Practice offers: the open service's first, then the rest of the library, by title.
pub fn practice_songs(data: &Data) -> Vec<(String, ServiceItem)> {
    let mut out = vec![];
    let mut seen = vec![];
    for item in data.service().map(|s| s.items.as_slice()).unwrap_or_default() {
        let ItemKind::Song { song_id, .. } = &item.kind else { continue };
        let Some(song) = data.songs.iter().find(|s| s.id == *song_id) else { continue };
        seen.push(*song_id);
        out.push((crate::tr!("{title} (in this service)", title = song.title), item.clone()));
    }
    let mut rest: Vec<_> = data.songs.iter().filter(|s| !seen.contains(&s.id)).collect();
    rest.sort_by_key(|s| s.title.to_lowercase());
    for song in rest {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Song { song_id: song.id, arrangement: None }, background: None, notes: String::new(), theme: None };
        out.push((song.title.clone(), item));
    }
    out
}

fn fill_practice(op: &Operator) {
    let g = op.ui.global::<SettingsDlg>();
    let Some(snap) = super::settings::snap(op) else { return };
    let songs = practice_songs(&snap.data);
    g.set_practice_songs(strings(songs.iter().map(|(t, _)| t.clone()).collect()));
    g.set_practice_song(0);
    let file = PRACTICE.with(|p| { let mut p = p.borrow_mut(); p.0 = songs.into_iter().map(|(_, i)| i).collect(); p.1.clone() });
    g.set_practice_file(file.map(|f| f.display().to_string()).unwrap_or_default().into());
    g.set_practice_running(false);
}

/// "Choose recording…": starts in the recordings folder (File → Start recording saves there).
fn choose_recording(op: &Rc<Operator>) {
    let folder = super::settings::snap(op).map(|s| s.settings.record.folder()).unwrap_or_else(crate::record::default_folder);
    crate::ui::operator::files::start_in(op, &folder);
    let me = Rc::downgrade(op);
    crate::ui::operator::files::pick(op, crate::ui::operator::file_input::Filter::Recording, false, move |paths| {
        let (Some(op), Some(path)) = (me.upgrade(), paths.into_iter().next()) else { return };
        op.ui.global::<SettingsDlg>().set_practice_file(path.display().to_string().into());
        PRACTICE.with(|p| p.borrow_mut().1 = Some(path));
    });
}

/// Practice: the chosen song over the recording, off the UI thread; its log shows when done.
fn run_practice(op: &Rc<Operator>) {
    let g = op.ui.global::<SettingsDlg>();
    let Some(snap) = super::settings::snap(op) else { return };
    let (item, file) = PRACTICE.with(|p| { let p = p.borrow(); (p.0.get(g.get_practice_song().max(0) as usize).cloned(), p.1.clone()) });
    let (Some(item), Some(file)) = (item, file) else { return };
    let slides = crate::slides::build_item(&snap.data, &item).2;
    let mut settings = snap.settings.follow.clone();
    read_form(&g, &mut settings);
    g.set_practice_running(true);
    g.set_practice_lines(strings(vec![]));
    g.set_practice_status(crate::tr!("Practicing… this takes about a minute for a 5-minute song.").into());
    let run = runner();
    let me = Rc::downgrade(op);
    let done = move |r: Result<Vec<LogEntry>, String>| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<SettingsDlg>();
        g.set_practice_running(false);
        match r {
            Ok(log) => {
                g.set_practice_status(crate::trn!("Done: {n} change.", "Done: {n} changes.", log.len()).into());
                g.set_practice_lines(strings(log.iter().map(LogEntry::line).collect()));
            }
            Err(e) => g.set_practice_status(crate::tr!("Practice stopped: {error}", error = e).into()),
        }
    };
    if op.ops().is_inline() { return done(run(&file, slides, settings)) }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = std::thread::Builder::new().name("follow-practice".into()).spawn(move || { let _ = tx.send(run(&file, slides, settings)); });
    let _ = slint::spawn_local(async move { if let Ok(r) = rx.await { done(r) } });
}

/// The review log as lines, each with its song's title: `Grace · 1:02.5  Slide 2  heard "…" (92%)`.
pub fn log_lines(entries: &[LogEntry], title: impl Fn(Uuid) -> Option<String>) -> Vec<String> {
    entries.iter().map(|e| match title(e.item) {
        Some(t) => format!("{t} · {}", e.line()),
        None => e.line(),
    }).collect()
}

fn review_lines(op: &Operator) -> Vec<String> {
    let entries = op.app().follower().map(|f| f.log()).unwrap_or_default();
    let snap = super::settings::snap(op);
    log_lines(&entries, |id| snap.as_ref().and_then(|s| s.items.iter().find(|i| i.id == id)).map(|i| i.title.clone()))
}

/// Test: listens to the chosen input until the dialog closes, the meter showing its level, or says why it can't.
fn test(op: &Rc<Operator>) {
    let g = op.ui.global::<SettingsDlg>();
    stop();
    let (tx, rx) = std::sync::mpsc::channel();
    let device = read_device(&g).or_else(|| audio::input_devices().into_iter().next());
    let started = match device {
        Some(d) => audio::Capture::start(&d, Box::new(audio::NoEar), tx),
        None => Err(crate::tr!("no microphone or line-in found")),
    };
    let said = match started {
        Ok(c) => {
            TEST.with(|t| *t.borrow_mut() = Some((c, rx)));
            let me = Rc::downgrade(op);
            METER.with(|m| m.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(100), move || {
                let Some(op) = me.upgrade() else { return };
                let level = TEST.with(|t| t.borrow().as_ref().map(|(c, rx)| { while rx.try_recv().is_ok() {} c.level() }));
                op.ui.global::<SettingsDlg>().set_follow_level(level.unwrap_or(0.0));
            }));
            crate::tr!("Listening… sing or say a line.")
        }
        Err(e) => crate::tr!("Can't listen to this input: {error}", error = e),
    };
    g.set_follow_test_result(said.into());
}

/// Stops Test's listening (the dialog closed).
pub(super) fn stop() {
    METER.with(|m| m.stop());
    TEST.with(|t| t.borrow_mut().take());
}

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
        // The defaults: 2 s lead, no words-off timers (15 s and 30 s offered when switched on), 4 words to jump.
        assert_eq!((g.get_follow_lead(), g.get_follow_blank_secs().as_str(), g.get_follow_talk_secs().as_str(), g.get_follow_jump().as_str()), (20, "15", "30", "4"));
        assert!(!g.get_follow_blank_on() && !g.get_follow_talk_on());
        let mut s = Settings::default();
        for (tenths, secs) in [(99, 4.0), (1, 0.5), (-5, 0.5), (35, 3.5)] {
            g.set_follow_lead(tenths);
            read_form(&g, &mut s);
            assert_eq!(s.lead_s, secs, "{tenths}");
        }
        // Off, and numbers kept to their ranges.
        g.set_follow_blank_on(false);
        g.set_follow_talk_on(true);
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

    fn fake(file: &std::path::Path, slides: Vec<Slide>, s: Settings) -> Result<Vec<LogEntry>, String> {
        use crate::follow::LogAction;
        assert!(file.ends_with("sunday.mp4") && !slides.is_empty());
        assert_eq!(s.lead_s, 3.0, "the form's settings, not yet saved");
        Ok(vec![LogEntry { at: 62.5, item: Uuid::nil(), action: LogAction::Show(1), reason: "heard \"my chains are gone\"".into(), confidence: 0.92 }])
    }

    #[test]
    fn practice_runs_a_recording_and_shows_the_log() {
        crate::ui::ui_test_init();
        let app = test_app();
        let song = crate::model::Song { id: Uuid::new_v4(), title: "Grace".into(), lyrics: "Verse 1\nAmazing grace\n\nChorus\nMy chains are gone".into(), ..Default::default() };
        app.data.lock().unwrap().songs.push(song);
        let op = open(&app);
        refresh(&op);
        open_page(&op, 1);
        let g = op.ui.global::<SettingsDlg>();
        assert_eq!((g.get_tab(), g.get_follow_page()), (4, 1));
        assert_eq!(g.get_practice_songs().row_data(0).unwrap(), "Grace");
        assert!(g.get_practice_file().is_empty());
        crate::ui::operator::files::testing::will_pick(vec![PathBuf::from("/rec/sunday.mp4")]);
        g.invoke_practice_choose();
        assert!(g.get_practice_file().ends_with("sunday.mp4"));
        g.set_follow_lead(30);
        FAKE_PRACTICE.with(|f| f.set(Some(fake)));
        g.invoke_practice_run();
        FAKE_PRACTICE.with(|f| f.set(None));
        assert!(!g.get_practice_running());
        assert_eq!(g.get_practice_status(), "Done: 1 change.");
        assert_eq!(g.get_practice_lines().row_data(0).unwrap(), "1:02.5  Slide 2  heard \"my chains are gone\" (92%)");
    }

    #[test]
    fn review_log_lines_name_the_song() {
        use crate::follow::LogAction;
        let id = Uuid::from_u128(7);
        let e = |at: f64, item: Uuid, action| LogEntry { at, item, action, reason: "no singing for 3 s".into(), confidence: 1.0 };
        let lines = log_lines(&[e(5.0, id, LogAction::Blank), e(70.0, Uuid::nil(), LogAction::Suggest(0))], |i| (i == id).then(|| "Grace".to_string()));
        assert_eq!(lines, ["Grace · 0:05.0  Words off  no singing for 3 s (100%)", "1:10.0  Suggested slide 1  no singing for 3 s (100%)"]);
    }

    #[test]
    fn review_log_opens_from_the_chip_and_copies() {
        crate::ui::ui_test_init();
        let app = test_app();
        app.start_follower();
        let op = open(&app);
        refresh(&op);
        op.ui.global::<crate::ui::FollowUi>().invoke_open_log();
        let g = op.ui.global::<SettingsDlg>();
        assert_eq!((g.get_tab(), g.get_follow_page()), (4, 2), "Settings → Auto-follow → Review log");
        assert_eq!(g.get_review_lines().row_count(), 0, "nothing yet");
        assert!(crate::ui::operator::window::testing::find(&op, "SettingsFollowReview::review-copy").is_some());
    }

    #[test]
    fn the_usual_settings_entry_keeps_its_tab() {
        crate::ui::ui_test_init();
        let app = test_app();
        let op = open(&app);
        refresh(&op);
        let g = op.ui.global::<SettingsDlg>();
        op.ui.invoke_settings();
        assert_eq!(g.get_tab(), 0, "the gear opens on Look, as before");
        op.close_dialog();
        open_page(&op, 2);
        assert_eq!((g.get_tab(), g.get_follow_page()), (4, 2), "⋯ → Review log");
        op.close_dialog();
        op.ui.invoke_settings();
        assert_eq!(g.get_tab(), 0, "the gear still opens on Look after the ⋯ menu");
        op.close_dialog();
        settings::open(&op, SettingsTab::Bible);
        assert_eq!(g.get_tab(), settings::tab_index(SettingsTab::Bible), "a menu asking for a tab gets it");
    }

    #[test]
    fn a_saved_input_that_is_unplugged_stays_chosen() {
        let (labels, names, index) = device_choices(&["Desk mix".into()], "");
        assert_eq!((labels.len(), names, index), (2, vec![String::new(), "Desk mix".into()], 0));
        let (labels, _, index) = device_choices(&[], "USB Mic");
        assert_eq!((labels[1].as_str(), index), ("USB Mic (not connected)", 1));
    }
}
