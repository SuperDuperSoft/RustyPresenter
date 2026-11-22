//! The backup actions of `App::do_action`: Back up now, and Restore. `use super::*` gives them what `app.rs` itself
//! uses (`parse`, `tryr!`, the model types); they are the arms of that match as they were, moved here so no one
//! function holds every action of the app.
use super::*;

/// Whether `action` is one of these.
pub(super) fn handles(action: &str) -> bool {
    matches!(action, "backup/now" | "backup/restore")
}

/// Runs one of them. `App::do_action` sends an action here only when `handles` says so.
pub(super) async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        // ---- backups ----
        "backup/now" => {
            let data = app.data().clone();
            // Copies every asset: keep it off the runtime's worker threads.
            let a = app.clone();
            let name = tokio::task::spawn_blocking(move || a.store.backup(&data)).await
                // Only a panic inside the backup lands here: a server fault, not the user's.
                .map_err(|_| ActionError { status: 500, message: tr!("Backup failed.") })?;
            let name = tryr!(name.map_err(|e| e.to_string()));
            app.broadcast_data();
            Ok(json!({ "name": name }))
        }
        "backup/restore" => {
            #[derive(Deserialize)] struct B { name: String }
            let B { name } = parse(body)?;
            if app.read_only() { return Err(ActionError::bad(crate::theme::strings::newer_data())); }
            let app = app.clone();
            tokio::task::spawn_blocking(move || {
                let mut restored = tryr!(app.store.restore(&name));
                let removed = bible::blocklist::strip(&mut restored);
                if removed > 0 { app.live().notice = Some(passages_removed(removed)); }
                // Saved again through the numbered saves, so an edit's save still on its way to the disk can't put the
                // replaced data back over the restored file.
                let save = {
                    let mut data = app.data();
                    *data = restored;
                    app.take_save(&data)
                };
                let saved = save.and_then(|s| app.write_save(s));
                app.reconcile_live();
                app.refresh_bible_credits();
                app.online_text_from_cache();
                app.broadcast_data();
                app.broadcast_live();
                tryr!(saved);
                Ok(json!({}))
            }).await.map_err(|_| ActionError { status: 500, message: tr!("Backup failed.") })?
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}
