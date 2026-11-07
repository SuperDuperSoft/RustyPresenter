//! Edit → Undo and Redo for the services and the library. Each `App::edit` keeps what it changed: only the changed
//! entries of each list, as they were and as they became. Settings are never undone. An undo is an edit like any
//! other (`App::undo`), so the live screen never changes by itself: a change to the live item shows the "changed"
//! banner, as an edit there does.
//!
//! A step is only undone when what it changed is still as it left it; otherwise it is dropped and the operator is
//! told. Changes the app makes by itself (`App::edit_untracked`, e.g. a video's length found after an import) are
//! carried into the kept steps instead, so they do not block an undo.
use crate::{
    model::{Data, Deck, MediaFile, Service, ServiceTemplate, Song},
    theme::{SavedTheme, ThemeId, backgrounds::BackgroundEntry, overlay::Overlay},
    tr,
};
use std::{collections::HashMap, hash::Hash};
use uuid::Uuid;

/// How many steps Undo can go back.
pub const LIMIT: usize = 30;

/// An entry of a list in `Data`, known by its key.
trait Keyed: Clone + PartialEq + std::fmt::Debug {
    type Key: Clone + Eq + Hash + std::fmt::Debug;
    fn key(&self) -> Self::Key;
}

macro_rules! keyed_by_id { ($($t:ty),*) => { $(impl Keyed for $t { type Key = Uuid; fn key(&self) -> Uuid { self.id } })* } }
keyed_by_id!(Song, MediaFile, Deck, Service, BackgroundEntry, SavedTheme, Overlay, ServiceTemplate);

impl Keyed for String {
    type Key = String;
    fn key(&self) -> String { self.clone() }
}

/// What one edit did to one list: the keys in order before and after, and the entries that differ.
#[derive(Clone, Debug)]
struct Diff<T: Keyed> {
    keys: [Vec<T::Key>; 2],
    /// The entries of each side with no equal entry on the other.
    changed: [Vec<T>; 2],
}

/// Which side of a step: as the edit found things (`BEFORE`) or as it left them (`AFTER`).
const BEFORE: usize = 0;
const AFTER: usize = 1;

impl<T: Keyed> Diff<T> {
    fn of(a: &[T], b: &[T]) -> Option<Self> {
        if a == b { return None }
        let only = |x: &[T], y: &[T]| {
            let y: HashMap<T::Key, &T> = y.iter().map(|e| (e.key(), e)).collect();
            x.iter().filter(|e| y.get(&e.key()) != Some(e)).cloned().collect()
        };
        Some(Self { keys: [keys(a), keys(b)], changed: [only(a, b), only(b, a)] })
    }

    /// The list is as side `side` left it: the same keys in order, and its changed entries unchanged since.
    fn matches(&self, list: &[T], side: usize) -> bool {
        if keys(list) != self.keys[side] { return false }
        let now: HashMap<T::Key, &T> = list.iter().map(|e| (e.key(), e)).collect();
        self.changed[side].iter().all(|e| now.get(&e.key()) == Some(&e))
    }

    /// Makes the list as side `side` had it. Entries the step did not change are taken from the list as it is.
    fn restore(&self, list: &mut Vec<T>, side: usize) {
        let now: HashMap<T::Key, T> = std::mem::take(list).into_iter().map(|e| (e.key(), e)).collect();
        let kept: HashMap<T::Key, &T> = self.changed[side].iter().map(|e| (e.key(), e)).collect();
        *list = self.keys[side].iter().filter_map(|k| kept.get(k).map(|e| (*e).clone()).or_else(|| now.get(k).cloned())).collect();
    }

    /// The app changed the list from `old` to `new` by itself: side `side`'s entries that were as in `old` become as
    /// in `new`. A list whose keys changed is left alone (the step then no longer matches).
    fn carry(&mut self, old: &[T], new: &[T], side: usize) {
        if keys(old) != keys(new) { return }
        let old: HashMap<T::Key, &T> = old.iter().map(|e| (e.key(), e)).collect();
        let new: HashMap<T::Key, &T> = new.iter().map(|e| (e.key(), e)).collect();
        for e in &mut self.changed[side] {
            if old.get(&e.key()) == Some(&&*e) && let Some(n) = new.get(&e.key()) { *e = (*n).clone() }
        }
    }
}

fn keys<T: Keyed>(list: &[T]) -> Vec<T::Key> { list.iter().map(Keyed::key).collect() }

/// The lists of `Data` that Undo covers, written once: each is a `Diff` in `Changes`.
macro_rules! lists {
    ($($f:ident: $t:ty),*) => {
        #[derive(Clone, Debug, Default)]
        struct Changes {
            $($f: Option<Diff<$t>>,)*
            default_theme: Option<[Option<ThemeId>; 2]>,
        }

        impl Changes {
            fn of(a: &Data, b: &Data) -> Self {
                Self { $($f: Diff::of(&a.$f, &b.$f),)* default_theme: (a.default_theme != b.default_theme).then_some([a.default_theme, b.default_theme]) }
            }
            fn is_empty(&self) -> bool { $(self.$f.is_none() &&)* self.default_theme.is_none() }
            fn matches(&self, d: &Data, side: usize) -> bool {
                $(self.$f.as_ref().is_none_or(|x| x.matches(&d.$f, side)) &&)* self.default_theme.is_none_or(|t| t[side] == d.default_theme)
            }
            fn restore(&self, d: &mut Data, side: usize) {
                $(if let Some(x) = &self.$f { x.restore(&mut d.$f, side) })*
                if let Some(t) = self.default_theme { d.default_theme = t[side] }
            }
            fn carry(&mut self, old: &Data, new: &Data, side: usize) {
                $(if let Some(x) = &mut self.$f { x.carry(&old.$f, &new.$f, side) })*
                if let Some(t) = &mut self.default_theme && t[side] == old.default_theme { t[side] = new.default_theme }
            }
        }
    };
}

lists!(songs: Song, media: MediaFile, media_folders: String, decks: Deck, services: Service, backgrounds: BackgroundEntry,
    themes: SavedTheme, overlays: Overlay, templates: ServiceTemplate);

/// One edit that can be undone.
#[derive(Clone, Debug)]
struct Step {
    /// What it did, for the toast ("adding “Amazing Grace”").
    what: String,
    changes: Changes,
    /// The open service before and after. Undo goes back to it only if the operator has not opened another since.
    current: [Option<Uuid>; 2],
}

/// The steps Undo and Redo can take, newest last.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl History {
    /// Keeps an edit that went from `before` to `after`, unless it changed nothing Undo covers (a settings change,
    /// opening another service). A new step clears Redo.
    pub fn record(&mut self, before: &Data, after: &Data) {
        let changes = Changes::of(before, after);
        if changes.is_empty() { return }
        self.redo.clear();
        self.undo.push(Step { what: describe(before, after), changes, current: [before.current_service, after.current_service] });
        if self.undo.len() > LIMIT { self.undo.remove(0); }
    }

    /// The app changed the data by itself (no step): the kept steps take the change in.
    pub fn carry(&mut self, before: &Data, after: &Data) {
        for s in &mut self.undo { s.changes.carry(before, after, AFTER) }
        for s in &mut self.redo { s.changes.carry(before, after, BEFORE) }
    }

    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }

    /// Undoes the newest step on `data`; the toast's text, or why nothing changed.
    pub fn undo(&mut self, data: &mut Data) -> Result<String, String> {
        let what = self.go(data, true)?;
        Ok(tr!("Undone: {what}", what = what))
    }

    /// Does again the step Undo took last.
    pub fn redo(&mut self, data: &mut Data) -> Result<String, String> {
        let what = self.go(data, false)?;
        Ok(tr!("Redone: {what}", what = what))
    }

    fn go(&mut self, data: &mut Data, back: bool) -> Result<String, String> {
        let (from, to, leave, reach) = if back { (&mut self.undo, &mut self.redo, AFTER, BEFORE) } else { (&mut self.redo, &mut self.undo, BEFORE, AFTER) };
        let Some(step) = from.pop() else { return Err(nothing(!back)) };
        if !step.changes.matches(data, leave) {
            return Err(if back { tr!("Can't undo {what}: it was changed again since.", what = step.what) }
                else { tr!("Can't redo {what}: it was changed again since.", what = step.what) });
        }
        step.changes.restore(data, reach);
        if data.current_service == step.current[leave] { data.current_service = step.current[reach] }
        if data.current_service.is_none_or(|id| !data.services.iter().any(|s| s.id == id)) {
            data.current_service = data.services.first().map(|s| s.id);
        }
        let what = step.what.clone();
        to.push(step);
        Ok(what)
    }
}

/// Undo (`redo` false) or Redo found no step.
pub fn nothing(redo: bool) -> String { if redo { tr!("Nothing to redo.") } else { tr!("Nothing to undo.") } }

/// What an edit did, in a few words: services and songs by name, anything else as "the last change".
fn describe(a: &Data, b: &Data) -> String {
    if a.services.len() != b.services.len() || keys(&a.services) != keys(&b.services) {
        if let Some(s) = b.services.iter().find(|s| !a.services.iter().any(|x| x.id == s.id)) {
            return tr!("the new service “{name}”", name = s.name);
        }
        if let Some(s) = a.services.iter().find(|s| !b.services.iter().any(|x| x.id == s.id)) {
            return tr!("deleting the service “{name}”", name = s.name);
        }
    }
    if let Some((was, now)) = b.services.iter().find_map(|s| a.services.iter().find(|x| x.id == s.id && *x != s).map(|x| (x, s))) {
        let title = |d: &Data, i: &crate::model::ServiceItem| crate::slides::build_item_with_notes(d, i).0;
        if let Some(i) = now.items.iter().find(|i| !was.items.iter().any(|x| x.id == i.id)) {
            if now.items.len() > was.items.len() { return tr!("adding “{name}”", name = title(b, i)) }
        }
        if let Some(i) = was.items.iter().find(|i| !now.items.iter().any(|x| x.id == i.id)) {
            if now.items.len() < was.items.len() { return tr!("removing “{name}”", name = title(a, i)) }
        }
        if was.items.len() == now.items.len() && was.items.iter().all(|i| now.items.contains(i))
            && let Some(i) = now.items.iter().zip(&was.items).find(|(x, y)| x.id != y.id).map(|(x, _)| x) {
            return tr!("moving “{name}”", name = title(b, i));
        }
        return tr!("changes to “{name}”", name = now.name);
    }
    if let Some(s) = b.songs.iter().find(|s| !a.songs.iter().any(|x| x.id == s.id)) {
        return tr!("the new song “{name}”", name = s.title);
    }
    if let Some(s) = a.songs.iter().find(|s| !b.songs.iter().any(|x| x.id == s.id)) {
        return tr!("deleting the song “{name}”", name = s.title);
    }
    if let Some(s) = b.songs.iter().find(|s| a.songs.iter().any(|x| x.id == s.id && x != *s)) {
        return tr!("changes to the song “{name}”", name = s.title);
    }
    tr!("the last change")
}

#[cfg(test)]
#[path = "undo_tests.rs"]
mod tests;
