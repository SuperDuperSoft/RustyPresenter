//! The Bible actions of `App::do_action`: looking passages up, the keys of the online services, and the Bibles a church imports or adds. `use super::*` gives them what `app.rs` itself
//! uses (`parse`, `tryr!`, the model types); they are the arms of that match as they were, moved here so no one
//! function holds every action of the app.
use super::*;

/// Whether `action` is one of these.
pub(super) fn handles(action: &str) -> bool {
    matches!(action, "bible/lookup" | "bible/paste" | "bible/key" | "bible/catalog" | "bible/install" | "bible/import-remove") || action.starts_with("bible/import/")
}

/// Runs one of them. `App::do_action` sends an action here only when `handles` says so.
pub(super) async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        // ---- bible ----
        "bible/lookup" => {
            #[derive(Deserialize)] struct L { reference: String, translation: String, #[serde(default)] also: Vec<String> }
            let L { reference, translation, also } = parse(body)?;
            // Built in, then a file the church imported (works offline, wins over any online source), then online.
            let (label, verses) = tryr!(app.lookup_passage(&translation, &reference).await);
            // More Bibles shown with it (Nov 5 spec §3): one that cannot be looked up is left out, never the passage.
            let (also, left_out) = app.lookup_also(&translation, &also, &reference).await;
            Ok(json!({ "label": label, "verses": verses, "translation": translation, "also": also, "left_out": left_out }))
        }
        "bible/paste" => {
            // Verses typed or pasted from the church's own Bible, for a translation with no source here (e.g. HCSB).
            #[derive(Deserialize)] struct P { reference: String, translation: String, text: String }
            let P { reference, translation, text } = parse(body)?;
            let (label, verses) = tryr!(bible::pasted(&reference, &text));
            if bible::blocklist::blocked_passage(translation.trim(), &verses) { return Err(ActionError::bad(blocked())); }
            Ok(json!({ "label": label, "verses": verses, "translation": bible::normalise_id(&translation) }))
        }
        "bible/key" => {
            // An empty key removes it. A non-empty key is checked with one lookup before it is saved.
            #[derive(Deserialize)] struct K { provider: String, key: String }
            let K { provider, key } = parse(body)?;
            let p = bible::sources::Provider::from_key_name(&provider).ok_or_else(|| ActionError::bad(tr!("Unknown Bible service.")))?;
            tryr!(bible::sources::set_key(&app.store.root, p, &key).await);
            app.refresh_bible_credits();
            app.broadcast_data();
            Ok(json!({}))
        }
        // The body is the raw file. `bible/import/-` keeps the file's own short name.
        a if a.starts_with("bible/import/") => {
            let abbr = a["bible/import/".len()..].trim_matches('-').to_string();
            let (root, bytes) = (app.store.root.clone(), body.to_vec());
            let b = tokio::task::spawn_blocking(move || bible::import::import(&root, &bytes, &abbr)).await.map_err(|e| ActionError::bad(e.to_string()))?;
            let b = tryr!(b);
            app.refresh_bible_credits();
            app.broadcast_data();
            Ok(json!({ "abbreviation": b.abbreviation, "name": b.name, "copyright": b.copyright, "verses": b.len() }))
        }
        "bible/catalog" => {
            #[derive(Deserialize, Default)] #[serde(default)] struct C { force: bool }
            let C { force } = if body.is_empty() { C::default() } else { parse(body)? };
            Ok(tryr!(app.bible_catalog(force).await))
        }
        "bible/install" => {
            let T { translation } = parse(body)?;
            let m = tryr!(app.install_bible(&translation).await);
            Ok(json!({ "id": m.id, "abbr": m.abbr, "name": m.name }))
        }
        "bible/import-remove" => {
            let T { translation } = parse(body)?;
            // A Bible added from YouVersion's catalog has no file: it only leaves the list.
            if translation.starts_with("yv:") { tryr!(bible::online::remove(&app.store.root, &translation).map_err(|e| e.to_string())); }
            else { tryr!(bible::import::remove(&app.store.root, &translation).map_err(|e| e.to_string())); }
            app.refresh_bible_credits();
            app.broadcast_data();
            Ok(json!({}))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}
