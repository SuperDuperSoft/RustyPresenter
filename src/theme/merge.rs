//! Merge-by-key for `settings/save` and `service/update` (spec §9.1). A key the patch leaves out
//! keeps its stored value, so a dialog that predates a field never wipes it; an explicit `null`
//! or a value replaces it. Service items merge by id, in the patch's order.
use std::collections::HashSet;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::theme::strings;

/// Top-level keys of `patch` over `stored`: a missing key keeps its value, `null` or a value
/// replaces it. Nested objects are replaced whole.
pub fn merge_object(stored: &Value, patch: &Value) -> Result<Value, String> {
    let patch = patch.as_object().ok_or(strings::expected_object())?;
    let mut out = stored.as_object().cloned().unwrap_or_default();
    for (k, v) in patch {
        out.insert(k.clone(), v.clone());
    }
    Ok(Value::Object(out))
}

/// `merge_object` on the service, with `items` (when sent) taken in the patch's order: an item
/// whose id is stored merges into it unless its kind changed; unknown ids are taken as sent.
pub fn merge_service(stored: &Value, patch: &Value) -> Result<Value, String> {
    let mut out = merge_object(stored, patch)?;
    let Some(items) = patch.get("items") else { return Ok(out) };
    let items = items.as_array().ok_or(strings::expected_object())?;
    let old: Vec<&Map<String, Value>> =
        stored.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object).collect();
    let mut seen = HashSet::new();
    let mut merged = Vec::with_capacity(items.len());
    for item in items {
        let fields = item.as_object().ok_or(strings::expected_object())?;
        let Some(id) = fields.get("id").filter(|id| !id.is_null()) else {
            merged.push(item.clone());
            continue;
        };
        if !seen.insert(id.to_string()) {
            return Err(strings::item_twice().into());
        }
        let prior = old.iter().copied().find(|o| o.get("id") == Some(id));
        // A missing kind keeps the stored one, like any other key.
        let same_kind = |o: &Map<String, Value>| fields.get("kind").is_none_or(|k| o.get("kind") == Some(k));
        merged.push(match prior {
            Some(o) if same_kind(o) => merge_object(&Value::Object(o.clone()), item)?,
            _ => item.clone(),
        });
    }
    out["items"] = Value::Array(merged);
    Ok(out)
}

/// Merges `patch` into a typed value through its JSON form. An explicit `null` clears an
/// `Option` field; on a required field the deserialize error is returned.
pub fn merge_typed<T: Serialize + DeserializeOwned>(
    stored: &T,
    patch: &Value,
    f: fn(&Value, &Value) -> Result<Value, String>,
) -> Result<T, String> {
    let current = serde_json::to_value(stored).map_err(|e| e.to_string())?;
    serde_json::from_value(f(&current, patch)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Settings;
    use crate::theme::strings::*;
    use serde_json::{Value, json};

    const ID: &str = "00000000-0000-4000-8000-0000000000aa";
    const I: &str = "00000000-0000-4000-8000-0000000000bb";
    const J: &str = "00000000-0000-4000-8000-0000000000cc";
    const K: &str = "00000000-0000-4000-8000-0000000000dd";
    const T: &str = "00000000-0000-4000-8000-0000000000c1";

    fn layout() -> Value {
        json!({"boxes": [{"kind": "clock", "x": 0, "y": 0}]})
    }

    #[test]
    fn settings_save_missing_key_keeps_value() {
        let s = Settings { logo: Some("a.png".into()), ..Settings::default() };
        let out: Settings = merge_typed(&s, &json!({"lines_per_slide": 4}), merge_object).unwrap();
        assert_eq!((out.logo.as_deref(), out.lines_per_slide), (Some("a.png"), 4));
    }

    #[test]
    fn settings_save_null_clears_optional_field() {
        let s = Settings { logo: Some("a.png".into()), ..Settings::default() };
        let out: Settings = merge_typed(&s, &json!({"logo": null}), merge_object).unwrap();
        assert_eq!(out.logo, None);
    }

    #[test]
    fn settings_save_null_on_required_field_is_an_error() {
        let s = Settings::default();
        assert!(merge_typed(&s, &json!({"lines_per_slide": null}), merge_object).is_err());
    }

    #[test]
    fn settings_save_keeps_logo_set_meanwhile() {
        // The dialog opened before the logo was set to b; its save doesn't mention the logo.
        let stored = Settings { logo: Some("b.png".into()), ..Settings::default() };
        let dialog = json!({"lines_per_slide": 3, "credits": "last", "church_ccli": "123"});
        let out: Settings = merge_typed(&stored, &dialog, merge_object).unwrap();
        assert_eq!((out.logo.as_deref(), out.credits.as_str()), (Some("b.png"), "last"));
    }

    #[test]
    fn service_update_missing_keys_keep_every_new_field() {
        let stored = json!({"id": ID, "name": "Sun", "theme": T, "items": [{"id": I, "kind": "text", "title": "A", "body": "x", "styles": [{"start":0,"end":1,"bold":true}], "theme": T, "starts_at": "10:00", "advance": {"after_s": 5}, "stage_layout": layout(), "slide_notes": {}, "arrangement": [], "second": "KJV", "layout": "x", "breaks": [], "pco_item_id": "9"}]});
        let patch = json!({"id": ID, "name": "Sunday", "items": [{"id": I, "kind": "text", "title": "B", "body": "x"}]});
        let out = merge_service(&stored, &patch).unwrap();
        for k in ["styles", "theme", "starts_at", "advance", "stage_layout", "slide_notes", "arrangement", "second", "layout", "breaks", "pco_item_id"] {
            assert_eq!(out["items"][0][k], stored["items"][0][k], "{k}");
        }
        assert_eq!(
            (out["name"].as_str(), out["theme"].clone(), out["items"][0]["title"].as_str()),
            (Some("Sunday"), stored["theme"].clone(), Some("B"))
        );
    }

    #[test]
    fn service_update_kind_change_replaces_item() {
        let stored = json!({"id": ID, "items": [{"id": I, "kind": "text", "title": "A", "body": "x", "styles": []}]});
        let patch = json!({"items": [{"id": I, "kind": "song", "song_id": K}]});
        let out = merge_service(&stored, &patch).unwrap();
        assert_eq!(out["items"][0], patch["items"][0]);
    }

    #[test]
    fn service_update_order_and_removal_follow_patch() {
        let stored = json!({"id": ID, "items": [
            {"id": I, "kind": "text", "title": "A", "theme": T},
            {"id": J, "kind": "text", "title": "B"},
            {"id": K, "kind": "text", "title": "C"},
        ]});
        let new = "00000000-0000-4000-8000-0000000000ee";
        let patch = json!({"items": [{"id": K, "kind": "text"}, {"id": new, "kind": "text", "title": "N"}, {"id": I, "kind": "text"}]});
        let out = merge_service(&stored, &patch).unwrap();
        let ids: Vec<&str> = out["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
        assert_eq!(ids, [K, new, I]);
        assert_eq!((out["items"][0]["title"].as_str(), out["items"][2]["theme"].as_str()), (Some("C"), Some(T)));
        assert_eq!(out["items"][1], patch["items"][1]);
        // Without "items" the stored list stays.
        assert_eq!(merge_service(&stored, &json!({"name": "X"})).unwrap()["items"], stored["items"]);
    }

    #[test]
    fn service_update_rejects_duplicate_ids_and_non_objects() {
        let stored = json!({"id": ID, "items": []});
        assert_eq!(merge_service(&stored, &json!({"items": [{"id": I, "kind": "text"}, {"id": I, "kind": "text"}]})).unwrap_err(), item_twice());
        assert_eq!(merge_service(&stored, &json!([1, 2])).unwrap_err(), expected_object());
        assert_eq!(merge_service(&stored, &json!({"items": [1]})).unwrap_err(), expected_object());
        assert_eq!(merge_object(&json!({}), &json!("x")).unwrap_err(), expected_object());
    }
}
