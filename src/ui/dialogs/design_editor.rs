//! The slide editor (N26, N27; `ui/dialogs/design-editor.slint`): a designed item's slides edited as a copy
//! (`design::edit::Editor`) and saved at once with `service/add` or `service/update`. The slide in the middle is
//! drawn by the room's renderer over the item's background, so it looks as it will in the room.
use super::{DialogKind, confirm};
use crate::{
    design::{
        AnimKind, Design, Dir, Effect, Fit, HAlign, MotionPath, ObjKind, Seg, Shape, Start, TransitionKind, VAlign,
        edit::{AlignTo, Editor, Field, Order, StyleCopy, motion_preset},
        strings as s,
    },
    model::{Background, ItemKind, MediaFile, MediaKind, MediaStatus, Theme},
    render::RenderKey,
    stage::geom::{Guide, Nudge},
    store::Store,
    ui::{DesignDlg, DesignGuideVm, DesignMediaVm, DesignObjVm, DesignSlideVm, Sprite, operator::window::Operator, room::anim::{self, SpriteImg}, thumbs::{self, TextRenderer, ThumbJob}},
};
use slint::{ComponentHandle, Image, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, collections::HashMap, rc::{Rc, Weak}};
use uuid::Uuid;

thread_local! {
    /// The style copied with Ctrl+Alt+C (N27): kept while the app runs, so it can go on another item's slides.
    static STYLE: RefCell<Option<StyleCopy>> = const { RefCell::new(None) };
    /// The objects copied with Ctrl+C or Ctrl+X, to paste on any slide while the app runs.
    static OBJECTS: RefCell<Vec<crate::design::Obj>> = const { RefCell::new(Vec::new()) };
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
    /// The animation step the Animations panel edits (an index into the slide's `anims`).
    pub anim_at: Option<usize>,
    /// Preview: the slide's animations playing in the picture.
    preview: Option<Preview>,
    /// For drawing the preview's pictures, made on first use.
    fonts: Option<crate::render::Fonts>,
}

/// The slide's click groups one after another, each with its pictures and when it starts (ms), and its timer.
struct Preview { groups: Vec<(u32, crate::design::Play, Vec<SpriteImg>)>, start: std::time::Instant, end: u32, _timer: slint::Timer }

/// The pause between clicks in the preview.
const PREVIEW_GAP_MS: u32 = 700;

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
    match k { ObjKind::Text { .. } | ObjKind::Table { .. } => 0, ObjKind::Shape { .. } => 1, ObjKind::Picture { .. } => 2, ObjKind::Video { .. } => 3, ObjKind::Path { .. } => 4, ObjKind::Web { .. } => 5, ObjKind::Unknown => 6 }
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
    g.set_transition(d.transition.kind.index());
    g.set_transition_dir(d.transition.dir.index());
    g.set_transition_ms(d.transition.ms as i32);
    if g.get_notes().as_str() != d.notes { g.set_notes(d.notes.as_str().into()) }
    show_path(g, ss);
    let Some(o) = ed.primary() else { return };
    g.set_kind(kind_index(&o.kind));
    g.set_mixed(match &o.kind {
        ObjKind::Text { paras, .. } => crate::design::rich::mixed(paras),
        ObjKind::Table { table } => table.cells.iter().any(|c| crate::design::rich::mixed(&c.paras)),
        _ => false,
    });
    g.set_is_table(matches!(o.kind, ObjKind::Table { .. }));
    let words = match &o.kind { ObjKind::Table { table } => Some((table.text(), &table.style)), ObjKind::Text { text, style, .. } => Some((text.clone(), style)), _ => None };
    match &o.kind {
        ObjKind::Text { .. } | ObjKind::Table { .. } => {
            let (text, style) = words.expect("words");
            if g.get_text().as_str() != text { g.set_text(text.as_str().into()) }
            // The marked words' look, when some are marked: the settings change those words only.
            g.set_part(ed.marked_words().is_some());
            let shown = ed.shown_look();
            let style = shown.as_ref().unwrap_or(style);
            g.set_underline(style.underline);
            g.set_strike(style.strike);
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
    show_anims(g, ss);
}

/// The motion step being edited, drawn on the slide with its handles (or nothing).
fn show_path(g: &DesignDlg, ss: &Session) {
    let view = ss.path_from().map(|(path, from)| path_view(&path, from));
    g.set_path_shown(view.is_some());
    let Some(v) = view else { return };
    g.set_path_commands(v.commands.into());
    (g.set_path_end_x(v.end.0), g.set_path_end_y(v.end.1));
    g.set_path_has_bend(v.bend.is_some());
    if let Some((x, y)) = v.bend { (g.set_path_bend_x(x), g.set_path_bend_y(y)); }
}

/// A motion path as the slide editor draws it, in % of the slide: the line (SVG commands), its end, and the middle
/// of the bend when it is one arc.
#[derive(Debug, PartialEq)]
pub(crate) struct PathView { pub commands: String, pub end: (f32, f32), pub bend: Option<(f32, f32)> }

/// `path` drawn from `from` (the middle of its object).
pub(crate) fn path_view(path: &MotionPath, from: (f32, f32)) -> PathView {
    let at = |p: (f32, f32)| (from.0 + p.0, from.1 + p.1);
    let pts: Vec<(f32, f32)> = path.points().into_iter().map(at).collect();
    let commands = pts.iter().enumerate().map(|(i, p)| format!("{} {:.2} {:.2}", if i == 0 { "M" } else { "L" }, p.0, p.1)).collect::<Vec<_>>().join(" ");
    PathView { commands, end: *pts.last().unwrap_or(&from), bend: arc(path).map(|(start, c1, c2, to)| at(cubic_mid(start, c1, c2, to))) }
}

/// The path's one curve (start, both control points, end), when it is a single arc.
fn arc(path: &MotionPath) -> Option<((f32, f32), (f32, f32), (f32, f32), (f32, f32))> {
    let (start, rest) = match path.segs.as_slice() {
        [Seg::Move { to }, rest @ ..] => (*to, rest),
        rest => ((0.0, 0.0), rest),
    };
    match rest { [Seg::Cubic { c1, c2, to }] => Some((start, *c1, *c2, *to)), _ => None }
}

fn cubic_mid(a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32)) -> (f32, f32) {
    ((a.0 + 3.0 * b.0 + 3.0 * c.0 + d.0) / 8.0, (a.1 + 3.0 * b.1 + 3.0 * c.1 + d.1) / 8.0)
}

/// The path with its end dragged to `to` (relative to the object): the whole path turns and stretches about its
/// start (on a 16:9 screen), so an arc keeps its shape. A path that ends where it starts becomes a line.
pub(crate) fn drag_path_end(path: &MotionPath, to: (f32, f32)) -> MotionPath {
    let pts = path.points();
    let (start, end) = (pts[0], *pts.last().unwrap_or(&pts[0]));
    // On screen: % across times 16, % down times 9.
    let (ox, oy) = ((end.0 - start.0) * 16.0, (end.1 - start.1) * 9.0);
    let (nx, ny) = ((to.0 - start.0) * 16.0, (to.1 - start.1) * 9.0);
    let len = ox * ox + oy * oy;
    if len < 1e-3 {
        let mut segs: Vec<Seg> = path.segs.iter().take_while(|s| matches!(s, Seg::Move { .. })).copied().take(1).collect();
        segs.push(Seg::Line { to });
        return MotionPath { segs, ease: path.ease };
    }
    // `new / old` as complex numbers: a turn and a stretch.
    let (re, im) = ((nx * ox + ny * oy) / len, (ny * ox - nx * oy) / len);
    let map = |p: (f32, f32)| {
        let (x, y) = ((p.0 - start.0) * 16.0, (p.1 - start.1) * 9.0);
        (start.0 + (re * x - im * y) / 16.0, start.1 + (im * x + re * y) / 9.0)
    };
    let segs = path.segs.iter().map(|s| match *s {
        Seg::Move { to } => Seg::Move { to: map(to) },
        Seg::Line { to } => Seg::Line { to: map(to) },
        Seg::Cubic { c1, c2, to } => Seg::Cubic { c1: map(c1), c2: map(c2), to: map(to) },
    }).collect();
    MotionPath { segs, ease: path.ease }
}

/// An arc with the middle of its bend dragged to `to` (relative to the object): both control points move so the
/// curve passes through it. Other paths are left as they are.
pub(crate) fn drag_path_bend(path: &MotionPath, to: (f32, f32)) -> MotionPath {
    let Some((start, c1, c2, end)) = arc(path) else { return path.clone() };
    let mid = cubic_mid(start, c1, c2, end);
    let d = ((to.0 - mid.0) * 4.0 / 3.0, (to.1 - mid.1) * 4.0 / 3.0);
    let mut out = path.clone();
    if let Some(Seg::Cubic { c1, c2, .. }) = out.segs.last_mut() {
        *c1 = (c1.0 + d.0, c1.1 + d.1);
        *c2 = (c2.0 + d.0, c2.1 + d.1);
    }
    out
}

const KINDS: [AnimKind; 4] = [AnimKind::Entrance, AnimKind::Emphasis, AnimKind::Exit, AnimKind::Motion];
const STARTS: [Start; 3] = [Start::OnClick, Start::WithPrevious, Start::AfterPrevious];

/// The Animations panel: the first chosen object's steps, and the one being edited.
fn show_anims(g: &DesignDlg, ss: &Session) {
    let d = ss.ed.slide();
    let mine = ss.ed.anims_of_primary();
    let rows: Vec<SharedString> = mine.iter().enumerate().map(|(n, i)| {
        let a = &d.anims[*i];
        s::anim_row(n + 1, &s::effect_name(a.kind, a.effect), &s::start_name(a.start), a.dur_ms as f32 / 1000.0).into()
    }).collect();
    g.set_anim_rows(ModelRc::new(VecModel::from(rows)));
    let Some((row, a)) = ss.anim_at.and_then(|i| Some((mine.iter().position(|x| *x == i)?, &d.anims[i]))) else { return g.set_anim_at(-1) };
    g.set_anim_at(row as i32);
    g.set_anim_kind(KINDS.iter().position(|k| *k == a.kind).unwrap_or(0) as i32);
    let effects = Effect::for_kind(a.kind);
    g.set_anim_effects(ModelRc::new(VecModel::from(effects.iter().map(|e| SharedString::from(s::effect_name(a.kind, *e))).collect::<Vec<_>>())));
    g.set_anim_effect(effects.iter().position(|e| *e == a.effect).unwrap_or(0) as i32);
    g.set_anim_start(STARTS.iter().position(|x| *x == a.start).unwrap_or(0) as i32);
    let has_dir = matches!(a.kind, AnimKind::Entrance | AnimKind::Exit) && a.effect.has_dir();
    g.set_anim_has_dir(has_dir);
    g.set_anim_dir(a.dir.unwrap_or(Dir::Down).index());
    g.set_anim_delay(a.delay_ms as i32);
    g.set_anim_dur(a.dur_ms as i32);
    let amount_kind = match (a.kind, a.effect) { (AnimKind::Emphasis, Effect::Spin) => 1, (AnimKind::Emphasis, Effect::Grow) => 2, _ => 0 };
    g.set_anim_amount_kind(amount_kind);
    g.set_anim_amount(a.amount.unwrap_or(if amount_kind == 1 { 360.0 } else { 150.0 }).round() as i32);
    let (x, y) = a.path.as_ref().map_or((0.0, 0.0), |p| p.moved());
    g.set_anim_path_x(x.round() as i32);
    g.set_anim_path_y(y.round() as i32);
}

impl Session {
    /// The step being edited, if it is still one of the first chosen object's.
    fn anim(&self) -> Option<usize> { self.anim_at.filter(|i| self.ed.anims_of_primary().contains(i)) }

    /// The motion step being edited, with the middle of its object, while the slide is not previewing.
    fn path_from(&self) -> Option<(MotionPath, (f32, f32))> {
        if self.preview.is_some() || self.pen { return None }
        let a = &self.ed.slide().anims[self.anim()?];
        let o = self.ed.primary()?;
        (a.kind == AnimKind::Motion).then(|| (a.path.clone().unwrap_or_else(|| motion_preset(0)), (o.rect.x + o.rect.w / 2.0, o.rect.y + o.rect.h / 2.0)))
    }

    /// Changes the step being edited; `field` merges a dragged slider into one undo step.
    fn set_anim(&mut self, field: Option<Field>, f: impl FnOnce(&mut crate::design::Anim)) {
        if let Some(i) = self.anim() { self.ed.set_anim(i, field, f) }
    }

    /// Preview's click groups, drawn at the picture's size.
    fn preview_groups(&mut self) -> Vec<(u32, crate::design::Play, Vec<SpriteImg>)> {
        let d = self.ed.slide().clone();
        let fonts = self.fonts.get_or_insert_with(crate::render::Fonts::new);
        let store = &self.store;
        let mut picture = |a: &str| image::ImageReader::open(store.asset_path(a)).ok()?.with_guessed_format().ok()?.decode().ok().map(|i| i.into_rgba8());
        let mut at = 0;
        (0..=d.steps()).filter_map(|g| d.play(g)).map(|play| {
            let layers = crate::render::design::render_layers(fonts, &play, &self.theme.font, PICTURE, &mut picture);
            let start = at;
            at += play.duration_ms() + PREVIEW_GAP_MS;
            (start, play, anim::Layers { canvas: PICTURE, layers }.images())
        }).collect()
    }
}

/// The preview's pictures `t_ms` in, or None once it is over.
fn preview_at(p: &Preview, t_ms: u32) -> Option<Vec<Sprite>> {
    if t_ms >= p.end { return None }
    let (start, play, imgs) = p.groups.iter().rev().find(|(s, _, _)| *s <= t_ms)?;
    Some(imgs.iter().map(|i| anim::sprite_at(i, play, (t_ms - start) as f32, (0.0, 0.0, 1.0, 1.0))).collect())
}

/// Preview: plays the slide's animations in the picture, from the slide coming up to its last click; again: stops.
fn toggle_preview(op: &Rc<Operator>, ss: &Rc<RefCell<Session>>) {
    let g = op.ui.global::<DesignDlg>();
    if ss.borrow_mut().preview.take().is_some() { return stop_preview(op, ss) }
    let groups = ss.borrow_mut().preview_groups();
    if groups.is_empty() { return }
    let end = groups.last().map_or(0, |(s, p, _)| s + p.duration_ms() + PREVIEW_GAP_MS);
    {
        let s = ss.borrow();
        let bare = Design { objects: vec![], ..s.ed.slide().clone() };
        g.set_picture(s.draw(&bare, PICTURE));
    }
    g.set_previewing(true);
    let timer = slint::Timer::default();
    let (me, weak) = (Rc::downgrade(op), Rc::downgrade(ss));
    timer.start(slint::TimerMode::Repeated, crate::ui::fade::TICK, move || {
        let (Some(op), Some(ss)) = (me.upgrade(), weak.upgrade()) else { return };
        let now = ss.borrow().preview.as_ref().map(|p| preview_at(p, p.start.elapsed().as_millis() as u32));
        match now {
            Some(Some(rows)) => op.ui.global::<DesignDlg>().set_preview_sprites(ModelRc::new(VecModel::from(rows))),
            Some(None) => { ss.borrow_mut().preview = None; stop_preview(&op, &ss) }
            None => {}
        }
    });
    let p = Preview { groups, start: std::time::Instant::now(), end, _timer: timer };
    if let Some(rows) = preview_at(&p, 0) { g.set_preview_sprites(ModelRc::new(VecModel::from(rows))) }
    ss.borrow_mut().preview = Some(p);
}

fn stop_preview(op: &Rc<Operator>, ss: &Rc<RefCell<Session>>) {
    let g = op.ui.global::<DesignDlg>();
    g.set_previewing(false);
    g.set_preview_sprites(ModelRc::default());
    show(&g, &ss.borrow());
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
        Some(i) => thumbs::item_look(data, i),
        None => thumbs::Look::of(&data.settings),
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
        renderer: TextRenderer::new(), thumbs: RefCell::default(), anim_at: None, preview: None, fonts: None,
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
    if ss.borrow_mut().preview.take().is_some() { op.ui.global::<DesignDlg>().set_previewing(false); op.ui.global::<DesignDlg>().set_preview_sprites(ModelRc::default()) }
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
            ("c", false) => { let objs = ss.ed.copy_objects(); if !objs.is_empty() { OBJECTS.with(|c| *c.borrow_mut() = objs) } }
            ("x", false) => { let objs = ss.ed.cut_objects(); if !objs.is_empty() { OBJECTS.with(|c| *c.borrow_mut() = objs) } }
            ("v", false) => return ss.ed.paste_objects(&OBJECTS.with(|c| c.borrow().clone())),
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
        if crate::render::color::is_color(&c) { e.ed.set_color(&c) }
    });
    on!(on_words_marked, |e, from: i32, to: i32| e.ed.mark_words(from.max(0) as usize, to.max(0) as usize));
    on!(on_bold_changed, |e, v: bool| e.ed.set_bold(v));
    on!(on_italic_changed, |e, v: bool| e.ed.set_italic(v));
    on!(on_underline_changed, |e, v: bool| e.ed.set_underline(v));
    on!(on_strike_changed, |e, v: bool| e.ed.set_strike(v));
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
    on!(on_anim_add, |e| e.anim_at = e.ed.add_anim().or(e.anim_at));
    on!(on_anim_remove, |e| if let Some(i) = e.anim() { e.ed.remove_anim(i); e.anim_at = None });
    on!(on_anim_move, |e, up: bool| if let Some(i) = e.anim() { e.anim_at = Some(e.ed.move_anim(i, up)) });
    on!(on_anim_pick, |e, row: i32| e.anim_at = e.ed.anims_of_primary().get(row.max(0) as usize).copied());
    on!(on_anim_kind_changed, |e, k: i32| if let Some(i) = e.anim() { e.ed.set_anim_kind(i, KINDS[k.clamp(0, 3) as usize]) });
    on!(on_anim_effect_changed, |e, n: i32| if let Some(i) = e.anim() {
        let fx = Effect::for_kind(e.ed.slide().anims[i].kind);
        e.ed.set_anim_effect(i, fx[(n.max(0) as usize).min(fx.len() - 1)]);
    });
    on!(on_anim_start_changed, |e, n: i32| e.set_anim(None, |a| a.start = STARTS[n.clamp(0, 2) as usize]));
    on!(on_anim_dir_changed, |e, n: i32| e.set_anim(None, |a| a.dir = Some(Dir::from_index(n))));
    on!(on_anim_delay_changed, |e, v: i32| e.set_anim(Some(Field::AnimDelay), |a| a.delay_ms = v.clamp(0, crate::design::anim::ANIM_MAX_MS as i32) as u32));
    on!(on_anim_dur_changed, |e, v: i32| e.set_anim(Some(Field::AnimDur), |a| a.dur_ms = v.clamp(0, crate::design::anim::ANIM_MAX_MS as i32) as u32));
    on!(on_anim_amount_changed, |e, v: i32| e.set_anim(Some(Field::AnimAmount), |a| a.amount = Some(v as f32)));
    on!(on_anim_path_changed, |e, x: i32, y: i32| e.set_anim(Some(Field::AnimPath), |a| a.path = Some(crate::design::MotionPath::line((x as f32, y as f32)))));
    on!(on_anim_preset, |e, n: i32| e.set_anim(None, |a| a.path = Some(motion_preset(n))));
    on!(on_anim_path_drag, |e, which: i32, x: f32, y: f32| if let Some((path, from)) = e.path_from() {
        let to = (x.clamp(-50.0, 150.0) - from.0, y.clamp(-50.0, 150.0) - from.1);
        let path = if which == 1 { drag_path_bend(&path, to) } else { drag_path_end(&path, to) };
        e.set_anim(Some(Field::AnimPath), |a| a.path = Some(path));
    });
    let (me, ss2) = (Rc::downgrade(op), ss.clone());
    g.on_preview(move || if let Some(op) = me.upgrade() { toggle_preview(&op, &ss2) });
    on!(on_transition_changed, |e, i: i32| e.ed.set_transition(TransitionKind::from_index(i)));
    on!(on_transition_dir_changed, |e, i: i32| e.ed.set_transition_dir(crate::design::Dir::from_index(i)));
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
