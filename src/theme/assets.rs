//! Which asset files FEAT-1 data uses (spec §4.7). Any future asset cleanup must keep the union of
//! `referenced` and the assets of the live and committed frames (`slot_assets` for their overlay
//! slots), so deleting an overlay or background that is on air never deletes its file.
use std::collections::BTreeSet;

use crate::model::AssetId;
use crate::theme::SavedTheme;
use crate::theme::backgrounds::{Background, BackgroundEntry};
use crate::theme::overlay::{ImageSource, Overlay, OverlaySlot, OverlaySource};

fn source_asset(source: &OverlaySource) -> Option<&AssetId> {
    match source {
        OverlaySource::Image { source: ImageSource::Asset { asset }, .. } => Some(asset),
        _ => None,
    }
}

/// Assets used by saved themes' backgrounds, background library entries (file and thumbnail) and
/// saved picture overlays.
pub fn referenced(themes: &[SavedTheme], backgrounds: &[BackgroundEntry], overlays: &[Overlay]) -> BTreeSet<AssetId> {
    let themes = themes.iter().filter_map(|t| t.background.as_ref().and_then(Background::asset));
    let entries = backgrounds.iter().flat_map(|e| e.background.asset().into_iter().chain(e.thumb.as_ref()));
    let pictures = overlays.iter().filter_map(|o| source_asset(&o.source));
    themes.chain(entries).chain(pictures).cloned().collect()
}

/// Assets shown by the overlay slots of a frame.
pub fn slot_assets(slots: &[OverlaySlot]) -> BTreeSet<AssetId> {
    slots.iter().filter_map(|s| source_asset(&s.source)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MediaStatus;
    use crate::theme::Theme;
    use crate::theme::overlay::{ImageSource, OverlayKind, OverlayLook, OverlaySource, Placement, slots};
    use chrono::Utc;
    use uuid::Uuid;

    fn theme(background: Option<Background>) -> SavedTheme {
        SavedTheme { id: Uuid::new_v4(), name: "T".into(), style: Theme::default(), background }
    }

    fn picture(asset: &str) -> Overlay {
        Overlay {
            id: Uuid::new_v4(),
            name: asset.into(),
            source: OverlaySource::Image { source: ImageSource::Asset { asset: asset.into() }, opacity: 100 },
            place: Placement::default(),
        }
    }

    fn set(ids: &[&str]) -> BTreeSet<AssetId> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn referenced_assets_include_themes_backgrounds_overlays() {
        let themes = [
            theme(Some(Background::Image { asset: "theme.png".into(), dim: 0 })),
            theme(Some(Background::Color { color: "#000000".into() })),
            theme(None),
        ];
        let mut video = BackgroundEntry::new("Waves", Background::Video { asset: "waves.mp4".into(), dim: 20 }, MediaStatus::Ready, Utc::now());
        video.thumb = Some("waves-thumb.png".into());
        let gradient =
            BackgroundEntry::new("Dusk", Background::Gradient { from: "#000".into(), to: "#fff".into(), angle: 90, dim: 0 }, MediaStatus::Ready, Utc::now());
        let overlays = [
            picture("bug.png"),
            Overlay { source: OverlaySource::Image { source: ImageSource::ChurchLogo, opacity: 100 }, ..picture("x") },
            Overlay {
                source: OverlaySource::Text { text: "Hi".into(), styles: Vec::new(), look: OverlayLook::default() },
                place: Placement::default_for(OverlayKind::Text),
                ..picture("y")
            },
        ];
        assert_eq!(referenced(&themes, &[video, gradient], &overlays), set(&["bug.png", "theme.png", "waves-thumb.png", "waves.mp4"]));
        assert!(referenced(&[], &[], &[]).is_empty());
    }

    #[test]
    fn referenced_assets_plus_live_frame_covers_deleted_overlay() {
        let mut library = vec![picture("keep.png"), picture("deleted.png")];
        let on: Vec<_> = library.iter().enumerate().map(|(i, o)| slots::saved_slot(o.id, &library, None, i as u64).unwrap()).collect();
        library.pop();
        let stored = referenced(&[], &[], &library);
        assert!(!stored.contains("deleted.png"));
        let live = slot_assets(&on);
        assert_eq!(live, set(&["deleted.png", "keep.png"]));
        let all: BTreeSet<AssetId> = stored.union(&live).cloned().collect();
        assert!(all.contains("deleted.png") && all.contains("keep.png"));
    }
}
