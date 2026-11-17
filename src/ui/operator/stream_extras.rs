//! Go Live's extras on the operator window: live captions while live (`stream::captions`), the health light and
//! status list beside Live (`stream::health`, `ui/operator/live-status.slint`), and the first-time helper when Go
//! Live has nowhere to go (`stream::helper`).
use super::window::Operator;
use crate::{
    record::{Plan, RecordPrefs, Sound},
    stream::{self, captions, health},
    ui::{LiveStatus, LiveStatusLine},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc, time::Duration};

/// What Go Live started besides the recording, until it stops.
#[derive(Default)]
struct Live {
    captions: Option<captions::Captioner>,
    /// The captions file, to say where it is at the end.
    captions_file: Option<std::path::PathBuf>,
    ndi: bool,
}

thread_local! { static LIVE: RefCell<Live> = RefCell::default(); }

impl Operator {
    /// Go Live with nothing set up opens the first-time helper instead (true when it did).
    pub(crate) fn go_live_helper_instead(self: &Rc<Self>) -> bool {
        if self.is_recording() { return false }
        let prefs = self.state.borrow().snap.as_ref().map(|s| s.settings.record.clone()).unwrap_or_default();
        if !stream::helper::needed(&prefs) { return false }
        crate::ui::dialogs::go_live_helper::open(self);
        true
    }

    /// Go Live started with `plan` (before the recording takes it): live captions when Settings has them on, and
    /// the broadcast's title and description for the status list's Copy buttons.
    pub(crate) fn started_live(&self, plan: &Plan, prefs: &RecordPrefs) {
        self.stop_captions(false);
        let now = chrono::Local::now().naive_local();
        let title = health::fill(&if prefs.stream_title.trim().is_empty() { stream::accounts::default_title() } else { prefs.stream_title.clone() }, now, "");
        let g = self.ui.global::<LiveStatus>();
        g.set_title(title.into());
        g.set_description(health::fill(&prefs.stream_description, now, "").into());
        g.set_captions("".into());
        let mut live = Live { ndi: prefs.ndi().any() && stream::ndi::runtime().is_some(), ..Live::default() };
        let c = &prefs.extras.captions;
        if c.on {
            let stream_input = match &plan.sound { Sound::Input(n) => Some(n.as_str()), _ => None };
            match c.listen_to(stream_input) {
                None => self.toast(captions::tr::no_input(), true),
                Some(input) => {
                    let file = c.file().then(|| captions::file_beside(&plan.out));
                    let setup = captions::Setup {
                        ffmpeg: plan.ffmpeg.clone(),
                        args: crate::follow::audio::device_args(crate::platform::tools::OS, &input),
                        picture: c.on_picture().then(|| plan.preset.size()),
                        slot: plan.controls.captions.clone(),
                        file: file.clone(),
                        youtube: c.youtube.clone(),
                        delay: Duration::from_secs(u64::from(stream::delay::checked(prefs.stream_delay_secs))),
                        mute: plan.controls.mute.clone(),
                        retry: Duration::from_secs(2),
                    };
                    match captions::start(setup, Box::new(captions::model)) {
                        Ok(cap) => { live.captions = Some(cap); live.captions_file = file; self.toast(captions::tr::on(), false) }
                        Err(e) => self.toast(captions::tr::failed(&e), true),
                    }
                }
            }
        }
        LIVE.with(|l| *l.borrow_mut() = live);
    }

    /// The stream stopped: captions stop, their file is said (when anything was written), the light goes.
    pub(crate) fn stop_captions(&self, say: bool) {
        let live = LIVE.with(|l| std::mem::take(&mut *l.borrow_mut()));
        let Live { captions: Some(mut c), captions_file, .. } = live else { return };
        c.stop();
        if say && let Some(f) = captions_file && std::fs::read_to_string(&f).is_ok_and(|t| t.contains(" --> ")) {
            self.toast(captions::tr::saved(&f.display().to_string()), false);
        }
    }

    /// The health light and status list, once a second while live.
    pub(crate) fn show_health(&self) {
        let Some(mut inputs) = self.health_inputs() else { return };
        let (ndi, captions, text) = LIVE.with(|l| {
            let l = l.borrow();
            let state = l.captions.as_ref().map(|c| match c.state() { captions::State::Failed(why) => Some(why), _ => None });
            let text = l.captions.as_ref().map(|c| c.text.lock().unwrap_or_else(|e| e.into_inner()).clone()).unwrap_or_default();
            (l.ndi, state, text)
        });
        inputs.ndi = ndi;
        inputs.captions = captions;
        let (light, lines) = health::status(&inputs);
        let g = self.ui.global::<LiveStatus>();
        g.set_light(light.index());
        g.set_summary(health::summary(light).into());
        let rows: Vec<LiveStatusLine> = lines.into_iter().map(|l| LiveStatusLine { light: l.light.index(), text: l.text.into() }).collect();
        g.set_lines(ModelRc::new(VecModel::from(rows)));
        g.set_captions(text.into());
    }
}

pub(super) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<LiveStatus>();
    let me = Rc::downgrade(op);
    g.on_copy_title(move || if let Some(op) = me.upgrade() {
        let t = op.ui.global::<LiveStatus>().get_title();
        crate::ui::dialogs::settings::copy(&op, &t, &tr::copied());
    });
    let me = Rc::downgrade(op);
    g.on_copy_description(move || if let Some(op) = me.upgrade() {
        let t = op.ui.global::<LiveStatus>().get_description();
        crate::ui::dialogs::settings::copy(&op, &t, &tr::copied());
    });
}

pub mod tr {
    use crate::tr;
    pub fn copied() -> String { tr!("Copied. Paste it into the service's own page for this stream.") }
}
