//! FEAT-1 actions (spec §5.1), reached through `App::do_action` like every other action. Each one edits through
//! `App::edit`, so it is saved, refused in read-only mode and broadcast as `Update::Data`.
//!
//! | Action | Body → result |
//! |---|---|
//! | `theme/list` | `{}` → `{rows: [{id, name, builtin, overridden, is_default, items, services}]}` (Standard first) |
//! | `theme/save` | `SavedTheme` (nil id = new) → `{id, screen_views: []}` |
//! | `theme/save-standard` | `{style, background}` → `{}` |
//! | `theme/make-default` | `{id}` → `{}` |
//! | `theme/delete` | `{id}` → `{items, services, was_default}` |
//! | `theme/usage` | `{id}` → `{items, services, is_default}` |
//! | `service/theme` | `{service_id, theme: id or null}` → `{own_background: n}` |
//! | `service/clear-item-backgrounds` | `{service_id}` → `{cleared}` |
//! | `item/theme` | `{item_id, theme: id or null}` → `{}` (any service) |
//! | `background/add` | `{name, background}` → `{id}` |
//! | `background/rename` | `{id, name}` → `{}` |
//! | `background/remove` | `{id}` → `{}` (what uses it keeps it) |
//! | `overlay/list` | `{}` → `{chips: [{id, name, kind, on, changed, deleted}], clear_epoch}` (the Overlays strip) |
//! | `overlay/save` | `Overlay` (nil id = new) → `{id}` |
//! | `overlay/delete` | `{id}` → `{}` (a copy on screen stays until turned off) |
//! | `overlay/move` | `{id, to}` → `{}` (library order = drawing order) |
//! | `settings/logo` | `{asset: name or null}` → `{toast}` |
//!
//! Showing and hiding overlays are live commands, not actions: `command` with `{"id": <command id>, "cmd": "overlay",
//! "overlay": <overlay id>, "on": true, "seen_clear": <LiveView.clear_epoch>}`, the same with `"on": false`, and
//! `{"id": <command id>, "cmd": "clear_overlays"}` (F6). None of them needs a `base`.
use std::sync::Arc;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::app::{ActionError, App};
use crate::model::{AssetId, Background, BackgroundEntry, Data, MediaStatus, Theme};
use crate::theme::overlay::{self, Overlay, OverlayKind, slots};
use crate::theme::{self, DeleteKind, SavedTheme, ThemeId, clamp_style, default_ref, delete_kind, strings, validate_save};

fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(format!("Bad request: {e}")))
}

fn bad(e: String) -> ActionError {
    ActionError::bad(e)
}

#[derive(Deserialize)]
struct Id {
    id: Uuid,
}

#[derive(Deserialize)]
struct ServiceTheme {
    service_id: Uuid,
    #[serde(default)]
    theme: Option<ThemeId>,
}

#[derive(Deserialize)]
struct ItemTheme {
    item_id: Uuid,
    #[serde(default)]
    theme: Option<ThemeId>,
}

#[derive(Deserialize)]
struct Standard {
    style: Theme,
    background: Background,
}

/// A theme id that must name a theme (Standard, saved or built-in).
fn known(d: &Data, id: Option<ThemeId>) -> Result<(), String> {
    match id {
        Some(id) if theme::find(d, id).is_none() => Err(strings::theme_gone().into()),
        _ => Ok(()),
    }
}

/// Runs a FEAT-1 action; `None` when `action` is not one of them.
pub async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Option<Result<Value, ActionError>> {
    Some(match action {
        "theme/list" => Ok(list(&app.data.lock().unwrap())),
        "theme/save" => save(app, body),
        "theme/save-standard" => (|| {
            let Standard { mut style, background } = parse(body)?;
            clamp_style(&mut style);
            let background = background.clone().with_dim(background.dim());
            app.edit(|d| {
                d.settings.theme = style;
                d.settings.background = background;
                Ok(())
            })
            .map_err(bad)?;
            Ok(json!({}))
        })(),
        "theme/make-default" => (|| {
            let Id { id } = parse(body)?;
            app.edit(|d| {
                known(d, Some(id))?;
                d.default_theme = default_ref(id);
                Ok(())
            })
            .map_err(bad)?;
            Ok(json!({}))
        })(),
        "theme/delete" => (|| {
            let Id { id } = parse(body)?;
            let (items, services, was_default) = app
                .edit(|d| {
                    let kind = delete_kind(&d.catalog(), id)?;
                    d.themes.retain(|t| t.id != id);
                    let was_default = d.default_theme == Some(id);
                    if kind == DeleteKind::ResetBuiltin {
                        // The built-in stays, so what uses it keeps it.
                        return Ok((0, 0, false));
                    }
                    let (items, services) = theme::resolve::usage(d, id);
                    for s in &mut d.services {
                        if s.theme == Some(id) {
                            s.theme = None;
                        }
                        for i in &mut s.items {
                            if i.theme == Some(id) {
                                i.theme = None;
                            }
                        }
                    }
                    if was_default {
                        d.default_theme = None;
                    }
                    Ok((items, services, was_default))
                })
                .map_err(bad)?;
            Ok(json!({ "items": items, "services": services, "was_default": was_default }))
        })(),
        "theme/usage" => (|| {
            let Id { id } = parse(body)?;
            let d = app.data.lock().unwrap();
            let (items, services) = theme::resolve::usage(&d, id);
            Ok(json!({ "items": items, "services": services, "is_default": theme::church_default(&d).id == id }))
        })(),
        "service/theme" => (|| {
            let ServiceTheme { service_id, theme: id } = parse(body)?;
            let own = app
                .edit(|d| {
                    known(d, id)?;
                    let has_background = id.and_then(|id| theme::find(d, id)).is_some_and(|t| t.background.is_some());
                    let s = d.services.iter_mut().find(|s| s.id == service_id).ok_or(strings::service_gone())?;
                    s.theme = id;
                    let own = s.items.iter().filter(|i| theme::draws_text(i) && i.background.is_some()).count();
                    Ok(if has_background { own } else { 0 })
                })
                .map_err(bad)?;
            Ok(json!({ "own_background": own }))
        })(),
        "service/clear-item-backgrounds" => (|| {
            #[derive(Deserialize)]
            struct S {
                service_id: Uuid,
            }
            let S { service_id } = parse(body)?;
            let cleared = app
                .edit(|d| {
                    let s = d.services.iter_mut().find(|s| s.id == service_id).ok_or(strings::service_gone())?;
                    let mut n = 0;
                    for i in s.items.iter_mut().filter(|i| theme::draws_text(i) && i.background.is_some()) {
                        i.background = None;
                        n += 1;
                    }
                    Ok(n)
                })
                .map_err(bad)?;
            Ok(json!({ "cleared": cleared }))
        })(),
        "item/theme" => (|| {
            let ItemTheme { item_id, theme: id } = parse(body)?;
            app.edit(|d| {
                known(d, id)?;
                let item = d.services.iter_mut().flat_map(|s| s.items.iter_mut()).find(|i| i.id == item_id).ok_or(strings::item_gone())?;
                if id.is_some() && !theme::draws_text(item) {
                    return Err(strings::item_no_theme().into());
                }
                item.theme = id;
                Ok(())
            })
            .map_err(bad)?;
            Ok(json!({}))
        })(),
        "background/add" => (|| {
            #[derive(Deserialize)]
            struct Add {
                name: String,
                background: Background,
            }
            let Add { name, background } = parse(body)?;
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(ActionError::bad(strings::background_name_required()));
            }
            let dim = background.dim();
            let entry = BackgroundEntry::new(&name, background.with_dim(dim), MediaStatus::Ready, chrono::Utc::now());
            let id = entry.id;
            app.edit(|d| {
                d.backgrounds.push(entry);
                Ok(())
            })
            .map_err(bad)?;
            Ok(json!({ "id": id }))
        })(),
        "background/rename" => (|| {
            #[derive(Deserialize)]
            struct R {
                id: Uuid,
                name: String,
            }
            let R { id, name } = parse(body)?;
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(ActionError::bad(strings::background_name_required()));
            }
            app.edit(|d| {
                d.backgrounds.iter_mut().find(|e| e.id == id).ok_or(strings::background_gone())?.name = name;
                Ok(())
            })
            .map_err(bad)?;
            Ok(json!({}))
        })(),
        "background/remove" => (|| {
            let Id { id } = parse(body)?;
            app.edit(|d| {
                let before = d.backgrounds.len();
                d.backgrounds.retain(|e| e.id != id);
                if d.backgrounds.len() == before { Err(strings::background_gone().into()) } else { Ok(()) }
            })
            .map_err(bad)?;
            Ok(json!({}))
        })(),
        "overlay/list" => {
            let chips = {
                let d = app.data.lock().unwrap();
                let live = app.live.lock().unwrap();
                slots::chips(&d.overlays, &live.frame.overlays, d.settings.logo.as_ref())
            };
            let chips: Vec<Value> = chips
                .into_iter()
                .map(|c| json!({ "id": c.id, "name": c.name, "kind": kind_name(c.kind), "on": c.on, "changed": c.changed, "deleted": c.deleted }))
                .collect();
            Ok(json!({ "chips": chips, "clear_epoch": app.live.lock().unwrap().clear_epoch }))
        }
        "overlay/save" => (|| {
            let o: Overlay = parse(body)?;
            let exists = |a: &AssetId| app.store.asset_path(a).exists();
            let o = overlay::validate(o, exists).map_err(bad)?;
            let id = o.id;
            app.edit(|d| {
                match d.overlays.iter_mut().find(|x| x.id == id) {
                    Some(x) => *x = o,
                    None => d.overlays.push(o),
                }
                Ok(())
            })
            .map_err(bad)?;
            Ok(json!({ "id": id }))
        })(),
        "overlay/delete" => (|| {
            let Id { id } = parse(body)?;
            app.edit(|d| {
                let before = d.overlays.len();
                d.overlays.retain(|o| o.id != id);
                if d.overlays.len() == before { Err(strings::overlay_gone().into()) } else { Ok(()) }
            })
            .map_err(bad)?;
            Ok(json!({}))
        })(),
        "overlay/move" => (|| {
            #[derive(Deserialize)]
            struct Mv {
                id: Uuid,
                to: usize,
            }
            let Mv { id, to } = parse(body)?;
            app.edit(|d| {
                let from = d.overlays.iter().position(|o| o.id == id).ok_or(strings::overlay_gone())?;
                let o = d.overlays.remove(from);
                let to = to.min(d.overlays.len());
                d.overlays.insert(to, o);
                Ok(())
            })
            .map_err(bad)?;
            Ok(json!({}))
        })(),
        "settings/logo" => (|| {
            #[derive(Deserialize)]
            struct Logo {
                #[serde(default)]
                asset: Option<AssetId>,
            }
            let Logo { asset } = parse(body)?;
            if let Some(a) = &asset
                && !app.store.asset_path(a).exists()
            {
                return Err(ActionError::bad(strings::picture_gone()));
            }
            let toast = if asset.is_some() { strings::logo_saved() } else { strings::logo_removed() };
            app.edit(|d| {
                d.settings.logo = asset;
                Ok(())
            })
            .map_err(bad)?;
            Ok(json!({ "toast": toast }))
        })(),
        _ => return None,
    })
}

fn kind_name(k: OverlayKind) -> &'static str {
    match k {
        OverlayKind::Text => "text",
        OverlayKind::Picture => "picture",
        OverlayKind::Clock => "clock",
        OverlayKind::Other => "other",
    }
}

fn save(app: &Arc<App>, body: &[u8]) -> Result<Value, ActionError> {
    let t: SavedTheme = parse(body)?;
    let id = app
        .edit(|d| {
            let mut t = validate_save(&d.catalog(), t)?;
            if let Some(bg) = t.background.take() {
                t.background = Some(bg.clone().with_dim(bg.dim()));
            }
            let id = t.id;
            match d.themes.iter_mut().find(|x| x.id == id) {
                Some(x) => *x = t,
                None => d.themes.push(t),
            }
            Ok(id)
        })
        .map_err(bad)?;
    // FEAT-7 fills `screen_views` with the screen views that use this theme.
    Ok(json!({ "id": id, "screen_views": Vec::<String>::new() }))
}

/// The Themes tab's rows: Standard, the built-ins, then saved themes by name, each with its usage.
pub fn list(d: &Data) -> Value {
    let rows: Vec<Value> = d
        .catalog()
        .rows()
        .into_iter()
        .map(|r| {
            let (items, services) = theme::resolve::usage(d, r.id);
            json!({ "id": r.id, "name": r.name, "builtin": r.builtin, "overridden": r.overridden, "is_default": r.is_default,
                "items": items, "services": services })
        })
        .collect();
    json!({ "rows": rows })
}
