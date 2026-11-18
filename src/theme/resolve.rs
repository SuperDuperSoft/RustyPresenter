//! Theme lookups on the app's data (spec §4.2): FEAT-6 and FEAT-7 call these. Resolution is item > service > church
//! default; the background is the item's own, else the chosen theme's, else Standard's.
use uuid::Uuid;

use crate::model::{Data, ItemKind, Service, ServiceItem};
use crate::theme::{ItemThemeRef, Resolved, SavedTheme, ThemeId};

/// Standard, then a saved theme (or a built-in's edited copy), then a built-in.
pub fn find(data: &Data, id: ThemeId) -> Option<SavedTheme> {
    data.catalog().find(id)
}

/// The starred theme, or Standard when none is starred or it is gone.
pub fn church_default(data: &Data) -> SavedTheme {
    data.catalog().church_default()
}

/// Whether an item draws text in a theme (songs, scripture, announcements); decks and media only take a background.
pub fn draws_text(item: &ServiceItem) -> bool {
    matches!(item.kind, ItemKind::Song { .. } | ItemKind::Scripture { .. } | ItemKind::Text { .. })
}

/// The style and background `item` shows with, in `service` (`None`: no service theme). The style is the theme's
/// layout for the item (N25): songs Lyrics, passages Scripture, an announcement its own pick.
pub fn resolve(data: &Data, service: Option<&Service>, item: &ServiceItem) -> Resolved {
    let item_ref = ItemThemeRef { theme: item.theme, background: item.background.as_ref(), draws_text: draws_text(item) };
    let r = data.catalog().resolve(service.and_then(|s| s.theme), item_ref);
    Resolved { style: crate::theme::layouts::apply(&r.style, crate::theme::layouts::for_item(&item.kind)), ..r }
}

/// The service holding item `item_id`, searching every service (the open one first).
pub fn service_of(data: &Data, item_id: Uuid) -> Option<(&Service, &ServiceItem)> {
    let open = data.service().map(|s| s.id);
    let mut services: Vec<&Service> = data.services.iter().collect();
    services.sort_by_key(|s| Some(s.id) != open);
    services.into_iter().find_map(|s| s.items.iter().find(|i| i.id == item_id).map(|i| (s, i)))
}

/// How many items and services name theme `id` (for the delete question and "Used by N items").
pub fn usage(data: &Data, id: ThemeId) -> (usize, usize) {
    let items = data.services.iter().flat_map(|s| &s.items).filter(|i| i.theme == Some(id)).count();
    let services = data.services.iter().filter(|s| s.theme == Some(id)).count();
    (items, services)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Background, Theme};
    use crate::theme::{STANDARD_ID, ThemeSource};

    fn saved(name: &str, size: u32) -> SavedTheme {
        SavedTheme { id: Uuid::new_v4(), name: name.into(), style: Theme { size, ..Theme::default() }, background: None }
    }

    fn item(kind: ItemKind, theme: Option<ThemeId>) -> ServiceItem {
        ServiceItem { id: Uuid::new_v4(), kind, background: None, notes: String::new(), theme }
    }

    fn text() -> ItemKind {
        ItemKind::Text { title: "t".into(), body: "b".into(), layout: Default::default() }
    }

    #[test]
    fn theme_find_reads_saved_and_builtin_themes() {
        let mut d = Data::default();
        let x = saved("Christmas", 90);
        d.themes.push(x.clone());
        assert_eq!(find(&d, x.id), Some(x));
        assert_eq!(find(&d, STANDARD_ID).unwrap().style, d.settings.theme);
        for b in crate::theme::builtin() {
            assert_eq!(find(&d, b.id), Some(b));
        }
        assert_eq!(find(&d, Uuid::new_v4()), None);
    }

    #[test]
    fn church_default_reads_data_default_theme() {
        let mut d = Data::default();
        let x = saved("Christmas", 90);
        d.themes.push(x.clone());
        d.default_theme = Some(x.id);
        assert_eq!(church_default(&d), x);
    }

    #[test]
    fn church_default_dangling_falls_back_to_standard() {
        let mut d = Data::default();
        d.default_theme = Some(Uuid::new_v4());
        assert_eq!(church_default(&d).id, STANDARD_ID);
    }

    #[test]
    fn resolve_wrapper_uses_item_then_service_then_default() {
        let mut d = Data::default();
        let (a, b, c) = (saved("A", 50), saved("B", 60), saved("C", 70));
        d.themes = vec![a.clone(), b.clone(), c.clone()];
        d.default_theme = Some(c.id);
        d.services[0].theme = Some(b.id);
        let own = item(text(), Some(a.id));
        let plain = item(text(), None);
        let s = Some(&d.services[0]);
        assert_eq!(resolve(&d, s, &own).from, ThemeSource::Item(a.id));
        assert_eq!(resolve(&d, s, &plain).from, ThemeSource::Service(b.id));
        assert_eq!(resolve(&d, None, &plain).style.size, 70);
        let deck = item(ItemKind::Deck { deck_id: Uuid::new_v4() }, Some(a.id));
        assert_eq!(resolve(&d, s, &deck).from, ThemeSource::Service(b.id), "decks ignore an item theme");
        let mut with_bg = plain.clone();
        with_bg.background = Some(Background::Color { color: "#ff0000".into() });
        assert_eq!(resolve(&d, s, &with_bg).background, Background::Color { color: "#ff0000".into() });
    }

    #[test]
    fn resolve_draws_an_announcement_with_its_layout() {
        let mut d = Data::default();
        let plain = item(text(), None);
        assert_eq!(resolve(&d, None, &plain).style, d.settings.theme, "Text changes nothing");
        let title = item(ItemKind::Text { title: "t".into(), body: "b".into(), layout: crate::theme::LayoutKind::Title }, None);
        assert_eq!(resolve(&d, None, &title).style.weight, 700);
        // A theme's own Title layout wins over the shipped one.
        let mut x = saved("Bold titles", 100);
        x.style.layouts.set(crate::theme::LayoutKind::Title, crate::theme::Layout { size: 50, ..Default::default() });
        d.themes.push(x.clone());
        let title = ServiceItem { theme: Some(x.id), ..title };
        assert_eq!((resolve(&d, None, &title).style.size, resolve(&d, None, &title).style.weight), (50, 600));
    }

    #[test]
    fn service_of_finds_item_in_unselected_service() {
        let mut d = Data::default();
        let mut other = Service::new("Evening");
        let it = item(text(), None);
        other.items.push(it.clone());
        d.services.push(other.clone());
        let (s, i) = service_of(&d, it.id).unwrap();
        assert_eq!((s.id, i.id), (other.id, it.id));
        assert!(service_of(&d, Uuid::new_v4()).is_none());
    }

    #[test]
    fn usage_counts_items_and_services() {
        let mut d = Data::default();
        let x = saved("X", 80);
        d.services[0].theme = Some(x.id);
        d.services[0].items = vec![item(text(), Some(x.id)), item(text(), Some(x.id)), item(text(), None)];
        assert_eq!(usage(&d, x.id), (2, 1));
    }
}
