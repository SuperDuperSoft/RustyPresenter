//! Slide rendering against stored reference images. Run with `UPDATE_GOLDENS=1` to rewrite them
//! after an intended change, then look at every changed PNG before committing.
//!
//! Every reference render uses `Fonts::bundled_only()`, so installed fonts (a system "Noto Sans"
//! of another version, say) cannot change the pixels.
use rustypresenter::model::Theme;
use rustypresenter::render::{Fonts, Rendered, TextSlide, parse_color, render_text};
use std::path::PathBuf;

fn theme() -> Theme { Theme { font: "Noto Sans".into(), ..Theme::default() } }

fn slide(text: &str) -> TextSlide<'_> { TextSlide { text, footer: None, reference: None, scripture: false, size: 0, passage: &[], parallel: None } }

fn render_at(text: &str, canvas: (u32, u32)) -> Rendered { render_text(&mut Fonts::bundled_only(), &slide(text), &theme(), canvas) }

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
        size: 0,
        passage: &[], parallel: None
    };
    check_golden("scripture_with_reference", &render_text(&mut Fonts::bundled_only(), &s, &theme(), (1920, 1080)));
}

#[test]
fn golden_lower_third() {
    let t = Theme { position: "lower".into(), ..theme() };
    let s = TextSlide { text: "Welcome to Grace First", footer: Some("Service starts at 10:30"), reference: None, scripture: false, size: 0, passage: &[], parallel: None };
    check_golden("lower_third", &render_text(&mut Fonts::bundled_only(), &s, &t, (1920, 1080)));
}

#[test]
fn golden_uppercase_no_shadow() {
    let t = Theme { uppercase: true, shadow: false, color: "#ffd84a".into(), ..theme() };
    check_golden("uppercase_no_shadow", &render_text(&mut Fonts::bundled_only(), &slide("Be thou my vision"), &t, (1920, 1080)));
}

#[test]
fn empty_slide_is_fully_transparent() { assert!(render(" \n ").rgba_premul.iter().all(|&b| b == 0)); }

#[test]
fn non_latin_text_draws_pixels() {
    // System fonts on purpose: this is about falling back to installed faces for other scripts.
    let r = render_text(&mut Fonts::new(), &slide("Sublime gracia del Señor 놀라운 은혜 🙏"), &theme(), (1920, 1080));
    assert!(r.rgba_premul.chunks(4).any(|p| p[3] > 0));
}

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

/// Prints how long a shadowed full-HD slide takes. Run with
/// `cargo test --release --test render_golden -- --ignored --nocapture bench_`.
#[test]
#[ignore]
fn bench_1080p_render_ms() {
    let mut fonts = Fonts::bundled_only();
    let lyric = slide("Amazing grace, how sweet the sound\nThat saved a wretch like me");
    let scripture = TextSlide {
        text: "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
        footer: None,
        reference: Some("John 3:16 KJV"),
        scripture: true,
        size: 0,
        passage: &[], parallel: None
    };
    for (name, s) in [("lyric", &lyric), ("scripture", &scripture)] {
        render_text(&mut fonts, s, &theme(), (1920, 1080)); // warm the shaping and glyph caches
        let n = 10;
        let start = std::time::Instant::now();
        for _ in 0..n { std::hint::black_box(render_text(&mut fonts, s, &theme(), (1920, 1080))); }
        println!("bench {name}: {:.1} ms per 1080p render", start.elapsed().as_secs_f64() * 1000.0 / n as f64);
    }
}

// ---- FEAT-1 theme options (spec §6.2): checked by what they change, on top of the unchanged goldens above ----

use rustypresenter::model::TextAlign;

fn render_with(text: &str, t: Theme) -> Rendered { render_text(&mut Fonts::bundled_only(), &slide(text), &t, (480, 270)) }

/// (left, top, right, bottom) of the pixels with any coverage.
fn ink_box(r: &Rendered) -> (u32, u32, u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, p) in r.rgba_premul.chunks(4).enumerate() {
        if p[3] < 128 { continue }
        let (x, y) = (i as u32 % r.width, i as u32 / r.width);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    (x0, y0, x1, y1)
}

fn ink(r: &Rendered) -> u64 { r.rgba_premul.chunks(4).map(|p| p[3] as u64).sum() }

fn plain() -> Theme { Theme { shadow: false, ..theme() } }

#[test]
fn default_theme_fields_keep_todays_pixels() {
    let t = Theme { weight: 600, align: TextAlign::Center, outline: 0, band: false, ..theme() };
    assert_eq!(render_text(&mut Fonts::bundled_only(), &slide("Amazing grace"), &t, (480, 270)).rgba_premul.to_vec(), render_at("Amazing grace", (480, 270)).rgba_premul.to_vec());
}

#[test]
fn bold_weight_draws_more_ink_than_semibold() {
    assert!(ink(&render_with("Amazing grace", Theme { weight: 700, ..plain() })) > ink(&render_with("Amazing grace", plain())));
    assert!(ink(&render_with("Amazing grace", Theme { weight: 400, ..plain() })) < ink(&render_with("Amazing grace", plain())));
}

#[test]
fn align_left_and_right() {
    let centre = ink_box(&render_with("Grace", plain()));
    let left = ink_box(&render_with("Grace", Theme { align: TextAlign::Left, ..plain() }));
    let right = ink_box(&render_with("Grace", Theme { align: TextAlign::Right, ..plain() }));
    assert!(left.0 < centre.0 && centre.2 < right.2, "{left:?} {centre:?} {right:?}");
    assert!(left.0 < 40 && right.2 > 440);
}

#[test]
fn position_top_starts_at_the_top() {
    let top = ink_box(&render_with("Grace", Theme { position: "top".into(), ..plain() }));
    let centre = ink_box(&render_with("Grace", plain()));
    assert!(top.1 < 40 && centre.1 > 90, "{top:?} {centre:?}");
}

#[test]
fn outline_draws_a_ring_in_its_colour() {
    let r = render_with("Grace", Theme { outline: 6, outline_color: "#ff0000".into(), ..plain() });
    assert!(r.rgba_premul.chunks(4).any(|p| p[3] > 200 && p[0] > 200 && p[1] < 60), "red ring");
    assert!(ink(&r) > ink(&render_with("Grace", plain())));
}

#[test]
fn text_band_sits_behind_the_text_and_replaces_the_lower_shade() {
    let t = Theme { band: true, band_color: "#0000ff".into(), band_opacity: 100, position: "lower".into(), ..plain() };
    let r = render_with("Grace", t);
    // The white letters' box (the band is blue, so it is not counted).
    let (mut x0, mut y0) = (u32::MAX, u32::MAX);
    for (i, p) in r.rgba_premul.chunks(4).enumerate() {
        if p[0] > 200 && p[1] > 200 && p[3] > 200 { (x0, y0) = (x0.min(i as u32 % r.width), y0.min(i as u32 / r.width)) }
    }
    let text = (x0, y0);
    let at = |x: u32, y: u32| { let i = ((y * r.width + x) * 4) as usize; [r.rgba_premul[i], r.rgba_premul[i + 1], r.rgba_premul[i + 2], r.rgba_premul[i + 3]] };
    assert_eq!(at(text.0.saturating_sub(4), text.1), [0, 0, 255, 255], "band left of the text");
    assert_eq!(at(2, 140)[3], 0, "no lower-third shade around the band");
    let shade = render_with("Grace", Theme { position: "lower".into(), ..plain() });
    assert!(shade.rgba_premul[((260 * shade.width + 2) * 4 + 3) as usize] > 0, "without a band the shade is there");
}

#[test]
fn lower_third_builtin_theme_renders_low_on_a_band() {
    let t = rustypresenter::theme::builtin().into_iter().find(|t| t.id == rustypresenter::theme::LOWER_THIRD_ID).unwrap().style;
    let b = ink_box(&render_with("Amazing grace", Theme { font: "Noto Sans".into(), ..t }));
    assert!(b.1 > 135, "{b:?}");
}
