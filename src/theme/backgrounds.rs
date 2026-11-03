//! Background values (colour, picture, video, gradient, each with a Darken value) and the
//! background library entries (spec §4.3). Backgrounds are values: items, themes and settings
//! store a `Background`, never an entry id.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{AssetId, MediaStatus};

/// The strongest Darken value, in percent.
pub const MAX_DIM: u8 = 80;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Background {
    Color {
        color: String,
    },
    Image {
        asset: AssetId,
        /// Darken, 0–80 %
        #[serde(default, skip_serializing_if = "is_zero")]
        dim: u8,
    },
    Video {
        asset: AssetId,
        #[serde(default, skip_serializing_if = "is_zero")]
        dim: u8,
    },
    Gradient {
        from: String,
        to: String,
        /// CSS `linear-gradient` angle in degrees; used mod 360.
        #[serde(default)]
        angle: u16,
        #[serde(default, skip_serializing_if = "is_zero")]
        dim: u8,
    },
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

impl Background {
    /// The Darken value; a plain colour has none.
    pub fn dim(&self) -> u8 {
        match self {
            Background::Color { .. } => 0,
            Background::Image { dim, .. } | Background::Video { dim, .. } | Background::Gradient { dim, .. } => *dim,
        }
    }

    /// The same background with another Darken value, at most `MAX_DIM`. A colour is returned unchanged.
    pub fn with_dim(mut self, value: u8) -> Self {
        match &mut self {
            Background::Color { .. } => {}
            Background::Image { dim, .. } | Background::Video { dim, .. } | Background::Gradient { dim, .. } => {
                *dim = value.min(MAX_DIM);
            }
        }
        self
    }

    pub fn asset(&self) -> Option<&AssetId> {
        match self {
            Background::Image { asset, .. } | Background::Video { asset, .. } => Some(asset),
            Background::Color { .. } | Background::Gradient { .. } => None,
        }
    }

    /// True when both show the same thing, whatever their Darken values.
    pub fn same_source(&self, other: &Background) -> bool {
        self.clone().with_dim(0) == other.clone().with_dim(0)
    }

    /// The gradient angle mod 360; 0 for anything else.
    pub fn angle_deg(&self) -> u16 {
        match self {
            Background::Gradient { angle, .. } => angle % 360,
            _ => 0,
        }
    }
}

/// One entry in the background library.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BackgroundEntry {
    pub id: Uuid,
    pub name: String,
    pub background: Background,
    /// Videos may be Converting or Failed; only Ready entries can be picked.
    #[serde(default = "ready")]
    pub status: MediaStatus,
    /// Video: a frame at 1 s; otherwise none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<AssetId>,
    #[serde(with = "rfc3339")]
    pub added: DateTime<Utc>,
}

fn ready() -> MediaStatus {
    MediaStatus::Ready
}

impl BackgroundEntry {
    pub fn new(name: &str, background: Background, status: MediaStatus, now: DateTime<Utc>) -> Self {
        Self { id: Uuid::new_v4(), name: name.to_string(), background, status, thumb: None, added: now }
    }

    pub fn pickable(&self) -> bool {
        self.status == MediaStatus::Ready
    }
}

/// The built-in backgrounds: the six colour swatches, then the six gradients.
pub fn builtin_backgrounds() -> Vec<(&'static str, Background)> {
    let color = |c: &str| Background::Color { color: c.into() };
    let gradient = |from: &str, to: &str, angle: u16| Background::Gradient { from: from.into(), to: to.into(), angle, dim: 0 };
    vec![
        ("Black", color("#000000")),
        ("Night blue", color("#0b1a33")),
        ("Dark green", color("#1d2b1f")),
        ("Plum", color("#2b1530")),
        ("Brown", color("#3a2412")),
        ("White", color("#ffffff")),
        ("Midnight", gradient("#0b1a33", "#000000", 180)),
        ("Deep purple", gradient("#2b1530", "#0d0f13", 135)),
        ("Forest", gradient("#1d2b1f", "#050805", 180)),
        ("Ember", gradient("#3a2412", "#0d0806", 135)),
        ("Ocean", gradient("#0e3a4f", "#06141c", 180)),
        ("Slate", gradient("#2a303b", "#0d0f13", 180)),
    ]
}

/// The name for an imported file: its name without the last extension, or "Background" when empty.
pub fn file_stem_name(file_name: &str) -> String {
    let stem = match file_name.rfind('.') {
        Some(i) if i > 0 => &file_name[..i],
        _ => file_name,
    };
    let stem = stem.trim();
    if stem.is_empty() { crate::theme::strings::UNNAMED_BACKGROUND.to_string() } else { stem.to_string() }
}

/// RFC 3339 for `DateTime<Utc>` (the chrono `serde` feature is off).
pub mod rfc3339 {
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(t: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&t.to_rfc3339_opts(SecondsFormat::Secs, true))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        let s = String::deserialize(d)?;
        DateTime::parse_from_rfc3339(&s).map(|t| t.with_timezone(&Utc)).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn v02_background_json_loads_with_zero_dim_and_reserializes_identically() {
        for j in [r##"{"type":"color","color":"#000000"}"##, r#"{"type":"image","asset":"ab.png"}"#, r#"{"type":"video","asset":"cd.mp4"}"#] {
            let b: Background = serde_json::from_str(j).unwrap();
            assert_eq!(b.dim(), 0);
            assert_eq!(serde_json::to_string(&b).unwrap(), j);
        }
    }

    #[test]
    fn gradient_round_trips_and_angle_wraps() {
        let g = Background::Gradient { from: "#0b1a33".into(), to: "#000000".into(), angle: 400, dim: 30 };
        assert_eq!(serde_json::from_value::<Background>(serde_json::to_value(&g).unwrap()).unwrap(), g);
        assert_eq!(g.angle_deg(), 40);
    }

    #[test]
    fn with_dim_clamps_to_80_and_ignores_colors() {
        let img = Background::Image { asset: "a.png".into(), dim: 0 }.with_dim(95);
        assert_eq!(img.dim(), MAX_DIM);
        assert_eq!(img, Background::Image { asset: "a.png".into(), dim: 80 });
        let c = Background::Color { color: "#000000".into() };
        assert_eq!(c.clone().with_dim(50), c);
        assert_eq!(c.dim(), 0);
    }

    #[test]
    fn same_source_ignores_dim() {
        let a0 = Background::Image { asset: "a.png".into(), dim: 0 };
        let a50 = Background::Image { asset: "a.png".into(), dim: 50 };
        let v = Background::Video { asset: "a.png".into(), dim: 0 };
        assert!(a0.same_source(&a50));
        assert!(!a0.same_source(&v));
        assert_eq!(a0.asset().map(String::as_str), Some("a.png"));
        assert_eq!(Background::Color { color: "#000000".into() }.asset(), None);
    }

    #[test]
    fn builtin_backgrounds_match_plan3_swatches_and_six_gradients() {
        let b = builtin_backgrounds();
        let colors: Vec<_> = b.iter().filter_map(|(_, x)| match x { Background::Color { color } => Some(color.as_str()), _ => None }).collect();
        assert_eq!(colors, ["#000000", "#0b1a33", "#1d2b1f", "#2b1530", "#3a2412", "#ffffff"]);
        let grads: Vec<_> = b.iter().filter(|(_, x)| matches!(x, Background::Gradient { .. })).map(|(n, _)| *n).collect();
        assert_eq!(grads, ["Midnight", "Deep purple", "Forest", "Ember", "Ocean", "Slate"]);
        assert!(b.contains(&("Ember", Background::Gradient { from: "#3a2412".into(), to: "#0d0806".into(), angle: 135, dim: 0 })));
    }

    #[test]
    fn background_entry_added_is_rfc3339_and_status_defaults_ready() {
        let e = BackgroundEntry::new("Cross", Background::Image { asset: "a.png".into(), dim: 0 }, MediaStatus::Ready, Utc.with_ymd_and_hms(2026, 11, 1, 9, 30, 0).unwrap());
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["added"], "2026-11-01T09:30:00Z");
        let mut v2 = v.clone();
        v2.as_object_mut().unwrap().remove("status");
        assert_eq!(serde_json::from_value::<BackgroundEntry>(v2).unwrap().status, MediaStatus::Ready);
    }

    #[test]
    fn converting_entry_is_not_pickable() {
        let mut e = BackgroundEntry::new("Clip", Background::Video { asset: "c.mp4".into(), dim: 0 }, MediaStatus::Converting { percent: 40 }, Utc::now());
        assert!(!e.pickable());
        e.status = MediaStatus::Ready;
        assert!(e.pickable());
    }

    #[test]
    fn file_stem_name_drops_only_the_last_extension() {
        assert_eq!(file_stem_name("Easter Cross.final.jpg"), "Easter Cross.final");
        assert_eq!(file_stem_name(""), "Background");
    }
}
