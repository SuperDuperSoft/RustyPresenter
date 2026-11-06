//! Where slide text goes and how big it is. Mirrors the rules of the web renderer (`web/render.js`):
//! a 1920-wide logical canvas, text shrinks until it fits, and words are never dropped.
use crate::model::Theme;
use cosmic_text::{Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, Wrap};

/// Logical canvas width every size below is measured against.
pub const CANVAS_W: f32 = 1920.0;
const PAD_X: f32 = 110.0;
const PAD_Y: f32 = 66.0;
const PAD_BOTTOM_LOWER: f32 = 70.0;
const FOOT_PAD_BOTTOM: f32 = 46.0;
const REFERENCE_PX: f32 = 44.0;
const FOOTER_PX: f32 = 30.0;
/// Line height of the reference/footer line, relative to its font size.
const FOOT_LINE: f32 = 1.25;
const MIN_PX: f32 = 26.0;
/// Largest canvas side drawn. Bigger (or zero) sizes are clamped so a bad size can never crash rendering.
pub const MAX_CANVAS: u32 = 8192;

/// `canvas` with each side kept within 1..=`MAX_CANVAS`.
pub fn clamp_canvas(canvas: (u32, u32)) -> (u32, u32) { (canvas.0.clamp(1, MAX_CANVAS), canvas.1.clamp(1, MAX_CANVAS)) }
pub const BUNDLED_FAMILY: &str = "Noto Sans";
/// The web renderer's defaults for theme values left empty or zero (`t.font || 'Segoe UI'` etc.).
const DEFAULT_FAMILY: &str = "Segoe UI";
const DEFAULT_SIZE: u32 = 84;
const DEFAULT_SCRIPTURE_SIZE: u32 = 64;

/// The bundled Noto Sans faces (regular 400, medium 500, semibold 600).
const BUNDLED_FACES: [&[u8]; 3] = [
    include_bytes!("../../assets/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../../assets/fonts/NotoSans-Medium.ttf"),
    include_bytes!("../../assets/fonts/NotoSans-SemiBold.ttf"),
];

/// The font database: system fonts plus the bundled Noto Sans, so text looks the same everywhere.
pub struct Fonts { pub(crate) sys: FontSystem }

impl Fonts {
    /// Installed system fonts plus the bundled Noto Sans. What the app uses: theme fonts the
    /// operator installed work, and other scripts (Korean, emoji, ...) fall back to system faces.
    pub fn new() -> Self {
        let mut sys = FontSystem::new();
        for face in BUNDLED_FACES { sys.db_mut().load_font_data(face.to_vec()); }
        Self { sys }
    }

    /// Only the bundled Noto Sans, with a fixed locale: the same pixels on every machine, whatever
    /// is installed (a system "Noto Sans" of another version cannot change the result). For
    /// reference-image tests; text outside Noto Sans' coverage draws as missing-glyph boxes.
    pub fn bundled_only() -> Self {
        let mut db = cosmic_text::fontdb::Database::new();
        for face in BUNDLED_FACES { db.load_font_data(face.to_vec()); }
        for set in [cosmic_text::fontdb::Database::set_sans_serif_family, cosmic_text::fontdb::Database::set_serif_family,
                    cosmic_text::fontdb::Database::set_monospace_family] {
            set(&mut db, BUNDLED_FAMILY);
        }
        Self { sys: FontSystem::new_with_locale_and_db("en-US".into(), db) }
    }

    pub fn has_family(&self, name: &str) -> bool {
        self.sys.db().faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
    }
}

impl Default for Fonts { fn default() -> Self { Self::new() } }

/// The text parts of a slide (`live::SlideLayer::Text`).
#[derive(Clone, Copy, Debug)]
pub struct TextSlide<'a> {
    pub text: &'a str,
    pub footer: Option<&'a str>,
    pub reference: Option<&'a str>,
    pub scripture: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

pub struct TextLayout {
    /// Body font size in canvas pixels after fitting.
    pub font_px: f32,
    /// The theme's font is not installed; the bundled one was used.
    pub font_fallback: bool,
    pub family: String,
    /// The body text, shaped and wrapped to `body_box.w`. Its height is not limited: nothing is cut.
    pub body: Buffer,
    /// Reference (scripture) or footer line, shaped to the canvas width minus padding.
    pub foot: Option<Buffer>,
    /// Where the foot line's top edge sits, in canvas pixels.
    pub foot_y: f32,
    pub body_box: Rect,
    /// Lower-third position: text sits at the bottom over a dark gradient.
    pub lower: bool,
    pub scripture: bool,
    pub canvas: (u32, u32),
}

/// Font sizes to try, largest first: −4 px above 60 px, then −2 px, ending at 26 px.
pub fn fit_steps(max: f32) -> impl Iterator<Item = f32> {
    let mut next = Some(max.max(MIN_PX));
    std::iter::from_fn(move || {
        let cur = next?;
        let step = if cur > 60.0 { 4.0 } else { 2.0 };
        next = if cur <= MIN_PX { None } else { Some((cur - step).max(MIN_PX)) };
        Some(cur)
    })
}

/// Lay out a text slide on a `canvas` (width, height) in pixels. All sizes scale by width / 1920.
pub fn layout(fonts: &mut Fonts, slide: &TextSlide, theme: &Theme, canvas: (u32, u32)) -> TextLayout {
    let canvas = clamp_canvas(canvas);
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    let s = cw / CANVAS_W;
    let lower = theme.position == "lower";
    // Like render.js `t.font || 'Segoe UI'`; a blank name is no font either.
    let wanted = if theme.font.trim().is_empty() { DEFAULT_FAMILY } else { theme.font.as_str() };
    let font_fallback = !fonts.has_family(wanted);
    let family = if font_fallback { BUNDLED_FAMILY.to_string() } else { wanted.to_string() };

    // render.js shows `layer.reference || layer.footer`: a blank reference gives way to the footer.
    let filled = |t: &&str| !t.trim().is_empty();
    let reference = slide.reference.filter(filled);
    let foot_text = reference.or(slide.footer.filter(filled));
    let (foot, foot_h) = match foot_text {
        Some(t) => {
            let is_ref = reference.is_some();
            let (px, weight) = if is_ref { (REFERENCE_PX, 600) } else { (FOOTER_PX, 400) };
            let align = if is_ref { Align::Right } else { Align::Center };
            let px = px * s;
            let mut b = Buffer::new(&mut fonts.sys, Metrics::new(px, px * FOOT_LINE));
            let attrs = Attrs::new().family(Family::Name(&family)).weight(Weight(weight));
            b.set_size(Some(cw - 2.0 * PAD_X * s), None);
            b.set_wrap(Wrap::WordOrGlyph);
            b.set_text(t, &attrs, Shaping::Advanced, Some(align));
            b.shape_until_scroll(&mut fonts.sys, false);
            let h = text_height(&b) + FOOT_PAD_BOTTOM * s;
            (Some(b), h)
        }
        None => (None, 0.0),
    };

    let top = PAD_Y * s;
    let bottom = if lower { PAD_BOTTOM_LOWER } else { PAD_Y } * s;
    let body_box = Rect { x: PAD_X * s, y: top, w: cw - 2.0 * PAD_X * s, h: (ch - top - bottom - foot_h).max(0.0) };

    let text = if theme.uppercase && !slide.scripture { slide.text.to_uppercase() } else { slide.text.to_string() };
    let (weight, line, align, max) = if slide.scripture {
        (500, 1.32, Align::Left, or_default(theme.scripture_size, DEFAULT_SCRIPTURE_SIZE))
    } else {
        (600, 1.22, Align::Center, or_default(theme.size, DEFAULT_SIZE))
    };
    let attrs = Attrs::new().family(Family::Name(&family)).weight(Weight(weight));
    let mut body = Buffer::new(&mut fonts.sys, Metrics::new(max * s, max * s * line));
    body.set_size(Some(body_box.w), None);
    body.set_wrap(Wrap::WordOrGlyph);
    body.set_text(&text, &attrs, Shaping::Advanced, Some(align));

    let mut font_px = MIN_PX;
    for px in fit_steps(max) {
        body.set_metrics(Metrics::new(px * s, px * s * line));
        body.shape_until_scroll(&mut fonts.sys, false);
        font_px = px;
        // Like render.js, measure against the padded box: its clientHeight includes the vertical padding.
        if text_height(&body) <= body_box.h + top + bottom - 4.0 * s && max_line_width(&body) <= body_box.w + 1.0 { break; }
    }

    TextLayout {
        font_px,
        font_fallback,
        family,
        body,
        foot,
        foot_y: ch - foot_h,
        body_box,
        lower,
        scripture: slide.scripture,
        canvas,
    }
}

/// A theme size, or the web's default when it is 0 (`t.size || 84`).
fn or_default(size: u32, default: u32) -> f32 { (if size == 0 { default } else { size }) as f32 }

/// Total height of all laid-out lines.
pub fn text_height(b: &Buffer) -> f32 { b.layout_runs().map(|r| r.line_height).sum() }

/// Width of the widest laid-out line.
pub fn max_line_width(b: &Buffer) -> f32 { b.layout_runs().map(|r| r.line_w).fold(0.0, f32::max) }

#[cfg(test)]
mod tests {
    use super::*;

    fn slide(text: &str) -> TextSlide<'_> { TextSlide { text, footer: None, reference: None, scripture: false } }

    fn lay(text: &str) -> TextLayout {
        let mut t = Theme::default();
        t.font = BUNDLED_FAMILY.into();
        layout(&mut Fonts::new(), &slide(text), &t, (1920, 1080))
    }

    /// Words in the laid-out lines, read back from the glyphs that were placed.
    fn laid_out_words(b: &Buffer) -> usize {
        b.layout_runs().map(|r| {
            let (Some(first), Some(last)) = (r.glyphs.first(), r.glyphs.last()) else { return 0 };
            r.text[first.start.min(last.start)..first.end.max(last.end)].split_whitespace().count()
        }).sum()
    }

    #[test]
    fn fit_steps_match_render_js() {
        let s: Vec<f32> = fit_steps(84.0).collect();
        assert_eq!(&s[..7], &[84., 80., 76., 72., 68., 64., 60.]);
        assert_eq!(s[7], 58.);
        assert_eq!(*s.last().unwrap(), 26.);
    }

    #[test]
    fn short_lyric_keeps_theme_size() { assert_eq!(lay("Amazing grace").font_px, 84.0); }

    /// web/render.js measures against the padded box (clientHeight includes padding), so ten
    /// lines at 84 px (1025 px) still fit a 1080 px screen there.
    #[test]
    fn fit_threshold_includes_vertical_padding_like_the_web() {
        assert_eq!(lay(&vec!["Amazing grace"; 10].join("
")).font_px, 84.0);
    }

    #[test]
    fn long_lyric_shrinks_but_not_below_26() {
        let l = lay(&"How sweet the sound that saved a wretch like me ".repeat(12));
        assert!(l.font_px < 84.0 && l.font_px >= 26.0, "{}", l.font_px);
    }

    #[test]
    fn overlong_lyric_stops_at_26_and_keeps_every_word() {
        let l = lay(&"word ".repeat(2000));
        assert_eq!(l.font_px, 26.0);
        assert_eq!(laid_out_words(&l.body), 2000);
    }

    #[test]
    fn unbreakable_word_wraps_inside_the_box() {
        let l = lay(&"x".repeat(400));
        assert!(max_line_width(&l.body) <= l.body_box.w + 1.0);
    }

    #[test]
    fn missing_font_falls_back_to_bundled() {
        let mut t = Theme::default();
        t.font = "No Such Font 123".into();
        let l = layout(&mut Fonts::new(), &slide("Hi"), &t, (1920, 1080));
        assert!(l.font_fallback);
        assert_eq!(l.family, BUNDLED_FAMILY);
    }

    // render.js reads theme values with `||` defaults, so empty/zero values mean "the default".

    #[test]
    fn empty_theme_font_means_segoe_ui_like_the_web() {
        for font in ["", "   "] {
            let mut fonts = Fonts::new();
            let want = if fonts.has_family("Segoe UI") { "Segoe UI" } else { BUNDLED_FAMILY };
            let t = Theme { font: font.into(), ..Theme::default() };
            let l = layout(&mut fonts, &slide("Hi"), &t, (1920, 1080));
            assert_eq!(l.family, want, "font {font:?}");
            assert_eq!(l.font_fallback, want == BUNDLED_FAMILY, "font {font:?}");
        }
    }

    #[test]
    fn empty_theme_font_without_segoe_ui_falls_back_to_bundled() {
        let t = Theme { font: String::new(), ..Theme::default() };
        let l = layout(&mut Fonts::bundled_only(), &slide("Hi"), &t, (1920, 1080));
        assert!(l.font_fallback);
        assert_eq!(l.family, BUNDLED_FAMILY);
    }

    #[test]
    fn zero_theme_size_means_84_like_the_web() {
        let t = Theme { font: BUNDLED_FAMILY.into(), size: 0, ..Theme::default() };
        assert_eq!(layout(&mut Fonts::bundled_only(), &slide("Amazing grace"), &t, (1920, 1080)).font_px, 84.0);
    }

    #[test]
    fn zero_scripture_size_means_64_like_the_web() {
        let t = Theme { font: BUNDLED_FAMILY.into(), scripture_size: 0, ..Theme::default() };
        let s = TextSlide { text: "For God so loved", footer: None, reference: None, scripture: true };
        assert_eq!(layout(&mut Fonts::bundled_only(), &s, &t, (1920, 1080)).font_px, 64.0);
    }

    #[test]
    fn blank_reference_shows_the_footer_like_the_web() {
        let t = Theme { font: BUNDLED_FAMILY.into(), ..Theme::default() };
        for reference in ["", "  "] {
            let s = TextSlide { text: "Welcome", footer: Some("Service at 10:30"), reference: Some(reference), scripture: false };
            let l = layout(&mut Fonts::bundled_only(), &s, &t, (1920, 1080));
            let foot = l.foot.as_ref().unwrap_or_else(|| panic!("reference {reference:?}: footer missing"));
            assert_eq!(foot.metrics().font_size, FOOTER_PX, "reference {reference:?}: footer style");
            let run = foot.layout_runs().next().unwrap();
            assert_eq!(run.text, "Service at 10:30");
            // Footers are centered; a right-aligned line would end at the right padding.
            let mid = run.glyphs.first().unwrap().x + run.line_w / 2.0;
            assert!((mid - l.body_box.w / 2.0).abs() < 2.0, "reference {reference:?}: footer centered, mid {mid}");
        }
    }

    #[test]
    fn reference_wins_over_footer() {
        let t = Theme { font: BUNDLED_FAMILY.into(), ..Theme::default() };
        let s = TextSlide { text: "For God", footer: Some("Footer"), reference: Some("John 3:16"), scripture: true };
        let l = layout(&mut Fonts::bundled_only(), &s, &t, (1920, 1080));
        let foot = l.foot.as_ref().unwrap();
        assert_eq!(foot.metrics().font_size, REFERENCE_PX);
        assert_eq!(foot.layout_runs().next().unwrap().text, "John 3:16");
    }

    #[test]
    fn bundled_only_holds_just_the_three_noto_sans_faces() {
        let f = Fonts::bundled_only();
        let faces: Vec<_> = f.sys.db().faces().collect();
        assert_eq!(faces.len(), 3);
        assert!(faces.iter().all(|face| face.families.iter().any(|(n, _)| n == BUNDLED_FAMILY)));
        let mut weights: Vec<u16> = faces.iter().map(|face| face.weight.0).collect();
        weights.sort();
        assert_eq!(weights, [400, 500, 600]);
    }

    #[test]
    fn bundled_only_lays_out_with_noto_sans() {
        let l = layout(&mut Fonts::bundled_only(), &slide("Amazing grace"), &Theme::default(), (1920, 1080));
        assert!(l.font_fallback, "Segoe UI is never in the bundled-only database");
        assert_eq!(l.family, BUNDLED_FAMILY);
        assert_eq!(l.font_px, 84.0);
    }

    #[test]
    fn scripture_uses_scripture_size_and_left_align() {
        let mut t = Theme::default();
        t.font = BUNDLED_FAMILY.into();
        let l = layout(&mut Fonts::new(), &TextSlide { text: "For God so loved", footer: None, reference: Some("John 3:16"), scripture: true }, &t, (1920, 1080));
        assert_eq!(l.font_px, 64.0);
        assert!(l.foot.is_some());
        assert!(l.foot_y < 1080.0 && l.foot_y > l.body_box.y + l.body_box.h - 1.0);
    }
}
