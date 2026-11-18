//! Data format steps (spec §4.7). `store::load` runs them in order, each after its own "before-update"
//! backup; the format number they work against is `store::SUPPORTED_FORMAT`, the only one in the code.
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::model::{Background, BackgroundEntry, Data, MediaStatus};
use crate::theme::strings;

/// The label of the backup taken before a format step.
pub const BEFORE_UPDATE: &str = "before-update";

/// What a load must do with data of format `found`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Already the format this build writes.
    Current,
    /// Older: run the steps after `found`, then save.
    Migrate,
    /// Saved by a newer build: open read-only.
    Newer,
}

pub fn step(found: u32, supported: u32) -> Step {
    match found.cmp(&supported) {
        std::cmp::Ordering::Less => Step::Migrate,
        std::cmp::Ordering::Equal => Step::Current,
        std::cmp::Ordering::Greater => Step::Newer,
    }
}

/// The `format` of a `data.json`, reading nothing else. Data from before the field existed is format 1.
/// A JSON syntax error is an error (the "could not be read" path).
pub fn read_format(text: &str) -> Result<u32, serde_json::Error> {
    #[derive(Deserialize)]
    struct Header {
        #[serde(default = "one")]
        format: u32,
    }
    fn one() -> u32 {
        1
    }
    serde_json::from_str::<Header>(text).map(|h| h.format)
}

/// Where `data.json` is copied when the "before-update" backup can't be made (a missing asset).
pub fn before_update_copy_name(stamp: &str) -> String {
    format!("data.{BEFORE_UPDATE}-{stamp}.json")
}

/// Format 1 → 2: every distinct picture or video background in use (the church background, then each item's, in
/// service order) gets a library entry named after its media file, else "Background 1", "Background 2", …
/// Nothing else changes: no themes, no overlays, the church default stays Standard.
pub fn migrate_1_to_2(data: &mut Data, now: DateTime<Utc>) {
    let used = std::iter::once(&data.settings.background)
        .chain(data.services.iter().flat_map(|s| s.items.iter().filter_map(|i| i.background.as_ref())))
        .filter(|b| b.asset().is_some())
        .cloned()
        .collect::<Vec<Background>>();
    let mut unnamed = 0;
    for bg in used {
        if data.backgrounds.iter().any(|e| e.background.same_source(&bg)) {
            continue;
        }
        let asset = bg.asset().cloned().unwrap_or_default();
        let name = match data.media.iter().find(|m| m.asset == asset || m.original_asset.as_ref() == Some(&asset)) {
            Some(m) => m.name.clone(),
            None => {
                unnamed += 1;
                strings::numbered_background(unnamed)
            }
        };
        data.backgrounds.push(BackgroundEntry::new(&name, bg.with_dim(0), MediaStatus::Ready, now));
    }
    data.format = 2;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MediaFile, MediaKind, ServiceItem};

    fn image(a: &str) -> Background {
        Background::Image { asset: a.into(), dim: 0 }
    }

    #[test]
    fn read_format_reads_only_the_header() {
        assert_eq!(read_format(r#"{"format":7,"settings":{"theme":"not a theme"}}"#).unwrap(), 7);
        assert_eq!(read_format(r#"{"songs":[]}"#).unwrap(), 1, "older data has no format");
        assert!(read_format("{not json").is_err());
    }

    #[test]
    fn step_compares_with_the_supported_format() {
        assert_eq!((step(1, 2), step(2, 2), step(3, 2)), (Step::Migrate, Step::Current, Step::Newer));
    }

    #[test]
    fn migrate_adds_library_entries_for_used_backgrounds() {
        let mut d = Data::default();
        d.format = 1;
        d.settings.background = image("a.png");
        let item = |bg: Option<Background>| ServiceItem {
            id: uuid::Uuid::new_v4(), kind: crate::model::ItemKind::Text { title: "t".into(), body: "b".into(), layout: Default::default() }, background: bg, notes: String::new(), theme: None,
        };
        d.services[0].items = vec![item(Some(image("a.png"))), item(Some(Background::Video { asset: "v.mp4".into(), dim: 0 })), item(None),
            item(Some(Background::Color { color: "#123456".into() }))];
        migrate_1_to_2(&mut d, Utc::now());
        assert_eq!(d.format, 2);
        let names: Vec<_> = d.backgrounds.iter().map(|e| (e.name.as_str(), e.background.clone(), e.status.clone())).collect();
        assert_eq!(names, [("Background 1", image("a.png"), MediaStatus::Ready), ("Background 2", Background::Video { asset: "v.mp4".into(), dim: 0 }, MediaStatus::Ready)]);
    }

    #[test]
    fn migrate_names_entries_from_media() {
        let mut d = Data::default();
        d.settings.background = image("sky.png");
        d.media.push(MediaFile { id: uuid::Uuid::new_v4(), name: "Blue sky".into(), kind: MediaKind::Image, asset: "sky.png".into(), status: MediaStatus::Ready,
            original_asset: None, duration: None, thumb: None, size: None, folder: String::new() });
        migrate_1_to_2(&mut d, Utc::now());
        assert_eq!(d.backgrounds[0].name, "Blue sky");
        migrate_1_to_2(&mut d, Utc::now());
        assert_eq!(d.backgrounds.len(), 1, "an entry is never added twice");
    }

    #[test]
    fn background_entry_added_is_rfc3339() {
        let mut d = Data::default();
        d.settings.background = image("sky.png");
        let at = DateTime::parse_from_rfc3339("2026-11-06T10:20:30Z").unwrap().with_timezone(&Utc);
        migrate_1_to_2(&mut d, at);
        let json = serde_json::to_value(&d.backgrounds[0]).unwrap();
        assert_eq!(json["added"], "2026-11-06T10:20:30Z");
    }
}
