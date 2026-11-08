//! Settings → Media → Recording (N22): the picture size, the folder and the sound of File → Start recording. The
//! microphones are asked of FFmpeg off the UI thread; a saved one that is not plugged in stays chosen.
use crate::{
    record::{self, Preset, RecordPrefs, Sound, args::Input},
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

pub(super) fn fill_form(g: &SettingsDlg, p: &RecordPrefs) {
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
pub(super) fn read_form(g: &SettingsDlg, p: &mut RecordPrefs) {
    p.size = usize::try_from(g.get_record_size()).ok().and_then(|i| Preset::ALL.get(i).copied()).unwrap_or(p.size);
    let folder = g.get_record_folder().to_string();
    p.folder = if std::path::Path::new(&folder) == record::default_folder() { String::new() } else { folder };
    p.sound = read_sound(g).key();
}

/// Lists the microphones (when FFmpeg is there) and connects "Choose folder…".
pub(super) fn load(op: &Rc<Operator>) {
    let data = op.app().store.root.clone();
    let me = Rc::downgrade(op);
    let work = move || record::run::find_ffmpeg(&data).map(|f| record::run::list_inputs(&f)).unwrap_or_default();
    let done = move |inputs: Vec<Input>| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<SettingsDlg>();
        let chosen = read_sound(&g);
        fill_sound(&g, &inputs, &chosen);
    };
    // Tests never run FFmpeg for the list (`choices` is tested on its own).
    if !op.ops().is_inline() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = std::thread::Builder::new().name("record-inputs".into()).spawn(move || { let _ = tx.send(work()); });
        let _ = slint::spawn_local(async move { if let Ok(v) = rx.await { done(v) } });
    }
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
