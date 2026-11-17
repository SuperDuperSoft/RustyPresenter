//! Settings → Media → Livestream: the stream's sound beyond the levels and clean-up (`ui/dialogs/settings-sound.slint`):
//! "Sound setup" (`stream::sound_setup`), the second and third microphone or line-in, the phone as a microphone,
//! Listen, and each sound's filters (More sound settings).
use crate::{
    record::{self, RecordPrefs, Sound, args::{Filters, Input, Strength}},
    stream::{listen, phone_mic, sound_setup::{self, Mode, StreamFilters}},
    ui::{SettingsDlg, SoundFilters, operator::window::Operator},
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;

fn strings(v: Vec<String>) -> ModelRc<SharedString> { ModelRc::new(VecModel::from(v.into_iter().map(Into::into).collect::<Vec<SharedString>>())) }

fn not_connected() -> String { crate::tr!("not connected") }

/// The lines of a list and the saved name of each, with the chosen one.
#[derive(Debug, PartialEq)]
pub struct Choices { pub labels: Vec<String>, pub names: Vec<String>, pub index: usize }

/// The list for the second and third input: none, each input, the phone; a saved one that is not plugged in stays.
pub fn input_choices(inputs: &[Input], saved: &str) -> Choices {
    let mut labels = vec![sound_setup::tr::no_extra()];
    let mut names = vec![String::new()];
    labels.extend(inputs.iter().map(|i| i.label.clone()));
    names.extend(inputs.iter().map(|i| i.name.clone()));
    labels.push(phone_mic::tr::input_label());
    names.push(phone_mic::INPUT.to_string());
    Choices { index: chosen(&mut labels, &mut names, saved), labels, names }
}

/// "Listen to the stream": off, the usual output, then each output.
pub fn listen_choices(devices: &[crate::video::ffi::AudioDevice], saved: &str) -> Choices {
    let mut labels = vec![listen::tr::off(), listen::tr::usual()];
    let mut names = vec![String::new(), listen::DEFAULT.to_string()];
    labels.extend(devices.iter().map(|d| d.label.clone()));
    names.extend(devices.iter().map(|d| d.name.clone()));
    Choices { index: chosen(&mut labels, &mut names, saved), labels, names }
}

/// Where `saved` is among `names`, adding it as "not connected" when it is not there.
fn chosen(labels: &mut Vec<String>, names: &mut Vec<String>, saved: &str) -> usize {
    if let Some(i) = names.iter().position(|n| n == saved) { return i }
    labels.push(format!("{saved} ({})", not_connected()));
    names.push(saved.to_string());
    names.len() - 1
}

pub fn to_ui(f: Filters) -> SoundFilters {
    SoundFilters { gate: f.gate.index(), even: f.even.index(), limit: f.limit, low: f.low_db, mid: f.mid_db, high: f.high_db }
}

pub fn from_ui(f: &SoundFilters) -> Filters {
    let eq = |db: i32| db.clamp(-record::args::MAX_EQ_DB, record::args::MAX_EQ_DB);
    Filters { gate: Strength::from_index(f.gate), even: Strength::from_index(f.even), limit: f.limit, low_db: eq(f.low), mid_db: eq(f.mid), high_db: eq(f.high) }
}

fn fill_inputs(g: &SettingsDlg, inputs: &[Input], saved: [&str; 2]) {
    let two = input_choices(inputs, saved[0]);
    let three = input_choices(inputs, saved[1]);
    // One list for both: the third's saved one, when it is not there, is added too.
    let mut labels = two.labels;
    let mut names = two.names;
    let i3 = chosen(&mut labels, &mut names, &three.names[three.index]);
    g.set_stream_input_labels(strings(labels));
    g.set_stream_input_names(strings(names));
    g.set_stream_input2_index(two.index as i32);
    g.set_stream_input3_index(i3 as i32);
}

fn fill_filters(g: &SettingsDlg, f: &StreamFilters) {
    g.set_stream_board_filters(to_ui(f.board));
    g.set_stream_input2_filters(to_ui(f.input2));
    g.set_stream_input3_filters(to_ui(f.input3));
    g.set_stream_pc_filters(to_ui(f.pc));
}

/// Puts the saved settings into the form (with no input or output lists yet: `fill_inputs_found`, `fill_listen`).
pub(crate) fn fill_form(g: &SettingsDlg, p: &RecordPrefs) {
    g.set_stream_sound_mode_labels(strings(sound_setup::tr::modes()));
    g.set_stream_sound_mode(p.stream_sound_mode.index());
    g.set_stream_sound_mode_note(SharedString::new());
    g.set_stream_phone_name(phone_mic::INPUT.into());
    fill_inputs(g, &[], [&p.stream_input2, &p.stream_input3]);
    g.set_stream_input2_level(p.stream_input2_db.clamp(-record::MAX_LEVEL_DB, record::MAX_LEVEL_DB));
    g.set_stream_input3_level(p.stream_input3_db.clamp(-record::MAX_LEVEL_DB, record::MAX_LEVEL_DB));
    fill_filters(g, &p.filters());
    fill_listen(g, &[], &p.stream_listen);
}

fn name_at(names: ModelRc<SharedString>, i: i32) -> String { names.row_data(i.max(0) as usize).map(|s| s.to_string()).unwrap_or_default() }

/// Reads the form back. The setup stays only while everything it set is as it set it.
pub(crate) fn read_form(g: &SettingsDlg, p: &mut RecordPrefs) {
    let level = |db: i32| db.clamp(-record::MAX_LEVEL_DB, record::MAX_LEVEL_DB);
    p.stream_input2 = name_at(g.get_stream_input_names(), g.get_stream_input2_index());
    p.stream_input3 = if p.stream_input2.is_empty() { String::new() } else { name_at(g.get_stream_input_names(), g.get_stream_input3_index()) };
    p.stream_input2_db = level(g.get_stream_input2_level());
    p.stream_input3_db = level(g.get_stream_input3_level());
    let f = StreamFilters {
        board: from_ui(&g.get_stream_board_filters()), input2: from_ui(&g.get_stream_input2_filters()),
        input3: from_ui(&g.get_stream_input3_filters()), pc: from_ui(&g.get_stream_pc_filters()),
    };
    p.stream_filters = (!f.is_default()).then_some(f);
    p.stream_listen = name_at(g.get_stream_listen_names(), g.get_stream_listen_index());
    let mode = Mode::from_index(g.get_stream_sound_mode());
    p.stream_sound_mode = if sound_setup::matches(mode, p) { mode } else { Mode::Custom };
}

/// The inputs found (after FFmpeg was asked), keeping what is chosen by then.
pub(crate) fn fill_inputs_found(g: &SettingsDlg, inputs: &[Input]) {
    let (two, three) = (name_at(g.get_stream_input_names(), g.get_stream_input2_index()), name_at(g.get_stream_input_names(), g.get_stream_input3_index()));
    fill_inputs(g, inputs, [&two, &three]);
}

/// The outputs found, keeping what is chosen by then.
pub(crate) fn fill_listen(g: &SettingsDlg, devices: &[crate::video::ffi::AudioDevice], saved: &str) {
    let c = listen_choices(devices, saved);
    g.set_stream_listen_labels(strings(c.labels));
    g.set_stream_listen_names(strings(c.names));
    g.set_stream_listen_index(c.index as i32);
}

pub(crate) fn read_listen(g: &SettingsDlg) -> String { name_at(g.get_stream_listen_names(), g.get_stream_listen_index()) }

/// The inputs Settings lists for "Sound in the recording" (its names are `input:<name>`).
fn listed_inputs(g: &SettingsDlg) -> Vec<Input> {
    let (names, labels) = (g.get_record_sound_names(), g.get_record_sound_labels());
    (0..names.row_count()).filter_map(|i| {
        let name = names.row_data(i)?.strip_prefix("input:")?.to_string();
        Some(Input { name, label: labels.row_data(i).map(|l| l.to_string()).unwrap_or_default() })
    }).collect()
}

/// A setup chosen: what it sets is put in the form at once (the inputs it found, the filters), to be saved with Save.
pub(crate) fn choose_mode(g: &SettingsDlg, i: i32) {
    let mode = Mode::from_index(i);
    let mut p = RecordPrefs::default();
    super::settings_record::read_form(g, &mut p);
    let missing = sound_setup::apply(mode, &mut p, &listed_inputs(g));
    g.set_stream_sound_mode_note(if missing { sound_setup::tr::no_input_found().into() } else { SharedString::new() });
    if mode == Mode::Custom { return }
    let pick = |names: ModelRc<SharedString>, key: &str| (0..names.row_count()).find(|r| names.row_data(*r).is_some_and(|n| n == key)).map(|r| r as i32);
    if let Some(r) = pick(g.get_record_sound_names(), &p.sound().key()) { g.set_record_sound_index(r) }
    if let Some(r) = pick(g.get_stream_sound_names(), &p.stream_sound) { g.set_stream_sound_index(r) }
    g.set_stream_input2_index(0);
    g.set_stream_input3_index(0);
    fill_filters(g, &p.filters());
    g.set_stream_quiet_noise(p.stream_quiet_noise);
    g.set_stream_even(p.stream_even.unwrap_or(true));
    g.set_stream_sound_mode(i);
}

/// The phone page's addresses, and "Sound setup".
pub(crate) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<SettingsDlg>();
    let app = op.app();
    let host = app.lan_url.as_deref().and_then(|u| u.strip_prefix("http://")).and_then(|u| u.split('/').next()).unwrap_or("").to_string();
    g.set_stream_phone_address(if host.is_empty() { SharedString::new() } else { format!("http://{host}/stage-view/mic").into() });
    let secure = Some(crate::network::https::port_for(app.port)).filter(|_| !host.is_empty());
    g.set_stream_phone_secure_address(secure.map(|p| crate::network::phone_mic::secure_url(&host, p)).unwrap_or_default().into());
    let me = Rc::downgrade(op);
    g.on_stream_sound_mode_chosen(move |i| {
        let Some(op) = me.upgrade() else { return };
        choose_mode(&op.ui.global::<SettingsDlg>(), i);
    });
}

/// `Sound::key` of the phone, for "Sound in the recording".
pub fn phone_key() -> String { Sound::Input(phone_mic::INPUT.into()).key() }

#[cfg(test)]
mod tests {
    use super::*;

    fn i(name: &str) -> Input { Input { name: name.into(), label: name.into() } }

    #[test]
    fn the_other_inputs_list_none_each_input_and_the_phone() {
        let c = input_choices(&[i("Line In")], "");
        assert_eq!(c.names, ["", "Line In", phone_mic::INPUT]);
        assert_eq!((c.labels[0].as_str(), c.index), ("None", 0));
        let c = input_choices(&[], "USB Mic");
        assert_eq!((c.names.last().map(String::as_str), c.index), (Some("USB Mic"), 2), "unplugged: kept");
        assert_eq!(c.labels[2], "USB Mic (not connected)");
        let l = listen_choices(&[crate::video::ffi::AudioDevice { name: "wasapi/{h}".into(), label: "Headphones".into() }], "wasapi/{h}");
        assert_eq!((l.names, l.index), (vec![String::new(), listen::DEFAULT.into(), "wasapi/{h}".into()], 2));
    }

    #[test]
    fn filters_go_to_the_form_and_back() {
        let f = Filters { gate: Strength::Strong, even: Strength::Light, limit: false, low_db: -6, mid_db: 2, high_db: 3 };
        assert_eq!(from_ui(&to_ui(f)), f);
        assert_eq!(from_ui(&SoundFilters { low: 40, ..to_ui(f) }).low_db, 12, "kept to its range");
    }
}
