//! Settings → General → Screens and graphics and media (N21, N24): the form's fields, the headphones list, and "Clean up unused media…" (N23), which
//! File → Clean up unused media… opens too.
use super::{ask::notice, confirm, settings_audio, strings as dlg};
use crate::{
    media_prefs::{self, CHANNELS, ENDS, MediaPrefs, PICTURE_SECS, SCALES},
    ui::{SettingsDlg, operator::window::Operator},
    video::ffi::AudioDevice,
};
use serde::Deserialize;
use serde_json::json;
use slint::{Model, ModelRc, VecModel};
#[cfg(test)]
use slint::ComponentHandle;
use std::rc::Rc;

fn pos<T: PartialEq>(all: &[T], v: &T) -> i32 { all.iter().position(|x| x == v).unwrap_or(0) as i32 }
fn at<T: Copy>(all: &[T], i: i32, or: T) -> T { usize::try_from(i).ok().and_then(|i| all.get(i).copied()).unwrap_or(or) }

pub(super) fn fill_form(g: &SettingsDlg, m: &MediaPrefs, devices: &[AudioDevice]) {
    g.set_media_scale(pos(&SCALES, &m.scale));
    g.set_picture_secs(media_prefs::picture_index(m.picture_secs) as i32);
    g.set_video_end(pos(&ENDS, &m.video_end));
    g.set_delay_ms(m.delay_ms.to_string().into());
    g.set_channels(pos(&CHANNELS, &m.channels));
    fill_listen(g, devices, &m.listen_device);
}

/// Reads the form back. A delay that isn't a number keeps the one saved.
pub(super) fn read_form(g: &SettingsDlg, m: &mut MediaPrefs) {
    m.scale = at(&SCALES, g.get_media_scale(), m.scale);
    m.picture_secs = at(&PICTURE_SECS, g.get_picture_secs(), m.picture_secs);
    m.video_end = at(&ENDS, g.get_video_end(), m.video_end);
    if let Some(ms) = media_prefs::parse_delay(&g.get_delay_ms()) { m.delay_ms = ms }
    m.channels = at(&CHANNELS, g.get_channels(), m.channels);
    m.listen_device = read_listen(g);
}

/// The headphones list: the sound outputs as Settings → General → Screens and graphics lists them, with "None" first instead of the default.
pub(super) fn fill_listen(g: &SettingsDlg, devices: &[AudioDevice], saved: &str) {
    let mut c = settings_audio::choices(devices, saved);
    c.labels[0] = crate::tr!("None");
    let strings = |v: Vec<String>| ModelRc::new(VecModel::from(v.into_iter().map(Into::into).collect::<Vec<slint::SharedString>>()));
    g.set_listen_labels(strings(c.labels));
    g.set_listen_names(strings(c.names));
    g.set_listen_index(c.index as i32);
}

pub(super) fn read_listen(g: &SettingsDlg) -> String {
    g.get_listen_names().row_data(g.get_listen_index().max(0) as usize).unwrap_or_default().into()
}

#[derive(Deserialize)]
struct Unused { nothing: bool, question: String }

#[derive(Deserialize)]
struct Cleaned { toast: String }

/// Asks what a clean-up would take out, shows it, and takes it out on Yes. The ids sent are the ones the question
/// was about: the core takes out only those that are still unused.
pub fn clean_up(op: &Rc<Operator>) {
    let me = Rc::downgrade(op);
    op.ops().call::<Unused>("media/unused", json!({}), move |r| {
        let Some(op) = me.upgrade() else { return };
        let u = match r { Ok(u) => u, Err(e) => return op.toast(e, true) };
        if u.nothing { return notice(&op, &nothing_to_clean()) }
        let ids = op.state.borrow().snap.as_ref().map(|s| unused_ids(&s.data)).unwrap_or_default();
        let me = Rc::downgrade(&op);
        confirm(&op, &u.question, &clean_up_label(), &dlg::cancel(), true, move |yes| {
            let Some(op) = me.upgrade().filter(|_| yes) else { return };
            let me = Rc::downgrade(&op);
            op.ops().call::<Cleaned>("media/cleanup", json!({ "ids": ids }), move |r| {
                let Some(op) = me.upgrade() else { return };
                match r { Ok(c) => op.toast(c.toast, false), Err(e) => op.toast(e, true) }
            });
        });
    });
}

/// The library entries no service or template uses, as the operator's snapshot has them (what the question listed).
fn unused_ids(data: &crate::model::Data) -> Vec<uuid::Uuid> {
    crate::media_care::find(data, &Default::default(), &[]).entries.into_iter().map(|(id, _)| id).collect()
}

fn nothing_to_clean() -> String { crate::tr!("Nothing to clean up: every file in the media library is used.") }
fn clean_up_label() -> String { crate::tr!("Clean up") }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::test_app,
        model::{EndAction, MediaFile, MediaKind, MediaStatus},
        media_prefs::{Channels, Scale},
        ui::dialogs::{SettingsTab, settings},
        ui::operator::window::testing::{open as open_window, refresh},
    };

    fn dev(name: &str, label: &str) -> AudioDevice { AudioDevice { name: name.into(), label: label.into() } }

    #[test]
    fn the_form_shows_and_reads_back_the_settings() {
        crate::ui::ui_test_init();
        let app = test_app();
        let prefs = MediaPrefs { scale: Scale::Stretch, picture_secs: 10, video_end: EndAction::Next, listen_device: "wasapi/{phones}".into(), delay_ms: 120, channels: Channels::Left };
        app.data.lock().unwrap().settings.media = prefs.clone();
        let op = open_window(&app);
        refresh(&op);
        settings::open(&op, SettingsTab::Media);
        let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
        assert_eq!(g.get_tab(), settings::tab_index(SettingsTab::Media));
        assert_eq!((g.get_media_scale(), g.get_picture_secs(), g.get_video_end(), g.get_channels()), (2, 3, 3, 3));
        assert_eq!(g.get_delay_ms(), "120");
        assert_eq!(read_listen(&g), "wasapi/{phones}", "kept while the list is read");
        let mut back = MediaPrefs::default();
        read_form(&g, &mut back);
        assert_eq!(back, prefs);
        g.set_delay_ms("soon".into());
        g.set_media_scale(1);
        read_form(&g, &mut back);
        assert_eq!((back.delay_ms, back.scale), (120, Scale::Fill), "a delay that isn't a number keeps the saved one");
    }

    #[test]
    fn the_headphones_list_starts_with_none() {
        crate::ui::ui_test_init();
        let app = test_app();
        let op = open_window(&app);
        let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
        fill_listen(&g, &[dev("wasapi/{a}", "Speakers"), dev("wasapi/{b}", "Headphones")], "wasapi/{b}");
        let labels: Vec<String> = g.get_listen_labels().iter().map(Into::into).collect();
        assert_eq!(labels, ["None", "Speakers", "Headphones"]);
        assert_eq!(g.get_listen_index(), 2);
        fill_listen(&g, &[dev("wasapi/{a}", "Speakers")], "");
        assert_eq!(read_listen(&g), "", "None");
    }

    #[test]
    fn the_snapshot_names_the_entries_no_service_uses() {
        let mut d = crate::model::Data::default();
        let f = MediaFile { id: uuid::Uuid::new_v4(), name: "Old".into(), kind: MediaKind::Video, asset: "o.mp4".into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        d.media.push(f.clone());
        assert_eq!(unused_ids(&d), [f.id]);
    }
}
