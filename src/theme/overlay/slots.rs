//! The rules for the overlay slots on the live frame (spec §5.2): show, hide, the cap of 8 saved
//! overlays, the clear epoch check, failure reports, the operator chips and the phone list. These
//! are pure functions over `Vec<OverlaySlot>`; none of them bumps a version (the caller does).
//! This file moves to `src/overlay/slots.rs` in Part B, so it names other FEAT-1 items only by
//! absolute paths.
use serde::Serialize;
use uuid::Uuid;

use crate::model::AssetId;
use crate::theme::overlay::{
    ImageSource, MAX_SAVED_ON, Overlay, OverlayKind, OverlaySlot, OverlaySource, SAVED_PREFIX, saved_id, saved_key,
};
use crate::theme::strings;

/// Same content, ignoring `inst`.
fn same_content(a: &OverlaySlot, b: &OverlaySlot) -> bool {
    a.key == b.key && a.z == b.z && a.name == b.name && a.source == b.source && a.place == b.place
}

/// Puts `slot` up, replacing a slot with the same key, and keeps the slots sorted by `(z, key)`.
/// Returns false (and keeps the slot that is up, with its `inst`) when an equal copy is already up.
pub fn show(slots: &mut Vec<OverlaySlot>, slot: OverlaySlot) -> bool {
    if let Some(i) = slots.iter().position(|s| s.key == slot.key) {
        if same_content(&slots[i], &slot) {
            return false;
        }
        slots.remove(i);
    }
    let at = slots.partition_point(|s| (s.z, s.key.as_str()) < (slot.z, slot.key.as_str()));
    slots.insert(at, slot);
    true
}

/// Takes the slot with this key down; false when it was not up.
pub fn hide(slots: &mut Vec<OverlaySlot>, key: &str) -> bool {
    match slots.iter().position(|s| s.key == key) {
        Some(i) => {
            slots.remove(i);
            true
        }
        None => false,
    }
}

/// Keeps only the slots `f` accepts; true when any was removed.
pub fn retain(slots: &mut Vec<OverlaySlot>, f: impl Fn(&OverlaySlot) -> bool) -> bool {
    let before = slots.len();
    slots.retain(|s| f(s));
    slots.len() != before
}

/// How many saved overlays are up (timers and messages don't count toward the cap).
pub fn saved_on(slots: &[OverlaySlot]) -> usize {
    slots.iter().filter(|s| s.key.starts_with(SAVED_PREFIX)).count()
}

/// A show without a base is refused when its sender has not seen the newest clear.
pub fn check_seen_clear(seen: Option<u64>, epoch: u64) -> Result<(), String> {
    match seen {
        Some(seen) if seen >= epoch => Ok(()),
        _ => Err(strings::SCREEN_CLEARED.into()),
    }
}

/// The source with `ChurchLogo` replaced by the logo; `None` when it names the logo and there is none.
fn with_logo(source: &OverlaySource, logo: Option<&AssetId>) -> Option<OverlaySource> {
    match source {
        OverlaySource::Image { source: ImageSource::ChurchLogo, opacity } => {
            logo.map(|asset| OverlaySource::Image { source: ImageSource::Asset { asset: asset.clone() }, opacity: *opacity })
        }
        other => Some(other.clone()),
    }
}

/// The slot for saved overlay `id`: key `saved:<id>`, z = its library index, `ChurchLogo` resolved
/// to the current logo.
pub fn saved_slot(id: Uuid, library: &[Overlay], logo: Option<&AssetId>, inst: u64) -> Result<OverlaySlot, String> {
    let (index, o) = library.iter().enumerate().find(|(_, o)| o.id == id).ok_or_else(|| strings::OVERLAY_GONE.to_string())?;
    let source = with_logo(&o.source, logo).ok_or_else(|| strings::LOGO_FIRST.to_string())?;
    Ok(OverlaySlot { key: saved_key(id), z: index as u32, inst, name: o.name.clone(), source, place: o.place })
}

/// Turns a saved overlay on: refused when 8 saved overlays are up and this is not one of them.
/// Then `show`; the result says whether anything changed.
pub fn turn_on(slots: &mut Vec<OverlaySlot>, slot: OverlaySlot) -> Result<bool, String> {
    if saved_on(slots) >= MAX_SAVED_ON && !slots.iter().any(|s| s.key == slot.key) {
        return Err(strings::OVERLAY_CAP.into());
    }
    Ok(show(slots, slot))
}

/// A slot could not be drawn: removes it when both key and inst match (a stale report is ignored)
/// and returns its name for `strings::overlay_failed`.
pub fn failed(slots: &mut Vec<OverlaySlot>, key: &str, inst: u64) -> Option<String> {
    let i = slots.iter().position(|s| s.key == key && s.inst == inst)?;
    Some(slots.remove(i).name)
}

/// One chip in the operator's Overlays strip.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    pub id: Uuid,
    pub name: String,
    pub kind: OverlayKind,
    pub on: bool,
    /// On, and the library copy differs from what is up.
    pub changed: bool,
    /// Up, but deleted from the library.
    pub deleted: bool,
}

/// The chips in library order, then the saved slots that are up but whose overlay was deleted.
pub fn chips(library: &[Overlay], slots: &[OverlaySlot], logo: Option<&AssetId>) -> Vec<Chip> {
    let mut out: Vec<Chip> = library
        .iter()
        .map(|o| {
            let key = saved_key(o.id);
            let up = slots.iter().find(|s| s.key == key);
            let changed = up.is_some_and(|s| {
                let source = with_logo(&o.source, logo).unwrap_or_else(|| o.source.clone());
                s.name != o.name || s.source != source || s.place != o.place
            });
            Chip { id: o.id, name: o.name.clone(), kind: o.source.kind(), on: up.is_some(), changed, deleted: false }
        })
        .collect();
    out.extend(slots.iter().filter_map(|s| {
        let id = saved_id(&s.key)?;
        (!library.iter().any(|o| o.id == id))
            .then(|| Chip { id, name: s.name.clone(), kind: s.source.kind(), on: true, changed: false, deleted: true })
    }));
    out
}

/// One saved overlay in the phone remote's view.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct RemoteOverlay {
    pub id: Uuid,
    pub name: String,
    pub on: bool,
    /// `"text"`, `"picture"` or `"clock"` (`"other"` for a newer kind).
    pub kind: &'static str,
}

fn kind_name(kind: OverlayKind) -> &'static str {
    match kind {
        OverlayKind::Text => "text",
        OverlayKind::Picture => "picture",
        OverlayKind::Clock => "clock",
        OverlayKind::Other => "other",
    }
}

/// The saved overlays for the phone, in `chips` order; timer and message slots are not listed.
pub fn remote_list(library: &[Overlay], slots: &[OverlaySlot]) -> Vec<RemoteOverlay> {
    chips(library, slots, None)
        .into_iter()
        .map(|c| RemoteOverlay {
            id: c.id,
            name: if c.deleted { strings::deleted_overlay(&c.name) } else { c.name },
            on: c.on,
            kind: kind_name(c.kind),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::overlay::{Anchor, OverlayLook, Placement};
    use crate::theme::strings::*;

    fn text(name: &str, body: &str) -> Overlay {
        Overlay {
            id: Uuid::new_v4(),
            name: name.into(),
            source: OverlaySource::Text { text: body.into(), styles: Vec::new(), look: OverlayLook::default() },
            place: Placement::default_for(OverlayKind::Text),
        }
    }

    fn logo_bug() -> Overlay {
        Overlay {
            id: Uuid::new_v4(),
            name: "Logo".into(),
            source: OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 },
            place: Placement::default(),
        }
    }

    fn clock() -> Overlay {
        Overlay {
            id: Uuid::new_v4(),
            name: "Clock".into(),
            source: OverlaySource::Clock { look: OverlayLook::default(), seconds: false, h24: false },
            place: Placement::default_for(OverlayKind::Clock),
        }
    }

    fn slot(key: &str, z: u32, inst: u64) -> OverlaySlot {
        OverlaySlot {
            key: key.into(),
            z,
            inst,
            name: key.into(),
            source: OverlaySource::Text { text: "Hi".into(), styles: Vec::new(), look: OverlayLook::default() },
            place: Placement::default_for(OverlayKind::Text),
        }
    }

    fn keys(slots: &[OverlaySlot]) -> Vec<&str> {
        slots.iter().map(|s| s.key.as_str()).collect()
    }

    #[test]
    fn overlay_on_bumps_and_is_idempotent() {
        let mut slots = Vec::new();
        assert!(show(&mut slots, slot("saved:a", 0, 1)));
        assert!(!show(&mut slots, slot("saved:a", 0, 2)));
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].inst, 1, "an unchanged copy keeps the inst a failure report names");
    }

    #[test]
    fn overlay_on_changed_copy_replaces() {
        let mut slots = vec![slot("saved:a", 0, 1)];
        let mut edited = slot("saved:a", 0, 2);
        edited.place.anchor = Anchor::Top;
        assert!(show(&mut slots, edited.clone()));
        assert_eq!(slots, vec![edited]);
    }

    #[test]
    fn overlay_off_when_absent_is_noop() {
        let mut slots = vec![slot("saved:a", 0, 1)];
        assert!(!hide(&mut slots, "saved:b"));
        assert_eq!(slots.len(), 1);
        assert!(hide(&mut slots, "saved:a"));
        assert!(slots.is_empty());
        assert!(!hide(&mut slots, "saved:a"));
    }

    #[test]
    fn slots_stay_sorted_by_z_then_key() {
        let mut slots = Vec::new();
        show(&mut slots, slot("message", u32::MAX, 1));
        show(&mut slots, slot("saved:b", 2, 2));
        show(&mut slots, slot("saved:c", 0, 3));
        show(&mut slots, slot("saved:a", 2, 4));
        show(&mut slots, slot("timer:x", 1_000_000, 5));
        assert_eq!(keys(&slots), ["saved:c", "saved:a", "saved:b", "timer:x", "message"]);
        // a replacement whose z moved is re-sorted
        show(&mut slots, slot("saved:c", 3, 6));
        assert_eq!(keys(&slots), ["saved:a", "saved:b", "saved:c", "timer:x", "message"]);
    }

    #[test]
    fn ninth_saved_overlay_is_rejected() {
        let mut slots = Vec::new();
        for i in 0..8u32 {
            assert_eq!(turn_on(&mut slots, slot(&format!("saved:{i}"), i, i as u64)), Ok(true));
        }
        assert_eq!(saved_on(&slots), MAX_SAVED_ON);
        assert_eq!(turn_on(&mut slots, slot("saved:9", 9, 9)).unwrap_err(), OVERLAY_CAP);
        assert_eq!(slots.len(), 8);
        let mut edited = slot("saved:3", 3, 10);
        edited.name = "Edited".into();
        assert_eq!(turn_on(&mut slots, edited), Ok(true));
        assert_eq!(turn_on(&mut slots, slot("saved:4", 4, 11)), Ok(false));
    }

    #[test]
    fn cap_ignores_non_saved_slots() {
        let mut slots: Vec<OverlaySlot> = (0..7u32).map(|i| slot(&format!("saved:{i}"), i, i as u64)).collect();
        slots.push(slot("message", u32::MAX, 20));
        slots.push(OverlaySlot { source: OverlaySource::Unknown, ..slot("timer:x", 1_000_000, 21) });
        assert_eq!(saved_on(&slots), 7);
        assert_eq!(turn_on(&mut slots, slot("saved:7", 7, 22)), Ok(true));
        assert_eq!(saved_on(&slots), 8);
    }

    #[test]
    fn overlay_on_with_stale_seen_clear_is_refused() {
        assert_eq!(check_seen_clear(Some(3), 4).unwrap_err(), SCREEN_CLEARED);
        assert!(check_seen_clear(Some(4), 4).is_ok());
        assert!(check_seen_clear(Some(5), 4).is_ok());
    }

    #[test]
    fn overlay_on_without_seen_clear_is_refused() {
        assert_eq!(check_seen_clear(None, 0).unwrap_err(), SCREEN_CLEARED);
    }

    #[test]
    fn logo_retain_keeps_text_picture_clock_drops_unknown() {
        let lib = [text("Wi-Fi", "Guest"), logo_bug(), clock()];
        let logo: AssetId = "logo.png".into();
        let mut slots: Vec<OverlaySlot> =
            lib.iter().enumerate().map(|(i, o)| saved_slot(o.id, &lib, Some(&logo), i as u64).unwrap()).collect();
        slots.push(OverlaySlot { source: OverlaySource::Unknown, ..slot("timer:x", 1_000_000, 9) });
        assert!(retain(&mut slots, |s| s.source.survives_logo()));
        assert_eq!(slots.len(), 3);
        assert!(!retain(&mut slots, |s| s.source.survives_logo()), "nothing removed");
        assert!(retain(&mut slots, |_| false));
        assert!(slots.is_empty());
    }

    #[test]
    fn logo_bug_follows_logo_change_on_next_toggle() {
        let lib = [text("Wi-Fi", "Guest"), logo_bug()];
        let id = lib[1].id;
        let a: AssetId = "a.png".into();
        let b: AssetId = "b.png".into();
        let first = saved_slot(id, &lib, Some(&a), 1).unwrap();
        assert_eq!(first.key, saved_key(id));
        assert_eq!((first.z, first.inst, first.name.as_str()), (1, 1, "Logo"));
        assert_eq!(first.source, OverlaySource::Image { source: ImageSource::Asset { asset: a }, opacity: 100 });
        let second = saved_slot(id, &lib, Some(&b), 2).unwrap();
        assert_eq!(second.source, OverlaySource::Image { source: ImageSource::Asset { asset: b }, opacity: 100 });
        let mut slots = vec![first];
        assert_eq!(turn_on(&mut slots, second.clone()), Ok(true));
        assert_eq!(slots, vec![second]);
    }

    #[test]
    fn logo_bug_without_logo_errors() {
        let lib = [logo_bug()];
        assert_eq!(saved_slot(lib[0].id, &lib, None, 1).unwrap_err(), LOGO_FIRST);
        // a picture with its own asset needs no logo
        let own = Overlay { source: OverlaySource::Image { source: ImageSource::Asset { asset: "p.png".into() }, opacity: 50 }, ..logo_bug() };
        assert!(saved_slot(own.id, std::slice::from_ref(&own), None, 1).is_ok());
    }

    #[test]
    fn gone_overlay_errors() {
        let lib = [text("Wi-Fi", "Guest")];
        assert_eq!(saved_slot(Uuid::new_v4(), &lib, None, 1).unwrap_err(), OVERLAY_GONE);
    }

    #[test]
    fn overlay_failed_removes_slot_and_names_it() {
        let lib = [text("Wi-Fi", "Guest"), logo_bug()];
        let logo: AssetId = "logo.png".into();
        let mut slots = vec![saved_slot(lib[0].id, &lib, None, 4).unwrap(), saved_slot(lib[1].id, &lib, Some(&logo), 5).unwrap()];
        let key = saved_key(lib[1].id);
        let name = failed(&mut slots, &key, 5).unwrap();
        assert_eq!(overlay_failed(&name, PICTURE_UNREADABLE), "Couldn't show the overlay “Logo”: the picture could not be loaded.");
        assert_eq!(keys(&slots), [saved_key(lib[0].id).as_str()]);
    }

    #[test]
    fn stale_overlay_failed_report_ignored() {
        let mut slots = vec![slot("saved:a", 0, 7)];
        assert_eq!(failed(&mut slots, "saved:a", 6), None);
        assert_eq!(failed(&mut slots, "saved:b", 7), None);
        assert_eq!(slots.len(), 1);
    }

    #[test]
    fn chips_mark_changed_and_deleted() {
        let mut lib = vec![text("Wi-Fi", "Guest"), logo_bug(), clock(), text("Gone", "Bye")];
        let a: AssetId = "a.png".into();
        let mut slots = Vec::new();
        for (i, o) in lib.iter().enumerate().filter(|(i, _)| *i != 2) {
            show(&mut slots, saved_slot(o.id, &lib, Some(&a), i as u64).unwrap());
        }
        let gone = lib.pop().unwrap();
        // edit Wi-Fi while it is on
        lib[0].source = OverlaySource::Text { text: "Guest 2".into(), styles: Vec::new(), look: OverlayLook::default() };

        let unchanged = chips(&lib, &slots, Some(&a));
        assert_eq!(
            unchanged.iter().map(|c| (c.id, c.name.as_str(), c.kind, c.on, c.changed, c.deleted)).collect::<Vec<_>>(),
            vec![
                (lib[0].id, "Wi-Fi", OverlayKind::Text, true, true, false),
                (lib[1].id, "Logo", OverlayKind::Picture, true, false, false),
                (lib[2].id, "Clock", OverlayKind::Clock, false, false, false),
                (gone.id, "Gone", OverlayKind::Text, true, false, true),
            ]
        );
        // the church logo changed after the bug went up
        let b: AssetId = "b.png".into();
        assert!(chips(&lib, &slots, Some(&b))[1].changed);
        assert!(chips(&lib, &slots, None)[1].changed);
        // an off chip is never "changed"
        assert!(chips(&lib, &[], Some(&a)).iter().all(|c| !c.on && !c.changed));
    }

    #[test]
    fn remote_view_lists_saved_overlays_with_state() {
        let mut lib = vec![text("Wi-Fi", "Guest"), logo_bug(), clock()];
        let a: AssetId = "a.png".into();
        let mut slots = vec![saved_slot(lib[0].id, &lib, None, 1).unwrap(), saved_slot(lib[1].id, &lib, Some(&a), 2).unwrap()];
        slots.push(slot("message", u32::MAX, 3));
        slots.push(OverlaySlot { source: OverlaySource::Unknown, ..slot("timer:x", 1_000_000, 4) });
        let wifi = lib.remove(0);
        let list = remote_list(&lib, &slots);
        assert_eq!(
            list,
            vec![
                RemoteOverlay { id: lib[0].id, name: "Logo".into(), on: true, kind: "picture" },
                RemoteOverlay { id: lib[1].id, name: "Clock".into(), on: false, kind: "clock" },
                RemoteOverlay { id: wifi.id, name: "(deleted) Wi-Fi".into(), on: true, kind: "text" },
            ]
        );
        let v = serde_json::to_value(&list[0]).unwrap();
        assert_eq!(v, serde_json::json!({"id": lib[0].id, "name": "Logo", "on": true, "kind": "picture"}));
    }
}
