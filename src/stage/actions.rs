//! The stage layout actions (FEAT-3 spec §5.5): read the church's layouts, save them all at once, say what uses one
//! before it is deleted, and pick the stage default. Called from `App::do_action`.
use super::{presets, refs, strings as s, StageLayout};
use crate::app::{ActionError, App};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub fn handles(action: &str) -> bool {
    matches!(action, "stage/layouts" | "stage/layouts/replace" | "stage/layout/refs" | "stage/layout/default")
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(format!("Bad request: {e}")))
}

/// What uses a layout, from the stored data. Items and macros don't name stage layouts yet.
fn layout_refs(data: &crate::model::Data, id: Uuid) -> Vec<refs::LayoutRef> {
    refs::layout_refs(&refs::RefSources { stage_default: data.settings.stage.layout, items: &[], macros: &[] }, id)
}

pub async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    match action {
        // The church's layouts and the stage default, for the editor and the pickers.
        "stage/layouts" => {
            let d = app.data();
            Ok(json!({ "layouts": d.stage_layouts, "default": d.settings.stage.layout }))
        }
        // Everything that uses a layout, and the question to ask before deleting it.
        "stage/layout/refs" => {
            #[derive(Deserialize)] struct R { id: Uuid, #[serde(default)] name: Option<String> }
            let R { id, name } = parse(body)?;
            let d = app.data();
            let name = name.or_else(|| presets::find_layout(&d.stage_layouts, id).map(|l| l.name.clone())).unwrap_or_default();
            let found = layout_refs(&d, id);
            Ok(json!({ "refs": found, "question": refs::delete_question(&name, &found) }))
        }
        // Every change from the editor in one go: all of it is saved, or none of it.
        "stage/layouts/replace" => {
            #[derive(Deserialize)] struct R { layouts: Vec<StageLayout>, #[serde(default)] deleted: Vec<Uuid> }
            let R { layouts, deleted } = parse(body)?;
            let cleared = app.edit(|d| {
                let church = refs::plan_replace(&d.stage_layouts, layouts, &deleted)?;
                let cleared = refs::clear_deleted(&mut d.settings.stage.layout, &mut [], &deleted);
                d.stage_layouts = church;
                Ok(cleared)
            }).map_err(ActionError::bad)?;
            Ok(json!({ "cleared": cleared }))
        }
        // The layout the stage shows from now on.
        "stage/layout/default" => {
            #[derive(Deserialize)] struct R { id: Uuid }
            let R { id } = parse(body)?;
            app.edit(|d| {
                if presets::find_layout(&d.stage_layouts, id).is_none() { return Err(s::layout_gone()); }
                d.settings.stage.layout = id;
                Ok(())
            }).map_err(ActionError::bad)?;
            Ok(json!({}))
        }
        _ => Err(ActionError { status: 404, message: "unknown action".into() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, lenient::Lenient, model::Data, stage::{ElementKind, PctRect}};

    fn layout(name: &str) -> StageLayout {
        let el = presets::new_box(ElementKind::Clock { seconds: false, h24: false }, Uuid::new_v4());
        StageLayout { id: Uuid::new_v4(), name: name.into(), background: super::super::BACKGROUND.into(), font: String::new(), elements: vec![Lenient::Known(el)] }
    }

    async fn act(app: &Arc<App>, action: &str, body: Value) -> Result<Value, String> {
        app.do_action(action, body.to_string().as_bytes()).await.map_err(|e| e.message)
    }

    #[tokio::test]
    async fn replace_saves_church_layouts_and_reads_them_back() {
        let app = test_app();
        let ours = layout("Ours");
        act(&app, "stage/layouts/replace", json!({ "layouts": [ours], "deleted": [] })).await.unwrap();
        assert_eq!(app.data().stage_layouts, vec![ours.clone()]);
        let read = act(&app, "stage/layouts", json!({})).await.unwrap();
        assert_eq!(read["layouts"][0]["name"], "Ours");
        assert_eq!(read["default"], json!(presets::CLASSIC));
    }

    #[tokio::test]
    async fn replace_is_all_or_nothing_and_keeps_the_error() {
        let app = test_app();
        let err = act(&app, "stage/layouts/replace", json!({ "layouts": [layout("Ours"), layout("  ")], "deleted": [] })).await.unwrap_err();
        assert_eq!(err, "Give the stage layout a name.");
        assert!(app.data().stage_layouts.is_empty());
        let err = act(&app, "stage/layouts/replace", json!({ "layouts": [], "deleted": [presets::BAND] })).await.unwrap_err();
        assert_eq!(err, "Built-in stage layouts can't be changed. Make a copy first.");
    }

    #[tokio::test]
    async fn deleting_the_default_puts_classic_back_and_counts_it() {
        let app = test_app();
        let ours = layout("Ours");
        act(&app, "stage/layouts/replace", json!({ "layouts": [ours], "deleted": [] })).await.unwrap();
        act(&app, "stage/layout/default", json!({ "id": ours.id })).await.unwrap();
        assert_eq!(app.data().settings.stage.layout, ours.id);
        let r = act(&app, "stage/layout/refs", json!({ "id": ours.id })).await.unwrap();
        assert_eq!(r["question"], "Delete “Ours”? It is used by the stage default. The default will use Classic.");
        assert_eq!(r["refs"], json!([{ "kind": "stage_default" }]));
        let r = act(&app, "stage/layouts/replace", json!({ "layouts": [], "deleted": [ours.id] })).await.unwrap();
        assert_eq!(r["cleared"], 1);
        let d = app.data();
        assert!(d.stage_layouts.is_empty());
        assert_eq!(d.settings.stage.layout, presets::CLASSIC);
    }

    #[tokio::test]
    async fn refs_question_for_an_unused_or_unsaved_layout() {
        let app = test_app();
        let r = act(&app, "stage/layout/refs", json!({ "id": Uuid::new_v4(), "name": "New stage layout" })).await.unwrap();
        assert_eq!(r["question"], "Delete “New stage layout”?");
    }

    #[tokio::test]
    async fn default_must_name_a_layout() {
        let app = test_app();
        act(&app, "stage/layout/default", json!({ "id": presets::SPEAKER })).await.unwrap();
        assert_eq!(app.data().settings.stage.layout, presets::SPEAKER);
        let err = act(&app, "stage/layout/default", json!({ "id": Uuid::new_v4() })).await.unwrap_err();
        assert_eq!(err, "That stage layout is gone.");
    }

    #[test]
    fn old_data_loads_without_stage_layouts_and_new_data_round_trips() {
        let d: Data = serde_json::from_str("{}").unwrap();
        assert!(d.stage_layouts.is_empty());
        assert!(serde_json::to_value(&d).unwrap().get("stage_layouts").is_none(), "nothing written when there are none");
        let mut d = Data::default();
        d.stage_layouts.push(layout("Ours"));
        let back: Data = serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
        assert_eq!(back.stage_layouts, d.stage_layouts);
    }

    #[test]
    fn hand_edited_layout_is_tidied_on_load() {
        let mut d = Data::default();
        let mut l = layout("Ours");
        if let Lenient::Known(e) = &mut l.elements[0] { e.rect = PctRect::new(f32::NAN, 10.0, 150.0, 10.0) }
        d.stage_layouts.push(l);
        assert!(d.after_load());
        let Lenient::Known(e) = &d.stage_layouts[0].elements[0] else { panic!() };
        assert_eq!(e.rect, PctRect::new(0.0, 10.0, 100.0, 10.0));
        assert!(!d.after_load(), "once");
    }
}
