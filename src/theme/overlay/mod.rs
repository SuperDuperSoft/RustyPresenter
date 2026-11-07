//! The overlay layer (spec §4.5): saved overlays, the slots on the live frame, the Logo policy per
//! source, bound (clock) text and save validation. FEAT-2 adds its `Timer` and `Message` arms to
//! `OverlaySource`. This file moves to `src/overlay.rs` in Part B, so it names other FEAT-1 items
//! only by absolute paths.
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::AssetId;
use crate::theme::richtext::{self, StyleSpan};
use crate::theme::strings;

pub mod slots;

/// `"saved:<uuid>"`, `"timer:<uuid>"` (FEAT-2) or `"message"` (FEAT-2).
pub type SlotKey = String;

/// Show, hide and in-place replacement fade, in milliseconds.
pub const OVERLAY_FADE_MS: u32 = 400;
/// How many saved overlays may be on at once.
pub const MAX_SAVED_ON: usize = 8;

/// The key prefix of saved overlays.
pub const SAVED_PREFIX: &str = "saved:";

/// The slot key of a saved overlay.
pub fn saved_key(id: Uuid) -> SlotKey {
    format!("{SAVED_PREFIX}{id}")
}

/// The saved overlay a slot key names, if it is a `saved:` key.
pub fn saved_id(key: &str) -> Option<Uuid> {
    key.strip_prefix(SAVED_PREFIX).and_then(|id| Uuid::parse_str(id).ok())
}

/// One overlay on the live frame: a copy of its content as it was when shown.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct OverlaySlot {
    pub key: SlotKey,
    /// Saved: library index; FEAT-2 timers 1_000_000 + index; message `u32::MAX`.
    pub z: u32,
    /// From `Live.next_inst` at show time; failure reports name it.
    pub inst: u64,
    /// For chips, notices and the phone.
    pub name: String,
    pub source: OverlaySource,
    #[serde(default)]
    pub place: Placement,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OverlaySource {
    Text {
        text: String,
        #[serde(default)]
        styles: Vec<StyleSpan>,
        #[serde(default)]
        look: OverlayLook,
    },
    Image {
        source: ImageSource,
        /// 0–100
        #[serde(default = "full")]
        opacity: u8,
    },
    Clock {
        #[serde(default)]
        look: OverlayLook,
        #[serde(default)]
        seconds: bool,
        #[serde(default)]
        h24: bool,
    },
    /// A newer source kind: never drawn, removed by every clear.
    #[serde(other)]
    Unknown,
}

fn full() -> u8 {
    100
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSource {
    ChurchLogo,
    Asset { asset: AssetId },
}

/// The font, colour and box of a text-bearing overlay.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct OverlayLook {
    /// `""` = the church default font.
    pub font: String,
    /// px at 1080p
    pub size: u32,
    pub color: String,
    pub weight: u16,
    pub shadow: bool,
    pub box_kind: BoxKind,
    pub box_color: String,
    pub box_opacity: u8,
}

impl Default for OverlayLook {
    fn default() -> Self {
        Self {
            font: String::new(),
            size: 48,
            color: "#ffffff".into(),
            weight: 600,
            shadow: true,
            box_kind: BoxKind::None,
            box_color: "#000000".into(),
            box_opacity: 60,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum BoxKind {
    #[default]
    None,
    /// A rounded box behind the text.
    Behind,
    /// Flush on its edge, full width.
    Bar,
}

/// Where an overlay sits: `width` in percent of the screen (5–100), `margin` in logical px / 10.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct Placement {
    pub anchor: Anchor,
    pub width: u8,
    pub margin: u8,
}

impl Default for Placement {
    fn default() -> Self {
        Placement::default_for(OverlayKind::Picture)
    }
}

impl Placement {
    /// The placement a new overlay of this kind starts with.
    pub fn default_for(kind: OverlayKind) -> Placement {
        let (anchor, width) = match kind {
            OverlayKind::Text => (Anchor::Bottom, 60),
            OverlayKind::Clock => (Anchor::BottomRight, 20),
            OverlayKind::Picture | OverlayKind::Other => (Anchor::BottomRight, 12),
        };
        Placement { anchor, width, margin: 4 }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// A saved overlay in the library (`Data.overlays`); the library order is the drawing order.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Overlay {
    pub id: Uuid,
    pub name: String,
    pub source: OverlaySource,
    #[serde(default)]
    pub place: Placement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverlayKind {
    Text,
    Picture,
    Clock,
    Other,
}

impl OverlaySource {
    /// Whether the overlay stays up when Logo is pressed.
    pub fn survives_logo(&self) -> bool {
        match self {
            OverlaySource::Text { .. } | OverlaySource::Image { .. } | OverlaySource::Clock { .. } => true,
            OverlaySource::Unknown => false,
        }
    }

    pub fn kind(&self) -> OverlayKind {
        match self {
            OverlaySource::Text { .. } => OverlayKind::Text,
            OverlaySource::Image { .. } => OverlayKind::Picture,
            OverlaySource::Clock { .. } => OverlayKind::Clock,
            OverlaySource::Unknown => OverlayKind::Other,
        }
    }

    /// Bound sources compute their text when drawn (`bound_text`).
    pub fn is_bound(&self) -> bool {
        matches!(self, OverlaySource::Clock { .. })
    }
}

/// What bound text is computed from. Build it only with `BoundCtx::at`; FEAT-2 adds `timers`.
#[derive(Clone, Debug)]
pub struct BoundCtx {
    pub now: DateTime<Local>,
}

impl BoundCtx {
    pub fn at(now: DateTime<Local>) -> Self {
        BoundCtx { now }
    }
}

/// The text of a bound source at `ctx`; `None` for sources that are not bound.
pub fn bound_text(src: &OverlaySource, ctx: &BoundCtx) -> Option<String> {
    match src {
        OverlaySource::Clock { seconds, h24, .. } => {
            let format = match (*h24, *seconds) {
                (false, false) => "%-I:%M %p",
                (false, true) => "%-I:%M:%S %p",
                (true, false) => "%H:%M",
                (true, true) => "%H:%M:%S",
            };
            Some(ctx.now.format(format).to_string())
        }
        _ => None,
    }
}

/// Plan decision 7: size 20–120, weight snapped to 400/600/700, box opacity at most 100.
fn clamp_look(look: &mut OverlayLook) {
    look.size = look.size.clamp(20, 120);
    look.weight = match look.weight {
        w if w < 500 => 400,
        w if w < 650 => 600,
        _ => 700,
    };
    look.box_opacity = look.box_opacity.min(100);
}

/// Checks and tidies an overlay before `overlay/save` stores it.
pub fn validate(mut o: Overlay, asset_exists: impl Fn(&AssetId) -> bool) -> Result<Overlay, String> {
    o.name = o.name.trim().to_string();
    if o.name.is_empty() {
        return Err(strings::overlay_name_required().into());
    }
    match &mut o.source {
        OverlaySource::Text { text, styles, look } => {
            if text.trim().is_empty() {
                return Err(strings::overlay_text_required().into());
            }
            *styles = richtext::normalize(text, std::mem::take(styles));
            clamp_look(look);
        }
        OverlaySource::Image { source, opacity } => {
            if let ImageSource::Asset { asset } = source
                && !asset_exists(asset)
            {
                return Err(strings::picture_gone().into());
            }
            *opacity = (*opacity).min(100);
        }
        OverlaySource::Clock { look, .. } => clamp_look(look),
        OverlaySource::Unknown => return Err(strings::overlay_kind().into()),
    }
    o.place.width = o.place.width.clamp(5, 100);
    o.place.margin = o.place.margin.min(20);
    if o.id.is_nil() {
        o.id = Uuid::new_v4();
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::richtext::{RunStyle, StyleSpan};
    use crate::theme::strings::*;
    use chrono::{Local, TimeZone};
    use serde_json::json;

    fn text_overlay(name: &str, text: &str) -> Overlay {
        Overlay {
            id: Uuid::nil(),
            name: name.into(),
            source: OverlaySource::Text { text: text.into(), styles: Vec::new(), look: OverlayLook::default() },
            place: Placement::default_for(OverlayKind::Text),
        }
    }

    #[test]
    fn unknown_overlay_source_loads_as_unknown() {
        let s: OverlaySource = serde_json::from_str(r#"{"type":"hologram","x":1}"#).unwrap();
        assert_eq!(s, OverlaySource::Unknown);
        assert!(!s.survives_logo());
        assert_eq!(s.kind(), OverlayKind::Other);
    }

    #[test]
    fn logo_policy_per_source() {
        let text = OverlaySource::Text { text: "Hi".into(), styles: Vec::new(), look: OverlayLook::default() };
        let image = OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 };
        let clock = OverlaySource::Clock { look: OverlayLook::default(), seconds: false, h24: false };
        assert!(text.survives_logo() && image.survives_logo() && clock.survives_logo());
        assert_eq!((text.kind(), image.kind(), clock.kind()), (OverlayKind::Text, OverlayKind::Picture, OverlayKind::Clock));
        assert!(clock.is_bound());
        assert!(!text.is_bound() && !image.is_bound() && !OverlaySource::Unknown.is_bound());
    }

    #[test]
    fn slot_round_trips_and_image_opacity_defaults_full() {
        let s: OverlaySource = serde_json::from_str(r#"{"type":"image","source":{"kind":"church_logo"}}"#).unwrap();
        assert_eq!(s, OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 });
        let slot = OverlaySlot {
            key: saved_key(Uuid::new_v4()),
            z: 2,
            inst: 7,
            name: "Logo".into(),
            source: OverlaySource::Image { source: ImageSource::Asset { asset: "a.png".into() }, opacity: 80 },
            place: Placement { anchor: Anchor::TopRight, width: 15, margin: 3 },
        };
        let v = serde_json::to_value(&slot).unwrap();
        assert_eq!(v["source"]["source"], json!({"kind": "asset", "asset": "a.png"}));
        assert_eq!(v["place"]["anchor"], json!("top_right"));
        assert_eq!(serde_json::from_value::<OverlaySlot>(v).unwrap(), slot);
    }

    #[test]
    fn saved_key_round_trips() {
        let id = Uuid::new_v4();
        assert_eq!(saved_key(id), format!("saved:{id}"));
        assert_eq!(saved_id(&saved_key(id)), Some(id));
        assert_eq!(saved_id("message"), None);
        assert_eq!(saved_id("saved:not-a-uuid"), None);
    }

    #[test]
    fn clock_text_12_and_24_hour_edges() {
        let at = |h, m, s| BoundCtx::at(Local.with_ymd_and_hms(2026, 1, 4, h, m, s).unwrap());
        let clock = |seconds, h24| OverlaySource::Clock { look: OverlayLook::default(), seconds, h24 };
        assert_eq!(bound_text(&clock(false, false), &at(0, 0, 0)).unwrap(), "12:00 AM");
        assert_eq!(bound_text(&clock(false, false), &at(12, 0, 0)).unwrap(), "12:00 PM");
        assert_eq!(bound_text(&clock(false, false), &at(9, 5, 0)).unwrap(), "9:05 AM");
        assert_eq!(bound_text(&clock(true, false), &at(21, 5, 9)).unwrap(), "9:05:09 PM");
        assert_eq!(bound_text(&clock(false, true), &at(21, 5, 0)).unwrap(), "21:05");
        assert_eq!(bound_text(&clock(true, true), &at(23, 59, 59)).unwrap(), "23:59:59");
        assert_eq!(bound_text(&OverlaySource::Unknown, &at(1, 0, 0)), None);
        let text = OverlaySource::Text { text: "Hi".into(), styles: Vec::new(), look: OverlayLook::default() };
        assert_eq!(bound_text(&text, &at(1, 0, 0)), None);
    }

    #[test]
    fn clock_without_look_loads_with_defaults() {
        let s: OverlaySource = serde_json::from_str(r#"{"type":"clock"}"#).unwrap();
        assert_eq!(s, OverlaySource::Clock { look: OverlayLook::default(), seconds: false, h24: false });
        let t: OverlaySource = serde_json::from_str(r#"{"type":"text","text":"Hi"}"#).unwrap();
        assert!(matches!(t, OverlaySource::Text { ref look, ref styles, .. } if *look == OverlayLook::default() && styles.is_empty()));
        let look = OverlayLook::default();
        assert_eq!(
            (look.font.as_str(), look.size, look.color.as_str(), look.weight, look.shadow, look.box_kind, look.box_color.as_str(), look.box_opacity),
            ("", 48, "#ffffff", 600, true, BoxKind::None, "#000000", 60)
        );
    }

    #[test]
    fn overlay_without_place_loads_with_default_placement() {
        let o: Overlay = serde_json::from_value(json!({"id": Uuid::nil(), "name": "Logo", "source": {"type":"image","source":{"kind":"church_logo"}}})).unwrap();
        assert_eq!(o.place, Placement::default());
        assert_eq!(Placement::default(), Placement::default_for(OverlayKind::Picture));
    }

    #[test]
    fn default_placements_follow_kind() {
        assert_eq!(Placement::default_for(OverlayKind::Picture), Placement { anchor: Anchor::BottomRight, width: 12, margin: 4 });
        assert_eq!(Placement::default_for(OverlayKind::Clock), Placement { anchor: Anchor::BottomRight, width: 20, margin: 4 });
        assert_eq!(Placement::default_for(OverlayKind::Text), Placement { anchor: Anchor::Bottom, width: 60, margin: 4 });
    }

    #[test]
    fn overlay_save_validates() {
        let yes = |_: &AssetId| true;
        let no = |_: &AssetId| false;
        assert_eq!(validate(text_overlay("   ", "Hi"), yes).unwrap_err(), overlay_name_required());
        assert_eq!(validate(text_overlay("Wi-Fi", "  \n "), yes).unwrap_err(), overlay_text_required());
        let picture = Overlay {
            id: Uuid::nil(),
            name: "Logo".into(),
            source: OverlaySource::Image { source: ImageSource::Asset { asset: "gone.png".into() }, opacity: 100 },
            place: Placement::default(),
        };
        assert_eq!(validate(picture.clone(), no).unwrap_err(), picture_gone());
        assert!(validate(picture, yes).is_ok());
        let logo = Overlay { source: OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 250 }, ..text_overlay("Logo", "") };
        assert_eq!(validate(logo, no).unwrap().source, OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 });
        let unknown = Overlay { source: OverlaySource::Unknown, ..text_overlay("Odd", "") };
        assert_eq!(validate(unknown, yes).unwrap_err(), overlay_kind());

        let mut wide = text_overlay("  Wi-Fi  ", "Café ß");
        wide.place = Placement { anchor: Anchor::Top, width: 200, margin: 99 };
        let saved = validate(wide, yes).unwrap();
        assert_eq!(saved.name, "Wi-Fi");
        assert_eq!((saved.place.width, saved.place.margin), (100, 20));
        assert!(!saved.id.is_nil());
        let mut narrow = text_overlay("Wi-Fi", "Hi");
        narrow.place.width = 0;
        assert_eq!(validate(narrow, yes).unwrap().place.width, 5);
        let kept = Uuid::new_v4();
        assert_eq!(validate(Overlay { id: kept, ..text_overlay("A", "B") }, yes).unwrap().id, kept);
    }

    #[test]
    fn overlay_save_clamps_look_and_normalizes_styles() {
        let bold = RunStyle { bold: true, ..RunStyle::default() };
        let look = OverlayLook { size: 500, weight: 450, box_opacity: 200, ..OverlayLook::default() };
        let o = Overlay {
            source: OverlaySource::Text {
                text: "Café".into(),
                // 4 falls inside "é" (bytes 3..5); 0..0 is empty
                styles: vec![StyleSpan { start: 0, end: 0, style: bold.clone() }, StyleSpan { start: 1, end: 4, style: bold.clone() }],
                look,
            },
            ..text_overlay("Sign", "")
        };
        let OverlaySource::Text { styles, look, .. } = validate(o, |_| true).unwrap().source else { panic!("text") };
        assert_eq!((look.size, look.weight, look.box_opacity), (120, 400, 100));
        assert_eq!(styles, crate::theme::richtext::normalize("Café", vec![StyleSpan { start: 1, end: 4, style: bold }]));
        let clock = Overlay {
            source: OverlaySource::Clock { look: OverlayLook { size: 1, weight: 900, ..OverlayLook::default() }, seconds: false, h24: false },
            ..text_overlay("Clock", "")
        };
        let OverlaySource::Clock { look, .. } = validate(clock, |_| true).unwrap().source else { panic!("clock") };
        assert_eq!((look.size, look.weight), (20, 700));
    }
}
