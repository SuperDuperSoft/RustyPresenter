//! FEAT-6's operator actions (spec §5.1), reached through `App::do_action`. None of them is on the phone's list.
use super::strings as tr;
use crate::{
    app::{ActionError, App},
    model::{Data, ItemKind, PrintSettings, ServiceItem, ServiceTemplate},
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

/// The actions `run` answers.
pub fn handles(action: &str) -> bool {
    matches!(action, "service/copy-week" | "service/from-template" | "service/set-date" | "service/fill"
        | "template/save" | "template/from-service" | "template/rename" | "template/delete" | "template/day" | "print/write")
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(format!("Bad request: {e}")))
}

fn today() -> NaiveDate { chrono::Local::now().date_naive() }

/// A date sent as "YYYY-MM-DD".
fn iso(text: &str) -> Result<NaiveDate, ActionError> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").map_err(|_| ActionError::bad(tr::bad_date()))
}

/// The template that names something in the library, for the delete guards (`song/delete` and the others).
pub fn template_using(data: &Data, uses: impl Fn(&ItemKind) -> bool) -> Option<String> {
    data.templates.iter().find(|t| t.items.iter().any(|i| uses(&i.kind))).map(|t| t.name.clone())
}

/// Adds a new service, opens it, and says what was made.
fn add_service(data: &mut Data, s: crate::model::Service) -> Value {
    let v = json!({ "id": s.id, "name": s.name, "date": s.date, "blanks": super::blank_count(&s.items) });
    data.current_service = Some(s.id);
    data.services.push(s);
    v
}

pub async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    let bad = ActionError::bad;
    match action {
        "service/copy-week" => {
            #[derive(Deserialize)] struct C { source: Option<Uuid>, date: Option<String>, #[serde(default)] blanks: bool }
            let C { source, date, blanks } = parse(body)?;
            let date = date.as_deref().map(iso).transpose()?;
            let today = today();
            app.edit(|d| {
                let order = super::date_order(&d.settings);
                let id = source.or_else(|| super::week_source(&d.services, d.current_service, today)).ok_or(tr::no_service_to_copy())?;
                let src = d.services.iter().find(|s| s.id == id).ok_or(tr::no_service_to_copy())?;
                let date = date.unwrap_or_else(|| super::next_week_date(src.date, today));
                let new = super::copy_week(src, date, blanks, order);
                Ok(add_service(d, new))
            }).map_err(bad)
        }
        "service/from-template" => {
            #[derive(Deserialize)] struct F { template: Uuid, name: Option<String>, date: Option<String> }
            let F { template, name, date } = parse(body)?;
            let date = date.as_deref().map(iso).transpose()?;
            let today = today();
            app.edit(|d| {
                let order = super::date_order(&d.settings);
                let t = d.templates.iter().find(|t| t.id == template).ok_or(tr::template_gone())?;
                let date = date.unwrap_or_else(|| super::default_date(t.day, today));
                let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("{} {}", t.name, super::format_date(date, order)));
                let new = super::service_from_template(t, name.trim(), date);
                Ok(add_service(d, new))
            }).map_err(bad)
        }
        "service/set-date" => {
            #[derive(Deserialize)] struct S { id: Uuid, date: Option<String> }
            let S { id, date } = parse(body)?;
            let date = date.filter(|t| !t.trim().is_empty()).as_deref().map(iso).transpose()?;
            app.edit(|d| { d.services.iter_mut().find(|s| s.id == id).ok_or(tr::service_gone())?.date = date; Ok(json!({})) }).map_err(bad)
        }
        "service/fill" => {
            #[derive(Deserialize)] struct F { id: Uuid, item: Value }
            let F { id, mut item } = parse(body)?;
            if item.get("id").is_none() { item["id"] = json!(id); }
            if item.get("kind").and_then(Value::as_str) == Some("scripture") && item.get("per_slide").is_none() {
                item["per_slide"] = json!(app.data().settings.bible_per_slide);
            }
            let chosen: ServiceItem = serde_json::from_value(item).map_err(|e| bad(e.to_string()))?;
            if matches!(chosen.kind, ItemKind::Blank { .. } | ItemKind::Header { .. }) { return Err(ActionError::bad(tr::not_a_blank())) }
            if crate::bible::blocklist::strip_items(&mut vec![chosen.clone()]) > 0 { return Err(ActionError::bad(crate::scripture::strings::blocked())) }
            app.edit(|d| {
                let slot = d.services.iter_mut().flat_map(|s| s.items.iter_mut()).find(|i| i.id == id).ok_or(tr::blank_gone())?;
                if !super::is_blank(slot) { return Err(tr::not_a_blank().into()) }
                *slot = super::fill(slot, chosen);
                Ok(json!({ "id": id }))
            }).map_err(bad)
        }
        "template/save" => {
            let mut t: ServiceTemplate = parse(body)?;
            t.name = t.name.trim().to_string();
            if t.name.is_empty() { return Err(ActionError::bad(tr::template_needs_name())) }
            if t.items.is_empty() { return Err(ActionError::bad(tr::template_needs_parts())) }
            if t.id.is_nil() { t.id = Uuid::new_v4() }
            let id = t.id;
            app.edit(|d| { match d.templates.iter_mut().find(|x| x.id == id) { Some(x) => *x = t, None => d.templates.push(t) } Ok(json!({ "id": id })) }).map_err(bad)
        }
        // Save as template: songs and Bible readings become blanks (spec §5.3, decision 7).
        "template/from-service" => {
            #[derive(Deserialize)] struct F { service: Uuid, name: String }
            let F { service, name } = parse(body)?;
            if name.trim().is_empty() { return Err(ActionError::bad(tr::template_needs_name())) }
            app.edit(|d| {
                let s = d.services.iter().find(|s| s.id == service).ok_or(tr::service_gone())?;
                if s.items.is_empty() { return Err(tr::template_needs_parts().into()) }
                let t = super::template_from_service(s, &name);
                let v = json!({ "id": t.id, "name": t.name });
                d.templates.push(t);
                Ok(v)
            }).map_err(bad)
        }
        "template/rename" => {
            #[derive(Deserialize)] struct R { id: Uuid, name: String }
            let R { id, name } = parse(body)?;
            if name.trim().is_empty() { return Err(ActionError::bad(tr::template_needs_name())) }
            app.edit(|d| { d.templates.iter_mut().find(|t| t.id == id).ok_or(tr::template_gone())?.name = name.trim().into(); Ok(json!({})) }).map_err(bad)
        }
        "template/day" => {
            #[derive(Deserialize)] struct D { id: Uuid, day: Option<chrono::Weekday> }
            let D { id, day } = parse(body)?;
            app.edit(|d| { d.templates.iter_mut().find(|t| t.id == id).ok_or(tr::template_gone())?.day = day; Ok(json!({})) }).map_err(bad)
        }
        "template/delete" => {
            #[derive(Deserialize)] struct I { id: Uuid }
            let I { id } = parse(body)?;
            app.edit(|d| { d.templates.retain(|t| t.id != id); Ok(json!({})) }).map_err(bad)
        }
        "print/write" => {
            #[derive(Deserialize)]
            struct P { service: Uuid, #[serde(flatten)] options: PrintSettings, path: Option<PathBuf> }
            let P { service, options, path } = parse(body)?;
            let (html, stem) = {
                let mut d = app.data();
                d.settings.print = options.clone();
                let s = d.services.iter().find(|s| s.id == service).ok_or_else(|| ActionError::bad(tr::service_gone()))?;
                let html = crate::print::html(&d, s, &options, today());
                let stem = crate::print::file_stem(&s.name, options.what);
                app.save(&d).map_err(bad)?;
                (html, stem)
            };
            app.broadcast_data();
            let root = app.store.root.clone();
            let r = tokio::task::spawn_blocking(move || match path {
                Some(p) => crate::report_page::write_to(&p, &html).map(|()| (p, true)),
                None => crate::report_page::write_and_open(&root, &stem, &html),
            }).await.map_err(|e| bad(e.to_string()))?;
            let (path, opened) = r.map_err(bad)?;
            Ok(json!({ "path": path, "opened": opened }))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}

#[cfg(test)]
mod tests;
