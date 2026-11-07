//! Settings → Video → Graphics (`video::graphics`): the choices this build can draw with, saved at once and used from
//! the next start, and the one in use now.
use super::settings::Edit;
use crate::{model::Graphics, ui::{SettingsDlg, operator::window::Operator}, video::graphics::{self, Build}};
use slint::{ModelRc, VecModel};
use std::rc::Rc;

/// The picker's choices; a saved one this build lacks shows as Auto, which is what is used then.
fn choices() -> Vec<Graphics> { graphics::available(Build::this()) }

pub(super) fn fill(g: &SettingsDlg, saved: Graphics) {
    let list = choices();
    let labels: Vec<slint::SharedString> = list.iter().map(|c| graphics::label(*c).into()).collect();
    g.set_graphics_labels(ModelRc::new(VecModel::from(labels)));
    g.set_graphics_index(list.iter().position(|c| *c == saved).unwrap_or(0) as i32);
    g.set_graphics_now(graphics::in_use(graphics::chosen().graphics).into());
}

/// A choice in the picker: saved now (`settings/graphics`), used from the next start.
pub(super) fn choose(op: &Rc<Operator>, edit: &Rc<Edit>, i: usize) {
    let Some(choice) = choices().get(i).copied() else { return };
    let (me, edit) = (Rc::downgrade(op), edit.clone());
    op.ops().call::<serde_json::Value>("settings/graphics", serde_json::json!({ "graphics": choice }), move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(v) => {
                edit.settings.borrow_mut().graphics = choice;
                if let Some(t) = v["toast"].as_str() { op.toast(t, false) }
            }
            Err(e) => op.toast(e, true),
        }
    });
}
