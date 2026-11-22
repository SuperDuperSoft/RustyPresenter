//! The service actions of `App::do_action`: the open service's items, and the list of services. `use super::*` gives them what `app.rs` itself
//! uses (`parse`, `tryr!`, the model types); they are the arms of that match as they were, moved here so no one
//! function holds every action of the app.
use super::*;

/// Whether `action` is one of these.
pub(super) fn handles(action: &str) -> bool {
    matches!(action, "service/add" | "service/update" | "service/remove" | "service/move" | "service/new" | "service/select" | "service/rename" | "service/delete")
}

/// Runs one of them. `App::do_action` sends an action here only when `handles` says so.
pub(super) async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        // ---- service ----
        "service/add" => {
            #[derive(Deserialize)] struct Add { item: Value, position: Option<usize> }
            let Add { mut item, position } = parse(body)?;
            if item.get("id").is_none() { item["id"] = json!(Uuid::new_v4()); }
            // Pictures added together as one item (`"slideshow": true` beside the item's fields).
            let slideshow = item.as_object_mut().and_then(|o| o.remove("slideshow")).and_then(|v| v.as_bool()).unwrap_or(false);
            // A video added without a choice does what Settings → General → Screens and graphics and media says when it ends (N24).
            if item.get("kind").and_then(Value::as_str) == Some("media") && item.get("end").is_none() {
                item["end"] = json!(app.data().settings.media.video_end);
            }
            // A passage added without a choice gets the church's verses per slide (Settings → Look).
            if item.get("kind").and_then(Value::as_str) == Some("scripture") && item.get("per_slide").is_none() {
                item["per_slide"] = json!(app.data().settings.bible_per_slide);
            }
            // ... and the church's Also Bibles (Settings → Bible), looked up here.
            let fill_also = item.get("kind").and_then(Value::as_str) == Some("scripture") && item.get("also").is_none();
            let mut item: ServiceItem = serde_json::from_value(item).map_err(|e| ActionError::bad(e.to_string()))?;
            let mut toast = None;
            if fill_also { toast = app.fill_also(&mut item).await; }
            refuse_blocked(&item)?;
            item.id = Uuid::new_v4();
            let id = item.id;
            tryr!(app.edit(|d| {
                let s = d.service_mut().ok_or_else(|| tr!("No service is open."))?;
                let pos = position.unwrap_or(s.items.len()).min(s.items.len());
                s.items.insert(pos, item);
                // Pictures added as one slideshow turn by themselves (Settings → General → Screens and graphics and media, N24).
                let secs = d.settings.media.picture_secs;
                if slideshow && crate::automation::advance::AutoAdvance::validate(secs).is_ok() {
                    d.automation.advance.insert(id, crate::automation::advance::AutoAdvance { secs, loop_item: true });
                }
                Ok(())
            }));
            Ok(json!({ "id": id, "toast": toast }))
        }
        "service/update" => {
            // Merged by key into the stored item, so a client that does not know a field (an item's theme) keeps it.
            let patch: Value = parse(body)?;
            let stored = patch.get("id").and_then(|id| serde_json::from_value::<Uuid>(id.clone()).ok()).and_then(|id| {
                app.data().service().and_then(|s| s.items.iter().find(|i| i.id == id)).and_then(|i| serde_json::to_value(i).ok())
            });
            let merged = match stored { Some(s) => tryr!(crate::theme::merge::merge_object(&s, &patch)), None => patch };
            let item: ServiceItem = serde_json::from_value(merged).map_err(|e| ActionError::bad(tr!("Bad request: {error}", error = e)))?;
            refuse_blocked(&item)?;
            tryr!(app.edit(|d| {
                let s = d.service_mut().ok_or_else(|| tr!("No service is open."))?;
                let slot = s.items.iter_mut().find(|i| i.id == item.id).ok_or_else(|| tr!("That item is gone."))?;
                *slot = item;
                Ok(())
            }));
            Ok(json!({}))
        }
        "service/remove" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| { if let Some(s) = d.service_mut() { s.items.retain(|i| i.id != id); } Ok(()) }));
            Ok(json!({}))
        }
        "service/move" => {
            #[derive(Deserialize)] struct Mv { id: Uuid, to: usize }
            let Mv { id, to } = parse(body)?;
            tryr!(app.edit(|d| {
                let s = d.service_mut().ok_or_else(|| tr!("No service is open."))?;
                let from = s.items.iter().position(|i| i.id == id).ok_or_else(|| tr!("That item is gone."))?;
                let it = s.items.remove(from);
                s.items.insert(to.min(s.items.len()), it);
                Ok(())
            }));
            Ok(json!({}))
        }
        "service/new" => {
            #[derive(Deserialize)] struct N { name: String, copy_from: Option<Uuid> }
            let N { name, copy_from } = parse(body)?;
            let id = tryr!(app.edit(|d| {
                let mut s = Service::new(&if name.trim().is_empty() { tr!("New service") } else { name.trim().to_string() });
                s.date = Some(chrono::Local::now().date_naive());
                if let Some(src) = copy_from.and_then(|c| d.services.iter().find(|x| x.id == c)) {
                    s.template = src.template;
                    s.items = src.items.iter().cloned().map(|mut i| { i.id = Uuid::new_v4(); i }).collect();
                }
                let id = s.id;
                d.services.push(s);
                d.current_service = Some(id);
                Ok(id)
            }));
            Ok(json!({ "id": id }))
        }
        "service/select" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| { if !d.services.iter().any(|s| s.id == id) { return Err(tr!("That service is gone.")); } d.current_service = Some(id); Ok(()) }));
            app.online_text_from_cache();
            let me = app.clone();
            tokio::spawn(async move { me.fetch_online_text().await });
            Ok(json!({}))
        }
        "service/rename" => {
            #[derive(Deserialize)] struct R { id: Uuid, name: String }
            let R { id, name } = parse(body)?;
            tryr!(app.edit(|d| { if let Some(s) = d.services.iter_mut().find(|s| s.id == id) { s.name = name; } Ok(()) }));
            Ok(json!({}))
        }
        "service/delete" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| {
                if d.services.len() <= 1 { return Err(tr!("You need at least one service.")); }
                d.services.retain(|s| s.id != id);
                if d.current_service == Some(id) { d.current_service = d.services.first().map(|s| s.id); }
                Ok(())
            }));
            Ok(json!({}))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}
