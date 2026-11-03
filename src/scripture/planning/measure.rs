//! How much text fits on a scripture slide: the `Measure` trait, the theme facts planning
//! needs, and the deterministic character-count `CharMeasure` (spec §5.4, §5.5).
use crate::scripture::ParallelText;
use std::hash::Hash;

/// The smallest scripture font size, in px at 1080p scale.
pub const MIN_PX: f32 = 26.0;
/// The scripture size a theme with `scripture_size = 0` uses.
pub const DEFAULT_SCRIPTURE_SIZE: u32 = 64;

/// The only theme facts planning needs. Part B implements it for the resolved theme type,
/// so a real `FitSource` still receives the full theme it renders with.
pub trait PlanTheme: Clone + Hash + Send + Sync + 'static {
    /// The theme's scripture font size; 0 means [`DEFAULT_SCRIPTURE_SIZE`].
    fn scripture_size(&self) -> u32;
    fn is_lower_third(&self) -> bool;
}

/// How a slide's text fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Fits at a readable size or larger.
    Comfortable,
    /// Fits only between [`MIN_PX`] and the readable size.
    Shrunk,
    /// Does not fit even at [`MIN_PX`].
    TooLong,
}

/// Decides how a composed slide (main text plus optional second column) fits.
pub trait Measure {
    fn fits(&mut self, main: &str, second: Option<&ParallelText>) -> Fit;
}

/// `READABLE = max(0.75 × resolved scripture size, 40)` px.
pub fn readable_px(scripture_size: u32) -> f32 {
    let size = if scripture_size == 0 { DEFAULT_SCRIPTURE_SIZE } else { scripture_size };
    (0.75 * size as f32).max(40.0)
}

/// Character-count stand-in for the real layout: used in tests and on a plan-cache miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharMeasure {
    pub comfortable: usize,
    pub max: usize,
}

impl CharMeasure {
    pub fn with_limits(comfortable: usize, max: usize) -> Self { CharMeasure { comfortable, max } }

    /// 330 characters at a readable 48 px on a 16:9 canvas, scaled by font area and aspect.
    pub fn for_input<T: PlanTheme>(theme: &T, canvas: (u32, u32)) -> Self {
        let readable = readable_px(theme.scripture_size()) as f64;
        let (w, h) = (canvas.0.max(1) as f64, canvas.1.max(1) as f64);
        let aspect = (w / h) / (16.0 / 9.0);
        let comfortable = (330.0 * (48.0 / readable).powi(2) * aspect).round();
        let max = (comfortable * (readable / MIN_PX as f64).powi(2)).round();
        CharMeasure { comfortable: comfortable as usize, max: max as usize }
    }
}

impl Measure for CharMeasure {
    fn fits(&mut self, main: &str, second: Option<&ParallelText>) -> Fit {
        let main = main.chars().count();
        let count = second.map_or(main, |s| 2 * main.max(s.text.chars().count()));
        if count <= self.comfortable {
            Fit::Comfortable
        } else if count <= self.max {
            Fit::Shrunk
        } else {
            Fit::TooLong
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripture::ParallelLayout;
    use crate::scripture::test_support::TestTheme;

    #[test]
    fn char_measure_thresholds() {
        let mut m = CharMeasure::with_limits(60, 120);
        assert_eq!(
            (m.fits(&"a".repeat(60), None), m.fits(&"a".repeat(61), None), m.fits(&"a".repeat(120), None), m.fits(&"a".repeat(121), None)),
            (Fit::Comfortable, Fit::Shrunk, Fit::Shrunk, Fit::TooLong)
        );
        let p = ParallelText { main_code: "KJV".into(), second_code: "NIV".into(), text: "b".repeat(31), layout: ParallelLayout::SideBySide };
        assert_eq!(m.fits(&"a".repeat(10), Some(&p)), Fit::Shrunk); // 2 × max(10, 31) = 62
    }

    #[test]
    fn readable_uses_resolved_scripture_size() {
        assert_eq!((readable_px(0), readable_px(64), readable_px(40), readable_px(100)), (48.0, 48.0, 40.0, 75.0));
    }

    #[test]
    fn char_measure_for_default_theme_is_330_chars() {
        let m = CharMeasure::for_input(&TestTheme::default(), (1920, 1080));
        assert_eq!((m.comfortable, m.max), (330, 1125)); // 330 × (48 / 26)² = 1124.7
    }
}
