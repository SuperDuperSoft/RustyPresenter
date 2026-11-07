//! Settings → Songs → Sections and labels (N19): each kind of section's name, colour and key, more section names,
//! and the slide labels. Edits go into the open dialog's clone of `Settings` (`Edit`) and are kept by Save settings,
//! which refuses a look with a key used twice (`SectionLook::problem`). Also the copyright line's example (N36).
use super::settings::Edit;
use crate::{
    model::{Settings, Song},
    sections::{SectionKind, SectionLook, look::{AddedSection, FAMILIES, KindLook, SlideLabel, default_labels}, strings},
    ui::{AddedSectionRow, ColorFamily, SectionKindRow, SectionsDlg, SlideLabelRow, operator::window::Operator},
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::{Rc, Weak};

/// The kinds the page lists (Other has no colour or key).
pub const KINDS: [SectionKind; 8] = [
    SectionKind::Verse, SectionKind::Chorus, SectionKind::PreChorus, SectionKind::Bridge, SectionKind::Tag,
    SectionKind::Intro, SectionKind::Ending, SectionKind::Interlude,
];

/// `picking` for a label's row (a kind's row is its own index).
const LABEL_PICK: i32 = 100;
/// A label added on this page, until a colour is picked for it.
const NEW_LABEL_COLOR: &str = "#8d95a8";

fn color(hex: &str) -> slint::Color { super::services::parse_color(hex).unwrap_or_default() }

fn kind_look(l: &SectionLook, k: SectionKind) -> Option<&KindLook> { l.kinds.iter().find(|x| x.kind == k) }

fn kind_mut(l: &mut SectionLook, k: SectionKind) -> &mut KindLook {
    let at = match l.kinds.iter().position(|x| x.kind == k) {
        Some(at) => at,
        None => { l.kinds.push(KindLook { kind: k, name: String::new(), color: String::new(), key: String::new() }); l.kinds.len() - 1 }
    };
    &mut l.kinds[at]
}

fn labels_mut(l: &mut SectionLook) -> &mut Vec<SlideLabel> { l.labels.get_or_insert_with(default_labels) }

/// The name a kind goes by in this look.
fn kind_name(l: &SectionLook, k: SectionKind) -> String { l.name(k).unwrap_or_else(|| k.default_name()) }

/// Fills the page and points its callbacks at this dialog.
pub fn open(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let g = op.ui.global::<SectionsDlg>();
    g.set_picking(-1);
    g.set_new_section(SharedString::new());
    g.set_new_family(0);
    g.set_new_label(SharedString::new());
    let families: Vec<ColorFamily> = FAMILIES.iter().map(|f| ColorFamily {
        colors: ModelRc::new(VecModel::from(f.iter().map(|h| color(h)).collect::<Vec<_>>())),
        hexes: ModelRc::new(VecModel::from(f.iter().map(|h| SharedString::from(*h)).collect::<Vec<_>>())),
    }).collect();
    g.set_families(ModelRc::new(VecModel::from(families)));
    fill(op, edit);
    connect(op, edit);
}

/// The page from the edited look.
fn fill(op: &Operator, edit: &Edit) {
    let g = op.ui.global::<SectionsDlg>();
    let s = edit.settings.borrow();
    let l = &s.sections;
    let kinds: Vec<SectionKindRow> = KINDS.iter().map(|k| {
        let own = kind_look(l, *k);
        SectionKindRow {
            color: color(&l.color(*k).unwrap_or_else(|| k.default_color().into())),
            name: own.map_or("", |o| o.name.as_str()).into(),
            usual_name: k.default_name().into(),
            key: own.map_or(String::new(), |o| o.key.to_uppercase()).into(),
            usual_key: k.default_key().map_or(String::new(), |c| c.to_ascii_uppercase().to_string()).into(),
        }
    }).collect();
    g.set_kinds(ModelRc::new(VecModel::from(kinds)));
    g.set_family_names(ModelRc::new(VecModel::from(KINDS.iter().map(|k| SharedString::from(kind_name(l, *k))).collect::<Vec<_>>())));
    let added: Vec<AddedSectionRow> = l.added.iter().map(|a| AddedSectionRow {
        name: a.name.as_str().into(), family: kind_name(l, a.kind).into(), color: color(&l.color(a.kind).unwrap_or_else(|| a.kind.default_color().into())),
    }).collect();
    g.set_added(ModelRc::new(VecModel::from(added)));
    let labels: Vec<SlideLabelRow> = l.labels().iter().map(|x| SlideLabelRow { name: x.name.as_str().into(), color: color(&x.color) }).collect();
    g.set_labels(ModelRc::new(VecModel::from(labels)));
}

/// Changes the edited look; `redraw` fills the page again (not while a box is being typed in).
fn change(op: &Operator, edit: &Edit, redraw: bool, f: impl FnOnce(&mut SectionLook)) {
    f(&mut edit.settings.borrow_mut().sections);
    if redraw { fill(op, edit) }
}

fn connect(op: &Rc<Operator>, edit: &Rc<Edit>) {
    let g = op.ui.global::<SectionsDlg>();
    let ctx = |op: &Rc<Operator>, edit: &Rc<Edit>| -> (Weak<Operator>, Rc<Edit>) { (Rc::downgrade(op), edit.clone()) };
    let (me, e) = ctx(op, edit);
    g.on_name_edited(move |i, t| if let (Some(op), Some(k)) = (me.upgrade(), KINDS.get(i.max(0) as usize)) {
        change(&op, &e, false, |l| kind_mut(l, *k).name = t.trim().to_string())
    });
    let (me, e) = ctx(op, edit);
    g.on_key_edited(move |i, t| if let (Some(op), Some(k)) = (me.upgrade(), KINDS.get(i.max(0) as usize)) {
        change(&op, &e, false, |l| kind_mut(l, *k).key = t.trim().to_lowercase())
    });
    let (me, e) = ctx(op, edit);
    g.on_pick(move |hex| {
        let Some(op) = me.upgrade() else { return };
        let at = op.ui.global::<SectionsDlg>().get_picking();
        op.ui.global::<SectionsDlg>().set_picking(-1);
        change(&op, &e, true, |l| match at {
            i if (0..LABEL_PICK).contains(&i) => if let Some(k) = KINDS.get(i as usize) { kind_mut(l, *k).color = hex.to_string() },
            i if i >= LABEL_PICK => if let Some(x) = labels_mut(l).get_mut((i - LABEL_PICK) as usize) { x.color = hex.to_string() },
            _ => {}
        });
    });
    let (me, e) = ctx(op, edit);
    g.on_add_section(move || {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<SectionsDlg>();
        let name = g.get_new_section().trim().to_string();
        let Some(kind) = KINDS.get(g.get_new_family().max(0) as usize).copied() else { return };
        if name.is_empty() { return }
        g.set_new_section(SharedString::new());
        change(&op, &e, true, |l| {
            l.added.retain(|a| !a.name.eq_ignore_ascii_case(&name));
            l.added.push(AddedSection { name, kind });
        });
    });
    let (me, e) = ctx(op, edit);
    g.on_remove_section(move |i| if let Some(op) = me.upgrade() {
        change(&op, &e, true, |l| if (0..l.added.len() as i32).contains(&i) { l.added.remove(i as usize); })
    });
    let (me, e) = ctx(op, edit);
    g.on_add_label(move || {
        let Some(op) = me.upgrade() else { return };
        let g = op.ui.global::<SectionsDlg>();
        let name = g.get_new_label().trim().to_string();
        if name.is_empty() { return }
        g.set_new_label(SharedString::new());
        change(&op, &e, true, |l| {
            let labels = labels_mut(l);
            if !labels.iter().any(|x| x.name.eq_ignore_ascii_case(&name)) { labels.push(SlideLabel { name, color: NEW_LABEL_COLOR.into() }) }
        });
    });
    let (me, e) = ctx(op, edit);
    g.on_label_edited(move |i, t| if let Some(op) = me.upgrade() {
        change(&op, &e, false, |l| if let Some(x) = labels_mut(l).get_mut(i.max(0) as usize) { x.name = t.trim().to_string() })
    });
    let (me, e) = ctx(op, edit);
    g.on_remove_label(move |i| if let Some(op) = me.upgrade() {
        change(&op, &e, true, |l| { let labels = labels_mut(l); if (0..labels.len() as i32).contains(&i) { labels.remove(i as usize); } })
    });
    let (me, e) = ctx(op, edit);
    g.on_reset(move || if let Some(op) = me.upgrade() {
        op.ui.global::<SectionsDlg>().set_picking(-1);
        change(&op, &e, true, |l| *l = SectionLook::default())
    });
}

/// "Example: Amazing Grace (My Chains Are Gone) · …": the copyright line for a sample song as `s` would make it, or
/// "" with credits off.
pub fn credit_sample(s: &Settings) -> String {
    if s.credits == "none" { return String::new() }
    let song = Song {
        title: "Amazing Grace (My Chains Are Gone)".into(), artist: "Chris Tomlin".into(), writers: "John Newton, Chris Tomlin, Louie Giglio".into(),
        publisher: "sixsteps Music".into(), year: "2006".into(), ccli: "4768151".into(), ..Default::default()
    };
    let template = if s.credit_template.trim().is_empty() { crate::slides::DEFAULT_CREDIT_TEMPLATE } else { s.credit_template.as_str() };
    strings::credit_example(&crate::slides::fill_credit(template, &song, &s.church_ccli))
}

#[cfg(test)]
#[path = "settings_sections_tests.rs"]
mod tests;
