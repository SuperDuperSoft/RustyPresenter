//! What was Livestream → More settings (design `docs/superpowers/specs/2026-11-17-livestream-output-design.md`):
//! saved setups, everything the easy rows leave to the app (`record::advanced`), and each destination's own quality.
//! Since the November 17, 2026 reorganisation these are the advanced rows of Settings → Livestream's pages
//! (`ui/dialogs/settings-stream.slint`, the Settings window's `StreamMoreDlg`), saved with the other settings.
use super::{settings::{self, Edit}, settings_window::win, strings as dlg};
use crate::{
    record::{self, Preset, RecordPrefs, advanced::{self, Codec, Format, Keep, More, Profile, Rate, Speed}, setups},
    stream, tr,
    ui::{MoreDestination, StreamMoreDlg, operator::window::Operator},
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc};

/// The dialog's choices, each as its place in its list (the first is what the app does by itself).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub fps: usize,
    pub codec: usize,
    pub rate: usize,
    pub keyframe: usize,
    pub speed: usize,
    pub profile: usize,
    pub b_frames: usize,
    pub sound_rate: usize,
    pub sound_kbps: usize,
    pub channels: usize,
    /// The network card's address (empty: the computer chooses).
    pub bind: String,
    pub srt: usize,
    /// Each of Settings' destinations (`stream::BUILT_IN`): size (0 the main stream's, 1 720p, 2 1080p) and bitrate
    /// (0 the main stream's, then `record::BITRATES`).
    pub destinations: Vec<(usize, usize)>,
    pub format: usize,
    pub name: String,
    pub split: usize,
    pub clean: bool,
    pub replay: usize,
}

fn place<T: PartialEq>(all: &[T], v: &T) -> Option<usize> { all.iter().position(|x| x == v) }

impl Form {
    pub fn from(p: &RecordPrefs) -> Form {
        let m = &p.more;
        let after = |found: Option<usize>| found.map_or(0, |i| i + 1);
        let quality = |i: usize| p.stream.iter().find(|d| d.id == stream::built_in_id(i)).map(|d| d.quality).unwrap_or_default();
        Form {
            fps: place(&advanced::FRAME_RATES, &m.fps()).unwrap_or(2),
            codec: place(&Codec::ALL, &m.codec).unwrap_or(0),
            rate: place(&Rate::ALL, &m.video.rate).unwrap_or(0),
            keyframe: place(&advanced::KEYFRAMES, &m.video.keyframe_secs).unwrap_or(1),
            speed: place(&Speed::ALL, &m.video.speed).unwrap_or(0),
            profile: place(&Profile::ALL, &m.video.profile).unwrap_or(0),
            b_frames: after(m.video.b_frames.and_then(|b| place(&advanced::B_FRAMES, &b))),
            sound_rate: after(place(&advanced::SOUND_RATES, &m.sound.rate)),
            sound_kbps: place(&advanced::SOUND_KBPS, &m.sound.kbps).unwrap_or(2),
            channels: usize::from(m.sound.channels == 1),
            bind: m.network.bind.clone(),
            srt: after(place(&advanced::SRT_LATENCIES, &m.network.srt_latency_ms)),
            destinations: (0..stream::ROWS).map(|i| {
                let q = quality(i);
                (q.size.map_or(0, |s| place(&Preset::ALL, &s).map_or(0, |i| i + 1)), after(place(&record::BITRATES, &q.kbps)))
            }).collect(),
            format: usize::from(m.keep.format == Format::Mkv),
            name: m.keep.name.clone(),
            split: if let Some(i) = place(&advanced::SPLIT_MINUTES, &m.keep.split_minutes) { i + 1 }
                else { place(&advanced::SPLIT_GB, &m.keep.split_gb).map_or(0, |i| i + 1 + advanced::SPLIT_MINUTES.len()) },
            clean: m.keep.clean,
            replay: after(place(&advanced::REPLAY_MINUTES, &m.keep.replay_minutes)),
        }
    }

    /// Puts the choices into `p` (its other settings are left as they are).
    pub fn apply(&self, p: &mut RecordPrefs) {
        let pick = |all: &[u32], i: usize| all.get(i).copied().unwrap_or(0);
        let after = |all: &[u32], i: usize| i.checked_sub(1).and_then(|i| all.get(i)).copied().unwrap_or(0);
        let fps = pick(&advanced::FRAME_RATES, self.fps);
        let keyframe = pick(&advanced::KEYFRAMES, self.keyframe);
        let kbps = pick(&advanced::SOUND_KBPS, self.sound_kbps);
        let (split_minutes, split_gb) = match self.split {
            0 => (0, 0),
            i if i <= advanced::SPLIT_MINUTES.len() => (advanced::SPLIT_MINUTES[i - 1], 0),
            i => (0, advanced::SPLIT_GB.get(i - 1 - advanced::SPLIT_MINUTES.len()).copied().unwrap_or(0)),
        };
        p.more = More {
            // The usual ones are kept as "not set", so an untouched dialog changes nothing.
            fps: if fps == record::FPS { 0 } else { fps },
            codec: Codec::ALL.get(self.codec).copied().unwrap_or_default(),
            video: advanced::Video {
                rate: Rate::ALL.get(self.rate).copied().unwrap_or_default(),
                keyframe_secs: if keyframe == 2 { 0 } else { keyframe },
                speed: Speed::ALL.get(self.speed).copied().unwrap_or_default(),
                profile: Profile::ALL.get(self.profile).copied().unwrap_or_default(),
                b_frames: self.b_frames.checked_sub(1).and_then(|i| advanced::B_FRAMES.get(i)).copied(),
            },
            sound: advanced::Sound { rate: after(&advanced::SOUND_RATES, self.sound_rate), kbps: if kbps == 160 { 0 } else { kbps }, channels: if self.channels == 1 { 1 } else { 0 } },
            network: advanced::Network { bind: self.bind.trim().to_string(), srt_latency_ms: after(&advanced::SRT_LATENCIES, self.srt) },
            keep: Keep {
                format: if self.format == 1 { Format::Mkv } else { Format::Mp4 }, name: self.name.trim().to_string(), split_minutes, split_gb, clean: self.clean,
                replay_minutes: after(&advanced::REPLAY_MINUTES, self.replay),
            },
        };
        // Each destination's own quality; a destination with one and no row yet gets a row (off).
        for (i, (size, kbps)) in self.destinations.iter().enumerate().take(stream::ROWS) {
            let q = stream::Quality { size: size.checked_sub(1).and_then(|s| Preset::ALL.get(s)).copied(), kbps: after(&record::BITRATES, *kbps), cap: None };
            let id = stream::built_in_id(i);
            match p.stream.iter_mut().find(|d| d.id == id) {
                Some(d) => d.quality = q,
                None if !q.is_same() => p.stream.push(stream::Destination { id, service: stream::BUILT_IN[i].0, quality: q, ..Default::default() }),
                None => {}
            }
        }
    }
}

thread_local! {
    /// The network cards' addresses, in the list's order ("Send the stream from").
    static BINDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn g(op: &Operator) -> StreamMoreDlg<'static> { win(op).global::<StreamMoreDlg>() }

/// The lists' words: each destination's size (the main stream's, then each named size) and bitrate (the main
/// stream's, then `record::BITRATES`), and the main bitrate's (Automatic with what it is now, then the same).
pub fn fill_labels(op: &Operator, automatic_kbps: u32) {
    let g = g(op);
    let kbps: Vec<String> = record::BITRATES.iter().map(|b| format!("{b} kbit/s")).collect();
    let mut sizes = vec![tr!("Size: the main stream's")];
    sizes.extend(Preset::ALL.iter().map(|p| p.label()));
    g.set_size_labels(strings(sizes));
    let mut dest = vec![tr!("Bitrate: the main stream's")];
    dest.extend(kbps.iter().cloned());
    g.set_dest_kbps_labels(strings(dest));
    let mut main = vec![tr!("Automatic ({kbps} kbit/s)", kbps = automatic_kbps)];
    main.extend(kbps);
    g.set_kbps_labels(strings(main));
}

/// Fills the rows from the settings being edited, and connects them (Settings is opening).
pub(crate) fn load(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let p = edit.settings.borrow().record.clone();
    show(op, &p);
    wire(op, edit);
}

/// The rows' choices as the page has them now.
fn read(op: &Operator) -> Form {
    let g = g(op);
    let i = |v: i32| v.max(0) as usize;
    Form {
        fps: i(g.get_fps_index()), codec: i(g.get_codec_index()), rate: i(g.get_rate_index()), keyframe: i(g.get_keyframe_index()), speed: i(g.get_speed_index()),
        profile: i(g.get_profile_index()), b_frames: i(g.get_b_frames_index()), sound_rate: i(g.get_sound_rate_index()), sound_kbps: i(g.get_sound_kbps_index()),
        channels: i(g.get_channels_index()), bind: BINDS.with(|b| b.borrow().get(i(g.get_bind_index())).cloned()).unwrap_or_default(), srt: i(g.get_srt_index()),
        destinations: g.get_destinations().iter().map(|d| (i(d.size), i(d.kbps))).collect(),
        format: i(g.get_format_index()), name: g.get_name().to_string(), split: i(g.get_split_index()), clean: g.get_clean(), replay: i(g.get_replay_index()),
    }
}

/// Puts the rows' choices into `p` (Save, and seeing whether anything changed).
pub(crate) fn read_into(op: &Operator, p: &mut RecordPrefs) { read(op).apply(p) }

/// The pictures a second chosen now.
pub(crate) fn fps(op: &Operator) -> u32 { advanced::FRAME_RATES.get(g(op).get_fps_index().max(0) as usize).copied().unwrap_or(record::FPS) }

pub(crate) fn show(op: &Operator, p: &RecordPrefs) {
    let g = g(op);
    let f = Form::from(p);
    let n = |v: usize| v as i32;
    g.set_fps_index(n(f.fps));
    g.set_codec_index(n(f.codec));
    g.set_rate_index(n(f.rate));
    g.set_keyframe_index(n(f.keyframe));
    g.set_speed_index(n(f.speed));
    g.set_profile_index(n(f.profile));
    g.set_b_frames_index(n(f.b_frames));
    g.set_sound_rate_index(n(f.sound_rate));
    g.set_sound_kbps_index(n(f.sound_kbps));
    g.set_channels_index(n(f.channels));
    let (labels, keys, index) = stream::network_card::choices(&stream::network_card::list(), &f.bind);
    BINDS.with(|b| *b.borrow_mut() = keys);
    g.set_bind_labels(strings(labels));
    g.set_bind_index(n(index));
    g.set_srt_index(n(f.srt));
    let rows: Vec<MoreDestination> = f.destinations.iter().enumerate().map(|(i, (size, kbps))| MoreDestination {
        name: stream::BUILT_IN[i].0.name().into(), size: n(*size), kbps: n(*kbps),
    }).collect();
    g.set_destinations(ModelRc::new(VecModel::from(rows)));
    g.set_format_index(n(f.format));
    g.set_name(f.name.as_str().into());
    g.set_split_index(n(f.split));
    g.set_clean(f.clean);
    g.set_replay_index(n(f.replay));
    fill_labels(op, p.size.kbps_at(p.more.fps()));
    show_setups(op, p);
    show_example(op);
}

fn show_setups(op: &Operator, p: &RecordPrefs) {
    let g = g(op);
    let names = setups::names(p);
    let chosen = setups::current(p).and_then(|c| names.iter().position(|n| *n == c));
    g.set_setups(strings(names));
    g.set_setup_index(chosen.map_or(-1, |i| i as i32));
}

/// What the file name comes out as now.
fn show_example(op: &Operator) {
    let g = g(op);
    let title = op.app().data().service().map(|s| s.name.clone()).unwrap_or_default();
    let ext = if g.get_format_index() == 1 { "mkv" } else { "mp4" };
    let stem = advanced::file_stem(&g.get_name(), chrono::Local::now().naive_local(), &title);
    g.set_name_example(format!("{stem}.{ext}").into());
}

fn strings(v: Vec<String>) -> ModelRc<SharedString> { ModelRc::new(VecModel::from(v.into_iter().map(SharedString::from).collect::<Vec<_>>())) }

fn wire(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let g = g(op);
    let on = |f: fn(&Rc<Operator>, &Rc<Edit>)| {
        let (me, edit) = (Rc::downgrade(op), edit.clone());
        move || if let Some(op) = me.upgrade() { f(&op, &edit) }
    };
    let (me, e) = (Rc::downgrade(op), edit.clone());
    g.on_setup_use(move |i| if let Some(op) = me.upgrade() { use_setup(&op, &e, i) });
    g.on_setup_save(on(save_setup));
    g.on_setup_delete(on(delete_setup));
    let me = Rc::downgrade(op);
    g.on_destination_size(move |i, v| if let Some(op) = me.upgrade() { set_row(&op, i, |d| d.size = v) });
    let me = Rc::downgrade(op);
    g.on_destination_kbps(move |i, v| if let Some(op) = me.upgrade() { set_row(&op, i, |d| d.kbps = v) });
    let me = Rc::downgrade(op);
    g.on_name_edited(move || if let Some(op) = me.upgrade() { show_example(&op) });
}

fn set_row(op: &Operator, i: i32, f: impl FnOnce(&mut MoreDestination)) {
    let rows = g(op).get_destinations();
    let Ok(i) = usize::try_from(i) else { return };
    let Some(mut d) = rows.row_data(i) else { return };
    f(&mut d);
    rows.set_row_data(i, d);
}

/// Uses setup `i`: its settings (all of the livestream's, the easy rows' too) replace the ones being edited.
fn use_setup(op: &Rc<Operator>, edit: &Rc<Edit>, i: i32) {
    let mut s = settings::current(op, edit);
    let Some(name) = usize::try_from(i).ok().and_then(|i| setups::names(&s.record).get(i).cloned()) else { return };
    if setups::apply(&mut s.record, &name) {
        super::settings_record::refill(op, &s.record);
        show(op, &s.record);
        op.toast(tr!("The setup “{name}” is chosen. Save to use it.", name = name), false);
    }
}

fn save_setup(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let suggested = setups::current(&settings::current(op, edit).record).unwrap_or_default();
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    super::ask::in_settings(op, || super::prompt(op, &tr!("Name of the setup (for example Sunday, or Wedding)"), &suggested, move |typed| {
        let (Some(op), Some(typed)) = (me.upgrade(), typed) else { return };
        if !settings::is_open(&op) { return }
        let mut p = settings::current(&op, &edit).record;
        if setups::save(&mut p, &typed) {
            edit.settings.borrow_mut().record.setups = p.setups.clone();
            show_setups(&op, &p);
        }
    }));
}

fn delete_setup(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let p = settings::current(op, edit).record;
    let Some(name) = usize::try_from(g(op).get_setup_index()).ok().and_then(|i| setups::names(&p).get(i).cloned()) else { return };
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    super::ask::in_settings(op, || super::confirm(op, &tr!("Delete the setup “{name}”? The settings in use now stay as they are.", name = name), &tr!("Delete"), &dlg::cancel(), true, move |ok| {
        let Some(op) = me.upgrade().filter(|_| ok) else { return };
        if !settings::is_open(&op) { return }
        let mut p = settings::current(&op, &edit).record;
        setups::remove(&mut p, &name);
        edit.settings.borrow_mut().record.setups = p.setups.clone();
        show_setups(&op, &p);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_rows_change_nothing() {
        let p = RecordPrefs::default();
        let f = Form::from(&p);
        assert_eq!((f.fps, f.keyframe, f.sound_kbps, f.destinations.len()), (2, 1, 2, stream::ROWS));
        let mut q = p.clone();
        f.apply(&mut q);
        assert_eq!(q, p);
        assert_eq!(serde_json::to_value(&q).unwrap().get("more"), None, "nothing saved");
    }

    #[test]
    fn every_choice_is_kept_and_read_back() {
        let f = Form {
            fps: 4, codec: 1, rate: 1, keyframe: 0, speed: 2, profile: 3, b_frames: 1, sound_rate: 1, sound_kbps: 4, channels: 1, bind: " 192.168.1.20 ".into(), srt: 3,
            destinations: vec![(0, 0), (2, 2), (0, 0), (0, 0), (0, 0), (0, 0), (2, 0)], format: 1, name: "{date} {title}".into(), split: 5, clean: true, replay: 3,
        };
        let mut p = RecordPrefs::default();
        f.apply(&mut p);
        let m = &p.more;
        assert_eq!((m.fps(), m.codec, m.video.rate, m.video.keyframe_secs, m.video.speed, m.video.profile, m.video.b_frames), (60, Codec::Hevc, Rate::Constant, 1, Speed::Fast, Profile::High, Some(0)));
        assert_eq!((m.sound.rate, m.sound.kbps, m.sound.channels), (44100, 256, 1));
        assert_eq!((m.network.bind.as_str(), m.network.srt_latency_ms), ("192.168.1.20", 2000));
        assert_eq!((m.keep.format, m.keep.split_minutes, m.keep.split_gb, m.keep.clean, m.keep.replay_minutes), (Format::Mkv, 0, 4, true, 5));
        // Facebook (row 1): 720p at 2500; TikTok (row 6): 720p. Rows made for them, off.
        let fb = p.stream.iter().find(|d| d.id == stream::built_in_id(1)).unwrap();
        assert_eq!((fb.quality.size, fb.quality.kbps, fb.on), (Some(Preset::P720), 2500, false));
        assert!(p.stream.iter().any(|d| d.id == stream::built_in_id(6) && d.service == stream::Service::TikTok));
        assert_eq!(p.stream.len(), 2, "no row for the others");
        let mut back = Form::from(&p);
        back.bind = f.bind.trim().to_string();
        assert_eq!(back, Form { bind: "192.168.1.20".into(), ..f });
        // By minutes too.
        let mut q = RecordPrefs::default();
        Form { split: 2, ..Form::from(&q) }.apply(&mut q);
        assert_eq!((q.more.keep.split_minutes, q.more.keep.split_gb), (60, 0));
        assert_eq!(Form::from(&q).split, 2);
    }
}
