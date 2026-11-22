//! The media and deck actions of `App::do_action`: the media library and its folders, imports, decks, and the jobs box. `use super::*` gives them what `app.rs` itself
//! uses (`parse`, `tryr!`, the model types); they are the arms of that match as they were, moved here so no one
//! function holds every action of the app.
use super::*;

/// Whether `action` is one of these.
pub(super) fn handles(action: &str) -> bool {
    matches!(action, "media/rename" | "media/folder" | "media/folder-new" | "media/folder-delete" | "media/delete" | "media/reprobe" | "import/path" | "deck/from-images" | "deck/rename" | "deck/notes" | "deck/transitions" | "deck/show" | "deck/delete" | "job/cancel" | "job/dismiss")
}

/// Runs one of them. `App::do_action` sends an action here only when `handles` says so.
pub(super) async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        // ---- media & decks ----
        "media/rename" => {
            #[derive(Deserialize)] struct R { id: Uuid, name: String }
            let R { id, name } = parse(body)?;
            tryr!(app.edit(|d| { if let Some(m) = d.media.iter_mut().find(|m| m.id == id) { m.name = name; } Ok(()) }));
            Ok(json!({}))
        }
        "media/folder" => {
            #[derive(Deserialize)] struct F { id: Uuid, folder: String }
            let F { id, folder } = parse(body)?;
            let folder = folder.trim().to_string();
            tryr!(app.edit(|d| {
                let m = d.media.iter_mut().find(|m| m.id == id).ok_or_else(|| tr!("That file is gone."))?;
                m.folder = folder.clone();
                if !folder.is_empty() && !d.media_folders.contains(&folder) { d.media_folders.push(folder); }
                Ok(())
            }));
            Ok(json!({}))
        }
        "media/folder-new" => {
            #[derive(Deserialize)] struct F { name: String }
            let F { name } = parse(body)?;
            let name = name.trim().to_string();
            if name.is_empty() { return Err(ActionError::bad(tr!("Give the folder a name."))); }
            tryr!(app.edit(|d| {
                if !d.media_folders.iter().any(|f| f.eq_ignore_ascii_case(&name)) { d.media_folders.push(name); }
                Ok(())
            }));
            Ok(json!({}))
        }
        "media/folder-delete" => {
            #[derive(Deserialize)] struct F { name: String }
            let F { name } = parse(body)?;
            // The files stay in the library; only the folder goes.
            tryr!(app.edit(|d| {
                d.media_folders.retain(|f| *f != name);
                for m in d.media.iter_mut().filter(|m| m.folder == name) { m.folder.clear(); }
                Ok(())
            }));
            Ok(json!({}))
        }
        "media/delete" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| {
                let used = d.services.iter().any(|s| s.items.iter().any(|i| matches!(i.kind, ItemKind::Media { media_id, .. } if media_id == id)));
                if used { return Err(tr!("This is in a service. Remove it from the service first.")); }
                if let Some(t) = crate::planning::actions::template_using(d, |k| matches!(k, ItemKind::Media { media_id, .. } if *media_id == id)) { return Err(crate::planning::strings::in_template(&t)); }
                d.media.retain(|m| m.id != id);
                Ok(())
            }));
            Ok(json!({}))
        }
        "media/reprobe" => {
            let Id { id } = parse(body)?;
            import::media::reprobe(app.clone(), vec![id]);
            Ok(json!({}))
        }
        // Local automation and tests. Never on the phone's list (`server::remote_command`).
        "import/path" => {
            #[derive(Deserialize)] struct P { path: PathBuf, purpose: String }
            let P { path, purpose } = parse(body)?;
            let purpose = match purpose.as_str() {
                "media" => ImportPurpose::Media,
                "asset" => ImportPurpose::Asset,
                "powerpoint" => ImportPurpose::PowerPoint,
                "lyrics" => ImportPurpose::Lyrics,
                "slides" => ImportPurpose::Slides,
                "show" => ImportPurpose::Show,
                other => return Err(ActionError::bad(tr!("Bad request: unknown purpose {purpose}", purpose = other))),
            };
            Ok(match app.import_path(&path, purpose, |_| {}).await? {
                Imported::Media(m) => json!({ "media": m }),
                Imported::Asset(asset) => json!({ "asset": asset }),
                Imported::Job(job) => json!({ "job": job }),
                Imported::Song { id, title, duplicate_of } => json!({ "id": id, "title": title, "duplicate_of": duplicate_of }),
            })
        }
        "deck/from-images" => {
            #[derive(Deserialize)] struct D { title: String, assets: Vec<AssetId> }
            let D { title, assets } = parse(body)?;
            if assets.is_empty() { return Err(ActionError::bad(tr!("Choose at least one picture."))); }
            for a in &assets { if !app.store.asset_path(a).exists() { return Err(ActionError::bad(tr!("A picture is missing. Try uploading again."))); } }
            let id = Uuid::new_v4();
            tryr!(app.edit(|d| { d.decks.push(Deck { id, title, slides: assets, notes: vec![], transitions: vec![], advance_ms: vec![], show: None }); Ok(()) }));
            Ok(json!({ "id": id }))
        }
        "deck/rename" => {
            #[derive(Deserialize)] struct R { id: Uuid, title: String }
            let R { id, title } = parse(body)?;
            tryr!(app.edit(|d| { if let Some(x) = d.decks.iter_mut().find(|x| x.id == id) { x.title = title; } Ok(()) }));
            Ok(json!({}))
        }
        "deck/notes" => {
            #[derive(Deserialize)] struct N { id: Uuid, notes: Vec<String> }
            let N { id, notes } = parse(body)?;
            tryr!(app.edit(|d| { if let Some(x) = d.decks.iter_mut().find(|x| x.id == id) { x.notes = crate::model::deck_notes(notes, x.slides.len()); } Ok(()) }));
            Ok(json!({}))
        }
        "deck/transitions" => {
            // `advance_ms` (each slide's auto-advance time) is left as it is when the call has none.
            #[derive(Deserialize)] struct T { id: Uuid, transitions: Vec<Option<crate::design::Transition>>, #[serde(default)] advance_ms: Option<Vec<Option<u32>>> }
            let T { id, transitions, advance_ms } = parse(body)?;
            tryr!(app.edit(|d| {
                if let Some(x) = d.decks.iter_mut().find(|x| x.id == id) {
                    x.transitions = crate::model::deck_transitions(transitions, x.slides.len());
                    if let Some(a) = advance_ms { x.advance_ms = crate::model::deck_advance(a, x.slides.len()); }
                }
                Ok(())
            }));
            Ok(json!({}))
        }
        "deck/show" => {
            // 1.2: run a deck in PowerPoint, or show it as pictures again. Only a deck that kept its file can run.
            #[derive(Deserialize)] struct S { id: Uuid, run: bool }
            let S { id, run } = parse(body)?;
            tryr!(app.edit(|d| {
                let deck = d.decks.iter_mut().find(|x| x.id == id).ok_or_else(|| tr!("That item is gone."))?;
                match &mut deck.show {
                    Some(show) => show.run = run,
                    None if run => return Err(crate::pplive::strings::choose_file_again(&deck.title)),
                    None => {}
                }
                Ok(())
            }));
            Ok(json!({}))
        }
        "deck/delete" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| {
                let used = d.services.iter().any(|s| s.items.iter().any(|i| matches!(i.kind, ItemKind::Deck { deck_id } if deck_id == id)));
                if used { return Err(tr!("These slides are in a service. Remove them from the service first.")); }
                if let Some(t) = crate::planning::actions::template_using(d, |k| matches!(k, ItemKind::Deck { deck_id } if *deck_id == id)) { return Err(crate::planning::strings::in_template(&t)); }
                d.decks.retain(|x| x.id != id);
                Ok(())
            }));
            Ok(json!({}))
        }
        "job/cancel" => {
            let Id { id } = parse(body)?;
            app.job_cancel(id);
            Ok(json!({}))
        }
        "job/dismiss" => {
            let Id { id } = parse(body)?;
            app.jobs.lock().unwrap().retain(|j| j.id != id);
            app.broadcast_data();
            Ok(json!({}))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}
