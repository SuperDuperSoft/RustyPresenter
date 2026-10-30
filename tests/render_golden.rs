//! Slide rendering against stored reference images. Run with `UPDATE_GOLDENS=1` to rewrite them
//! after an intended change, then look at every changed PNG before committing.
use rustypresenter::model::Theme;
use rustypresenter::render::{Fonts, Rendered, TextSlide, parse_color, render_text};
use std::path::PathBuf;

fn theme() -> Theme { Theme { font: "Noto Sans".into(), ..Theme::default() } }

fn slide(text: &str) -> TextSlide<'_> { TextSlide { text, footer: None, reference: None, scripture: false } }

fn render_at(text: &str, canvas: (u32, u32)) -> Rendered { render_text(&mut Fonts::new(), &slide(text), &theme(), canvas) }

fn render(text: &str) -> Rendered { render_at(text, (1920, 1080)) }

/// Straight-alpha RGBA, as stored in the PNGs.
fn unpremultiply(r: &Rendered) -> Vec<u8> {
    r.rgba_premul.chunks(4).flat_map(|p| {
        let a = p[3] as u32;
        if a == 0 { [0, 0, 0, 0] } else { [(p[0] as u32 * 255 / a) as u8, (p[1] as u32 * 255 / a) as u8, (p[2] as u32 * 255 / a) as u8, p[3]] }
    }).collect()
}

/// Passes when the mean channel difference is at most 2 and at most 0.5% of pixels differ by more than 32.
fn check_golden(name: &str, r: &Rendered) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.png"));
    let got = unpremultiply(r);
    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::save_buffer(&path, &got, r.width, r.height, image::ColorType::Rgba8).unwrap();
        return;
    }
    let want = image::open(&path).unwrap_or_else(|e| panic!("{}: {e} (run with UPDATE_GOLDENS=1)", path.display())).to_rgba8();
    assert_eq!((want.width(), want.height()), (r.width, r.height), "{name}: size");
    let want = want.into_raw();
    let total: u64 = got.iter().zip(&want).map(|(a, b)| a.abs_diff(*b) as u64).sum();
    let mean = total as f64 / got.len() as f64;
    let far = got.chunks(4).zip(want.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 32)).count();
    let far_pct = far as f64 * 100.0 / (r.width * r.height) as f64;
    assert!(mean <= 2.0 && far_pct <= 0.5, "{name}: mean diff {mean:.2}, {far_pct:.2}% of pixels far off");
}

#[test]
fn golden_short_lyric() { check_golden("short_lyric", &render("Amazing grace, how sweet the sound\nThat saved a wretch like me")); }

#[test]
fn golden_long_lyric_fitted() {
    check_golden("long_lyric_fitted", &render(&"Through many dangers, toils and snares I have already come\n".repeat(6)));
}

#[test]
fn golden_scripture_with_reference() {
    let s = TextSlide {
        text: "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
        footer: None,
        reference: Some("John 3:16 KJV"),
        scripture: true,
    };
    check_golden("scripture_with_reference", &render_text(&mut Fonts::new(), &s, &theme(), (1920, 1080)));
}

#[test]
fn golden_lower_third() {
    let t = Theme { position: "lower".into(), ..theme() };
    let s = TextSlide { text: "Welcome to Grace First", footer: Some("Service starts at 10:30"), reference: None, scripture: false };
    check_golden("lower_third", &render_text(&mut Fonts::new(), &s, &t, (1920, 1080)));
}

#[test]
fn golden_uppercase_no_shadow() {
    let t = Theme { uppercase: true, shadow: false, color: "#ffd84a".into(), ..theme() };
    check_golden("uppercase_no_shadow", &render_text(&mut Fonts::new(), &slide("Be thou my vision"), &t, (1920, 1080)));
}

#[test]
fn empty_slide_is_fully_transparent() { assert!(render(" \n ").rgba_premul.iter().all(|&b| b == 0)); }

#[test]
fn non_latin_text_draws_pixels() { assert!(render("Sublime gracia del Señor 놀라운 은혜 🙏").rgba_premul.chunks(4).any(|p| p[3] > 0)); }

#[test]
fn bad_colors_fall_back_to_white() {
    assert_eq!(parse_color("#zz"), [255, 255, 255, 255]);
    assert_eq!(parse_color(""), [255, 255, 255, 255]);
    assert_eq!(parse_color("red"), [255, 0, 0, 255]);
    assert_eq!(parse_color("#0f0"), [0, 255, 0, 255]);
}

#[test]
fn thumbnail_scales_font_with_canvas() { assert_eq!(render_at("Amazing grace", (480, 270)).font_px, 21.0); }

#[test]
fn zero_and_huge_canvases_do_not_panic() {
    for c in [(0, 0), (0, 1080), (1920, 0), (100_000, 10)] {
        let r = render_at("Amazing grace", c);
        assert_eq!(r.rgba_premul.len(), (r.width * r.height * 4) as usize, "{c:?}");
    }
}
