//! Adding files to the service (`uploadFiles`, `watchDeckJob` and `imagesToDeck` in
//! `web/operator.js:536-598`): which import a dropped or picked file takes, the window glue that runs the
//! imports one after another as client jobs, and the pure steps it is built from. Files arrive from the native
//! file picker (`pick`) or a desktop drop (`wire`, through the window's winit event filter).
use super::{
    add_item::add_item,
    file_input::{DropBatch, DropEvent, DropPlan, Filter, drop_plan},
    overlays::JobHandle, strings as tr, view, window::Operator,
};
use crate::{
    app::{ImportPurpose, Imported},
    import::deck::DECK_EXT,
    media,
    model::{AssetId, Deck, MediaStatus},
    platform::winit_events::{self, Dispatcher, WindowEvent},
    ui::dialogs::{self, ask::Choice},
};
use slint::Timer;
use serde::Deserialize;
use serde_json::json;
use std::{
    cell::RefCell,
    cmp::Ordering,
    collections::VecDeque,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route { Song, Deck, Media }

impl Route {
    pub fn purpose(self) -> ImportPurpose {
        match self {
            Route::Song => ImportPurpose::Lyrics,
            Route::Deck => ImportPurpose::PowerPoint,
            Route::Media => ImportPurpose::Media,
        }
    }
}

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn route(path: &Path) -> Route {
    let e = ext(path);
    if e == "txt" { Route::Song } else if DECK_EXT.contains(&e.as_str()) { Route::Deck } else { Route::Media }
}

/// True when there is at least one file and every file is a picture, by extension.
pub fn all_images(paths: &[PathBuf]) -> bool {
    !paths.is_empty() && paths.iter().all(|p| media::IMAGE.contains(&ext(p).as_str()))
}

/// Sorts by file name the way `localeCompare(.., {numeric: true})` does for ordinary names, so `2.png` comes
/// before `10.png`. Names that compare equal (`7.png`, `007.png`) keep their order.
pub fn sort_by_name(paths: &mut [PathBuf]) {
    let name = |p: &PathBuf| p.file_name().unwrap_or(p.as_os_str()).to_string_lossy().into_owned();
    paths.sort_by(|a, b| natural_cmp(&name(a), &name(b)));
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        let (x, y) = match (a.peek(), b.peek()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) => (*x, *y),
        };
        let order = if x.is_ascii_digit() && y.is_ascii_digit() {
            // By length first, so no run can overflow an integer.
            let (m, n) = (digits(&mut a), digits(&mut b));
            m.len().cmp(&n.len()).then_with(|| m.cmp(&n))
        } else {
            a.next();
            b.next();
            rank(x).cmp(&rank(y))
        };
        if order != Ordering::Equal { return order; }
    }
}

/// A run of digits without its leading zeros.
fn digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut run = String::new();
    while let Some(c) = chars.next_if(char::is_ascii_digit) { run.push(c); }
    run.trim_start_matches('0').to_string()
}

/// Punctuation, then digits, then letters, ignoring case.
fn rank(c: char) -> (u8, char) {
    let class = if c.is_ascii_digit() { 1 } else if c.is_alphanumeric() { 2 } else { 0 };
    (class, c.to_lowercase().next().unwrap_or(c))
}

/// The name the "Name these slides" prompt starts with: the file name without its last extension.
pub fn suggested_title(path: &Path) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
    match name.rsplit_once('.') {
        Some((stem, e)) if !e.is_empty() => stem.to_string(),
        _ => name.into_owned(),
    }
}

/// The deck title from the prompt's answer. Cancel and a blank answer still make a deck.
pub fn deck_title(answer: Option<&str>) -> String {
    match answer {
        Some(a) if !a.trim().is_empty() => a.to_string(),
        _ => tr::DEFAULT_DECK_TITLE.to_string(),
    }
}

/// What the window keeps for adding files: the deck imports waiting for their deck, the desktop drop in
/// progress, the folder the picker last opened, and the window's one winit event filter.
#[derive(Default)]
pub struct FilesState {
    watches: RefCell<Vec<DeckWatch>>,
    drop: RefCell<DropBatch>,
    #[cfg_attr(test, allow(dead_code))]
    last_folder: RefCell<Option<PathBuf>>,
    events: Dispatcher<WindowEvent>,
}

/// A deck import whose deck is added to the service when its job ends (`watchDeckJob`). Only the deck the job
/// itself made (`result`) is added, never "every new deck", so imports that overlap each add their own.
#[derive(Clone, Debug)]
pub struct DeckWatch { pub job: Uuid }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeckWatchStep {
    Wait,
    /// The job failed: its error card says so, and nothing is added.
    Failed,
    /// The job ended: its deck (id, title) goes into the service. Empty when it made none (cancelled).
    Ready(Vec<(Uuid, String)>),
}

impl DeckWatch {
    /// One look at a new snapshot (`Update::Data`). `ended` is the snapshot's `ended`: a job that ends without a
    /// note leaves `jobs` at once.
    pub fn step(&self, jobs: &[view::JobRow], ended: &[(Uuid, Option<Uuid>)], decks: &[Deck]) -> DeckWatchStep {
        // A job that ended well but has a note stays listed (`done`) until dismissed.
        let made = match jobs.iter().find(|j| j.id == self.job) {
            Some(j) if j.error.is_some() => return DeckWatchStep::Failed,
            Some(j) if !j.done => return DeckWatchStep::Wait,
            Some(j) => j.result,
            // Not listed and not ended: a read from before the job started.
            None => match ended.iter().find(|(id, _)| *id == self.job) { Some((_, made)) => *made, None => return DeckWatchStep::Wait },
        };
        match made {
            None => DeckWatchStep::Ready(Vec::new()),
            Some(id) => match decks.iter().find(|d| d.id == id) {
                Some(d) => DeckWatchStep::Ready(vec![(d.id, d.title.clone())]),
                // A read that saw the job end but not the deck it had just added: the next read has both.
                None => DeckWatchStep::Wait,
            },
        }
    }
}

/// `deck/from-images`'s answer.
#[derive(Deserialize)]
struct Added { id: Uuid }

/// The name a file's toasts and job card use.
fn file_name(path: &Path) -> String { path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned() }

/// `uploadFiles(files, {add: true})`: each file in turn, as a client job "Uploading `<name>`", then into the
/// service. A song or media file is added quietly with its own toast; a PowerPoint is added when its job ends. A
/// failure toasts "`<file>`: `<error>`" and the next file goes on. The open dialog is redrawn at the end.
pub fn add_files(op: &Rc<Operator>, paths: Vec<PathBuf>) { next_file(op, paths.into()); }

fn next_file(op: &Rc<Operator>, mut rest: VecDeque<PathBuf>) {
    let Some(path) = rest.pop_front() else { return op.dialog_refresh() };
    let name = file_name(&path);
    let job = op.client_job(tr::uploading(&name));
    let progress = job.progress();
    let me = Rc::downgrade(op);
    op.ops.import(path.clone(), route(&path).purpose(), progress, move |r| {
        drop(job);
        let Some(op) = me.upgrade() else { return };
        let shown = name.clone();
        // Each file ends with its toast (none for a PowerPoint, whose deck comes later), then the next file starts.
        let then = move |op: &Rc<Operator>, r: Result<Option<String>, String>| {
            match r {
                Ok(text) => if let Some(text) = text { op.toast(text, false) },
                Err(e) => op.toast(tr::file_failed(&name, &e), true),
            }
            next_file(op, rest);
        };
        match r {
            Err(e) => then(&op, Err(e)),
            Ok(Imported::Media(m)) => {
                let converting = matches!(m.status, MediaStatus::Converting { .. });
                let item = json!({ "kind": "media", "media_id": m.id, "autoplay": true, "end": "stop", "looped": false });
                let me = Rc::downgrade(&op);
                add_item(&op, item, true, move |r| {
                    let Some(op) = me.upgrade() else { return };
                    then(&op, r.map(|_| Some(if converting { tr::media_added_converting(&shown) } else { tr::media_added(&shown) })));
                });
            }
            Ok(Imported::Song { id: Some(id), title, .. }) => add_song(&op, id, tr::song_imported(&title), then),
            // Same CCLI # as a song in the library: nothing was saved. The answer imports the file again in that
            // mode; Cancel or Esc adds nothing and the next file goes on.
            Ok(Imported::Song { id: None, title, .. }) => {
                let me = Rc::downgrade(&op);
                dialogs::ask::choose(&op, &tr::song_already(&title), tr::UPDATE_EXISTING, tr::KEEP_BOTH, move |choice| {
                    let Some(op) = me.upgrade() else { return };
                    let mode = match choice { Choice::Ok => "update", Choice::Other => "keep_both", Choice::Cancel => return then(&op, Ok(None)) };
                    let me = Rc::downgrade(&op);
                    op.ops.import_song::<SongImported>(path, mode, move |r| {
                        let Some(op) = me.upgrade() else { return };
                        match r {
                            Ok(s) => {
                                let text = if mode == "update" { tr::song_updated(&s.title) } else { tr::song_imported(&s.title) };
                                add_song(&op, s.id, text, then);
                            }
                            Err(e) => then(&op, Err(e)),
                        }
                    });
                });
            }
            Ok(Imported::Job(job)) => {
                op.files.watches.borrow_mut().push(DeckWatch { job });
                op.watch_decks();
                then(&op, Ok(None));
            }
            // A media, lyrics or PowerPoint import never answers with a bare asset.
            Ok(Imported::Asset(_)) => then(&op, Err(tr::ACTION_STOPPED.into())),
        }
    });
}

/// `song/import`'s answer when it saved the song.
#[derive(Deserialize)]
struct SongImported { id: Uuid, title: String }

/// Adds a saved song to the service, then `then` toasts `text`.
fn add_song(op: &Rc<Operator>, id: Uuid, text: String, then: impl FnOnce(&Rc<Operator>, Result<Option<String>, String>) + 'static) {
    let me = Rc::downgrade(op);
    add_item(op, json!({ "kind": "song", "song_id": id }), true, move |r| {
        if let Some(op) = me.upgrade() { then(&op, r.map(|_| Some(text))); }
    });
}

impl Operator {
    /// Looks at each PowerPoint import waiting for its deck; called by `apply_snapshot`.
    pub(super) fn watch_decks(&self) {
        let Some(op) = self.me.upgrade() else { return };
        let Some(s) = self.state.borrow().snap.clone() else { return };
        let mut ready = Vec::new();
        self.files.watches.borrow_mut().retain(|w| match w.step(&s.jobs, &s.ended, &s.data.decks) {
            DeckWatchStep::Wait => true,
            DeckWatchStep::Failed => false,
            DeckWatchStep::Ready(decks) => { ready.push(decks); false }
        });
        for decks in ready { add_decks(&op, decks.into(), None); }
    }
}

/// Adds the decks a PowerPoint made, one after another, then toasts "“T” is ready" with the first one's title.
fn add_decks(op: &Rc<Operator>, mut rest: VecDeque<(Uuid, String)>, first: Option<String>) {
    let Some((id, title)) = rest.pop_front() else {
        if let Some(t) = first { op.toast(tr::deck_ready(&t), false); }
        return;
    };
    let first = first.or(Some(title));
    let me = Rc::downgrade(op);
    add_item(op, json!({ "kind": "deck", "deck_id": id }), true, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => add_decks(&op, rest, first),
            Err(e) => op.toast(e, true),
        }
    });
}

/// `imagesToDeck` (`operator.js:580-598`): upload the pictures in file-name order as one client job, ask for a
/// name, make the deck, add it to the service (with its toast) and close the dialog. The job card stays until the
/// end; any error is an error toast with the core's text.
pub fn images_to_deck(op: &Rc<Operator>, mut paths: Vec<PathBuf>) {
    if paths.is_empty() { return }
    sort_by_name(&mut paths);
    let job = op.client_job(tr::uploading_pictures(paths.len()));
    let first = suggested_title(&paths[0]);
    let total = paths.len();
    next_picture(op, Pictures { rest: paths.into(), assets: Vec::new(), total, job, first });
}

/// `imagesToDeck`'s progress between uploads.
struct Pictures { rest: VecDeque<PathBuf>, assets: Vec<AssetId>, total: usize, job: JobHandle, first: String }

fn next_picture(op: &Rc<Operator>, mut p: Pictures) {
    let Some(path) = p.rest.pop_front() else { return name_deck(op, p) };
    let me = Rc::downgrade(op);
    op.ops.import(path, ImportPurpose::Asset, |_| {}, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(Imported::Asset(a)) => {
                p.assets.push(a);
                p.job.set_percent((p.assets.len() as f64 * 100.0 / p.total as f64).round() as u8);
                next_picture(&op, p);
            }
            Ok(_) => op.toast(tr::ACTION_STOPPED, true),
            Err(e) => op.toast(e, true),
        }
    });
}

fn name_deck(op: &Rc<Operator>, p: Pictures) {
    let me = Rc::downgrade(op);
    let first = p.first.clone();
    dialogs::prompt(op, tr::NAME_THESE_SLIDES, &first, move |answer| {
        let Some(op) = me.upgrade() else { return };
        let Pictures { assets, job, .. } = p;
        let me = Rc::downgrade(&op);
        op.ops.call::<Added>("deck/from-images", json!({ "title": deck_title(answer.as_deref()), "assets": assets }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(Added { id }) => {
                    let me = Rc::downgrade(&op);
                    add_item(&op, json!({ "kind": "deck", "deck_id": id }), false, move |r| {
                        let Some(op) = me.upgrade() else { return };
                        match r {
                            Ok(_) => op.close_dialog(),
                            Err(e) => op.toast(e, true),
                        }
                        drop(job);
                    });
                }
                Err(e) => { op.toast(e, true); drop(job); }
            }
        });
    });
}

/// Installs the operator window's one event filter and its desktop drop handler (`operator.js:601-612`).
pub(super) fn wire(op: &Rc<Operator>) {
    winit_events::attach(op.window(), op.files.events.clone());
    let me = Rc::downgrade(op);
    op.files.events.add(move |e| {
        if let (Some(op), Some(event)) = (me.upgrade(), drop_event(e)) { on_drop_event(&op, event) }
    });
}

/// The drop events among the window's events.
pub fn drop_event(e: &WindowEvent) -> Option<DropEvent> {
    match e {
        WindowEvent::HoveredFile(_) => Some(DropEvent::Hovered),
        WindowEvent::HoveredFileCancelled => Some(DropEvent::Cancelled),
        WindowEvent::DroppedFile(p) => Some(DropEvent::Dropped(p.clone())),
        _ => None,
    }
}

/// One drop event: shows or hides the "Drop to add" hint, and on the first file of a batch starts the zero timer
/// that handles every file winit sent in that turn together.
pub fn on_drop_event(op: &Rc<Operator>, event: DropEvent) {
    let step = op.files.drop.borrow_mut().on_event(event);
    if let Some(on) = step.hint { op.ui.set_drop_hint(on); }
    if step.start_timer {
        let me = Rc::downgrade(op);
        Timer::single_shot(Duration::ZERO, move || {
            let Some(op) = me.upgrade() else { return };
            let paths = op.files.drop.borrow_mut().take();
            handle_drop(&op, paths);
        });
    }
}

/// A finished drop: several pictures ask once whether they make one set of slides; anything else is added.
pub fn handle_drop(op: &Rc<Operator>, paths: Vec<PathBuf>) {
    match drop_plan(paths) {
        None => {}
        Some(DropPlan::AddFiles(paths)) => add_files(op, paths),
        Some(DropPlan::AskOneDeck(paths)) => {
            let me = Rc::downgrade(op);
            let question = tr::pictures_to_deck(paths.len());
            dialogs::confirm(op, &question, tr::ONE_SLIDE_SET, tr::SEPARATE_PICTURES, false, move |one| {
                let Some(op) = me.upgrade() else { return };
                if one { images_to_deck(&op, paths) } else { add_files(&op, paths) }
            });
        }
    }
}

/// The native file picker (`pickFiles`, `operator.js:516-521`) over the operator window, starting in the folder
/// last picked from this session. `done` runs only when files were picked. Tests never open the dialog: a pick
/// takes the next paths queued with `testing::will_pick`.
pub fn pick(op: &Rc<Operator>, filter: Filter, multi: bool, done: impl FnOnce(Vec<PathBuf>) + 'static) {
    #[cfg(test)]
    {
        let _ = (op, filter);
        let picked = testing::PICKS.with(|p| p.borrow_mut().pop_front()).unwrap_or_default();
        let picked: Vec<PathBuf> = if multi { picked } else { picked.into_iter().take(1).collect() };
        if !picked.is_empty() { done(picked) }
    }
    #[cfg(not(test))]
    native_pick(op, filter, multi, done);
}

#[cfg(not(test))]
fn native_pick(op: &Rc<Operator>, filter: Filter, multi: bool, done: impl FnOnce(Vec<PathBuf>) + 'static) {
    let mut dialog = rfd::AsyncFileDialog::new().add_filter(filter.name(), &filter.extensions()).set_parent(&op.window().window_handle());
    if let Some(dir) = op.files.last_folder.borrow().as_ref() { dialog = dialog.set_directory(dir); }
    let me = Rc::downgrade(op);
    let picking = async move {
        let picked: Vec<PathBuf> = if multi { dialog.pick_files().await.unwrap_or_default() } else { dialog.pick_file().await.into_iter().collect() }
            .into_iter().map(|f| f.path().to_path_buf()).collect();
        let Some(op) = me.upgrade() else { return };
        let Some(dir) = picked.first().and_then(|p| p.parent()) else { return };
        *op.files.last_folder.borrow_mut() = Some(dir.to_path_buf());
        done(picked);
    };
    if let Err(e) = slint::spawn_local(picking) { op.toast(e.to_string(), true) }
}

impl Operator {
    /// The window's event filter, for anything else that needs raw window events (Slint allows one filter).
    pub fn window_events(&self) -> &Dispatcher<WindowEvent> { &self.files.events }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::{cell::RefCell, collections::VecDeque, path::PathBuf};

    thread_local! {
        /// What the next picks return, in order; an empty or missing entry is a cancelled pick.
        pub(crate) static PICKS: RefCell<VecDeque<Vec<PathBuf>>> = RefCell::default();
    }

    /// Queues the files the next `pick` "chooses".
    pub(crate) fn will_pick(paths: Vec<PathBuf>) { PICKS.with(|p| p.borrow_mut().push_back(paths)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> { names.iter().map(PathBuf::from).collect() }

    fn sorted(names: &[&str]) -> Vec<PathBuf> {
        let mut v = paths(names);
        sort_by_name(&mut v);
        v
    }

    #[test]
    fn route_by_extension() {
        assert_eq!(route(Path::new("C:/songs/Amazing Grace.txt")), Route::Song);
        for f in ["a.ppt", "a.pptx", "a.pps", "a.ppsx", "a.pot", "a.potx", "a.pptm", "a.odp", "a.key", "a.pdf", "Talk.key"] {
            assert_eq!(route(Path::new(f)), Route::Deck, "{f}");
        }
        for f in ["a.mp4", "a.png", "a.mp3", "no-extension"] { assert_eq!(route(Path::new(f)), Route::Media, "{f}"); }
    }

    #[test]
    fn deck_filter_matches_router() {
        let filter = Filter::PowerPoint.extensions();
        for e in DECK_EXT { assert_eq!(route(Path::new(&format!("a.{e}"))), Route::Deck, "{e}"); }
        let (mut a, mut b) = (filter.clone(), DECK_EXT.to_vec());
        a.sort();
        b.sort();
        assert_eq!(a, b, "the picker offers exactly the files the router sends to a deck");
    }

    #[test]
    fn route_ignores_extension_case() {
        assert_eq!(route(Path::new("SONG.TXT")), Route::Song);
        assert_eq!(route(Path::new("Deck.PPTX")), Route::Deck);
    }

    #[test]
    fn each_route_imports_with_its_purpose() {
        assert_eq!(Route::Song.purpose(), ImportPurpose::Lyrics);
        assert_eq!(Route::Deck.purpose(), ImportPurpose::PowerPoint);
        assert_eq!(Route::Media.purpose(), ImportPurpose::Media);
    }

    #[test]
    fn all_images_needs_every_file() {
        assert!(all_images(&paths(&["a.png", "b.JPG", "c.webp"])));
        assert!(!all_images(&paths(&["a.png", "b.mp4"])));
        assert!(!all_images(&paths(&["no-extension"])));
    }

    #[test]
    fn all_images_is_false_with_no_files() { assert!(!all_images(&[])); }

    #[test]
    fn sort_puts_2_before_10() {
        assert_eq!(sorted(&["10.png", "2.png", "1.png"]), paths(&["1.png", "2.png", "10.png"]));
        assert_eq!(sorted(&["Slide10.png", "Slide9.png"]), paths(&["Slide9.png", "Slide10.png"]));
    }

    #[test]
    fn sort_uses_the_file_name_not_the_folder() {
        assert_eq!(sorted(&["a/2.png", "z/1.png"]), paths(&["z/1.png", "a/2.png"]));
    }

    #[test]
    fn sort_ignores_case() {
        assert_eq!(sorted(&["Banana.png", "apple.png", "cherry.png"]), paths(&["apple.png", "Banana.png", "cherry.png"]));
        assert_eq!(sorted(&["IMG_10.png", "img_2.png"]), paths(&["img_2.png", "IMG_10.png"]));
    }

    #[test]
    fn sort_puts_punctuation_before_digits_before_letters() {
        // `localeCompare` order: a shorter name wins only when it is a true prefix.
        assert_eq!(sorted(&["a1.png", "a_1.png", "ab.png"]), paths(&["a_1.png", "a1.png", "ab.png"]));
    }

    #[test]
    fn sort_handles_numbers_too_long_for_an_integer() {
        let (big, bigger) = ("99999999999999999999999998.png", "99999999999999999999999999.png");
        assert_eq!(sorted(&[bigger, big, "3.png"]), paths(&["3.png", big, bigger]));
    }

    #[test]
    fn sort_keeps_equal_numbers_in_their_given_order() {
        assert_eq!(sorted(&["007.png", "7.png"]), paths(&["007.png", "7.png"]));
        assert_eq!(sorted(&["7.png", "007.png"]), paths(&["7.png", "007.png"]));
    }

    #[test]
    fn suggested_title_drops_only_the_last_extension() {
        assert_eq!(suggested_title(Path::new("C:/pics/Easter Sunday.png")), "Easter Sunday");
        assert_eq!(suggested_title(Path::new("v1.2.final.jpg")), "v1.2.final");
        assert_eq!(suggested_title(Path::new("plain")), "plain");
    }

    #[test]
    fn deck_title_falls_back_when_cancelled_or_blank() {
        assert_eq!(deck_title(None), "Slides");
        assert_eq!(deck_title(Some("")), "Slides");
        assert_eq!(deck_title(Some("   ")), "Slides");
    }

    #[test]
    fn deck_title_keeps_what_was_typed() { assert_eq!(deck_title(Some("Easter Sunday")), "Easter Sunday"); }
}

#[cfg(test)]
mod window_tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        model::ItemKind,
        ui::{dialogs::{AddTab, DialogKind, ask}, operator::window::testing::{find, open, refresh}},
    };
    use i_slint_backend_testing::mock_elapsed_time;
    use slint::Model;
    use std::sync::Arc;

    /// A folder of its own for one test's files.
    fn folder() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rp-files-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A `size`-pixel square PNG; different sizes give different files (assets are named by their hash).
    fn png(dir: &Path, name: &str, size: u32) -> PathBuf {
        let p = dir.join(name);
        image::RgbaImage::from_pixel(size, size, image::Rgba([10, 20, 30, 255])).save(&p).unwrap();
        p
    }

    fn items(app: &App) -> Vec<ItemKind> { app.data.lock().unwrap().services[0].items.iter().map(|i| i.kind.clone()).collect() }

    fn toasts(op: &Operator) -> Vec<(String, bool)> { op.state.borrow().toasts.clone() }

    /// The job cards (these tests start no server jobs, so these are the uploads).
    fn cards(op: &Operator) -> Vec<(String, i32)> { op.ui.get_jobs().iter().map(|j| (j.label.to_string(), j.percent)).collect() }

    fn opened(app: &Arc<App>) -> Rc<Operator> {
        let op = open(app);
        refresh(&op);
        op
    }

    #[test]
    fn add_files_media_adds_to_service_quietly() {
        let app = test_app();
        let op = opened(&app);
        let pic = png(&folder(), "pic.png", 4);
        add_files(&op, vec![pic]);
        let items = items(&app);
        assert_eq!(items.len(), 1);
        let ItemKind::Media { media_id, autoplay, looped, .. } = &items[0] else { panic!("{items:?}") };
        assert!(*autoplay && !*looped);
        assert!(app.data.lock().unwrap().media.iter().any(|m| m.id == *media_id));
        assert_eq!(toasts(&op), [("pic.png added".to_string(), false)], "no \"Added to the service\"");
        refresh(&op);
        let added = app.data.lock().unwrap().services[0].items[0].id;
        assert_eq!(op.state.borrow().selected, Some(added), "the new item is selected");
        assert!(cards(&op).is_empty(), "the upload card is gone");
    }

    #[test]
    fn add_txt_imports_song_and_adds_it() {
        let app = test_app();
        let op = opened(&app);
        let txt = folder().join("song.txt");
        std::fs::write(&txt, "A Test Song\r\n\r\nVerse 1\r\nLine one\r\nLine two\r\n\r\nTest Writer\r\nCCLI Song # 12345\r\n© 2026 Test Music\r\nCCLI License # 98765").unwrap();
        add_files(&op, vec![txt.clone()]);
        let ItemKind::Song { song_id } = items(&app)[0].clone() else { panic!() };
        assert!(app.data.lock().unwrap().songs.iter().any(|s| s.id == song_id && s.title == "A Test Song"));
        assert_eq!(toasts(&op), [("Imported “A Test Song”".to_string(), false)]);
    }

    /// A SongSelect file whose CCLI number is `ccli`, with `line` as its only lyric line.
    fn song_txt(dir: &Path, name: &str, ccli: u32, line: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("A Test Song\r\n\r\nVerse 1\r\n{line}\r\n\r\nTest Writer\r\nCCLI Song # {ccli}\r\n© 2026 Test Music\r\nCCLI License # 98765")).unwrap();
        p
    }

    fn song_count(app: &App) -> usize { app.data.lock().unwrap().songs.len() }

    #[test]
    fn duplicate_song_file_asks_update_or_keep_both() {
        use crate::ui::dialogs::services::testing::{asked, click};
        let app = test_app();
        let op = opened(&app);
        let dir = folder();
        add_files(&op, vec![song_txt(&dir, "v1.txt", 4242, "Old line")]);
        let old_id = app.data.lock().unwrap().songs[0].id;

        // Update existing: still one song, same id, new words; added again to the service.
        add_files(&op, vec![song_txt(&dir, "v2.txt", 4242, "New line")]);
        assert_eq!(asked(&op).map(|a| a.0).as_deref(), Some("“A Test Song” is already in the library."));
        assert_eq!(song_count(&app), 1, "nothing is saved while asking");
        assert_eq!(items(&app).len(), 1, "nothing is added while asking");
        click(&op, "AskOverlay::ok", 0);
        assert!(asked(&op).is_none());
        assert_eq!(song_count(&app), 1);
        let song = app.data.lock().unwrap().songs[0].clone();
        assert_eq!(song.id, old_id);
        assert!(serde_json::to_string(&song).unwrap().contains("New line"), "{song:?}");
        assert!(matches!(&items(&app)[..], [ItemKind::Song { song_id: a }, ItemKind::Song { song_id: b }] if *a == old_id && *b == old_id));
        assert_eq!(toasts(&op).last().cloned(), Some(("Updated “A Test Song”".to_string(), false)));

        // Keep both: a second song, added to the service.
        add_files(&op, vec![song_txt(&dir, "v3.txt", 4242, "Third line")]);
        click(&op, "AskOverlay::other", 0);
        assert_eq!(song_count(&app), 2);
        let ItemKind::Song { song_id } = items(&app)[2].clone() else { panic!() };
        assert_ne!(song_id, old_id);
        assert_eq!(toasts(&op).last().cloned(), Some(("Imported “A Test Song”".to_string(), false)));
        assert!(!toasts(&op).iter().any(|t| t.0.contains("added a second copy")), "the old toast is not used");
    }

    #[test]
    fn closing_duplicate_confirm_adds_nothing() {
        use crate::ui::dialogs::services::testing::{asked, key};
        use slint::platform::Key;
        let app = test_app();
        let op = opened(&app);
        let dir = folder();
        add_files(&op, vec![song_txt(&dir, "v1.txt", 777, "Old line")]);
        // The next file waits for the answer, then goes on.
        add_files(&op, vec![song_txt(&dir, "v2.txt", 777, "New line"), png(&dir, "after.png", 4)]);
        assert!(asked(&op).is_some());
        key(&op, Key::Escape);
        assert!(asked(&op).is_none());
        assert_eq!(song_count(&app), 1);
        assert!(serde_json::to_string(&app.data.lock().unwrap().songs[0]).unwrap().contains("Old line"));
        assert!(matches!(&items(&app)[..], [ItemKind::Song { .. }, ItemKind::Media { .. }]), "{:?}", items(&app));
        assert_eq!(toasts(&op).last().cloned(), Some(("after.png added".to_string(), false)));
    }

    #[test]
    fn a_failed_file_toasts_its_name_and_the_next_goes_on() {
        let app = test_app();
        let op = opened(&app);
        let dir = folder();
        let missing = dir.join("gone.mp4");
        add_files(&op, vec![missing, png(&dir, "ok.png", 4)]);
        let t = toasts(&op);
        assert_eq!(t.len(), 2);
        assert!(t[0].1 && t[0].0.starts_with("gone.mp4: "), "{t:?}");
        assert_eq!(t[1], ("ok.png added".to_string(), false));
        assert_eq!(items(&app).len(), 1);
    }

    #[test]
    fn images_to_deck_cancel_still_makes_slides() {
        let app = test_app();
        let op = opened(&app);
        op.open_dialog(DialogKind::Add(AddTab::Deck));
        let dir = folder();
        images_to_deck(&op, vec![png(&dir, "Easter.png", 4), png(&dir, "b.png", 5)]);
        assert_eq!(op.state.borrow().asks, 1, "the name prompt is up");
        assert_eq!(cards(&op), [("Uploading 2 pictures".to_string(), 100)], "the card stays while asking");
        ask::answer(&op, false, String::new());
        let ItemKind::Deck { deck_id } = items(&app)[0].clone() else { panic!() };
        assert_eq!(app.data.lock().unwrap().decks.iter().find(|d| d.id == deck_id).unwrap().title, "Slides");
        assert_eq!(toasts(&op), [("Added to the service".to_string(), false)]);
        assert_eq!(op.dialog(), None, "the dialog closed");
        assert!(cards(&op).is_empty());
    }

    #[test]
    fn images_to_deck_prompt_starts_with_the_first_name_and_keeps_the_answer() {
        let app = test_app();
        let op = opened(&app);
        let dir = folder();
        images_to_deck(&op, vec![png(&dir, "b.png", 5), png(&dir, "a.png", 4)]);
        assert_eq!(op.ui.get_ask().row_data(0).unwrap().value, "a");
        ask::answer(&op, true, "Easter".into());
        assert_eq!(app.data.lock().unwrap().decks[0].title, "Easter");
    }

    #[test]
    fn images_to_deck_sorts_numerically() {
        let app = test_app();
        let op = opened(&app);
        let dir = folder();
        images_to_deck(&op, vec![png(&dir, "10.png", 10), png(&dir, "2.png", 2)]);
        ask::answer(&op, true, "Pictures".into());
        let deck = app.data.lock().unwrap().decks[0].clone();
        let sizes: Vec<u32> = deck.slides.iter().map(|a| image::image_dimensions(app.store.asset_path(a)).unwrap().0).collect();
        assert_eq!(sizes, [2, 10]);
    }

    #[test]
    fn deck_watch_waits_for_the_job_then_adds_its_result() {
        let job = Uuid::new_v4();
        let other = Deck { id: Uuid::new_v4(), title: "Other".into(), slides: vec![] };
        let new = Deck { id: Uuid::new_v4(), title: "Talk".into(), slides: vec![] };
        let decks = [other.clone(), new.clone()];
        let running = view::JobRow { id: job, label: "Importing".into(), percent: 10, error: None, note: None, cancellable: true, done: false, result: None };
        let w = DeckWatch { job };
        assert_eq!(w.step(&[], &[], &decks), DeckWatchStep::Wait, "a read from before the job started");
        assert_eq!(w.step(std::slice::from_ref(&running), &[], &decks), DeckWatchStep::Wait);
        let talk = DeckWatchStep::Ready(vec![(new.id, "Talk".into())]);
        assert_eq!(w.step(&[], &[(job, Some(new.id))], &decks), talk, "only its own deck, never the other new one");
        assert_eq!(w.step(&[], &[(job, Some(new.id))], std::slice::from_ref(&other)), DeckWatchStep::Wait, "the deck is in the next read");
        assert_eq!(w.step(&[], &[(job, None)], &decks), DeckWatchStep::Ready(vec![]), "cancelled: nothing is added");
        let failed = view::JobRow { error: Some("PowerPoint couldn't open this presentation.".into()), ..running.clone() };
        assert_eq!(w.step(&[failed], &[(job, None)], &decks), DeckWatchStep::Failed);
        // A job that ended well with a note stays listed until dismissed; it has still ended.
        let noted = view::JobRow { note: Some("Imported with LibreOffice".into()), cancellable: false, done: true, percent: 100, result: Some(new.id), ..running };
        assert_eq!(w.step(&[noted], &[], &decks), talk);
    }

    /// A deck import's job ending as the pipeline ends it: the deck, then its result, then the end.
    fn finish(app: &App, job: Uuid, title: &str) -> Uuid {
        let id = Uuid::new_v4();
        app.job_result(job, id);
        app.edit(|d| { d.decks.push(Deck { id, title: title.into(), slides: vec![] }); Ok(()) }).unwrap();
        app.job_end(job, None);
        id
    }

    #[test]
    fn watch_deck_job_adds_only_its_result() {
        for flip in [false, true] {
            let app = test_app();
            let op = opened(&app);
            let jobs = [app.job_start("Importing “Talk”"), app.job_start("Importing “Notices”")];
            for job in jobs { op.files.watches.borrow_mut().push(DeckWatch { job }) }
            refresh(&op);
            // A deck from somewhere else appears meanwhile; no import made it.
            app.edit(|d| { d.decks.push(Deck { id: Uuid::new_v4(), title: "Pictures".into(), slides: vec![] }); Ok(()) }).unwrap();
            refresh(&op);
            let order = if flip { [(jobs[1], "Notices"), (jobs[0], "Talk")] } else { [(jobs[0], "Talk"), (jobs[1], "Notices")] };
            // Both end before the window reads again, as two quick imports may.
            let made = order.map(|(job, title)| finish(&app, job, title));
            refresh(&op);
            refresh(&op);
            let added: Vec<Uuid> = items(&app).iter().filter_map(|k| match k { ItemKind::Deck { deck_id } => Some(*deck_id), _ => None }).collect();
            assert_eq!(added.len(), 2, "flip {flip}: {added:?}");
            assert!(made.iter().all(|d| added.contains(d)), "each import's own deck, once");
        }
    }

    #[test]
    fn finished_powerpoint_job_adds_its_deck_and_says_ready() {
        let app = test_app();
        let op = opened(&app);
        let job = app.job_start("Importing “Talk”");
        op.files.watches.borrow_mut().push(DeckWatch { job });
        refresh(&op);
        assert!(items(&app).is_empty(), "still converting");
        finish(&app, job, "Talk");
        refresh(&op);
        assert!(matches!(items(&app)[..], [ItemKind::Deck { .. }]));
        assert_eq!(toasts(&op), [("“Talk” is ready".to_string(), false)]);
        refresh(&op);
        assert_eq!(items(&app).len(), 1, "added once");
    }

    #[test]
    fn import_powerpoint_adds_only_the_decks_its_jobs_made() {
        let app = test_app();
        let old = Deck { id: Uuid::new_v4(), title: "Last week".into(), slides: vec![] };
        app.edit(|d| { d.decks.push(old.clone()); Ok(()) }).unwrap();
        let op = opened(&app);
        let jobs = [app.job_start("Importing “Talk”"), app.job_start("Importing “Notices”")];
        for job in jobs { crate::ui::ops::testing::will_import(Ok(Imported::Job(job))) }
        testing::will_pick(vec![PathBuf::from("Talk.pptx"), PathBuf::from("Notices.pptx")]);
        crate::ui::dialogs::add::open_add(&op, AddTab::Deck);
        slint::ComponentHandle::global::<crate::ui::AddState>(&op.ui).invoke_import_powerpoint();
        refresh(&op);
        assert!(items(&app).is_empty(), "both decks are still converting");
        let decks = [("Talk", jobs[0]), ("Notices", jobs[1])].map(|(title, job)| {
            let id = finish(&app, job, title);
            refresh(&op);
            id
        });
        let added: Vec<Uuid> = items(&app).iter().filter_map(|k| match k { ItemKind::Deck { deck_id, .. } => Some(*deck_id), _ => None }).collect();
        assert_eq!(added.len(), 2, "each import added its own deck once, and the old deck stayed out: {added:?}");
        assert!(!added.contains(&old.id), "the deck from before the upload is not added");
        assert!(decks.iter().all(|d| added.contains(d)));
        assert_eq!(toasts(&op), [("“Talk” is ready".to_string(), false), ("“Notices” is ready".to_string(), false)]);
    }

    fn hint_shown(op: &Operator) -> bool { find(op, "DropHint::box").is_some() }

    /// What winit sends for a drop of `files`: one hover per file, then one drop per file.
    fn drag_and_drop(op: &Operator, files: &[PathBuf]) {
        for f in files { op.window_events().dispatch(&WindowEvent::HoveredFile(f.clone())); }
        for f in files { op.window_events().dispatch(&WindowEvent::DroppedFile(f.clone())); }
    }

    #[test]
    fn winit_drop_events_map_to_drop_events() {
        let p = PathBuf::from("a.png");
        assert_eq!(drop_event(&WindowEvent::HoveredFile(p.clone())), Some(DropEvent::Hovered));
        assert_eq!(drop_event(&WindowEvent::HoveredFileCancelled), Some(DropEvent::Cancelled));
        assert_eq!(drop_event(&WindowEvent::DroppedFile(p.clone())), Some(DropEvent::Dropped(p)));
        assert_eq!(drop_event(&WindowEvent::Focused(true)), None);
    }

    #[test]
    fn hovering_files_shows_the_drop_hint_until_cancelled() {
        let app = test_app();
        let op = opened(&app);
        assert!(!hint_shown(&op));
        op.window_events().dispatch(&WindowEvent::HoveredFile("a.mp4".into()));
        let hint = find(&op, "DropHint::box").expect("the drop hint");
        assert_eq!(hint.accessible_label().unwrap_or_default(), "Drop to add to the service");
        assert_eq!(hint.accessible_description().unwrap_or_default(), "Videos, pictures, music, presentations (PowerPoint, Keynote, PDF), SongSelect lyric files");
        op.window_events().dispatch(&WindowEvent::HoveredFileCancelled);
        assert!(!hint_shown(&op));
    }

    #[test]
    fn dropped_pictures_ask_once_then_make_one_deck() {
        let app = test_app();
        let op = opened(&app);
        let dir = folder();
        let files = [png(&dir, "1.png", 1), png(&dir, "2.png", 2), png(&dir, "3.png", 3)];
        drag_and_drop(&op, &files);
        assert!(!hint_shown(&op), "a drop hides the hint");
        assert_eq!(op.state.borrow().asks, 0, "the batch waits for the end of the turn");
        mock_elapsed_time(Duration::from_millis(1));
        assert_eq!(op.state.borrow().asks, 1, "asked once for the three files");
        let q = op.ui.get_ask().row_data(0).unwrap();
        assert_eq!((q.text.as_str(), q.ok_label.as_str(), q.cancel_label.as_str()), ("Make 3 pictures into one set of slides?", "One slide set", "Separate pictures"));
        ask::answer(&op, true, String::new());
        assert_eq!(op.ui.get_ask().row_data(0).unwrap().text, "Name these slides");
        ask::answer(&op, true, "Easter".into());
        assert!(matches!(items(&app)[..], [ItemKind::Deck { .. }]));
        assert_eq!(app.data.lock().unwrap().decks[0].slides.len(), 3);
    }

    #[test]
    fn separate_pictures_adds_each_one() {
        let app = test_app();
        let op = opened(&app);
        let dir = folder();
        drag_and_drop(&op, &[png(&dir, "1.png", 1), png(&dir, "2.png", 2)]);
        mock_elapsed_time(Duration::from_millis(1));
        ask::answer(&op, false, String::new());
        assert_eq!(op.state.borrow().asks, 0);
        assert!(matches!(items(&app)[..], [ItemKind::Media { .. }, ItemKind::Media { .. }]));
        assert_eq!(toasts(&op), [("1.png added".to_string(), false), ("2.png added".to_string(), false)]);
    }

    #[test]
    fn a_mixed_drop_is_added_without_asking() {
        let app = test_app();
        let op = opened(&app);
        let dir = folder();
        let txt = dir.join("song.txt");
        std::fs::write(&txt, "A Test Song\r\n\r\nVerse 1\r\nLine one\r\n\r\nTest Writer\r\nCCLI Song # 777\r\n© 2026 Test Music\r\nCCLI License # 1").unwrap();
        drag_and_drop(&op, &[png(&dir, "1.png", 1), txt]);
        mock_elapsed_time(Duration::from_millis(1));
        assert_eq!(op.state.borrow().asks, 0);
        assert!(matches!(items(&app)[..], [ItemKind::Media { .. }, ItemKind::Song { .. }]));
    }

    #[test]
    fn two_drops_are_two_batches() {
        let app = test_app();
        let op = opened(&app);
        let dir = folder();
        drag_and_drop(&op, &[png(&dir, "1.png", 1)]);
        mock_elapsed_time(Duration::from_millis(1));
        drag_and_drop(&op, &[png(&dir, "2.png", 2)]);
        mock_elapsed_time(Duration::from_millis(1));
        assert_eq!(op.state.borrow().asks, 0, "one picture at a time is just added");
        assert_eq!(items(&app).len(), 2);
    }
}
