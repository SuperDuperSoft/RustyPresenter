//! Where slide text goes and how big it is. Mirrors the rules of the web renderer (`web/render.js`):
//! a 1920-wide logical canvas, text shrinks until it fits, and words are never dropped.
use crate::model::{PassageSlide, Theme};
use cosmic_text::{Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, Wrap};
use std::collections::HashMap;

/// Logical canvas width every size below is measured against.
pub const CANVAS_W: f32 = 1920.0;
const PAD_X: f32 = 110.0;
const PAD_Y: f32 = 66.0;
const PAD_BOTTOM_LOWER: f32 = 70.0;
const FOOT_PAD_BOTTOM: f32 = 46.0;
pub(super) const REFERENCE_PX: f32 = 44.0;
const FOOTER_PX: f32 = 30.0;
/// Space between the reference and the credit under it.
const FOOT_GAP: f32 = 8.0;
/// Line height of the reference/footer line, relative to its font size.
const FOOT_LINE: f32 = 1.25;
pub(super) const MIN_PX: f32 = 26.0;
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
pub struct Fonts {
    pub(crate) sys: FontSystem,
    /// The fitted size of passage slides already measured, so drawing each slide of a passage does not measure
    /// all the others again.
    fits: HashMap<FitKey, f32>,
}

/// Everything that decides the fitted size of one passage slide.
#[derive(PartialEq, Eq, Hash)]
struct FitKey { slide: PassageSlide, scripture: bool, size: u32, theme: Theme, canvas: (u32, u32) }

/// The most fitted sizes kept before the memo starts again.
const MAX_FITS: usize = 1024;

impl Fonts {
    /// Installed system fonts plus the bundled Noto Sans. What the app uses: theme fonts the
    /// operator installed work, and other scripts (Korean, emoji, ...) fall back to system faces.
    pub fn new() -> Self {
        let mut sys = FontSystem::new();
        for face in BUNDLED_FACES { sys.db_mut().load_font_data(face.to_vec()); }
        Self { sys, fits: HashMap::new() }
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
        Self { sys: FontSystem::new_with_locale_and_db("en-US".into(), db), fits: HashMap::new() }
    }

    /// Whether `text` shapes with a missing-glyph box somewhere: no bundled or installed face (system fallback
    /// included) has a glyph for one of its characters. Spaces and invisible format marks (zero-width joiners,
    /// direction marks, soft hyphens) are not checked. Settings → Bible warns with this (Library spec §9.1).
    pub fn lacks_glyphs(&mut self, text: &str) -> bool {
        let text: String = text.chars().map(|c| if is_invisible(c) { ' ' } else { c }).collect();
        if text.trim().is_empty() { return false }
        let mut b = Buffer::new(&mut self.sys, Metrics::new(32.0, 40.0));
        b.set_wrap(Wrap::None);
        b.set_text(&text, &Attrs::new().family(Family::Name(BUNDLED_FAMILY)), Shaping::Advanced, None);
        b.shape_until_scroll(&mut self.sys, false);
        b.layout_runs().any(|r| r.glyphs.iter().any(|g| g.glyph_id == 0 && r.text.get(g.start..g.end).is_some_and(|s| !s.trim().is_empty())))
    }

    pub fn has_family(&self, name: &str) -> bool {
        self.sys.db().faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
    }

    /// The size `p` fits at, laid out like `slide` but on its own.
    fn passage_fit(&mut self, p: &PassageSlide, slide: &TextSlide, theme: &Theme, canvas: (u32, u32)) -> f32 {
        let key = FitKey { slide: p.clone(), scripture: slide.scripture, size: slide.size, theme: theme.clone(), canvas };
        if let Some(px) = self.fits.get(&key) { return *px }
        let alone = TextSlide { text: &p.text, footer: p.footer.as_deref(), reference: p.reference.as_deref(), scripture: slide.scripture, size: slide.size, passage: &[], parallel: None };
        let px = layout(self, &alone, theme, canvas).font_px;
        if self.fits.len() >= MAX_FITS { self.fits.clear() }
        self.fits.insert(key, px);
        px
    }
}

impl Default for Fonts { fn default() -> Self { Self::new() } }

/// Characters that draw nothing and that a face may map to no glyph: white space, controls, zero-width joiners and
/// spaces, direction marks and embeddings, word joiners, the byte-order mark, soft hyphens and variation selectors.
fn is_invisible(c: char) -> bool {
    c.is_whitespace() || c.is_control()
        || matches!(c, '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}' | '\u{E0100}'..='\u{E01EF}')
}

/// The text parts of a slide (`live::SlideLayer::Text`).
#[derive(Clone, Copy, Debug)]
pub struct TextSlide<'a> {
    pub text: &'a str,
    pub footer: Option<&'a str>,
    pub reference: Option<&'a str>,
    pub scripture: bool,
    /// Largest text size in pixels at 1080p; 0 = the theme's (`size` or `scripture_size`).
    pub size: u32,
    /// The slides of the passage this one belongs to (it included): the text gets the largest size that fits
    /// every one of them. Empty fits this slide alone.
    pub passage: &'a [PassageSlide],
    /// More Bibles beside or under the main text (a parallel scripture slide).
    pub parallel: Option<&'a crate::scripture::Parallel>,
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
    /// The footer when a reference has the foot line (a passage's credit): one line under the reference, centred.
    pub credit: Option<Buffer>,
    pub credit_y: f32,
    pub body_box: Rect,
    /// Lower-third position: text sits at the bottom over a dark gradient.
    pub lower: bool,
    pub scripture: bool,
    pub canvas: (u32, u32),
    /// A parallel scripture slide: every Bible's column (the main one too); `body` is then empty and unused.
    pub columns: Option<super::parallel::Columns>,
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

    // A reference shows right-aligned at the foot. A footer alone takes its place, centred; with a reference it goes
    // under it (Bible library spec §9.2: a passage's credit and its reference show together).
    let filled = |t: &&str| !t.trim().is_empty();
    let reference = slide.reference.filter(filled);
    let footer = slide.footer.filter(filled);
    let line_buffer = |fonts: &mut Fonts, t: &str, is_ref: bool, one_line: bool| {
        let (px, weight) = if is_ref { (REFERENCE_PX, 600) } else { (FOOTER_PX, 400) };
        let align = if is_ref { Align::Right } else { Align::Center };
        let px = px * s;
        let mut b = Buffer::new(&mut fonts.sys, Metrics::new(px, px * FOOT_LINE));
        let attrs = Attrs::new().family(Family::Name(&family)).weight(Weight(weight));
        b.set_size(Some(cw - 2.0 * PAD_X * s), None);
        b.set_wrap(Wrap::WordOrGlyph);
        if one_line { set_one_line(&mut fonts.sys, &mut b, t, &attrs, align) } else {
            b.set_text(t, &attrs, Shaping::Advanced, Some(align));
            b.shape_until_scroll(&mut fonts.sys, false);
        }
        b
    };
    // A scripture credit is cut to one line on the slide; the full notice is on the credits page (spec §10).
    let credit = reference.and(footer).map(|t| line_buffer(fonts, t, false, slide.scripture));
    let foot = reference.or(footer).map(|t| line_buffer(fonts, t, reference.is_some(), false));
    let credit_h = credit.as_ref().map_or(0.0, |c| text_height(c) + FOOT_GAP * s);
    let foot_h = foot.as_ref().map_or(0.0, |f| text_height(f) + FOOT_PAD_BOTTOM * s + credit_h);
    let credit_y = ch - FOOT_PAD_BOTTOM * s - credit.as_ref().map_or(0.0, text_height);

    let top = PAD_Y * s;
    let bottom = if lower { PAD_BOTTOM_LOWER } else { PAD_Y } * s;
    let body_box = Rect { x: PAD_X * s, y: top, w: cw - 2.0 * PAD_X * s, h: (ch - top - bottom - foot_h).max(0.0) };

    let text = if theme.uppercase && !slide.scripture { slide.text.to_uppercase() } else { slide.text.to_string() };
    let (weight, line, align, max) = if slide.scripture {
        (500, 1.32, Align::Left, or_default(theme.scripture_size, DEFAULT_SCRIPTURE_SIZE))
    } else {
        (600, 1.22, Align::Center, or_default(theme.size, DEFAULT_SIZE))
    };
    // Like the Settings range (`app.rs` keeps theme sizes within 24–200), so a bad size cannot stall fitting.
    let max = if slide.size > 0 { slide.size.clamp(24, 200) as f32 } else { max };
    // One size for the whole passage: never above what its fullest slide fits. Every fitted size is a step of
    // `fit_steps(max)`, so fitting from it continues the same steps.
    let max = slide.passage.iter().fold(max, |m, p| m.min(fonts.passage_fit(p, slide, theme, canvas)));
    let attrs = Attrs::new().family(Family::Name(&family)).weight(Weight(weight));
    let mut body = Buffer::new(&mut fonts.sys, Metrics::new(max * s, max * s * line));
    body.set_size(Some(body_box.w), None);
    body.set_wrap(Wrap::WordOrGlyph);
    body.set_text(&text, &attrs, Shaping::Advanced, Some(align));

    if let Some(p) = slide.parallel.filter(|p| !p.also.is_empty()) {
        let frame = super::parallel::Frame { family: &family, scale: s, canvas: (cw, ch), body_box, max_px: max, line, reference_px: REFERENCE_PX, lower };
        let cols = super::parallel::columns(fonts, &frame, &super::parallel::texts(&text, p, lower), p.layout);
        return TextLayout {
            font_px: cols.font_px, font_fallback, family, body, foot, foot_y: ch - foot_h, credit, credit_y, body_box, lower,
            scripture: slide.scripture, canvas, columns: Some(cols),
        };
    }

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
        credit,
        credit_y,
        body_box,
        lower,
        scripture: slide.scripture,
        canvas,
        columns: None,
    }
}

/// The size a passage's text shows at (`TextSlide::passage`): the largest that fits every one of its slides, at
/// most `size` (0 = the theme's scripture size). On 1080p-canvas pixels, like `TextLayout::font_px`.
pub fn passage_px(fonts: &mut Fonts, passage: &[PassageSlide], size: u32, theme: &Theme, canvas: (u32, u32)) -> f32 {
    let first = passage.first().map_or("", |p| p.text.as_str());
    let slide = TextSlide { text: first, footer: passage.first().and_then(|p| p.footer.as_deref()), reference: passage.first().and_then(|p| p.reference.as_deref()), scripture: true, size, passage, parallel: None };
    layout(fonts, &slide, theme, canvas).font_px
}

/// A theme size, or the web's default when it is 0 (`t.size || 84`).
fn or_default(size: u32, default: u32) -> f32 { (if size == 0 { default } else { size }) as f32 }

/// `text` in `b` on one line: cut at a character with "…" after it when it is wider than the buffer.
fn set_one_line(sys: &mut FontSystem, b: &mut Buffer, text: &str, attrs: &Attrs, align: Align) {
    let mut put = |b: &mut Buffer, t: &str| {
        b.set_text(t, attrs, Shaping::Advanced, Some(align));
        b.shape_until_scroll(sys, false);
        b.layout_runs().count() <= 1
    };
    if put(b, text) { return }
    let chars: Vec<char> = text.chars().collect();
    let cut = |n: usize| format!("{}…", chars[..n].iter().collect::<String>().trim_end());
    // The longest start that still fits with the ellipsis; one character at least.
    let (mut lo, mut hi) = (1, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if put(b, &cut(mid)) { lo = mid } else { hi = mid - 1 }
    }
    put(b, &cut(lo));
}

/// Total height of all laid-out lines.
pub fn text_height(b: &Buffer) -> f32 { b.layout_runs().map(|r| r.line_height).sum() }

/// Width of the widest laid-out line.
pub fn max_line_width(b: &Buffer) -> f32 { b.layout_runs().map(|r| r.line_w).fold(0.0, f32::max) }

#[cfg(test)]
mod tests {
    use super::*;

    fn slide(text: &str) -> TextSlide<'_> { TextSlide { text, footer: None, reference: None, scripture: false, size: 0, passage: &[], parallel: None } }

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
        let s = TextSlide { text: "For God so loved", footer: None, reference: None, scripture: true, size: 0, passage: &[], parallel: None };
        assert_eq!(layout(&mut Fonts::bundled_only(), &s, &t, (1920, 1080)).font_px, 64.0);
    }

    #[test]
    fn blank_reference_shows_the_footer_like_the_web() {
        let t = Theme { font: BUNDLED_FAMILY.into(), ..Theme::default() };
        for reference in ["", "  "] {
            let s = TextSlide { text: "Welcome", footer: Some("Service at 10:30"), reference: Some(reference), scripture: false, size: 0, passage: &[], parallel: None };
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
    fn reference_and_footer_show_together_the_credit_under_the_reference() {
        let t = Theme { font: BUNDLED_FAMILY.into(), ..Theme::default() };
        let s = TextSlide { text: "For God", footer: Some("Footer"), reference: Some("John 3:16"), scripture: true, size: 0, passage: &[], parallel: None };
        let l = layout(&mut Fonts::bundled_only(), &s, &t, (1920, 1080));
        let foot = l.foot.as_ref().unwrap();
        assert_eq!(foot.metrics().font_size, REFERENCE_PX);
        assert_eq!(foot.layout_runs().next().unwrap().text, "John 3:16");
        let credit = l.credit.as_ref().expect("the footer shows under the reference");
        assert_eq!(credit.metrics().font_size, FOOTER_PX);
        assert_eq!(credit.layout_runs().next().unwrap().text, "Footer");
        assert!(l.foot_y + text_height(foot) <= l.credit_y, "reference {} ends above the credit at {}", l.foot_y + text_height(foot), l.credit_y);
        assert!(l.credit_y + text_height(credit) <= 1080.0 - FOOT_PAD_BOTTOM + 0.5, "credit inside the bottom padding");
        assert!(l.body_box.y + l.body_box.h <= l.foot_y + 0.5, "the text box ends above the reference");
        let alone = layout(&mut Fonts::bundled_only(), &TextSlide { footer: None, ..s }, &t, (1920, 1080));
        assert!(alone.credit.is_none());
        assert!(l.body_box.h < alone.body_box.h, "the credit takes room from the text");
    }

    #[test]
    fn a_long_scripture_credit_is_cut_to_one_line_with_an_ellipsis() {
        let t = Theme { font: BUNDLED_FAMILY.into(), ..Theme::default() };
        let notice = "Copyright © 2026 Some Bible Society. Used by permission. All rights reserved worldwide. ".repeat(4);
        let s = TextSlide { text: "For God", footer: Some(&notice), reference: Some("John 3:16 (XYZ)"), scripture: true, size: 0, passage: &[], parallel: None };
        let l = layout(&mut Fonts::bundled_only(), &s, &t, (1920, 1080));
        let credit = l.credit.as_ref().unwrap();
        let lines: Vec<_> = credit.layout_runs().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.ends_with('…') && lines[0].text.starts_with("Copyright © 2026"), "{}", lines[0].text);
        assert!(lines[0].text.chars().count() > 40, "cut no shorter than needed: {}", lines[0].text);
        // A song's footer (no reference) still wraps: nothing of the CCLI line is lost.
        let song = layout(&mut Fonts::bundled_only(), &TextSlide { reference: None, scripture: false, ..s }, &t, (1920, 1080));
        assert!(song.foot.as_ref().unwrap().layout_runs().count() > 1);
    }

    #[test]
    fn a_passage_fits_its_credit_slide_at_the_shared_size() {
        let t = Theme { font: BUNDLED_FAMILY.into(), ..Theme::default() };
        let (reference, credit) = (Some("Psalm 1:1 (XYZ)"), Some("XYZ: Copyright notice"));
        let mut fonts = Fonts::bundled_only();
        let alone = |fonts: &mut Fonts, text: &str, footer| layout(fonts, &TextSlide { text, footer, reference, scripture: true, size: 0, passage: &[], parallel: None }, &t, (1920, 1080)).font_px;
        // A length where the credit's line costs the text a size step.
        let text = (20..400).step_by(5).map(|n| "word ".repeat(n))
            .find(|text| alone(&mut fonts, text, credit) < alone(&mut fonts, text, None)).expect("a length where the credit matters");
        let want = alone(&mut fonts, &text, credit);
        let parts = [
            PassageSlide { text: text.clone(), reference: reference.map(String::from), footer: credit.map(String::from) },
            PassageSlide { text: text.clone(), reference: reference.map(String::from), footer: None },
        ];
        for p in &parts {
            let s = TextSlide { text: &p.text, footer: p.footer.as_deref(), reference: p.reference.as_deref(), scripture: true, size: 0, passage: &parts, parallel: None };
            assert_eq!(layout(&mut fonts, &s, &t, (1920, 1080)).font_px, want, "every slide at the credit slide's size");
        }
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

    fn bundled() -> Theme { Theme { font: BUNDLED_FAMILY.into(), ..Theme::default() } }

    fn scripture(text: &str, size: u32, passage: &[PassageSlide]) -> f32 {
        let s = TextSlide { text, footer: None, reference: Some("Psalm 119:1–2 (KJV)"), scripture: true, size, passage, parallel: None };
        layout(&mut Fonts::bundled_only(), &s, &bundled(), (1920, 1080)).font_px
    }

    fn part(text: &str) -> PassageSlide { PassageSlide { text: text.into(), reference: Some("Psalm 119:1–2 (KJV)".into()), footer: None } }

    #[test]
    fn a_passage_whose_second_slide_is_longer_makes_the_first_smaller_too() {
        let short = "Blessed are the undefiled in the way.";
        let long = "Blessed are they that keep his testimonies, and that seek him with the whole heart. ".repeat(12);
        let alone = (scripture(short, 0, &[]), scripture(&long, 0, &[]));
        assert_eq!(alone.0, 64.0, "on its own the short slide keeps the scripture size");
        assert!(alone.1 < 64.0, "the long slide shrinks: {}", alone.1);
        let passage = [part(short), part(&long)];
        assert_eq!(scripture(short, 0, &passage), alone.1, "one size for the whole passage");
        assert_eq!(scripture(&long, 0, &passage), alone.1);
    }

    #[test]
    fn a_passage_that_fits_keeps_its_size() {
        let passage = [part("For God so loved the world."), part("That whosoever believeth.")];
        assert_eq!(scripture("For God so loved the world.", 0, &passage), 64.0);
    }

    #[test]
    fn passage_size_is_the_largest_and_never_above_it() {
        assert_eq!(scripture("For God so loved the world.", 96, &[]), 96.0, "the passage's own size wins over the theme's");
        let passage = [part("For God so loved the world."), part("That whosoever believeth.")];
        assert_eq!(scripture("For God so loved the world.", 40, &passage), 40.0);
        assert_eq!(scripture("Hi", 1000, &[]), 200.0, "a bad size is kept within range");
    }

    #[test]
    fn songs_without_a_passage_fit_slide_by_slide() {
        let mut fonts = Fonts::bundled_only();
        let long = "How sweet the sound that saved a wretch like me ".repeat(8);
        let t = bundled();
        assert!(layout(&mut fonts, &slide(&long), &t, (1920, 1080)).font_px < 84.0);
        assert_eq!(layout(&mut fonts, &slide("Amazing grace"), &t, (1920, 1080)).font_px, 84.0);
    }

    #[test]
    fn scripture_uses_scripture_size_and_left_align() {
        let mut t = Theme::default();
        t.font = BUNDLED_FAMILY.into();
        let l = layout(&mut Fonts::new(), &TextSlide { text: "For God so loved", footer: None, reference: Some("John 3:16"), scripture: true, size: 0, passage: &[], parallel: None }, &t, (1920, 1080));
        assert_eq!(l.font_px, 64.0);
        assert!(l.foot.is_some());
        assert!(l.foot_y < 1080.0 && l.foot_y > l.body_box.y + l.body_box.h - 1.0);
    }

    #[test]
    fn lacks_glyphs_finds_a_script_no_face_covers() {
        let mut f = Fonts::bundled_only();
        for covered in ["Reina Valera 1909", "Ἐν ἀρχῇ ἦν ὁ λόγος", "Синодальный перевод", "Kinh Thánh Tiếng Việt", "", "  \t"] {
            assert!(!f.lacks_glyphs(covered), "{covered:?}: Noto Sans covers it");
        }
        assert!(!f.lacks_glyphs("Bible\u{200D}\u{200F}\u{FEFF}\u{00AD} 2020"), "invisible marks are not checked");
        for missing in ["성경전서 개역개정판", "Holy Bible 新译本", "መጽሐፍ ቅዱስ"] {
            assert!(f.lacks_glyphs(missing), "{missing:?}: Noto Sans has no glyph for it and no other face is loaded");
        }
    }
}
