//! Taking part of a picture out (design `docs/superpowers/specs/2026-11-17-scene-extras-design.md`): a chosen colour
//! ("green screen"), or the dark or the light parts. `Cutout` is made once for a layer; for each pixel it then says
//! how much of it stays, from a small table, so a 1080p camera costs a few additions and one table look-up a pixel.
use serde::{Deserialize, Serialize};

/// What a layer takes out of its picture (cameras, pictures and videos).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(tag = "out", rename_all = "snake_case")]
pub enum Key {
    /// A colour, `#rrggbb`: `closeness` (0–100) how near a colour must be to go, `soft` (0–100) how gently the edge
    /// fades between what goes and what stays.
    Colour { #[serde(default)] colour: String, #[serde(default)] closeness: u8, #[serde(default)] soft: u8 },
    /// The parts darker than `level` (0 black – 100 white).
    Dark { #[serde(default)] level: u8, #[serde(default)] soft: u8 },
    /// The parts lighter than `level`.
    Light { #[serde(default)] level: u8, #[serde(default)] soft: u8 },
    /// Nothing taken out (also anything a newer RustyPresenter made).
    #[default]
    #[serde(other)]
    Nothing,
}

/// The green of a green screen, as a new colour key starts.
pub const GREEN: &str = "#00b140";

impl Key {
    pub fn is_nothing(&self) -> bool { *self == Key::Nothing }

    /// The choices the editor offers, in order: nothing, a colour, the dark parts, the light parts.
    pub fn index(&self) -> usize { match self { Key::Nothing => 0, Key::Colour { .. } => 1, Key::Dark { .. } => 2, Key::Light { .. } => 3 } }

    /// Choice `i` of `index`, keeping what `self` holds when it is the same kind.
    pub fn changed_to(&self, i: usize) -> Key {
        if self.index() == i { return self.clone() }
        match i {
            1 => Key::Colour { colour: GREEN.into(), closeness: 40, soft: 10 },
            2 => Key::Dark { level: 15, soft: 10 },
            3 => Key::Light { level: 85, soft: 10 },
            _ => Key::Nothing,
        }
    }

    /// How close and how soft (both 0–100).
    pub fn amounts(&self) -> (u8, u8) {
        match self {
            Key::Colour { closeness, soft, .. } => (*closeness, *soft),
            Key::Dark { level, soft } | Key::Light { level, soft } => (*level, *soft),
            Key::Nothing => (0, 0),
        }
    }

    /// The same key with `amount` (closeness or level) and `soft` (both kept within 0–100).
    pub fn with_amounts(&self, amount: u8, soft: u8) -> Key {
        let (amount, soft) = (amount.min(100), soft.min(100));
        match self {
            Key::Colour { colour, .. } => Key::Colour { colour: colour.clone(), closeness: amount, soft },
            Key::Dark { .. } => Key::Dark { level: amount, soft },
            Key::Light { .. } => Key::Light { level: amount, soft },
            Key::Nothing => Key::Nothing,
        }
    }
}

/// The colour difference that counts as "as far as can be" for a colour key's closeness and softness.
const FAR: f32 = 128.0;
/// Biggest squared colour difference (each of the two colour parts differs by at most 255).
const MOST: usize = 2 * 255 * 255;

/// A key made ready.
#[derive(Clone, Debug)]
pub struct Cutout {
    by: By,
    /// How much of a pixel stays (0–255): by squared colour difference, or by brightness.
    table: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
enum By { Colour { cb: i32, cr: i32 }, Bright }

/// The two colour parts of a pixel (YCbCr's Cb and Cr): brightness does not change them much, so a shadow on a
/// green screen is still green.
fn chroma(r: i32, g: i32, b: i32) -> (i32, i32) { ((-43 * r - 85 * g + 128 * b) >> 8, (128 * r - 107 * g - 21 * b) >> 8) }

fn bright(r: u32, g: u32, b: u32) -> u32 { (77 * r + 150 * g + 29 * b) >> 8 }

/// 0 up to `from`, 255 from `from + soft`, a straight line between (`soft` 0: a hard edge).
fn ramp(v: f32, from: f32, soft: f32) -> u8 {
    if v < from { 0 } else if soft <= 0.0 || v >= from + soft { 255 } else { ((v - from) / soft * 255.0).round() as u8 }
}

impl Cutout {
    /// `None` for a key that takes nothing out.
    pub fn new(key: &Key) -> Option<Cutout> {
        let pct = |v: u8| f32::from(v.min(100)) / 100.0;
        match key {
            Key::Nothing => None,
            Key::Colour { colour, closeness, soft } => {
                let [r, g, b] = super::own_scenes::rgb(colour);
                let (cb, cr) = chroma(i32::from(r), i32::from(g), i32::from(b));
                let (from, soft) = (pct(*closeness) * FAR, pct(*soft) * FAR);
                let table = (0..=MOST).map(|d2| ramp((d2 as f32).sqrt(), from, soft)).collect();
                Some(Cutout { by: By::Colour { cb, cr }, table })
            }
            Key::Dark { level, soft } => {
                let (from, soft) = (pct(*level) * 255.0, pct(*soft) * 255.0);
                Some(Cutout { by: By::Bright, table: (0..256).map(|y| ramp(y as f32, from, soft)).collect() })
            }
            Key::Light { level, soft } => {
                let (to, soft) = (pct(*level) * 255.0, pct(*soft) * 255.0);
                // Mirrored: what is lighter than the level goes.
                Some(Cutout { by: By::Bright, table: (0..256).map(|y| ramp(255.0 - y as f32, 255.0 - to, soft)).collect() })
            }
        }
    }

    /// How much of the (opaque, or straight) pixel `px` stays: 0 taken out, 255 kept.
    #[inline]
    pub fn keep(&self, px: &[u8]) -> u32 {
        match self.by {
            By::Colour { cb, cr } => {
                let (b, r) = chroma(i32::from(px[0]), i32::from(px[1]), i32::from(px[2]));
                let (db, dr) = (b - cb, r - cr);
                u32::from(self.table[((db * db + dr * dr) as usize).min(MOST)])
            }
            By::Bright => u32::from(self.table[bright(u32::from(px[0]), u32::from(px[1]), u32::from(px[2])) as usize]),
        }
    }

    /// Takes the key out of a straight (not premultiplied) RGBA picture, in its see-through.
    pub fn apply_straight(&self, rgba: &mut [u8]) {
        for p in rgba.chunks_exact_mut(4) {
            let k = self.keep(p);
            if k < 255 { p[3] = ((u32::from(p[3]) * k + 127) / 255) as u8 }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_green_screen_goes_and_the_person_stays() {
        let c = Cutout::new(&Key::Colour { colour: GREEN.into(), closeness: 40, soft: 10 }).unwrap();
        assert_eq!(c.keep(&[0, 177, 64, 255]), 0, "the screen itself");
        assert_eq!(c.keep(&[0, 90, 32, 255]), 0, "the screen in shadow is still green");
        assert_eq!(c.keep(&[220, 170, 140, 255]), 255, "skin");
        assert_eq!(c.keep(&[30, 30, 30, 255]), 255, "a dark coat");
        assert_eq!(c.keep(&[200, 30, 30, 255]), 255, "red");
    }

    #[test]
    fn a_soft_edge_lets_part_of_a_nearly_green_pixel_show() {
        let hard = Cutout::new(&Key::Colour { colour: GREEN.into(), closeness: 30, soft: 0 }).unwrap();
        let soft = Cutout::new(&Key::Colour { colour: GREEN.into(), closeness: 30, soft: 60 }).unwrap();
        // Half way between the green screen and grey.
        let between = [64, 150, 96, 255];
        assert!(matches!(hard.keep(&between), 0 | 255), "a hard edge: all or nothing");
        let k = soft.keep(&between);
        assert!(k > 0 && k < 255, "part of it: {k}");
    }

    #[test]
    fn the_dark_or_the_light_parts_go() {
        let dark = Cutout::new(&Key::Dark { level: 20, soft: 0 }).unwrap();
        assert_eq!((dark.keep(&[5, 5, 5, 255]), dark.keep(&[200, 200, 200, 255])), (0, 255));
        let light = Cutout::new(&Key::Light { level: 80, soft: 0 }).unwrap();
        assert_eq!((light.keep(&[250, 250, 250, 255]), light.keep(&[40, 40, 40, 255])), (0, 255));
        let soft = Cutout::new(&Key::Dark { level: 20, soft: 20 }).unwrap();
        let k = soft.keep(&[77, 77, 77, 255]);
        assert!(k > 0 && k < 255, "between the level and the soft edge: {k}");
    }

    #[test]
    fn a_picture_keeps_its_own_see_through_too() {
        let c = Cutout::new(&Key::Colour { colour: "#ffffff".into(), closeness: 10, soft: 0 }).unwrap();
        let mut px = vec![255, 255, 255, 255, 0, 0, 200, 128];
        c.apply_straight(&mut px);
        assert_eq!((px[3], px[7]), (0, 128));
    }

    #[test]
    fn keys_are_kept_and_newer_ones_read_as_nothing() {
        let k = Key::Colour { colour: GREEN.into(), closeness: 40, soft: 10 };
        let v = serde_json::to_value(&k).unwrap();
        assert_eq!(v, serde_json::json!({ "out": "colour", "colour": GREEN, "closeness": 40, "soft": 10 }));
        assert_eq!(serde_json::from_value::<Key>(v).unwrap(), k);
        assert_eq!(serde_json::from_value::<Key>(serde_json::json!({ "out": "hologram" })).unwrap(), Key::Nothing);
        assert_eq!(Key::Nothing.changed_to(2), Key::Dark { level: 15, soft: 10 });
        assert_eq!(k.changed_to(1), k, "the same kind keeps its colour");
        assert_eq!(k.with_amounts(120, 5), Key::Colour { colour: GREEN.into(), closeness: 100, soft: 5 });
    }
}
