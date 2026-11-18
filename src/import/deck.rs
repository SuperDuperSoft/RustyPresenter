//! A presentation to a deck of slide images (Plan 4 Task 6c), by whichever converter this computer has. Everything
//! happens in a `WorkDir`, and one `App::edit` adds the deck once every slide is in the store, so a failed or
//! cancelled import adds nothing.
use super::{
    CANCELLED, Stage, WorkDir, libreoffice::LibreOffice, notes, pdf::PdfRenderer, percent, powerpoint::PowerPoint,
    sniff::{self, encrypted, Kind, not_presentation},
    strings::{self, fallback_note, keynote_package, lo_snap, pdf_no_slides, pdfium_missing, slide_not_saved},
};
use crate::{
    app::App,
    model::{AssetId, Deck, DeckShow},
    platform::tools::{self, Soffice},
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The only list of presentation extensions.
pub const DECK_EXT: &[&str] = &["pptx", "ppt", "pps", "ppsx", "pot", "potx", "pptm", "odp", "key", "pdf"];

/// Whether LibreOffice can be used here. A Snap one is found, but cannot read our files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lo {
    None,
    Snap,
    Ok,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Pdf,
    /// `fallback`: LibreOffice can take over if PowerPoint fails.
    PowerPoint { fallback: bool },
    LibreOffice,
    Missing,
}

pub fn route(kind: &Kind, ppt: bool, lo: Lo) -> Result<Route, String> {
    let by_libreoffice = || match lo {
        Lo::Ok => Ok(Route::LibreOffice),
        Lo::Snap => Err(lo_snap()),
        Lo::None => Ok(Route::Missing),
    };
    match kind {
        Kind::Encrypted => Err(encrypted()),
        Kind::NotPresentation => Err(not_presentation()),
        Kind::Pdf => Ok(Route::Pdf),
        Kind::Keynote => by_libreoffice(),
        Kind::Ooxml { .. } | Kind::Ppt | Kind::Odp if ppt => Ok(Route::PowerPoint { fallback: lo == Lo::Ok }),
        Kind::Ooxml { .. } | Kind::Ppt | Kind::Odp => by_libreoffice(),
    }
}

/// What the Slides tab and About say about this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConverterStatus {
    pub powerpoint: bool,
    pub libreoffice: Lo,
    pub pdfium: bool,
}

/// What About shows: the status, and which LibreOffice it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConverterInfo {
    pub status: ConverterStatus,
    pub soffice: Option<PathBuf>,
}

type FindSoffice = Box<dyn Fn() -> Option<Soffice> + Send + Sync>;

pub struct Converters {
    pub libreoffice: Mutex<Option<Arc<LibreOffice>>>,
    /// `None` when pdfium would not load. Decks need it, but nothing else does.
    pub pdf: Option<Arc<PdfRenderer>>,
    pub powerpoint: Option<PowerPoint>,
    find: FindSoffice,
    data_dir: PathBuf,
}

impl Converters {
    pub fn new(find: FindSoffice, pdf: Option<Arc<PdfRenderer>>, powerpoint: Option<PowerPoint>, data_dir: PathBuf) -> Converters {
        Converters { libreoffice: Mutex::new(None), pdf, powerpoint, find, data_dir }
    }

    /// LibreOffice, looking for it again while it is missing: a volunteer may install it with the app running.
    pub fn libreoffice(&self) -> Option<Arc<LibreOffice>> {
        let mut found = self.libreoffice.lock().unwrap_or_else(|e| e.into_inner());
        if found.is_none() {
            *found = (self.find)().map(|soffice| LibreOffice::new(soffice, &self.data_dir));
        }
        found.clone()
    }

    pub fn status(&self) -> ConverterStatus {
        ConverterStatus { powerpoint: self.powerpoint.is_some(), libreoffice: self.lo(), pdfium: self.pdf.is_some() }
    }

    pub fn info(&self) -> ConverterInfo {
        ConverterInfo { status: self.status(), soffice: self.libreoffice().map(|lo| lo.path().to_path_buf()) }
    }

    fn lo(&self) -> Lo {
        match self.libreoffice() {
            None => Lo::None,
            Some(lo) if lo.is_snap() => Lo::Snap,
            Some(_) => Lo::Ok,
        }
    }
}

/// Start importing `file`, and return the job. `file` is the staged copy, which this import owns, or a folder (a
/// Keynote package), which is the operator's and stays.
pub fn start(app: Arc<App>, file: PathBuf, display: String) -> Uuid { start_as(app, file, display, false) }

/// `start`, and with `run` (1.2) the deck keeps its file and runs in PowerPoint on the audience screen.
pub fn start_as(app: Arc<App>, file: PathBuf, display: String, run: bool) -> Uuid {
    let label = strings::importing(&display);
    let (job, token) = app.job_start_cancellable(&label);
    tokio::spawn(async move {
        let import = Import { app: &app, job, label, cancel: token, run };
        let result = import.run(&file, display).await;
        // `run` moves it unless it stopped early. Never a folder: `remove_file` refuses one.
        let _ = std::fs::remove_file(&file);
        match result {
            Ok(()) => app.job_end(job, None),
            Err(e) if e == CANCELLED => app.job_end_cancelled(job),
            Err(e) => app.job_end(job, Some(e)),
        }
    });
    job
}

/// One import's way of talking to its job.
struct Import<'a> {
    app: &'a Arc<App>,
    job: Uuid,
    label: String,
    cancel: CancellationToken,
    /// Keep the file and run it in PowerPoint (1.2).
    run: bool,
}

/// What a conversion made, and what the operator should hear about it.
struct Made {
    slides: Vec<PathBuf>,
    note: Option<String>,
}

impl Import<'_> {
    async fn run(&self, file: &Path, display: String) -> Result<(), String> {
        if file.is_dir() {
            return Err(keynote_package());
        }
        let kind = sniff_off_thread(file.to_path_buf()).await?;
        let converters = self.app.converters();
        let (ppt, lo) = converters.as_ref().map_or((false, Lo::None), |c| (c.powerpoint.is_some(), c.lo()));
        let route = route(&kind, ppt, lo)?;
        if route == Route::Missing {
            return Err(strings::install_libreoffice(&tools::install_hint(tools::OS)));
        }
        let converters = converters.ok_or(pdfium_missing())?;
        let has_pdfium = converters.pdf.is_some();
        if !has_pdfium && !matches!(route, Route::PowerPoint { .. }) {
            return Err(pdfium_missing());
        }

        self.app.job_progress(self.job, percent(Stage::Prepare, 0.0));
        let work = WorkDir::new(&self.app.store.tmp()).map_err(|_| slide_not_saved())?;
        let input = work.path().join(format!("input.{}", sniff::staged_ext(&kind).ok_or(not_presentation())?));
        move_or_copy(file, &input).map_err(|_| slide_not_saved())?;
        self.app.job_progress(self.job, percent(Stage::Prepare, 1.0));
        let notes = notes_off_thread(&input, notes::format(&kind)).await;
        let (transitions, advance) = transitions_off_thread(&input, notes::format(&kind)).await;

        let made = match route {
            Route::Pdf => Made { slides: self.render(&converters, input.clone(), work.path(), None).await?, note: None },
            Route::LibreOffice => self.by_libreoffice(&converters, &kind, &input, work.path(), false).await?,
            Route::PowerPoint { fallback } => self.by_powerpoint(&converters, &kind, &input, work.path(), fallback && has_pdfium).await?,
            Route::Missing => unreachable!("answered above"),
        };
        if made.slides.is_empty() {
            return Err(pdf_no_slides());
        }
        let notes = notes::line_up(notes, made.slides.len());
        let transitions = super::transitions::line_up(transitions, made.slides.len());
        let advance_ms = crate::model::deck_advance(super::transitions::line_up(advance, made.slides.len()), made.slides.len());
        let slides = self.publish(made.slides).await?;
        let show = if self.run && runs_in_powerpoint(&kind) { Some(self.keep(&input, &kind).await?) } else { None };
        let id = Uuid::new_v4();
        self.app.edit(|d| { d.decks.push(Deck { id, title: display.clone(), slides: slides.clone(), notes: notes.clone(), transitions: transitions.clone(), advance_ms: advance_ms.clone(), show: show.clone() }); Ok(()) })?;
        self.app.job_result(self.job, id);
        if let Some(note) = made.note {
            self.app.job_note(self.job, note);
        }
        Ok(())
    }

    fn progress(&self, stage: Stage, frac: f32) {
        self.app.job_progress(self.job, percent(stage, frac));
    }

    /// The label while this import is behind another one, and back.
    fn waiting(&self, behind: String) -> impl Fn(bool) + Sync + '_ {
        move |waiting| self.app.job_label(self.job, if waiting { &behind } else { &self.label })
    }

    async fn by_powerpoint(&self, converters: &Converters, kind: &Kind, input: &Path, work: &Path, fallback: bool) -> Result<Made, String> {
        let powerpoint = converters.powerpoint.as_ref().ok_or(pdfium_missing())?;
        let waiting = self.waiting(strings::waiting_for_powerpoint());
        let progress = |done: usize, total: usize| self.progress(Stage::Convert, done as f32 / total.max(1) as f32);
        match powerpoint.export(input, &work.join("out"), &waiting, &progress, &self.cancel).await {
            Ok(slides) => Ok(Made { slides, note: None }),
            // A password stops LibreOffice too, and a cancel means stop.
            Err(e) if e == CANCELLED || e == encrypted() || !fallback => Err(e),
            Err(e) => {
                eprintln!("PowerPoint import failed, trying LibreOffice: {e}");
                self.app.job_note(self.job, fallback_note());
                self.by_libreoffice(converters, kind, input, work, true).await
            }
        }
    }

    async fn by_libreoffice(&self, converters: &Converters, kind: &Kind, input: &Path, work: &Path, fell_back: bool) -> Result<Made, String> {
        let lo = converters.libreoffice().ok_or(pdfium_missing())?;
        let waiting = self.waiting(strings::waiting_for_libreoffice());
        self.progress(Stage::Convert, 0.0);
        let pdf = lo.to_pdf(input, work, &waiting, &self.cancel).await?;
        self.progress(Stage::Convert, 1.0);
        let slides = self.render(converters, pdf, work, expected_ratio(kind)).await?;
        let expected = expected_slides(kind).filter(|n| *n != slides.len());
        // After a fallback the job already says which program made these.
        let note = match (fell_back, expected) {
            (true, n) => n.map(|n| strings::slide_count_differs(n, slides.len())),
            (false, None) => Some(strings::libreoffice_note()),
            (false, Some(n)) => Some(strings::libreoffice_note_slide_count(n, slides.len())),
        };
        Ok(Made { slides, note })
    }

    async fn render(&self, converters: &Converters, pdf: PathBuf, work: &Path, ratio: Option<f32>) -> Result<Vec<PathBuf>, String> {
        let renderer = converters.pdf.as_ref().ok_or(pdfium_missing())?;
        let out = work.join("slides");
        std::fs::create_dir_all(&out).map_err(|_| slide_not_saved())?;
        let (app, job) = (self.app.clone(), self.job);
        let progress = Box::new(move |done: usize, total: usize| app.job_progress(job, percent(Stage::Render, done as f32 / total.max(1) as f32)));
        renderer.render(pdf, out, ratio, progress, self.cancel.clone()).await
    }

    /// The presentation itself into the store, to run in PowerPoint.
    async fn keep(&self, input: &Path, kind: &Kind) -> Result<DeckShow, String> {
        let (app, input, ext) = (self.app.clone(), input.to_path_buf(), sniff::staged_ext(kind).ok_or(not_presentation())?);
        let file = tokio::task::spawn_blocking(move || app.store.import_file(&input, ext)).await.map_err(|_| slide_not_saved())?.map_err(|_| slide_not_saved())?;
        if self.cancel.is_cancelled() { return Err(CANCELLED.to_string()) }
        Ok(DeckShow { file, run: true })
    }

    /// Off the async threads: a big deck is a lot of hashing.
    async fn publish(&self, slides: Vec<PathBuf>) -> Result<Vec<AssetId>, String> {
        let (app, job, cancel) = (self.app.clone(), self.job, self.cancel.clone());
        tokio::task::spawn_blocking(move || {
            let mut assets = Vec::with_capacity(slides.len());
            for (i, slide) in slides.iter().enumerate() {
                if cancel.is_cancelled() {
                    return Err(CANCELLED.to_string());
                }
                assets.push(app.store.import_file(slide, "png").map_err(|_| slide_not_saved())?);
                app.job_progress(job, percent(Stage::Publish, (i + 1) as f32 / slides.len() as f32));
            }
            if cancel.is_cancelled() { Err(CANCELLED.to_string()) } else { Ok(assets) }
        }).await.map_err(|_| slide_not_saved())?
    }
}

/// The speaker notes, read before the converter sees the file. `None` when there are none to read.
async fn notes_off_thread(file: &Path, format: Option<notes::Format>) -> Option<Vec<String>> {
    let (file, format) = (file.to_path_buf(), format?);
    tokio::task::spawn_blocking(move || notes::read(&file, format)).await.ok().flatten()
}

/// Each slide's transition, read like the notes.
/// Each slide's transition and auto-advance time, read from the file.
#[allow(clippy::type_complexity)]
async fn transitions_off_thread(file: &Path, format: Option<notes::Format>) -> (Option<Vec<Option<crate::design::Transition>>>, Option<Vec<Option<u32>>>) {
    let Some(format) = format else { return (None, None) };
    let file = file.to_path_buf();
    tokio::task::spawn_blocking(move || (super::transitions::read(&file, format), super::transitions::read_advance(&file, format))).await.unwrap_or((None, None))
}

async fn sniff_off_thread(file: PathBuf) -> Result<Kind, String> {
    let read = tokio::task::spawn_blocking(move || sniff::sniff(&file)).await;
    match read {
        Ok(Ok(kind)) => Ok(kind),
        Ok(Err(e)) => {
            eprintln!("Could not read the presentation: {e}");
            Err(not_presentation())
        }
        Err(e) => {
            eprintln!("Could not read the presentation: {e}");
            Err(not_presentation())
        }
    }
}

/// PowerPoint can run a slide show of these (not a PDF or a Keynote file).
pub fn runs_in_powerpoint(kind: &Kind) -> bool { matches!(kind, Kind::Ooxml { .. } | Kind::Ppt | Kind::Odp) }

/// Staged files are already in `tmp`, so this is a rename unless something odd is going on.
fn move_or_copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to).or_else(|_| std::fs::copy(from, to).map(|_| ()))
}

/// The shape PowerPoint says the slides have, so a PDF of another shape is noticed.
fn expected_ratio(kind: &Kind) -> Option<f32> {
    match kind {
        Kind::Ooxml { slide_size: Some((cx, cy)), .. } if *cy > 0 => Some(*cx as f32 / *cy as f32),
        _ => None,
    }
}

/// The slides that are not hidden. `None` when the file did not say.
fn expected_slides(kind: &Kind) -> Option<usize> {
    match kind {
        Kind::Ooxml { slides: Some(n), hidden, .. } => Some(n.saturating_sub(*hidden)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "deck_tests.rs"]
mod tests;
