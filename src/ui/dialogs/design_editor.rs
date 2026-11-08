//! The slide editor (N26, N27; `ui/dialogs/design-editor.slint`): a designed item's slides edited as a copy
//! (`design::edit::Editor`) and saved at once with `service/add` or `service/update`. The slide in the middle is
//! drawn by the room's renderer over the item's background, so it looks as it will in the room.
use super::{DialogKind, confirm};
use crate::{
    design::{
        Design, Fit, HAlign, ObjKind, Shape, TextStyle, TransitionKind, VAlign,
        edit::{AlignTo, Editor, Field, Order, StyleCopy},
        strings as s,
    },
    model::{Background, ItemKind, MediaFile, MediaKind, MediaStatus, Theme},
    render::RenderKey,
    stage::geom::{Guide, Nudge},
    store::Store,
    ui::{DesignDlg, DesignGuideVm, DesignMediaVm, DesignObjVm, DesignSlideVm, operator::window::Operator, thumbs::{self, TextRenderer, ThumbJob}},
};
use slint::{ComponentHandle, Image, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, collections::HashMap, rc::{Rc, Weak}};
use uuid::Uuid;

thread_local! {
    /// The style copied with Ctrl+Alt+C (N27): kept while the app runs, so it can go on another item's slides.
    static STYLE: RefCell<Option<StyleCopy>> = const { RefCell::new(None) };
}

/// The size the slide in the middle is drawn at, and each slide in the list.
const PICTURE: (u32, u32) = (960, 540);
const THUMB: (u32, u32) = (192, 108);
/// The pen's colour and width.
const PEN_COLOR: &str = "#ffd166";
const PEN_WIDTH: f32 = 8.0;

/// The open editor: the slides, the item they belong to (`None`: a new one), and what it draws with.
pub struct Session {
    pub ed: Editor,
    pub item: Option<Uuid>,
    pub title: String,
    stored_title: String,
    pub pen: bool,
    background: Background,
    theme: Theme,
    /// The pictures and videos of the media library that can go on a slide.
    media: Vec<MediaFile>,
    store: Store,
    renderer: TextRenderer,
    thumbs: RefCell<HashMap<String, Image>>,
}

impl Session {
    /// Something differs from what is saved.
    pub fn changed(&self) -> bool { self.ed.changed() || self.title.trim() != self.stored_title.trim() }

    /// What Save sends: the item's fields for `service/update` (with its id) or `service/add`.
    pub fn save_body(&self) -> serde_json::Value {
        let title = if self.title.trim().is_empty() { s::new_title() } else { self.title.trim().to_string() };
        let mut item = serde_json::json!({ "kind": "design", "title": title, "slides": self.ed.slides });
        match self.item {
            Some(id) => { item["id"] = serde_json::json!(id); item }
            None => serde_json::json!({ "item": item }),
        }
    }

    /// `d` over the item's background, as a picture `size` big.
    fn draw(&self, d: &Design, size: (u32, u32)) -> Image {
        let key = RenderKey::design(d, self.theme.clone(), size, self.store.assets());
        let job = ThumbJob { background: Some(self.background.clone()), media: None, picture: None, text: Some(key) };
        Image::from_rgba8(thumbs::compose(&job, size, &self.renderer, &self.store))
    }

    fn thumb(&self, d: &Design) -> Image {
        let key = serde_json::to_string(d).unwrap_or_default();
        if let Some(i) = self.thumbs.borrow().get(&key) { return i.clone() }
        let img = self.draw(d, THUMB);
        let mut t = self.thumbs.borrow_mut();
        if t.len() > 400 { t.clear() }
        t.insert(key, img.clone());
        img
    }
}

/// The pictures and videos that are ready, by name.
fn usable_media(media: &[MediaFile]) -> Vec<MediaFile> {
    let mut m: Vec<MediaFile> = media.iter().filter(|m| m.status == MediaStatus::Ready && matches!(m.kind, MediaKind::Image | MediaKind::Video)).cloned().collect();
    m.sort_by_key(|m| m.name.to_lowercase());
    m
}

/// The object an entry of the media list adds.
pub fn media_object(m: &MediaFile) -> ObjKind {
    match m.kind {
        MediaKind::Video => ObjKind::Video { asset: m.asset.clone(), thumb: m.thumb.clone(), looped: false },
        _ => ObjKind::Picture { asset: m.asset.clone(), fit: Fit::Fit },
    }
}

/// Which kind the side panel shows for an object.
fn kind_index(k: &ObjKind) -> i32 {
    match k { ObjKind::Text { .. } => 0, ObjKind::Shape { .. } => 1, ObjKind::Picture { .. } => 2, ObjKind::Video { .. } => 3, ObjKind::Path { .. } => 4, ObjKind::Web { .. } => 5, ObjKind::Unknown => 6 }
}

fn shape_of(i: i32) -> Shape { [Shape::Rect, Shape::Ellipse, Shape::Triangle, Shape::Line, Shape::Arrow][i.clamp(0, 4) as usize] }
fn shape_index(s: Shape) -> i32 { match s { Shape::Rect => 0, Shape::Ellipse => 1, Shape::Triangle => 2, Shape::Line => 3, Shape::Arrow => 4 } }

/// Puts the session into the dialog's global.
fn show(g: &DesignDlg, ss: &Session) {
    let ed = &ss.ed;
    let slides: Vec<DesignSlideVm> = ed.slides.iter().enumerate().map(|(i, d)| DesignSlideVm {
        name: s::slide_name(i + 1).into(), thumb: ss.thumb(d), builds: i32::from(d.steps()), selected: i == ed.at,
    }).collect();
    g.set_slides(ModelRc::new(VecModel::from(slides)));
    g.set_picture(ss.draw(ed.slide(), PICTURE));
    let objects: Vec<DesignObjVm> = ed.slide().objects.iter().map(|o| DesignObjVm {
        id: o.id.to_string().into(), x: o.rect.x, y: o.rect.y, w: o.rect.w, h: o.rect.h, rotation: o.rotation, selected: ed.sel.contains(&o.id),
    }).collect();
    g.set_objects(ModelRc::new(VecModel::from(objects)));
    let guides: Vec<DesignGuideVm> = ed.guides.iter().map(|g| match g {
        Guide::V(at) => DesignGuideVm { vertical: true, at: *at },
        Guide::H(at) => DesignGuideVm { vertical: false, at: *at },
    }).collect();
    g.set_guides(ModelRc::new(VecModel::from(guides)));
    g.set_can_undo(ed.can_undo());
    g.set_can_redo(ed.can_redo());
    g.set_pen(ss.pen);
    g.set_style_copied(STYLE.with(|c| c.borrow().is_some()));
    g.set_chosen(ed.sel.len() as i32);
    let d = ed.slide();
    g.set_transition(match d.transition.kind { TransitionKind::Fade => 0, TransitionKind::Cut => 1, TransitionKind::Black => 2 });
    g.set_transition_ms(d.transition.ms as i32);
    if g.get_notes().as_str() != d.notes { g.set_notes(d.notes.as_str().into()) }
    let Some(o) = ed.primary() else { return };
    g.set_kind(kind_index(&o.kind));
    match &o.kind {
        ObjKind::Text { text, style } => {
            if g.get_text().as_str() != text { g.set_text(text.as_str().into()) }
            g.set_size(style.size.round() as i32);
            if crate::render::color::is_color(&g.get_color()) || !matches!(ss.ed_last(), Some(Field::Color)) { g.set_color(style.color.as_str().into()) }
            g.set_bold(style.bold);
            g.set_italic(style.italic);
            g.set_shrink(style.shrink);
            g.set_align(match style.align { HAlign::Left => 0, HAlign::Center => 1, HAlign::Right => 2 });
            g.set_valign(match style.valign { VAlign::Top => 0, VAlign::Middle => 1, VAlign::Bottom => 2 });
        }
        ObjKind::Shape { shape } => g.set_shape(shape_index(*shape)),
        ObjKind::Picture { fit, .. } => g.set_fit(match fit { Fit::Fit => 0, Fit::Fill => 1, Fit::Stretch => 2 }),
        ObjKind::Web { url } => if g.get_text().as_str() != url { g.set_text(url.as_str().into()) },
        _ => {}
    }
    let l = &o.look;
    g.set_has_fill(l.fill.is_some());
    if let Some(f) = &l.fill && !matches!(ss.ed_last(), Some(Field::Fill)) { g.set_fill(f.as_str().into()) }
    g.set_has_outline(l.outline.is_some());
    if let Some(ol) = &l.outline {
        if !matches!(ss.ed_last(), Some(Field::OutlineColor)) { g.set_outline_color(ol.color.as_str().into()) }
        g.set_outline_width(ol.width.round() as i32);
    }
    g.set_has_shadow(l.shadow.is_some());
    if let Some(sh) = &l.shadow { g.set_shadow_blur(sh.blur.round() as i32) }
    g.set_soft(l.soft.round() as i32);
    g.set_radius(l.radius.round() as i32);
    g.set_opacity(i32::from(o.opacity));
    g.set_rotation(o.rotation.round() as i32);
    g.set_build(o.build > 0);
    let steps = d.steps();
    g.set_build_note(if o.build > 0 { crate::tr!("Appears on Next {step} of {of}", step = o.build, of = steps).into() } else { SharedString::default() });
}

impl Session {
    fn ed_last(&self) -> Option<Field> { self.ed.last_field() }
}

/// Opens the editor on the service item `item` (a designed one), or on new slides when `None`.
pub fn open(op: &Rc<Operator>, item: Option<Uuid>) {
    let Some(snap) = op.state.borrow().snap.clone() else { return };
    let data = &snap.data;
    let found = item.and_then(|id| data.service().and_then(|s| s.items.iter().find(|i| i.id == id)).cloned());
    let (slides, title) = match found.as_ref().map(|i| &i.kind) {
        Some(ItemKind::Design { title, slides }) => (slides.clone(), title.clone()),
        _ => (vec![], s::new_title()),
    };
    let look = match &found {
        Some(i) => thumbs::item_settings(data, i),
        None => data.settings.clone(),
    };
    let background = match look.background {
        Background::Video { asset, dim } => match thumbs::video_thumb(&data.media, &asset) {
            Some(thumb) => Background::Image { asset: thumb, dim },
            None => Background::Color { color: "#000000".into() },
        },
        b => b,
    };
    let item = found.map(|i| i.id);
    let ss = Session {
        ed: Editor::new(slides), item, stored_title: title.clone(), title, pen: false,
        background, theme: look.theme, media: usable_media(&data.media), store: Store { root: op.app().store.root.clone() },
        renderer: TextRenderer::new(), thumbs: RefCell::default(),
    };
    op.open_dialog(DialogKind::Design(item));
    let g = op.ui.global::<DesignDlg>();
    g.set_title(ss.title.as_str().into());
    g.set_error("".into());
    g.set_text("".into());
    g.set_notes("".into());
    // Live Edit: these slides are on screen now, so Save and show now is offered.
    g.set_live(item.is_some() && item == op.shown_live().and_then(|l| Some(l.view().cursor.as_ref()?.item_id)));
    let media: Vec<DesignMediaVm> = ss.media.iter().map(|m| DesignMediaVm { name: m.name.as_str().into(), video: m.kind == MediaKind::Video }).collect();
    g.set_media(ModelRc::new(VecModel::from(media)));
    show(&g, &ss);
    let ss = Rc::new(RefCell::new(ss));
    connect(op, &ss);
}

/// Runs `f` on the session, then redraws; an error from `f` shows as a toast.
fn with(op: &Rc<Operator>, ss: &Rc<RefCell<Session>>, f: impl FnOnce(&mut Session) -> Result<(), String>) {
    if op.dialog().is_none_or(|d| !matches!(d, DialogKind::Design(_))) { return }
    let r = f(&mut ss.borrow_mut());
    show(&op.ui.global::<DesignDlg>(), &ss.borrow());
    if let Err(e) = r { op.toast(e, true) }
}

/// Ctrl+Alt+C: the chosen object's style is kept to paste.
pub fn copy_style(ss: &Session) -> Result<String, String> {
    let style = ss.ed.copy_style().ok_or_else(s::nothing_to_copy)?;
    STYLE.with(|c| *c.borrow_mut() = Some(style));
    Ok(s::style_copied())
}

/// Ctrl+Alt+V: the copied style goes on the chosen objects (or every text box).
pub fn paste_style(ss: &mut Session) -> Result<String, String> {
    let style = STYLE.with(|c| c.borrow().clone()).ok_or_else(s::nothing_copied)?;
    ss.ed.paste_style(&style);
    Ok(s::style_pasted())
}

/// A key pressed with Ctrl on the slide.
fn command(op: &Rc<Operator>, ss: &Rc<RefCell<Session>>, key: &str, alt: bool, shift: bool) {
    let key = key.to_lowercase();
    let mut said = None;
    with(op, ss, |ss| {
        match (key.as_str(), alt) {
            ("c", true) => said = Some(copy_style(ss)),
            ("v", true) => said = Some(paste_style(ss)),
            ("z", false) if shift => ss.ed.redo(),
            ("z", false) => ss.ed.undo(),
            ("y", false) => ss.ed.redo(),
            ("d", false) => return ss.ed.duplicate(),
            ("a", false) => ss.ed.select_all(),
            _ => {}
        }
        Ok(())
    });
    match said { Some(Ok(t)) => op.toast(t, false), Some(Err(e)) => op.toast(e, true), None => {} }
}

fn connect(op: &Rc<Operator>, ss: &Rc<RefCell<Session>>) {
    let g = op.ui.global::<DesignDlg>();
    // A callback that changes the session and redraws.
    macro_rules! on {
        ($set:ident, |$e:ident $(, $a:ident : $t:ty)*| $body:expr) => {{
            let (me, ss): (Weak<Operator>, Rc<RefCell<Session>>) = (Rc::downgrade(op), ss.clone());
            g.$set(move |$($a: $t),*| if let Some(op) = me.upgrade() { with(&op, &ss, |$e| { $body; Ok(()) }) });
        }};
        ($set:ident, fallible |$e:ident $(, $a:ident : $t:ty)*| $body:expr) => {{
            let (me, ss): (Weak<Operator>, Rc<RefCell<Session>>) = (Rc::downgrade(op), ss.clone());
            g.$set(move |$($a: $t),*| if let Some(op) = me.upgrade() { with(&op, &ss, |$e| $body) });
        }};
    }
    on!(on_select_slide, |e, i: i32| e.ed.select_slide(i.max(0) as usize));
    on!(on_add_slide, fallible |e, copy: bool| e.ed.add_slide(copy));
    on!(on_move_slide, |e, up: bool| e.ed.move_slide(up));
    on!(on_delete_slide, |e| e.ed.delete_slide());
    on!(on_add_text, fallible |e| e.ed.add_text().map(|_| ()));
    on!(on_add_web, fallible |e| e.ed.add_web().map(|_| ()));
    on!(on_add_shape, fallible |e, i: i32| e.ed.add(ObjKind::Shape { shape: shape_of(i) }).map(|_| ()));
    on!(on_add_media, fallible |e, i: i32| match e.media.get(i.max(0) as usize).map(media_object) {
        Some(k) => e.ed.add(k).map(|_| ()),
        None => Ok(()),
    });
    on!(on_toggle_pen, |e| { e.pen = !e.pen; e.ed.sel.clear() });
    on!(on_press, fallible |e, x: f32, y: f32, tx: f32, ty: f32, add: bool, free: bool| {
        if e.pen { e.ed.pen_down((x, y), PEN_COLOR, PEN_WIDTH) } else { e.ed.press((x, y), (tx, ty), add, free); Ok(()) }
    });
    on!(on_drag, |e, x: f32, y: f32| if e.pen { e.ed.pen_to((x, y)) } else { e.ed.drag_to((x, y)) });
    on!(on_release, |e| if e.pen { e.ed.pen_up() } else { e.ed.release() });
    on!(on_nudge, |e, k: i32, shift: bool| e.ed.nudge([Nudge::Left, Nudge::Right, Nudge::Up, Nudge::Down][k.clamp(0, 3) as usize], shift));
    on!(on_remove, |e| e.ed.delete());
    on!(on_next, |e, back: bool| e.ed.next(back));
    on!(on_undo, |e| e.ed.undo());
    on!(on_redo, |e| e.ed.redo());
    on!(on_align_to, |e, i: i32| e.ed.align([AlignTo::Left, AlignTo::Center, AlignTo::Right, AlignTo::Top, AlignTo::Middle, AlignTo::Bottom][i.clamp(0, 5) as usize]));
    on!(on_distribute, |e, across: bool| e.ed.distribute(across));
    on!(on_order, |e, i: i32| e.ed.order([Order::Front, Order::Forward, Order::Backward, Order::Back][i.clamp(0, 3) as usize]));
    on!(on_duplicate, fallible |e| e.ed.duplicate());
    on!(on_text_edited, |e, t: SharedString| e.ed.set_text(&t));
    on!(on_size_changed, |e, v: i32| e.ed.set_size(v as f32));
    on!(on_color_changed, |e, c: SharedString| {
        let c = c.trim().to_string();
        if crate::render::color::is_color(&c) { e.ed.set_style(Field::Color, |st: &mut TextStyle| st.color = c.clone()) }
    });
    on!(on_bold_changed, |e, v: bool| e.ed.set_bold(v));
    on!(on_italic_changed, |e, v: bool| e.ed.set_italic(v));
    on!(on_shrink_changed, |e, v: bool| e.ed.set_shrink(v));
    on!(on_align_changed, |e, i: i32| e.ed.set_align([HAlign::Left, HAlign::Center, HAlign::Right][i.clamp(0, 2) as usize]));
    on!(on_valign_changed, |e, i: i32| e.ed.set_valign([VAlign::Top, VAlign::Middle, VAlign::Bottom][i.clamp(0, 2) as usize]));
    on!(on_shape_changed, |e, i: i32| e.ed.set_shape(shape_of(i)));
    on!(on_fit_changed, |e, i: i32| e.ed.set_fit([Fit::Fit, Fit::Fill, Fit::Stretch][i.clamp(0, 2) as usize]));
    let (me, ss2) = (Rc::downgrade(op), ss.clone());
    g.on_fill_on(move |v| if let Some(op) = me.upgrade() {
        let typed = op.ui.global::<DesignDlg>().get_fill().to_string();
        let f = if crate::render::color::is_color(&typed) { typed } else { "#3b82f6".to_string() };
        with(&op, &ss2, |e| { e.ed.set_fill(v.then_some(f.as_str())); Ok(()) });
    });
    on!(on_fill_changed, |e, c: SharedString| e.ed.set_fill(Some(&c)));
    on!(on_outline_on, |e, v: bool| e.ed.set_outline(v));
    on!(on_outline_color_changed, |e, c: SharedString| e.ed.set_outline_color(&c));
    on!(on_outline_width_changed, |e, v: i32| e.ed.set_outline_width(v as f32));
    on!(on_shadow_on, |e, v: bool| e.ed.set_shadow(v));
    on!(on_shadow_blur_changed, |e, v: i32| e.ed.set_shadow_blur(v as f32));
    on!(on_soft_changed, |e, v: i32| e.ed.set_soft(v as f32));
    on!(on_radius_changed, |e, v: i32| e.ed.set_radius(v as f32));
    on!(on_opacity_changed, |e, v: i32| e.ed.set_opacity(v));
    on!(on_rotation_changed, |e, v: i32| e.ed.set_rotation(v as f32));
    on!(on_build_on, |e, v: bool| e.ed.set_build(v));
    on!(on_build_move, |e, earlier: bool| e.ed.move_build(earlier));
    on!(on_transition_changed, |e, i: i32| e.ed.set_transition([TransitionKind::Fade, TransitionKind::Cut, TransitionKind::Black][i.clamp(0, 2) as usize]));
    on!(on_transition_ms_changed, |e, v: i32| e.ed.set_transition_ms(v));
    on!(on_transition_to_all, |e| e.ed.transition_to_all());
    on!(on_notes_edited, |e, t: SharedString| e.ed.set_notes(&t));
    on!(on_title_edited, |e, t: SharedString| e.title = t.to_string());
    let (me, ss2) = (Rc::downgrade(op), ss.clone());
    g.on_command(move |k, alt, shift| if let Some(op) = me.upgrade() { command(&op, &ss2, &k, alt, shift) });
    let (me, ss2) = (Rc::downgrade(op), ss.clone());
    g.on_copy_style(move || if let Some(op) = me.upgrade() { command(&op, &ss2, "c", true, false) });
    let (me, ss2) = (Rc::downgrade(op), ss.clone());
    g.on_paste_style(move || if let Some(op) = me.upgrade() { command(&op, &ss2, "v", true, false) });
    let (me, ss2) = (Rc::downgrade(op), ss.clone());
    g.on_save(move || if let Some(op) = me.upgrade() { save(&op, &ss2, false) });
    let (me, ss2) = (Rc::downgrade(op), ss.clone());
    g.on_save_and_show(move || if let Some(op) = me.upgrade() { save(&op, &ss2, true) });
    let (me, ss2) = (Rc::downgrade(op), ss.clone());
    g.on_cancel(move || if let Some(op) = me.upgrade() { cancel(&op, &ss2) });
}

/// Saves the item (a new one is added to the service) and closes. On an error the dialog stays open with it.
/// `show_now` (Live Edit): once saved, Update screen shows the new version of the live slide.
fn save(op: &Rc<Operator>, ss: &Rc<RefCell<Session>>, show_now: bool) {
    let (body, route) = { let s = ss.borrow(); (s.save_body(), if s.item.is_some() { "service/update" } else { "service/add" }) };
    let me = Rc::downgrade(op);
    op.ops().call::<serde_json::Value>(route, body, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) if show_now => { op.close_dialog(); crate::ui::dialogs::song_editor::refresh_live(&op) }
            Ok(_) => { op.close_dialog(); op.toast(s::saved(), false) }
            Err(e) => op.ui.global::<DesignDlg>().set_error(e.into()),
        }
    });
}

/// Cancel: asks first only when something changed.
fn cancel(op: &Rc<Operator>, ss: &Rc<RefCell<Session>>) {
    if !ss.borrow().changed() { return op.close_dialog() }
    let me = Rc::downgrade(op);
    confirm(op, &s::discard_question(), &s::discard(), &s::keep_editing(), true, move |ok| {
        if let Some(op) = me.upgrade().filter(|_| ok) { op.close_dialog() }
    });
}

#[cfg(test)]
#[path = "design_editor_tests.rs"]
mod tests;
