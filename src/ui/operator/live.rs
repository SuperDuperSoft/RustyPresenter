//! The live column (`renderLive()` and `renderTransport()`, `operator.js:212-261`, buttons at `:345-347`): what is
//! live, the monitor, the play/pause rows, the notice, Back/Next, the clears and Up next. The monitor's layers are
//! drawn by `Preview`; while a video is live its frame comes from the room's player 4 times a second (once a second
//! in light mode, `ui::light::grab_every`).
use super::{view::{self, RoomStatus, TransportKind, TransportRow}, window::Operator};
use crate::{
    app::LiveView,
    live::{Command, drawing::Drawing},
    render::drawing::DrawingCanvas,
    room::Kind,
    design::{Transition, TransitionKind},
    live::SlideLayer,
    ui::{Geo, LiveSong, OperatorWindow, Side, TransportCell, fade::{Chan, Fader}, ops::outcome_toast, preview::{self, Drawn}, thumbs},
};
use slint::{ComponentHandle, Image, Model, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::Cell, rc::{Rc, Weak}, sync::Arc, time::Duration};

/// The column between updates.
#[derive(Default)]
pub(super) struct LiveState {
    /// The transport rows shown (a click names one by index).
    rows: Vec<TransportRow>,
    /// The model behind them. It is kept and changed row by row, never replaced: a new model drops every button,
    /// and the player's time updates (2-4 a second) would eat a click whose press and release straddle one.
    cells: Rc<VecModel<TransportCell>>,
    /// The instance of the video the preview follows, with a count bumped whenever it changes, so a frame grabbed
    /// for an older video is dropped.
    video: Option<u64>,
    video_gen: u64,
    grab_timer: Timer,
    /// The phone drawing over the monitor: its layer (finished strokes cached), the newest drawing and its `rev`.
    drawing: DrawingCanvas,
    drawing_now: Option<Arc<Drawing>>,
    /// Draws the monitor's drawing again while ink fades.
    fade_timer: Timer,
    /// The live slide layer the monitor last drew, so only a new slide plays its transition (not a resize).
    slide_now: Option<SlideLayer>,
    /// The monitor's slide transition, made on first use.
    motion: Option<Motion>,
    /// A designed slide's click group playing on the monitor (the room's player, drawn small), and its pictures.
    anim: Rc<crate::ui::room::anim::Animator>,
    sprites: Option<Rc<VecModel<crate::ui::Sprite>>>,
}

/// A new designed slide's click group for the monitor to play, and whether it adds to the slide before it.
type NewPlay = Option<(crate::design::Play, bool)>;

/// A deck or designed slide's own transition on the monitor, moved by its own fader like the room's.
struct Motion { fader: Fader, ch: Chan, playing: Rc<Cell<Option<Transition>>> }

/// The transition a newly live slide comes in with, as the room plays it (none for a build step).
fn own_transition(slide: Option<&SlideLayer>) -> Option<Transition> {
    match slide? {
        SlideLayer::Design { design, build: false, .. } => Some(design.transition),
        SlideLayer::Image { transition, .. } => *transition,
        _ => None,
    }
}

/// Writes the monitor's two slide faces `p` (0–1) of the way through `t`; none: the new slide alone, in place.
fn set_motion(ui: &OperatorWindow, t: Option<Transition>, p: f32) {
    let still = Geo::default;
    let (old_op, new_op, old_geo, new_geo, black) = match t.map(|t| (t.kind, t.dir)) {
        Some((TransitionKind::Fade, _)) => (1.0 - p, p, still(), still(), 0.0),
        // Through black: the old slide goes dark, then the new one comes up out of it.
        Some((TransitionKind::Black, _)) if p < 0.5 => (1.0, 0.0, still(), still(), p * 2.0),
        Some((TransitionKind::Black, _)) => (0.0, 1.0, still(), still(), 2.0 - p * 2.0),
        Some((kind, dir)) if kind.moves() => (1.0, 1.0, crate::ui::room::geo(kind, dir, p, false), crate::ui::room::geo(kind, dir, p, true), 0.0),
        Some((TransitionKind::Cut, _)) | None => (0.0, 1.0, still(), still(), 0.0),
        Some(_) => (1.0 - p, p, still(), still(), 0.0),
    };
    ui.set_monitor_slide_old_op(old_op);
    ui.set_monitor_slide_op(new_op);
    ui.set_monitor_slide_old_geo(old_geo);
    ui.set_monitor_slide_geo(new_geo);
    ui.set_monitor_black(black);
    if t.is_none() { ui.set_monitor_slide_old(Side::default()) }
}

impl Operator {
    /// `renderLive()`: everything in the column that follows the live view.
    /// The transport rows (a playing video's or audio's place and time).
    fn render_transport(&self, v: &crate::app::LiveView) {
        let rows = view::transport(v);
        let cells: Vec<TransportCell> = rows.iter().map(|r| TransportCell {
            video: r.kind == TransportKind::Video, label: r.label.as_str().into(), paused: r.paused, percent: r.percent, time: r.time.as_str().into(),
        }).collect();
        let mut st = self.live.borrow_mut();
        put_cells(&st.cells, cells);
        st.rows = rows;
    }

    /// A player said how far it has played (`Update::Playback`): the live view keeps everything but the play
    /// status, and only the transport rows are drawn again.
    pub fn apply_playback(&self, p: &crate::app::LiveUpdate) {
        let patched = {
            let mut st = self.state.borrow_mut();
            let Some(now) = st.live.as_ref().filter(|now| now.view().version == p.view().version) else { return };
            let v = crate::app::LiveView { media: p.view().media.clone(), audio: p.view().audio.clone(), ..now.view().clone() };
            let patched = crate::app::LiveUpdate::new(v);
            st.live = Some(patched.clone());
            patched
        };
        self.render_transport(patched.view());
    }

    pub(super) fn render_live(&self) {
        let Some(live) = self.state.borrow().live.clone() else { return };
        let v = live.view();
        let ui = &self.ui;
        ui.set_live_pending(v.pending);
        ui.set_live_what(view::live_what(v).into());
        ui.set_live_notice(v.notice.clone().unwrap_or_default().into());
        ui.set_live_notes(v.slide.as_ref().map_or("", |s| s.notes.as_str()).into());
        ui.set_live_song(live_song(v));
        if let Some(crate::ui::dialogs::DialogKind::SongEditor { song_id: Some(id), .. }) = self.dialog() {
            ui.global::<crate::ui::SongEditorState>().set_live_song(v.item.as_ref().and_then(|i| i.song_id) == Some(id));
        }
        ui.set_logo_on(v.frame.logo);
        let (title, body) = view::next_up(v);
        ui.set_up_next_title(title.into());
        ui.set_up_next_body(body.into());
        self.render_transport(v);
        self.render_monitor(v);
        self.render_drawing(&v.drawing, false);
        self.render_overlay_strip();
        super::follow::render(self);
    }

    /// The phone drawing on the monitor, and the Drawing chip. An older drawing than the one shown is ignored (a
    /// live view read before the newest strokes can arrive after them); `again` draws it even when unchanged.
    pub(crate) fn render_drawing(&self, d: &Arc<Drawing>, again: bool) {
        let mut st = self.live.borrow_mut();
        let newer = st.drawing_now.as_ref().is_none_or(|old| d.rev > old.rev);
        if !newer && !again { return }
        let d = if newer { st.drawing_now = Some(d.clone()); d.clone() } else { st.drawing_now.clone().unwrap_or_else(|| d.clone()) };
        let px = st.drawing.render(&d, self.preview.size());
        match d.next_change(std::time::Instant::now()) {
            Some(wait) => {
                let me = self.me.clone();
                st.fade_timer.start(TimerMode::SingleShot, wait, move || if let Some(op) = me.upgrade() {
                    let now = op.live.borrow().drawing_now.clone();
                    if let Some(d) = now { op.render_drawing(&d, true) }
                });
            }
            None => st.fade_timer.stop(),
        }
        drop(st);
        self.ui.set_monitor_drawing(px.map(|p| Image::from_rgba8_premultiplied(p.buffer())).unwrap_or_default());
        self.ui.set_drawing_on(!d.is_empty());
    }

    /// View → Clear drawing and the chip's ✕.
    fn clear_drawing(&self) { self.ops().call::<serde_json::Value>("drawing/clear", serde_json::json!({}), |_| {}) }

    /// The live view as this window last drew it.
    pub(crate) fn shown_live(&self) -> Option<crate::app::LiveUpdate> { self.state.borrow().live.clone() }

    /// The monitor: its layers (only those that changed are drawn again), and the video under them.
    fn render_monitor(&self, v: &LiveView) {
        self.preview.set_size(thumbs::thumb_size(self.ui.get_monitor_width(), self.ui.window().scale_factor()));
        let me = self.me.clone();
        let (transition, play) = {
            let mut st = self.live.borrow_mut();
            let new = st.slide_now.as_ref() != v.frame.slide.as_ref();
            if new { st.slide_now = v.frame.slide.clone() }
            let play: NewPlay = match &v.frame.slide { Some(SlideLayer::Design { play: Some(p), build, .. }) if new => Some(((**p).clone(), *build)), _ => None };
            (if new { own_transition(v.frame.slide.as_ref()) } else { None }, play)
        };
        self.preview.show(&v.frame, &v.theme, v.logo.as_deref(), move |layers| if let Some(op) = me.upgrade() { op.monitor_drawn(layers, transition, play) });
        self.ui.set_monitor_bg_visible(preview::background_visible(&v.frame));
        self.follow_video(preview::drawn_video(&v.frame));
    }

    /// The monitor cuts straight to each new layer (no crossfade): a fade here would redraw the operator window
    /// for 400 ms on every slide change, while the room does its own crossfade. A slide with its own transition
    /// (`transition`, a deck or designed slide) plays it, as the room does, and a new designed slide's click
    /// group (`play`) plays over it with the room's pictures, drawn small.
    fn monitor_drawn(&self, layers: Vec<(Kind, Drawn)>, transition: Option<Transition>, play: NewPlay) {
        for (kind, drawn) in layers {
            if let Drawn::Anim(px, made) = drawn {
                let side = Side { image: px.image(), color: Default::default() };
                match &play {
                    Some((p, build)) if kind == Kind::Slide => self.animate_slide(side, made, p.clone(), *build, transition),
                    // Drawn again at a new size: the slide as it ends.
                    _ => self.move_slide(side, None),
                }
                continue;
            }
            let set: fn(&OperatorWindow, Side) = match kind {
                Kind::Background => OperatorWindow::set_monitor_bg,
                Kind::Media => OperatorWindow::set_monitor_media,
                Kind::Slide => OperatorWindow::set_monitor_slide,
                Kind::Logo => OperatorWindow::set_monitor_logo,
                Kind::Overlays | Kind::Audio => continue,
            };
            let side = match drawn {
                Drawn::Pic(px, color) => Side { image: px.map(|p| p.image()).unwrap_or_default(), color },
                Drawn::Hidden | Drawn::Anim(..) => Side::default(),
            };
            if kind == Kind::Slide { self.move_slide(side, transition) } else { set(&self.ui, side) }
        }
    }

    /// Puts the new slide on the monitor, playing `t` from the slide shown now (a cut when none, or when there was
    /// no slide to go through black from). A slide arriving mid-transition starts from where it is.
    fn move_slide(&self, side: Side, t: Option<Transition>) {
        self.end_monitor_anim();
        let mut st = self.live.borrow_mut();
        let m = st.motion.get_or_insert_with(|| {
            let fader = Fader::new();
            let playing: Rc<Cell<Option<Transition>>> = Rc::default();
            let (w, now) = (self.ui.as_weak(), playing.clone());
            let ch = fader.channel(1.0, move |p| if let Some(ui) = w.upgrade() { set_motion(&ui, if p >= 1.0 { None } else { now.get() }, p) });
            Motion { fader, ch, playing }
        });
        let old = self.ui.get_monitor_slide();
        let blank = old.image.size().width == 0 && old.color.alpha() == 0;
        let t = t.filter(|t| t.kind != TransitionKind::Cut && t.ms > 0 && !(blank && t.kind == TransitionKind::Black));
        m.playing.set(t);
        m.fader.jump(m.ch, if t.is_some() { 0.0 } else { 1.0 });
        if t.is_some() { self.ui.set_monitor_slide_old(old) }
        self.ui.set_monitor_slide(side);
        set_motion(&self.ui, t, if t.is_some() { 0.0 } else { 1.0 });
        if let Some(t) = t { m.fader.fade(m.ch, 1.0, Duration::from_millis(u64::from(t.ms.min(crate::design::TRANSITION_MAX_MS)))) }
    }

    /// Plays a designed slide's click group on the monitor, as the room does: `side` (the slide once played) waits
    /// hidden while the group's pictures play over it; a new slide's pictures first come in with its transition
    /// as the old slide goes, a build's play at once. The frame timer runs only while it plays.
    fn animate_slide(&self, side: Side, made: crate::ui::room::anim::Layers, play: crate::design::Play, build: bool, t: Option<Transition>) {
        self.end_monitor_anim();
        let (anim, sprites) = {
            let mut st = self.live.borrow_mut();
            if let Some(m) = &st.motion { m.playing.set(None); m.fader.jump(m.ch, 1.0) }
            let ui = &self.ui;
            let sprites = st.sprites.get_or_insert_with(|| {
                let rows: Rc<VecModel<crate::ui::Sprite>> = Rc::default();
                ui.set_monitor_sprites(ModelRc::from(rows.clone()));
                rows
            }).clone();
            (st.anim.clone(), sprites)
        };
        let lead = t.filter(|t| !build && t.kind != TransitionKind::Cut && t.ms > 0).map_or(0, |t| t.ms.min(crate::design::TRANSITION_MAX_MS));
        let old = self.ui.get_monitor_slide();
        set_motion(&self.ui, None, 1.0);
        if lead > 0 { self.ui.set_monitor_slide_old(old) }
        self.ui.set_monitor_slide_old_op(if lead > 0 { 1.0 } else { 0.0 });
        self.ui.set_monitor_slide(side);
        self.ui.set_monitor_slide_op(0.0);
        let canvas = made.canvas;
        let run = crate::ui::room::anim::Running { imgs: made.images(), play, area: crate::ui::room::anim::area(canvas), lead_ms: lead as f32 };
        let w = self.ui.as_weak();
        anim.start(run, move |rows| {
            let Some(ui) = w.upgrade() else { return };
            match rows {
                Some((rows, k)) => { crate::ui::room::anim::set_rows(&sprites, rows); if lead > 0 { ui.set_monitor_slide_old_op(1.0 - k) } }
                None => { crate::ui::room::anim::set_rows(&sprites, vec![]); set_motion(&ui, None, 1.0) }
            }
        });
    }

    /// Stops the monitor's click group where it is: its pictures go and the slide shows.
    fn end_monitor_anim(&self) {
        let (anim, sprites) = { let st = self.live.borrow(); (st.anim.clone(), st.sprites.clone()) };
        if !anim.stop() { return }
        if let Some(s) = sprites { crate::ui::room::anim::set_rows(&s, vec![]) }
        set_motion(&self.ui, None, 1.0);
    }

    /// Whether a click group is playing on the monitor.
    #[cfg(test)]
    pub(super) fn monitor_animating(&self) -> bool { self.live.borrow().anim.running() }

    /// Starts or stops the 4 Hz grab when the drawn video changes. With the room closed there is nothing to grab:
    /// the monitor stays black and says so.
    fn follow_video(&self, drawn: Option<(u64, bool)>) {
        let video = drawn.map(|(inst, _)| inst);
        self.ui.set_monitor_video(video.is_some());
        self.ui.set_monitor_video_cover(drawn.is_some_and(|(_, background)| background));
        self.mark_room_off();
        let mut st = self.live.borrow_mut();
        if st.video == video { return }
        st.video = video;
        st.video_gen += 1;
        self.ui.set_monitor_video_frame(Image::default());
        if video.is_none() { return st.grab_timer.stop() }
        let me = self.me.clone();
        st.grab_timer.start(TimerMode::Repeated, crate::ui::light::grab_every(), move || if let Some(op) = me.upgrade() { op.grab_tick() });
    }

    /// Light mode came on or went off while a video is live: the grab takes its new pace now, not with the next
    /// video (Automatic turns it on in the middle of a service, when the computer is already struggling).
    pub(super) fn follow_light(&self) {
        let every = crate::ui::light::grab_every();
        let st = self.live.borrow();
        if st.grab_timer.running() && st.grab_timer.interval() != every { st.grab_timer.set_interval(every) }
    }

    /// "Audience screen is off" over a live video while the room window is not open (a room that is opening, or
    /// preparing the video, shows black without it).
    pub(super) fn mark_room_off(&self) {
        let closed = self.state.borrow().snap.as_ref().is_none_or(|s| s.room == RoomStatus::Off);
        self.ui.set_monitor_room_off(closed && self.ui.get_monitor_video());
    }

    /// One 250 ms tick: ask the room's player for its frame, unless the last grab is still on its way.
    fn grab_tick(&self) {
        let Some(grabber) = self.screens().preview_grabber() else { return self.ui.set_monitor_video_frame(Image::default()) };
        let generation = self.live.borrow().video_gen;
        let me = self.me.clone();
        self.preview.grab(grabber, self.preview.size().0, move |px| {
            let Some(op) = me.upgrade() else { return };
            if op.live.borrow().video_gen != generation { return }
            if let Some(px) = px { op.ui.set_monitor_video_frame(Image::from_rgb8(px)) }
        });
    }

    /// Back and Next send the live version as `base` (a double press is refused as stale) and leave the Bible arm
    /// alone; only the keys clear it (`operator.js:343-344`).
    pub(super) fn step(self: &Rc<Self>, next: bool) {
        let base = self.state.borrow().live.as_ref().map(|l| l.view().version);
        self.command(if next { Command::Next } else { Command::Prev }, base);
    }

    /// A command from this column; a refusal becomes a toast (`cmd()`, `operator.js:80-86`).
    pub(crate) fn command(self: &Rc<Self>, cmd: Command, base: Option<u64>) {
        let me = Rc::downgrade(self);
        self.ops.cmd(cmd, base, move |o| {
            if let (Some(op), Some((msg, err))) = (me.upgrade(), outcome_toast(&o)) { op.toast(&msg, err) }
        });
    }

    /// Play/pause on a transport row: the opposite of what it shows, for the instance it shows.
    fn toggle_transport(self: &Rc<Self>, i: usize) {
        let Some(row) = self.live.borrow().rows.get(i).cloned() else { return };
        let cmd = match row.kind {
            TransportKind::Video => Command::MediaPause { inst: row.inst, paused: !row.paused },
            TransportKind::Audio => Command::AudioPause { inst: row.inst, paused: !row.paused },
        };
        self.command(cmd, None);
    }

    /// N4: back 15 s, forward 15 s or 10 s before the end, in transport row `i`.
    fn seek_transport(self: &Rc<Self>, i: usize, which: &str) {
        let Some(row) = self.live.borrow().rows.get(i).cloned() else { return };
        if let Some(cmd) = seek_command(&row, which) { self.command(cmd, None) }
    }

    /// The monitor was resized: draw its layers again at the new size.
    fn monitor_resized(&self) {
        let v = self.state.borrow().live.clone();
        if let Some(v) = v { self.render_monitor(v.view()); self.render_drawing(&v.view().drawing, true) }
    }
}

/// How far each skip button jumps (N4).
pub const SKIP_SECS: f64 = 15.0;
pub const BEFORE_END_SECS: f64 = 10.0;

/// The command for a skip button: "back", "forward" or "end". Anything else does nothing.
pub fn seek_command(row: &TransportRow, which: &str) -> Option<Command> {
    use crate::live::SeekTo;
    let to = match which {
        "back" => SeekTo::By { secs: -SKIP_SECS },
        "forward" => SeekTo::By { secs: SKIP_SECS },
        "end" => SeekTo::BeforeEnd { secs: BEFORE_END_SECS },
        _ => return None,
    };
    Some(Command::Seek { inst: row.inst, to })
}

/// The live song's line, Edit words and the change banner (FEAT-4 §3.4).
pub fn live_song(v: &LiveView) -> LiveSong {
    let line = v.cursor.as_ref().zip(v.slide.as_ref()).and_then(|(_, s)| {
        let r = s.section.as_ref()?;
        Some((crate::sections::strings::live_section_line(&r.label, r.page + 1, r.pages), r.kind.id()))
    });
    let (line, kind) = line.unwrap_or_default();
    let changed = v.changed.as_ref().filter(|c| v.cursor.as_ref().is_some_and(|cur| cur.item_id == c.item_id));
    LiveSong {
        line: line.into(), kind: kind.into(),
        edit: v.item.as_ref().is_some_and(|i| i.song_id.is_some()),
        banner: changed.map_or_else(String::new, |c| c.reason.banner()).into(),
        update: changed.is_some_and(|c| c.reason.can_update()),
    }
}

/// Puts `cells` in `model` in place: rows that changed get their new data, and only a different number of rows
/// resets it (the web rebuilds the rows every time, `renderTransport()`).
fn put_cells(model: &VecModel<TransportCell>, cells: Vec<TransportCell>) {
    if model.row_count() != cells.len() { return model.set_vec(cells) }
    for (i, cell) in cells.into_iter().enumerate() {
        if model.row_data(i).as_ref() != Some(&cell) { model.set_row_data(i, cell) }
    }
}

/// The web's `data-cmd` of a clear button.
fn clear_command(name: &str) -> Option<Command> {
    Some(match name {
        "clear_all" => Command::ClearAll,
        "clear_slide" => Command::ClearSlide,
        "clear_media" => Command::ClearMedia,
        "clear_audio" => Command::ClearAudio,
        "logo" => Command::Logo,
        "clear_overlays" => Command::ClearOverlays,
        "clear_message" => Command::ClearLayers { layers: vec![crate::live::ClearLayer::Message] },
        "clear_timers" => Command::ClearLayers { layers: vec![crate::live::ClearLayer::Timers] },
        _ => return None,
    })
}

/// Connects the column's callbacks. Each holds a `Weak`, so the window does not keep itself alive.
pub(super) fn wire(op: &Rc<Operator>) {
    let ui = &op.ui;
    ui.set_transport(ModelRc::from(op.live.borrow().cells.clone()));
    let me: Weak<Operator> = Rc::downgrade(op);
    ui.on_live_next(move || if let Some(op) = me.upgrade() { op.step(true) });
    let me = Rc::downgrade(op);
    ui.on_live_prev(move || if let Some(op) = me.upgrade() { op.step(false) });
    // The clears send no base.
    let me = Rc::downgrade(op);
    ui.on_live_clear(move |name| {
        let Some(op) = me.upgrade() else { return };
        if name == "clear_drawing" { return op.clear_drawing() }
        if let Some(cmd) = clear_command(&name) { op.command(cmd, None) }
    });
    let me = Rc::downgrade(op);
    ui.on_transport_toggle(move |i| if let (Some(op), Ok(i)) = (me.upgrade(), usize::try_from(i)) { op.toggle_transport(i) });
    let me = Rc::downgrade(op);
    ui.on_transport_seek(move |i, which| if let (Some(op), Ok(i)) = (me.upgrade(), usize::try_from(i)) { op.seek_transport(i, &which) });
    let me = Rc::downgrade(op);
    ui.on_live_song_action(move |a| {
        let Some(op) = me.upgrade() else { return };
        match a.as_str() {
            "edit" => super::slide_edit::edit_live(&op),
            "update" => crate::ui::dialogs::song_editor::refresh_live(&op),
            _ => {}
        }
    });
    let me = Rc::downgrade(op);
    ui.on_monitor_resized(move |w| if let Some(op) = me.upgrade() {
        // Kept here for every window: the preview may be in a window of its own.
        op.ui.set_monitor_width(w);
        op.monitor_resized()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, LiveUpdate, test_app},
        live::Command,
        model::{EndAction, ItemKind, MediaFile, MediaKind, MediaStatus, ServiceItem},
        render::Renderer,
        ui::{operator::{view, window::testing::{find, open, refresh}}, screens::NativeScreens, ops::Ops, ui_test_init},
        platform::monitors::MonitorId,
    };
    use slint::platform::{PointerEventButton, WindowEvent};
    use std::sync::Arc;
    use uuid::Uuid;

    #[test]
    fn skip_buttons_make_seek_commands_for_their_row() {
        use crate::live::SeekTo;
        let row = TransportRow { kind: TransportKind::Video, inst: 7, label: "Video".into(), paused: false, percent: 0.0, time: String::new() };
        assert_eq!(seek_command(&row, "back"), Some(Command::Seek { inst: 7, to: SeekTo::By { secs: -15.0 } }));
        assert_eq!(seek_command(&row, "forward"), Some(Command::Seek { inst: 7, to: SeekTo::By { secs: 15.0 } }));
        assert_eq!(seek_command(&row, "end"), Some(Command::Seek { inst: 7, to: SeekTo::BeforeEnd { secs: 10.0 } }));
        assert_eq!(seek_command(&row, "sideways"), None);
    }

    fn add_text(app: &App, title: &str, body: &str) -> Uuid {
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: title.into(), body: body.into(), layout: Default::default() }, background: None, notes: String::new(), theme: None };
        app.data().services[0].items.push(item.clone());
        item.id
    }

    fn add_video(app: &App) -> Uuid {
        let media = MediaFile { id: Uuid::new_v4(), name: "clip.mp4".into(), kind: MediaKind::Video, asset: "clip.mp4".into(), status: MediaStatus::Ready, original_asset: None, duration: None, thumb: None, size: None, folder: String::new() };
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Media { media_id: media.id, autoplay: true, end: EndAction::Stop, looped: false }, background: None, notes: String::new(), theme: None };
        let mut d = app.data();
        d.media.push(media);
        d.services[0].items.push(item.clone());
        item.id
    }

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        op
    }

    /// Runs `cmd` the way a phone would and hands the operator the new live view (no follower in tests).
    fn run(op: &Operator, cmd: Command) {
        op.ops().cmd(cmd, None, |o| assert!(o.ok, "{o:?}"));
        live(op);
    }

    fn live(op: &Operator) { op.apply_live(LiveUpdate::new(op.app().live_view())); }

    fn click(op: &Operator, id: &str) {
        find(op, id).unwrap_or_else(|| panic!("{id}")).mock_single_click(PointerEventButton::Left);
        live(op);
    }

    fn cursor(app: &App) -> Option<(Uuid, usize)> { app.live_view().cursor.map(|c| (c.item_id, c.slide)) }

    fn label(op: &Operator, id: &str) -> String { find(op, id).and_then(|e| e.accessible_label()).unwrap_or_default().to_string() }

    #[test]
    fn transport_rows_from_view() {
        let app = test_app();
        let id = add_video(&app);
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        let want = view::transport(&app.live_view());
        let rows: Vec<_> = slint::Model::iter(&op.ui.get_transport()).collect();
        assert_eq!(rows.len(), want.len());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].video);
        assert_eq!((rows[0].label.as_str(), rows[0].time.as_str(), rows[0].paused), ("Video", "—", want[0].paused));
    }

    #[test]
    fn next_button_advances_live() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2\n\na3");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        click(&op, "LiveColumn::next-btn");
        assert_eq!(cursor(&app), Some((id, 1)));
        click(&op, "LiveColumn::next-btn");
        assert_eq!(cursor(&app), Some((id, 2)));
    }

    #[test]
    fn next_button_keeps_the_arm() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        op.state.borrow_mut().armed = Some(id);
        click(&op, "LiveColumn::next-btn");
        assert_eq!(cursor(&app), Some((id, 1)));
        assert_eq!(op.state.borrow().armed, Some(id), "only the keys clear the arm (operator.js:343-344)");
        click(&op, "LiveColumn::prev-btn");
        assert_eq!(op.state.borrow().armed, Some(id));
    }

    #[test]
    fn prev_button_goes_back() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2\n\na3");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 2 });
        click(&op, "LiveColumn::prev-btn");
        assert_eq!(cursor(&app), Some((id, 1)));
    }

    #[test]
    fn next_button_sends_the_live_version_as_base() {
        let app = test_app();
        let id = add_text(&app, "A", "a1\n\na2\n\na3");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        // Something else moved the screen; this window has not heard yet, so its Next is stale and silent.
        app.command(crate::live::Envelope::new(Command::Show { item_id: id, slide: 2 }));
        find(&op, "LiveColumn::next-btn").unwrap().mock_single_click(PointerEventButton::Left);
        assert_eq!(cursor(&app), Some((id, 2)), "a stale Next does nothing");
    }

    #[test]
    fn clear_all_button_clears_frame() {
        let app = test_app();
        let id = add_text(&app, "A", "a1");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        assert!(app.live_view().frame.slide.is_some());
        assert_eq!(label(&op, "LiveColumn::live-what"), "A · A 1");
        click(&op, "LiveColumn::clear-all");
        assert!(app.live_view().frame.slide.is_none());
        assert_eq!(label(&op, "LiveColumn::live-what"), "Clear");
        run(&op, Command::Show { item_id: id, slide: 0 });
        click(&op, "LiveColumn::clear-text");
        assert!(app.live_view().frame.slide.is_none());
    }

    #[test]
    fn logo_button_marked_on_while_logo_shown() {
        let app = test_app();
        let op = opened(&app);
        let on = || find(&op, "LiveColumn::clear-logo").unwrap().accessible_checked();
        assert_eq!(on(), Some(false));
        click(&op, "LiveColumn::clear-logo");
        assert!(app.live_view().frame.logo);
        assert_eq!(on(), Some(true));
        assert_eq!(label(&op, "LiveColumn::live-what"), "Logo");
        // Logo is not a toggle (`live.rs`): another clear takes it off.
        click(&op, "LiveColumn::clear-all");
        assert_eq!(on(), Some(false));
    }

    #[test]
    fn pending_marker_while_requested_not_committed() {
        let app = test_app();
        let id = add_text(&app, "A", "a1");
        let op = opened(&app);
        assert!(find(&op, "LiveColumn::pending").is_none());
        run(&op, Command::Show { item_id: id, slide: 0 });
        assert!(app.live_view().pending, "no room committed it");
        assert_eq!(label(&op, "LiveColumn::pending"), "updating…");
        let mut v = app.live_view();
        v.pending = false;
        v.version += 1;
        op.apply_live(LiveUpdate::new(v));
        assert!(find(&op, "LiveColumn::pending").is_none());
    }

    #[test]
    fn live_slide_notes_show_under_the_monitor() {
        let app = test_app();
        let op = opened(&app);
        assert!(find(&op, "LiveColumn::live-notes").is_none());
        let mut v = app.live_view();
        let mut slide = crate::model::Slide::new("Slide 1", crate::model::SlideContent::Image { asset: "a.png".into(), transition: None });
        slide.notes = "Welcome everyone".into();
        v.slide = Some(slide);
        v.version += 1;
        op.apply_live(LiveUpdate::new(v));
        assert_eq!(label(&op, "LiveColumn::live-notes"), "Notes: Welcome everyone");
    }

    #[test]
    fn notice_shown_from_live_view() {
        let app = test_app();
        let op = opened(&app);
        assert!(find(&op, "LiveColumn::notice").is_none());
        let mut v = app.live_view();
        v.notice = Some("The audience screen restarted.".into());
        v.version += 1;
        op.apply_live(LiveUpdate::new(v));
        assert_eq!(label(&op, "LiveColumn::notice"), "The audience screen restarted.");
    }

    #[test]
    fn room_monitor_gone_shows_notice() {
        let app = test_app();
        ui_test_init();
        let screens = NativeScreens::new(&app, Renderer::new(), None);
        let op = crate::ui::operator::window::Operator::open(app.clone(), Ops::inline(app.clone()), Rc::new(screens.clone())).unwrap();
        let mon = |name: &str, x: i32| MonitorId { name: name.into(), size: (1920, 1080), position: (x, 0) };
        let (laptop, projector) = (mon(r"\\.\DISPLAY1", 0), mon(r"\\.\DISPLAY2", 1920));
        screens.look_at(vec![laptop.clone(), projector.clone()], Some(laptop.clone()));
        crate::app::Screens::open(&*screens, "room", Some(&projector.key())).unwrap();
        // The projector is unplugged: the Plan 2 watcher closes the room and leaves a notice for the operator.
        screens.look_at(vec![laptop.clone()], Some(laptop));
        live(&op);
        assert_eq!(label(&op, "LiveColumn::notice"), crate::ui::screens::gone_notice("room"));
        screens.shutdown();
    }

    #[test]
    fn up_next_end_of_service() {
        let app = test_app();
        let op = opened(&app);
        assert_eq!(label(&op, "LiveColumn::up-next"), "—");
        let id = add_text(&app, "A", "a1\n\na2");
        refresh(&op);
        run(&op, Command::Show { item_id: id, slide: 0 });
        let up = find(&op, "LiveColumn::up-next").unwrap();
        assert_eq!((up.accessible_description().unwrap().as_str(), up.accessible_label().unwrap().as_str()), ("A · A 2", "a2"));
        run(&op, Command::Show { item_id: id, slide: 1 });
        assert_eq!(label(&op, "LiveColumn::up-next"), "End of service");
    }

    #[test]
    fn transport_button_tooltip_play_or_pause() {
        let app = test_app();
        let id = add_video(&app);
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        let tip = || find(&op, "LiveColumn::transport-video").and_then(|b| b.accessible_description()).unwrap_or_default().to_string();
        let paused = || matches!(app.live_view().frame.media.map(|m| m.v), Some(crate::live::MediaLayer::Video { paused: true, .. }));
        assert!(!paused());
        assert_eq!(tip(), "Pause");
        click(&op, "LiveColumn::transport-video");
        assert!(paused(), "the button sends media_pause for the instance on air");
        assert_eq!(tip(), "Play");
        click(&op, "LiveColumn::transport-video");
        assert!(!paused());
        assert_eq!(tip(), "Pause");
    }

    #[test]
    fn transport_click_survives_a_live_update_mid_press() {
        let app = test_app();
        let id = add_video(&app);
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        let btn = find(&op, "LiveColumn::transport-video").unwrap();
        let (p, s) = (btn.absolute_position(), btn.size());
        let at = slint::LogicalPosition::new(p.x + s.width / 2.0, p.y + s.height / 2.0);
        let w = op.window();
        w.dispatch_event(WindowEvent::PointerMoved { position: at });
        w.dispatch_event(WindowEvent::PointerPressed { position: at, button: PointerEventButton::Left });
        // The player reports its time (every 500 ms while it plays) between the press and the release.
        live(&op);
        w.dispatch_event(WindowEvent::PointerReleased { position: at, button: PointerEventButton::Left });
        let paused = matches!(app.live_view().frame.media.map(|m| m.v), Some(crate::live::MediaLayer::Video { paused: true, .. }));
        assert!(paused, "the click lands although the live view changed under the mouse");
    }

    #[test]
    fn monitor_fits_a_foreground_video_and_crops_a_background_video() {
        let app = test_app();
        let id = add_video(&app);
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        assert!(op.ui.get_monitor_video());
        assert!(!op.ui.get_monitor_video_cover(), "a foreground video is letterboxed like the room (object-fit: contain)");
        let mut v = app.live_view();
        v.frame.media = None;
        v.frame.background = Some(crate::live::Inst { inst: 99, v: crate::model::Background::Video { asset: "bg.mp4".into(), dim: 0 } });
        v.version += 1;
        op.apply_live(LiveUpdate::new(v));
        assert!(op.ui.get_monitor_video());
        assert!(op.ui.get_monitor_video_cover(), "a background video fills and crops, as the room's panscan does (object-fit: cover)");
    }

    #[test]
    fn the_video_preview_slows_down_when_light_mode_comes_on_mid_video() {
        use crate::{model::LightMode, ui::light};
        light::set_mode(LightMode::Off);
        let app = test_app();
        let id = add_video(&app);
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        assert_eq!(op.live.borrow().grab_timer.interval(), light::GRAB_EVERY);
        light::set_mode(LightMode::On);
        op.light_tick();
        assert_eq!(op.live.borrow().grab_timer.interval(), light::GRAB_EVERY_LIGHT, "the video that is playing, not only the next");
        light::set_mode(LightMode::Off);
        op.light_tick();
        assert_eq!(op.live.borrow().grab_timer.interval(), light::GRAB_EVERY);
        light::set_mode(LightMode::Auto);
    }

    #[test]
    fn video_preview_without_room_says_room_screen_is_off() {
        let app = test_app();
        let id = add_video(&app);
        let txt = add_text(&app, "A", "a1");
        let op = opened(&app);
        assert!(find(&op, "LiveMonitor::room-off").is_none());
        run(&op, Command::Show { item_id: id, slide: 0 });
        assert!(op.ui.get_monitor_video());
        assert_eq!(label(&op, "LiveMonitor::room-off"), "Audience screen is off");
        assert_eq!(op.ui.get_monitor_video_frame().size().width, 0, "no second player: the frame stays empty");
        // Once the room is open (and drawing nothing yet) the caption goes; a slide has no caption at all.
        let _room = app.screen_guard("room");
        refresh(&op);
        assert!(find(&op, "LiveMonitor::room-off").is_none());
        run(&op, Command::Show { item_id: txt, slide: 0 });
        assert!(!op.ui.get_monitor_video());
    }

    #[test]
    fn monitor_shows_the_live_slide() {
        let app = test_app();
        let id = add_text(&app, "A", "Amazing grace");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        assert!(op.ui.get_monitor_slide().image.size().width > 0, "the slide text is drawn");
        assert!(op.ui.get_monitor_bg().color.alpha() > 0, "the settings background is drawn");
        click(&op, "LiveColumn::clear-all");
        let slide = op.ui.get_monitor_slide();
        assert!(slide.image.size().width == 0 && slide.color.alpha() == 0, "cleared at once, with no fade");
    }

    #[test]
    fn monitor_plays_a_deck_slides_own_transition() {
        use crate::design::{Dir, Transition, TransitionKind};
        let app = test_app();
        let id = add_text(&app, "A", "Amazing grace");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        let first = op.ui.get_monitor_slide();
        assert_eq!(op.ui.get_monitor_slide_geo(), Geo::default(), "a text slide cuts in");
        let push = Transition { kind: TransitionKind::Push, ms: 300, dir: Dir::Left };
        let mut v = app.live_view();
        v.frame.slide = Some(SlideLayer::Image { asset: "missing.png".into(), transition: Some(push) });
        v.version += 1;
        op.apply_live(LiveUpdate::new(v.clone()));
        assert_eq!(op.ui.get_monitor_slide_old(), first, "the old slide goes out");
        assert!(op.ui.get_monitor_slide_geo().x > 0.95, "the new one starts off to the right");
        assert!(op.ui.get_monitor_slide_old_geo().x.abs() < 1e-3 && op.ui.get_monitor_slide_old_op() == 1.0, "the old one starts in place");
        // A redraw of the same slide (a resize) plays nothing.
        op.apply_live(LiveUpdate::new(v.clone()));
        assert!(op.ui.get_monitor_slide_geo().x > 0.95);
        std::thread::sleep(Duration::from_millis(350));
        op.live.borrow().motion.as_ref().unwrap().fader.tick();
        assert_eq!((op.ui.get_monitor_slide_geo(), op.ui.get_monitor_slide_old_geo()), (Geo::default(), Geo::default()));
        let old = op.ui.get_monitor_slide_old();
        assert!(old.image.size().width == 0 && old.color.alpha() == 0, "the old slide is gone once it is over");
    }

    #[test]
    fn monitor_plays_a_designed_slides_click_group_then_shows_the_slide() {
        use crate::design::{Anim, AnimKind, Design, Effect, Obj, ObjKind, PctRect, Shape, Start, Transition, TransitionKind};
        let app = test_app();
        let op = opened(&app);
        let shape = |x| { let mut o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, PctRect::new(x, 10.0, 20.0, 20.0)); o.look.fill = Some("#ff0000".into()); o };
        let (a, b) = (shape(10.0), shape(60.0));
        let mut fly = Anim::new(b.id, AnimKind::Entrance, Effect::Fly, Start::OnClick);
        fly.dur_ms = 200;
        let mut fade = Anim::new(a.id, AnimKind::Entrance, Effect::Fade, Start::WithPrevious);
        fade.dur_ms = 200;
        let transition = Transition { kind: TransitionKind::Fade, ms: 200, ..Default::default() };
        let d = Design { objects: vec![a, b], anims: vec![fade, fly], transition, ..Default::default() };
        let rows = || op.ui.get_monitor_sprites().row_count();
        let over = |op: &Operator| {
            std::thread::sleep(Duration::from_millis(450));
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(450));
            assert!(!op.monitor_animating(), "the timer stops once it has played");
        };
        // A new slide: group 0 comes in with the slide's transition, then plays.
        let mut v = app.live_view();
        v.frame.slide = Some(SlideLayer::Design { design: d.at_step(0), build: false, play: d.play(0).map(Box::new) });
        v.version += 1;
        op.apply_live(LiveUpdate::new(v.clone()));
        assert!(op.monitor_animating());
        assert_eq!(rows(), 1, "the fading shape");
        assert_eq!(op.ui.get_monitor_sprites().row_data(0).unwrap().op, 0.0);
        assert_eq!(op.ui.get_monitor_slide_op(), 0.0, "the slide waits under them");
        assert!(op.ui.get_monitor_slide().image.size().width > 0);
        // A redraw of the same slide (a resize) starts nothing new.
        op.apply_live(LiveUpdate::new(v.clone()));
        over(&op);
        assert_eq!((rows(), op.ui.get_monitor_slide_op()), (0, 1.0));
        // The click: both shapes, the flying one starting off the slide, the old slide gone at once.
        v.frame.slide = Some(SlideLayer::Design { design: d.at_step(1), build: true, play: d.play(1).map(Box::new) });
        v.version += 1;
        op.apply_live(LiveUpdate::new(v.clone()));
        assert_eq!(rows(), 2);
        let flying = op.ui.get_monitor_sprites().row_data(1).unwrap();
        assert!(flying.dy > 0.5, "it comes up from below: {}", flying.dy);
        assert_eq!((op.ui.get_monitor_slide_op(), op.ui.get_monitor_slide_old_op()), (0.0, 0.0));
        over(&op);
        assert_eq!((rows(), op.ui.get_monitor_slide_op()), (0, 1.0));
        // Any other slide stops a group at once.
        v.frame.slide = Some(SlideLayer::Design { design: d.at_step(0), build: false, play: d.play(0).map(Box::new) });
        v.version += 1;
        op.apply_live(LiveUpdate::new(v.clone()));
        assert!(op.monitor_animating());
        v.frame.slide = None;
        v.version += 1;
        op.apply_live(LiveUpdate::new(v));
        assert!(!op.monitor_animating());
        assert_eq!(rows(), 0);
    }

    #[test]
    fn monitor_transition_steps() {
        use crate::design::{Dir, Transition, TransitionKind};
        let app = test_app();
        let op = opened(&app);
        let t = |kind| Some(Transition { kind, ms: 500, dir: Dir::Right });
        let ui = &op.ui;
        let read = || (ui.get_monitor_slide_old_op(), ui.get_monitor_slide_op(), ui.get_monitor_black());
        set_motion(ui, t(TransitionKind::Fade), 0.25);
        assert_eq!(read(), (0.75, 0.25, 0.0));
        set_motion(ui, t(TransitionKind::Black), 0.25);
        assert_eq!(read(), (1.0, 0.0, 0.5), "the old slide darkens first");
        set_motion(ui, t(TransitionKind::Black), 0.75);
        assert_eq!(read(), (0.0, 1.0, 0.5), "then the new one comes up");
        set_motion(ui, t(TransitionKind::Wipe), 0.25);
        assert_eq!((ui.get_monitor_slide_geo().l, ui.get_monitor_slide_old_geo().r), (0.0, 0.0));
        assert_eq!((ui.get_monitor_slide_geo().r, ui.get_monitor_slide_old_geo().l), (0.75, 0.25));
        set_motion(ui, None, 1.0);
        assert_eq!(read(), (0.0, 1.0, 0.0));
        assert_eq!(own_transition(Some(&SlideLayer::Image { asset: "a.png".into(), transition: t(TransitionKind::Cut) })), t(TransitionKind::Cut));
    }

    #[test]
    fn a_phone_drawing_shows_on_the_monitor_with_a_chip_that_clears_it() {
        use crate::live::drawing::{DrawCmd, Pen, PALETTE};
        let app = test_app();
        let phone = Uuid::new_v4();
        app.phones.lock().unwrap().push(crate::app::Phone { id: phone, name: "iPad".into(), hash: String::new(), last_seen: 0, may_draw: true });
        let id = add_text(&app, "A", "Amazing grace");
        let op = opened(&app);
        run(&op, Command::Show { item_id: id, slide: 0 });
        assert!(find(&op, "LiveColumn::drawing-chip").is_none());
        assert_eq!(op.ui.get_monitor_drawing().size().width, 0);
        app.draw(phone, &DrawCmd::Start { stroke: 1, color: PALETTE[0].into(), width: 20.0, pen: Pen::Pen, points: vec![[0.2, 0.5], [0.8, 0.5]], ink: Default::default() }).unwrap();
        // What the follower hands over for `Update::Drawing`.
        op.render_drawing(&app.live_view().drawing, false);
        let size = op.ui.get_monitor_drawing().size();
        assert_eq!((size.width, size.height), op.preview.size(), "drawn at the monitor's size");
        assert!(size.width > 0);
        assert_eq!(label(&op, "LiveColumn::drawing-chip"), "Drawing");
        // An older live view does not bring back an older drawing.
        let mut old = app.live_view();
        old.drawing = Default::default();
        op.render_drawing(&old.drawing, false);
        assert!(op.ui.get_monitor_drawing().size().width > 0);
        // The ✕ clears it.
        find(&op, "LiveColumn::clear-drawing").unwrap().mock_single_click(PointerEventButton::Left);
        assert!(app.live_view().drawing.is_empty());
        live(&op);
        assert!(find(&op, "LiveColumn::drawing-chip").is_none());
        assert_eq!(op.ui.get_monitor_drawing().size().width, 0);
        // A new slide clears a drawing on its own.
        app.draw(phone, &DrawCmd::Start { stroke: 2, color: PALETTE[0].into(), width: 20.0, pen: Pen::Pen, points: vec![[0.2, 0.5]], ink: Default::default() }).unwrap();
        live(&op);
        assert!(find(&op, "LiveColumn::drawing-chip").is_some());
        click(&op, "LiveColumn::clear-all");
        assert!(find(&op, "LiveColumn::drawing-chip").is_none());
    }
}
