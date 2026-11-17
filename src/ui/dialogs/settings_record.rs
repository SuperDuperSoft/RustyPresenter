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
fn stream_rows(p: &RecordPrefs) -> ([bool; 4], [bool; 4], String) {
    let (mut on, mut tall) = ([false; 4], [false; 4]);
    let mut url = String::new();
    for d in &p.stream {
        let Some(i) = (0..4).find(|&i| stream::built_in_id(i) == d.id) else { continue };
        on[i] = d.on;
        tall[i] = d.tall;
        if i == 3 { url = d.url.clone() }
    }
    (on, tall, url)
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

/// The key of the last choice in the camera lists, "Network camera": the address is typed below it.
const NETWORK_CAMERA: &str = "://";

/// The list "Camera": none, each camera, then a camera on the network; a saved one that is not plugged in stays
/// chosen, and a saved address chooses "Network camera".
pub fn camera_choices(cameras: &[Input], saved: &str) -> Choices {
    let mut labels = vec![crate::tr!("No camera")];
    let mut keys = vec![String::new()];
    labels.extend(cameras.iter().map(|c| c.label.clone()));
    keys.extend(cameras.iter().map(|c| c.name.clone()));
    let network = record::args::is_network_camera(saved);
    if !saved.is_empty() && !network && !keys.iter().any(|k| k == saved) {
        labels.push(format!("{} ({})", record::args::camera_label(saved), not_connected()));
        keys.push(saved.to_string());
    }
    labels.push(crate::tr!("Network camera"));
    keys.push(NETWORK_CAMERA.to_string());
    let index = if network { keys.len() - 1 } else { keys.iter().position(|k| k == saved).unwrap_or(0) };
    Choices { labels, keys, index }
}

/// The four camera lists, each with its saved camera (`saved[0]` the first).
fn fill_camera(g: &SettingsDlg, cameras: &[Input], saved: [&str; 4]) {
    let address = |s: &str| if record::args::is_network_camera(s) { s.into() } else { slint::SharedString::new() };
    let c = camera_choices(cameras, saved[0]);
    g.set_stream_camera_labels(strings(c.labels));
    g.set_stream_camera_names(strings(c.keys));
    g.set_stream_camera_index(c.index as i32);
    g.set_stream_camera_address(address(saved[0]));
    let c = camera_choices(cameras, saved[1]);
    g.set_stream_camera2_labels(strings(c.labels));
    g.set_stream_camera2_names(strings(c.keys));
    g.set_stream_camera2_index(c.index as i32);
    g.set_stream_camera2_address(address(saved[1]));
    let c = camera_choices(cameras, saved[2]);
    g.set_stream_camera3_labels(strings(c.labels));
    g.set_stream_camera3_names(strings(c.keys));
    g.set_stream_camera3_index(c.index as i32);
    g.set_stream_camera3_address(address(saved[2]));
    let c = camera_choices(cameras, saved[3]);
    g.set_stream_camera4_labels(strings(c.labels));
    g.set_stream_camera4_names(strings(c.keys));
    g.set_stream_camera4_index(c.index as i32);
    g.set_stream_camera4_address(address(saved[3]));
}

/// The camera chosen in a list: its name, or the address typed for "Network camera" (none if it is no address).
fn chosen_camera(names: slint::ModelRc<slint::SharedString>, index: i32, address: &str) -> String {
    let key = names.row_data(index.max(0) as usize).map(|s| s.to_string()).unwrap_or_default();
    if key != NETWORK_CAMERA { return key }
    let address = address.trim();
    if record::args::is_network_camera(address) { address.to_string() } else { String::new() }
}

fn read_camera(g: &SettingsDlg) -> String {
    chosen_camera(g.get_stream_camera_names(), g.get_stream_camera_index(), &g.get_stream_camera_address())
}

/// The four cameras chosen: each none without the one before it, or when it is one of those again.
fn read_cameras(g: &SettingsDlg) -> [String; 4] {
    let picked = [
        read_camera(g),
        chosen_camera(g.get_stream_camera2_names(), g.get_stream_camera2_index(), &g.get_stream_camera2_address()),
        chosen_camera(g.get_stream_camera3_names(), g.get_stream_camera3_index(), &g.get_stream_camera3_address()),
        chosen_camera(g.get_stream_camera4_names(), g.get_stream_camera4_index(), &g.get_stream_camera4_address()),
    ];
    let mut out: [String; 4] = Default::default();
    for (i, c) in picked.into_iter().enumerate() {
        if c.is_empty() || out[..i].iter().any(|o| o.is_empty() || *o == c) { break }
        out[i] = c;
    }
    out
}

pub(crate) fn fill_form(g: &SettingsDlg, p: &RecordPrefs) {
    fill_stream_sound(g, &[], &p.stream_sound);
    g.set_stream_sound_note(if crate::platform::tools::OS == crate::platform::tools::Os::MacOs { stream::tr::mac_needs_loopback().into() } else { Default::default() });
    fill_camera(g, &[], [&p.camera, &p.camera2, &p.camera3, &p.camera4]);
    g.set_stream_camera_control(p.camera_control.as_str().into());
    g.set_record_encoder(record::args::CHOOSABLE.iter().position(|e| *e == p.encoder).map_or(0, |i| i as i32 + 1));
    g.set_record_bitrate(record::BITRATES.iter().position(|b| *b == p.bitrate_kbps).map_or(0, |i| i as i32 + 1));
    use stream::compose::{Corner, CornerSize};
    g.set_stream_own_count(p.own_scenes.len() as i32);
    let l = p.stream_layout;
    g.set_stream_corner(match l.corner { Corner::TopLeft => 0, Corner::TopRight => 1, Corner::BottomLeft => 2, Corner::BottomRight => 3 });
    g.set_stream_corner_size(match l.corner_size { CornerSize::Small => 0, CornerSize::Medium => 1, CornerSize::Large => 2 });
    g.set_stream_words_top(i32::from(l.words_top));
    g.set_stream_camera2_control(p.camera2_control.as_str().into());
    g.set_stream_camera3_control(p.camera3_control.as_str().into());
    g.set_stream_camera4_control(p.camera4_control.as_str().into());
    g.set_stream_auto_scenes(p.auto().on);
    g.set_stream_sound_delay(p.sound_delay_ms.min(record::MAX_SOUND_DELAY) as i32);
    g.set_stream_delay(crate::stream::delay::DELAYS.iter().position(|d| *d == p.stream_delay_secs).map_or(0, |i| i as i32 + 1));
    let fx = p.sound_fx();
    g.set_stream_board_level(fx.board_db);
    g.set_stream_pc_level(fx.pc_db);
    g.set_stream_even(fx.even);
    g.set_stream_lighter(p.stream_lighter.unwrap_or(true));
    g.set_stream_quiet_noise(fx.quiet_noise);
    g.set_stream_tracks(fx.tracks);
    g.set_stream_ndi(p.ndi_stream);
    g.set_stream_ndi_words(p.ndi_words);
    g.set_stream_words_file(p.words_file);
    g.set_stream_camera_lost(p.camera_lost.index());
    g.set_stream_presentation_camera(i32::from(p.presentation_camera));
    let c = &p.cards;
    g.set_stream_card_words_0(c.starting.words.as_str().into());
    g.set_stream_card_words_1(c.ending.words.as_str().into());
    g.set_stream_card_words_2(c.hold.words.as_str().into());
    g.set_stream_card_picture_0(c.starting.picture.as_str().into());
    g.set_stream_card_picture_1(c.ending.picture.as_str().into());
    g.set_stream_card_picture_2(c.hold.picture.as_str().into());
    let (on, tall, url) = stream_rows(p);
    g.set_stream_tall_0(tall[0]);
    g.set_stream_tall_1(tall[1]);
    g.set_stream_tall_2(tall[2]);
    g.set_stream_tall_3(tall[3]);
    g.set_stream_on_0(on[0]);
    g.set_stream_on_1(on[1]);
    g.set_stream_on_2(on[2]);
    g.set_stream_on_3(on[3]);
    g.set_stream_url(url.into());
    fill_saved(g);
    fill_accounts(g);
    g.set_stream_sign_in_text("".into());
    g.set_stream_title(p.stream_title.as_str().into());
    g.set_stream_description(p.stream_description.as_str().into());
    g.set_stream_thumbnail(p.stream_thumbnail.as_str().into());
    g.set_stream_scheduled_id(p.stream_scheduled.as_str().into());
    g.set_stream_scheduled_link(p.stream_scheduled_link.as_str().into());
    g.set_stream_privacy(i32::from(p.stream_privacy == stream::accounts::Privacy::Unlisted));
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
    let tall = [g.get_stream_tall_0(), g.get_stream_tall_1(), g.get_stream_tall_2(), g.get_stream_tall_3()];
    let url = g.get_stream_url().trim().to_string();
    let rows: Vec<stream::Destination> = (0..4).filter(|&i| on[i] || (i == 3 && !url.is_empty())).map(|i| stream::Destination {
        id: stream::built_in_id(i), name: String::new(), service: stream::BUILT_IN[i].0, url: if i == 3 { url.clone() } else { String::new() }, key: String::new(), on: on[i],
        tall: tall[i],
    }).collect();
    if rows != p.stream { p.stream = rows }
    let pc = read_stream_sound(g);
    if pc != p.stream_sound { p.stream_sound = pc }
    let [camera, camera2, camera3, camera4] = read_cameras(g);
    if camera != p.camera { p.camera = camera }
    if camera2 != p.camera2 { p.camera2 = camera2 }
    if camera3 != p.camera3 { p.camera3 = camera3 }
    if camera4 != p.camera4 { p.camera4 = camera4 }
    p.camera_control = g.get_stream_camera_control().trim().to_string();
    p.encoder = usize::try_from(g.get_record_encoder() - 1).ok().and_then(|i| record::args::CHOOSABLE.get(i)).map(|e| e.to_string()).unwrap_or_default();
    p.bitrate_kbps = usize::try_from(g.get_record_bitrate() - 1).ok().and_then(|i| record::BITRATES.get(i)).copied().unwrap_or(0);
    {
        use stream::compose::{Corner, CornerSize, Layout};
        p.stream_layout = Layout {
            corner: match g.get_stream_corner() { 0 => Corner::TopLeft, 1 => Corner::TopRight, 2 => Corner::BottomLeft, _ => Corner::BottomRight },
            corner_size: match g.get_stream_corner_size() { 0 => CornerSize::Small, 2 => CornerSize::Large, _ => CornerSize::Medium },
            words_top: g.get_stream_words_top() == 1,
        };
    }
    p.camera2_control = g.get_stream_camera2_control().trim().to_string();
    p.camera3_control = g.get_stream_camera3_control().trim().to_string();
    p.camera4_control = g.get_stream_camera4_control().trim().to_string();
    // Unchanged keeps the saved form (`None` is on), so an untouched form has no changes.
    let auto = g.get_stream_auto_scenes();
    if auto != p.auto().on { p.auto_scenes = if auto { None } else { Some(false) } }
    p.presentation_camera = g.get_stream_presentation_camera() == 1;
    p.sound_delay_ms = (g.get_stream_sound_delay().max(0) as u32).min(record::MAX_SOUND_DELAY);
    p.stream_delay_secs = usize::try_from(g.get_stream_delay() - 1).ok().and_then(|i| crate::stream::delay::DELAYS.get(i)).copied().unwrap_or(0);
    let level = |db: i32| db.clamp(-record::MAX_LEVEL_DB, record::MAX_LEVEL_DB);
    p.stream_board_db = level(g.get_stream_board_level());
    p.stream_pc_db = level(g.get_stream_pc_level());
    p.stream_even = if g.get_stream_even() { None } else { Some(false) };
    p.stream_lighter = if g.get_stream_lighter() { None } else { Some(false) };
    p.stream_quiet_noise = g.get_stream_quiet_noise();
    p.stream_tracks = g.get_stream_tracks();
    p.ndi_stream = g.get_stream_ndi();
    p.ndi_words = g.get_stream_ndi_words();
    p.words_file = g.get_stream_words_file();
    p.camera_lost = stream::camera_lost::CameraLost::from_index(g.get_stream_camera_lost());
    let card = |w: slint::SharedString, pic: slint::SharedString| stream::pictures::Card { words: w.trim().to_string(), picture: pic.trim().to_string() };
    p.cards = stream::pictures::Cards {
        starting: card(g.get_stream_card_words_0(), g.get_stream_card_picture_0()),
        ending: card(g.get_stream_card_words_1(), g.get_stream_card_picture_1()),
        hold: card(g.get_stream_card_words_2(), g.get_stream_card_picture_2()),
    };
    p.stream_title = g.get_stream_title().trim().to_string();
    p.stream_description = g.get_stream_description().trim().to_string();
    p.stream_thumbnail = g.get_stream_thumbnail().trim().to_string();
    p.stream_scheduled = g.get_stream_scheduled_id().to_string();
    p.stream_scheduled_link = g.get_stream_scheduled_link().to_string();
    p.stream_privacy = if g.get_stream_privacy() == 1 { stream::accounts::Privacy::Unlisted } else { stream::accounts::Privacy::Public };
}

/// Which of YouTube and Facebook this copy can sign in to, and the account each is signed in to on this computer.
pub(crate) fn fill_accounts(g: &SettingsDlg) {
    let store = stream::keys::app_store();
    let ids = stream::accounts::app_ids();
    let name = |i: usize| stream::accounts::kept(store.as_ref(), stream::built_in_id(i))
        .map(|(n, _)| if n.is_empty() { stream::BUILT_IN[i].0.name().to_string() } else { n }).unwrap_or_default();
    g.set_stream_can_sign_in_0(ids.can_sign_in(stream::Service::YouTube));
    g.set_stream_can_sign_in_1(ids.can_sign_in(stream::Service::Facebook));
    g.set_stream_account_0(name(0).into());
    g.set_stream_account_1(name(1).into());
    g.set_stream_can_sign_in_2(ids.can_sign_in(stream::Service::Twitch));
    g.set_stream_account_2(name(2).into());
}

/// Signing in to YouTube (row 0: the web browser opens) or Facebook (row 1: a code to type at facebook.com/device),
/// off the UI thread; what is kept goes to the computer's password store. Gives the account's name.
fn sign_in(i: usize, ui: slint::Weak<crate::ui::OperatorWindow>) -> Result<String, String> {
    use stream::accounts::{self, facebook, google, twitch};
    let (ids, ends, store) = (accounts::app_ids(), accounts::app_ends(), stream::keys::app_store());
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    let client = crate::bible::sources::client().map_err(|e| format!("{e:?}"))?;
    let open = |url: &str| { let _ = crate::usage::open::open_file(std::path::Path::new(url)); };
    let (token, name) = rt.block_on(async {
        if i == 0 { return google::sign_in(client, &ends, &ids, open).await }
        if i == 2 {
            let device = twitch::start(client, &ends, &ids).await?;
            let text = accounts::tr::twitch_code(&device.user_code, &device.at);
            let _ = ui.upgrade_in_event_loop(move |ui| ui.global::<SettingsDlg>().set_stream_sign_in_text(text.into()));
            open(&device.at);
            return twitch::wait(client, &ends, &ids, &device).await;
        }
        let device = facebook::start(client, &ends, &ids).await?;
        let text = accounts::tr::facebook_code(&device.user_code, &device.at);
        let _ = ui.upgrade_in_event_loop(move |ui| ui.global::<SettingsDlg>().set_stream_sign_in_text(text.into()));
        open(&device.at);
        facebook::wait(client, &ends, &ids, &device).await
    })?;
    accounts::keep(store.as_ref(), stream::built_in_id(i), &name, &token)?;
    Ok(name)
}

fn signed_in(g: &SettingsDlg, r: Result<String, String>) {
    g.set_stream_signing_in(false);
    g.set_stream_sign_in_text(match r {
        Ok(name) => stream::accounts::tr::signed_in(&name),
        Err(e) => { eprintln!("stream sign-in: {e}"); stream::accounts::tr::sign_in_failed(&e) }
    }.into());
    fill_accounts(g);
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
    let me = Rc::downgrade(op);
    op.ui.global::<SettingsDlg>().on_stream_sign_in(move |i| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<SettingsDlg>();
        if g.get_stream_signing_in() { return }
        let i = i.clamp(0, 2) as usize;
        g.set_stream_signing_in(true);
        g.set_stream_sign_in_text(if i == 0 { stream::accounts::tr::open_browser() } else { String::new() }.into());
        let ui = op.ui.as_weak();
        // Tests have no event loop: the answer is waited for here.
        if op.ops().is_inline() { return signed_in(&g, sign_in(i, ui)) }
        let _ = std::thread::Builder::new().name("stream-sign-in".into()).spawn(move || {
            let r = sign_in(i, ui.clone());
            let _ = ui.upgrade_in_event_loop(move |ui| signed_in(&ui.global::<SettingsDlg>(), r));
        });
    });
    let me = Rc::downgrade(op);
    op.ui.global::<SettingsDlg>().on_stream_schedule(move || {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<SettingsDlg>();
        if g.get_stream_signing_in() { return }
        let Some((_, refresh)) = stream::accounts::kept(stream::keys::app_store().as_ref(), stream::built_in_id(0)) else { return };
        // The broadcast as Go Live would make it, for the open service's day and start time.
        let mut prefs = RecordPrefs::default();
        read_form(&g, &mut prefs);
        let (day, time) = {
            let d = op.app().data.lock().unwrap();
            (d.service().and_then(|s| s.date), crate::automation::actions::starts_at(&d).map(|t| (u32::from(t.hour), u32::from(t.minute))))
        };
        let at = stream::accounts::google::schedule_time(day, time, chrono::Local::now());
        let details = stream::accounts::Details {
            title: stream::accounts::title_for(&prefs.stream_title, at.date_naive(), stream::Service::YouTube),
            description: prefs.stream_description.clone(), privacy: prefs.stream_privacy, thumbnail: prefs.stream_thumbnail.clone(), scheduled: String::new(),
        };
        g.set_stream_signing_in(true);
        g.set_stream_sign_in_text(stream::accounts::tr::scheduling().into());
        let work = move || -> Result<(String, String), String> {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
            let client = crate::bible::sources::client().map_err(|e| format!("{e:?}"))?;
            let (ids, ends) = (stream::accounts::app_ids(), stream::accounts::app_ends());
            rt.block_on(stream::accounts::google::schedule(client, &ends, &ids, &refresh, &details, at.with_timezone(&chrono::Utc)))
        };
        let when = at.format("%A %-d %B, %H:%M").to_string();
        let done = move |g: &SettingsDlg, r: Result<(String, String), String>| {
            g.set_stream_signing_in(false);
            match r {
                Ok((id, link)) => {
                    g.set_stream_scheduled_id(id.into());
                    g.set_stream_scheduled_link(link.as_str().into());
                    g.set_stream_sign_in_text(stream::accounts::tr::scheduled(&when, &link).into());
                }
                Err(e) => { eprintln!("youtube: making the broadcast ahead: {e}"); g.set_stream_sign_in_text(stream::accounts::tr::schedule_failed(&e).into()) }
            }
        };
        if op.ops().is_inline() { return done(&g, work()) }
        let ui = op.ui.as_weak();
        let _ = std::thread::Builder::new().name("youtube-schedule".into()).spawn(move || {
            let r = work();
            let _ = ui.upgrade_in_event_loop(move |ui| done(&ui.global::<SettingsDlg>(), r));
        });
    });
    let me = Rc::downgrade(op);
    op.ui.global::<SettingsDlg>().on_stream_copy_link(move || {
        let Some(op) = me.upgrade() else { return };
        let link = op.ui.global::<SettingsDlg>().get_stream_scheduled_link();
        if !link.is_empty() { crate::ui::dialogs::settings::copy(&op, &link, &stream::accounts::tr::link_copied()) }
    });
    let me = Rc::downgrade(op);
    op.ui.global::<SettingsDlg>().on_stream_sign_out(move |i| {
        let Some(op) = me.upgrade() else { return };
        stream::accounts::forget(stream::keys::app_store().as_ref(), stream::built_in_id(i.clamp(0, 2) as usize));
        let g = op.ui.global::<SettingsDlg>();
        g.set_stream_sign_in_text("".into());
        fill_accounts(&g);
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
    op.ui.global::<SettingsDlg>().set_phone_camera_help(crate::network::phone_camera::settings_help(op.app()).into());
    let me = Rc::downgrade(op);
    let work = move || {
        let (inputs, mut cameras) = record::run::find_ffmpeg(&data).map(|f| (record::run::list_inputs(&f), record::run::list_cameras(&f))).unwrap_or_default();
        // The NDI sources on the network, when NDI is installed.
        let ready = stream::ndi::runtime().is_some();
        cameras.extend(stream::ndi::sources(std::time::Duration::from_millis(1500)).into_iter()
            .map(|n| Input { label: stream::ndi::label(&n), name: format!("{}{n}", stream::ndi::SCHEME) }));
        // The phones that have sent pictures since the app started (`stream::phone_camera`).
        cameras.extend(stream::phone_camera::names().into_iter().map(|n| Input { label: stream::phone_camera::label(&n), name: format!("{}{n}", stream::phone_camera::SCHEME) }));
        (inputs, cameras, ready)
    };
    let done = move |(inputs, cameras, ready): (Vec<Input>, Vec<Input>, bool)| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<SettingsDlg>();
        g.set_ndi_ready(ready);
        let chosen = read_sound(&g);
        fill_sound(&g, &inputs, &chosen);
        let chosen = read_cameras(&g);
        fill_camera(&g, &cameras, [&chosen[0], &chosen[1], &chosen[2], &chosen[3]]);
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
            match i { 0 => g.set_stream_card_picture_0(path), 1 => g.set_stream_card_picture_1(path), 3 => g.set_stream_thumbnail(path), _ => g.set_stream_card_picture_2(path) }
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
