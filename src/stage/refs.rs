//! What uses a stage layout, and saving the church's layouts all at once (all or nothing).
use super::{geom, presets, strings as s, StageLayout};
use serde::Serialize;
use uuid::Uuid;

/// Something that names a stage layout, listed before it is deleted.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayoutRef {
    StageDefault,
    ServiceItem { service: Uuid, item: Uuid, title: String },
    MacroStep { macro_id: Uuid, name: String },
}

/// A service item and the stage layout it asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemUse { pub service: Uuid, pub item: Uuid, pub title: String, pub layout: Option<Uuid> }

/// A macro and the stage layouts its steps switch to.
#[derive(Clone, Debug, PartialEq)]
pub struct MacroUse { pub macro_id: Uuid, pub name: String, pub layouts: Vec<Uuid> }

/// Where to look for uses (the caller gathers them from the stored data).
#[derive(Clone, Copy, Debug)]
pub struct RefSources<'a> { pub stage_default: Uuid, pub items: &'a [ItemUse], pub macros: &'a [MacroUse] }

/// Everything that uses layout `id`: the stage default first, then items in order, then each macro once.
pub fn layout_refs(src: &RefSources, id: Uuid) -> Vec<LayoutRef> {
    let mut out = vec![];
    if src.stage_default == id { out.push(LayoutRef::StageDefault); }
    out.extend(src.items.iter().filter(|i| i.layout == Some(id))
        .map(|i| LayoutRef::ServiceItem { service: i.service, item: i.item, title: i.title.clone() }));
    out.extend(src.macros.iter().filter(|m| m.layouts.contains(&id))
        .map(|m| LayoutRef::MacroStep { macro_id: m.macro_id, name: m.name.clone() }));
    out
}

/// The question before deleting a layout, saying what uses it and what happens to them.
pub fn delete_question(name: &str, refs: &[LayoutRef]) -> String {
    let items = refs.iter().filter(|r| matches!(r, LayoutRef::ServiceItem { .. })).count();
    let default = refs.contains(&LayoutRef::StageDefault);
    let macros = refs.iter().filter(|r| matches!(r, LayoutRef::MacroStep { .. })).count();
    let ask = s::delete_layout(name);
    if items == 0 && !default && macros == 0 { return ask; }
    let mut parts = vec![];
    if items > 0 { parts.push(s::service_items(items)); }
    if default { parts.push(s::the_stage_default()); }
    if macros > 0 { parts.push(s::macros(macros)); }
    let things = match parts.as_slice() {
        [one] => one.clone(),
        [a, b] => s::list_two(a, b),
        [a, b, c] => s::list_three(a, b, c),
        _ => parts.join(", "),
    };
    let first = match (items, default) {
        (0, true) => Some(s::default_uses_classic()),
        (1, false) => Some(s::item_uses_default()),
        (_, false) if items > 1 => Some(s::items_use_default()),
        (_, true) => Some(s::items_and_default_use_classic()),
        _ => None,
    };
    let what_happens = match (first, macros) {
        (Some(f), 0) => f,
        (Some(f), 1) => s::two_effects(&f, &s::macro_wont_run()),
        (Some(f), _) => s::two_effects(&f, &s::macros_wont_run()),
        (None, 1) => s::macro_wont_run_alone(),
        (None, _) => s::macros_wont_run_alone(),
    };
    s::used_by(&ask, &things, &what_happens)
}

/// The church's layouts after saving `layouts` (added or changed) and removing `deleted`, or why not.
/// Nothing is applied on an error. Each layout is tidied (`geom::sanitise_layout`); names are trimmed.
pub fn plan_replace(church: &[StageLayout], layouts: Vec<StageLayout>, deleted: &[Uuid]) -> Result<Vec<StageLayout>, String> {
    if layouts.iter().map(|l| l.id).chain(deleted.iter().copied()).any(presets::is_builtin) { return Err(s::builtin_locked().into()); }
    if layouts.iter().any(|l| l.elements.len() > geom::MAX_BOXES) { return Err(s::too_many_boxes().into()); }
    let mut out: Vec<StageLayout> = church.iter().filter(|l| !deleted.contains(&l.id)).cloned().collect();
    let sent: Vec<Uuid> = layouts.iter().map(|l| l.id).collect();
    for mut l in layouts {
        if deleted.contains(&l.id) { continue; }
        l.name = l.name.trim().to_string();
        geom::sanitise_layout(&mut l);
        match out.iter_mut().find(|o| o.id == l.id) { Some(o) => *o = l, None => out.push(l) }
    }
    // Only the layouts sent are checked, in the order sent, so the error names what the person typed.
    for id in sent {
        let Some(l) = out.iter().find(|l| l.id == id) else { continue };
        if l.name.is_empty() { return Err(s::name_empty().into()); }
        let key = l.name.to_lowercase();
        let clash = presets::builtins().iter().any(|b| b.name.to_lowercase() == key)
            || out.iter().any(|o| o.id != id && o.name.trim().to_lowercase() == key);
        if clash { return Err(s::name_taken(&l.name)); }
    }
    Ok(out)
}

/// After layouts are deleted: the stage default goes back to Classic and items that named one use the default.
/// Returns how many were cleared.
pub fn clear_deleted(default: &mut Uuid, item_layouts: &mut [&mut Option<Uuid>], deleted: &[Uuid]) -> usize {
    let mut n = 0;
    if deleted.contains(default) { *default = presets::CLASSIC; n += 1; }
    for l in item_layouts.iter_mut() {
        if l.is_some_and(|id| deleted.contains(&id)) { **l = None; n += 1; }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lenient::Lenient, stage::{ElementKind, ElementStyle, PctRect, StageElement, BACKGROUND}};

    const L1: Uuid = Uuid::from_u128(901);
    const L2: Uuid = Uuid::from_u128(902);
    const S1: Uuid = Uuid::from_u128(11);
    const S2: Uuid = Uuid::from_u128(12);
    const M1: Uuid = Uuid::from_u128(21);

    fn layout(id: Uuid, name: &str) -> StageLayout {
        StageLayout { id, name: name.into(), background: BACKGROUND.into(), font: String::new(), elements: vec![] }
    }
    fn item(service: Uuid, n: u128, layout: Option<Uuid>) -> ItemUse {
        ItemUse { service, item: Uuid::from_u128(n), title: format!("Item {n}"), layout }
    }
    fn refs_for(items: usize, default: bool, macros: usize) -> Vec<LayoutRef> {
        let mut out = vec![];
        if default { out.push(LayoutRef::StageDefault); }
        out.extend((0..items).map(|n| LayoutRef::ServiceItem { service: S1, item: Uuid::from_u128(n as u128), title: "x".into() }));
        out.extend((0..macros).map(|n| LayoutRef::MacroStep { macro_id: Uuid::from_u128(n as u128), name: "m".into() }));
        out
    }

    #[test]
    fn delete_question_matches_spec_example() {
        assert_eq!(delete_question("Band", &refs_for(3, true, 1)),
            "Delete “Band”? It is used by 3 service items, the stage default and 1 macro. Items and the default will use Classic; the macro won't run until it is edited.");
    }

    #[test]
    fn delete_question_without_refs() { assert_eq!(delete_question("Band", &[]), "Delete “Band”?"); }

    #[test]
    fn delete_question_items_only_singular() {
        assert_eq!(delete_question("Band", &refs_for(1, false, 0)), "Delete “Band”? It is used by 1 service item. That item will use the stage default.");
        assert_eq!(delete_question("Band", &refs_for(2, false, 0)), "Delete “Band”? It is used by 2 service items. Those items will use the stage default.");
        assert_eq!(delete_question("Band", &refs_for(0, false, 2)), "Delete “Band”? It is used by 2 macros. The macros won't run until they are edited.");
        assert_eq!(delete_question("Band", &refs_for(0, true, 0)), "Delete “Band”? It is used by the stage default. The default will use Classic.");
    }

    #[test]
    fn layout_refs_counts_items_in_every_service_and_macros_once() {
        let items = [item(S1, 1, Some(L1)), item(S1, 2, Some(L2)), item(S2, 3, Some(L1)), item(S1, 4, None), item(S2, 5, Some(L1))];
        let macros = [MacroUse { macro_id: M1, name: "Sermon".into(), layouts: vec![L1, L2, L1] }, MacroUse { macro_id: M1, name: "Other".into(), layouts: vec![L2] }];
        let src = RefSources { stage_default: presets::CLASSIC, items: &items, macros: &macros };
        let refs = layout_refs(&src, L1);
        let titles: Vec<&str> = refs.iter().filter_map(|r| match r { LayoutRef::ServiceItem { title, .. } => Some(title.as_str()), _ => None }).collect();
        assert_eq!(titles, ["Item 1", "Item 3", "Item 5"]);
        assert_eq!(refs.last(), Some(&LayoutRef::MacroStep { macro_id: M1, name: "Sermon".into() }));
        assert_eq!(refs.len(), 4);
        let src = RefSources { stage_default: L1, ..src };
        assert_eq!(layout_refs(&src, L1)[0], LayoutRef::StageDefault);
    }

    #[test]
    fn layouts_replace_rejects_builtin_empty_duplicate_and_too_many() {
        let church = vec![layout(L1, "Ours")];
        let err = |layouts: Vec<StageLayout>, deleted: &[Uuid]| plan_replace(&church, layouts, deleted).unwrap_err();
        assert_eq!(err(vec![layout(presets::CLASSIC, "Mine")], &[]), "Built-in stage layouts can't be changed. Make a copy first.");
        assert_eq!(err(vec![], &[presets::BAND]), "Built-in stage layouts can't be changed. Make a copy first.");
        assert_eq!(err(vec![layout(L2, "  ")], &[]), "Give the stage layout a name.");
        assert_eq!(err(vec![layout(L2, " ours ")], &[]), "There is already a stage layout called “ours”.");
        assert_eq!(err(vec![layout(L2, "classic")], &[]), "There is already a stage layout called “classic”.");
        let mut big = layout(L2, "Big");
        big.elements = (0..25).map(|n| Lenient::Known(StageElement { id: Uuid::from_u128(n), kind: ElementKind::Notes,
            rect: PctRect::new(0.0, 0.0, 10.0, 10.0), style: ElementStyle::default() })).collect();
        assert_eq!(err(vec![big], &[]), "A stage layout can have at most 24 boxes.");
    }

    #[test]
    fn layouts_replace_is_all_or_nothing_and_trims_names() {
        let church = vec![layout(L1, "Ours")];
        let before = church.clone();
        assert!(plan_replace(&church, vec![layout(L1, "Renamed"), layout(L2, "")], &[]).is_err());
        assert_eq!(church, before);
        let out = plan_replace(&church, vec![layout(L1, " Renamed "), layout(L2, "New")], &[]).unwrap();
        assert_eq!(out.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Renamed", "New"]);
    }

    #[test]
    fn rename_into_deleted_name_succeeds() {
        let church = vec![layout(L1, "Band 2"), layout(L2, "Band 2 copy")];
        let out = plan_replace(&church, vec![layout(L2, "Band 2")], &[L1]).unwrap();
        assert_eq!(out, vec![layout(L2, "Band 2")]);
    }

    #[test]
    fn replace_keeps_unknown_boxes_and_sanitises_rects() {
        let v = serde_json::json!({"id": "x", "kind": {"type": "hologram"}});
        let mut l = layout(L1, "Ours");
        l.elements = vec![Lenient::Unknown(v.clone()), Lenient::Known(StageElement { id: Uuid::from_u128(1), kind: ElementKind::Notes,
            rect: PctRect::new(f32::NAN, 0.0, 150.0, 10.0), style: ElementStyle::default() })];
        let out = plan_replace(&[], vec![l], &[]).unwrap();
        assert_eq!(out[0].elements[0], Lenient::Unknown(v));
        assert_eq!(out[0].elements[1].known().unwrap().rect, PctRect::new(0.0, 0.0, 100.0, 10.0));
    }

    #[test]
    fn clear_deleted_resets_default_and_items() {
        let (mut default, mut a, mut b, mut c) = (L1, Some(L1), Some(L2), None);
        assert_eq!(clear_deleted(&mut default, &mut [&mut a, &mut b, &mut c], &[L1]), 2);
        assert_eq!((default, a, b, c), (presets::CLASSIC, None, Some(L2), None));
    }
}
