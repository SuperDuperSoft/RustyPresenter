//! Any presentation or PDF to a deck of slide images (Plan 4 Task 6c), through PowerPoint on Windows when it is
//! installed, otherwise LibreOffice, then pdfium. Each import is a cancellable job; the deck is added in one edit
//! only after every slide is in the store, so a failed or cancelled import adds nothing.
use super::{
    CANCELLED, Stage, WorkDir,
    libreoffice::LibreOffice,
    pdf::PdfRenderer,
    percent,
    powerpoint::PowerPoint,
    sniff::{self, ENCRYPTED, Kind, NOT_PRESENTATION},
    strings,
};
use crate::{
    app::{App, FALLBACK_NOTE},
    model::Deck,
    platform::tools::{self, Soffice},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Every file name a deck import accepts. The operator's router, its file picker and the drop read this list.
pub const DECK_EXT: &[&str] = &["pptx", "ppt", "pps", "ppsx", "pot", "potx", "pptm", "odp", "key", "pdf"];

pub use super::strings::KEYNOTE_PACKAGE;

/// Whether LibreOffice can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lo {
    None,
    /// A Snap build, which cannot read our files.
    Snap,
    Ok,
}

/// Which converter makes the slides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Pdf,
    /// `fallback`: LibreOffice is there if PowerPoint fails.
    PowerPoint { fallback: bool },
    LibreOffice,
    /// It needs LibreOffice, which is not installed.
    Missing,
}

/// The route for a file of this kind, or the message that refuses it. `ppt` is PowerPoint being installed.
pub fn route(kind: &Kind, ppt: bool, lo: Lo) -> Result<Route, String> {
    let libreoffice = || match lo {
        Lo::Ok => Ok(Route::LibreOffice),
        Lo::Snap => Err(strings::LO_SNAP.to_string()),
        Lo::None => Ok(Route::Missing),
    };
    match kind {
        Kind::Encrypted => Err(ENCRYPTED.to_string()),
        Kind::NotPresentation => Err(NOT_PRESENTATION.to_string()),
        Kind::Pdf => Ok(Route::Pdf),
        Kind::Ooxml { .. } | Kind::Ppt | Kind::Odp if ppt => Ok(Route::PowerPoint { fallback: lo == Lo::Ok }),
        _ => libreoffice(),
    }
}

/// What the Slides tab and About say about the converters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConverterStatus {
    pub powerpoint: bool,
    pub libreoffice: Lo,
    pub pdfium: bool,
}

/// Looks for LibreOffice on this computer.
pub type FindSoffice = Box<dyn Fn() -> Option<Soffice> + Send + Sync>;

/// The slide converters, made once at start-up by `import::init` and held by `App`.
pub struct Converters {
    /// `None` until LibreOffice is found; see `libreoffice()`.
    pub libreoffice: Mutex<Option<Arc<LibreOffice>>>,
    /// `None` when pdfium could not be loaded: the PDF and LibreOffice routes then fail with the reinstall message.
    pub pdf: Option<Arc<PdfRenderer>>,
    /// `Some` on Windows with PowerPoint installed.
    pub powerpoint: Option<PowerPoint>,
    data_dir: PathBuf,
    find: FindSoffice,
}

impl Converters {
    pub fn new(data_dir: PathBuf, find: FindSoffice, pdf: Option<Arc<PdfRenderer>>, powerpoint: Option<PowerPoint>) -> Converters {
        Converters { libreoffice: Mutex::new(None), pdf, powerpoint, data_dir, find }
    }

    /// LibreOffice, looking for it again while it is missing: a volunteer may install it while the app runs.
    /// Called at the start of every import and by Check again. It may read the disk and the registry, so not on the
    /// UI thread.
    pub fn libreoffice(&self) -> Option<Arc<LibreOffice>> {
        let mut lo = self.libreoffice.lock().unwrap_or_else(|e| e.into_inner());
        if lo.is_none() {
            *lo = (self.find)().map(|soffice| LibreOffice::new(soffice, &self.data_dir));
        }
        lo.clone()
    }

    /// What is there now, without looking again, so it is cheap enough for the UI thread.
    pub fn status(&self) -> ConverterStatus {
        let lo = self.libreoffice.lock().unwrap_or_else(|e| e.into_inner());
        ConverterStatus { powerpoint: self.powerpoint.is_some(), libreoffice: lo_state(lo.as_deref()), pdfium: self.pdf.is_some() }
    }
}

fn lo_state(lo: Option<&LibreOffice>) -> Lo {
    match lo {
        None => Lo::None,
        Some(lo) if lo.snap() => Lo::Snap,
        Some(_) => Lo::Ok,
    }
}

/// Imports the user's `file` as a deck titled `display`, as a cancellable job, and returns the job's id. The file is
/// copied, never moved. Needs a tokio runtime.
pub fn start(app: Arc<App>, file: PathBuf, display: String) -> Uuid {
    spawn(app, file, display, false)
}

/// `start` for a file already staged in the store's tmp (an upload), which the import takes over and removes.
pub(crate) fn start_staged(app: Arc<App>, file: PathBuf, display: String) -> Uuid {
    spawn(app, file, display, true)
}

/// Removes a staged upload when the import is over, however it ends.
struct Staged(PathBuf);

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn spawn(app: Arc<App>, file: PathBuf, display: String, staged: bool) -> Uuid {
    let (id, cancel) = app.job_start_cancellable(&strings::importing(&display));
    tokio::spawn(async move {
        let staged = staged.then(|| Staged(file.clone()));
        let result = import(&app, id, &file, &display, &cancel).await;
        drop(staged);
        match result {
            Ok(()) => app.job_end(id, None),
            Err(_) if cancel.is_cancelled() => app.job_end_cancelled(id),
            Err(e) if e == CANCELLED => app.job_end_cancelled(id),
            Err(e) => app.job_end(id, Some(e)),
        }
    });
    id
}

/// The slide shape and visible slide count the file says it has, when it says.
fn expected(kind: &Kind) -> (Option<f32>, Option<usize>) {
    match kind {
        Kind::Ooxml { slide_size, slides, hidden, .. } => (
            slide_size.filter(|(cx, cy)| *cx > 0 && *cy > 0).map(|(cx, cy)| cx as f32 / cy as f32),
            slides.map(|n| n.saturating_sub(*hidden)),
        ),
        _ => (None, None),
    }
}

/// The note for a PDF whose page count is not the visible slide count.
fn count_note(expected: Option<usize>, imported: usize) -> Option<String> {
    expected.filter(|n| *n != imported).map(|n| strings::slides_expected(n, imported))
}

/// One import, from the user's file to a deck in the library. The work folder goes when this returns.
async fn import(app: &Arc<App>, id: Uuid, file: &Path, display: &str, cancel: &CancellationToken) -> Result<(), String> {
    // A Keynote package is a folder, which the picker and a drop hand over like a file.
    if file.is_dir() {
        return Err(KEYNOTE_PACKAGE.to_string());
    }
    let conv = app.converters().ok_or_else(|| strings::PDFIUM_MISSING.to_string())?;
    let open_failed = |e: String| {
        eprintln!("Deck import: {}: {e}", file.display());
        strings::could_not_open(display)
    };
    let (src, c) = (file.to_path_buf(), conv.clone());
    let (kind, lo) = tokio::task::spawn_blocking(move || (sniff::sniff(&src), c.libreoffice()))
        .await
        .map_err(|e| open_failed(e.to_string()))?;
    let kind = kind.map_err(|e| open_failed(e.to_string()))?;
    let route = route(&kind, conv.powerpoint.is_some(), lo_state(lo.as_deref()))?;
    if route == Route::Missing {
        return Err(strings::install_libreoffice(tools::install_hint(tools::OS)));
    }

    let work = WorkDir::new(&app.store.tmp()).map_err(|e| open_failed(e.to_string()))?;
    let input = work.path().join(format!("input.{}", sniff::staged_ext(&kind).unwrap_or("bin")));
    tokio::fs::copy(file, &input).await.map_err(|e| open_failed(e.to_string()))?;
    app.job_progress(id, percent(Stage::Prepare, 1.0));

    let steps = Steps { app, id, display, conv: &conv, input: &input, work: work.path(), cancel };
    let (ratio, count) = expected(&kind);
    let (slides, notes) = match route {
        Route::Pdf => (steps.render(input.clone(), None).await?, vec![]),
        Route::LibreOffice => steps.libreoffice(lo, ratio, count, false).await?,
        Route::PowerPoint { fallback } => match steps.powerpoint().await {
            Ok(slides) => (slides, vec![]),
            Err(e) if e == CANCELLED || cancel.is_cancelled() => return Err(CANCELLED.to_string()),
            Err(e) if fallback => {
                eprintln!("PowerPoint could not import “{display}” ({e}); trying LibreOffice.");
                steps.libreoffice(lo, ratio, count, true).await?
            }
            Err(e) => return Err(e),
        },
        Route::Missing => unreachable!("refused above"),
    };

    // Publish: every slide into the store, then the deck in one edit.
    let total = slides.len();
    let mut assets = Vec::with_capacity(total);
    for (i, png) in slides.into_iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(CANCELLED.to_string());
        }
        let a = app.clone();
        let stored = tokio::task::spawn_blocking(move || a.store.import_file(&png, "png").map_err(|e| format!("{}: {e}", png.display())))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
        assets.push(stored.map_err(|e| {
            eprintln!("Deck import: {e}");
            strings::SLIDE_NOT_SAVED.to_string()
        })?);
        app.job_progress(id, percent(Stage::Publish, (i + 1) as f32 / total as f32));
    }
    if cancel.is_cancelled() {
        return Err(CANCELLED.to_string());
    }
    let deck = Uuid::new_v4();
    // Before the edit, so the snapshot that first shows the deck also shows whose it is.
    app.job_result(id, deck);
    app.edit(|d| {
        d.decks.push(Deck { id: deck, title: display.to_string(), slides: assets });
        Ok(())
    })?;
    for note in notes {
        app.job_note(id, note);
    }
    Ok(())
}

/// What every step of one import needs.
struct Steps<'a> {
    app: &'a Arc<App>,
    id: Uuid,
    display: &'a str,
    conv: &'a Converters,
    input: &'a Path,
    work: &'a Path,
    cancel: &'a CancellationToken,
}

impl Steps<'_> {
    /// The job's label while it waits for a converter another import is using, and back again.
    fn waiting(&self, waiting_label: &'static str) -> impl Fn(bool) + Sync + '_ {
        move |waiting| self.app.job_label(self.id, &if waiting { waiting_label.to_string() } else { strings::importing(self.display) })
    }

    /// PDF pages to `work/slides/slide-001.png`….
    async fn render(&self, pdf: PathBuf, ratio: Option<f32>) -> Result<Vec<PathBuf>, String> {
        let renderer = self.conv.pdf.clone().ok_or_else(|| strings::PDFIUM_MISSING.to_string())?;
        let (app, id) = (self.app.clone(), self.id);
        let progress = Box::new(move |done: usize, total: usize| app.job_progress(id, percent(Stage::Render, done as f32 / total.max(1) as f32)));
        renderer.render(pdf, self.work.join("slides"), ratio, progress, self.cancel.clone()).await
    }

    /// LibreOffice to PDF, then pdfium, with the notes the finished job keeps.
    async fn libreoffice(&self, lo: Option<Arc<LibreOffice>>, ratio: Option<f32>, count: Option<usize>, fell_back: bool) -> Result<(Vec<PathBuf>, Vec<String>), String> {
        let lo = lo.ok_or_else(|| strings::install_libreoffice(tools::install_hint(tools::OS)))?;
        let pdf = lo.to_pdf(self.input, self.work, &self.waiting(strings::WAITING_LIBREOFFICE), self.cancel).await?;
        self.app.job_progress(self.id, percent(Stage::Convert, 1.0));
        let slides = self.render(pdf, ratio).await?;
        // `App::job_note` keeps the fallback note beside a later one, so "Imported with LibreOffice" is only for an
        // import that went to LibreOffice first.
        let mut notes = vec![if fell_back { FALLBACK_NOTE } else { strings::IMPORTED_WITH_LIBREOFFICE }.to_string()];
        notes.extend(count_note(count, slides.len()));
        Ok((slides, notes))
    }

    /// PowerPoint's own pictures of the slides, in `work/ppt/`.
    async fn powerpoint(&self) -> Result<Vec<PathBuf>, String> {
        let ppt = self.conv.powerpoint.as_ref().ok_or_else(|| strings::PPT_NOT_STARTED.to_string())?;
        #[cfg(windows)]
        {
            let (app, id) = (self.app, self.id);
            let progress = move |done: usize, total: usize| app.job_progress(id, percent(Stage::Convert, done as f32 / total.max(1) as f32));
            ppt.export(self.input, &self.work.join("ppt"), &self.waiting(strings::WAITING_POWERPOINT), &progress, self.cancel).await
        }
        #[cfg(not(windows))]
        {
            let _ = ppt;
            Err(strings::PPT_NOT_STARTED.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{Job, test_app},
        platform::tools::{OS, ToolEnv, find_soffice_in},
        import::sniff::Variant,
    };
    use std::time::{Duration, Instant};

    fn ooxml() -> Kind {
        Kind::Ooxml { variant: Variant::Pptx, slide_size: Some((12_192_000, 6_858_000)), slides: Some(3), hidden: 1 }
    }

    #[test]
    fn route_table() {
        use Lo::*;
        let snap = || Err(strings::LO_SNAP.to_string());
        for ppt in [false, true] {
            for lo in [None, Snap, Ok] {
                let at = format!("ppt {ppt}, {lo:?}");
                assert_eq!(route(&Kind::Encrypted, ppt, lo), Err(ENCRYPTED.to_string()), "{at}");
                assert_eq!(route(&Kind::NotPresentation, ppt, lo), Err(NOT_PRESENTATION.to_string()), "{at}");
                assert_eq!(route(&Kind::Pdf, ppt, lo), std::result::Result::Ok(Route::Pdf), "{at}");
                let libreoffice = match lo {
                    None => std::result::Result::Ok(Route::Missing),
                    Snap => snap(),
                    Ok => std::result::Result::Ok(Route::LibreOffice),
                };
                assert_eq!(route(&Kind::Keynote, ppt, lo), libreoffice, "{at}: PowerPoint cannot open Keynote");
                for kind in [ooxml(), Kind::Ppt, Kind::Odp] {
                    let want = if ppt { std::result::Result::Ok(Route::PowerPoint { fallback: lo == Ok }) } else { libreoffice.clone() };
                    assert_eq!(route(&kind, ppt, lo), want, "{at}: {kind:?}");
                }
            }
        }
    }

    #[test]
    fn expected_shape_and_visible_slides_come_from_ooxml_only() {
        let (ratio, count) = expected(&ooxml());
        assert!((ratio.unwrap() - 16.0 / 9.0).abs() < 0.001);
        assert_eq!(count, Some(2), "the hidden slide is not exported");
        let unknown = Kind::Ooxml { variant: Variant::Pptx, slide_size: Some((0, 0)), slides: None, hidden: 0 };
        assert_eq!(expected(&unknown), (None, None));
        assert_eq!(expected(&Kind::Odp), (None, None));
    }

    #[test]
    fn count_note_only_when_the_count_differs() {
        assert_eq!(count_note(Some(3), 3), None);
        assert_eq!(count_note(None, 3), None);
        assert_eq!(count_note(Some(5), 4).as_deref(), Some("5 slides were expected but 4 were imported."));
    }

    /// A folder of one test's own.
    fn folder() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rp-deck-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An ODP: by content a presentation that PowerPoint is not asked about here, so it goes to LibreOffice.
    fn odp(dir: &Path, name: &str) -> PathBuf {
        use std::io::Write;
        let path = dir.join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("mimetype", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
        zip.write_all(b"application/vnd.oasis.opendocument.presentation").unwrap();
        zip.finish().unwrap();
        path
    }

    fn nothing() -> FindSoffice {
        Box::new(|| None)
    }

    /// A test app whose converters are `lo` (found already) and `pdf`, with no PowerPoint.
    fn app_with(lo: Option<Arc<LibreOffice>>, pdf: Option<Arc<PdfRenderer>>) -> Arc<App> {
        let app = test_app();
        let conv = Converters::new(app.store.root.clone(), nothing(), pdf, None);
        *conv.libreoffice.lock().unwrap() = lo;
        app.set_converters(Arc::new(conv));
        app
    }

    fn job(app: &App, id: Uuid) -> Option<Job> {
        app.jobs.lock().unwrap().iter().find(|j| j.id == id).cloned()
    }

    /// Waits for the job to end: gone, failed, or done and kept for its note. Returns what is left of it.
    async fn ended(app: &App, id: Uuid) -> Option<Job> {
        let began = Instant::now();
        loop {
            match job(app, id) {
                Some(j) if j.error.is_none() && !j.done => {}
                left => return left,
            }
            assert!(began.elapsed() < Duration::from_secs(60), "the import never ended");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn tmp_is_empty(app: &App) -> bool {
        std::fs::read_dir(app.store.tmp()).map_or(true, |mut d| d.next().is_none())
    }

    fn marks(mark: &Path, word: &str) -> usize {
        std::fs::read_to_string(mark).unwrap_or_default().lines().filter(|l| l.starts_with(word)).count()
    }

    #[tokio::test]
    async fn not_presentation_adds_no_deck_and_ends_job_with_message() {
        let app = app_with(None, None);
        let file = folder().join("Talk.pptx");
        std::fs::write(&file, "just some text").unwrap();
        let id = start(app.clone(), file, "Talk".into());
        let left = ended(&app, id).await.unwrap();
        assert_eq!(left.error.as_deref(), Some(NOT_PRESENTATION));
        assert_eq!(app.jobs.lock().unwrap()[0].error.as_deref(), Some("This doesn't look like a PowerPoint file."));
        assert!(app.data.lock().unwrap().decks.is_empty());
        assert!(tmp_is_empty(&app));
    }

    #[tokio::test]
    async fn keynote_package_directory_gives_plain_message() {
        let app = app_with(None, None);
        let package = folder().join("Talk.key");
        std::fs::create_dir_all(package.join("Data")).unwrap();
        std::fs::write(package.join("Index.zip"), "x").unwrap();
        let id = start(app.clone(), package, "Talk".into());
        assert_eq!(ended(&app, id).await.unwrap().error.as_deref(), Some("Save the Keynote file as a single file and import again."));
        assert!(app.data.lock().unwrap().decks.is_empty());
    }

    #[tokio::test]
    async fn cancel_removes_job_and_work_dir() {
        let dir = folder();
        let mark = dir.join("mark");
        let app = test_app();
        let app = app_with(Some(LibreOffice::fake(&app.store.root, "tick:30000", &mark)), None);
        let id = start(app.clone(), odp(&dir, "Talk.odp"), "Talk".into());
        let began = Instant::now();
        while marks(&mark, "tick") == 0 {
            assert!(began.elapsed() < Duration::from_secs(30), "LibreOffice never started");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!tmp_is_empty(&app), "the work folder is there while it converts");
        tokio::time::sleep(Duration::from_millis(200)).await;
        app.job_cancel(id);
        assert!(ended(&app, id).await.is_none(), "a cancelled job leaves no card");
        assert!(began.elapsed() < Duration::from_secs(20));
        assert!(tmp_is_empty(&app));
        assert!(app.data.lock().unwrap().decks.is_empty());
        // The fake ticks every 50 ms while it lives.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let ticks = marks(&mark, "tick");
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(marks(&mark, "tick"), ticks, "LibreOffice is still running");
    }

    #[tokio::test]
    async fn waiting_label_shows_then_restores() {
        let dir = folder();
        let mark = dir.join("mark");
        let app = test_app();
        let app = app_with(Some(LibreOffice::fake(&app.store.root, "sleep:1000", &mark)), None);
        let first = start(app.clone(), odp(&dir, "a.odp"), "a".into());
        while marks(&mark, "start") == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let second = start(app.clone(), odp(&dir, "b.odp"), "b".into());
        let mut labels: Vec<String> = Vec::new();
        while let Some(j) = job(&app, second).filter(|j| j.error.is_none()) {
            if labels.last() != Some(&j.label) {
                labels.push(j.label.clone());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let waiting = labels.iter().position(|l| l == "Waiting for LibreOffice…").unwrap_or_else(|| panic!("{labels:?}"));
        assert_eq!(labels.get(waiting + 1).map(String::as_str), Some("Importing “b”"), "{labels:?}");
        // This fake writes no PDF, so both fail after converting, one after the other.
        for id in [first, second] {
            assert_eq!(ended(&app, id).await.unwrap().error.as_deref(), Some(strings::LO_CANNOT_READ));
        }
        assert_eq!(marks(&mark, "start"), 2);
    }

    #[tokio::test]
    async fn pdf_route_without_pdfium_gives_reinstall_message() {
        let app = app_with(None, None);
        let file = folder().join("Notices.pdf");
        std::fs::write(&file, "%PDF-1.7\n").unwrap();
        let id = start(app.clone(), file, "Notices".into());
        assert_eq!(ended(&app, id).await.unwrap().error.as_deref(), Some("The slide converter (pdfium) is missing. Reinstall RustyPresenter."));
        assert!(tmp_is_empty(&app));
    }

    #[tokio::test]
    async fn libreoffice_found_after_install_without_restart() {
        let dir = folder();
        let soffice = dir.join("soffice.exe");
        let app = test_app();
        // Only `RP_SOFFICE` points anywhere, so a LibreOffice on this computer cannot change the result.
        let (cache, path) = (Mutex::new(None), soffice.clone());
        let find: FindSoffice = Box::new(move || {
            let path = path.clone();
            let env = ToolEnv { os: OS, var: Box::new(move |k| (k == "RP_SOFFICE").then(|| path.clone().into())), home: None, path_dirs: vec![], registry: vec![], opt_dirs: vec![] };
            find_soffice_in(&cache, &env)
        });
        let conv = Arc::new(Converters::new(app.store.root.clone(), find, None, None));
        app.set_converters(conv.clone());
        assert_eq!(conv.status().libreoffice, Lo::None);

        let id = start(app.clone(), odp(&dir, "Talk.odp"), "Talk".into());
        let error = ended(&app, id).await.unwrap().error.unwrap();
        assert_eq!(error, strings::install_libreoffice(tools::install_hint(OS)));
        assert!(error.starts_with("To import presentations, install LibreOffice. Install LibreOffice"), "{error}");

        // "Installed": a file that is not a program, so LibreOffice is tried and cannot start.
        std::fs::write(&soffice, "").unwrap();
        let id = start(app.clone(), odp(&dir, "Talk.odp"), "Talk".into());
        assert_eq!(ended(&app, id).await.unwrap().error.as_deref(), Some(strings::LO_NOT_STARTED));
        assert_eq!(conv.status().libreoffice, Lo::Ok);
    }

    #[tokio::test]
    async fn import_staged_powerpoint_takes_over_the_staged_file() {
        let app = app_with(None, None);
        let staged = app.store.tmp().join(format!("{}.part", Uuid::new_v4()));
        std::fs::create_dir_all(app.store.tmp()).unwrap();
        std::fs::write(&staged, "not a deck").unwrap();
        let imported = app.import_staged(staged.clone(), "Talk.pptx", "sha", crate::app::ImportPurpose::PowerPoint).await.unwrap();
        let crate::app::Imported::Job(id) = imported else { panic!("{imported:?}") };
        assert_eq!(job(&app, id).map(|j| j.label).as_deref(), Some("Importing “Talk”"));
        assert_eq!(ended(&app, id).await.unwrap().error.as_deref(), Some(NOT_PRESENTATION));
        assert!(!staged.exists(), "the staged upload is removed");
        assert!(tmp_is_empty(&app));
    }

    /// Real pdfium: `RP_PDFIUM=<library>`. LibreOffice is the fake, handing over a three-page 16:9 PDF.
    fn pdfium_app(dir: &Path, steps: &str) -> Option<(Arc<App>, PathBuf)> {
        std::env::var_os("RP_PDFIUM")?;
        let app = test_app();
        let mark = dir.join("mark");
        let pdf = PdfRenderer::start(&app.store.root).unwrap();
        let conv = Converters::new(app.store.root.clone(), nothing(), Some(Arc::new(pdf)), None);
        *conv.libreoffice.lock().unwrap() = Some(LibreOffice::fake(&app.store.root, steps, &mark));
        app.set_converters(Arc::new(conv));
        Some((app, mark))
    }

    fn fixture_pdf(dir: &Path) -> PathBuf {
        let pdf = dir.join("fixture.pdf");
        std::fs::write(&pdf, crate::import::pdf::tests::three_page_pdf()).unwrap();
        pdf
    }

    #[tokio::test]
    async fn libreoffice_import_adds_note() {
        let dir = folder();
        let pdf = fixture_pdf(&dir);
        let Some((app, _)) = pdfium_app(&dir, &format!("copy:{}", pdf.display())) else { return };
        let id = start(app.clone(), odp(&dir, "Talk.odp"), "Talk".into());
        let left = ended(&app, id).await.expect("a job with a note stays");
        assert_eq!(left.error, None);
        assert!(left.done && left.percent == 100 && !left.cancellable);
        assert_eq!(left.note.as_deref(), Some("Imported with LibreOffice — check the slides."));
        let decks = app.data.lock().unwrap().decks.clone();
        assert_eq!(decks.len(), 1);
        assert_eq!((decks[0].title.as_str(), decks[0].slides.len()), ("Talk", 3));
        assert_eq!(left.result, Some(decks[0].id));
        let size = image::image_dimensions(app.store.asset_path(&decks[0].slides[0])).unwrap();
        assert_eq!(size, (1920, 1080));
        assert!(tmp_is_empty(&app));
    }

    #[tokio::test]
    async fn overlapping_imports_set_their_own_results() {
        let dir = folder();
        let pdf = fixture_pdf(&dir);
        let Some((app, _)) = pdfium_app(&dir, &format!("sleep:300,copy:{}", pdf.display())) else { return };
        let a = start(app.clone(), odp(&dir, "a.odp"), "Morning".into());
        let b = start(app.clone(), odp(&dir, "b.odp"), "Evening".into());
        let (ja, jb) = (ended(&app, a).await.unwrap(), ended(&app, b).await.unwrap());
        let decks = app.data.lock().unwrap().decks.clone();
        assert_eq!(decks.len(), 2, "each import adds one deck");
        let title = |r: Option<Uuid>| decks.iter().find(|d| Some(d.id) == r).map(|d| d.title.clone());
        assert_eq!(title(ja.result).as_deref(), Some("Morning"));
        assert_eq!(title(jb.result).as_deref(), Some("Evening"));
        assert_ne!(ja.result, jb.result);
    }
}
