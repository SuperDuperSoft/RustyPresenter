//! Settings → Media → Recording (N22): the picture size, the folder and the sound of File → Start recording. The
//! microphones are asked of FFmpeg off the UI thread; a saved one that is not plugged in stays chosen.
use crate::{
    record::{self, Preset, RecordPrefs, Sound, args::Input},
    stream,
    ui::{SettingsDlg, operator::window::Operator},
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;

/// The sound list's lines and the saved key of each, with the chosen one.
#[derive(Debug, PartialEq)]
pub struct Choices { pub labels: Vec<String>, pub keys: Vec<String>, pub index: usize }

pub fn choices(inputs: &[Input], saved: &Sound) -> Choices {
    let mut labels = vec![record::tr::none(), record::tr::app_sound()];
    let mut keys = vec![Sound::None.key(), Sound::App.key()];
    for i in inputs {
        labels.push(record::tr::input(&i.label));
        keys.push(Sound::Input(i.name.clone()).key());
    }
    if let Sound::Input(name) = saved && !inputs.iter().any(|i| &i.name == name) {
        labels.push(record::tr::input(&format!("{name} ({})", not_connected())));
        keys.push(saved.key());
    }
    let index = keys.iter().position(|k| *k == saved.key()).unwrap_or(1);
    Choices { labels, keys, index }
}

fn not_connected() -> String { crate::tr!("not connected") }

fn strings(v: Vec<String>) -> ModelRc<SharedString> { ModelRc::new(VecModel::from(v.into_iter().map(Into::into).collect::<Vec<SharedString>>())) }

/// Settings' four destinations as saved: whether each is on, and the custom server's address.
fn stream_rows(p: &RecordPrefs) -> ([bool; 4], String) {
    let mut on = [false; 4];
    let mut url = String::new();
    for d in &p.stream {
        let Some(i) = (0..4).find(|&i| stream::built_in_id(i) == d.id) else { continue };
        on[i] = d.on;
        if i == 3 { url = d.url.clone() }
    }
    (on, url)
}

/// Which of the four have a key in this computer's store.
fn fill_saved(g: &SettingsDlg) {
    let store = stream::keys::app_store();
    let has = |i: usize| store.get(stream::built_in_id(i)).is_some();
    g.set_stream_saved_0(has(0));
    g.set_stream_saved_1(has(1));
    g.set_stream_saved_2(has(2));
    g.set_stream_saved_3(has(3));
}

/// The list "Sound from this computer": off, the output the app plays to, then each output; a saved one that is
/// not plugged in stays chosen.
pub fn stream_sound_choices(devices: &[crate::video::ffi::AudioDevice], saved: &str) -> Choices {
    let mut labels = vec![crate::tr!("Off"), crate::tr!("The output the app plays its sound to")];
    let mut keys = vec!["off".to_string(), String::new()];
    labels.extend(devices.iter().map(|d| d.label.clone()));
    keys.extend(devices.iter().map(|d| d.name.clone()));
    if !keys.iter().any(|k| k == saved) {
        labels.push(format!("{saved} ({})", not_connected()));
        keys.push(saved.to_string());
    }
    let index = keys.iter().position(|k| k == saved).unwrap_or(1);
    Choices { labels, keys, index }
}

pub(crate) fn fill_stream_sound(g: &SettingsDlg, devices: &[crate::video::ffi::AudioDevice], saved: &str) {
    let c = stream_sound_choices(devices, saved);
    g.set_stream_sound_labels(strings(c.labels));
    g.set_stream_sound_names(strings(c.keys));
    g.set_stream_sound_index(c.index as i32);
}

pub(crate) fn read_stream_sound(g: &SettingsDlg) -> String {
    g.get_stream_sound_names().row_data(g.get_stream_sound_index().max(0) as usize).map(|s| s.to_string()).unwrap_or_default()
}

/// The list "Camera": none, then each camera; a saved one that is not plugged in stays chosen.
pub fn camera_choices(cameras: &[Input], saved: &str) -> Choices {
    let mut labels = vec![crate::tr!("No camera")];
    let mut keys = vec![String::new()];
    labels.extend(cameras.iter().map(|c| c.label.clone()));
    keys.extend(cameras.iter().map(|c| c.name.clone()));
    if !saved.is_empty() && !keys.iter().any(|k| k == saved) {
        labels.push(format!("{saved} ({})", not_connected()));
        keys.push(saved.to_string());
    }
    let index = keys.iter().position(|k| k == saved).unwrap_or(0);
    Choices { labels, keys, index }
}

fn fill_camera(g: &SettingsDlg, cameras: &[Input], saved: &str, saved2: &str) {
    let c = camera_choices(cameras, saved);
    g.set_stream_camera_labels(strings(c.labels));
    g.set_stream_camera_names(strings(c.keys));
    g.set_stream_camera_index(c.index as i32);
    let c = camera_choices(cameras, saved2);
    g.set_stream_camera2_labels(strings(c.labels));
    g.set_stream_camera2_names(strings(c.keys));
    g.set_stream_camera2_index(c.index as i32);
}

fn read_camera(g: &SettingsDlg) -> String {
    g.get_stream_camera_names().row_data(g.get_stream_camera_index().max(0) as usize).map(|s| s.to_string()).unwrap_or_default()
}

/// The second camera: none without a first, or when it is the first again.
fn read_camera2(g: &SettingsDlg) -> String {
    let first = read_camera(g);
    let second = g.get_stream_camera2_names().row_data(g.get_stream_camera2_index().max(0) as usize).map(|s| s.to_string()).unwrap_or_default();
    if first.is_empty() || second == first { String::new() } else { second }
}

pub(crate) fn fill_form(g: &SettingsDlg, p: &RecordPrefs) {
    fill_stream_sound(g, &[], &p.stream_sound);
    fill_camera(g, &[], &p.camera, &p.camera2);
    g.set_stream_auto_scenes(p.auto().on);
    g.set_stream_sound_delay(p.sound_delay_ms.min(record::MAX_SOUND_DELAY) as i32);
    g.set_stream_presentation_camera(i32::from(p.presentation_camera));
    let c = &p.cards;
    g.set_stream_card_words_0(c.starting.words.as_str().into());
    g.set_stream_card_words_1(c.ending.words.as_str().into());
    g.set_stream_card_words_2(c.hold.words.as_str().into());
    g.set_stream_card_picture_0(c.starting.picture.as_str().into());
    g.set_stream_card_picture_1(c.ending.picture.as_str().into());
    g.set_stream_card_picture_2(c.hold.picture.as_str().into());
    let (on, url) = stream_rows(p);
    g.set_stream_on_0(on[0]);
    g.set_stream_on_1(on[1]);
    g.set_stream_on_2(on[2]);
    g.set_stream_on_3(on[3]);
    g.set_stream_url(url.into());
    fill_saved(g);
    g.set_record_size(Preset::ALL.iter().position(|s| *s == p.size).unwrap_or(1) as i32);
    g.set_record_folder(p.folder().display().to_string().into());
    fill_sound(g, &[], &p.sound());
}

fn fill_sound(g: &SettingsDlg, inputs: &[Input], saved: &Sound) {
    let c = choices(inputs, saved);
    g.set_record_sound_labels(strings(c.labels));
    g.set_record_sound_names(strings(c.keys));
    g.set_record_sound_index(c.index as i32);
}

fn read_sound(g: &SettingsDlg) -> Sound {
    Sound::parse(&g.get_record_sound_names().row_data(g.get_record_sound_index().max(0) as usize).unwrap_or_default())
}

/// Reads the form back. The default folder is saved as empty, so it follows the computer's Videos folder.
pub(crate) fn read_form(g: &SettingsDlg, p: &mut RecordPrefs) {
    p.size = usize::try_from(g.get_record_size()).ok().and_then(|i| Preset::ALL.get(i).copied()).unwrap_or(p.size);
    let folder = g.get_record_folder().to_string();
    p.folder = if std::path::Path::new(&folder) == record::default_folder() { String::new() } else { folder };
    // Unchanged keeps the saved spelling (empty is the app's sound too), so an untouched form has no changes.
    let sound = read_sound(g);
    if sound != p.sound() { p.sound = sound.key() }
    // The destinations: a row is kept while it is on, or (the custom one) has an address. Keys are not here.
    let on = [g.get_stream_on_0(), g.get_stream_on_1(), g.get_stream_on_2(), g.get_stream_on_3()];
    let url = g.get_stream_url().trim().to_string();
    let rows: Vec<stream::Destination> = (0..4).filter(|&i| on[i] || (i == 3 && !url.is_empty())).map(|i| stream::Destination {
        id: stream::built_in_id(i), name: String::new(), service: stream::BUILT_IN[i].0, url: if i == 3 { url.clone() } else { String::new() }, key: String::new(), on: on[i],
    }).collect();
    if rows != p.stream { p.stream = rows }
    let pc = read_stream_sound(g);
    if pc != p.stream_sound { p.stream_sound = pc }
    let camera = read_camera(g);
    if camera != p.camera { p.camera = camera }
    let camera2 = read_camera2(g);
    if camera2 != p.camera2 { p.camera2 = camera2 }
    // Unchanged keeps the saved form (`None` is on), so an untouched form has no changes.
    let auto = g.get_stream_auto_scenes();
    if auto != p.auto().on { p.auto_scenes = if auto { None } else { Some(false) } }
    p.presentation_camera = g.get_stream_presentation_camera() == 1;
    p.sound_delay_ms = (g.get_stream_sound_delay().max(0) as u32).min(record::MAX_SOUND_DELAY);
    let card = |w: slint::SharedString, pic: slint::SharedString| stream::pictures::Card { words: w.trim().to_string(), picture: pic.trim().to_string() };
    p.cards = stream::pictures::Cards {
        starting: card(g.get_stream_card_words_0(), g.get_stream_card_picture_0()),
        ending: card(g.get_stream_card_words_1(), g.get_stream_card_picture_1()),
        hold: card(g.get_stream_card_words_2(), g.get_stream_card_picture_2()),
    };
}

/// A stream key typed in Settings is kept in the computer's password store at once (it is no part of the
/// settings, so Save and Cancel do not touch it); an emptied field forgets it.
pub(crate) fn wire_keys(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ui.global::<SettingsDlg>().on_stream_key_edited(move |i, key| {
        let Some(op) = me.upgrade() else { return };
        let id = stream::built_in_id(i.clamp(0, 3) as usize);
        if let Err(e) = stream::keys::app_store().set(id, key.as_str()) { op.toast(crate::tr!("The stream key could not be kept: {error}", error = e), true) }
        fill_saved(&op.ui.global::<SettingsDlg>());
    });
}

/// The speed test's answer: what it found, and the recording size that fits picked.
pub(crate) fn show_speed(g: &SettingsDlg, measured: Result<f64, String>, destinations: usize) {
    g.set_stream_speed_testing(false);
    match measured {
        Ok(mbps) => {
            let (preset, text) = stream::speed::advice(mbps, destinations);
            if let Some(p) = preset && let Some(i) = Preset::ALL.iter().position(|x| *x == p) { g.set_record_size(i as i32) }
            g.set_stream_speed_text(text.into());
        }
        Err(e) => {
            eprintln!("speed test: {e}");
            g.set_stream_speed_text(crate::tr!("The speed test could not reach the internet. Check the connection and try again.").into());
        }
    }
}

/// Lists the microphones (when FFmpeg is there) and connects "Choose folder…".
pub(super) fn load(op: &Rc<Operator>) {
    let data = op.app().store.root.clone();
    let me = Rc::downgrade(op);
    let work = move || record::run::find_ffmpeg(&data).map(|f| (record::run::list_inputs(&f), record::run::list_cameras(&f))).unwrap_or_default();
    let done = move |(inputs, cameras): (Vec<Input>, Vec<Input>)| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<SettingsDlg>();
        let chosen = read_sound(&g);
        fill_sound(&g, &inputs, &chosen);
        let (camera, camera2) = (read_camera(&g), read_camera2(&g));
        fill_camera(&g, &cameras, &camera, &camera2);
    };
    // Tests never run FFmpeg for the list (`choices` is tested on its own).
    if !op.ops().is_inline() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = std::thread::Builder::new().name("record-inputs".into()).spawn(move || { let _ = tx.send(work()); });
        let _ = slint::spawn_local(async move { if let Ok(v) = rx.await { done(v) } });
    }
    let me = Rc::downgrade(op);
    op.ui.global::<SettingsDlg>().on_stream_speed_test(move || {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<SettingsDlg>();
        let destinations = [g.get_stream_on_0(), g.get_stream_on_1(), g.get_stream_on_2(), g.get_stream_on_3()].iter().filter(|on| **on).count();
        g.set_stream_speed_testing(true);
        g.set_stream_speed_text("".into());
        let ui = op.ui.as_weak();
        let _ = std::thread::Builder::new().name("speed-test".into()).spawn(move || {
            let measured = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string()).and_then(|rt| {
                let client = crate::bible::sources::client().map_err(|e| format!("{e:?}"))?;
                rt.block_on(stream::speed::measure(client, stream::speed::URL, stream::speed::BYTES))
            });
            let _ = ui.upgrade_in_event_loop(move |ui| show_speed(&ui.global::<SettingsDlg>(), measured, destinations));
        });
    });
    let me = Rc::downgrade(op);
    op.ui.global::<SettingsDlg>().on_stream_card_choose(move |i| {
        let Some(op) = me.upgrade() else { return };
        let me = Rc::downgrade(&op);
        crate::ui::operator::files::pick(&op, crate::ui::operator::file_input::Filter::Pictures, false, move |paths| {
            let Some(op) = me.upgrade() else { return };
            let g = op.ui.global::<SettingsDlg>();
            let Some(path) = paths.first() else { return };
            let path: slint::SharedString = path.display().to_string().into();
            match i { 0 => g.set_stream_card_picture_0(path), 1 => g.set_stream_card_picture_1(path), _ => g.set_stream_card_picture_2(path) }
        });
    });
    let me = Rc::downgrade(op);
    op.ui.global::<SettingsDlg>().on_record_choose_folder(move || {
        let Some(op) = me.upgrade() else { return };
        let start = std::path::PathBuf::from(op.ui.global::<SettingsDlg>().get_record_folder().as_str());
        let me = Rc::downgrade(&op);
        crate::ui::operator::files::pick_folder(&op, &start, move |dir| {
            if let Some(op) = me.upgrade() { op.ui.global::<SettingsDlg>().set_record_folder(dir.display().to_string().into()) }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mic(name: &str) -> Input { Input { name: name.into(), label: name.into() } }

    #[test]
    fn the_apps_sound_is_chosen_by_default_and_inputs_follow() {
        let c = choices(&[mic("USB Mic")], &Sound::App);
        assert_eq!(c.keys, ["none", "app", "input:USB Mic"]);
        assert_eq!(c.labels, ["No sound", "The sound the app plays (videos and songs)", "Microphone or line-in: USB Mic"]);
        assert_eq!(c.index, 1);
    }

    #[test]
    fn a_saved_microphone_that_is_unplugged_stays_chosen() {
        let c = choices(&[], &Sound::Input("Line In".into()));
        assert_eq!(c.keys, ["none", "app", "input:Line In"]);
        assert_eq!(c.labels[2], "Microphone or line-in: Line In (not connected)");
        assert_eq!(c.index, 2);
        assert_eq!(choices(&[], &Sound::None).index, 0);
    }
}
