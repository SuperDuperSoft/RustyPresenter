//! The operator's one way into the core: `App::do_action`, `App::command` and `App::import_path`, run on tokio
//! with the answer handed back on the Slint thread. `done` runs on the UI thread, so it needs no `Send` bound
//! and may hold `Rc` and `RefCell` state. `Ops::inline` runs everything at once for tests on the testing
//! backend, which has no event loop.
use crate::{
    app::{App, ImportPurpose, Imported},
    live::{Command, Envelope, Outcome},
    ui::operator::strings as tr,
    youtube,
};
use serde::de::DeserializeOwned;
use std::{future::Future, path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub struct Ops { app: Arc<App>, rt: Option<tokio::runtime::Handle> }

thread_local! {
    /// `Ops::inline`'s runtime, one per test thread, so tasks a call spawns survive between calls.
    static INLINE: tokio::runtime::Runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("test runtime");
}

impl Ops {
    /// Core calls run on `rt`; answers come back through `slint::spawn_local` (needs a running event loop).
    pub fn new(app: Arc<App>, rt: tokio::runtime::Handle) -> Self { Self { app, rt: Some(rt) } }

    /// Runs each call to the end on a local current-thread runtime and calls `done` before returning.
    pub fn inline(app: Arc<App>) -> Self { Self { app, rt: None } }

    pub fn app(&self) -> &Arc<App> { &self.app }

    /// Made by `inline` (tests): everything runs at once on the calling thread.
    pub fn is_inline(&self) -> bool { self.rt.is_none() }

    fn run<R: Send + 'static>(&self, work: impl Future<Output = R> + Send + 'static, done: impl FnOnce(Result<R, String>) + 'static) {
        match &self.rt {
            Some(rt) => {
                let task = rt.spawn(work);
                if let Err(e) = slint::spawn_local(async move { done(task.await.map_err(|_| tr::ACTION_STOPPED.to_string())) }) {
                    eprintln!("Operator action not delivered: {e}");
                }
            }
            None => done(Ok(INLINE.with(|rt| rt.block_on(work)))),
        }
    }

    /// `App::do_action(action, body)`; the answer is parsed as `T`. Errors carry the core's message unchanged.
    pub fn call<T: DeserializeOwned + Send + 'static>(&self, action: &'static str, body: serde_json::Value, done: impl FnOnce(Result<T, String>) + 'static) {
        let app = self.app.clone();
        let work = async move {
            let v = app.do_action(action, body.to_string().as_bytes()).await.map_err(|e| e.message)?;
            serde_json::from_value::<T>(v).map_err(|e| e.to_string())
        };
        self.run(work, |r| done(r.and_then(|r| r)));
    }

    /// `App::do_action(action, <the file's bytes>)`, for actions whose body is a raw file (`bible/import/{ABBR}`).
    /// The file is read on tokio, never on the UI thread.
    pub fn call_file<T: DeserializeOwned + Send + 'static>(&self, action: String, path: PathBuf, done: impl FnOnce(Result<T, String>) + 'static) {
        let app = self.app.clone();
        let work = async move {
            let body = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
            let v = app.do_action(&action, &body).await.map_err(|e| e.message)?;
            serde_json::from_value::<T>(v).map_err(|e| e.to_string())
        };
        self.run(work, |r| done(r.and_then(|r| r)));
    }

    /// `song/import {text, mode}` with the text of a SongSelect file, read on tokio, never on the UI thread.
    pub fn import_song<T: DeserializeOwned + Send + 'static>(&self, path: PathBuf, mode: &'static str, done: impl FnOnce(Result<T, String>) + 'static) {
        let app = self.app.clone();
        let work = async move {
            let bytes = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
            let body = serde_json::json!({ "text": String::from_utf8_lossy(&bytes), "mode": mode });
            let v = app.do_action("song/import", body.to_string().as_bytes()).await.map_err(|e| e.message)?;
            serde_json::from_value::<T>(v).map_err(|e| e.to_string())
        };
        self.run(work, |r| done(r.and_then(|r| r)));
    }

    /// `App::command` with a fresh id (`operator.js:79-89`). Pass `base` only for Next and Prev.
    pub fn cmd(&self, cmd: Command, base: Option<u64>, done: impl FnOnce(Outcome) + 'static) {
        let app = self.app.clone();
        let work = async move { app.command(Envelope { id: Uuid::new_v4().to_string(), base, cmd }) };
        self.run(work, |r| done(r.unwrap_or_else(|e| Outcome { ok: false, version: 0, error: Some(e), duplicate: false })));
    }

    /// Looks up a YouTube video (`youtube::Hub::look_up`): its title, length and picture. `setup` hears how far
    /// getting the helper programs ready has got, from another thread.
    pub fn youtube_look_up(&self, link: youtube::Link, quality: youtube::Quality, setup: impl Fn(youtube::Setup) + Send + Sync + 'static, done: impl FnOnce(Result<youtube::Found, String>) + 'static) {
        let app = self.app.clone();
        let work = async move { app.youtube.look_up(&link, quality, &setup, &CancellationToken::new()).await.map_err(|e| e.message()) };
        self.run(work, |r| done(r.and_then(|r| r)));
    }

    /// Starts downloading a YouTube video as a job card (`youtube::start_download`); `done` gets the job's id.
    pub fn youtube_download(&self, video: youtube::Video, quality: youtube::Quality, service: Option<Uuid>, done: impl FnOnce(Uuid) + 'static) {
        let app = self.app.clone();
        let work = async move { youtube::start_download(&app, video, quality, service) };
        self.run(work, |r| if let Ok(id) = r { done(id) });
    }

    /// Copies a file from this computer into the library (`App::import_path`).
    pub fn import(&self, path: PathBuf, purpose: ImportPurpose, progress: impl Fn(u8) + Send + Sync + 'static, done: impl FnOnce(Result<Imported, String>) + 'static) {
        #[cfg(test)]
        if let Some(r) = testing::IMPORTS.with(|i| i.borrow_mut().pop_front()) { return done(r) }
        let app = self.app.clone();
        let work = async move { app.import_path(&path, purpose, progress).await.map_err(|e| e.message) };
        self.run(work, |r| done(r.and_then(|r| r)));
    }
}

/// The toast for a refused command, `(message, is_error)` (`operator.js:83`): a stale base is silent, running
/// off either end of the service is a plain toast, anything else is an error.
pub fn outcome_toast(o: &Outcome) -> Option<(String, bool)> {
    if o.ok { return None; }
    let e = o.error.as_deref()?;
    if e.starts_with("stale") { return None; }
    Some((e.to_string(), !(e == tr::END_OF_SERVICE || e == tr::START_OF_SERVICE)))
}

#[cfg(test)]
pub(crate) mod testing {
    use crate::app::Imported;
    use std::{cell::RefCell, collections::VecDeque};

    thread_local! {
        /// What the next `Ops::import` calls answer, in order, without touching the file (a PowerPoint job without
        /// PowerPoint). Once empty, imports are real again.
        pub(crate) static IMPORTS: RefCell<VecDeque<Result<Imported, String>>> = RefCell::default();
    }

    /// Queues the answer of the next import.
    pub(crate) fn will_import(r: Result<Imported, String>) { IMPORTS.with(|i| i.borrow_mut().push_back(r)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    fn refused(error: &str) -> Outcome { Outcome { ok: false, version: 3, error: Some(error.into()), duplicate: false } }

    #[test]
    fn outcome_toast_silent_for_stale() {
        assert_eq!(outcome_toast(&refused("stale: the screen changed")), None);
        assert_eq!(outcome_toast(&Outcome { ok: true, version: 4, error: None, duplicate: false }), None);
    }

    #[test]
    fn outcome_toast_plain_for_end_of_service() {
        assert_eq!(outcome_toast(&refused("End of service")), Some(("End of service".into(), false)));
        assert_eq!(outcome_toast(&refused("Start of service")), Some(("Start of service".into(), false)));
    }

    #[test]
    fn outcome_toast_error_otherwise() {
        assert_eq!(outcome_toast(&refused("That item is gone.")), Some(("That item is gone.".into(), true)));
        assert_eq!(outcome_toast(&refused("End of service soon")), Some(("End of service soon".into(), true)));
    }

    #[test]
    fn inline_call_returns_core_error_message() {
        let ops = Ops::inline(crate::app::test_app());
        let got = Rc::new(RefCell::new(None));
        let g = got.clone();
        ops.call::<serde_json::Value>("song/save", serde_json::json!({ "id": Uuid::nil(), "title": " ", "lyrics": "" }), move |r| *g.borrow_mut() = Some(r));
        assert_eq!(got.borrow_mut().take(), Some(Err("Give the song a title.".to_string())));
    }

    #[test]
    fn inline_done_may_capture_rc() {
        #[derive(serde::Deserialize)]
        struct Saved { id: Uuid }
        let app = crate::app::test_app();
        let ops = Ops::inline(app.clone());
        let seen: Rc<RefCell<Vec<Uuid>>> = Rc::default();
        let s = seen.clone();
        ops.call::<Saved>("song/save", serde_json::json!({ "id": Uuid::nil(), "title": "Amazing Grace", "lyrics": "Verse 1\nAmazing grace" }), move |r| s.borrow_mut().push(r.unwrap().id));
        assert_eq!(seen.borrow().len(), 1, "done ran before call returned");
        assert_eq!(app.data.lock().unwrap().songs[0].id, seen.borrow()[0]);
    }

    #[test]
    fn inline_cmd_uses_a_fresh_id_each_time() {
        let ops = Ops::inline(crate::app::test_app());
        let outs: Rc<RefCell<Vec<Outcome>>> = Rc::default();
        for _ in 0..2 { let o = outs.clone(); ops.cmd(Command::ClearAll, None, move |r| o.borrow_mut().push(r)); }
        let outs = outs.borrow();
        assert!(outs.iter().all(|o| o.ok && !o.duplicate), "{outs:?}");
        // Next on an empty service is refused with the plain "End of service" toast.
        let o = Rc::new(RefCell::new(None));
        let o2 = o.clone();
        ops.cmd(Command::Next, Some(outs[1].version), move |r| *o2.borrow_mut() = Some(r));
        let r = o.borrow_mut().take().unwrap();
        assert!(!r.ok);
        assert_eq!(r.error.as_deref(), Some(tr::END_OF_SERVICE));
        assert_eq!(outcome_toast(&r), Some(("End of service".into(), false)));
    }
}
