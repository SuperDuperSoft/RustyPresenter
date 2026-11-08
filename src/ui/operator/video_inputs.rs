//! N11: the media library's Video inputs tab. A camera or capture card is saved as a background (`Background::Camera`
//! in the background library), so it can sit behind the words like a video loop: Use as background puts it on the
//! selected item, and the background picker and themes list it too. The devices are looked for off the UI thread
//! (`video::inputs::list`); a name can always be typed instead.
use super::{strings as tr, window::Operator};
use crate::{
    model::{Background, BackgroundEntry},
    ui::{BinRow, VideoInputs, dialogs::ask},
    video::inputs,
};
use serde_json::{Value, json};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::{Rc, Weak};
use uuid::Uuid;

/// The tab's id in `media.slint`.
pub const TAB: &str = "inputs";

/// The "ON SCREEN" mark of a camera (the files' marks are their asset names).
pub fn on_screen_mark(device: &str) -> String { format!("input:{device}") }

/// The saved video inputs, newest first, whose names hold the search words.
pub fn shown<'a>(library: &'a [BackgroundEntry], query: &str) -> Vec<&'a BackgroundEntry> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    library.iter().rev().filter(|e| matches!(e.background, Background::Camera { .. }))
        .filter(|e| { let name = e.name.to_lowercase(); words.iter().all(|w| name.contains(w.as_str())) })
        .collect()
}

/// The name a new input is saved under: the device's own, without `/dev/` on Linux.
pub fn input_name(device: &str) -> String {
    let d = device.trim();
    d.strip_prefix("/dev/").unwrap_or(d).to_string()
}

fn device_of(e: &BackgroundEntry) -> &str {
    match &e.background { Background::Camera { device, .. } => device, _ => "" }
}

impl Operator {
    fn bin_library(&self) -> Vec<BackgroundEntry> {
        self.state.borrow().snap.as_ref().map(|s| s.data.backgrounds.clone()).unwrap_or_default()
    }

    /// The chosen video input, on the Video inputs tab.
    pub(super) fn picked_input(&self) -> Option<BackgroundEntry> {
        let id = self.bin.borrow().picked?;
        self.bin_library().into_iter().find(|e| e.id == id && matches!(e.background, Background::Camera { .. }))
    }

    /// The tiles of the Video inputs tab.
    pub(super) fn input_rows(&self, on_screen: Option<&str>) -> Vec<BinRow> {
        let library = self.bin_library();
        let (picked, query) = { let b = self.bin.borrow(); (b.picked, b.query.clone()) };
        shown(&library, &query).into_iter().map(|e| BinRow {
            id: e.id.to_string().into(), name: e.name.as_str().into(), kind: "input".into(), tag: tr_camera().into(),
            image: Default::default(), picked: picked == Some(e.id), on_screen: on_screen == Some(on_screen_mark(device_of(e)).as_str()),
        }).collect()
    }

    /// The side panel on the Video inputs tab.
    pub(super) fn render_input_pick(&self) {
        let e = self.picked_input();
        let ui = &self.ui;
        ui.set_bin_has_pick(e.is_some());
        ui.set_bin_pick_name(e.as_ref().map_or("", |e| e.name.as_str()).into());
        ui.set_bin_can_use(e.is_some());
        ui.set_bin_pick_hint(if e.is_some() { tr::bin_hint_use() } else { tr::bin_hint_none() }.into());
        ui.set_bin_can_delete_folder(false);
    }

    /// The tab was opened: look for devices the first time.
    pub(super) fn inputs_opened(self: &Rc<Self>) {
        let g = self.ui.global::<VideoInputs>();
        if g.get_found().row_count() == 0 && !g.get_searching() { self.find_inputs() }
    }

    /// Looks for cameras and capture cards on a thread of its own; the list fills in when it is done.
    pub(super) fn find_inputs(self: &Rc<Self>) {
        let g = self.ui.global::<VideoInputs>();
        if g.get_searching() { return }
        g.set_searching(true);
        let ffmpeg = self.app().youtube.ffmpeg_exe();
        let weak = self.ui.as_weak();
        let spawned = std::thread::Builder::new().name("video-inputs".into()).spawn(move || {
            let found = inputs::list(ffmpeg);
            let _ = weak.upgrade_in_event_loop(move |ui| show_found(&ui.global::<VideoInputs>(), found));
        });
        if spawned.is_err() { g.set_searching(false) }
    }

    /// Saves the device picked or typed as a background, and chooses it.
    pub(super) fn add_input(self: &Rc<Self>) {
        let g = self.ui.global::<VideoInputs>();
        let typed = g.get_typed().trim().to_string();
        let device = if typed.is_empty() { g.get_found().row_data(g.get_found_index().max(0) as usize).map(|s| s.to_string()).unwrap_or_default() } else { typed };
        if device.is_empty() { return self.toast(crate::tr!("Pick a camera or type its name first."), false) }
        let name = input_name(&device);
        let me = Rc::downgrade(self);
        let body = json!({ "name": name, "background": Background::Camera { device, dim: 0 } });
        self.ops.call::<Value>("background/add", body, move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(v) => {
                    op.ui.global::<VideoInputs>().set_typed(SharedString::new());
                    // Chosen now; its tile shows once the new library arrives.
                    op.bin.borrow_mut().picked = v["id"].as_str().and_then(|id| id.parse::<Uuid>().ok());
                    op.render_media();
                    op.toast(crate::tr!("Added “{name}”.", name = name), false);
                }
                Err(e) => op.toast(e, true),
            }
        });
    }

    /// Takes the chosen input out of the library (items already using it keep it).
    pub(super) fn remove_input(self: &Rc<Self>) {
        let Some(e) = self.picked_input() else { return };
        let me = Rc::downgrade(self);
        let question = crate::tr!("Remove “{name}” from the video inputs?", name = e.name);
        ask::confirm(self, &question, &crate::tr!("Remove"), &tr::cancel_action(), true, move |ok| {
            let (true, Some(op)) = (ok, me.upgrade()) else { return };
            let me = Rc::downgrade(&op);
            op.ops.call::<Value>("background/remove", json!({ "id": e.id }), move |r| {
                if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true) }
            });
        });
    }

    /// The chosen input becomes the selected service item's background.
    pub(super) fn use_input(self: &Rc<Self>) {
        let Some(e) = self.picked_input() else { return };
        let (selected, snap) = { let st = self.state.borrow(); (st.selected, st.snap.clone()) };
        let Some(mut item) = selected.and_then(|id| snap?.items.iter().find(|i| i.id == id).map(|i| i.item.clone())) else {
            return self.toast(tr::pick_item_first(), false);
        };
        item.background = Some(e.background.clone());
        let me = Rc::downgrade(self);
        self.ops.call::<Value>("service/update", json!(item), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(_) => op.toast(tr::background_set(&e.name), false),
                Err(err) => op.toast(err, true),
            }
        });
    }
}

fn tr_camera() -> String { crate::tr!("Camera") }

/// Puts what a device search found in the list.
pub(super) fn show_found(g: &VideoInputs, found: Vec<String>) {
    let names: Vec<SharedString> = found.into_iter().map(SharedString::from).collect();
    g.set_found(ModelRc::new(VecModel::from(names)));
    g.set_found_index(0);
    g.set_searching(false);
}

pub(super) fn wire(op: &Rc<Operator>) {
    let g = op.ui.global::<VideoInputs>();
    let on = |f: fn(&Rc<Operator>)| { let me: Weak<Operator> = Rc::downgrade(op); move || if let Some(op) = me.upgrade() { f(&op) } };
    g.on_find(on(Operator::find_inputs));
    g.on_add(on(Operator::add_input));
    g.on_remove(on(Operator::remove_input));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{LiveUpdate, test_app},
        model::MediaStatus,
        ui::operator::window::testing::{find, open, refresh},
    };
    use slint::platform::PointerEventButton;

    fn camera(name: &str, device: &str) -> BackgroundEntry {
        BackgroundEntry::new(name, Background::Camera { device: device.into(), dim: 0 }, MediaStatus::Ready, chrono::Utc::now())
    }

    #[test]
    fn only_cameras_show_newest_first_and_search_narrows() {
        let lib = vec![camera("Front cam", "Front"), BackgroundEntry::new("Blue", Background::Color { color: "#00f".into() }, MediaStatus::Ready, chrono::Utc::now()), camera("Stage capture", "HD60")];
        let names = |q: &str| shown(&lib, q).into_iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(""), ["Stage capture", "Front cam"]);
        assert_eq!(names("front"), ["Front cam"]);
        assert_eq!(input_name("/dev/video0"), "video0");
        assert_eq!(input_name(" Integrated Camera "), "Integrated Camera");
    }

    fn settle() { slint::platform::update_timers_and_animations(); }

    fn opened(app: &std::sync::Arc<crate::app::App>) -> Rc<Operator> {
        let op = open(app);
        op.window().set_size(slint::LogicalSize::new(1366.0, 900.0));
        settle();
        refresh(&op);
        op
    }

    fn click(op: &Operator, id: &str) {
        settle();
        find(op, id).unwrap_or_else(|| panic!("no {id}")).mock_single_click(PointerEventButton::Left);
        settle();
    }

    #[test]
    fn a_typed_camera_is_saved_chosen_and_used_as_the_background() {
        let app = test_app();
        let op = opened(&app);
        // The tab is opened by hand here: a real search would run on a thread.
        op.ui.global::<VideoInputs>().set_searching(true);
        op.ui.invoke_bin_set_tab(TAB.into());
        settle();
        assert!(find(&op, "MediaBin::inputs-empty").is_some(), "says what to do with none saved");
        show_found(&op.ui.global::<VideoInputs>(), vec!["Integrated Camera".into(), "USB Capture HDMI".into()]);
        op.ui.global::<VideoInputs>().set_found_index(1);
        settle();
        assert!(find(&op, "MediaBin::inputs-found").is_some());
        click(&op, "MediaBin::inputs-add");
        refresh(&op);
        settle();
        let lib = app.data.lock().unwrap().backgrounds.clone();
        assert_eq!(lib.last().map(|e| (e.name.clone(), e.background.clone())), Some(("USB Capture HDMI".into(), Background::Camera { device: "USB Capture HDMI".into(), dim: 0 })));
        assert_eq!(op.bin_rows.row_count(), 1);
        assert!(op.picked_input().is_some(), "the new input is chosen");
        assert!(find(&op, "MediaBin::inputs-remove").is_some());
        assert!(find(&op, "MediaBin::add-picked").is_none(), "an input is not a file to add to the service");
        // Use as background puts it on the selected item.
        let item = Uuid::new_v4();
        app.data.lock().unwrap().services[0].items.push(crate::model::ServiceItem { id: item, kind: crate::model::ItemKind::Text { title: "Welcome".into(), body: "Hi".into(), layout: Default::default() }, background: None, notes: String::new(), theme: None });
        op.state.borrow_mut().selected = Some(item);
        refresh(&op);
        click(&op, "MediaBin::use-picked");
        let bg = app.data.lock().unwrap().services[0].items.iter().find(|i| i.id == item).and_then(|i| i.background.clone());
        assert_eq!(bg, Some(Background::Camera { device: "USB Capture HDMI".into(), dim: 0 }));
        op.apply_live(LiveUpdate::new(app.live_view()));
    }

    #[test]
    fn a_typed_name_wins_and_nothing_is_refused() {
        let app = test_app();
        let op = opened(&app);
        let g = op.ui.global::<VideoInputs>();
        g.set_searching(true);
        op.ui.invoke_bin_set_tab(TAB.into());
        show_found(&g, vec![]);
        op.add_input();
        assert!(app.data.lock().unwrap().backgrounds.iter().all(|e| !matches!(e.background, Background::Camera { .. })), "nothing to add");
        assert!(op.toast_log().iter().any(|t| t.0 == "Pick a camera or type its name first."));
        g.set_typed("/dev/video2".into());
        op.add_input();
        refresh(&op);
        let lib = app.data.lock().unwrap().backgrounds.clone();
        assert_eq!(lib.last().map(|e| e.name.as_str()), Some("video2"));
        // Remove asks first.
        op.remove_input();
        settle();
        click(&op, "AskOverlay::ok");
        assert!(app.data.lock().unwrap().backgrounds.iter().all(|e| !matches!(e.background, Background::Camera { .. })));
    }
}
