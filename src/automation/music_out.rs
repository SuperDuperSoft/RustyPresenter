//! N13: the walk-in music's player, on a thread of its own so it never waits for a window. It is told what to play
//! (`Want`), and its reports go back to `App::music_report`. Without libmpv every song reports the missing player.
use crate::app::App;
use crate::media_prefs::SoundOut;
use crate::video::{Options, Player, Role, ffi::Mpv, players};
use std::path::PathBuf;
use std::sync::{Arc, mpsc::channel};

/// What the player should do; `None` on the channel stops it.
#[derive(Clone, Debug, PartialEq)]
pub struct Want { pub inst: u64, pub path: PathBuf, pub paused: bool, pub volume: u8, pub sound: SoundOut }

/// Starts the player thread and hands its channel to the app.
pub fn start(app: &Arc<App>, mpv: Option<Arc<Mpv>>) {
    let (tx, rx) = channel::<Option<Want>>();
    let (reports_tx, reports) = channel();
    let to_app = app.clone();
    let _ = std::thread::Builder::new().name("music-reports".into()).spawn(move || for r in reports { to_app.music_report(r) });
    let spawned = std::thread::Builder::new().name("music".into()).spawn(move || {
        let mut factory = players(mpv, reports_tx);
        factory.options = Options { vo: "null", ..Options::default() };
        let mut player: Option<(Player, Want)> = None;
        for want in rx {
            let Some(w) = want else { player = None; continue };
            match &player {
                Some((p, had)) if had.inst == w.inst => {
                    if had.paused != w.paused { p.set_paused(w.paused) }
                    if had.volume != w.volume { set_volume(p, w.volume) }
                    if had.sound != w.sound { p.set_sound(&w.sound) }
                    let p = player.take().map(|(p, _)| p);
                    player = p.map(|p| (p, w));
                }
                _ => {
                    // A new song: the old player goes first, so two never play at once.
                    player = None;
                    let extra = crate::media_prefs::mpv_options(Role::Audio { looped: false }, Default::default(), &w.sound);
                    let Some(p) = factory.create(Role::Audio { looped: false }, w.inst, &w.sound.device, extra) else { continue };
                    set_volume(&p, w.volume);
                    p.set_paused(w.paused);
                    p.load(&w.path, 0.0);
                    player = Some((p, w));
                }
            }
        }
    });
    if spawned.is_ok() { app.automation.lock().unwrap().music_out = Some(tx) }
}

fn set_volume(p: &Player, volume: u8) {
    let _ = unsafe { p.mpv().set_property(p.handle(), "volume", &volume.min(100).to_string()) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::music::{MusicList, MusicOp, MusicState, Track};
    use std::time::{Duration, Instant};

    /// A quarter of a second of silence.
    fn silent_wav(path: &std::path::Path) {
        let (rate, n) = (8000u32, 2000u32);
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n * 2).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(n * 2).to_le_bytes());
        b.resize(b.len() + (n * 2) as usize, 0);
        std::fs::write(path, b).unwrap();
    }

    /// With libmpv (`RP_VIDEO_TESTS=1`): the list plays through, song after song, and stops after the last.
    #[test]
    fn plays_the_list_through_libmpv() {
        if std::env::var("RP_VIDEO_TESTS").as_deref() != Ok("1") { return }
        let app = crate::app::test_app();
        let _ = std::fs::create_dir_all(app.store.asset_path("x").parent().unwrap());
        for a in ["a.wav", "b.wav"] { silent_wav(&app.store.asset_path(a)) }
        let tracks = ["a.wav", "b.wav"].iter().map(|a| Track { id: uuid::Uuid::new_v4(), asset: a.to_string(), title: a.to_string(), duration: None }).collect();
        app.music_save(MusicList { tracks, repeat: false, ..MusicList::default() }).unwrap();
        start(&app, Some(Mpv::load().expect("libmpv (set RP_LIBMPV)")));
        app.music(MusicOp::Play).unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        let mut saw_second = false;
        while Instant::now() < end {
            let (state, at) = { let rt = app.automation.lock().unwrap(); (rt.music.state, rt.music.at) };
            saw_second |= at == 1;
            if state == MusicState::Stopped { break }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(saw_second, "the second song played");
        assert_eq!(app.automation.lock().unwrap().music.state, MusicState::Stopped, "and it stopped after the last");
    }
}
