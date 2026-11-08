//! N13: the walk-in music dialog (`ui/dialogs/music.slint`). Songs come from the media library's audio; every change
//! is kept at once through `automation/music/*`, so the core checks it and what plays follows.
use super::{AddTab, DialogKind};
use crate::{
    automation::music::{MusicList, MusicState, Track, tracks_from_library},
    model::MediaKind,
    ui::{MusicDlg, MusicRow, operator::window::Operator},
};
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// Opens the dialog.
pub fn open(op: &Rc<Operator>) {
    op.open_dialog(DialogKind::Music);
    let g = op.ui.global::<MusicDlg>();
    g.set_error("".into());
    g.set_pick(0);
    connect(op);
    load(op);
}

fn list(op: &Operator) -> MusicList { op.app().data.lock().unwrap().automation.music.clone() }

/// The library's audio files: id and name.
fn library(op: &Operator) -> Vec<(Uuid, String)> {
    op.app().data.lock().unwrap().media.iter().filter(|m| m.kind == MediaKind::Audio).map(|m| (m.id, m.name.clone())).collect()
}

fn length(t: &Track) -> String { t.duration.map(|d| crate::timers::text((d * 1000.0) as i64)).unwrap_or_default() }

/// Fills the dialog from the saved list and what plays now.
fn load(op: &Operator) {
    if op.dialog() != Some(DialogKind::Music) { return }
    let g = op.ui.global::<MusicDlg>();
    let l = list(op);
    let lib: Vec<SharedString> = library(op).into_iter().map(|(_, n)| n.into()).collect();
    g.set_library(ModelRc::new(VecModel::from(lib)));
    g.set_shuffle(l.shuffle);
    g.set_repeat(l.repeat);
    readouts_with(op, &l);
}

/// The playing song and the play buttons, after each `Update::Timers`.
pub fn readouts(op: &Operator) {
    if op.dialog() != Some(DialogKind::Music) { return }
    readouts_with(op, &list(op));
}

fn readouts_with(op: &Operator, l: &MusicList) {
    let g = op.ui.global::<MusicDlg>();
    let (current, readout, dragging) = {
        let rt = op.app().automation.lock().unwrap();
        (rt.music.current(l).map(|t| t.id), rt.music.readout(l), rt.music.volume.is_some())
    };
    let rows: Vec<MusicRow> = l.tracks.iter().map(|t| MusicRow { title: t.title.as_str().into(), length: length(t).into(), playing: Some(t.id) == current }).collect();
    g.set_rows(ModelRc::new(VecModel::from(rows)));
    let state = readout.as_ref().map_or(MusicState::Stopped, |r| r.state);
    g.set_playing(state == MusicState::Playing);
    g.set_stopped(state == MusicState::Stopped);
    let now = readout.filter(|r| r.state != MusicState::Stopped)
        .map(|r| crate::tr!("Now: {title} · {time} left", title = r.title, time = crate::timers::text(r.left_ms))).unwrap_or_default();
    // The slider is the operator's while it is being dragged; it follows the saved volume otherwise.
    if !dragging { g.set_volume(i32::from(l.volume)) }
    g.set_now(now.into());
}

/// Saves `l` and shows the result.
fn save(op: &Rc<Operator>, l: MusicList) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("automation/music/save", serde_json::to_value(l).unwrap_or_default(), move |r| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<MusicDlg>();
        match r { Ok(_) => g.set_error("".into()), Err(e) => g.set_error(e.into()) }
        load(&op);
    });
}

fn control(op: &Rc<Operator>, action: &'static str, body: Value) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>(action, body, move |r| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<MusicDlg>();
        match r { Ok(_) => g.set_error("".into()), Err(e) => g.set_error(e.into()) }
        readouts(&op);
    });
}

fn connect(op: &Rc<Operator>) {
    let g = op.ui.global::<MusicDlg>();
    let with = |f: fn(&Rc<Operator>)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move || if let Some(op) = me.upgrade() { f(&op) }
    };
    let with_i = |f: fn(&Rc<Operator>, usize)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move |i: i32| if let Some(op) = me.upgrade() { f(&op, i.max(0) as usize) }
    };
    g.on_add(with(|op| {
        let pick = op.ui.global::<MusicDlg>().get_pick().max(0) as usize;
        let Some((id, _)) = library(op).get(pick).cloned() else { return };
        let mut l = list(op);
        let media = op.app().data.lock().unwrap().media.clone();
        l.tracks.extend(tracks_from_library(&media, &[id]));
        save(op, l);
    }));
    g.on_add_all(with(|op| {
        let mut l = list(op);
        let media = op.app().data.lock().unwrap().media.clone();
        let have: Vec<String> = l.tracks.iter().map(|t| t.asset.clone()).collect();
        let ids: Vec<Uuid> = media.iter().filter(|m| m.kind == MediaKind::Audio && !have.contains(&m.asset)).map(|m| m.id).collect();
        l.tracks.extend(tracks_from_library(&media, &ids));
        save(op, l);
    }));
    g.on_import(with(|op| crate::ui::dialogs::add::open_add(op, AddTab::Media)));
    g.on_play_row(with_i(|op, i| {
        let Some(t) = list(op).tracks.get(i).cloned() else { return };
        control(op, "automation/music/play-track", json!({ "id": t.id }));
    }));
    let me = Rc::downgrade(op);
    g.on_move(move |i, by| {
        let Some(op) = me.upgrade() else { return };
        let mut l = list(&op);
        let (i, j) = (i.max(0) as usize, i + by);
        if j < 0 || j as usize >= l.tracks.len() || i >= l.tracks.len() { return }
        l.tracks.swap(i, j as usize);
        save(&op, l);
    });
    g.on_remove(with_i(|op, i| {
        let mut l = list(op);
        if i < l.tracks.len() { l.tracks.remove(i); save(op, l) }
    }));
    g.on_shuffle_changed(with(|op| {
        let on = op.ui.global::<MusicDlg>().get_shuffle();
        control(op, "automation/music/shuffle", json!({ "on": on }));
    }));
    g.on_repeat_changed(with(|op| {
        let mut l = list(op);
        l.repeat = op.ui.global::<MusicDlg>().get_repeat();
        save(op, l);
    }));
    let me = Rc::downgrade(op);
    g.on_volume_changed(move |v| if let Some(op) = me.upgrade() { control(&op, "automation/music/volume", json!({ "volume": v.clamp(0, 100) })) });
    g.on_toggle(with(|op| control(op, "automation/music/toggle", json!({}))));
    g.on_next(with(|op| control(op, "automation/music/next", json!({}))));
    g.on_stop(with(|op| control(op, "automation/music/stop", json!({}))));
    g.on_done(with(|op| op.close_dialog()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::test_app,
        model::{MediaFile, MediaStatus},
        ui::{AutoStrip, operator::window::testing::{open as open_window, refresh}},
    };
    use slint::Model;

    fn audio(name: &str) -> MediaFile {
        MediaFile { id: Uuid::new_v4(), name: name.into(), kind: MediaKind::Audio, asset: format!("{name}.mp3"), status: MediaStatus::Ready,
            original_asset: None, duration: Some(125.0), thumb: None, size: None, folder: String::new() }
    }

    #[test]
    fn songs_are_added_from_the_library_and_play_from_the_mini_player() {
        let app = test_app();
        app.data.lock().unwrap().media.extend([audio("Hymn"), audio("Psalm")]);
        let op = open_window(&app);
        refresh(&op);
        assert!(!op.ui.global::<AutoStrip>().get_music_shown(), "no walk-in music: no mini player");
        open(&op);
        let g = op.ui.global::<MusicDlg>();
        assert_eq!(g.get_library().row_count(), 2);
        g.set_pick(1);
        g.invoke_add();
        g.invoke_add_all();
        let titles: Vec<String> = g.get_rows().iter().map(|r| r.title.to_string()).collect();
        assert_eq!(titles, ["Psalm", "Hymn"], "Add all leaves out the songs already there");
        assert_eq!(g.get_rows().row_data(0).unwrap().length, "2:05");
        g.invoke_move(1, -1);
        assert_eq!(app.data.lock().unwrap().automation.music.tracks[0].title, "Hymn");
        g.invoke_play_row(1);
        assert!(g.get_playing());
        assert_eq!(app.automation.lock().unwrap().music.current(&app.data.lock().unwrap().automation.music).unwrap().title, "Psalm");
        op.apply_timers();
        let s = op.ui.global::<AutoStrip>();
        assert!(s.get_music_shown() && s.get_music_playing());
        assert_eq!(s.get_music_title(), "Psalm");
        s.invoke_music_toggle();
        op.apply_timers();
        assert!(!s.get_music_playing(), "the mini player pauses it");
        s.invoke_music_stop();
        op.apply_timers();
        assert!(s.get_music_stopped());
        assert_eq!(s.get_music_title(), "2 songs");
        g.invoke_remove(0);
        assert_eq!(app.data.lock().unwrap().automation.music.tracks.len(), 1);
    }

    #[test]
    fn shuffle_repeat_and_volume_are_kept() {
        let app = test_app();
        app.data.lock().unwrap().media.push(audio("Hymn"));
        let op = open_window(&app);
        open(&op);
        let g = op.ui.global::<MusicDlg>();
        g.invoke_add();
        g.set_shuffle(true);
        g.invoke_shuffle_changed();
        g.set_repeat(false);
        g.invoke_repeat_changed();
        g.invoke_volume_changed(35);
        let saved = app.data.lock().unwrap().automation.music.clone();
        assert!(saved.shuffle && !saved.repeat);
        assert_eq!(app.automation.lock().unwrap().music.volume(&saved), 35, "the new volume plays at once");
        app.music_save_volume(chrono::Utc::now() + chrono::TimeDelta::seconds(5));
        assert_eq!(app.data.lock().unwrap().automation.music.volume, 35, "and is saved once the slider rests");
    }
}
