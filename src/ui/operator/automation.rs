//! FEAT-2's automation strip in the live column (`ui/operator/automation-strip.slint`, spec §3.1): the Message
//! button follows the frame, timer chips follow `Update::Timers`, macro buttons follow the data.
use super::window::Operator;
use crate::{
    automation::{macros::MacroColor, slots},
    live::Command,
    timers::{RunState, TimerState, TimersView},
    ui::{AutoMacroBtn, AutoMacroRow, AutoStrip, AutoTimerChip, ops::outcome_toast},
};
use serde_json::{Value, json};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// At most this many timer chips; the rest are in the Automation dialog.
pub const MAX_CHIPS: usize = 2;
/// At most this many macro buttons (two rows of three).
pub const MAX_MACROS: usize = 6;

/// A button colour for each macro colour.
pub fn macro_color(c: MacroColor) -> slint::Color {
    let rgb = match c {
        MacroColor::Amber => 0xf5b94a,
        MacroColor::Red => 0xff6b6b,
        MacroColor::Orange => 0xff9f43,
        MacroColor::Green => 0x3ccf91,
        MacroColor::Teal => 0x2ec4b6,
        MacroColor::Blue => 0x5aa9ff,
        MacroColor::Purple => 0xb18cff,
        MacroColor::Gray => 0xa0a4ab,
    };
    slint::Color::from_rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// The timers worth a chip: running, paused, finished or on the screen, in Timers-tab order.
pub fn timer_chips(view: &TimersView, in_room: &[Uuid], can_pause: impl Fn(Uuid) -> bool) -> Vec<AutoTimerChip> {
    view.timers.iter()
        .filter(|t| t.state != TimerState::Stopped || in_room.contains(&t.id))
        .take(MAX_CHIPS)
        .map(|t| AutoTimerChip {
            id: t.id.to_string().into(),
            name: t.name.as_str().into(),
            text: crate::timers::text(t.value_ms).into(),
            over: t.overrun,
            running: t.state == TimerState::Running,
            can_pause: can_pause(t.id),
            in_room: in_room.contains(&t.id),
        })
        .collect()
}

/// Draws the strip from the newest data, live view and timer readout.
pub fn render(op: &Operator) {
    let g = op.ui.global::<AutoStrip>();
    let (overlays, macros) = {
        let st = op.state.borrow();
        let overlays = st.live.as_ref().map(|l| l.view().frame.overlays.clone()).unwrap_or_default();
        let macros = st.snap.as_ref().map(|s| s.data.automation.macros.clone()).unwrap_or_default();
        (overlays, macros)
    };
    let view = op.app().timers();
    g.set_message_on(slots::shown_message(&overlays).is_some());
    let in_room = slots::room_timers(&overlays);
    // A countdown to a time can't be paused (spec §5.1), so its chip has no pause button while it runs.
    let to_time: Vec<Uuid> = op.app().automation.lock().unwrap().timers.iter().filter(|(_, r)| matches!(r.state, RunState::RunningTo { .. })).map(|(id, _)| *id).collect();
    let chips = timer_chips(&view, &in_room, |id| !to_time.contains(&id));
    g.set_timers(ModelRc::new(VecModel::from(chips)));
    let buttons: Vec<AutoMacroBtn> = macros.iter().take(MAX_MACROS)
        .map(|m| AutoMacroBtn { id: m.id.to_string().into(), name: m.name.as_str().into(), color: macro_color(m.color) }).collect();
    let rows: Vec<AutoMacroRow> = buttons.chunks(3).map(|c| AutoMacroRow { buttons: ModelRc::new(VecModel::from(c.to_vec())) }).collect();
    g.set_macros(ModelRc::new(VecModel::from(rows)));
    match &view.auto {
        Some(a) => {
            g.set_auto_shown(true);
            g.set_auto_paused(a.paused);
            g.set_auto_label(crate::tr!("Auto · {time}", time = crate::timers::text(i64::from(a.next_in_secs) * 1000)).into());
        }
        None => g.set_auto_shown(false),
    }
}

/// The live view the operator last saw: its version and clear epoch.
fn seen(op: &Operator) -> (Option<u64>, Option<u64>) {
    let st = op.state.borrow();
    let v = st.live.as_ref().map(|l| l.view());
    (v.map(|v| v.version), v.map(|v| v.clear_epoch))
}

/// An action whose failure is a toast.
fn call(op: &Rc<Operator>, action: &'static str, body: Value) {
    let me = Rc::downgrade(op);
    op.ops().call::<Value>(action, body, move |r| {
        let Some(op) = me.upgrade() else { return };
        if let Err(e) = r { op.toast(&e, true) }
        render(&op);
    });
}

fn chip(op: &Operator, id: &str) -> Option<AutoTimerChip> {
    let g = op.ui.global::<AutoStrip>();
    g.get_timers().iter().find(|t| t.id == id)
}

pub(super) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<AutoStrip>();
    let with = |f: fn(&Rc<Operator>)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move || if let Some(op) = me.upgrade() { f(&op) }
    };
    let with_id = |f: fn(&Rc<Operator>, &str)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move |id: slint::SharedString| if let Some(op) = me.upgrade() { f(&op, &id) }
    };
    g.on_message(with(|op| {
        if op.ui.global::<AutoStrip>().get_message_on() { op.command(Command::HideMessage, None) } else { crate::ui::dialogs::show_message::open(op) }
    }));
    g.on_auto_toggle(with(|op| {
        let paused = op.ui.global::<AutoStrip>().get_auto_paused();
        call(op, "auto/set", json!({ "on": paused }));
    }));
    g.on_open_automation(with(|op| crate::ui::dialogs::automation::open(op)));
    g.on_timer_toggle(with_id(|op, id| {
        let running = chip(op, id).is_some_and(|c| c.running);
        call(op, if running { "timer/pause" } else { "timer/start" }, json!({ "id": id }));
    }));
    g.on_timer_reset(with_id(|op, id| call(op, "timer/reset", json!({ "id": id }))));
    g.on_timer_room(with_id(|op, id| {
        let Ok(timer) = Uuid::parse_str(id) else { return };
        let in_room = chip(op, id).is_some_and(|c| c.in_room);
        let cmd = if in_room { Command::HideTimer { timer } } else { Command::ShowTimer { timer, seen_clear: seen(op).1 } };
        op.command(cmd, None);
    }));
    g.on_run_macro(with_id(|op, id| run_macro(op, id)));
}

/// Runs a macro from its button: it sends the screen the operator saw, so a stale one says so (spec §7).
pub fn run_macro(op: &Rc<Operator>, id: &str) {
    let (base, seen_clear) = seen(op);
    let me = Rc::downgrade(op);
    op.ops().call::<Value>("automation/run", json!({ "id": id, "base": base, "seen_clear": seen_clear }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) if v["ok"] == true => if let Some(t) = v["toast"].as_str() { op.toast(t, v["problem"] == true) },
            Ok(v) => {
                let o = crate::live::Outcome { ok: false, version: 0, error: v["error"].as_str().map(str::to_string), duplicate: false };
                if let Some((msg, err)) = outcome_toast(&o) { op.toast(&msg, err) }
            }
            Err(e) => op.toast(&e, true),
        }
    });
}

