//! Compiles `ui/app.slint` (at the top of the repository) into Rust for `rp_ui`, then writes `panes.rs` (see `panes`).
fn main() {
    // Slint names the translation domain (and the .po file it bundles) after the package: keep the app's name.
    // SAFETY: the build script is one thread; nothing else reads the environment at the same time.
    unsafe { std::env::set_var("CARGO_PKG_NAME", "rustypresenter") };
    // Element ids stay in the generated code so the testing backend can find elements by id (check C4).
    // `@tr` text is looked up in `translations/<lang>/LC_MESSAGES/rustypresenter.po`, with no context unless the
    // `.slint` gives one (I18N spec D2).
    let slint = slint_build::CompilerConfiguration::new()
        .with_debug_info(true)
        .with_bundled_translations("../../translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("../../ui/app.slint", slint).expect("slint build failed");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let code = std::fs::read_to_string(out.join("app.rs")).expect("the code Slint made");
    std::fs::write(out.join("panes.rs"), panes::generate(&code)).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}

/// Dock panels: a panel can sit in a window of its own (`PanelWindow`), and Slint keeps a separate copy of every
/// global for each window. So every global a panel shows is written to the operator window and to every window of
/// its own, and a click in a window of its own runs the operator window's handler. This writes that code from the
/// globals' Rust API in `app.rs`, so nothing has to be listed twice:
/// - `<Global>All`: the global's setters write to every window; getters, `on_` and `invoke_` use the operator window.
/// - `op.ui.camera_switch_all()` and the like, on `OperatorWindow`.
/// - The `*Pane` globals' accessors under their old names (`op.ui.set_service_rows(…)`), on `OperatorWindow`, since
///   these were the operator window's own properties before the panels moved into docks.
/// - `copy_panes` (a new window of its own starts as the operator window is) and `forward_panes` (its clicks).
mod panes {
    use std::fmt::Write;

    /// Every global a panel uses that Rust writes to (OperatorState and TipState belong to each window).
    pub const MIRRORED: &[&str] = &[
        "ServicePane", "SlidesPane", "LibraryPane", "BiblePane", "LivePane", "StreamPane", "DockShared",
        "Theme", "MenuKeys", "ListFilter", "SlideActs", "AutoStrip", "CameraSwitch", "PanelExtras", "FollowUi",
        "LiveStatus", "VideoInputs", "OutputsState", "OverlayStrip", "ServicePlanning", "SongKeyState",
    ];

    struct Fun { name: String, sig: String, args: Vec<String> }

    /// The text from `open` (an opening brace) to its closing brace.
    fn block(code: &str, open: usize) -> &str {
        let mut depth = 0;
        for (i, c) in code[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => { depth -= 1; if depth == 0 { return &code[open + 1..open + i] } }
                _ => {}
            }
        }
        panic!("unbalanced braces in app.rs")
    }

    fn functions(body: &str) -> Vec<Fun> {
        let mut out = Vec::new();
        let mut rest = body;
        while let Some(at) = rest.find("pub fn ") {
            let after = &rest[at..];
            let open = after.find('{').unwrap();
            let sig = after[..open].trim().to_string();
            let name = sig["pub fn ".len()..].split(|c: char| c == ' ' || c == '(').next().unwrap().to_string();
            // `arg_0 : T , arg_1 : T ,` in `invoke_`'s parameters.
            let args = sig.split(&[',', '('][..]).filter_map(|p| p.trim().split(" :").next().filter(|n| n.starts_with("arg_")).map(str::to_string)).collect();
            out.push(Fun { name, sig, args });
            let len = block(after, open).len();
            rest = &after[open + len + 2..];
        }
        out
    }

    fn snake(name: &str) -> String {
        let mut s = String::new();
        for (i, c) in name.chars().enumerate() {
            if c.is_uppercase() { if i > 0 { s.push('_') } s.extend(c.to_lowercase()) } else { s.push(c) }
        }
        s
    }

    pub fn generate(code: &str) -> String {
        let module = code.split("mod ").nth(1).and_then(|s| s.split(|c: char| !c.is_alphanumeric() && c != '_').next()).expect("the generated module");
        let fix = |sig: &str| -> String {
            let sig = sig.replace("sp ::", "slint :: private_unstable_api :: re_exports ::");
            // Generated structs (`r#Side`) live in the generated module.
            let mut out = String::new();
            let mut rest = sig.as_str();
            while let Some(i) = rest.find("r#") {
                out.push_str(&rest[..i]);
                out.push_str(&format!("crate :: {module} :: "));
                out.push_str("r#");
                rest = &rest[i + 2..];
            }
            out.push_str(rest);
            out
        };
        let mut s = String::from("// Written by build.rs (`panes`). Do not edit.\n#[allow(unused_mut, clippy::all, clippy::pedantic)]\nmod panes_generated {\nuse slint::ComponentHandle;\nuse crate::{OperatorWindow, PanelWindow};\n");
        s += "thread_local!(static POPOUTS: std::cell::RefCell<Vec<PanelWindow>> = const { std::cell::RefCell::new(Vec::new()) });\n";
        s += "/// A window of its own opened: from now on the globals are written to it too.\npub fn register_popout(w: &PanelWindow) { POPOUTS.with(|p| p.borrow_mut().push(w.clone_strong())) }\n";
        s += "/// A window of its own closed.\npub fn unregister_popout(w: &PanelWindow) { POPOUTS.with(|p| p.borrow_mut().retain(|x| !std::ptr::eq(slint::ComponentHandle::window(x), slint::ComponentHandle::window(w)))) }\n";
        s += "/// The windows of their own open now.\npub fn popouts() -> Vec<PanelWindow> { POPOUTS.with(|p| p.borrow().iter().map(|w| w.clone_strong()).collect()) }\n";
        s += "pub fn each_popout(mut f: impl FnMut(&PanelWindow)) { for w in popouts() { f(&w) } }\n";
        let mut panes_impl = String::new();
        let mut props_impl = String::new();
        let mut copy = String::new();
        let mut forward = String::new();
        for g in MIRRORED {
            let head = format!("impl < 'a > r#{g} < 'a > {{");
            let at = code.find(&head).unwrap_or_else(|| panic!("global {g} is not exported from ui/app.slint"));
            let funs = functions(block(code, at + head.len() - 1));
            let all = format!("{g}All");
            let acc = format!("{}_all", snake(g));
            writeln!(s, "/// `{g}` in the operator window and in every window of its own.\npub struct {all}<'a>(pub &'a OperatorWindow);\nimpl<'a> {all}<'a> {{").unwrap();
            let pane = g.ends_with("Pane") || *g == "DockShared";
            for f in &funs {
                let sig = fix(&f.sig).replace("mut f :", "f :");
                let n = &f.name;
                let body = if n.starts_with("set_") {
                    format!("{{ each_popout(|w| w.global::<crate::{g}>().{n}(::core::clone::Clone::clone(&value))); self.0.global::<crate::{g}>().{n}(value) }}")
                } else if n.starts_with("on_") {
                    format!("{{ self.0.global::<crate::{g}>().{n}(f) }}")
                } else {
                    let args = f.args.join(", ");
                    format!("{{ self.0.global::<crate::{g}>().{n}({args}) }}")
                };
                writeln!(s, "    {sig} {body}").unwrap();
                if pane {
                    let tsig = sig.replacen("pub fn", "fn", 1);
                    let call_args = if n.starts_with("set_") { "value".to_string() } else if n.starts_with("on_") { "f".into() } else { f.args.join(", ") };
                    writeln!(props_impl, "    pub {tsig} {{ {all}(self).{n}({call_args}) }}").unwrap();
                }
                if let Some(p) = n.strip_prefix("set_") && funs.iter().any(|x| x.name == format!("get_{p}")) {
                    writeln!(copy, "    to.global::<crate::{g}>().set_{p}(from.global::<crate::{g}>().get_{p}());").unwrap();
                }
                if let Some(c) = n.strip_prefix("on_") {
                    let k = funs.iter().find(|x| x.name == format!("invoke_{c}")).map(|x| x.args.len()).unwrap_or(0);
                    let params: Vec<String> = (0..k).map(|i| format!("a{i}")).collect();
                    let p = params.join(", ");
                    writeln!(forward, "    {{ let m = main.as_weak(); to.global::<crate::{g}>().on_{c}(move |{p}| m.upgrade().map(|m| m.global::<crate::{g}>().invoke_{c}({p})).unwrap_or_default()); }}").unwrap();
                }
            }
            s += "}\n";
            writeln!(panes_impl, "    /// `{g}` in the operator window and in every window of its own.\n    pub fn {acc}(&self) -> {all}<'_> {{ {all}(self) }}").unwrap();
        }
        // Inherent methods (this crate makes `OperatorWindow`), so nothing needs importing; a name the operator
        // window still has as a property of its own would not compile, which keeps the two from mixing.
        writeln!(s, "impl OperatorWindow {{\n{panes_impl}{props_impl}}}").unwrap();
        writeln!(s, "/// A new window of its own starts showing what the operator window shows.\npub fn copy_panes(from: &OperatorWindow, to: &PanelWindow) {{\n{copy}}}").unwrap();
        writeln!(s, "/// A click in a window of its own runs the operator window's handler.\npub fn forward_panes(main: &OperatorWindow, to: &PanelWindow) {{\n{forward}}}").unwrap();
        let names: Vec<String> = MIRRORED.iter().map(|g| format!("{g:?}")).collect();
        writeln!(s, "/// The globals mirrored this way.\npub const MIRRORED: &[&str] = &[{}];\n}}\npub use panes_generated::*;", names.join(", ")).unwrap();
        s
    }
}
