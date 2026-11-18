//! Songs from SongSelect (`ui/dialogs/songselect.slint`): the church signs in and downloads in its own web browser,
//! and each song file that lands in the Downloads folder comes into the library by itself. The app never signs in to
//! SongSelect, never reads its pages and never downloads for the church (design note of November 17, 2026).
//!
//! One `Inbox` per window watches Downloads while this window is open, or all the time when Settings → Songs →
//! "Bring in SongSelect downloads whenever RustyPresenter is open" is on, so a file never comes in twice.
use super::{DialogKind, add_strings};
use crate::{
    app::App,
    songselect::{self, inbox::{self, Export, Landed}, watch::{self, Watcher}},
    tr,
    ui::{SongSelectDlg, SongSelectRow, operator::{add_item::add_item_then, file_input::Filter, files, window::Operator}},
};
use serde_json::json;
use slint::{ComponentHandle, ModelRc, TimerMode, VecModel};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::{Rc, Weak},
    sync::{Arc, Mutex},
    time::Duration,
};
use uuid::Uuid;

/// How often Downloads is looked at.
const EVERY: Duration = Duration::from_secs(1);

pub fn title() -> String { tr!("Songs from SongSelect") }

/// The window's SongSelect watcher and the songs that came in while the SongSelect window was open.
#[derive(Default)]
pub struct Inbox {
    watch: RefCell<Option<Arc<Mutex<Watcher>>>>,
    timer: slint::Timer,
    /// A look at Downloads is under way (it runs off the window's thread).
    busy: Cell<bool>,
    /// The SongSelect window is open.
    window: Cell<bool>,
    rows: RefCell<Vec<Came>>,
}

/// One song file that came in, and what became of it.
#[derive(Clone, Debug)]
struct Came {
    file: String,
    title: String,
    ccli: String,
    /// The library song to add to the service, and what happened; or why the file could not be read.
    landed: Result<(Uuid, Landed), String>,
    in_service: bool,
}

/// Reads each file and brings its song in. `picked`: the person chose these files themselves, so any song file is
/// taken; otherwise (from Downloads) only SongSelect's own song files are, and anything else is left out silently.
/// Runs off the window's thread: it reads files and saves the library.
fn take(app: &App, paths: Vec<PathBuf>, picked: bool) -> Vec<Came> {
    paths.into_iter().filter_map(|path| {
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = match read_limited(&path) {
            Ok(b) => b,
            Err(e) => return picked.then(|| Came { file: file.clone(), title: file, ccli: String::new(), landed: Err(e), in_service: false }),
        };
        let found = if picked {
            Some(inbox::read_song_file(&file, &bytes).map_or_else(Export::Unreadable, Export::Song))
        } else {
            inbox::read_export(&file, &bytes)
        };
        Some(match found? {
            Export::Song(song) => {
                let (title, ccli) = (song.title.clone(), song.ccli.clone());
                Came { file, title, ccli, landed: app.bring_in_songselect(song), in_service: false }
            }
            Export::Unreadable(why) => Came { title: file.clone(), file, ccli: String::new(), landed: Err(why), in_service: false },
        })
    }).collect()
}

/// A file's bytes, refusing one too big to be a song.
fn read_limited(path: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    f.take(inbox::MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > inbox::MAX_BYTES { return Err(tr!("That file is too big to be a song file.")) }
    Ok(bytes)
}

/// Opens the window and starts watching Downloads.
pub fn open(op: &Rc<Operator>) {
    op.open_dialog(DialogKind::SongSelect);
    let inbox = &op.songselect;
    inbox.window.set(true);
    inbox.rows.borrow_mut().clear();
    connect(op);
    ensure(op);
    render(op);
    op.on_dialog_close(|op| {
        op.songselect.window.set(false);
        ensure(op);
    });
}

/// "Look for it on SongSelect" after a song search found nothing: the window opens, SongSelect opens in the
/// browser, and the words searched for are copied to paste into SongSelect's search.
pub fn look_for(op: &Rc<Operator>, words: &str) {
    open(op);
    open_songselect(op);
    let words = words.trim();
    if words.is_empty() { return }
    copy(op, words, &tr!("“{words}” is copied: paste it into SongSelect's search box (Ctrl+V).", words = words));
}

/// Copies the searched words. Tests keep them instead of touching the clipboard.
fn copy(op: &Operator, text: &str, done: &str) {
    #[cfg(test)]
    { testing::COPIED.with(|c| *c.borrow_mut() = Some(text.to_string())); op.toast(done, false) }
    #[cfg(not(test))]
    super::settings::copy(op, text, done)
}

fn open_songselect(op: &Operator) {
    if let Err(e) = songselect::open_in_browser() { op.toast(tr!("Could not open the web browser: {error}", error = e), true) }
}

/// Called when the settings may have changed (each new data read): starts or stops watching as they now say.
pub(crate) fn follow_setting(op: &Rc<Operator>) {
    ensure(op);
    // Settings → Songs → Songs folder: its messages show at the bottom of the window too.
    super::songs_folder::watch_notes(op);
}

/// Watching is wanted while the window is open, or always when Settings says so.
fn wanted(op: &Operator) -> bool {
    op.songselect.window.get() || op.state.borrow().snap.as_ref().is_some_and(|s| s.data.settings.songselect_inbox)
}

/// Starts or stops the watcher to match `wanted`. A watcher already running keeps going, so a file that came in
/// before is not taken again, and one downloaded before it started is never taken.
fn ensure(op: &Rc<Operator>) {
    let inbox = &op.songselect;
    let want = wanted(op);
    let running = inbox.watch.borrow().is_some();
    if want && !running {
        let Some(dir) = watch::downloads_folder() else { return };
        *inbox.watch.borrow_mut() = Some(Arc::new(Mutex::new(Watcher::new(dir))));
        let me: Weak<Operator> = Rc::downgrade(op);
        inbox.timer.start(TimerMode::Repeated, EVERY, move || if let Some(op) = me.upgrade() { poll(&op) });
    } else if !want && running {
        inbox.timer.stop();
        *inbox.watch.borrow_mut() = None;
    }
}

/// One look at Downloads, off the window's thread; the songs found come in.
pub(crate) fn poll(op: &Rc<Operator>) {
    let inbox = &op.songselect;
    if inbox.busy.get() { return }
    let Some(w) = inbox.watch.borrow().clone() else { return };
    inbox.busy.set(true);
    let app = op.app().clone();
    let me = Rc::downgrade(op);
    op.ops().blocking(move || {
        let paths = w.lock().map(|mut w| w.poll()).unwrap_or_default();
        take(&app, paths, false)
    }, move |came| {
        let Some(op) = me.upgrade() else { return };
        op.songselect.busy.set(false);
        arrived(&op, came.unwrap_or_default());
    });
}

/// Songs that came in: listed in the window while it is open, else a short message each.
fn arrived(op: &Rc<Operator>, came: Vec<Came>) {
    if came.is_empty() { return }
    if op.songselect.window.get() && op.dialog() == Some(DialogKind::SongSelect) {
        op.songselect.rows.borrow_mut().extend(came);
        return render(op);
    }
    for c in came {
        match &c.landed {
            Ok((_, Landed::Added)) => op.toast(tr!("“{title}” came in from SongSelect.", title = c.title), false),
            Ok((_, Landed::AlreadyThere)) => op.toast(tr!("“{title}” from SongSelect is already in your library.", title = c.title), false),
            Ok((_, Landed::ChordsAdded)) => op.toast(tr!("“{title}” was already in your library: its chords were added for the stage screen.", title = c.title), false),
            Ok((_, Landed::KeptBoth(new))) => op.toast(tr!("“{title}” from SongSelect has other words than your copy, so it was kept as “{new}”.", title = c.title, new = new), false),
            Err(why) => op.toast(tr!("{file} could not be read: {why}", file = c.file, why = why), true),
        }
    }
}

/// What happened, in the row under the title.
fn note(c: &Came) -> String {
    match &c.landed {
        Ok((_, Landed::Added)) => tr!("Added to your song library"),
        Ok((_, Landed::AlreadyThere)) => tr!("Already in your song library"),
        Ok((_, Landed::ChordsAdded)) => tr!("Chords added to your copy"),
        Ok((_, Landed::KeptBoth(new))) => tr!("Your copy has other words, so this one is kept as “{new}”", new = new),
        Err(why) => tr!("Could not read it: {why}", why = why),
    }
}

fn render(op: &Operator) {
    if op.dialog() != Some(DialogKind::SongSelect) { return }
    let g = op.ui.global::<SongSelectDlg>();
    let rows: Vec<SongSelectRow> = op.songselect.rows.borrow().iter().map(|c| {
        let ccli = if c.ccli.is_empty() { String::new() } else { add_strings::ccli(&c.ccli) };
        let note = note(c);
        let detail = if ccli.is_empty() { note.clone() } else { format!("{ccli} · {note}") };
        SongSelectRow {
            id: c.landed.as_ref().map(|(id, _)| id.to_string()).unwrap_or_default().into(),
            title: c.title.as_str().into(), ccli: ccli.into(), note: note.into(), detail: detail.into(),
            problem: c.landed.is_err(), can_add: c.landed.is_ok(), in_service: c.in_service,
        }
    }).collect();
    g.set_rows(ModelRc::new(VecModel::from(rows)));
    let dir = op.songselect.watch.borrow().as_ref().and_then(|w| w.lock().ok().map(|w| w.dir().to_path_buf()));
    let (text, problem) = match dir {
        Some(d) => (tr!("Watching your Downloads folder ({folder}) for songs.", folder = d.display()), false),
        None => (tr!("Your Downloads folder could not be found. Use “Choose a file…” for the songs you download."), true),
    };
    g.set_watching(text.into());
    g.set_watching_problem(problem);
}

/// "Add to the service" on a row: the song goes at the end of the service; the window stays open for more.
fn add(op: &Rc<Operator>, id: Uuid) {
    add_item_then(op, json!({ "kind": "song", "song_id": id }), false, move |op, _| {
        for c in op.songselect.rows.borrow_mut().iter_mut() {
            if c.landed.as_ref().is_ok_and(|(i, _)| *i == id) { c.in_service = true }
        }
        render(op);
    });
}

/// "Choose a file…": songs downloaded earlier, picked by the person, starting in Downloads.
fn choose_file(op: &Rc<Operator>) {
    if let Some(d) = watch::downloads_folder() { files::start_in(op, &d) }
    let me = Rc::downgrade(op);
    files::pick(op, Filter::Lyrics, true, move |paths| {
        let Some(op) = me.upgrade() else { return };
        let app = op.app().clone();
        let me = Rc::downgrade(&op);
        op.ops().blocking(move || take(&app, paths, true), move |came| {
            if let Some(op) = me.upgrade() { arrived(&op, came.unwrap_or_default()) }
        });
    });
}

fn connect(op: &Rc<Operator>) {
    let g = op.ui.global::<SongSelectDlg>();
    let with = |f: fn(&Rc<Operator>)| {
        let me: Weak<Operator> = Rc::downgrade(op);
        move || if let Some(op) = me.upgrade() { f(&op) }
    };
    g.on_open_songselect(with(|op| open_songselect(op)));
    g.on_choose_file(with(choose_file));
    g.on_close(with(|op| op.close_dialog()));
    let me = Rc::downgrade(op);
    g.on_add(move |id| if let (Some(op), Ok(id)) = (me.upgrade(), Uuid::parse_str(&id)) { add(&op, id) });
}

#[cfg(test)]
pub(crate) mod testing {
    thread_local! {
        /// What "Look for it on SongSelect" copied, instead of the real clipboard.
        pub(crate) static COPIED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    }
}

#[cfg(test)]
#[path = "songselect_tests.rs"]
mod tests;
