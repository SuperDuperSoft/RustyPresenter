//! The settings actions of `App::do_action`: Settings as a whole, and the few settings saved as they change. `use super::*` gives them what `app.rs` itself
//! uses (`parse`, `tryr!`, the model types); they are the arms of that match as they were, moved here so no one
//! function holds every action of the app.
use super::*;

/// Whether `action` is one of these.
pub(super) fn handles(action: &str) -> bool {
    matches!(action, "settings" | "songs-folder/set" | "settings/translation" | "settings/language" | "settings/graphics" | "settings/also" | "settings/bible-history" | "settings/dock" | "settings/favourite")
}

/// Runs one of them. `App::do_action` sends an action here only when `handles` says so.
pub(super) async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        // ---- settings ----
        "settings" => {
            // Merged by key into the stored settings (FEAT-1 spec §9.1): a key the form leaves out (a logo set
            // meanwhile) keeps its value.
            let patch: Value = parse(body)?;
            tryr!(app.edit(|d| {
                let mut s: Settings = crate::theme::merge::merge_typed(&d.settings, &patch, crate::theme::merge::merge_object)?;
                s.lines_per_slide = s.lines_per_slide.clamp(1, 8);
                s.theme.size = s.theme.size.clamp(24, 200);
                s.theme.scripture_size = s.theme.scripture_size.clamp(24, 200);
                s.bible_per_slide = s.bible_per_slide.min(4);
                // The picker owns these (`settings/translation`, `settings/favourite`): a form saved from an older
                // copy must not undo a star or a pick made while it was open.
                s.favourites = std::mem::take(&mut d.settings.favourites);
                s.recent = std::mem::take(&mut d.settings.recent);
                // Settings → Language applies at once (`settings/language`); an older form must not undo it.
                s.ui_language = std::mem::take(&mut d.settings.ui_language);
                s.graphics = d.settings.graphics;
                s.print = std::mem::take(&mut d.settings.print);
                s.recent_files = std::mem::take(&mut d.settings.recent_files);
                s.bible_history = std::mem::take(&mut d.settings.bible_history);
                // The panels are saved by `settings/dock` as they change.
                s.dock = std::mem::take(&mut d.settings.dock);
                s.look_fade_ms = s.look_fade_ms.min(crate::model::LOOK_FADE_MAX_MS);
                if !crate::model::DRAW_HEIGHTS.contains(&s.draw_height) { s.draw_height = 0 }
                // N31: only `network/save` changes these (it hashes the passwords).
                s.network = std::mem::take(&mut d.settings.network);
                // Settings → Songs → "Keep my songs in a folder" saves itself (`songs-folder/set`).
                s.songs_folder = std::mem::take(&mut d.settings.songs_folder);
                d.settings = s;
                Ok(())
            }));
            Ok(json!({}))
        }
        // Settings → Songs → "Keep my songs in a folder" (`crate::songs_folder`): each choice saves at once.
        "songs-folder/set" => {
            #[derive(Deserialize)]
            struct F { #[serde(default)] on: Option<bool>, #[serde(default)] folder: Option<std::path::PathBuf>, #[serde(default)] usual: bool, #[serde(default)] tidy: Option<bool> }
            let F { on, folder, usual, tidy } = parse(body)?;
            if let Some(f) = &folder && !f.is_absolute() { return Err(ActionError::bad(tr!("Choose a whole folder path."))) }
            let prefs = tryr!(app.edit(|d| {
                let p = &mut d.settings.songs_folder;
                if let Some(on) = on { p.on = on }
                if let Some(f) = folder { p.folder = Some(f) }
                if usual { p.folder = None }
                if let Some(t) = tidy { p.tidy = t }
                Ok(p.clone())
            }));
            crate::songs_folder::wake();
            Ok(json!({ "on": prefs.on, "folder": prefs.dir(), "chosen": prefs.folder.is_some(), "tidy": prefs.tidy }))
        }
        "settings/translation" => {
            #[derive(Deserialize)] struct T { translation: String }
            let T { translation } = parse(body)?;
            if bible::blocklist::blocked(&translation, &[], &[], &[&translation]) { return Err(ActionError::bad(blocked())); }
            if !app.translations().contains(&translation) { return Err(ActionError::bad(tr!("Unknown translation."))); }
            // A main Bible that was also an Also Bible stops being one (Nov 5 spec §3).
            let turned_off = tryr!(app.edit(|d| {
                d.settings.used_translation(&translation);
                let was = d.settings.also_show.iter().any(|t| bible::id::same_id(t, &translation));
                d.settings.also_show.retain(|t| !bible::id::same_id(t, &translation));
                d.settings.translation = translation.clone();
                Ok(was)
            }));
            let toast = turned_off.then(|| crate::scripture::strings::also_turned_off(&app.abbr_of(&translation)));
            Ok(json!({ "toast": toast }))
        }
        // The app's language (I18N spec D6): `""` follows the computer. Rust text changes at once; windows follow
        // on their thread (`i18n::sync_slint`). The toast is in the new language.
        "settings/language" => {
            #[derive(Deserialize)] struct L { code: String }
            let L { code } = parse(body)?;
            if !code.is_empty() && !crate::i18n::is_known(&code) { return Err(ActionError::bad(crate::tr!("Unknown language."))); }
            // Switched before the save, so the update the save sends finds the new language.
            let before = crate::i18n::current();
            let _ = crate::i18n::set_language(&crate::i18n::resolve(&code));
            if let Err(e) = app.edit(|d| { d.settings.ui_language = code.clone(); Ok(()) }) {
                let _ = crate::i18n::set_language(&before);
                return Err(ActionError::bad(e));
            }
            Ok(json!({ "toast": crate::tr!("Language changed") }))
        }
        // Settings → General → Screens and graphics → Graphics: saved now, used from the next start (`video::graphics`).
        "settings/graphics" => {
            #[derive(Deserialize)] struct G { graphics: crate::model::Graphics }
            let G { graphics } = parse(body)?;
            tryr!(app.edit(|d| { d.settings.graphics = graphics; Ok(()) }));
            Ok(json!({ "toast": crate::video::graphics::restart_to_use() }))
        }
        // The church's Also Bibles for new passages and how they share a slide (Nov 5 spec §2–3).
        "settings/also" => {
            #[derive(Deserialize)] struct A { also_show: Vec<String>, #[serde(default)] parallel_layout: crate::scripture::ParallelLayout }
            let A { mut also_show, parallel_layout } = parse(body)?;
            let known = app.translations();
            if also_show.iter().any(|t| bible::blocklist::blocked(t, &[], &[], &[t])) { return Err(ActionError::bad(blocked())); }
            if also_show.iter().any(|t| !known.contains(t)) { return Err(ActionError::bad(tr!("Unknown translation."))); }
            tryr!(app.edit(|d| {
                crate::scripture::also::tidy_ids(&d.settings.translation, &mut also_show, &|_| false);
                d.settings.also_show = also_show;
                d.settings.parallel_layout = parallel_layout;
                Ok(())
            }));
            Ok(json!({}))
        }
        // The picker's star. A Bible no longer installed can still be unstarred.
        // N6: a reference added from the Bible box goes first in its history. Not a step for Undo.
        "settings/bible-history" => {
            #[derive(Deserialize)] struct H { reference: String }
            let H { reference } = parse(body)?;
            tryr!(app.edit_untracked(|d| { d.settings.used_reference(&reference); Ok(()) }));
            Ok(json!({}))
        }
        // The operator window's panels (dock panels design): saved as they change. Not a step for Undo.
        "settings/dock" => {
            let mut dock: crate::ui::dock::DockPrefs = parse(body)?;
            dock.current.repair();
            tryr!(app.edit_untracked(|d| { d.settings.dock = dock; Ok(()) }));
            Ok(json!({}))
        }
        "settings/favourite" => {
            #[derive(Deserialize)] struct F { translation: String, on: bool }
            let F { translation, on } = parse(body)?;
            if on {
                if bible::blocklist::blocked(&translation, &[], &[], &[&translation]) { return Err(ActionError::bad(blocked())); }
                if !app.translations().contains(&translation) { return Err(ActionError::bad(tr!("Unknown translation."))); }
            }
            tryr!(app.edit(|d| { d.settings.set_favourite(&translation, on); Ok(()) }));
            Ok(json!({}))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}
