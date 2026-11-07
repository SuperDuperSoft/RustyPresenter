//! Settings → Video → Graphics: which Slint renderer draws the windows (`choose_backend`), which choices this build
//! offers, and what is tried when the chosen one cannot be used (Auto, then Software).
//!
//! OpenGL is FemtoVG, which draws video under the scene through OpenGL (`DrawMode::OpenGl`). Vulkan, Direct3D (12)
//! and Metal are FemtoVG on wgpu, told which one through `require_wgpu_30` (feature `gpu-apis`, pure Rust),
//! or else Skia (feature `renderer-skia`, `require_vulkan` and so on). Both draw video into an image, like Software.
use super::surface::DrawMode;
use crate::{model::Graphics, tr};

/// The graphics interface a renderer is asked to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api { Any, Vulkan, Direct3D, Metal }

/// A Slint renderer (its `renderer_name`) and the interface it must use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Renderer { pub name: &'static str, pub api: Api }

/// What this build has: the optional renderers, and the system it is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Build { pub wgpu: bool, pub skia: bool, pub windows: bool, pub mac: bool }

impl Build {
    pub fn this() -> Build {
        Build {
            wgpu: cfg!(feature = "gpu-apis"),
            skia: cfg!(feature = "renderer-skia"),
            windows: cfg!(windows),
            mac: cfg!(target_os = "macos"),
        }
    }
}

/// FemtoVG on wgpu, which `select` steers with `require_wgpu_30`.
const WGPU: &str = "femtovg-wgpu";

/// Every choice, in the picker's order.
pub const ALL: [Graphics; 6] = [Graphics::Auto, Graphics::OpenGl, Graphics::Vulkan, Graphics::Direct3D, Graphics::Metal, Graphics::Software];

/// The renderer that gives `g` in build `b`, or `None` when the build can't. wgpu is preferred to Skia.
pub fn renderer(g: Graphics, b: Build) -> Option<Renderer> {
    let gpu = |api| {
        let name = if b.wgpu { WGPU } else if b.skia { "skia" } else { return None };
        Some(Renderer { name, api })
    };
    match g {
        Graphics::Auto | Graphics::OpenGl => Some(Renderer { name: "femtovg", api: Api::Any }),
        Graphics::Software => Some(Renderer { name: "software", api: Api::Any }),
        Graphics::Vulkan if !b.mac => gpu(Api::Vulkan),
        Graphics::Direct3D if b.windows => gpu(Api::Direct3D),
        Graphics::Metal if b.mac => gpu(Api::Metal),
        _ => None,
    }
}

/// The choices the picker shows: those this build can draw with.
pub fn available(b: Build) -> Vec<Graphics> {
    ALL.into_iter().filter(|g| renderer(*g, b).is_some()).collect()
}

/// What to try, in order: `want`, then Auto, then Software, leaving out what this build can't do. Software (safe
/// mode) never tries the graphics card.
pub fn attempts(want: Graphics, b: Build) -> Vec<Graphics> {
    if want == Graphics::Software { return vec![Graphics::Software] }
    let mut v: Vec<Graphics> = vec![];
    for g in [want, Graphics::Auto, Graphics::Software] {
        if !v.contains(&g) && renderer(g, b).is_some() { v.push(g) }
    }
    v
}

/// How video is drawn with a Slint renderer: under the scene through OpenGL, or into an image.
pub fn draw_mode(renderer: &str) -> DrawMode {
    if matches!(renderer, "femtovg" | "gl" | "skia-opengl") { DrawMode::OpenGl } else { DrawMode::Software }
}

/// Developers' environment variables, which beat the setting.
#[derive(Debug, PartialEq)]
pub enum Env {
    /// Neither is set: the setting decides.
    None,
    /// `RP_FORCE_SOFTWARE=1`.
    Software,
    /// `RP_RENDERER`: any Slint renderer by name (femtovg, femtovg-wgpu, skia, skia-opengl, skia-software, software).
    Renderer(String),
}

pub fn env(force_software: Option<&str>, renderer: Option<&str>) -> Env {
    match (force_software, renderer) {
        (Some("1"), _) => Env::Software,
        (_, Some(r)) if !r.is_empty() => Env::Renderer(r.into()),
        _ => Env::None,
    }
}

/// The choice a renderer named in `RP_RENDERER` stands for, as near as it can be told.
fn graphics_of(renderer: &str) -> Graphics {
    match renderer {
        "femtovg" | "gl" | "skia-opengl" => Graphics::OpenGl,
        "software" | "sw" | "skia-software" => Graphics::Software,
        _ => Graphics::Auto,
    }
}

/// What `choose_backend` picked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chosen {
    pub graphics: Graphics,
    pub draw: DrawMode,
    /// What was asked for, when it could not be used.
    pub fell_back_from: Option<Graphics>,
}

static CHOSEN: std::sync::OnceLock<Chosen> = std::sync::OnceLock::new();

/// Picks Slint's backend for `want` (the setting, or Software in safe mode), falling back to Auto and then Software.
/// `RP_FORCE_SOFTWARE=1` and `RP_RENDERER` override it. Call once, before any window.
pub fn choose_backend(want: Graphics) -> Chosen {
    let force = std::env::var("RP_FORCE_SOFTWARE").ok();
    let named = std::env::var("RP_RENDERER").ok();
    let chosen = match env(force.as_deref(), named.as_deref()) {
        Env::None => pick(want, Build::this()),
        Env::Software => pick(Graphics::Software, Build::this()),
        Env::Renderer(name) => match select(&name, Api::Any) {
            Ok(()) => Chosen { graphics: graphics_of(&name), draw: draw_mode(&name), fell_back_from: None },
            Err(e) => {
                eprintln!("{name} drawing unavailable ({e}); drawing in software");
                pick(Graphics::Software, Build::this())
            }
        },
    };
    let _ = CHOSEN.set(chosen);
    chosen
}

fn pick(want: Graphics, b: Build) -> Chosen {
    for g in attempts(want, b) {
        let Some(r) = renderer(g, b) else { continue };
        match select(r.name, r.api) {
            Ok(()) => return Chosen { graphics: g, draw: draw_mode(r.name), fell_back_from: (g != want).then_some(want) },
            Err(e) => eprintln!("{} graphics unavailable ({e})", r.name),
        }
    }
    // No backend at all (no display): opening a window says so.
    Chosen { graphics: Graphics::Software, draw: DrawMode::Software, fell_back_from: None }
}

fn select(name: &str, api: Api) -> Result<(), slint::PlatformError> {
    let sel = slint::BackendSelector::new().backend_name("winit".into()).renderer_name(name.into());
    let sel = match api {
        Api::Any => sel,
        #[cfg(feature = "gpu-apis")]
        api if name == WGPU => sel.require_wgpu_30(wgpu_only(api)),
        Api::Vulkan => sel.require_vulkan(),
        Api::Direct3D => sel.require_d3d(),
        Api::Metal => sel.require_metal(),
    };
    sel.select()
}

/// wgpu with just `api`'s backend; the rest as Slint sets it.
#[cfg(feature = "gpu-apis")]
fn wgpu_only(api: Api) -> i_slint_core::graphics::wgpu_30::api::WGPUConfiguration {
    use i_slint_core::graphics::wgpu_30::api::{wgpu::Backends, WGPUConfiguration, WGPUSettings};
    let mut settings = WGPUSettings::default();
    settings.backends = match api {
        Api::Vulkan => Backends::VULKAN,
        Api::Direct3D => Backends::DX12,
        Api::Metal => Backends::METAL,
        Api::Any => settings.backends,
    };
    WGPUConfiguration::Automatic(settings)
}

/// What `choose_backend` picked; Software when it was never called (tests, other backends).
pub fn chosen() -> Chosen {
    CHOSEN.get().copied().unwrap_or(Chosen { graphics: Graphics::Software, draw: DrawMode::Software, fell_back_from: None })
}

/// How video is drawn (see `chosen`).
pub fn drawing() -> DrawMode { chosen().draw }

// ---- What the operator reads ----

/// A choice's name, in sentences.
pub fn name(g: Graphics) -> String {
    match g {
        Graphics::Auto => tr!("Automatic"),
        Graphics::OpenGl => "OpenGL".into(),
        Graphics::Vulkan => "Vulkan".into(),
        Graphics::Direct3D => "Direct3D".into(),
        Graphics::Metal => "Metal".into(),
        Graphics::Software => tr!("Software"),
    }
}

/// A choice as the picker lists it.
pub fn label(g: Graphics) -> String {
    match g {
        Graphics::Auto => tr!("Automatic (recommended)"),
        Graphics::Software => tr!("Software (safe mode)"),
        g => name(g),
    }
}

pub fn in_use(g: Graphics) -> String { tr!("In use now: {mode}", mode = name(g)) }

pub fn restart_to_use() -> String { tr!("Restart RustyPresenter to use the new graphics setting.") }

pub fn fell_back(wanted: Graphics, used: Graphics) -> String {
    tr!("The {wanted} graphics mode could not be used on this computer, so RustyPresenter is using {mode}.", wanted = name(wanted), mode = name(used))
}

pub fn safe_mode_notice() -> String {
    tr!("RustyPresenter had trouble with the graphics card, so it started in safe mode. You can change this in Settings → Video.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use Graphics::*;

    const LINUX: Build = Build { wgpu: false, skia: false, windows: false, mac: false };
    const WINDOWS: Build = Build { windows: true, ..LINUX };
    const MAC: Build = Build { mac: true, ..LINUX };

    #[test]
    fn auto_and_opengl_are_femtovg_and_software_is_software() {
        for b in [LINUX, WINDOWS, MAC] {
            assert_eq!(renderer(Auto, b), Some(Renderer { name: "femtovg", api: Api::Any }));
            assert_eq!(renderer(OpenGl, b), Some(Renderer { name: "femtovg", api: Api::Any }));
            assert_eq!(renderer(Software, b), Some(Renderer { name: "software", api: Api::Any }));
        }
    }

    #[test]
    fn a_build_without_the_extra_renderers_offers_auto_opengl_and_software() {
        for b in [LINUX, WINDOWS, MAC] {
            assert_eq!(available(b), [Auto, OpenGl, Software], "{b:?}");
        }
    }

    #[test]
    fn skia_gives_each_system_its_own_interfaces() {
        let s = |b: Build| Build { skia: true, ..b };
        assert_eq!(available(s(WINDOWS)), [Auto, OpenGl, Vulkan, Direct3D, Software]);
        assert_eq!(available(s(LINUX)), [Auto, OpenGl, Vulkan, Software]);
        assert_eq!(available(s(MAC)), [Auto, OpenGl, Metal, Software]);
        assert_eq!(renderer(Direct3D, s(WINDOWS)), Some(Renderer { name: "skia", api: Api::Direct3D }));
        assert_eq!(renderer(Vulkan, s(LINUX)), Some(Renderer { name: "skia", api: Api::Vulkan }));
        assert_eq!(renderer(Metal, s(MAC)), Some(Renderer { name: "skia", api: Api::Metal }));
    }

    #[test]
    fn wgpu_gives_each_system_its_own_interfaces_and_beats_skia() {
        let w = |b: Build| Build { wgpu: true, ..b };
        assert_eq!(available(w(WINDOWS)), [Auto, OpenGl, Vulkan, Direct3D, Software]);
        assert_eq!(available(w(LINUX)), [Auto, OpenGl, Vulkan, Software]);
        assert_eq!(available(w(MAC)), [Auto, OpenGl, Metal, Software]);
        assert_eq!(renderer(Direct3D, w(WINDOWS)), Some(Renderer { name: "femtovg-wgpu", api: Api::Direct3D }));
        assert_eq!(renderer(Vulkan, w(WINDOWS)), Some(Renderer { name: "femtovg-wgpu", api: Api::Vulkan }));
        assert_eq!(renderer(Vulkan, w(LINUX)), Some(Renderer { name: "femtovg-wgpu", api: Api::Vulkan }));
        assert_eq!(renderer(Metal, w(MAC)), Some(Renderer { name: "femtovg-wgpu", api: Api::Metal }));
        assert_eq!(renderer(Vulkan, Build { skia: true, ..w(LINUX) }), Some(Renderer { name: "femtovg-wgpu", api: Api::Vulkan }));
        assert_eq!(renderer(Auto, w(WINDOWS)), Some(Renderer { name: "femtovg", api: Api::Any }), "Auto stays OpenGL");
        assert_eq!(attempts(Direct3D, w(WINDOWS)), [Direct3D, Auto, Software]);
        assert_eq!(attempts(Metal, w(WINDOWS)), [Auto, Software]);
        assert_eq!(draw_mode(renderer(Vulkan, w(LINUX)).unwrap().name), DrawMode::Software, "video into an image");
    }

    #[test]
    fn a_failed_choice_falls_back_to_auto_then_software() {
        let w = Build { skia: true, ..WINDOWS };
        assert_eq!(attempts(Direct3D, w), [Direct3D, Auto, Software]);
        assert_eq!(attempts(OpenGl, w), [OpenGl, Auto, Software]);
        assert_eq!(attempts(Auto, w), [Auto, Software]);
        assert_eq!(attempts(Software, w), [Software]);
        assert_eq!(attempts(Vulkan, WINDOWS), [Auto, Software], "not built in: straight to Auto");
        assert_eq!(attempts(Metal, Build { skia: true, ..LINUX }), [Auto, Software], "not on this system");
    }

    #[test]
    fn only_opengl_renderers_draw_video_under_the_scene() {
        for r in ["femtovg", "gl", "skia-opengl"] { assert_eq!(draw_mode(r), DrawMode::OpenGl, "{r}") }
        for r in ["software", "femtovg-wgpu", "skia", "skia-software"] { assert_eq!(draw_mode(r), DrawMode::Software, "{r}") }
    }

    #[test]
    fn developer_variables_beat_the_setting() {
        assert_eq!(env(None, None), Env::None);
        assert_eq!(env(Some("0"), Some("")), Env::None);
        assert_eq!(env(Some("1"), Some("skia")), Env::Software);
        assert_eq!(env(None, Some("skia-opengl")), Env::Renderer("skia-opengl".into()));
        assert_eq!(graphics_of("skia-opengl"), OpenGl);
        assert_eq!(graphics_of("software"), Software);
    }

    #[test]
    fn the_operator_is_told_in_plain_words() {
        assert_eq!(label(Auto), "Automatic (recommended)");
        assert_eq!(label(Software), "Software (safe mode)");
        assert_eq!(label(Direct3D), "Direct3D");
        assert_eq!(in_use(OpenGl), "In use now: OpenGL");
        assert_eq!(fell_back(Vulkan, Auto), "The Vulkan graphics mode could not be used on this computer, so RustyPresenter is using Automatic.");
        assert_eq!(restart_to_use(), "Restart RustyPresenter to use the new graphics setting.");
        assert_eq!(safe_mode_notice(), "RustyPresenter had trouble with the graphics card, so it started in safe mode. You can change this in Settings → Video.");
    }
}
