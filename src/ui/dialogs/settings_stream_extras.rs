//! Settings → Media → Livestream's last rows (`ui/dialogs/settings-stream-extras.slint`): live captions
//! (`stream::captions`), the website player (`stream::website`) and the first-time Go Live helper.
use crate::{
    record::{RecordPrefs, run},
    stream::{self, website},
    ui::{SettingsDlg, operator::window::Operator},
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc};

thread_local! {
    /// The captions' input list's names, in its order (the first, empty: the stream's own sound input).
    static INPUTS: RefCell<Vec<String>> = RefCell::default();
}

/// The captions' input list: the stream's own input first, then each input, and a saved one that is not plugged in.
pub fn input_choices(inputs: &[crate::record::args::Input], saved: &str) -> (Vec<String>, Vec<String>, usize) {
    let mut labels = vec![tr::stream_input()];
    let mut names = vec![String::new()];
    for i in inputs { labels.push(i.label.clone()); names.push(i.name.clone()) }
    if !saved.is_empty() && !names.iter().any(|n| n == saved) { labels.push(saved.to_string()); names.push(saved.to_string()) }
    let index = names.iter().position(|n| n == saved).unwrap_or(0);
    (labels, names, index)
}

fn strings(v: Vec<String>) -> ModelRc<SharedString> { ModelRc::new(VecModel::from(v.into_iter().map(Into::into).collect::<Vec<SharedString>>())) }

/// Puts the settings into the form (with the input list as far as it is known).
pub(crate) fn fill_form(g: &SettingsDlg, p: &RecordPrefs) {
    let c = &p.extras.captions;
    g.set_captions_on(c.on);
    g.set_captions_picture(c.on_picture());
    g.set_captions_file(c.file());
    g.set_captions_youtube(c.youtube.as_str().into());
    fill_inputs(g, &[], &c.input);
    g.set_captions_model(model_ready());
    g.set_website_youtube(p.extras.website.youtube.as_str().into());
    g.set_website_facebook(p.extras.website.facebook.as_str().into());
    g.set_website_note("".into());
}

fn fill_inputs(g: &SettingsDlg, inputs: &[crate::record::args::Input], saved: &str) {
    let (labels, names, index) = input_choices(inputs, saved);
    g.set_captions_inputs(strings(labels));
    g.set_captions_input(index as i32);
    INPUTS.with(|i| *i.borrow_mut() = names);
}

/// Reads the form back.
pub(crate) fn read_form(g: &SettingsDlg, p: &mut RecordPrefs) {
    let c = &mut p.extras.captions;
    c.on = g.get_captions_on();
    c.on_picture = (!g.get_captions_picture()).then_some(false);
    c.file = (!g.get_captions_file()).then_some(false);
    c.youtube = g.get_captions_youtube().trim().to_string();
    c.input = INPUTS.with(|i| i.borrow().get(g.get_captions_input().max(0) as usize).cloned()).unwrap_or_else(|| c.input.clone());
    p.extras.website.youtube = g.get_website_youtube().trim().to_string();
    p.extras.website.facebook = g.get_website_facebook().trim().to_string();
}

/// Whether the English letter model captions need is downloaded.
fn model_ready() -> bool {
    #[cfg(test)]
    return true;
    #[cfg(not(test))]
    crate::follow::fetch::installed(&crate::follow::fetch::deps_dir(), &crate::follow::fetch::FAST).is_some()
}

/// The sound inputs, asked of FFmpeg off the UI thread when Settings opens.
pub(crate) fn load(op: &Rc<Operator>) {
    if op.ops().is_inline() { return }
    let data = op.app().store.root.clone();
    let ui = op.ui.as_weak();
    let _ = std::thread::Builder::new().name("settings-captions".into()).spawn(move || {
        let inputs = run::find_ffmpeg(&data).map(|f| run::list_inputs(&f)).unwrap_or_default();
        let _ = ui.upgrade_in_event_loop(move |ui| {
            let g = ui.global::<SettingsDlg>();
            let saved = INPUTS.with(|i| i.borrow().get(g.get_captions_input().max(0) as usize).cloned()).unwrap_or_default();
            fill_inputs(&g, &inputs, &saved);
        });
    });
}

/// "Copy the code": the website code for YouTube (0) or Facebook (1), or what to type first.
fn copy(op: &Rc<Operator>, which: i32) {
    let g = op.ui.global::<SettingsDlg>();
    let code = if which == 0 {
        website::channel_id(&g.get_website_youtube()).map(|id| website::youtube_code(&id)).ok_or_else(website::tr::no_channel)
    } else {
        website::facebook_page(&g.get_website_facebook()).map(|p| website::facebook_code(&p)).ok_or_else(website::tr::no_page)
    };
    match code {
        Ok(code) => {
            g.set_website_note(website::tr::how().into());
            super::settings::copy(op, &code, &website::tr::copied());
        }
        Err(why) => g.set_website_note(why.into()),
    }
}

/// "Fill in": the signed-in YouTube account's channel id, asked of YouTube off the UI thread.
fn fill_channel(op: &Rc<Operator>) {
    let store = stream::keys::app_store();
    let Some((_, token)) = stream::accounts::kept(store.as_ref(), stream::built_in_id(0)) else { return };
    let (ids, ends) = (stream::accounts::app_ids(), stream::accounts::app_ends());
    let ui = op.ui.as_weak();
    let _ = std::thread::Builder::new().name("settings-channel".into()).spawn(move || {
        let found = (|| {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
            let client = crate::bible::sources::client().map_err(|e| format!("{e:?}"))?;
            rt.block_on(stream::accounts::google::channel_id(client, &ends, &ids, &token))
        })();
        let _ = ui.upgrade_in_event_loop(move |ui| {
            let g = ui.global::<SettingsDlg>();
            match found {
                Ok(id) => { g.set_website_youtube(id.into()); g.set_website_note("".into()) }
                Err(e) => g.set_website_note(tr::no_fill(&e).into()),
            }
        });
    });
}

/// "Use the guest as a camera": the guest's address in the first camera row that has none (camera 1 to 4), as a
/// network camera. Saved with the other settings.
fn guest_camera(op: &Rc<Operator>) {
    let g = op.ui.global::<SettingsDlg>();
    let address: SharedString = crate::network::guest::camera_address(crate::startup::port()).into();
    use slint::Model;
    let rows: [(i32, usize, SharedString, &dyn Fn(i32), &dyn Fn(SharedString)); 4] = [
        (g.get_stream_camera_index(), g.get_stream_camera_labels().row_count(), g.get_stream_camera_address(), &|i| g.set_stream_camera_index(i), &|a| g.set_stream_camera_address(a)),
        (g.get_stream_camera2_index(), g.get_stream_camera2_labels().row_count(), g.get_stream_camera2_address(), &|i| g.set_stream_camera2_index(i), &|a| g.set_stream_camera2_address(a)),
        (g.get_stream_camera3_index(), g.get_stream_camera3_labels().row_count(), g.get_stream_camera3_address(), &|i| g.set_stream_camera3_index(i), &|a| g.set_stream_camera3_address(a)),
        (g.get_stream_camera4_index(), g.get_stream_camera4_labels().row_count(), g.get_stream_camera4_address(), &|i| g.set_stream_camera4_index(i), &|a| g.set_stream_camera4_address(a)),
    ];
    if rows.iter().any(|(_, _, a, _, _)| *a == address) { return g.set_guest_note(tr::guest_already().into()) }
    match rows.iter().enumerate().find(|(_, (i, _, _, _, _))| *i == 0) {
        Some((n, (_, count, _, set_index, set_address))) => {
            // The last choice of each list is "Network camera", with the address typed under it.
            set_index(*count as i32 - 1);
            set_address(address);
            g.set_guest_note(tr::guest_camera(n + 1).into());
        }
        None => g.set_guest_note(tr::guest_no_row().into()),
    }
}

pub(crate) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<SettingsDlg>();
    let me = Rc::downgrade(op);
    g.on_guest_camera(move || if let Some(op) = me.upgrade() { guest_camera(&op) });
    let me = Rc::downgrade(op);
    g.on_guest_invite(move || if let Some(op) = me.upgrade() {
        let url = crate::network::guest::host_address(crate::startup::port());
        op.ui.global::<SettingsDlg>().set_guest_note(tr::guest_invite(&url).into());
        if !op.ops().is_inline() { let _ = crate::usage::open::open_file(std::path::Path::new(&url)); }
    });
    let me = Rc::downgrade(op);
    g.on_website_copy(move |i| if let Some(op) = me.upgrade() { copy(&op, i) });
    let me = Rc::downgrade(op);
    g.on_website_fill(move || if let Some(op) = me.upgrade() { fill_channel(&op) });
    let me = Rc::downgrade(op);
    g.on_go_live_helper(move || if let Some(op) = me.upgrade() {
        // What was changed in Settings is kept first only when Save is pressed: the helper saves its own choices.
        op.close_dialog();
        super::go_live_helper::open(&op);
    });
}

pub mod tr {
    use crate::tr;
    pub fn stream_input() -> String { tr!("The stream's own microphone or line-in") }
    pub fn no_fill(why: &str) -> String { tr!("YouTube did not say the channel's id: {why}", why = why) }
    pub fn guest_invite(url: &str) -> String { tr!("The invitation page opened in this computer's web browser. If it did not, open {url} there.", url = url) }
    pub fn guest_camera(n: usize) -> String { tr!("The guest is camera {n}. Save, then switch to that camera while live once the guest is connected.", n = n.to_string()) }
    pub fn guest_already() -> String { tr!("The guest is already one of the cameras above.") }
    pub fn guest_no_row() -> String { tr!("All four camera rows are in use. Set one to No camera first.") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::args::Input;

    #[test]
    fn captions_listen_to_the_streams_input_unless_another_is_chosen() {
        let ins = vec![Input { name: "Line In (USB)".into(), label: "Line In (USB)".into() }];
        let (labels, names, index) = input_choices(&ins, "");
        assert_eq!((labels.len(), names[0].as_str(), index), (2, "", 0));
        let (_, names, index) = input_choices(&ins, "Line In (USB)");
        assert_eq!(names[index], "Line In (USB)");
        let (labels, names, index) = input_choices(&ins, "Pulpit mic");
        assert_eq!((labels.len(), names[index].as_str()), (3, "Pulpit mic"), "a saved one not plugged in stays chosen");
    }
}
