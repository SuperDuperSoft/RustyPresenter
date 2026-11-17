//! The first-time Go Live helper (`ui/dialogs/go-live-helper.slint`, pure steps in `stream::helper`): Go Live with
//! nowhere to stream to opens it. Its choices are saved into Settings → Media → Livestream as each step is left,
//! so closing it half way keeps what was done.
use super::DialogKind;
use crate::{
    platform::tools,
    record::{self, Sound, run},
    stream::{self, helper},
    ui::{GoLiveHelperDlg, operator::window::Operator},
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc, time::Duration};

pub fn title() -> String { crate::tr!("Go Live: getting started") }

/// The lists behind the dialog's choices: each camera's name and each sound's saved key, in the lists' order.
#[derive(Default)]
struct Lists { cameras: Vec<String>, sounds: Vec<String> }

thread_local! { static LISTS: RefCell<Lists> = RefCell::default(); }

fn strings(v: Vec<String>) -> ModelRc<SharedString> { ModelRc::new(VecModel::from(v.into_iter().map(Into::into).collect::<Vec<SharedString>>())) }

/// Runs `work` off the UI thread and `done` with its answer back on it (at once in tests, which have no event loop).
fn off_ui<T: Send + 'static>(op: &Operator, work: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static) {
    if op.ops().is_inline() { return done(work()) }
    let (tx, rx) = tokio::sync::oneshot::channel();
    if std::thread::Builder::new().name("go-live-helper".into()).spawn(move || { let _ = tx.send(work()); }).is_err() { return }
    if let Err(e) = slint::spawn_local(async move { if let Ok(v) = rx.await { done(v) } }) { eprintln!("go live helper: {e}") }
}

fn prefs(op: &Operator) -> record::RecordPrefs { op.state.borrow().snap.as_ref().map(|s| s.settings.record.clone()).unwrap_or_default() }

/// Opens the helper on its first step.
pub fn open(op: &Rc<Operator>) {
    op.open_dialog(DialogKind::GoLiveHelper);
    let g = op.ui.global::<GoLiveHelperDlg>();
    let p = prefs(op);
    g.set_step(0);
    g.set_service(0);
    g.set_key("".into());
    g.set_url(p.stream.iter().find(|d| d.id == stream::built_in_id(3)).map(|d| d.url.clone()).unwrap_or_default().into());
    g.set_error("".into());
    g.set_results(strings(vec![]));
    g.set_checking(false);
    service_changed(op, 0);
    fill_lists(&g, &p, &[], &[]);
    // The cameras and sound inputs are asked of FFmpeg off the UI thread.
    let data = op.app().store.root.clone();
    let me = Rc::downgrade(op);
    off_ui(op, move || run::find_ffmpeg(&data).map(|f| (run::list_cameras(&f), run::list_inputs(&f))), move |found| {
        let Some(op) = me.upgrade() else { return };
        if op.dialog() != Some(DialogKind::GoLiveHelper) { return }
        let g = op.ui.global::<GoLiveHelperDlg>();
        match found {
            Some((cams, ins)) => fill_lists(&g, &prefs(&op), &cams, &ins),
            None => g.set_error(record::tr::no_ffmpeg(&tools::ffmpeg_hint(tools::OS)).into()),
        }
    });
}

/// The camera and sound lists: "no camera" first, then each camera (the saved one kept even when it is not
/// plugged in); the sounds as Settings lists them.
fn fill_lists(g: &GoLiveHelperDlg, p: &record::RecordPrefs, cams: &[record::args::Input], ins: &[record::args::Input]) {
    let mut names = vec![String::new()];
    let mut labels = vec![tr::no_camera()];
    for c in cams { names.push(c.name.clone()); labels.push(c.label.clone()) }
    if !p.camera.is_empty() && !names.contains(&p.camera) { names.push(p.camera.clone()); labels.push(record::args::camera_label(&p.camera)) }
    let camera = names.iter().position(|n| *n == p.camera).unwrap_or(if names.len() > 1 { 1 } else { 0 });
    let sounds = super::settings_record::choices(ins, &p.sound());
    g.set_cameras(strings(labels));
    g.set_camera(camera as i32);
    g.set_sounds(strings(sounds.labels.clone()));
    g.set_sound(sounds.index as i32);
    LISTS.with(|l| *l.borrow_mut() = Lists { cameras: names, sounds: sounds.keys });
}

fn service_changed(op: &Operator, i: i32) {
    let g = op.ui.global::<GoLiveHelperDlg>();
    let i = i.clamp(0, 3) as usize;
    g.set_key_help(helper::key_help(i).into());
    g.set_test_help(helper::test_help(i).into());
    g.set_key_kept(stream::keys::app_store().get(stream::built_in_id(i)).is_some());
    g.set_error("".into());
}

/// The choices on the dialog now.
fn choice(g: &GoLiveHelperDlg) -> helper::Choice {
    LISTS.with(|l| {
        let l = l.borrow();
        helper::Choice {
            service: g.get_service().clamp(0, 3) as usize,
            url: g.get_url().to_string(),
            camera: l.cameras.get(g.get_camera().max(0) as usize).cloned().unwrap_or_default(),
            sound: l.sounds.get(g.get_sound().max(0) as usize).cloned().unwrap_or_default(),
        }
    })
}

/// Keeps the key typed (in the password store) and puts the choices so far into the settings. `upto`: the last
/// step whose choice counts (0 where, 1 camera, 2 sound).
fn save(op: &Operator, upto: i32) -> Result<(), String> {
    let g = op.ui.global::<GoLiveHelperDlg>();
    let mut c = choice(&g);
    let key = g.get_key().trim().to_string();
    if !key.is_empty() {
        stream::keys::app_store().set(stream::built_in_id(c.service), &key).map_err(|e| crate::tr!("The stream key could not be kept: {error}", error = e))?;
        g.set_key("".into());
        g.set_key_kept(true);
    }
    let old = prefs(op);
    if upto < 1 { c.camera = old.camera.clone() }
    if upto < 2 { c.sound = String::new() }
    op.app().edit(|d| { helper::apply(&mut d.settings.record, &c); Ok(()) })?;
    Ok(())
}

fn next(op: &Rc<Operator>) {
    let g = op.ui.global::<GoLiveHelperDlg>();
    let step = g.get_step();
    g.set_error("".into());
    if step == 0 {
        let c = choice(&g);
        if let Err(e) = helper::where_ready(&c, &g.get_key(), g.get_key_kept()) { return g.set_error(e.into()) }
    }
    if let Err(e) = save(op, step.min(2)) { return g.set_error(e.into()) }
    g.set_step((step + 1).min(4));
}

fn back(op: &Rc<Operator>) {
    let g = op.ui.global::<GoLiveHelperDlg>();
    g.set_error("".into());
    g.set_step((g.get_step() - 1).max(0));
}

/// The check: the internet's upload speed, a picture from the camera and a moment of the sound. Nothing is sent
/// to viewers.
fn check(op: &Rc<Operator>) {
    let g = op.ui.global::<GoLiveHelperDlg>();
    if g.get_checking() { return }
    g.set_checking(true);
    g.set_results(strings(vec![tr::checking()]));
    let c = choice(&g);
    let sound = Sound::parse(&c.sound);
    let data = op.app().store.root.clone();
    let me = Rc::downgrade(op);
    off_ui(op, move || {
        let mut lines = vec![];
        let speed = tokio::runtime::Builder::new_current_thread().enable_all().build().ok().and_then(|rt| {
            let client = crate::bible::sources::client().ok()?;
            Some(rt.block_on(stream::speed::measure(client, stream::speed::URL, stream::speed::BYTES)))
        });
        lines.push(match speed {
            Some(Ok(mbps)) => stream::speed::advice(mbps, 1).1,
            _ => tr::no_internet(),
        });
        let ffmpeg = run::find_ffmpeg(&data);
        if !c.camera.is_empty() {
            if stream::ndi::source_name(&c.camera).is_some() { lines.push(tr::ndi_camera()) }
            else if let Some(f) = &ffmpeg {
                let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error"].map(String::from).into();
                a.extend(record::args::camera_input(tools::OS, &c.camera));
                a.extend(["-frames:v", "1", "-f", "null", "-"].map(String::from));
                let ok = run_limited(f, &a, Duration::from_secs(15)).is_some_and(|(ok, _)| ok);
                lines.push(helper::camera_words(&record::args::camera_label(&c.camera), ok));
            }
        }
        if let (Sound::Input(name), Some(f)) = (&sound, &ffmpeg) {
            let format = record::args::input_format(tools::OS);
            let mut a: Vec<String> = ["-hide_banner", "-f", format, "-i"].map(String::from).into();
            a.push(record::args::device_input(format, name));
            a.extend(["-t", "3", "-af", "volumedetect", "-vn", "-f", "null", "-"].map(String::from));
            let db = run_limited(f, &a, Duration::from_secs(15)).filter(|(ok, _)| *ok).and_then(|(_, said)| helper::loudest(&said));
            lines.push(helper::sound_words(name, db));
        }
        lines
    }, move |lines| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<GoLiveHelperDlg>();
        g.set_checking(false);
        g.set_results(strings(lines));
    });
}

/// Runs FFmpeg for at most `limit` (a camera another program holds can make it wait): whether it ended well, and
/// what it printed.
fn run_limited(ffmpeg: &std::path::Path, args: &[String], limit: Duration) -> Option<(bool, String)> {
    let dir = std::env::temp_dir().join(format!("rp-helper-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).ok()?;
    let log = dir.join("ffmpeg.txt");
    let file = std::fs::File::create(&log).ok()?;
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(file);
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(crate::platform::proc::creation_flags()); }
    let mut child = cmd.spawn().ok()?;
    let until = std::time::Instant::now() + limit;
    let ok = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.success(),
            Ok(None) if std::time::Instant::now() < until => std::thread::sleep(Duration::from_millis(100)),
            _ => { let _ = child.kill(); let _ = child.wait(); break false }
        }
    };
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    Some((ok, said))
}

/// "Send a 20-second test": saved, then Go Live, stopped by itself after `helper::TEST_SECS`. The stream summary
/// said at Stop tells whether it reached the service.
fn test_stream(op: &Rc<Operator>) {
    if let Err(e) = save(op, 2) { return op.ui.global::<GoLiveHelperDlg>().set_error(e.into()) }
    op.close_dialog();
    op.go_live();
    if !op.is_recording() { return }
    op.toast(tr::testing(helper::TEST_SECS), false);
    let me = Rc::downgrade(op);
    slint::Timer::single_shot(Duration::from_secs(helper::TEST_SECS), move || {
        if let Some(op) = me.upgrade() && op.ui.get_live() { op.go_live() }
    });
}

pub(crate) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<GoLiveHelperDlg>();
    let me = Rc::downgrade(op);
    g.on_service_changed(move |i| if let Some(op) = me.upgrade() { service_changed(&op, i) });
    let me = Rc::downgrade(op);
    g.on_next(move || if let Some(op) = me.upgrade() { next(&op) });
    let me = Rc::downgrade(op);
    g.on_back(move || if let Some(op) = me.upgrade() { back(&op) });
    let me = Rc::downgrade(op);
    g.on_check(move || if let Some(op) = me.upgrade() { check(&op) });
    let me = Rc::downgrade(op);
    g.on_test_stream(move || if let Some(op) = me.upgrade() { test_stream(&op) });
    let me = Rc::downgrade(op);
    g.on_sign_in_instead(move || if let Some(op) = me.upgrade() {
        let _ = save(&op, 0);
        op.close_dialog();
        super::settings::open(&op, crate::ui::dialogs::SettingsTab::Media);
    });
    let me = Rc::downgrade(op);
    g.on_finish(move |go| if let Some(op) = me.upgrade() {
        op.close_dialog();
        if go { op.go_live() }
    });
}

pub mod tr {
    use crate::tr;
    pub fn no_camera() -> String { tr!("No camera: viewers see the room screen") }
    pub fn checking() -> String { tr!("Checking… (about 10 seconds)") }
    pub fn no_internet() -> String { tr!("Internet: the speed test could not reach the internet. Check this computer's connection.") }
    pub fn ndi_camera() -> String { tr!("Camera: an NDI camera is looked for when you go live.") }
    pub fn testing(secs: u64) -> String { tr!("Test stream: it stops by itself in {secs} seconds.", secs = secs.to_string()) }
}

#[cfg(test)]
#[path = "go_live_helper_tests.rs"]
mod tests;
