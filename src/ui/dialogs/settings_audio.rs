//! Settings → Video: where sound goes (spec §6.4). The list comes from libmpv; the saved choice stays even when its
//! device is unplugged, so that plugging it back in is all it takes.
use super::settings::strings as tr;
use crate::{ui::{SettingsDlg, operator::window::Operator}, video::ffi::AudioDevice};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::rc::Rc;

/// The lines of the list, the device each stands for, and which is chosen.
#[derive(Debug, PartialEq)]
pub struct Choices { pub labels: Vec<String>, pub names: Vec<String>, pub index: usize }

pub fn choices(devices: &[AudioDevice], saved: &str) -> Choices {
    let mut labels = vec![tr::audio_default()];
    let mut names = vec![String::new()];
    labels.extend(devices.iter().map(|d| d.label.clone()));
    names.extend(devices.iter().map(|d| d.name.clone()));
    if !saved.is_empty() && !names.iter().any(|n| n == saved) {
        labels.push(tr::audio_saved_gone().into());
        names.push(saved.into());
    }
    let index = names.iter().position(|n| n == saved).unwrap_or(0);
    Choices { labels, names, index }
}

pub(super) fn fill(g: &SettingsDlg, devices: &[AudioDevice], saved: &str) {
    let Choices { labels, names, index } = choices(devices, saved);
    let strings = |v: Vec<String>| ModelRc::new(VecModel::from(v.into_iter().map(Into::into).collect::<Vec<slint::SharedString>>()));
    g.set_audio_labels(strings(labels));
    g.set_audio_names(strings(names));
    g.set_audio_index(index as i32);
}

/// The device of the chosen line.
pub(super) fn read(g: &SettingsDlg) -> String {
    g.get_audio_names().row_data(g.get_audio_index().max(0) as usize).unwrap_or_default().into()
}

/// Asks for the list off the UI thread and shows it, keeping what is chosen by then.
pub(super) fn load(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ops().audio_devices(move |devices| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<SettingsDlg>();
        fill(&g, &devices, &read(&g));
        super::settings_media::fill_listen(&g, &devices, &super::settings_media::read_listen(&g));
        super::settings_record::fill_stream_sound(&g, &devices, &super::settings_record::read_stream_sound(&g));
        super::settings_sound::fill_listen(&g, &devices, &super::settings_sound::read_listen(&g));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, label: &str) -> AudioDevice { AudioDevice { name: name.into(), label: label.into() } }

    #[test]
    fn the_default_comes_first_and_is_chosen_when_nothing_is_saved() {
        let got = choices(&[dev("wasapi/{a}", "Speakers")], "");
        assert_eq!(got, Choices { labels: vec![tr::audio_default().into(), "Speakers".into()], names: vec![String::new(), "wasapi/{a}".into()], index: 0 });
    }

    #[test]
    fn a_saved_device_is_chosen() {
        let got = choices(&[dev("wasapi/{a}", "Speakers"), dev("wasapi/{b}", "Sound board")], "wasapi/{b}");
        assert_eq!(got.index, 2);
    }

    #[test]
    fn a_saved_device_that_is_gone_stays_chosen_and_says_so() {
        let got = choices(&[dev("wasapi/{a}", "Speakers")], "wasapi/{b}");
        assert_eq!(*got.labels.last().unwrap(), tr::audio_saved_gone());
        assert_eq!(got.names.last().unwrap(), "wasapi/{b}");
        assert_eq!(got.index, 2);
    }

    #[test]
    fn with_no_list_a_saved_device_is_kept() {
        assert_eq!(choices(&[], "wasapi/{b}").names, ["", "wasapi/{b}"]);
    }
}
