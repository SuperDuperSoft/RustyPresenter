//! When a song use is logged (FEAT-7 §5.4) and the `usage/*` operator actions (§5.5).
use super::{UsageEntry, export, open, parse_date, report, strings, today};
use crate::{
    app::{ActionError, App},
    live::{Frame, LiveItem, SlideLayer},
    model::{Data, ItemKind},
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

/// The use to log when `frame` is on screen with `item` live, if any: the item is a song in a service and the
/// frame shows a text slide. The song's details are copied from the library now.
///
/// Every screen shows the slide layer until screen views exist, and there is no livestream output yet, so a text
/// slide on screen means the words are visible and `streamed` is false.
pub fn hit_for(data: &Data, frame: &Frame, item: Option<&LiveItem>, at: DateTime<Utc>, local_date: NaiveDate) -> Option<UsageEntry> {
    if !matches!(frame.slide, Some(SlideLayer::Text { scripture: false, .. })) { return None }
    let item_id = item?.item_id;
    // The current service first: the same item id can't be in two services, but a copied service could share it.
    let services = data.service().into_iter().chain(data.services.iter());
    let (service, song_id) = services.into_iter().find_map(|s| s.items.iter().find(|i| i.id == item_id).and_then(|i| match i.kind {
        ItemKind::Song { song_id, .. } => Some((s, song_id)),
        _ => None,
    }))?;
    let song = data.songs.iter().find(|s| s.id == song_id)?;
    Some(UsageEntry {
        id: Uuid::new_v4(), at, local_date, song_id, title: song.title.clone(), ccli: song.ccli.trim().to_string(),
        writers: song.writers.clone(), copyright: song.copyright.clone(), service_id: service.id, service_name: service.name.clone(),
        streamed: false,
    })
}

/// A Details line, for the dialog.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DetailRow {
    pub id: Uuid,
    pub date: String,
    pub time: String,
    pub service: String,
    pub song: String,
    pub streamed: bool,
}

/// `usage/report`'s answer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReportView {
    pub from: String,
    pub to: String,
    pub rows: Vec<super::ReportRow>,
    pub entries: Vec<DetailRow>,
    pub license: String,
    pub enabled: bool,
    pub reported_through: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Range { from: Option<String>, to: Option<String>, include_no_ccli: bool }

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(format!("Bad request: {e}")))
}

fn date_arg(text: Option<&str>, or: NaiveDate) -> Result<NaiveDate, ActionError> {
    match text.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(or),
        Some(t) => parse_date(t).ok_or_else(|| ActionError::bad(strings::bad_date())),
    }
}

impl App {
    /// Checks the frame on screen for a song use and hands it to the log's writer thread. `committed`: the room just
    /// confirmed a frame; otherwise no room is connected and the frame just asked for counts. Never waits on a save.
    pub(crate) fn note_song_use(&self, committed: bool) {
        if !self.usage.enabled() { return }
        let (frame, item) = {
            let live = self.live();
            if committed {
                let Some((frame, _, item)) = live.committed() else { return };
                (frame, item)
            } else {
                if !matches!(live.frame.slide, Some(SlideLayer::Text { scripture: false, .. })) { return }
                (live.frame.clone(), live.item.clone())
            }
        };
        let hit = hit_for(&self.data(), &frame, item.as_ref(), Utc::now(), today());
        if let Some(hit) = hit { self.usage.hit(hit) }
    }

    fn range(&self, r: &Range) -> Result<(NaiveDate, NaiveDate), ActionError> {
        let today = today();
        let reported = self.usage.log().reported_through;
        let from = date_arg(r.from.as_deref(), super::default_from(reported, today))?;
        let to = date_arg(r.to.as_deref(), today)?;
        if from > to { return Err(ActionError::bad(strings::from_after_to())) }
        Ok((from, to))
    }

    fn license(&self) -> String { self.data().settings.church_ccli.trim().to_string() }

    /// The `usage/*` actions. Operator only: the phone remote has none of them.
    pub(crate) async fn usage_action(self: &Arc<Self>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
        match action {
            "usage/report" => {
                let r: Range = parse(body)?;
                let (from, to) = self.range(&r)?;
                // Read before taking the log, so the log's lock never waits on the data lock.
                let license = self.license();
                let log = self.usage.log();
                let rows = report(&log.entries, from, to, r.include_no_ccli);
                let mut entries: Vec<&UsageEntry> = log.entries.iter().filter(|e| e.local_date >= from && e.local_date <= to).collect();
                entries.sort_by(|a, b| b.at.cmp(&a.at));
                let entries = entries.into_iter().map(|e| DetailRow {
                    id: e.id, date: export::day(e.local_date), time: e.at.with_timezone(&chrono::Local).format("%-I:%M %p").to_string(),
                    service: e.service_name.clone(), song: e.title.clone(), streamed: e.streamed,
                }).collect();
                let view = ReportView {
                    from: export::day(from), to: export::day(to), rows, entries, license, enabled: log.enabled,
                    reported_through: log.reported_through.map(export::day),
                };
                Ok(serde_json::to_value(view).unwrap())
            }
            "usage/remove" => {
                #[derive(Deserialize)] struct Id { id: Uuid }
                let Id { id } = parse(body)?;
                self.usage.edit(|log| log.entries.retain(|e| e.id != id)).map_err(|e| ActionError::bad(strings::save_failed(&e)))?;
                Ok(json!({ "ok": true }))
            }
            "usage/mark-reported" => {
                #[derive(Deserialize)] struct Through { through: String }
                let Through { through } = parse(body)?;
                let through = parse_date(&through).ok_or_else(|| ActionError::bad(strings::bad_date()))?;
                self.usage.edit(|log| log.reported_through = Some(through)).map_err(|e| ActionError::bad(strings::save_failed(&e)))?;
                Ok(json!({ "ok": true }))
            }
            "usage/set" => {
                #[derive(Deserialize)] struct Set { enabled: bool }
                let Set { enabled } = parse(body)?;
                self.usage.edit(|log| log.enabled = enabled).map_err(|e| ActionError::bad(strings::save_failed(&e)))?;
                Ok(json!({ "enabled": enabled }))
            }
            "usage/export" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "lowercase")]
                enum Format { Csv, Html }
                #[derive(Deserialize)]
                struct Export { #[serde(flatten)] range: Range, path: Option<PathBuf>, format: Format }
                let Export { range, path, format } = parse(body)?;
                let (from, to) = self.range(&range)?;
                let rows = report(&self.usage.log().entries, from, to, range.include_no_ccli);
                let license = self.license();
                let (text, ext) = match format {
                    Format::Csv => (export::to_csv(&rows, &license), "csv"),
                    Format::Html => (export::to_html(&rows, &license, (from, to)), "html"),
                };
                // A page with no place chosen goes to the data folder's `reports/` and opens in the browser.
                let open_after = path.is_none();
                let path = path.unwrap_or_else(|| self.store.root.join("reports").join(export::file_name(from, to, ext)));
                let written = path.clone();
                let write = move || -> Result<(), String> {
                    if let Some(dir) = written.parent().filter(|d| !d.as_os_str().is_empty()) { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
                    crate::store::write_atomic(&written, text.as_bytes()).map_err(|e| e.to_string())?;
                    if open_after { open::open_file(&written)?; }
                    Ok(())
                };
                tokio::task::spawn_blocking(write).await.map_err(|e| e.to_string()).and_then(|r| r)
                    .map_err(|e| ActionError::bad(strings::save_failed(&e)))?;
                Ok(json!({ "path": path }))
            }
            _ => Err(ActionError { status: 404, message: "unknown action".into() }),
        }
    }
}
