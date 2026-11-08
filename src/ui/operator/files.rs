//! Adding files to the service (`uploadFiles`, `watchDeckJob` and `imagesToDeck` in
//! `web/operator.js:536-598`): which import a dropped or picked file takes, the window glue that runs the
//! imports one after another as client jobs, and the pure steps it is built from. Files arrive from the native
//! file picker (`pick`) or a desktop drop (`wire`, through the window's winit event filter).
use super::{
    add_item::add_item,
    file_input::{DropBatch, DropEvent, DropPlan, Filter, drop_plan},
    overlays::JobHandle, strings as tr, window::Operator,
};
use crate::{
    app::{ImportPurpose, Imported},
    import::{CANCELLED, deck::DECK_EXT, media},
    model::{AssetId, Deck},
    platform::winit_events::{self, Dispatcher, WindowEvent},
    ui::dialogs,
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
        _ => tr::default_deck_title(),
    }
}

/// What the window keeps for adding files: the deck imports waiting for their job to end, the desktop drop in
/// progress, the folder the picker last opened, and the window's one winit event filter.
#[derive(Default)]
pub struct FilesState {
    watches: RefCell<Vec<DeckWatch>>,
    drop: RefCell<DropBatch>,
    #[cfg_attr(test, allow(dead_code))]
    last_folder: RefCell<Option<PathBuf>>,
    events: Dispatcher<WindowEvent>,
}

/// A deck import whose result is added to the service when its job ends (`watchDeckJob`, Task 6c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeckWatch(pub Uuid);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeckWatchStep {
    Wait,
    /// The job ended with nothing made: failed (its card says so) or cancelled.
    Nothing,
    /// The job made this deck (id, title).
    Ready(Uuid, String),
}

impl DeckWatch {
    /// One look at a new snapshot. Only the job's own result counts, never "every new deck".
    pub fn step(self, ends: &[(Uuid, Option<Uuid>)], decks: &[Deck]) -> DeckWatchStep {
        match ends.iter().find(|(job, _)| *job == self.0) {
            None => DeckWatchStep::Wait,
            Some((_, None)) => DeckWatchStep::Nothing,
            // A read can catch the end of the job and not the deck it made; the next read has both.
            Some((_, Some(id))) => decks.iter().find(|d| d.id == *id).map_or(DeckWatchStep::Wait, |d| DeckWatchStep::Ready(d.id, d.title.clone())),
        }
    }
}

/// `deck/from-images`'s answer.
#[derive(Deserialize)]
struct Added { id: Uuid }

/// The name a file's toasts and job card use.
fn file_name(path: &Path) -> String { path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned() }

/// `uploadFiles(files, {add: true})`: each file in turn, as a client job "Uploading `<name>`", then into the
/// service. A song or media file is added quietly with its own toast; a deck is added when its job ends. A
/// failure toasts "`<file>`: `<error>`" and the next file goes on. The open dialog is redrawn at the end.
pub fn add_files(op: &Rc<Operator>, paths: Vec<PathBuf>) { next_file(op, paths.into()); }

/// Adds an imported song to the service; `toast` ends that file.
fn add_song(op: &Rc<Operator>, id: Uuid, toast: String, then: impl FnOnce(&Rc<Operator>, Result<Option<String>, String>) + 'static) {
    let me = Rc::downgrade(op);
    add_item(op, json!({ "kind": "song", "song_id": id }), true, move |r| {
        if let Some(op) = me.upgrade() { then(&op, r.map(|_| Some(toast))); }
    });
}

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
        // Each file ends with its toast (none for a deck, which comes later), then the next file starts.
        let then = move |op: &Rc<Operator>, r: Result<Option<String>, String>| {
            match r {
                Ok(text) => if let Some(text) = text { op.toast(text, false) },
                Err(e) => op.toast(tr::file_failed(&name, &e), true),
            }
            next_file(op, rest);
        };
        match r {
            // The operator pressed Cancel on the card: no toast, and the next file goes on.
            Err(e) if e == CANCELLED => then(&op, Ok(None)),
            Err(e) => then(&op, Err(e)),
            Ok(Imported::Media(m)) => {
                // No "end": the core gives a video what Settings → Media says.
                let item = json!({ "kind": "media", "media_id": m.id, "autoplay": true, "looped": false });
                let me = Rc::downgrade(&op);
                add_item(&op, item, true, move |r| {
                    let Some(op) = me.upgrade() else { return };
                    then(&op, r.map(|_| Some(tr::media_added(&shown))));
                });
            }
            Ok(Imported::Song { id: Some(id), title, .. }) => add_song(&op, id, tr::song_imported(&title), then),
            // Saved nothing: a song with this CCLI # is in the library. Asked one file at a time, like the rest.
            Ok(Imported::Song { id: None, title, .. }) => {
                let me = Rc::downgrade(&op);
                dialogs::choose(&op, &tr::song_in_library(&title), &tr::update_existing(), &tr::keep_both(), move |answer| {
                    let Some(op) = me.upgrade() else { return };
                    let Some(update) = answer else { return then(&op, Ok(None)) };
                    let (mode, toast) = if update { ("update", tr::song_updated(&title)) } else { ("keep_both", tr::song_imported(&title)) };
                    let me = Rc::downgrade(&op);
                    op.ops.import_song::<Added>(path, mode, move |r| {
                        let Some(op) = me.upgrade() else { return };
                        match r {
                            Ok(a) => add_song(&op, a.id, toast, then),
                            Err(e) => then(&op, Err(e)),
                        }
                    });
                });
            }
            Ok(Imported::Job(job)) => {
                op.files.watches.borrow_mut().push(DeckWatch(job));
                op.watch_decks();
                then(&op, Ok(None));
            }
            // A media, lyrics or PowerPoint import never answers with a bare asset.
            Ok(Imported::Asset(_)) => then(&op, Err(tr::action_stopped().into())),
        }
    });
}

impl Operator {
    /// Looks at each deck import waiting for its job to end; called by `apply_snapshot`.
    pub(super) fn watch_decks(&self) {
        let Some(op) = self.me.upgrade() else { return };
        let Some(s) = self.state.borrow().snap.clone() else { return };
        let mut ready = VecDeque::new();
        self.files.watches.borrow_mut().retain(|w| match w.step(&s.job_ends, &s.data.decks) {
            DeckWatchStep::Wait => true,
            DeckWatchStep::Nothing => false,
            DeckWatchStep::Ready(id, title) => { ready.push_back((id, title)); false }
        });
        add_decks(&op, ready);
    }
}

/// Adds each finished deck, with its own "“T” is ready" toast.
fn add_decks(op: &Rc<Operator>, mut rest: VecDeque<(Uuid, String)>) {
    let Some((id, title)) = rest.pop_front() else { return };
    let me = Rc::downgrade(op);
    add_item(op, json!({ "kind": "deck", "deck_id": id }), true, move |r| {
        let Some(op) = me.upgrade() else { return };
        match r {
            Ok(_) => { op.toast(tr::deck_ready(&title), false); add_decks(&op, rest) }
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
            Ok(_) => op.toast(tr::action_stopped(), true),
            Err(e) => op.toast(e, true),
        }
    });
}

fn name_deck(op: &Rc<Operator>, p: Pictures) {
    let me = Rc::downgrade(op);
    let first = p.first.clone();
    dialogs::prompt(op, &tr::name_these_slides(), &first, move |answer| {
        let Some(op) = me.upgrade() else { return };
        let Pictures { assets, job, .. } = p;
        let me = Rc::downgrade(&op);
        op.ops.call::<Added>("deck/from-images", json!({ "title": deck_title(answer.as_deref()), "assets": assets }), move |r| {
            let Some(op) = me.upgrade() else { return };
            match r {
                Ok(Added { id }) => {
                    let me = Rc::downgrade(&op);
                    add_item(&op, json!({ "kind": "deck", "deck_id": id, "slideshow": true }), false, move |r| {
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
            dialogs::confirm(op, &question, &tr::one_slide_set(), &tr::separate_pictures(), false, move |one| {
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

/// The native folder picker, starting in `start` when that folder exists. `done` runs only when one was picked.
/// Tests take the next path queued with `testing::will_pick`.
pub fn pick_folder(op: &Rc<Operator>, start: &Path, done: impl FnOnce(PathBuf) + 'static) {
    #[cfg(test)]
    {
        let _ = (op, start);
        if let Some(p) = testing::PICKS.with(|p| p.borrow_mut().pop_front()).and_then(|p| p.into_iter().next()) { done(p) }
    }
    #[cfg(not(test))]
    {
        let mut dialog = rfd::AsyncFileDialog::new().set_parent(&op.window().window_handle());
        if start.is_dir() { dialog = dialog.set_directory(start); }
        let picking = async move { if let Some(f) = dialog.pick_folder().await { done(f.path().to_path_buf()) } };
        if let Err(e) = slint::spawn_local(picking) { op.toast(e.to_string(), true) }
    }
}

/// The native save dialog, starting from `name`. `done` runs only when a place was chosen. Tests take the next
/// path queued with `testing::will_pick`.
pub fn pick_save(op: &Rc<Operator>, filter: Filter, name: &str, done: impl FnOnce(PathBuf) + 'static) {
    #[cfg(test)]
    {
        let _ = (op, filter, name);
        if let Some(p) = testing::PICKS.with(|p| p.borrow_mut().pop_front()).and_then(|p| p.into_iter().next()) { done(p) }
    }
    #[cfg(not(test))]
    {
        let mut dialog = rfd::AsyncFileDialog::new().add_filter(filter.name(), &filter.extensions()).set_file_name(name).set_parent(&op.window().window_handle());
        if let Some(dir) = op.files.last_folder.borrow().as_ref() { dialog = dialog.set_directory(dir); }
        let picking = async move { if let Some(f) = dialog.save_file().await { done(f.path().to_path_buf()) } };
        if let Err(e) = slint::spawn_local(picking) { op.toast(e.to_string(), true) }
    }
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
        for f in ["a.ppt", "a.pptx", "a.pps", "a.ppsx", "a.pot", "a.potx", "a.pptm", "a.odp", "a.key", "a.pdf"] { assert_eq!(route(Path::new(f)), Route::Deck, "{f}"); }
        for f in ["a.mp4", "a.png", "a.mp3", "no-extension"] { assert_eq!(route(Path::new(f)), Route::Media, "{f}"); }
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

    const SONG: &str = "A Test Song\r\n\r\nVerse 1\r\nLine one\r\nLine two\r\n\r\nTest Writer\r\nCCLI Song # 12345\r\n© 2026 Test Music\r\nCCLI License # 98765";

    /// A song is imported, then its file comes back with new words.
    fn song_again(app: &Arc<App>, op: &Rc<Operator>) -> (Uuid, PathBuf) {
        let txt = folder().join("song.txt");
        std::fs::write(&txt, SONG).unwrap();
        add_files(op, vec![txt.clone()]);
        let ItemKind::Song { song_id, .. } = items(app)[0].clone() else { panic!() };
        std::fs::write(&txt, SONG.replace("Line one", "New words")).unwrap();
        add_files(op, vec![txt.clone()]);
        assert_eq!(op.ui.get_ask().row_data(0).unwrap().text, "“A Test Song” is already in the library.");
        assert_eq!((items(app).len(), app.data.lock().unwrap().songs.len()), (1, 1), "nothing happens before the answer");
        (song_id, txt)
    }

    fn lyrics(app: &App, id: Uuid) -> String { app.data.lock().unwrap().songs.iter().find(|s| s.id == id).unwrap().lyrics.clone() }

    #[test]
    fn duplicate_song_file_asks_update_or_keep_both() {
        let app = test_app();
        let op = opened(&app);
        let (id, _) = song_again(&app, &op);
        ask::answer(&op, true, String::new());
        assert_eq!(app.data.lock().unwrap().songs.len(), 1);
        assert!(lyrics(&app, id).contains("New words"), "{}", lyrics(&app, id));
        assert!(matches!(items(&app)[1], ItemKind::Song { song_id, .. } if song_id == id), "the same song is added");
        assert_eq!(toasts(&op).last().unwrap().0, "Updated “A Test Song”");

        let app = test_app();
        let op = opened(&app);
        let (id, _) = song_again(&app, &op);
        ask::answer_other(&op);
        let songs = app.data.lock().unwrap().songs.clone();
        assert_eq!(songs.len(), 2);
        assert!(!lyrics(&app, id).contains("New words"));
        let ItemKind::Song { song_id, .. } = items(&app)[1].clone() else { panic!() };
        assert_ne!(song_id, id);
        assert!(lyrics(&app, song_id).contains("New words"));
        assert_eq!(toasts(&op).last().unwrap().0, "Imported “A Test Song”");
    }

    #[test]
    fn closing_duplicate_confirm_adds_nothing() {
        let app = test_app();
        let op = opened(&app);
        song_again(&app, &op);
        ask::answer(&op, false, String::new());
        assert_eq!((items(&app).len(), app.data.lock().unwrap().songs.len()), (1, 1));
    }

    #[test]
    fn a_duplicate_song_holds_back_the_next_file() {
        let app = test_app();
        let op = opened(&app);
        let (_, txt) = song_again(&app, &op);
        ask::answer(&op, false, String::new());
        let dir = txt.parent().unwrap().to_path_buf();
        add_files(&op, vec![txt, png(&dir, "next.png", 4)]);
        assert_eq!(items(&app).len(), 1, "the picture waits for the question");
        ask::answer(&op, false, String::new());
        assert_eq!(items(&app).len(), 2);
    }

    #[test]
    fn duplicate_answer_with_the_file_gone_says_so_and_goes_on() {
        let app = test_app();
        let op = opened(&app);
        let (_, txt) = song_again(&app, &op);
        std::fs::remove_file(&txt).unwrap();
        ask::answer(&op, true, String::new());
        let (text, err) = toasts(&op).last().unwrap().clone();
        assert!(err && text.starts_with("song.txt: "), "{text}");
        assert_eq!(items(&app).len(), 1);
    }

    #[test]
    fn add_txt_imports_song_and_adds_it() {
        let app = test_app();
        let op = opened(&app);
        let txt = folder().join("song.txt");
        std::fs::write(&txt, "A Test Song\r\n\r\nVerse 1\r\nLine one\r\nLine two\r\n\r\nTest Writer\r\nCCLI Song # 12345\r\n© 2026 Test Music\r\nCCLI License # 98765").unwrap();
        add_files(&op, vec![txt.clone()]);
        let ItemKind::Song { song_id, .. } = items(&app)[0].clone() else { panic!() };
        assert!(app.data.lock().unwrap().songs.iter().any(|s| s.id == song_id && s.title == "A Test Song"));
        assert_eq!(toasts(&op), [("Imported “A Test Song”".to_string(), false)]);
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
    fn deck_watch_takes_only_its_own_result() {
        let (job, other) = (Uuid::new_v4(), Uuid::new_v4());
        let old = Deck { id: Uuid::new_v4(), title: "Old".into(), slides: vec![], notes: vec![] };
        let new = Deck { id: Uuid::new_v4(), title: "Talk".into(), slides: vec![], notes: vec![] };
        let w = DeckWatch(job);
        assert_eq!(w.step(&[(other, Some(old.id))], &[old.clone(), new.clone()]), DeckWatchStep::Wait, "another job's end, and a new deck that may be anyone's");
        assert_eq!(w.step(&[(job, Some(new.id))], std::slice::from_ref(&old)), DeckWatchStep::Wait, "a read that has the end but not yet the deck");
        assert_eq!(w.step(&[(job, Some(new.id))], &[old.clone(), new.clone()]), DeckWatchStep::Ready(new.id, "Talk".into()));
        assert_eq!(w.step(&[(job, None)], &[old, new]), DeckWatchStep::Nothing, "failed or cancelled");
    }

    #[test]
    fn job_ends_are_kept_for_jobs_that_leave_the_list() {
        let app = test_app();
        let deck = Uuid::new_v4();
        let (well, failed, noted) = (app.job_start("a"), app.job_start("b"), app.job_start("c"));
        let (cancelled, _) = app.job_start_cancellable("d");
        app.job_result(well, deck);
        app.job_result(noted, deck);
        app.job_note(noted, "Check the slides.".into());
        app.job_result(failed, deck);
        app.job_end(well, None);
        app.job_end(failed, Some("No.".into()));
        app.job_end(noted, None);
        app.job_end_cancelled(cancelled);
        let ends = crate::ui::operator::view::OperatorSnapshot::read(&app).job_ends;
        assert_eq!(ends, [(well, Some(deck)), (failed, None), (noted, Some(deck)), (cancelled, None)]);
        for _ in 0..100 { let j = app.job_start("x"); app.job_end(j, None); }
        assert_eq!(crate::ui::operator::view::OperatorSnapshot::read(&app).job_ends.len(), 64, "the list has an end");
    }

    #[test]
    fn failed_or_cancelled_import_adds_nothing() {
        let app = test_app();
        let op = opened(&app);
        let (bad, stopped) = (app.job_start("a"), app.job_start_cancellable("b").0);
        op.files.watches.borrow_mut().extend([DeckWatch(bad), DeckWatch(stopped)]);
        app.edit(|d| { d.decks.push(Deck { id: Uuid::new_v4(), title: "Someone else's".into(), slides: vec![], notes: vec![] }); Ok(()) }).unwrap();
        app.job_end(bad, Some("No.".into()));
        app.job_end_cancelled(stopped);
        refresh(&op);
        assert!(items(&app).is_empty());
        assert!(op.files.watches.borrow().is_empty(), "the watches are done");
    }

    #[test]
    fn cancelled_media_import_shows_no_toast() {
        let app = test_app();
        let op = opened(&app);
        let dir = folder();
        crate::ui::ops::testing::will_import(Err(crate::import::CANCELLED.to_string()));
        add_files(&op, vec![dir.join("long.mp4"), png(&dir, "ok.png", 4)]);
        assert_eq!(toasts(&op), [("ok.png added".to_string(), false)], "no word about the cancel, and the next file goes on");
        assert_eq!(items(&app).len(), 1);
    }

    #[test]
    fn finished_powerpoint_job_adds_its_deck_and_says_ready() {
        let app = test_app();
        let op = opened(&app);
        let job = app.job_start("Importing “Talk” from PowerPoint");
        op.files.watches.borrow_mut().push(DeckWatch(job));
        refresh(&op);
        assert!(items(&app).is_empty(), "still converting");
        let id = Uuid::new_v4();
        app.edit(|d| { d.decks.push(Deck { id, title: "Talk".into(), slides: vec![], notes: vec![] }); Ok(()) }).unwrap();
        app.job_result(job, id);
        app.job_end(job, None);
        refresh(&op);
        assert!(matches!(items(&app)[..], [ItemKind::Deck { .. }]));
        assert_eq!(toasts(&op), [("“Talk” is ready".to_string(), false)]);
        refresh(&op);
        assert_eq!(items(&app).len(), 1, "added once");
    }

    #[test]
    fn import_powerpoint_adds_only_the_decks_its_jobs_made() {
        let app = test_app();
        let old = Deck { id: Uuid::new_v4(), title: "Last week".into(), slides: vec![], notes: vec![] };
        app.edit(|d| { d.decks.push(old.clone()); Ok(()) }).unwrap();
        let op = opened(&app);
        let jobs = [app.job_start("Importing “Talk” from PowerPoint"), app.job_start("Importing “Notices” from PowerPoint")];
        for job in jobs { crate::ui::ops::testing::will_import(Ok(Imported::Job(job))) }
        testing::will_pick(vec![PathBuf::from("Talk.pptx"), PathBuf::from("Notices.pptx")]);
        crate::ui::dialogs::add::open_add(&op, AddTab::Deck);
        slint::ComponentHandle::global::<crate::ui::AddState>(&op.ui).invoke_import_deck();
        refresh(&op);
        assert!(items(&app).is_empty(), "both decks are still converting");
        let decks = [("Talk", jobs[0]), ("Notices", jobs[1])].map(|(title, job)| {
            let id = Uuid::new_v4();
            app.edit(|d| { d.decks.push(Deck { id, title: title.into(), slides: vec![], notes: vec![] }); Ok(()) }).unwrap();
            app.job_result(job, id);
            app.job_end(job, None);
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
