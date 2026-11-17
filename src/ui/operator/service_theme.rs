//! The service's theme box at the top of the service column (FEAT-1 spec §3.2): "Church default (<name>)", every
//! theme, then "Manage themes…" (Settings → Themes). Picking runs `service/theme`; the room is not changed. When the
//! theme has its own background and some items of the service have theirs, the operator is asked which to keep.
use super::window::Operator;
use crate::{
    model::{Data, Service},
    theme::{self, ThemeId, strings as ts},
    ui::{ServicePlanning, dialogs::{SettingsTab, choose, settings}},
};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::{Rc, Weak};

/// What each line of the box picks: `Some(None)` the church default, `Some(Some(id))` a theme, `None` Manage themes.
pub fn choices(data: &Data) -> Vec<(String, Option<Option<ThemeId>>)> {
    let mut out = vec![(ts::church_default_named(&theme::church_default(data).name), Some(None))];
    out.extend(data.catalog().rows().into_iter().map(|r| (r.name, Some(Some(r.id)))));
    out.push((ts::manage_themes(), None));
    out
}

/// The line that shows the service's theme (a theme that is gone shows the church default).
pub fn index_of(data: &Data, service: Option<&Service>) -> usize {
    let theme = service.and_then(|s| s.theme).filter(|id| theme::find(data, *id).is_some());
    choices(data).iter().position(|(_, c)| *c == Some(theme)).unwrap_or(0)
}

impl Operator {
    pub(super) fn render_service_theme(&self) {
        let st = self.state.borrow();
        let p = self.ui.global::<ServicePlanning>();
        let Some(snap) = st.snap.as_ref() else { return };
        let service = snap.service_id.and_then(|id| snap.data.services.iter().find(|s| s.id == id));
        if service.is_none() { return p.set_theme_options(ModelRc::default()) }
        let labels: Vec<slint::SharedString> = choices(&snap.data).into_iter().map(|(l, _)| l.into()).collect();
        p.set_theme_options(ModelRc::new(VecModel::from(labels)));
        p.set_theme_index(index_of(&snap.data, service) as i32);
    }

    fn pick_service_theme(self: &Rc<Self>, i: usize) {
        let Some(snap) = self.state.borrow().snap.clone() else { return };
        let Some(service_id) = snap.service_id else { return };
        let Some((_, choice)) = choices(&snap.data).into_iter().nth(i) else { return };
        let Some(theme) = choice else {
            self.render_service_theme();
            return settings::open(self, SettingsTab::Themes);
        };
        let me = Rc::downgrade(self);
        self.ops().call::<serde_json::Value>("service/theme", serde_json::json!({ "service_id": service_id, "theme": theme }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(v) => {
                    op.request_refresh();
                    let own = v.get("own_background").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
                    if own > 0 { ask_backgrounds(&op, service_id, own) }
                }
                Err(e) => {
                    op.render_service_theme();
                    op.toast(e, true);
                }
            }
        });
    }
}

/// "N items have their own background. Use the theme's background for them too?"
fn ask_backgrounds(op: &Rc<Operator>, service_id: uuid::Uuid, n: usize) {
    let me = Rc::downgrade(op);
    choose(op, &ts::own_backgrounds(n), &ts::use_theme_background(), &ts::keep_theirs(), move |a| {
        let Some(op) = me.upgrade().filter(|_| a == Some(true)) else { return };
        let me = Rc::downgrade(&op);
        op.ops().call::<serde_json::Value>("service/clear-item-backgrounds", serde_json::json!({ "service_id": service_id }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => op.request_refresh(),
                Err(e) => op.toast(e, true),
            }
        });
    });
}

pub(super) fn wire(op: &Rc<Operator>) {
    let me: Weak<Operator> = Rc::downgrade(op);
    op.ui.global::<ServicePlanning>().on_pick_theme(move |i| if let Some(op) = me.upgrade() {
        op.ui.invoke_focus_keys();
        op.pick_service_theme(i.max(0) as usize);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::test_app,
        model::{Background, ItemKind, ServiceItem},
        theme::SavedTheme,
        ui::operator::window::testing::{find, open, refresh},
    };
    use slint::Model;
    use uuid::Uuid;

    fn settle() { slint::platform::update_timers_and_animations(); }

    fn christmas(with_background: bool) -> SavedTheme {
        SavedTheme { id: Uuid::new_v4(), name: "Christmas".into(), style: Default::default(),
            background: with_background.then(|| Background::Color { color: "#3a2412".into() }) }
    }

    fn labels(op: &Operator) -> Vec<String> { op.ui.global::<ServicePlanning>().get_theme_options().iter().map(|s| s.to_string()).collect() }

    #[test]
    fn service_theme_box_lists_church_default_themes_and_manage() {
        let app = test_app();
        app.data.lock().unwrap().themes.push(christmas(false));
        let op = open(&app);
        refresh(&op);
        settle();
        let mut expect: Vec<String> = vec!["Church default (Standard)".into(), "Standard".into()];
        expect.extend(crate::theme::builtin().into_iter().map(|b| b.name));
        expect.extend(["Christmas".into(), "Manage themes…".into()]);
        assert_eq!(labels(&op), expect);
        assert_eq!(op.ui.global::<ServicePlanning>().get_theme_index(), 0);
        assert!(find(&op, "ServiceColumn::service-theme").is_some());
    }

    #[test]
    fn service_theme_picks_theme_and_manage_opens_settings() {
        let app = test_app();
        let t = christmas(false);
        app.data.lock().unwrap().themes.push(t.clone());
        let op = open(&app);
        refresh(&op);
        let at = labels(&op).iter().position(|l| l == "Christmas").unwrap() as i32;
        op.ui.global::<ServicePlanning>().invoke_pick_theme(at);
        assert_eq!(app.data.lock().unwrap().services[0].theme, Some(t.id));
        assert_eq!(op.ui.global::<ServicePlanning>().get_theme_index(), at, "the box shows it after the read");
        op.ui.global::<ServicePlanning>().invoke_pick_theme(0);
        assert_eq!(app.data.lock().unwrap().services[0].theme, None, "back to the church default");
        op.ui.global::<ServicePlanning>().invoke_pick_theme(at + 1);
        assert_eq!(crate::ui::dialogs::settings::shown(&op), Some(SettingsTab::Themes));
        assert_eq!(op.ui.global::<ServicePlanning>().get_theme_index(), 0, "Manage themes… is not a theme");
    }

    #[test]
    fn service_theme_asks_about_own_backgrounds() {
        let app = test_app();
        let t = christmas(true);
        {
            let mut d = app.data.lock().unwrap();
            d.themes.push(t.clone());
            for bg in [Some(Background::Color { color: "#ffffff".into() }), None] {
                d.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: "A".into(), body: "a".into(), layout: Default::default() }, background: bg, notes: String::new(), theme: None });
            }
        }
        let op = open(&app);
        refresh(&op);
        op.ui.global::<ServicePlanning>().invoke_pick_theme(3);
        settle();
        let ask = op.ui.get_ask().row_data(0).expect("asked");
        assert_eq!(ask.text, "1 item has its own background. Use the theme's background for it too?");
        assert_eq!((ask.ok_label.as_str(), ask.other_label.as_str()), ("Use theme background", "Keep theirs"));
        crate::ui::dialogs::ask::answer(&op, true, String::new());
        assert!(app.data.lock().unwrap().services[0].items.iter().all(|i| i.background.is_none()));
    }
}
