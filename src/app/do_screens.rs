//! The screen actions of `App::do_action`: opening and closing the output windows, the screen setup, and the stage message. `use super::*` gives them what `app.rs` itself
//! uses (`parse`, `tryr!`, the model types); they are the arms of that match as they were, moved here so no one
//! function holds every action of the app.
use super::*;

/// Whether `action` is one of these.
pub(super) fn handles(action: &str) -> bool {
    matches!(action, "display/open" | "display/close" | "screens/power" | "screens/add" | "screens/remove" | "screens/update" | "screens/test" | "screens/identify" | "stage/message")
}

/// Runs one of them. `App::do_action` sends an action here only when `handles` says so.
pub(super) async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        // ---- displays ----
        "display/open" => {
            #[derive(Deserialize)] struct O { role: String, monitor: Option<String> }
            let O { role, monitor } = parse(body)?;
            if !matches!(role.as_str(), "room" | "stage" | "operator") { return Err(ActionError::bad(tr!("Unknown screen."))); }
            if role == "operator" { tryr!(app.open_window(&role, None)) } else {
                let target = monitor.map_or(ScreenTarget::Window, ScreenTarget::Monitor);
                let group = if role == "room" { "audience" } else { "stage" };
                let before = app.data().settings.screens.clone();
                tryr!(app.edit(|d| { d.settings.screens.assign(group, target); Ok(()) }));
                if let Err(e) = app.sync_screens() {
                    // The screen did not open: the saved setup goes back to what it was, so it never says a screen
                    // is on that is not. (Another screen failing does not undo this one.)
                    if !app.open_roles().contains(&role) {
                        let _ = app.edit_untracked(|d| { d.settings.screens = before; Ok(()) });
                        let _ = app.sync_screens();
                    }
                    return Err(ActionError::bad(e));
                }
            }
            app.broadcast_data();
            Ok(json!({}))
        }
        "display/close" => {
            #[derive(Deserialize)] struct C { role: String }
            let C { role } = parse(body)?;
            if !matches!(role.as_str(), "room" | "stage" | "operator") { return Err(ActionError::bad(tr!("Unknown screen."))); }
            if role == "operator" { app.close_window(&role) } else {
                let group = if role == "room" { "audience" } else { "stage" };
                tryr!(app.edit(|d| { d.settings.screens.group_mut(group).ok_or_else(|| tr!("Unknown screen."))?.on = false; Ok(()) }));
                tryr!(app.sync_screens());
            }
            app.broadcast_data();
            Ok(json!({}))
        }
        // ---- the screen setup ----
        "screens/power" => {
            #[derive(Deserialize)] struct P { group: String, on: bool }
            let P { group, on } = parse(body)?;
            let free = app.free_monitor(&app.data().settings.screens);
            tryr!(app.edit(|d| {
                let g = d.settings.screens.group_mut(&group).ok_or_else(|| tr!("Unknown screen."))?;
                // The first switch-on just works when a screen is free for it.
                if on && g.screens.iter().all(|s| s.output().target == ScreenTarget::None) {
                    let free = free.ok_or_else(|| tr!("Pick an output for this screen first."))?;
                    let first = g.screens.first_mut().ok_or_else(|| tr!("Add a screen first."))?;
                    *first.output_mut() = ScreenOutput { target: ScreenTarget::Monitor(free.id), width: free.width.max(0) as u32, height: free.height.max(0) as u32 };
                }
                g.on = on;
                Ok(())
            }));
            tryr!(app.sync_screens());
            app.broadcast_data();
            Ok(json!({}))
        }
        "screens/add" => {
            #[derive(Deserialize)] struct A { group: String }
            let A { group } = parse(body)?;
            let id = tryr!(app.edit(|d| {
                let g = d.settings.screens.group_mut(&group).ok_or_else(|| tr!("Unknown screen."))?;
                let n = g.screens.len() + 1;
                let name = (n..).map(|i| if group == "audience" { tr!("Audience screen {number}", number = i) } else { tr!("Stage screen {number}", number = i) }).find(|name| !g.screens.iter().any(|s| s.name == *name)).unwrap_or_default();
                let screen = Screen::new(Uuid::new_v4(), &name);
                let id = screen.id;
                g.screens.push(screen);
                Ok(id)
            }));
            Ok(json!({ "id": id }))
        }
        "screens/remove" => {
            #[derive(Deserialize)] struct R { id: Uuid }
            let R { id } = parse(body)?;
            tryr!(app.edit(|d| {
                let s = &mut d.settings.screens;
                let (group, _) = s.find(id).ok_or_else(|| tr!("That screen is gone."))?;
                let g = s.group_mut(group).ok_or_else(|| tr!("Unknown screen."))?;
                if g.screens.len() < 2 { return Err(tr!("Each list keeps at least one screen.")) }
                g.screens.retain(|s| s.id != id);
                Ok(())
            }));
            tryr!(app.sync_screens());
            app.broadcast_data();
            Ok(json!({}))
        }
        // Any of the name, where it shows and the size of a window. One monitor shows one screen, so taking a monitor
        // takes it from the screen that had it.
        "screens/update" => {
            #[derive(Deserialize)] struct U { id: Uuid, name: Option<String>, target: Option<ScreenTarget>, size: Option<(u32, u32)>, fix: Option<crate::screen_fix::ScreenFix>, mirror: Option<bool>, mask: Option<crate::render::mask::Mask> }
            let U { id, name, target, size, fix, mirror, mask } = parse(body)?;
            let move_windows = target.is_some() || size.is_some() || mirror.is_some();
            let monitors = app.monitors();
            tryr!(app.edit(|d| {
                let setup = &mut d.settings.screens;
                if let Some(ScreenTarget::Monitor(m)) = &target {
                    for g in [&mut setup.audience, &mut setup.stage] {
                        for s in g.screens.iter_mut().filter(|s| s.id != id) {
                            if s.output().target == ScreenTarget::Monitor(m.clone()) { s.output_mut().target = ScreenTarget::None }
                        }
                    }
                }
                let screen = setup.find_mut(id).ok_or_else(|| tr!("That screen is gone."))?;
                if let Some(name) = name.as_deref().map(str::trim).filter(|n| !n.is_empty()) { screen.name = name.into() }
                if let Some(f) = fix { screen.fix = f.clamped() }
                if let Some(m) = mirror { screen.mirror = m }
                if let Some(m) = mask { screen.mask = m.clamped() }
                let out = screen.output_mut();
                if let Some(t) = target {
                    if let ScreenTarget::Monitor(m) = &t && let Some(m) = monitors.iter().find(|x| x.id == *m) {
                        (out.width, out.height) = (m.width.max(0) as u32, m.height.max(0) as u32);
                    }
                    out.target = t;
                }
                if let Some((w, h)) = size { (out.width, out.height) = (w.clamp(320, 7680), h.clamp(240, 4320)) }
                Ok(())
            }));
            if move_windows { tryr!(app.sync_screens()) }
            app.broadcast_data();
            Ok(json!({}))
        }
        // A test pattern on one screen (N20), or with no id every pattern off. Not an edit to undo.
        "screens/test" => {
            #[derive(Deserialize)] struct T { id: Option<Uuid>, pattern: crate::screen_fix::TestPattern }
            let T { id, pattern } = parse(body)?;
            tryr!(app.edit_untracked(|d| {
                let setup = &mut d.settings.screens;
                match id {
                    Some(id) => setup.find_mut(id).ok_or_else(|| tr!("That screen is gone."))?.test = pattern,
                    None => { setup.clear_tests(); }
                }
                Ok(())
            }));
            Ok(json!({}))
        }
        "screens/identify" => {
            app.identify_screens();
            Ok(json!({}))
        }
        "stage/message" => {
            #[derive(Deserialize)] struct M { text: String }
            let M { text } = parse(body)?;
            app.set_stage_message(text.trim());
            Ok(json!({}))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}
