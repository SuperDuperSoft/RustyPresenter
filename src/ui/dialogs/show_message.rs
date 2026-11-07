//! Show message (FEAT-2 §3.3, `ui/dialogs/show-message.slint`): opens on the last message used this session, one
//! box per `{Field}`, a preview of the sentence, Show (Enter), Hide while one is up.
use super::DialogKind;
use crate::{
    automation::{messages, slots},
    live::Command,
    ui::{MessageField, ShowMessageDlg, operator::window::Operator},
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, collections::BTreeMap, rc::{Rc, Weak}};
use uuid::Uuid;

/// The open dialog's template ids (in the list's order) and what was typed into each box.
#[derive(Default)]
struct State { ids: Vec<Uuid>, values: BTreeMap<String, String> }

/// Opens Show message on the last message used, else the first.
pub fn open(op: &Rc<Operator>) {
    let (templates, start) = {
        let app = op.app();
        let data = app.data.lock().unwrap();
        let start = app.automation.lock().unwrap().show_message_template(&data.automation.messages);
        (data.automation.messages.clone(), start)
    };
    op.open_dialog(DialogKind::ShowMessage);
    let g = op.ui.global::<ShowMessageDlg>();
    let state = Rc::new(RefCell::new(State { ids: templates.iter().map(|t| t.id).collect(), values: BTreeMap::new() }));
    let names: Vec<slint::SharedString> = templates.iter().map(|t| t.name.as_str().into()).collect();
    g.set_templates(ModelRc::new(VecModel::from(names)));
    g.set_template(start.and_then(|id| templates.iter().position(|t| t.id == id)).unwrap_or(0) as i32);
    g.set_error("".into());
    // Practice mode (FEAT-6) is not built yet, so its warning never shows.
    g.set_warning("".into());
    let with = |f: fn(&Rc<Operator>, &Rc<RefCell<State>>)| {
        let (me, st): (Weak<Operator>, Rc<RefCell<State>>) = (Rc::downgrade(op), state.clone());
        move || if let Some(op) = me.upgrade() { f(&op, &st) }
    };
    g.on_template_changed(with(|op, st| { st.borrow_mut().values.clear(); fill(op, st) }));
    g.on_show(with(show));
    g.on_hide(with(|op, _| { op.command(Command::HideMessage, None); op.close_dialog() }));
    g.on_cancel(with(|op, _| op.close_dialog()));
    let (me, st) = (Rc::downgrade(op), state.clone());
    g.on_field_edited(move |i, text| {
        let Some(op) = me.upgrade() else { return };
        let name = op.ui.global::<ShowMessageDlg>().get_fields().iter().nth(i.max(0) as usize).map(|f| f.name.to_string());
        if let Some(name) = name { st.borrow_mut().values.insert(name, text.to_string()); }
        preview(&op, &st);
    });
    fill(op, &state);
}

fn template(op: &Operator, st: &State) -> Option<messages::MessageTemplate> {
    let i = op.ui.global::<ShowMessageDlg>().get_template().max(0) as usize;
    let id = *st.ids.get(i)?;
    op.app().data.lock().unwrap().automation.messages.iter().find(|t| t.id == id).cloned()
}

/// One box per field of the chosen message.
fn fill(op: &Rc<Operator>, st: &Rc<RefCell<State>>) {
    let g = op.ui.global::<ShowMessageDlg>();
    let fields = template(op, &st.borrow()).map(|t| messages::parse(&t.text).fields).unwrap_or_default();
    let values = st.borrow().values.clone();
    let rows: Vec<MessageField> = fields.iter().map(|f| MessageField { name: f.as_str().into(), value: values.get(f).cloned().unwrap_or_default().into() }).collect();
    g.set_fields(ModelRc::new(VecModel::from(rows)));
    let up = slots::shown_message(&op.app().live.lock().unwrap().frame.overlays).is_some();
    g.set_message_up(up);
    preview(op, st);
}

fn preview(op: &Operator, st: &Rc<RefCell<State>>) {
    let g = op.ui.global::<ShowMessageDlg>();
    let text = template(op, &st.borrow()).map(|t| messages::preview(&t.text, &st.borrow().values)).unwrap_or_default();
    g.set_preview(text.into());
    g.set_error("".into());
}

/// Show: refused with the reason in the dialog (an empty box, too long, a clear it hadn't seen).
fn show(op: &Rc<Operator>, st: &Rc<RefCell<State>>) {
    let Some(t) = template(op, &st.borrow()) else { return };
    let values = st.borrow().values.clone();
    if let Err(e) = messages::fill(&t.text, &values) { return op.ui.global::<ShowMessageDlg>().set_error(e.into()) }
    let seen_clear = op.app().live.lock().unwrap().clear_epoch;
    let me = Rc::downgrade(op);
    op.ops().cmd(Command::ShowMessage { template: t.id, values, seen_clear: Some(seen_clear) }, None, move |o| {
        let Some(op) = me.upgrade() else { return };
        if o.ok {
            if op.dialog() == Some(DialogKind::ShowMessage) { op.close_dialog() }
            op.toast(crate::automation::strings::message_showing(), false);
        } else if op.dialog() == Some(DialogKind::ShowMessage) {
            op.ui.global::<ShowMessageDlg>().set_error(o.error.unwrap_or_default().into());
        }
    });
}
