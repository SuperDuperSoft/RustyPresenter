//! N22: the room window as a recording's source. OpenGL drawing reads each finished frame (`record::gl`); software
//! drawing takes a snapshot 15 times a second. With the app's sound recorded, the players on air are looked at
//! four times a second (`record::sound`).
use super::{AUDIO, Inner, MEDIA};
use crate::record::{FPS, FrameSink, gl::GlCapture, pump::Raw, sound::Heard};
use crate::video::DrawMode;
use slint::{ComponentHandle, Timer, TimerMode};
use std::{sync::Arc, time::Duration};

/// Snapshots a second in software drawing, where each one draws the whole window again on the CPU.
const SNAPSHOTS: u64 = 15;
/// How often the players are looked at for the app's sound.
const LISTEN_EVERY: Duration = Duration::from_millis(250);

/// A recording this room feeds; dropping it stops the timers.
pub(super) struct RoomRec {
    pub(super) sink: Arc<FrameSink>,
    _snap: Timer,
    _sound: Timer,
}

impl Inner {
    /// Starts feeding `sink`, or stops (`None`).
    pub(super) fn record(&self, sink: Option<Arc<FrameSink>>) {
        let old = self.rec.borrow_mut().take();
        if old.is_some() && self.surface.mode() == DrawMode::OpenGl { self.surface.set_frame_tap(None) }
        let Some(sink) = sink else { return };
        let (snap, sound) = (Timer::default(), Timer::default());
        match self.surface.mode() {
            DrawMode::OpenGl => {
                let w = self.ui.as_weak();
                let redraw_later = move |after: Duration| {
                    let w = w.clone();
                    Timer::single_shot(after, move || if let Some(ui) = w.upgrade() { ui.window().request_redraw() });
                };
                self.surface.set_frame_tap(Some(GlCapture::new(sink.clone(), FPS, redraw_later)));
            }
            DrawMode::Software => {
                let (w, s) = (self.ui.as_weak(), sink.clone());
                let mut told = false;
                snap.start(TimerMode::Repeated, Duration::from_millis(1000 / SNAPSHOTS), move || {
                    let Some(ui) = w.upgrade().filter(|_| !s.is_closed()) else { return };
                    match ui.window().take_snapshot() {
                        Ok(px) => s.put_raw(&Raw { w: px.width(), h: px.height(), bottom_up: false, rgba: px.as_bytes() }),
                        Err(e) if !told => { told = true; eprintln!("recording: the room cannot be read ({e})") }
                        Err(_) => {}
                    }
                });
            }
        }
        if sink.sound.is_some() {
            let (me, s) = (self.me.clone(), sink.clone());
            sound.start(TimerMode::Repeated, LISTEN_EVERY, move || {
                let (Some(inner), Some(t), Some(log)) = (me.upgrade(), s.elapsed(), s.sound.as_ref()) else { return };
                let heard = inner.heard();
                log.lock().unwrap_or_else(|e| e.into_inner()).hear(t, &heard);
            });
        }
        *self.rec.borrow_mut() = Some(RoomRec { sink, _snap: snap, _sound: sound });
        self.ui.window().request_redraw();
    }

    /// The players the room can be heard from: the foreground video and the audio item (a background video is
    /// always muted).
    fn heard(&self) -> Vec<Heard> {
        let players = self.players.borrow();
        [MEDIA, AUDIO].iter().filter_map(|&slot| {
            let p = players[slot].as_ref()?;
            let (path, pos, volume) = p.heard()?;
            Some(Heard { key: p.serial(), path, pos, volume })
        }).collect()
    }

    /// The recording being fed, if any.
    pub(super) fn recording(&self) -> Option<Arc<FrameSink>> { self.rec.borrow().as_ref().map(|r| r.sink.clone()) }
}
