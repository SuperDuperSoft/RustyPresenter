//! The song library actions of `App::do_action`: saving, fitting, transposing, deleting and importing songs. `use super::*` gives them what `app.rs` itself
//! uses (`parse`, `tryr!`, the model types); they are the arms of that match as they were, moved here so no one
//! function holds every action of the app.
use super::*;

/// Whether `action` is one of these.
pub(super) fn handles(action: &str) -> bool {
    matches!(action, "song/save" | "song/fit" | "song/key" | "song/delete" | "song/import")
}

/// Runs one of them. `App::do_action` sends an action here only when `handles` says so.
pub(super) async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        // ---- songs ----
        "song/save" => {
            let mut song: Song = parse(body)?;
            if song.title.trim().is_empty() { return Err(ActionError::bad(tr!("Give the song a title."))); }
            if song.id.is_nil() { song.id = Uuid::new_v4(); }
            song.lines_per_slide = song.lines_per_slide.min(Song::MAX_LINES);
            if song.text_size > 0 { song.text_size = song.text_size.clamp(Song::MIN_TEXT, Song::MAX_TEXT) }
            song.transpose = crate::transpose::clamp(song.transpose);
            song.key = song.key.trim().to_string();
            let id = song.id;
            tryr!(app.edit(|d| { match d.songs.iter_mut().find(|s| s.id == id) { Some(s) => *s = song, None => d.songs.push(song) } Ok(()) }));
            Ok(json!({ "id": id }))
        }
        // One song's lines per slide and/or text size (0 = Settings'); the rest of the song stays as it is, so two
        // quick presses never undo each other.
        "song/fit" => {
            #[derive(Deserialize)]
            struct Fit { id: Uuid, #[serde(default)] lines_per_slide: Option<u8>, #[serde(default)] text_size: Option<u32> }
            let Fit { id, lines_per_slide, text_size } = parse(body)?;
            tryr!(app.edit(|d| {
                let Some(s) = d.songs.iter_mut().find(|s| s.id == id) else { return Err(tr!("That song is not in the library any more.")) };
                if let Some(n) = lines_per_slide { s.lines_per_slide = n.min(Song::MAX_LINES) }
                if let Some(px) = text_size { s.text_size = if px == 0 { 0 } else { px.clamp(Song::MIN_TEXT, Song::MAX_TEXT) } }
                Ok(())
            }));
            Ok(json!({ "id": id }))
        }
        // One song's key and how far the stage screen moves its chords; the rest of the song stays as it is. When
        // the song is live, the stage screen shows the moved chords at once: only the chords differ, so the live
        // engine takes the new slides in quietly (`Live::reconcile`) and the room's words never change.
        "song/key" => {
            #[derive(Deserialize)]
            struct K { id: Uuid, #[serde(default)] transpose: Option<i8>, #[serde(default)] step: Option<i8>, #[serde(default)] key: Option<String> }
            let K { id, transpose, step, key } = parse(body)?;
            let now = tryr!(app.edit(|d| {
                let Some(s) = d.songs.iter_mut().find(|s| s.id == id) else { return Err(tr!("That song is not in the library any more.")) };
                if let Some(t) = transpose { s.transpose = crate::transpose::clamp(t) }
                if let Some(n) = step { s.transpose = crate::transpose::clamp(s.transpose.saturating_add(n)) }
                if let Some(k) = key { s.key = k.trim().to_string() }
                Ok(s.transpose)
            }));
            Ok(json!({ "id": id, "transpose": now }))
        }
        "song/delete" => {
            let Id { id } = parse(body)?;
            tryr!(app.edit(|d| {
                let used = d.services.iter().any(|s| s.items.iter().any(|i| matches!(i.kind, ItemKind::Song { song_id, .. } if song_id == id)));
                if used { return Err(tr!("This song is in a service. Remove it from the service first.")); }
                if let Some(t) = crate::planning::actions::template_using(d, |k| matches!(k, ItemKind::Song { song_id, .. } if *song_id == id)) { return Err(crate::planning::strings::in_template(&t)); }
                d.songs.retain(|s| s.id != id);
                Ok(())
            }));
            Ok(json!({}))
        }
        "song/import" => {
            #[derive(Deserialize)] struct Import { text: String, mode: SongImportMode, #[serde(default)] name: String }
            let Import { text, mode, name } = parse(body)?;
            let (id, title, duplicate_of) = tryr!(app.import_song(&name, text.as_bytes(), mode));
            Ok(json!({ "id": id, "title": title, "duplicate_of": duplicate_of }))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}
