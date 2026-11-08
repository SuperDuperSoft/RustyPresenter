//! The Automation dialog (FEAT-2 §3.2–§3.6, `ui/dialogs/automation.slint`): Timers, Messages, Macros and the
//! Pre-service card. Saves go through the `automation/*` actions, so the core checks everything.
use super::{DialogKind, confirm, strings as dlg};
use crate::{
    automation::{
        Automation,
        calendar::{CalendarEntry, CalendarRun, EVERY_DAY, next_due},
        macros::{ClearWhich, ItemTarget, Macro, MacroColor, MacroStep},
        messages::{self, HIDE_AFTER_CHOICES, MessagePlace, MessageTemplate},
        preservice::{PRE_SERVICE_MACRO_ID, PreService, PreServiceEnd},
    },
    lenient::Lenient,
    model::Data,
    timers::{AtZero, OverlayPlace, Overrun, TimeOfDay, TimeTarget, TimerDef, TimerKind, TimerState},
    ui::{AutoListRow, AutoStepRow, AutomationDlg, operator::window::Operator},
};
use serde_json::{Value, json};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, collections::BTreeMap, rc::{Rc, Weak}};
use uuid::Uuid;

const PLACES: [OverlayPlace; 5] = [OverlayPlace::TopRight, OverlayPlace::TopLeft, OverlayPlace::BottomRight, OverlayPlace::BottomLeft, OverlayPlace::CenterLarge];
const COLORS: [MacroColor; 8] = [MacroColor::Amber, MacroColor::Red, MacroColor::Orange, MacroColor::Green, MacroColor::Teal, MacroColor::Blue, MacroColor::Purple, MacroColor::Gray];
const ENDS: [PreServiceEnd; 4] = [PreServiceEnd::Hide, PreServiceEnd::Logo, PreServiceEnd::ClearAll, PreServiceEnd::FirstItem];

/// The Calendar tab's number (N17).
pub const CALENDAR_TAB: i32 = 4;

/// The kinds of step the Add step list offers, in its order (spec §3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind { ShowItem, Next, Prev, Clear(ClearWhich), Logo, TimerStart, TimerPause, TimerReset, TimerRestart, TimerShow, TimerHide,
    MessageShow, MessageHide, OverlayOn, OverlayOff, AutoOff, AutoOn, StageMessage, StageMessageHide, MusicPlay, MusicStop }

pub const STEP_KINDS: [StepKind; 25] = [
    StepKind::ShowItem, StepKind::Next, StepKind::Prev, StepKind::Clear(ClearWhich::All), StepKind::Clear(ClearWhich::Slide),
    StepKind::Clear(ClearWhich::Media), StepKind::Clear(ClearWhich::Audio), StepKind::Clear(ClearWhich::Overlays), StepKind::Logo,
    StepKind::TimerStart, StepKind::TimerPause, StepKind::TimerReset, StepKind::TimerRestart, StepKind::TimerShow, StepKind::TimerHide,
    StepKind::MessageShow, StepKind::MessageHide, StepKind::OverlayOn, StepKind::OverlayOff, StepKind::AutoOff, StepKind::AutoOn,
    StepKind::StageMessage, StepKind::StageMessageHide, StepKind::MusicPlay, StepKind::MusicStop,
];

fn kind_label(k: StepKind) -> String {
    match k {
        StepKind::ShowItem => crate::tr!("Put a service item on the screen"),
        StepKind::Next => crate::tr!("Next slide"),
        StepKind::Prev => crate::tr!("Previous slide"),
        StepKind::Clear(ClearWhich::All) => crate::tr!("Clear everything"),
        StepKind::Clear(ClearWhich::Slide) => crate::tr!("Clear words"),
        StepKind::Clear(ClearWhich::Media) => crate::tr!("Clear video and pictures"),
        StepKind::Clear(ClearWhich::Audio) => crate::tr!("Clear audio"),
        StepKind::Clear(ClearWhich::Overlays) => crate::tr!("Clear overlays"),
        StepKind::Logo => crate::tr!("Show the logo"),
        StepKind::TimerStart => crate::tr!("Start timer"),
        StepKind::TimerPause => crate::tr!("Pause timer"),
        StepKind::TimerReset => crate::tr!("Reset timer"),
        StepKind::TimerRestart => crate::tr!("Restart timer"),
        StepKind::TimerShow => crate::tr!("Show timer on the screen"),
        StepKind::TimerHide => crate::tr!("Hide timer from the screen"),
        StepKind::MessageShow => crate::tr!("Show a message"),
        StepKind::MessageHide => crate::tr!("Hide the message"),
        StepKind::OverlayOn => crate::tr!("Turn an overlay on"),
        StepKind::OverlayOff => crate::tr!("Turn an overlay off"),
        StepKind::AutoOff => crate::tr!("Pause automatic slides"),
        StepKind::AutoOn => crate::tr!("Resume automatic slides"),
        StepKind::StageMessage => crate::tr!("Show a message to the stage"),
        StepKind::StageMessageHide => crate::tr!("Hide the stage message"),
        StepKind::MusicPlay => crate::tr!("Play the walk-in music"),
        StepKind::MusicStop => crate::tr!("Stop the walk-in music"),
    }
}

/// What a step's second list picks from.
fn targets(k: StepKind, data: &Data) -> Vec<(Uuid, String)> {
    let a = &data.automation;
    match k {
        StepKind::ShowItem => service_items(data).into_iter().map(|(id, t, _)| (id, t)).collect(),
        StepKind::TimerStart | StepKind::TimerPause | StepKind::TimerReset | StepKind::TimerRestart | StepKind::TimerShow | StepKind::TimerHide => {
            a.timers.iter().map(|t| (t.id, t.name.clone())).collect()
        }
        StepKind::MessageShow => a.messages.iter().map(|t| (t.id, t.name.clone())).collect(),
        StepKind::OverlayOn | StepKind::OverlayOff => data.overlays.iter().map(|o| (o.id, o.name.clone())).collect(),
        _ => vec![],
    }
}

fn service_items(data: &Data) -> Vec<(Uuid, String, &'static str)> {
    data.service().map(|s| s.items.iter().map(|i| { let (t, k, _) = crate::slides::build_item(data, i); (i.id, t, k) }).collect()).unwrap_or_default()
}

/// `Number=27, Plate=ABC` → the values of a message step.
pub fn parse_values(text: &str) -> BTreeMap<String, String> {
    text.split(',').filter_map(|p| { let (k, v) = p.split_once('=')?; Some((k.trim().to_string(), v.trim().to_string())) }).filter(|(k, _)| !k.is_empty()).collect()
}

/// A step as the list shows it.
pub fn step_label(s: &Lenient<MacroStep>, a: &Automation, data: &Data) -> String {
    let timer = |id: &Uuid| a.timers.iter().find(|t| t.id == *id).map_or_else(|| "?".to_string(), |t| t.name.clone());
    let with = |k: StepKind, what: String| format!("{}: {what}", kind_label(k));
    let Some(s) = s.known() else { return crate::tr!("Needs a newer RustyPresenter") };
    match s {
        MacroStep::ShowItem { target: ItemTarget::Titled { title }, .. } => with(StepKind::ShowItem, title.clone()),
        MacroStep::ShowItem { target: ItemTarget::Number { n }, .. } => with(StepKind::ShowItem, crate::tr!("item {n}", n = n)),
        MacroStep::Next => kind_label(StepKind::Next),
        MacroStep::Prev => kind_label(StepKind::Prev),
        MacroStep::Clear { which } => kind_label(StepKind::Clear(*which)),
        MacroStep::Logo => kind_label(StepKind::Logo),
        MacroStep::Overlay { id, on } => {
            let name = data.overlays.iter().find(|o| o.id == *id).map_or_else(|| "?".to_string(), |o| o.name.clone());
            with(if *on { StepKind::OverlayOn } else { StepKind::OverlayOff }, name)
        }
        MacroStep::TimerStart { timer: t } => with(StepKind::TimerStart, timer(t)),
        MacroStep::TimerPause { timer: t } => with(StepKind::TimerPause, timer(t)),
        MacroStep::TimerReset { timer: t } => with(StepKind::TimerReset, timer(t)),
        MacroStep::TimerRestart { timer: t } => with(StepKind::TimerRestart, timer(t)),
        MacroStep::TimerShow { timer: t } => with(StepKind::TimerShow, timer(t)),
        MacroStep::TimerHide { timer: t } => with(StepKind::TimerHide, timer(t)),
        MacroStep::MessageShow { template, values } => {
            let t = a.messages.iter().find(|t| t.id == *template);
            with(StepKind::MessageShow, t.map_or_else(|| "?".to_string(), |t| messages::preview(&t.text, values)))
        }
        MacroStep::MessageHide => kind_label(StepKind::MessageHide),
        MacroStep::AutoAdvance { on } => kind_label(if *on { StepKind::AutoOn } else { StepKind::AutoOff }),
        MacroStep::StageMessageShow { text } => with(StepKind::StageMessage, text.clone()),
        MacroStep::StageMessageHide => kind_label(StepKind::StageMessageHide),
        MacroStep::ScreenView { .. } => crate::tr!("Change what a screen shows"),
        MacroStep::StageLayout { .. } => crate::tr!("Change the stage layout"),
        MacroStep::StageLayoutAuto => crate::tr!("Let the stage layout follow the slides"),
        MacroStep::Music { play } => kind_label(if *play { StepKind::MusicPlay } else { StepKind::MusicStop }),
    }
}

/// Builds the step the Add step row describes; `None` when its list is empty.
pub fn build_step(k: StepKind, target: Option<(Uuid, String)>, text: &str) -> Option<MacroStep> {
    let id = target.as_ref().map(|t| t.0);
    Some(match k {
        StepKind::ShowItem => MacroStep::ShowItem { target: ItemTarget::Titled { title: target?.1 }, slide: 0 },
        StepKind::Next => MacroStep::Next,
        StepKind::Prev => MacroStep::Prev,
        StepKind::Clear(which) => MacroStep::Clear { which },
        StepKind::Logo => MacroStep::Logo,
        StepKind::TimerStart => MacroStep::TimerStart { timer: id? },
        StepKind::TimerPause => MacroStep::TimerPause { timer: id? },
        StepKind::TimerReset => MacroStep::TimerReset { timer: id? },
        StepKind::TimerRestart => MacroStep::TimerRestart { timer: id? },
        StepKind::TimerShow => MacroStep::TimerShow { timer: id? },
        StepKind::TimerHide => MacroStep::TimerHide { timer: id? },
        StepKind::MessageShow => MacroStep::MessageShow { template: id?, values: parse_values(text) },
        StepKind::MessageHide => MacroStep::MessageHide,
        StepKind::OverlayOn => MacroStep::Overlay { id: id?, on: true },
        StepKind::OverlayOff => MacroStep::Overlay { id: id?, on: false },
        StepKind::AutoOff => MacroStep::AutoAdvance { on: false },
        StepKind::AutoOn => MacroStep::AutoAdvance { on: true },
        StepKind::StageMessage => MacroStep::StageMessageShow { text: text.trim().to_string() },
        StepKind::StageMessageHide => MacroStep::StageMessageHide,
        StepKind::MusicPlay => MacroStep::Music { play: true },
        StepKind::MusicStop => MacroStep::Music { play: false },
    })
}

/// What the open dialog keeps between callbacks.
#[derive(Default)]
struct State {
    /// The ids of the list's rows, in order.
    ids: Vec<Uuid>,
    selected: Option<Uuid>,
    /// The macro being edited, its steps.
    steps: Vec<Lenient<MacroStep>>,
    /// The Add step row's second list.
    targets: Vec<(Uuid, String)>,
    /// Pre-service: the titles behind the two lists (index 0 is "None").
    loops: Vec<String>,
    music: Vec<String>,
    /// The at-zero list's macros (after "Do nothing" and "Hide it from the screen").
    at_zero: Vec<Uuid>,
    /// Calendar: the "What to do" list's choices, and the days picked.
    cal_what: Vec<CalWhat>,
    cal_days: u8,
}

/// One choice of the calendar's "What to do" list: a macro, or one step.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CalWhat { Macro(Uuid), Step(StepKind) }

/// The days' short names, Sunday first (bit 0 of `CalendarEntry::days`).
pub fn day_names() -> [String; 7] {
    [crate::tr!("Sun"), crate::tr!("Mon"), crate::tr!("Tue"), crate::tr!("Wed"), crate::tr!("Thu"), crate::tr!("Fri"), crate::tr!("Sat")]
}

/// "Every day", "Weekdays" or the days' names: "Sun, Wed".
pub fn days_label(days: u8) -> String {
    match days & EVERY_DAY {
        EVERY_DAY => crate::tr!("Every day"),
        0b011_1110 => crate::tr!("Weekdays"),
        d => day_names().into_iter().enumerate().filter(|(i, _)| d & (1 << i) != 0).map(|(_, n)| n).collect::<Vec<_>>().join(", "),
    }
}

/// A calendar entry's line in the list: when it runs and what it does.
fn calendar_sub(e: &CalendarEntry, d: &Data) -> String {
    let what = match e.run.known() {
        Some(CalendarRun::Macro { macro_id }) => d.automation.macros.iter().find(|m| m.id == *macro_id).map_or_else(|| "?".to_string(), |m| m.name.clone()),
        Some(CalendarRun::Step { step }) => step_label(&Lenient::Known(step.clone()), &d.automation, d),
        None => crate::tr!("Needs a newer RustyPresenter"),
    };
    let when = crate::tr!("{days} at {time}", days = days_label(e.days), time = e.at.label());
    if e.on { format!("{when} · {what}") } else { format!("{} · {when} · {what}", crate::tr!("Off")) }
}

type St = Rc<RefCell<State>>;

fn set_strings(list: Vec<String>) -> ModelRc<SharedString> { ModelRc::new(VecModel::from(list.into_iter().map(SharedString::from).collect::<Vec<_>>())) }

/// Opens the dialog on the Timers tab.
pub fn open(op: &Rc<Operator>) {
    op.open_dialog(DialogKind::Automation);
    let g = op.ui.global::<AutomationDlg>();
    g.set_tab(0);
    g.set_error("".into());
    g.set_note("".into());
    let st: St = Rc::default();
    connect(op, &st);
    watching(op, true);
    op.on_dialog_close(|op| watching(op, false));
    load(op, &st);
}

/// Shows `tab` with row `row` picked (-1: none). For pictures and tests.
pub fn goto(op: &Operator, tab: i32, row: i32) {
    let g = op.ui.global::<AutomationDlg>();
    if g.get_tab() != tab { g.set_tab(tab); g.invoke_tab_changed(tab); }
    if row >= 0 { g.invoke_pick(row) }
}

/// While the dialog is open, a stopped countdown to a time still shows its time left changing.
fn watching(op: &Rc<Operator>, on: bool) { op.ops().call::<Value>("automation/watching", json!({ "on": on }), |_| {}); }

fn connect(op: &Rc<Operator>, st: &St) {
    let g = op.ui.global::<AutomationDlg>();
    let with = |f: fn(&Rc<Operator>, &St)| {
        let (me, st): (Weak<Operator>, St) = (Rc::downgrade(op), st.clone());
        move || if let Some(op) = me.upgrade() { f(&op, &st) }
    };
    g.on_tab_changed({ let f = with(|op, st| { st.borrow_mut().selected = None; clear_messages(op); load(op, st) }); move |_| f() });
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_pick(move |i| if let Some(op) = me.upgrade() {
        let id = s.borrow().ids.get(i.max(0) as usize).copied();
        s.borrow_mut().selected = id;
        clear_messages(&op);
        load(&op, &s);
    });
    g.on_new_one(with(|op, st| { st.borrow_mut().selected = None; clear_messages(op); new_form(op, st) }));
    g.on_save(with(save));
    g.on_delete(with(delete));
    g.on_timer_start(with(|op, st| {
        let running = op.ui.global::<AutomationDlg>().get_t_running();
        timer_action(op, st, if running { "timer/pause" } else { "timer/start" })
    }));
    g.on_timer_reset(with(|op, st| timer_action(op, st, "timer/reset")));
    g.on_starts_at_save(with(starts_at_save));
    g.on_message_text_edited(with(|op, _| message_fields(op)));
    g.on_step_kind_changed(with(step_kind_changed));
    g.on_step_add(with(step_add));
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_step_move(move |i, by| if let Some(op) = me.upgrade() {
        let (i, j) = (i.max(0) as usize, i + by);
        let mut state = s.borrow_mut();
        if j >= 0 && (j as usize) < state.steps.len() && i < state.steps.len() { state.steps.swap(i, j as usize) }
        drop(state);
        show_steps(&op, &s);
    });
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_step_remove(move |i| if let Some(op) = me.upgrade() {
        let i = i.max(0) as usize;
        if i < s.borrow().steps.len() { s.borrow_mut().steps.remove(i); }
        show_steps(&op, &s);
    });
    g.on_preservice_save(with(preservice_save));
    let (me, s) = (Rc::downgrade(op), st.clone());
    g.on_c_day_toggle(move |i| if let Some(op) = me.upgrade() && (0..7).contains(&i) {
        s.borrow_mut().cal_days ^= 1 << i;
        show_days(&op, &s);
    });
    g.on_c_what_changed(with(|op, st| calendar_targets(op, st, &data(op), None)));
    g.on_calendar_run(with(|op, st| {
        let Some(id) = st.borrow().selected else { return };
        let me = Rc::downgrade(op);
        op.ops().call::<Value>("automation/calendar/run", json!({ "id": id }), move |r| {
            let Some(op) = me.upgrade() else { return };
            let g = op.ui.global::<AutomationDlg>();
            match r {
                Ok(v) => { g.set_error("".into()); g.set_note(v["toast"].as_str().unwrap_or_default().into()) }
                Err(e) => { g.set_note("".into()); g.set_error(e.into()) }
            }
        });
    }));
}

fn clear_messages(op: &Operator) {
    let g = op.ui.global::<AutomationDlg>();
    g.set_error("".into());
    g.set_note("".into());
}

fn data(op: &Operator) -> Data { op.app().data.lock().unwrap().clone() }

fn timer_sub(t: &TimerDef) -> String {
    match t.kind.known() {
        Some(TimerKind::Duration { secs }) => crate::tr!("Count down {time}", time = crate::timers::text(i64::from(*secs) * 1000)),
        Some(TimerKind::UntilTime { target: TimeTarget::ServiceStart }) => crate::tr!("Count down to the service start"),
        Some(TimerKind::UntilTime { target: TimeTarget::Fixed { at } }) => crate::tr!("Count down to {time}", time = at.label()),
        Some(TimerKind::CountUp) => crate::tr!("Count up"),
        Some(TimerKind::VideoLeft) => crate::tr!("Time left in the video"),
        Some(TimerKind::AudioLeft) => crate::tr!("Time left in the audio"),
        None => crate::tr!("Needs a newer RustyPresenter"),
    }
}

/// Fills the list of the open tab, and the form of the chosen row.
fn load(op: &Rc<Operator>, st: &St) {
    let g = op.ui.global::<AutomationDlg>();
    let d = data(op);
    let a = &d.automation;
    g.set_starts_at(crate::automation::actions::starts_at(&d).map(|t| format!("{:02}:{:02}", t.hour, t.minute)).unwrap_or_default().into());
    g.set_k_locked(false);
    let rows: Vec<(Uuid, AutoListRow)> = match g.get_tab() {
        0 => a.timers.iter().map(|t| (t.id, AutoListRow { id: t.id.to_string().into(), title: t.name.as_str().into(), sub: timer_sub(t).into(), readout: "".into(), over: false, locked: false })).collect(),
        1 => a.messages.iter().map(|t| (t.id, AutoListRow { id: t.id.to_string().into(), title: t.name.as_str().into(), sub: t.text.as_str().into(), readout: "".into(), over: false, locked: false })).collect(),
        2 => a.macros.iter().map(|m| (m.id, AutoListRow {
            id: m.id.to_string().into(), title: m.name.as_str().into(),
            sub: crate::trn!("{n} step", "{n} steps", m.steps.len()).into(), readout: "".into(), over: false, locked: m.id == PRE_SERVICE_MACRO_ID,
        })).collect(),
        CALENDAR_TAB => a.calendar.iter().map(|e| (e.id, AutoListRow {
            id: e.id.to_string().into(), title: e.name.as_str().into(), sub: calendar_sub(e, &d).into(), readout: "".into(), over: false, locked: false,
        })).collect(),
        _ => vec![],
    };
    let selected = st.borrow().selected.filter(|id| rows.iter().any(|(r, _)| r == id));
    st.borrow_mut().ids = rows.iter().map(|(id, _)| *id).collect();
    g.set_selected(selected.and_then(|id| rows.iter().position(|(r, _)| *r == id)).map_or(-1, |i| i as i32));
    g.set_rows(ModelRc::new(VecModel::from(rows.into_iter().map(|(_, r)| r).collect::<Vec<_>>())));
    g.set_editing(selected.is_some());
    match (g.get_tab(), selected) {
        (0, Some(id)) => if let Some(t) = a.timers.iter().find(|t| t.id == id) { timer_form(op, st, t, &d) },
        (1, Some(id)) => if let Some(t) = a.messages.iter().find(|t| t.id == id) { message_form(op, t) },
        (2, Some(id)) => if let Some(m) = a.macros.iter().find(|m| m.id == id) { macro_form(op, st, m, &d) },
        (3, _) => preservice_form(op, st, &d),
        (CALENDAR_TAB, Some(id)) => if let Some(e) = a.calendar.iter().find(|e| e.id == id) { calendar_form(op, st, e, &d) },
        _ => {}
    }
    readouts(op);
}

/// A new, empty form for the open tab.
fn new_form(op: &Rc<Operator>, st: &St) {
    let g = op.ui.global::<AutomationDlg>();
    let d = data(op);
    g.set_selected(-1);
    g.set_editing(true);
    match g.get_tab() {
        0 => timer_form(op, st, &TimerDef { kind: Lenient::Known(TimerKind::Duration { secs: 300 }), ..TimerDef::default() }, &d),
        1 => message_form(op, &MessageTemplate::default()),
        2 => macro_form(op, st, &Macro::default(), &d),
        CALENDAR_TAB => {
            // A new entry starts on the first macro, or else on the walk-in music, on Sundays at 10:45.
            let run = match d.automation.macros.first() {
                Some(m) => CalendarRun::Macro { macro_id: m.id },
                None => CalendarRun::Step { step: MacroStep::Music { play: true } },
            };
            calendar_form(op, st, &CalendarEntry { run: Lenient::Known(run), ..CalendarEntry::default() }, &d)
        }
        _ => {}
    }
}

fn calendar_form(op: &Operator, st: &St, e: &CalendarEntry, d: &Data) {
    let g = op.ui.global::<AutomationDlg>();
    g.set_c_name(e.name.as_str().into());
    g.set_c_at(format!("{:02}:{:02}", e.at.hour, e.at.minute).into());
    g.set_c_on(e.on);
    g.set_c_day_names(set_strings(day_names().to_vec()));
    let mut what: Vec<CalWhat> = d.automation.macros.iter().map(|m| CalWhat::Macro(m.id)).collect();
    what.extend(STEP_KINDS.iter().map(|k| CalWhat::Step(*k)));
    let labels: Vec<String> = what.iter().map(|w| match w {
        CalWhat::Macro(id) => crate::tr!("Run “{name}”", name = d.automation.macros.iter().find(|m| m.id == *id).map(|m| m.name.clone()).unwrap_or_default()),
        CalWhat::Step(k) => kind_label(*k),
    }).collect();
    g.set_c_what_options(set_strings(labels));
    let (chosen, step) = match e.run.known() {
        Some(CalendarRun::Macro { macro_id }) => (what.iter().position(|w| *w == CalWhat::Macro(*macro_id)), None),
        Some(CalendarRun::Step { step }) => (what.iter().position(|w| matches!(w, CalWhat::Step(k) if step_kind_of(step) == Some(*k))), Some(step.clone())),
        None => (None, None),
    };
    g.set_c_what(chosen.unwrap_or(0) as i32);
    {
        let mut s = st.borrow_mut();
        s.cal_what = what;
        s.cal_days = e.days;
    }
    calendar_targets(op, st, d, step.as_ref());
    show_days(op, st);
    let next = if e.id.is_nil() { None } else { next_due(e, chrono::Utc::now(), &chrono::Local) };
    g.set_c_next(next.map(|t| next_label(t.with_timezone(&chrono::Local))).unwrap_or_default().into());
}

/// "Next: Sun Jun 7, 10:45 AM"-style line, with the app's own day names.
pub fn next_label<Tz: chrono::TimeZone>(t: chrono::DateTime<Tz>) -> String {
    use chrono::{Datelike, Timelike};
    let day = day_names()[t.weekday().num_days_from_sunday() as usize].clone();
    let time = TimeOfDay::new(t.hour() as u8, t.minute() as u8).label();
    crate::tr!("Next: {day} {date}, {time}", day = day, date = format!("{}/{}", t.month(), t.day()), time = time)
}

/// The step kind a saved step was made with, to pick it again in the list.
fn step_kind_of(step: &MacroStep) -> Option<StepKind> {
    STEP_KINDS.iter().copied().find(|k| {
        let probe = build_step(*k, Some((Uuid::nil(), String::new())), "");
        probe.is_some_and(|p| std::mem::discriminant(&p) == std::mem::discriminant(step) && match (&p, step) {
            (MacroStep::Clear { which: a }, MacroStep::Clear { which: b }) => a == b,
            (MacroStep::Overlay { on: a, .. }, MacroStep::Overlay { on: b, .. }) => a == b,
            (MacroStep::AutoAdvance { on: a }, MacroStep::AutoAdvance { on: b }) => a == b,
            (MacroStep::Music { play: a }, MacroStep::Music { play: b }) => a == b,
            _ => true,
        })
    })
}

/// The chosen calendar step's second list (and text box), picking the saved target when there is one.
fn calendar_targets(op: &Operator, st: &St, d: &Data, step: Option<&MacroStep>) {
    let g = op.ui.global::<AutomationDlg>();
    let kind = match st.borrow().cal_what.get(g.get_c_what().max(0) as usize) { Some(CalWhat::Step(k)) => Some(*k), _ => None };
    let t = kind.map(|k| targets(k, d)).unwrap_or_default();
    let saved = step.and_then(|s| match s {
        MacroStep::ShowItem { target: ItemTarget::Titled { title }, .. } => t.iter().position(|(_, n)| n == title),
        MacroStep::TimerStart { timer } | MacroStep::TimerPause { timer } | MacroStep::TimerReset { timer } | MacroStep::TimerRestart { timer }
        | MacroStep::TimerShow { timer } | MacroStep::TimerHide { timer } => t.iter().position(|(id, _)| id == timer),
        MacroStep::MessageShow { template, .. } => t.iter().position(|(id, _)| id == template),
        MacroStep::Overlay { id, .. } => t.iter().position(|(x, _)| x == id),
        _ => None,
    });
    g.set_step_targets(set_strings(t.iter().map(|(_, n)| n.clone()).collect()));
    g.set_step_target(saved.unwrap_or(0) as i32);
    g.set_step_needs_text(matches!(kind, Some(StepKind::MessageShow | StepKind::StageMessage)));
    g.set_step_text(match step {
        Some(MacroStep::StageMessageShow { text }) => text.as_str().into(),
        Some(MacroStep::MessageShow { values, .. }) => values.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", ").into(),
        _ => "".into(),
    });
    st.borrow_mut().targets = t;
}

fn show_days(op: &Operator, st: &St) {
    let days = st.borrow().cal_days;
    let on: Vec<bool> = (0..7).map(|i| days & (1 << i) != 0).collect();
    op.ui.global::<AutomationDlg>().set_c_days(ModelRc::new(VecModel::from(on)));
}

fn calendar_from_form(op: &Operator, st: &St) -> Result<CalendarEntry, String> {
    let g = op.ui.global::<AutomationDlg>();
    let at = TimeOfDay::parse(&g.get_c_at()).ok_or_else(|| crate::tr!("Type the time like 10:30."))?;
    let s = st.borrow();
    let run = match s.cal_what.get(g.get_c_what().max(0) as usize) {
        Some(CalWhat::Macro(id)) => CalendarRun::Macro { macro_id: *id },
        Some(CalWhat::Step(k)) => {
            let target = s.targets.get(g.get_step_target().max(0) as usize).cloned();
            CalendarRun::Step { step: build_step(*k, target, &g.get_step_text()).ok_or_else(crate::automation::strings::calendar_pick_what)? }
        }
        None => return Err(crate::automation::strings::calendar_pick_what()),
    };
    Ok(CalendarEntry { id: s.selected.unwrap_or_default(), name: g.get_c_name().to_string(), at, days: s.cal_days, run: Lenient::Known(run), on: g.get_c_on() })
}

fn timer_form(op: &Operator, st: &St, t: &TimerDef, d: &Data) {
    let g = op.ui.global::<AutomationDlg>();
    g.set_t_name(t.name.as_str().into());
    let (kind, service, at, secs) = match t.kind.known() {
        Some(TimerKind::UntilTime { target: TimeTarget::ServiceStart }) => (0, true, String::new(), 0),
        Some(TimerKind::UntilTime { target: TimeTarget::Fixed { at } }) => (0, false, format!("{:02}:{:02}", at.hour, at.minute), 0),
        Some(TimerKind::Duration { secs }) => (1, true, String::new(), *secs),
        Some(TimerKind::VideoLeft) => (3, true, String::new(), 0),
        Some(TimerKind::AudioLeft) => (4, true, String::new(), 0),
        _ => (2, true, String::new(), 0),
    };
    g.set_t_kind(kind);
    g.set_t_service_start(service);
    g.set_t_at(at.into());
    g.set_t_minutes((secs / 60).to_string().into());
    g.set_t_seconds(format!("{:02}", secs % 60).into());
    g.set_t_keep_counting(t.overrun == Overrun::CountUp);
    g.set_t_place(PLACES.iter().position(|p| *p == t.place).unwrap_or(0) as i32);
    g.set_t_caption(t.caption.as_str().into());
    g.set_t_on_stage(t.on_stage);
    g.set_t_on_phone(t.on_remote);
    // Only macros that show or hide things may run at zero (spec §5.4).
    let ok: Vec<&Macro> = d.automation.macros.iter().filter(|m| m.is_overlay_only()).collect();
    let mut options = vec![crate::tr!("Do nothing"), crate::tr!("Hide it from the screen")];
    options.extend(ok.iter().map(|m| crate::tr!("Run “{name}”", name = m.name)));
    g.set_t_at_zero_options(set_strings(options));
    st.borrow_mut().at_zero = ok.iter().map(|m| m.id).collect();
    g.set_t_at_zero(match t.at_zero.known() {
        Some(AtZero::HideFromRoom) => 1,
        Some(AtZero::RunMacro { macro_id }) => ok.iter().position(|m| m.id == *macro_id).map_or(0, |i| i as i32 + 2),
        _ => 0,
    });
}

fn message_form(op: &Operator, t: &MessageTemplate) {
    let g = op.ui.global::<AutomationDlg>();
    g.set_m_name(t.name.as_str().into());
    g.set_m_text(t.text.as_str().into());
    g.set_m_place(i32::from(t.place == MessagePlace::Top));
    g.set_m_hide(HIDE_AFTER_CHOICES.iter().position(|c| *c == t.hide_after_secs).unwrap_or(0) as i32);
    g.set_m_phone(t.remote);
    message_fields(op);
}

/// The boxes the text makes, or the stray-brace warning.
fn message_fields(op: &Operator) {
    let g = op.ui.global::<AutomationDlg>();
    let p = messages::parse(&g.get_m_text());
    let line = match (p.warning, p.fields.is_empty()) {
        (Some(w), _) => w,
        (None, true) => String::new(),
        (None, false) => crate::tr!("Boxes to fill in: {fields}", fields = p.fields.join(", ")),
    };
    g.set_m_fields(line.into());
}

fn macro_form(op: &Operator, st: &St, m: &Macro, d: &Data) {
    let g = op.ui.global::<AutomationDlg>();
    g.set_k_name(m.name.as_str().into());
    g.set_k_color(COLORS.iter().position(|c| *c == m.color).unwrap_or(0) as i32);
    g.set_k_phone(m.remote);
    g.set_k_locked(m.id == PRE_SERVICE_MACRO_ID);
    st.borrow_mut().steps = m.steps.clone();
    g.set_step_kinds(set_strings(STEP_KINDS.iter().map(|k| kind_label(*k)).collect()));
    g.set_step_kind(0);
    g.set_step_text("".into());
    show_steps_with(op, st, d);
    targets_for(op, st, d);
}

fn show_steps(op: &Operator, st: &St) { show_steps_with(op, st, &data(op)) }

fn show_steps_with(op: &Operator, st: &St, d: &Data) {
    let rows: Vec<AutoStepRow> = st.borrow().steps.iter().enumerate().map(|(i, s)| AutoStepRow { label: format!("{}. {}", i + 1, step_label(s, &d.automation, d)).into() }).collect();
    op.ui.global::<AutomationDlg>().set_k_steps(ModelRc::new(VecModel::from(rows)));
}

fn step_kind(op: &Operator) -> StepKind { STEP_KINDS.get(op.ui.global::<AutomationDlg>().get_step_kind().max(0) as usize).copied().unwrap_or(StepKind::Next) }

fn targets_for(op: &Operator, st: &St, d: &Data) {
    let g = op.ui.global::<AutomationDlg>();
    let k = step_kind(op);
    let t = targets(k, d);
    g.set_step_targets(set_strings(t.iter().map(|(_, n)| n.clone()).collect()));
    g.set_step_target(0);
    g.set_step_needs_text(matches!(k, StepKind::MessageShow | StepKind::StageMessage));
    st.borrow_mut().targets = t;
}

fn step_kind_changed(op: &Rc<Operator>, st: &St) { targets_for(op, st, &data(op)) }

fn step_add(op: &Rc<Operator>, st: &St) {
    let g = op.ui.global::<AutomationDlg>();
    let target = st.borrow().targets.get(g.get_step_target().max(0) as usize).cloned();
    let Some(step) = build_step(step_kind(op), target, &g.get_step_text()) else { return };
    st.borrow_mut().steps.push(Lenient::Known(step));
    g.set_step_text("".into());
    show_steps(op, st);
}

fn preservice_form(op: &Operator, st: &St, d: &Data) {
    let g = op.ui.global::<AutomationDlg>();
    let p = d.automation.pre_service.clone().unwrap_or_default();
    let items = service_items(d);
    let none = crate::tr!("None");
    let loops: Vec<String> = std::iter::once(none.clone()).chain(items.iter().filter(|(id, _, _)| d.automation.advance.contains_key(id)).map(|(_, t, _)| t.clone())).collect();
    let music: Vec<String> = std::iter::once(none).chain(items.iter().filter(|(_, _, k)| *k == "audio").map(|(_, t, _)| t.clone())).collect();
    let pick = |list: &[String], want: &Option<String>| want.as_ref().and_then(|w| list.iter().skip(1).position(|t| t.trim().eq_ignore_ascii_case(w.trim()))).map_or(0, |i| i as i32 + 1);
    g.set_p_loop(pick(&loops, &p.loop_title));
    g.set_p_music(pick(&music, &p.music_title));
    g.set_p_loop_options(set_strings(loops.clone()));
    g.set_p_music_options(set_strings(music.clone()));
    g.set_p_place(PLACES.iter().position(|x| *x == p.place).unwrap_or(0) as i32);
    g.set_p_caption(p.caption.as_str().into());
    g.set_p_end(ENDS.iter().position(|e| *e == p.end).unwrap_or(0) as i32);
    let mut s = st.borrow_mut();
    s.loops = loops;
    s.music = music;
}

/// After a save: show the note or the error, then reload the list on the saved row.
fn done(op: &Rc<Operator>, st: &St, r: Result<Value, String>) {
    let g = op.ui.global::<AutomationDlg>();
    match r {
        Ok(v) => {
            if let Some(id) = v["id"].as_str().and_then(|s| Uuid::parse_str(s).ok()) { st.borrow_mut().selected = Some(id) }
            g.set_error("".into());
            g.set_note(v["warning"].as_str().map(str::to_string).unwrap_or_else(|| crate::tr!("Saved.")).into());
            load(op, st);
        }
        Err(e) => { g.set_note("".into()); g.set_error(e.into()) }
    }
}

fn call(op: &Rc<Operator>, st: &St, action: &'static str, body: Value) {
    let (me, s) = (Rc::downgrade(op), st.clone());
    op.ops().call::<Value>(action, body, move |r| if let Some(op) = me.upgrade() && op.dialog() == Some(DialogKind::Automation) { done(&op, &s, r) });
}

fn timer_from_form(op: &Operator, st: &St) -> Result<TimerDef, String> {
    let g = op.ui.global::<AutomationDlg>();
    let kind = match g.get_t_kind() {
        0 if g.get_t_service_start() => TimerKind::UntilTime { target: TimeTarget::ServiceStart },
        0 => TimerKind::UntilTime { target: TimeTarget::Fixed { at: TimeOfDay::parse(&g.get_t_at()).ok_or_else(|| crate::tr!("Type the time like 10:30."))? } },
        1 => {
            let n = |s: SharedString| s.trim().parse::<u32>().ok();
            let (m, s) = (n(g.get_t_minutes()).unwrap_or(0), n(g.get_t_seconds()).unwrap_or(0));
            TimerKind::Duration { secs: m.saturating_mul(60).saturating_add(s) }
        }
        3 => TimerKind::VideoLeft,
        4 => TimerKind::AudioLeft,
        _ => TimerKind::CountUp,
    };
    let at_zero = match g.get_t_at_zero() {
        1 => AtZero::HideFromRoom,
        i if i >= 2 => st.borrow().at_zero.get(i as usize - 2).map_or(AtZero::Nothing, |id| AtZero::RunMacro { macro_id: *id }),
        _ => AtZero::Nothing,
    };
    let old = st.borrow().selected.and_then(|id| op.app().data.lock().unwrap().automation.timers.iter().find(|t| t.id == id).cloned()).unwrap_or_default();
    Ok(TimerDef {
        id: st.borrow().selected.unwrap_or_default(),
        name: g.get_t_name().to_string(),
        kind: Lenient::Known(kind),
        overrun: if g.get_t_keep_counting() { Overrun::CountUp } else { Overrun::Stop },
        at_zero: Lenient::Known(at_zero),
        place: PLACES.get(g.get_t_place().max(0) as usize).copied().unwrap_or_default(),
        caption: g.get_t_caption().trim().to_string(),
        on_stage: g.get_t_on_stage(),
        on_remote: g.get_t_on_phone(),
        builtin: old.builtin,
    })
}

fn save(op: &Rc<Operator>, st: &St) {
    let g = op.ui.global::<AutomationDlg>();
    let id = st.borrow().selected.unwrap_or_default();
    match g.get_tab() {
        0 => match timer_from_form(op, st) {
            Ok(t) => call(op, st, "automation/timer/save", serde_json::to_value(t).unwrap_or_default()),
            Err(e) => g.set_error(e.into()),
        },
        1 => {
            let t = MessageTemplate {
                id, name: g.get_m_name().to_string(), text: g.get_m_text().to_string(),
                place: if g.get_m_place() == 1 { MessagePlace::Top } else { MessagePlace::Bottom },
                hide_after_secs: HIDE_AFTER_CHOICES.get(g.get_m_hide().max(0) as usize).copied().flatten(),
                remote: g.get_m_phone(),
            };
            call(op, st, "automation/message/save", serde_json::to_value(t).unwrap_or_default());
        }
        2 => {
            let m = Macro {
                id, name: g.get_k_name().to_string(), color: COLORS.get(g.get_k_color().max(0) as usize).copied().unwrap_or_default(),
                steps: st.borrow().steps.clone(), remote: g.get_k_phone(), builtin: None,
            };
            call(op, st, "automation/macro/save", serde_json::to_value(m).unwrap_or_default());
        }
        CALENDAR_TAB => match calendar_from_form(op, st) {
            Ok(e) => call(op, st, "automation/calendar/save", serde_json::to_value(e).unwrap_or_default()),
            Err(e) => g.set_error(e.into()),
        },
        _ => {}
    }
}

fn delete(op: &Rc<Operator>, st: &St) {
    let g = op.ui.global::<AutomationDlg>();
    let Some(id) = st.borrow().selected else { return };
    let action = match g.get_tab() { 0 => "automation/timer/delete", 1 => "automation/message/delete", CALENDAR_TAB => "automation/calendar/delete", _ => "automation/macro/delete" };
    let (me, s) = (Rc::downgrade(op), st.clone());
    op.ops().call::<Value>(action, json!({ "id": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) if v["question"].is_string() => {
                let q = v["question"].as_str().unwrap_or_default().to_string();
                let (me2, s2) = (Rc::downgrade(&op), s.clone());
                confirm(&op, &q, &crate::tr!("Delete"), &dlg::cancel(), true, move |yes| {
                    let Some(op) = me2.upgrade() else { return };
                    if !yes { return }
                    s2.borrow_mut().selected = None;
                    call(&op, &s2, action, json!({ "id": id, "confirm": true }));
                });
            }
            Ok(_) => { s.borrow_mut().selected = None; load(&op, &s) }
            Err(e) => op.ui.global::<AutomationDlg>().set_error(e.into()),
        }
    });
}

fn timer_action(op: &Rc<Operator>, st: &St, action: &'static str) {
    let Some(id) = st.borrow().selected else { return };
    let me = Rc::downgrade(op);
    op.ops().call::<Value>(action, json!({ "id": id }), move |r| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<AutomationDlg>();
        match r { Ok(_) => g.set_error("".into()), Err(e) => g.set_error(e.into()) }
        readouts(&op);
    });
}

fn starts_at_save(op: &Rc<Operator>, st: &St) {
    let g = op.ui.global::<AutomationDlg>();
    let Some(service) = op.app().data.lock().unwrap().service().map(|s| s.id) else { return };
    let at = g.get_starts_at().trim().to_string();
    let body = json!({ "service": service, "at": if at.is_empty() { Value::Null } else { Value::String(at) } });
    let (me, s) = (Rc::downgrade(op), st.clone());
    op.ops().call::<Value>("automation/starts-at", body, move |r| {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<AutomationDlg>();
        match r {
            Ok(_) => { g.set_error("".into()); g.set_note(crate::tr!("Saved.").into()); load(&op, &s) }
            Err(e) => g.set_error(e.into()),
        }
    });
}

fn preservice_save(op: &Rc<Operator>, st: &St) {
    let g = op.ui.global::<AutomationDlg>();
    let s = st.borrow();
    let title = |list: &[String], i: i32| (i > 0).then(|| list.get(i as usize).cloned()).flatten();
    let p = PreService {
        loop_title: title(&s.loops, g.get_p_loop()),
        music_title: title(&s.music, g.get_p_music()),
        place: PLACES.get(g.get_p_place().max(0) as usize).copied().unwrap_or_default(),
        caption: g.get_p_caption().trim().to_string(),
        end: ENDS.get(g.get_p_end().max(0) as usize).copied().unwrap_or_default(),
    };
    drop(s);
    call(op, st, "automation/preservice/save", serde_json::to_value(p).unwrap_or_default());
}

/// The timer readouts in the list and the Start/Pause button, after each `Update::Timers`.
pub fn readouts(op: &Operator) {
    if op.dialog() != Some(DialogKind::Automation) { return }
    let g = op.ui.global::<AutomationDlg>();
    if g.get_tab() != 0 { return }
    let view = op.app().timers();
    let rows = g.get_rows();
    for i in 0..slint::Model::row_count(&rows) {
        let Some(mut r) = slint::Model::row_data(&rows, i) else { continue };
        let snap = view.timers.iter().find(|t| t.id.to_string() == r.id.as_str());
        let (text, over) = snap.map_or((String::new(), false), |t| (crate::timers::text(t.value_ms), t.overrun));
        if r.readout.as_str() != text || r.over != over {
            r.readout = text.into();
            r.over = over;
            slint::Model::set_row_data(&rows, i, r);
        }
    }
    let selected = g.get_selected();
    let running = (selected >= 0).then(|| slint::Model::row_data(&rows, selected as usize)).flatten()
        .and_then(|r| view.timers.iter().find(|t| t.id.to_string() == r.id.as_str()).map(|t| t.state == TimerState::Running))
        .unwrap_or(false);
    g.set_t_running(running);
}

#[cfg(test)]
#[path = "automation_tests.rs"]
mod tests;
