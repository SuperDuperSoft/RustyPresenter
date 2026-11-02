//! PDF pages to slide images (Plan 4 Task 4). pdfium is not thread-safe, so one dedicated thread owns it and imports
//! take turns. A pdfium call cannot be interrupted, so a page that never finishes ends this renderer for good.
use super::CANCELLED;
use super::strings as s;
use crate::platform::tools;
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use pdfium_render::prelude::*;
use std::io::{BufWriter, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc as async_mpsc};
use tokio_util::sync::CancellationToken;

const WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 2160;
const PAGE_TIMEOUT: Duration = Duration::from_secs(5);
/// Opening is not per page, and a false alarm costs a restart, so it gets longer than a page.
const OPEN_TIMEOUT_PAGES: u32 = 6;
/// Pages this far apart count as the same size (1%).
const TOLERANCE: f32 = 0.01;
/// Page ratios of A4 and Letter, upright and sideways. LibreOffice opens a file it cannot read as a text document,
/// and a real slide deck is almost never this shape (check C5, gotcha 2).
const PAPER_RATIOS: [f32; 4] = [0.707, 0.773, 1.414, 1.294];

/// The image size for a slide ratio, the same for every import path.
pub fn output_size(ratio: f32) -> (u32, u32) {
    let even = |v: f32| ((v / 2.0).round() as u32 * 2).max(2);
    let height = even(WIDTH as f32 / ratio);
    if height <= MAX_HEIGHT { (WIDTH, height) } else { (even(MAX_HEIGHT as f32 * ratio), MAX_HEIGHT) }
}

fn close(a: f32, b: f32) -> bool {
    (a / b - 1.0).abs() <= TOLERANCE
}

/// The slide ratio of these page sizes, or why they cannot be slides.
pub fn check_pages(sizes: &[(f32, f32)], expected_ratio: Option<f32>) -> Result<f32, String> {
    let Some(&(w, h)) = sizes.first() else { return Err(s::NO_SLIDES.into()) };
    if sizes.iter().any(|&(w, h)| !(w > 0.0 && h > 0.0)) {
        return Err(s::LO_UNREADABLE.into());
    }
    let ratio = w / h;
    if sizes.iter().any(|&(w, h)| !close(w / h, ratio)) {
        return Err(s::MIXED_SIZES.into());
    }
    let wrong = match expected_ratio {
        Some(expected) => !close(ratio, expected),
        None => ratio < 1.0 || PAPER_RATIOS.iter().any(|&paper| close(ratio, paper)),
    };
    if wrong { Err(s::LO_UNREADABLE.into()) } else { Ok(ratio) }
}

/// The file exists and is a PDF. A converter's exit code does not prove that.
pub fn check_pdf(path: &Path) -> Result<(), String> {
    let mut magic = [0u8; 5];
    let is_pdf = std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && &magic == b"%PDF-";
    if is_pdf { Ok(()) } else { Err(s::LO_UNREADABLE.into()) }
}

/// One open document, used only on the renderer's thread.
trait Doc {
    fn sizes(&self) -> Vec<(f32, f32)>;
    fn render(&mut self, page: usize, size: (u32, u32), out: &Path) -> Result<(), String>;
}

/// `work` runs while the document is open, because pdfium's document borrows the library.
trait Backend {
    fn open(&mut self, pdf: &Path, work: &mut dyn FnMut(&mut dyn Doc) -> Result<(), String>) -> Result<(), String>;
}

struct PdfiumBackend(Pdfium);

struct PdfiumDoc<'a>(PdfDocument<'a>);

impl Backend for PdfiumBackend {
    fn open(&mut self, pdf: &Path, work: &mut dyn FnMut(&mut dyn Doc) -> Result<(), String>) -> Result<(), String> {
        let doc = self.0.load_pdf_from_file(pdf, None).map_err(|_| s::PDF_UNREADABLE.to_string())?;
        work(&mut PdfiumDoc(doc))
    }
}

impl Doc for PdfiumDoc<'_> {
    fn sizes(&self) -> Vec<(f32, f32)> {
        self.0.pages().iter().map(|p| (p.width().value, p.height().value)).collect()
    }

    fn render(&mut self, page: usize, (width, height): (u32, u32), out: &Path) -> Result<(), String> {
        let page = u16::try_from(page).ok().and_then(|i| self.0.pages().get(i).ok()).ok_or(s::PDF_UNREADABLE)?;
        // An exact size, because pages may differ by up to 1% and every slide image must match.
        let config = PdfRenderConfig::new().set_target_size(width as i32, height as i32).render_form_data(true).set_clear_color(PdfColor::WHITE).use_lcd_text_rendering(false);
        let bitmap = page.render_with_config(&config).map_err(|_| s::PDF_UNREADABLE.to_string())?;
        let image = bitmap.as_image().to_rgb8();
        let file = std::fs::File::create(out).map_err(|_| s::SLIDE_NOT_SAVED.to_string())?;
        let png = PngEncoder::new_with_quality(BufWriter::new(file), CompressionType::Fast, FilterType::Adaptive);
        png.write_image(image.as_raw(), width, height, image::ExtendedColorType::Rgb8).map_err(|_| s::SLIDE_NOT_SAVED.to_string())
    }
}

fn bind(candidates: &[PathBuf]) -> Result<PdfiumBackend, String> {
    let bindings = candidates.iter().find_map(|path| Pdfium::bind_to_library(path).ok()).map(Ok).unwrap_or_else(Pdfium::bind_to_system_library);
    bindings.map(|b| PdfiumBackend(Pdfium::new(b))).map_err(|_| s::PDFIUM_MISSING.to_string())
}

struct Job {
    pdf: PathBuf,
    out_dir: PathBuf,
    expected_ratio: Option<f32>,
    cancel: CancellationToken,
    events: async_mpsc::UnboundedSender<Event>,
}

enum Event {
    Opened(usize),
    Page(PathBuf),
    Done(Result<(), String>),
}

fn run_job(backend: &mut impl Backend, job: &Job) -> Result<(), String> {
    std::fs::create_dir_all(&job.out_dir).map_err(|_| s::SLIDE_NOT_SAVED.to_string())?;
    backend.open(&job.pdf, &mut |doc| {
        let sizes = doc.sizes();
        let size = output_size(check_pages(&sizes, job.expected_ratio)?);
        let _ = job.events.send(Event::Opened(sizes.len()));
        for page in 0..sizes.len() {
            if job.cancel.is_cancelled() {
                return Err(CANCELLED.into());
            }
            let out = job.out_dir.join(format!("slide-{:03}.png", page + 1));
            doc.render(page, size, &out)?;
            let _ = job.events.send(Event::Page(out));
        }
        Ok(())
    })
}

/// Renders PDFs on its one thread. After a stuck page it refuses all work until the app restarts.
pub struct PdfRenderer {
    tx: mpsc::Sender<Job>,
    /// One import at a time, so waiting behind another import never counts against the page timeout.
    turn: Semaphore,
    dead: AtomicBool,
    page_timeout: Duration,
}

impl PdfRenderer {
    pub fn start(data_dir: &Path) -> Result<PdfRenderer, String> {
        let candidates = tools::pdfium_candidates(data_dir);
        Self::spawn(move || bind(&candidates), PAGE_TIMEOUT)
    }

    fn spawn<B: Backend>(make: impl FnOnce() -> Result<B, String> + Send + 'static, page_timeout: Duration) -> Result<PdfRenderer, String> {
        let (tx, jobs) = mpsc::channel::<Job>();
        let (ready_tx, ready) = mpsc::channel();
        let thread = std::thread::Builder::new().name("pdfium".into()).spawn(move || {
            let mut backend = match make() {
                Ok(backend) => {
                    let _ = ready_tx.send(Ok(()));
                    backend
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            for job in jobs {
                let result = run_job(&mut backend, &job);
                let _ = job.events.send(Event::Done(result));
            }
        });
        thread.map_err(|_| s::PDFIUM_MISSING.to_string())?;
        ready.recv().unwrap_or_else(|_| Err(s::PDFIUM_MISSING.to_string()))?;
        Ok(PdfRenderer { tx, turn: Semaphore::new(1), dead: AtomicBool::new(false), page_timeout })
    }

    fn stopped(&self) -> String {
        self.dead.store(true, Ordering::SeqCst);
        s::PDFIUM_STOPPED.into()
    }

    fn check_alive(&self) -> Result<(), String> {
        if self.dead.load(Ordering::SeqCst) { Err(s::PDFIUM_STOPPED.into()) } else { Ok(()) }
    }

    /// Writes `slide-001.png`… into `out_dir`, in order. `progress` gets (pages done, pages in all). Cancel takes
    /// effect between pages: this waits for the thread, so the next import never meets a thread still busy with this one.
    pub async fn render(&self, pdf: PathBuf, out_dir: PathBuf, expected_ratio: Option<f32>, progress: Box<dyn Fn(usize, usize) + Send>, cancel: CancellationToken) -> Result<Vec<PathBuf>, String> {
        self.check_alive()?;
        let _turn = tokio::select! {
            turn = self.turn.acquire() => turn.map_err(|_| self.stopped())?,
            _ = cancel.cancelled() => return Err(CANCELLED.into()),
        };
        self.check_alive()?;
        let (events, mut rx) = async_mpsc::unbounded_channel();
        self.tx.send(Job { pdf, out_dir, expected_ratio, cancel, events }).map_err(|_| self.stopped())?;
        let mut wait = self.page_timeout * OPEN_TIMEOUT_PAGES;
        let (mut total, mut files) = (0, Vec::new());
        loop {
            match tokio::time::timeout(wait, rx.recv()).await {
                Ok(Some(Event::Opened(pages))) => {
                    total = pages;
                    wait = self.page_timeout;
                }
                Ok(Some(Event::Page(file))) => {
                    files.push(file);
                    progress(files.len(), total);
                }
                Ok(Some(Event::Done(result))) => return result.map(|()| files),
                // A timeout, or the thread died.
                Err(_) | Ok(None) => return Err(self.stopped()),
            }
        }
    }
}

#[cfg(test)]
mod tests;
