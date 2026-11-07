//! FEAT-2's overlay slots (spec §4.2, §6): timers are `timer:<uuid>`, the one message is `message`.
//! Both sit above every saved overlay and don't count toward FEAT-1's cap of 8.
use crate::automation::messages::{MessagePlace, MessageTemplate};
use crate::automation::strings;
use crate::theme::overlay::{Anchor, BoxKind, OverlayLook, OverlaySlot, OverlaySource, Placement, SlotKey};
use crate::timers::{OverlayPlace, TimerDef};
use uuid::Uuid;

pub const TIMER_PREFIX: &str = "timer:";
/// The one message slot.
pub const MESSAGE_KEY: &str = "message";
/// The message is on top of everything.
pub const MESSAGE_Z: u32 = u32::MAX;
/// At most this many timers on the screen at once.
pub const MAX_ROOM_TIMERS: usize = 4;

pub fn timer_key(id: Uuid) -> SlotKey { format!("{TIMER_PREFIX}{id}") }

/// The timer a slot key names, if it is a `timer:` key.
pub fn timer_id(key: &str) -> Option<Uuid> { key.strip_prefix(TIMER_PREFIX).and_then(|id| Uuid::parse_str(id).ok()) }

/// Timers sit above saved overlays, in Timers-tab order.
pub fn timer_z(index: usize) -> u32 { 1_000_000u32.saturating_add(index.min(999_999) as u32) }

/// A fifth timer is refused; showing one that is already up replaces it.
pub fn room_for_timer(slots: &[OverlaySlot], key: &str) -> Result<(), String> {
    let up = slots.iter().filter(|s| s.key.starts_with(TIMER_PREFIX)).count();
    if up >= MAX_ROOM_TIMERS && !slots.iter().any(|s| s.key == key) { return Err(strings::too_many_room_timers()) }
    Ok(())
}

/// FEAT-1 §4.5's mapping of the five choices: corners keep their anchor at a quarter of the width.
pub fn timer_placement(place: OverlayPlace) -> Placement {
    let corner = |anchor| Placement { anchor, width: 25, margin: 4 };
    match place {
        OverlayPlace::TopRight => corner(Anchor::TopRight),
        OverlayPlace::TopLeft => corner(Anchor::TopLeft),
        OverlayPlace::BottomRight => corner(Anchor::BottomRight),
        OverlayPlace::BottomLeft => corner(Anchor::BottomLeft),
        OverlayPlace::CenterLarge => Placement { anchor: Anchor::Center, width: 80, margin: 4 },
    }
}

/// A full-width bar along the bottom or the top.
pub fn message_placement(place: MessagePlace) -> Placement {
    let anchor = if place == MessagePlace::Top { Anchor::Top } else { Anchor::Bottom };
    Placement { anchor, width: 100, margin: 1 }
}

/// Spec §6: a dark plate in a corner, or big words with a shadow in the middle.
pub fn timer_look(large: bool) -> OverlayLook {
    if large {
        OverlayLook { size: 220, weight: 700, shadow: true, box_kind: BoxKind::None, ..OverlayLook::default() }
    } else {
        OverlayLook { size: 72, weight: 700, shadow: false, box_kind: BoxKind::Behind, box_color: "#000000".into(), box_opacity: 65, ..OverlayLook::default() }
    }
}

pub fn message_look() -> OverlayLook {
    OverlayLook { size: 52, weight: 600, shadow: false, box_kind: BoxKind::Bar, box_color: "#000000".into(), box_opacity: 75, ..OverlayLook::default() }
}

/// The slot for timer `def`, the `index`th in the Timers tab.
pub fn timer_slot(def: &TimerDef, index: usize, inst: u64) -> OverlaySlot {
    let large = def.place == OverlayPlace::CenterLarge;
    OverlaySlot {
        key: timer_key(def.id),
        z: timer_z(index),
        inst,
        name: def.name.clone(),
        source: OverlaySource::Timer { timer_id: def.id, caption: def.caption.clone(), large, look: timer_look(large) },
        place: timer_placement(def.place),
    }
}

/// The message slot for `text` filled from template `t`; `shown` is new for every Show.
pub fn message_slot(t: &MessageTemplate, shown: Uuid, text: String, inst: u64) -> OverlaySlot {
    OverlaySlot {
        key: MESSAGE_KEY.into(),
        z: MESSAGE_Z,
        inst,
        name: t.name.clone(),
        source: OverlaySource::Message { shown, template_id: t.id, text, look: message_look() },
        place: message_placement(t.place),
    }
}

/// The `shown` id of the message up in these slots.
pub fn shown_message(slots: &[OverlaySlot]) -> Option<Uuid> {
    slots.iter().find_map(|s| match &s.source { OverlaySource::Message { shown, .. } if s.key == MESSAGE_KEY => Some(*shown), _ => None })
}

/// The timers up in these slots.
pub fn room_timers(slots: &[OverlaySlot]) -> Vec<Uuid> { slots.iter().filter_map(|s| timer_id(&s.key)).collect() }

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(key: &str) -> OverlaySlot {
        OverlaySlot { key: key.into(), z: 0, inst: 1, name: String::new(), source: OverlaySource::Unknown, place: Placement::default() }
    }

    #[test]
    fn timer_key_format_and_z() {
        let id = Uuid::from_u128(7);
        assert_eq!(timer_key(id), format!("timer:{id}"));
        assert_eq!(timer_id(&timer_key(id)), Some(id));
        assert_eq!(timer_id("message"), None);
        assert_eq!(timer_z(2), 1_000_002);
    }

    #[test]
    fn fifth_timer_in_room_refused() {
        let mut slots: Vec<OverlaySlot> = (0..4u128).map(|i| slot(&timer_key(Uuid::from_u128(i)))).collect();
        slots.push(slot(MESSAGE_KEY));
        slots.push(slot("saved:x"));
        assert_eq!(room_for_timer(&slots, &timer_key(Uuid::from_u128(9))), Err(strings::too_many_room_timers()));
        assert_eq!(room_for_timer(&slots, &timer_key(Uuid::from_u128(2))), Ok(()), "re-showing one that is up replaces it");
        slots.remove(0);
        assert_eq!(room_for_timer(&slots, &timer_key(Uuid::from_u128(9))), Ok(()));
    }

    #[test]
    fn timer_place_maps_to_feat1_placement() {
        assert_eq!(timer_placement(OverlayPlace::BottomLeft), Placement { anchor: Anchor::BottomLeft, width: 25, margin: 4 });
        assert_eq!(timer_placement(OverlayPlace::CenterLarge), Placement { anchor: Anchor::Center, width: 80, margin: 4 });
        assert_eq!(message_placement(MessagePlace::Top).anchor, Anchor::Top);
        assert_eq!(message_placement(MessagePlace::Bottom).width, 100);
    }
}
