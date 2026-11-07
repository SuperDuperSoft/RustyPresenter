//! FEAT-5's operator actions, reached through `App::do_action`: keyword search over the Bibles stored on this
//! computer (each index built lazily in the background; closing the dialog cancels a build), a whole chapter for the
//! search's context view, and the Slide breaks dialog's plan. None of them is on the phone's list.
use super::breaks_view::breaks_view;
use super::fetch::{BibleReaders, Lookup};
use super::input::chapter_reference;
use super::search::{SearchReply, SearchRequest, SearchService};
use super::sources::Sources;
use super::strings::{not_available, searching_instead};
use super::VerseBreaks;
use crate::app::{ActionError, App};
use crate::bible;
use crate::model::{ItemKind, VerseText};
use serde::Deserialize;
use serde_json::{Value, json};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use uuid::Uuid;

/// The actions `run` answers.
pub fn handles(action: &str) -> bool {
    matches!(action, "bible/search" | "bible/searchable" | "bible/search-cancel" | "bible/chapter" | "scripture/breaks")
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ActionError> {
    serde_json::from_slice(body).map_err(|e| ActionError::bad(crate::tr!("Bad request: {error}", error = e)))
}

/// Reads the Bibles stored on this computer: the built-in ones, then the installed ones. Never the internet.
pub struct LocalReaders { pub root: PathBuf }

impl BibleReaders for LocalReaders {
    fn local(&self, tr: &str, input: &str) -> Lookup {
        if let Some(b) = bible::BUILT_IN.iter().find(|(id, _)| bible::id::same_id(id, tr)).and_then(|(id, _)| bible::built_in(id)) {
            return b.lookup(input);
        }
        let id = bible::import::list(&self.root).iter().find(|m| bible::id::same_id(&m.id, tr)).map(|m| m.id.clone());
        match id.and_then(|id| bible::import::get(&self.root, &id)) {
            Some(b) => b.lookup(input),
            None => Err(not_available(tr)),
        }
    }

    fn online<'a>(&'a self, _root: &'a Path, tr: &'a str, _input: &'a str) -> Pin<Box<dyn Future<Output = Lookup> + Send + 'a>> {
        Box::pin(async move { Err(not_available(tr)) })
    }
}

/// A Bible the search can read: its id, its short name and its full name.
pub struct Searchable { pub id: String, pub label: String, pub name: String }

/// Every Bible stored on this computer that may be used: the built-in ones, then the installed ones by id.
pub fn searchable(root: &Path) -> Vec<Searchable> {
    let built = bible::BUILT_IN.iter().map(|(id, name)| Searchable { id: id.to_string(), label: id.to_string(), name: name.to_string() });
    let installed = bible::import::list(root).iter().filter(|m| !m.blocked() && !bible::BUILT_IN.iter().any(|(id, _)| bible::id::same_id(id, &m.id)))
        .map(|m| Searchable { id: m.id.clone(), label: if m.abbr.is_empty() { m.id.clone() } else { m.abbr.clone() }, name: m.name.clone() }).collect::<Vec<_>>();
    built.chain(installed).collect()
}

/// The translation a search uses: the one asked for when it is stored here, else the first one that is, with the
/// note that says so.
fn search_translation(local: &[Searchable], asked: &str) -> (String, Option<String>) {
    match local.iter().find(|b| bible::id::same_id(&b.id, asked)) {
        Some(b) => (b.id.clone(), None),
        None => {
            let first = local.first().map_or_else(|| "KJV".to_string(), |b| b.id.clone());
            let note = (!asked.trim().is_empty()).then(|| searching_instead(asked.trim(), &first));
            (first, note)
        }
    }
}

impl App {
    /// The search service, made on first use with the current runtime.
    fn search_service(&self) -> &SearchService {
        self.search.get_or_init(|| SearchService::new(tokio::runtime::Handle::current(), Arc::new(LocalReaders { root: self.store.root.clone() }), Arc::new(|_| {})))
    }
}

pub async fn run(app: &Arc<App>, action: &str, body: &[u8]) -> Result<Value, ActionError> {
    let root = app.store.root.clone();
    match action {
        "bible/searchable" => {
            let list = searchable(&root);
            Ok(json!({ "bibles": list.iter().map(|b| json!({ "id": b.id, "label": b.label, "name": b.name })).collect::<Vec<_>>() }))
        }
        "bible/search" => {
            let mut req: SearchRequest = parse(body)?;
            let local = searchable(&root);
            let (translation, note) = search_translation(&local, &req.translation);
            req.translation = translation.clone();
            let sources = Sources { rows: vec![], local: local.iter().map(|b| b.id.clone()).collect() };
            let reply = app.search_service().search(&sources, &req).map_err(ActionError::bad)?;
            let mut v = serde_json::to_value(&reply).map_err(|e| ActionError::bad(e.to_string()))?;
            v["translation"] = json!(translation);
            v["note"] = json!(note);
            if matches!(reply, SearchReply::Pending) { v["label"] = json!(app.abbr_of(&translation)) }
            Ok(v)
        }
        "bible/search-cancel" => {
            if let Some(s) = app.search.get() { s.cancel() }
            Ok(json!({}))
        }
        "bible/chapter" => {
            #[derive(Deserialize)] struct C { translation: String, book: u8, chapter: u16 }
            let C { translation, book, chapter } = parse(body)?;
            if usize::from(book) >= bible::BOOKS.len() { return Err(ActionError::bad(not_available(&translation))) }
            let readers = LocalReaders { root };
            let (_, verses) = tokio::task::spawn_blocking(move || readers.local(&translation, &chapter_reference(book, chapter)))
                .await.map_err(|e| ActionError::bad(e.to_string()))?.map_err(ActionError::bad)?;
            Ok(json!({ "verses": verses }))
        }
        "scripture/breaks" => {
            #[derive(Deserialize)] struct B { id: Uuid, #[serde(default)] breaks: Option<VerseBreaks>, #[serde(default)] per_slide: Option<u8> }
            let B { id, breaks, per_slide } = parse(body)?;
            let d = app.data.lock().unwrap();
            let item = d.service().and_then(|s| s.items.iter().find(|i| i.id == id)).ok_or_else(|| ActionError::bad(crate::tr!("That item is gone.")))?;
            let ItemKind::Scripture { translation, verses, per_slide: stored, also, layout, breaks: saved, .. } = &item.kind else {
                return Err(ActionError::bad(crate::tr!("That item is gone.")));
            };
            let abbr_of = |t: &str| d.bible_credit(t).map(|c| c.abbr.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| t.to_string());
            let shown: Vec<(String, &[VerseText])> = also.iter().filter(|a| !a.verses.is_empty()).map(|a| (abbr_of(&a.translation), a.verses.as_slice())).collect();
            let breaks = breaks.unwrap_or_else(|| saved.clone());
            let per_slide = per_slide.unwrap_or(*stored).min(4);
            let input = crate::slides::scripture_input(verses, &abbr_of(translation), &shown, per_slide, *layout, &breaks, &d.settings.theme);
            let view = breaks_view(&input, &mut super::planning::CharMeasure::for_input(&input.theme, input.canvas));
            Ok(json!({ "view": view, "per_slide": per_slide, "breaks": breaks }))
        }
        _ => Err(ActionError::bad(crate::tr!("Unknown action."))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;
    use crate::model::ServiceItem;
    use crate::scripture::VerseId;

    async fn act(app: &Arc<App>, a: &str, body: Value) -> Result<Value, ActionError> { app.do_action(a, body.to_string().as_bytes()).await }

    /// Searches until the index is ready (it is built in the background).
    async fn search(app: &Arc<App>, body: Value) -> Value {
        for _ in 0..600 {
            let v = act(app, "bible/search", body.clone()).await.unwrap();
            if v["state"] == "ready" { return v }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("the index never got ready")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn search_finds_be_still_and_know_first_with_a_snippet() {
        let app = test_app();
        let first = act(&app, "bible/search", json!({ "query": "be still and know", "translation": "KJV" })).await.unwrap();
        assert_eq!(first["state"], "pending", "the first search builds the index in the background");
        let v = search(&app, json!({ "query": "be still and know", "translation": "KJV" })).await;
        assert_eq!(v["hits"][0]["reference"], "Psalm 46:10");
        assert!(v["hits"][0]["snippet"].as_str().unwrap().contains("Be still"), "{v}");
        assert_eq!(v["translation"], "KJV");
        assert!(v["note"].is_null());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn search_of_a_bible_not_stored_here_searches_the_first_local_one_and_says_so() {
        let app = test_app();
        let v = search(&app, json!({ "query": "jesus wept", "translation": "NIV", "scope": "nt" })).await;
        assert_eq!(v["translation"], "KJV");
        assert_eq!(v["note"], "Search isn't available for NIV, so this searches KJV.");
        assert_eq!(v["hits"][0]["reference"], "John 11:35");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn search_lists_built_in_bibles_and_an_empty_query_says_so() {
        let app = test_app();
        let v = act(&app, "bible/searchable", json!({})).await.unwrap();
        let ids: Vec<&str> = v["bibles"].as_array().unwrap().iter().map(|b| b["id"].as_str().unwrap()).collect();
        assert_eq!(&ids[..3], ["KJV", "WEB", "BSB"]);
        let err = act(&app, "bible/search", json!({ "query": " ", "translation": "KJV" })).await.unwrap_err();
        assert_eq!(err.message, "Type at least one word to search for.");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancel_stops_a_build_under_way() {
        let app = test_app();
        act(&app, "bible/search", json!({ "query": "love", "translation": "WEB" })).await.unwrap();
        act(&app, "bible/search-cancel", json!({})).await.unwrap();
        assert!(!app.search.get().unwrap().is_building("WEB"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn chapter_reads_the_whole_chapter() {
        let app = test_app();
        let v = act(&app, "bible/chapter", json!({ "translation": "KJV", "book": 18, "chapter": 46 })).await.unwrap();
        assert_eq!(v["verses"].as_array().unwrap().len(), 11);
        assert_eq!(v["verses"][9]["reference"], "Psalm 46:10");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn breaks_plan_follows_the_marks_sent_and_save_keeps_them() {
        let app = test_app();
        let (_, verses) = bible::kjv().lookup("Psalm 23:1-4").unwrap();
        let item = ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Scripture { reference: "Psalm 23:1-4".into(), translation: "KJV".into(), verses, per_slide: 0, text_size: 0,
            also: vec![], layout: Default::default(), breaks: Default::default() }, background: None, notes: String::new(), theme: None };
        let id = item.id;
        app.data.lock().unwrap().services[0].items.push(item);
        let v = act(&app, "scripture/breaks", json!({ "id": id })).await.unwrap();
        assert_eq!(v["view"]["verses"].as_array().unwrap().len(), 4);
        assert_eq!(v["view"]["gaps"].as_array().unwrap().len(), 3);
        let marks = VerseBreaks { before: [VerseId(18, 23, 3)].into(), join: Default::default() };
        let v = act(&app, "scripture/breaks", json!({ "id": id, "breaks": marks })).await.unwrap();
        assert_eq!(v["view"]["gaps"][1]["state"], "added");
        let labels: Vec<&str> = v["view"]["slides"].as_array().unwrap().iter().map(|s| s["label"].as_str().unwrap()).collect();
        assert_eq!(labels.last(), Some(&"Psalm 23:3–4"));
        act(&app, "service/update", json!({ "id": id, "breaks": v["view"]["save"], "per_slide": 0 })).await.unwrap();
        let d = app.data.lock().unwrap();
        assert!(matches!(&d.services[0].items[0].kind, ItemKind::Scripture { breaks, .. } if *breaks == marks));
    }
}
