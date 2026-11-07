//! Saved themes, the church default and theme resolution (spec §4.2). Standard is not stored with
//! the saved themes: its style and background live in `Settings`. Built-ins come from code and are
//! never stored either; saving one stores an override with the same id.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::theme::backgrounds::Background;
use crate::theme::strings;
use crate::theme::{Theme, clamp_style};

pub type ThemeId = Uuid;

/// The fixed id of the Standard theme.
pub const STANDARD_ID: ThemeId = Uuid::from_u128(0x00000000_0000_4000_8000_000000000101);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedTheme {
    pub id: ThemeId,
    pub name: String,
    pub style: Theme,
    /// None: the theme uses Standard's background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Background>,
}

/// The built-in themes defined in code. FEAT-1 has none; FEAT-7 adds its stream lower third here.
/// The built-in lower-third theme: smaller text at the bottom on a see-through band, for a livestream or a screen
/// where the picture must stay visible (FEAT-7's stream screen view uses it).
pub const LOWER_THIRD_ID: ThemeId = Uuid::from_u128(0x00000000_0000_4000_8000_000000000102);

/// The read-only built-in themes, with fixed ids. Never stored in `Data.themes`; saving one stores an edited copy
/// there, which `find` prefers (deleting that copy is Reset).
pub fn builtin() -> Vec<SavedTheme> {
    let style = Theme { size: 64, scripture_size: 52, position: "lower".into(), band: true, band_opacity: 55, ..Theme::default() };
    vec![SavedTheme { id: LOWER_THIRD_ID, name: strings::lower_third_name().into(), style, background: None }]
}

/// Everything theme lookups need, borrowed from the stored data.
#[derive(Clone, Debug)]
pub struct Catalog<'a> {
    pub standard_style: &'a Theme,
    pub standard_background: &'a Background,
    pub saved: &'a [SavedTheme],
    pub default_theme: Option<ThemeId>,
    pub builtins: Vec<SavedTheme>,
}

/// What an item says about its theme. `draws_text` is false for decks and media, which skip `theme`.
#[derive(Clone, Copy, Debug)]
pub struct ItemThemeRef<'a> {
    pub theme: Option<ThemeId>,
    pub background: Option<&'a Background>,
    pub draws_text: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub style: Theme,
    pub background: Background,
    /// The level that supplied the theme.
    pub from: ThemeSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeSource {
    Item(ThemeId),
    Service(ThemeId),
    Church(ThemeId),
}

/// One row of Settings → Themes.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeRow {
    pub id: ThemeId,
    pub name: String,
    pub builtin: bool,
    /// A built-in that has a saved override (offered Reset).
    pub overridden: bool,
    pub is_default: bool,
}

impl<'a> Catalog<'a> {
    pub fn new(standard_style: &'a Theme, standard_background: &'a Background, saved: &'a [SavedTheme], default_theme: Option<ThemeId>) -> Self {
        Self { standard_style, standard_background, saved, default_theme, builtins: builtin() }
    }

    pub fn with_builtins(mut self, b: Vec<SavedTheme>) -> Self {
        self.builtins = b;
        self
    }

    pub fn standard(&self) -> SavedTheme {
        SavedTheme { id: STANDARD_ID, name: strings::standard_name().into(), style: self.standard_style.clone(), background: None }
    }

    /// Standard, then a saved theme (a built-in's override wins), then a built-in.
    pub fn find(&self, id: ThemeId) -> Option<SavedTheme> {
        if id == STANDARD_ID {
            return Some(self.standard());
        }
        self.saved.iter().chain(&self.builtins).find(|t| t.id == id).cloned()
    }

    pub fn church_default(&self) -> SavedTheme {
        self.default_theme.and_then(|id| self.find(id)).unwrap_or_else(|| self.standard())
    }

    /// Theme: item (text items only), then service, then the church default; dangling ids are skipped.
    /// Background: the item's own, else the chosen theme's, else Standard's.
    pub fn resolve(&self, service_theme: Option<ThemeId>, item: ItemThemeRef<'_>) -> Resolved {
        let level = |id: Option<ThemeId>, from: fn(ThemeId) -> ThemeSource| id.and_then(|id| self.find(id)).map(|t| (from(t.id), t));
        let (from, theme) = level(item.theme.filter(|_| item.draws_text), ThemeSource::Item)
            .or_else(|| level(service_theme, ThemeSource::Service))
            .unwrap_or_else(|| {
                let t = self.church_default();
                (ThemeSource::Church(t.id), t)
            });
        let background = item.background.cloned().or(theme.background).unwrap_or_else(|| self.standard_background.clone());
        Resolved { style: theme.style, background, from }
    }

    /// Standard, then the built-ins (an override's name shown when there is one), then the other saved
    /// themes by name, ignoring case.
    pub fn rows(&self) -> Vec<ThemeRow> {
        let default_id = self.church_default().id;
        let row = |t: &SavedTheme, builtin: bool, overridden: bool| ThemeRow { id: t.id, name: t.name.clone(), builtin, overridden, is_default: t.id == default_id };
        let mut rows = vec![row(&self.standard(), true, false)];
        for b in &self.builtins {
            match self.saved.iter().find(|s| s.id == b.id) {
                Some(o) => rows.push(row(o, true, true)),
                None => rows.push(row(b, true, false)),
            }
        }
        let mut own: Vec<&SavedTheme> = self.saved.iter().filter(|s| s.id != STANDARD_ID && !self.is_builtin(s.id)).collect();
        own.sort_by_key(|s| s.name.to_lowercase());
        rows.extend(own.into_iter().map(|s| row(s, false, false)));
        rows
    }

    /// True when another theme (saved or built-in) has this name, or it is reserved; ignores case.
    pub fn name_taken(&self, name: &str, except: ThemeId) -> bool {
        let lower = name.trim().to_lowercase();
        is_reserved(&lower) || self.saved.iter().chain(&self.builtins).any(|t| t.id != except && t.name.trim().to_lowercase() == lower)
    }

    fn is_builtin(&self, id: ThemeId) -> bool {
        self.builtins.iter().any(|b| b.id == id)
    }
}

fn is_reserved(lower: &str) -> bool {
    [strings::standard_name(), strings::church_default()].iter().any(|r| r.to_lowercase() == lower)
}

/// Checks a theme sent by `theme/save` and returns it ready to store: name trimmed, required, not
/// reserved and unique; a nil id gets a new one; the style is clamped. Standard is saved elsewhere.
pub fn validate_save(cat: &Catalog, mut t: SavedTheme) -> Result<SavedTheme, String> {
    if t.id == STANDARD_ID {
        return Err(strings::standard_save_elsewhere().into());
    }
    t.name = t.name.trim().to_string();
    if t.name.is_empty() {
        return Err(strings::theme_name_required().into());
    }
    if is_reserved(&t.name.to_lowercase()) {
        return Err(strings::theme_name_reserved().into());
    }
    if cat.name_taken(&t.name, t.id) {
        return Err(strings::theme_exists(&t.name));
    }
    if t.id.is_nil() {
        t.id = Uuid::new_v4();
    }
    clamp_style(&mut t.style);
    Ok(t)
}

/// The value `theme/make-default` stores: Standard is stored as no default.
pub fn default_ref(id: ThemeId) -> Option<ThemeId> {
    (id != STANDARD_ID).then_some(id)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteKind {
    Saved,
    /// Removes a built-in's override.
    ResetBuiltin,
}

pub fn delete_kind(cat: &Catalog, id: ThemeId) -> Result<DeleteKind, String> {
    let stored = cat.saved.iter().any(|t| t.id == id);
    if id == STANDARD_ID || (cat.is_builtin(id) && !stored) {
        Err(strings::builtin_no_delete().into())
    } else if cat.is_builtin(id) {
        Ok(DeleteKind::ResetBuiltin)
    } else if stored {
        Ok(DeleteKind::Saved)
    } else {
        Err(strings::theme_gone().into())
    }
}

/// "<base> copy", then "<base> copy 2", "<base> copy 3", … until one is free.
pub fn copy_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let first = format!("{base} copy");
    if !taken(&first) {
        return first;
    }
    (2..).map(|n| format!("{base} copy {n}")).find(|n| !taken(n)).expect("an unused name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::strings::*;

    const CHRISTMAS: ThemeId = Uuid::from_u128(0xc1);
    const YOUTH: ThemeId = Uuid::from_u128(0xa1);
    const LOWER: ThemeId = Uuid::from_u128(0xb1);

    fn gradient() -> Background {
        Background::Gradient { from: "#3a2412".into(), to: "#000000".into(), angle: 180, dim: 0 }
    }

    /// Standard: the default style on black; saved: "Christmas" (bold, gradient) and "Youth" (no background).
    fn fx() -> (Theme, Background, Vec<SavedTheme>) {
        let christmas = SavedTheme { id: CHRISTMAS, name: "Christmas".into(), style: Theme { weight: 700, ..Theme::default() }, background: Some(gradient()) };
        let youth = SavedTheme { id: YOUTH, name: "Youth".into(), style: Theme { size: 96, ..Theme::default() }, background: None };
        (Theme::default(), Background::Color { color: "#000000".into() }, vec![christmas, youth])
    }

    fn text_item(theme: Option<ThemeId>) -> ItemThemeRef<'static> {
        ItemThemeRef { theme, background: None, draws_text: true }
    }

    fn lower_third() -> SavedTheme {
        SavedTheme { id: LOWER, name: "Lower third".into(), style: Theme { position: "lower".into(), ..Theme::default() }, background: None }
    }

    fn named(name: &str) -> SavedTheme {
        SavedTheme { id: Uuid::nil(), name: name.into(), style: Theme::default(), background: None }
    }

    #[test]
    fn item_theme_beats_service_and_default() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, Some(CHRISTMAS));
        let r = cat.resolve(Some(CHRISTMAS), text_item(Some(YOUTH)));
        assert_eq!(r.style, saved[1].style);
        assert_eq!(r.from, ThemeSource::Item(YOUTH));
    }

    #[test]
    fn service_theme_beats_default() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, Some(CHRISTMAS));
        let r = cat.resolve(Some(YOUTH), text_item(None));
        assert_eq!((r.style, r.from), (saved[1].style.clone(), ThemeSource::Service(YOUTH)));
    }

    #[test]
    fn dangling_item_theme_falls_through() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, None);
        let r = cat.resolve(Some(CHRISTMAS), text_item(Some(Uuid::new_v4())));
        assert_eq!(r.from, ThemeSource::Service(CHRISTMAS));
        assert_eq!(r.background, gradient());
        let r = cat.resolve(Some(Uuid::new_v4()), text_item(Some(Uuid::new_v4())));
        assert_eq!((r.from, r.style), (ThemeSource::Church(STANDARD_ID), Theme::default()));
    }

    #[test]
    fn theme_without_background_uses_standard_background_not_service_theme() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, None);
        let r = cat.resolve(Some(CHRISTMAS), text_item(Some(YOUTH)));
        assert_eq!(r.background, Background::Color { color: "#000000".into() });
    }

    #[test]
    fn item_background_beats_theme_background() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, None);
        let own = Background::Image { asset: "cross.png".into(), dim: 30 };
        let r = cat.resolve(None, ItemThemeRef { theme: Some(CHRISTMAS), background: Some(&own), draws_text: true });
        assert_eq!((r.background, r.from), (own, ThemeSource::Item(CHRISTMAS)));
    }

    #[test]
    fn deck_and_media_items_ignore_theme() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, Some(CHRISTMAS));
        let deck = ItemThemeRef { theme: Some(YOUTH), background: None, draws_text: false };
        assert_eq!(cat.resolve(Some(CHRISTMAS), deck).from, ThemeSource::Service(CHRISTMAS));
        let deck = ItemThemeRef { theme: Some(YOUTH), background: None, draws_text: false };
        assert_eq!(cat.resolve(None, deck).from, ThemeSource::Church(CHRISTMAS));
    }

    #[test]
    fn starred_theme_is_church_default() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, Some(CHRISTMAS));
        assert_eq!(cat.church_default(), saved[0]);
        let r = cat.resolve(None, text_item(None));
        assert_eq!((r.from, r.background), (ThemeSource::Church(CHRISTMAS), gradient()));
        assert!(cat.rows().iter().find(|r| r.id == CHRISTMAS).unwrap().is_default);
        assert!(!cat.rows()[0].is_default);
    }

    #[test]
    fn dangling_default_falls_back_to_standard() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, Some(Uuid::new_v4()));
        assert_eq!(cat.church_default().id, STANDARD_ID);
        assert_eq!(cat.church_default().name, standard_name());
        assert!(cat.rows()[0].is_default);
    }

    #[test]
    fn builtin_theme_resolves_and_override_wins() {
        let (s, b, saved) = fx();
        assert_eq!(builtin().iter().map(|b| b.id).collect::<Vec<_>>(), [LOWER_THIRD_ID]);
        let cat = Catalog::new(&s, &b, &saved, None).with_builtins(vec![lower_third()]);
        assert_eq!(cat.find(LOWER), Some(lower_third()));
        assert_eq!(cat.resolve(Some(LOWER), text_item(None)).from, ThemeSource::Service(LOWER));
        let row = cat.rows().into_iter().find(|r| r.id == LOWER).unwrap();
        assert!(row.builtin && !row.overridden);

        let mut with_override = saved.clone();
        let changed = SavedTheme { style: Theme { size: 50, ..lower_third().style }, ..lower_third() };
        with_override.push(changed.clone());
        let cat = Catalog::new(&s, &b, &with_override, None).with_builtins(vec![lower_third()]);
        assert_eq!(cat.find(LOWER), Some(changed));
        let rows: Vec<_> = cat.rows().into_iter().filter(|r| r.id == LOWER).collect();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].builtin && rows[0].overridden);
    }

    #[test]
    fn rows_list_standard_first_then_builtins_then_saved_by_name() {
        let (s, b, mut saved) = fx();
        saved.push(SavedTheme { id: Uuid::from_u128(0xd1), name: "advent".into(), style: Theme::default(), background: None });
        let cat = Catalog::new(&s, &b, &saved, None).with_builtins(vec![lower_third()]);
        let rows = cat.rows();
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Standard", "Lower third", "advent", "Christmas", "Youth"]);
        assert!(rows[0].builtin && rows[0].id == STANDARD_ID);
        assert!(!rows[2].builtin && !rows[2].overridden);
    }

    #[test]
    fn validate_rejects_blank_reserved_and_case_duplicates() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, None).with_builtins(vec![lower_third()]);
        let christmas = saved[0].clone();
        assert_eq!(validate_save(&cat, named("   ")).unwrap_err(), theme_name_required());
        assert_eq!(validate_save(&cat, named("church DEFAULT")).unwrap_err(), theme_name_reserved());
        assert_eq!(validate_save(&cat, named(" standard")).unwrap_err(), theme_name_reserved());
        assert_eq!(validate_save(&cat, named("christmas")).unwrap_err(), theme_exists("christmas"));
        assert_eq!(validate_save(&cat, named("LOWER THIRD")).unwrap_err(), theme_exists("LOWER THIRD"));
        let ok = validate_save(&cat, SavedTheme { id: CHRISTMAS, name: "Christmas ".into(), ..christmas }).unwrap();
        assert_eq!(ok.name, "Christmas");
        // Saving a built-in under its own name stores the override.
        assert!(validate_save(&cat, lower_third()).is_ok());
    }

    #[test]
    fn validate_assigns_id_and_clamps() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, None);
        let t = SavedTheme { style: Theme { weight: 900, size: 5, ..Theme::default() }, ..named("Easter") };
        let t = validate_save(&cat, t).unwrap();
        assert!(!t.id.is_nil());
        assert_eq!((t.style.weight, t.style.size), (700, 24));
    }

    #[test]
    fn validate_rejects_standard_id() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, None);
        let t = SavedTheme { id: STANDARD_ID, name: "Mine".into(), style: Theme::default(), background: None };
        assert_eq!(validate_save(&cat, t).unwrap_err(), standard_save_elsewhere());
    }

    #[test]
    fn delete_kind_refuses_standard_and_unedited_builtin() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, None).with_builtins(vec![lower_third()]);
        assert_eq!(delete_kind(&cat, STANDARD_ID).unwrap_err(), builtin_no_delete());
        assert_eq!(delete_kind(&cat, LOWER).unwrap_err(), builtin_no_delete());
        assert_eq!(delete_kind(&cat, Uuid::new_v4()).unwrap_err(), theme_gone());
        assert!(matches!(delete_kind(&cat, YOUTH), Ok(DeleteKind::Saved)));

        let mut with_override = saved.clone();
        with_override.push(lower_third());
        let cat = Catalog::new(&s, &b, &with_override, None).with_builtins(vec![lower_third()]);
        assert!(matches!(delete_kind(&cat, LOWER), Ok(DeleteKind::ResetBuiltin)));
    }

    #[test]
    fn copy_name_counts_up() {
        let taken = ["A copy", "A copy 2"];
        assert_eq!(copy_name("A", |n| taken.contains(&n)), "A copy 3");
        assert_eq!(copy_name("B", |n| taken.contains(&n)), "B copy");
    }

    #[test]
    fn make_default_keeps_previous_default_as_theme() {
        let (s, b, saved) = fx();
        let cat = Catalog::new(&s, &b, &saved, Some(CHRISTMAS));
        assert_eq!(cat.church_default().id, CHRISTMAS);
        assert_eq!(default_ref(CHRISTMAS), Some(CHRISTMAS));
        assert_eq!(default_ref(STANDARD_ID), None);
        let cat = Catalog::new(&s, &b, &saved, default_ref(STANDARD_ID));
        assert_eq!(cat.church_default(), cat.standard());
        assert_eq!(cat.resolve(None, text_item(None)).style, Theme::default());
        assert_eq!(cat.find(CHRISTMAS), Some(saved[0].clone()));
    }
}
