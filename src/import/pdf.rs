//! PDF pages to slide images (Plan 4 Task 4). pdfium is not thread-safe, so one thread does all of it.
use super::{CANCELLED, strings::*};
use crate::platform::tools;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use pdfium_render::prelude::{PdfColor, PdfDocument, PdfRenderConfig, Pdfium};
use std::{fs::File, io::{BufWriter, Read}, path::{Path, PathBuf}, sync::atomic::{AtomicBool, Ordering}, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const PAGE_TIMEOUT: Duration = Duration::from_secs(5);
const WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 2160;
const RATIO_SLACK: f32 = 0.01;
/// A4 and Letter, both ways round: LibreOffice's page when it did not understand the slides.
const PAPER_RATIOS: [f32; 4] = [0.707, 0.773, 1.414, 1.294];

/// One open PDF.
pub trait PdfDoc {
    /// Width and height of each page, in points.
    fn page_sizes(&self) -> Vec<(f32, f32)>;
    fn render_page(&self, index: usize, size: (u32, u32), out: &Path) -> Result<(), String>;
}

/// What opens PDFs. It stays on the `pdfium` thread.
pub trait PdfBackend {
    fn open<'a>(&'a mut self, pdf: &Path) -> Result<Box<dyn PdfDoc + 'a>, String>;
}

/// The slide image size for pages of this shape: the global rule of Plan 4.
pub fn output_size(ratio: f32) -> (u32, u32) {
    let even = |n: f32| (n / 2.0).round() as u32 * 2;
    let height = even(WIDTH as f32 / ratio);
    if height <= MAX_HEIGHT { (WIDTH, height) } else { (((MAX_HEIGHT as f32) * ratio).round() as u32, MAX_HEIGHT) }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= b * RATIO_SLACK
}

/// The ratio of a PDF's pages, or why they cannot be used.
pub fn check_pages(sizes: &[(f32, f32)], expected_ratio: Option<f32>) -> Result<f32, String> {
    let cannot_read = || Err(LO_CANNOT_READ.to_string());
    let Some(first) = sizes.first() else { return Err(PDF_NO_SLIDES.to_string()) };
    if sizes.iter().any(|(w, h)| *w <= 0.0 || *h <= 0.0) {
        return cannot_read();
    }
    let ratio = first.0 / first.1;
    if sizes.iter().any(|(w, h)| !near(w / h, ratio)) {
        return Err(PDF_MIXED_SIZES.to_string());
    }
    let good = match expected_ratio {
        Some(expected) => near(ratio, expected),
        None => ratio >= 1.0 && !PAPER_RATIOS.iter().any(|paper| near(ratio, *paper)),
    };
    if good { Ok(ratio) } else { cannot_read() }
}

struct Job {
    pdf: PathBuf,
    out_dir: PathBuf,
    expected_ratio: Option<f32>,
    cancel: CancellationToken,
    reply: mpsc::UnboundedSender<Reply>,
}

enum Reply {
    /// The `pdfium` thread reached this job: the time limit starts now, not while it waits behind another job.
    Begun,
    Page { done: usize, total: usize },
    Done(Result<Vec<PathBuf>, String>),
}

pub struct PdfRenderer {
    jobs: std::sync::mpsc::Sender<Job>,
    /// A pdfium call cannot be interrupted, so a stuck thread stays stuck until the app restarts.
    dead: AtomicBool,
    page_timeout: Duration,
}

impl PdfRenderer {
    pub fn start(data_dir: &Path) -> Result<PdfRenderer, String> {
        let candidates = tools::pdfium_candidates(data_dir);
        let (bound, ready) = std::sync::mpsc::channel();
        let (jobs, queue) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new().name("pdfium".to_string()).spawn(move || match bind(&candidates) {
            Ok(pdfium) => {
                let _ = bound.send(Ok(()));
                serve(pdfium, queue);
            }
            Err(e) => {
                let _ = bound.send(Err(e));
            }
        });
        thread.map_err(|_| PDFIUM_MISSING.to_string())?;
        ready.recv().map_err(|_| PDFIUM_MISSING.to_string())??;
        Ok(PdfRenderer { jobs, dead: AtomicBool::new(false), page_timeout: PAGE_TIMEOUT })
    }

    /// Runs `backend` on the renderer's thread, with this time limit per page. For fakes.
    pub fn with_backend(backend: Box<dyn PdfBackend + Send>, page_timeout: Duration) -> PdfRenderer {
        let (jobs, queue) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new().name("pdfium".to_string()).spawn(move || serve(backend, queue));
        drop(thread);
        PdfRenderer { jobs, dead: AtomicBool::new(false), page_timeout }
    }

    fn stuck(&self) -> Result<Vec<PathBuf>, String> {
        self.dead.store(true, Ordering::Relaxed);
        Err(PDFIUM_STUCK.to_string())
    }

    /// Renders every page to `out_dir/slide-001.png`…, in order. `progress(done, total)` follows each page.
    pub async fn render(&self, pdf: PathBuf, out_dir: PathBuf, expected_ratio: Option<f32>, progress: Box<dyn Fn(usize, usize) + Send>, cancel: CancellationToken) -> Result<Vec<PathBuf>, String> {
        if self.dead.load(Ordering::Relaxed) {
            return Err(PDFIUM_STUCK.to_string());
        }
        let (reply, mut replies) = mpsc::unbounded_channel();
        if self.jobs.send(Job { pdf, out_dir, expected_ratio, cancel, reply }).is_err() {
            return self.stuck();
        }
        let mut begun = false;
        loop {
            match tokio::time::timeout(self.page_timeout, replies.recv()).await {
                Ok(Some(Reply::Begun)) => begun = true,
                Ok(Some(Reply::Page { done, total })) => progress(done, total),
                Ok(Some(Reply::Done(result))) => return result,
                Ok(None) => return self.stuck(),
                Err(_) if !begun && !self.dead.load(Ordering::Relaxed) => {}
                Err(_) => return self.stuck(),
            }
        }
    }
}

fn serve(mut backend: Box<dyn PdfBackend>, queue: std::sync::mpsc::Receiver<Job>) {
    for job in queue {
        let _ = job.reply.send(Reply::Begun);
        let result = render_job(backend.as_mut(), &job);
        let _ = job.reply.send(Reply::Done(result));
    }
}

fn render_job(backend: &mut dyn PdfBackend, job: &Job) -> Result<Vec<PathBuf>, String> {
    let doc = backend.open(&job.pdf)?;
    let sizes = doc.page_sizes();
    let size = output_size(check_pages(&sizes, job.expected_ratio)?);
    std::fs::create_dir_all(&job.out_dir).map_err(|_| PDF_CANNOT_OPEN.to_string())?;
    let mut pages = Vec::with_capacity(sizes.len());
    for index in 0..sizes.len() {
        if job.cancel.is_cancelled() {
            return Err(CANCELLED.to_string());
        }
        let out = job.out_dir.join(format!("slide-{:03}.png", index + 1));
        doc.render_page(index, size, &out).map_err(|e| {
            eprintln!("pdfium: page {}: {e}", index + 1);
            pdf_page_failed(index + 1)
        })?;
        pages.push(out);
        // Nobody is listening once the renderer gave up on this job, so stop writing into its folder.
        if job.reply.send(Reply::Page { done: index + 1, total: sizes.len() }).is_err() {
            return Err(CANCELLED.to_string());
        }
    }
    if job.cancel.is_cancelled() { Err(CANCELLED.to_string()) } else { Ok(pages) }
}

fn bind(candidates: &[PathBuf]) -> Result<Box<dyn PdfBackend>, String> {
    let bindings = candidates.iter().find_map(|path| Pdfium::bind_to_library(path).ok()).map_or_else(Pdfium::bind_to_system_library, Ok);
    let bindings = bindings.map_err(|e| {
        eprintln!("pdfium: {e}");
        PDFIUM_MISSING.to_string()
    })?;
    Ok(Box::new(Pdfium::new(bindings)))
}

impl PdfBackend for Pdfium {
    fn open<'a>(&'a mut self, pdf: &Path) -> Result<Box<dyn PdfDoc + 'a>, String> {
        check_pdf(pdf)?;
        let doc = self.load_pdf_from_file(pdf, None).map_err(|e| {
            eprintln!("pdfium: {e}");
            PDF_CANNOT_OPEN.to_string()
        })?;
        Ok(Box::new(doc))
    }
}

impl PdfDoc for PdfDocument<'_> {
    fn page_sizes(&self) -> Vec<(f32, f32)> {
        self.pages().iter().map(|page| (page.width().value, page.height().value)).collect()
    }

    fn render_page(&self, index: usize, size: (u32, u32), out: &Path) -> Result<(), String> {
        let config = PdfRenderConfig::new().set_target_width(size.0 as i32).set_maximum_height(size.1 as i32).render_form_data(true).set_clear_color(PdfColor::WHITE).use_lcd_text_rendering(false);
        let page = self.pages().get(index as _).map_err(|e| e.to_string())?;
        let image = page.render_with_config(&config).map_err(|e| e.to_string())?.as_image().to_rgb8();
        let file = BufWriter::new(File::create(out).map_err(|e| e.to_string())?);
        let encoder = PngEncoder::new_with_quality(file, CompressionType::Fast, FilterType::Adaptive);
        image.write_with_encoder(encoder).map_err(|e| e.to_string())
    }
}

/// The file exists and starts like a PDF.
pub fn check_pdf(path: &Path) -> Result<(), String> {
    let mut start = [0u8; 5];
    let read = File::open(path).and_then(|mut f| f.read_exact(&mut start));
    if read.is_ok() && start == *b"%PDF-" { Ok(()) } else { Err(NOT_PDF.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::strings;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    const W: (f32, f32) = (960.0, 540.0);

    #[test]
    fn output_size_16x9_is_1920x1080() {
        assert_eq!(output_size(16.0 / 9.0), (1920, 1080));
    }

    #[test]
    fn output_size_4x3_is_1920x1440() {
        assert_eq!(output_size(4.0 / 3.0), (1920, 1440));
    }

    #[test]
    fn output_size_tall_caps_height_and_scales_width() {
        assert_eq!(output_size(0.5), (1080, 2160));
        assert_eq!(output_size(1.0), (1920, 1920));
        assert_eq!(output_size(0.8), (1728, 2160));
    }

    #[test]
    fn output_size_heights_are_even() {
        for ratio in [1.3, 1.5, 1.6180339, 2.0, 2.39, 1.91, 1.05, 3.1] {
            assert_eq!(output_size(ratio).1 % 2, 0, "{ratio}");
        }
    }

    #[test]
    fn no_pages_rejected() {
        assert_eq!(check_pages(&[], None), Err(strings::PDF_NO_SLIDES.to_string()));
    }

    #[test]
    fn zero_sized_page_rejected() {
        assert_eq!(check_pages(&[(0.0, 0.0)], None), Err(strings::LO_CANNOT_READ.to_string()));
    }

    #[test]
    fn mixed_page_sizes_rejected() {
        assert_eq!(check_pages(&[W, (800.0, 600.0)], None), Err(strings::PDF_MIXED_SIZES.to_string()));
        assert_eq!(check_pages(&[W, (960.0, 545.0)], None), Ok(960.0 / 540.0), "under 1% apart");
    }

    #[test]
    fn a4_without_expected_ratio_rejected() {
        for size in [(842.0, 595.0), (792.0, 612.0)] {
            assert_eq!(check_pages(&[size], None), Err(strings::LO_CANNOT_READ.to_string()), "{size:?}");
        }
    }

    #[test]
    fn portrait_rejected() {
        assert_eq!(check_pages(&[(595.0, 842.0)], None), Err(strings::LO_CANNOT_READ.to_string()));
        assert_eq!(check_pages(&[(540.0, 960.0)], None), Err(strings::LO_CANNOT_READ.to_string()));
    }

    #[test]
    fn ratio_must_match_expected() {
        assert_eq!(check_pages(&[W], Some(4.0 / 3.0)), Err(strings::LO_CANNOT_READ.to_string()));
        assert_eq!(check_pages(&[W], Some(16.0 / 9.0)), Ok(960.0 / 540.0));
        assert_eq!(check_pages(&[(842.0, 595.0)], Some(842.0 / 595.0)), Ok(842.0 / 595.0), "an A4 deck was asked for");
    }

    struct Fake {
        sizes: Vec<(f32, f32)>,
        stuck_on: Option<usize>,
        fail_on: Option<usize>,
        cancel_after: Option<(usize, CancellationToken)>,
        rendered: Arc<Mutex<Vec<(usize, (u32, u32))>>>,
    }

    impl Fake {
        fn new(pages: usize) -> Fake {
            Fake { sizes: vec![W; pages], stuck_on: None, fail_on: None, cancel_after: None, rendered: Arc::default() }
        }
    }

    struct FakeDoc<'a>(&'a Fake);

    impl PdfBackend for Fake {
        fn open<'a>(&'a mut self, pdf: &Path) -> Result<Box<dyn PdfDoc + 'a>, String> {
            check_pdf(pdf)?;
            Ok(Box::new(FakeDoc(self)))
        }
    }

    impl PdfDoc for FakeDoc<'_> {
        fn page_sizes(&self) -> Vec<(f32, f32)> {
            self.0.sizes.clone()
        }

        fn render_page(&self, index: usize, size: (u32, u32), out: &Path) -> Result<(), String> {
            if self.0.stuck_on == Some(index) {
                std::thread::sleep(Duration::from_secs(60));
            }
            if self.0.fail_on == Some(index) {
                return Err("bad page".to_string());
            }
            self.0.rendered.lock().unwrap().push((index, size));
            std::fs::write(out, "png").unwrap();
            if let Some((after, token)) = &self.0.cancel_after && *after == index {
                token.cancel();
            }
            Ok(())
        }
    }

    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Dir {
            let dir = std::env::temp_dir().join(format!("rp-pdf-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("in.pdf"), "%PDF-1.7").unwrap();
            Dir(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn run(r: &PdfRenderer, dir: &Dir, cancel: CancellationToken) -> Result<Vec<PathBuf>, String> {
        r.render(dir.0.join("in.pdf"), dir.0.clone(), None, Box::new(|_, _| ()), cancel).await
    }

    #[tokio::test]
    async fn renders_every_page_in_order_with_progress() {
        let dir = Dir::new();
        let fake = Fake::new(3);
        let rendered = fake.rendered.clone();
        let r = PdfRenderer::with_backend(Box::new(fake), Duration::from_secs(5));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let note = seen.clone();
        let pages = r.render(dir.0.join("in.pdf"), dir.0.clone(), Some(16.0 / 9.0), Box::new(move |d, t| note.lock().unwrap().push((d, t))), CancellationToken::new()).await.unwrap();
        assert_eq!(pages, ["slide-001.png", "slide-002.png", "slide-003.png"].map(|n| dir.0.join(n)));
        assert!(pages.iter().all(|p| p.exists()));
        assert_eq!(*seen.lock().unwrap(), vec![(1, 3), (2, 3), (3, 3)]);
        assert_eq!(*rendered.lock().unwrap(), vec![(0, (1920, 1080)), (1, (1920, 1080)), (2, (1920, 1080))]);
    }

    #[tokio::test]
    async fn bad_pages_render_nothing() {
        let dir = Dir::new();
        let mut fake = Fake::new(2);
        fake.sizes[1] = (800.0, 600.0);
        let rendered = fake.rendered.clone();
        let r = PdfRenderer::with_backend(Box::new(fake), Duration::from_secs(5));
        assert_eq!(run(&r, &dir, CancellationToken::new()).await, Err(strings::PDF_MIXED_SIZES.to_string()));
        assert!(rendered.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failed_page_names_its_slide() {
        let dir = Dir::new();
        let mut fake = Fake::new(3);
        fake.fail_on = Some(1);
        let r = PdfRenderer::with_backend(Box::new(fake), Duration::from_secs(5));
        assert_eq!(run(&r, &dir, CancellationToken::new()).await, Err(strings::pdf_page_failed(2)));
        assert_eq!(run(&r, &dir, CancellationToken::new()).await, Err(strings::pdf_page_failed(2)), "the renderer still works");
    }

    #[tokio::test]
    async fn a_file_that_will_not_open_reports_the_reason() {
        let dir = Dir::new();
        std::fs::write(dir.0.join("in.pdf"), "nope").unwrap();
        let r = PdfRenderer::with_backend(Box::new(Fake::new(1)), Duration::from_secs(5));
        assert_eq!(run(&r, &dir, CancellationToken::new()).await, Err(NOT_PDF.to_string()));
    }

    #[tokio::test]
    async fn cancel_stops_between_pages() {
        let dir = Dir::new();
        let cancel = CancellationToken::new();
        let mut fake = Fake::new(3);
        fake.cancel_after = Some((0, cancel.clone()));
        let rendered = fake.rendered.clone();
        let r = PdfRenderer::with_backend(Box::new(fake), Duration::from_secs(5));
        assert_eq!(run(&r, &dir, cancel).await, Err(CANCELLED.to_string()));
        assert_eq!(rendered.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn cancel_after_the_last_page_still_cancels() {
        let dir = Dir::new();
        let cancel = CancellationToken::new();
        let mut fake = Fake::new(1);
        fake.cancel_after = Some((0, cancel.clone()));
        let r = PdfRenderer::with_backend(Box::new(fake), Duration::from_secs(5));
        assert_eq!(run(&r, &dir, cancel).await, Err(CANCELLED.to_string()));
    }

    #[tokio::test]
    async fn stuck_page_marks_renderer_dead() {
        let dir = Dir::new();
        let mut fake = Fake::new(2);
        fake.stuck_on = Some(0);
        let r = PdfRenderer::with_backend(Box::new(fake), Duration::from_millis(200));
        assert_eq!(run(&r, &dir, CancellationToken::new()).await, Err(strings::PDFIUM_STUCK.to_string()));
        let began = Instant::now();
        assert_eq!(run(&r, &dir, CancellationToken::new()).await, Err(strings::PDFIUM_STUCK.to_string()));
        assert!(began.elapsed() < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn a_render_queued_behind_a_stuck_one_fails_too() {
        let dir = Dir::new();
        let mut fake = Fake::new(1);
        fake.stuck_on = Some(0);
        let r = PdfRenderer::with_backend(Box::new(fake), Duration::from_millis(300));
        let (a, b) = tokio::join!(run(&r, &dir, CancellationToken::new()), run(&r, &dir, CancellationToken::new()));
        assert_eq!(a, Err(strings::PDFIUM_STUCK.to_string()));
        assert_eq!(b, Err(strings::PDFIUM_STUCK.to_string()));
    }

    #[tokio::test]
    async fn a_queued_render_does_not_time_out_while_the_first_works() {
        struct Slow(Fake);
        struct SlowDoc<'a>(FakeDoc<'a>);
        impl PdfBackend for Slow {
            fn open<'a>(&'a mut self, _: &Path) -> Result<Box<dyn PdfDoc + 'a>, String> {
                Ok(Box::new(SlowDoc(FakeDoc(&self.0))))
            }
        }
        impl PdfDoc for SlowDoc<'_> {
            fn page_sizes(&self) -> Vec<(f32, f32)> {
                self.0.page_sizes()
            }
            fn render_page(&self, index: usize, size: (u32, u32), out: &Path) -> Result<(), String> {
                std::thread::sleep(Duration::from_millis(150));
                self.0.render_page(index, size, out)
            }
        }
        let dir = Dir::new();
        let r = PdfRenderer::with_backend(Box::new(Slow(Fake::new(4))), Duration::from_millis(300));
        let (a, b) = tokio::join!(run(&r, &dir, CancellationToken::new()), run(&r, &dir, CancellationToken::new()));
        assert_eq!(a.unwrap().len(), 4);
        assert_eq!(b.unwrap().len(), 4);
    }

    /// Three pages of red, green and blue, with a proper cross-reference table.
    fn three_page_pdf() -> Vec<u8> {
        let mut objects: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>".to_string(),
        ];
        for page in 0..3 {
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 960 540] /Contents {} 0 R >>", 6 + page));
        }
        for fill in ["1 0 0", "0 1 0", "0 0 1"] {
            let stream = format!("{fill} rg 0 0 960 540 re f");
            objects.push(format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()));
        }
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf += &format!("{} 0 obj\n{object}\nendobj\n", i + 1);
        }
        let xref = pdf.len();
        pdf += &format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for offset in offsets {
            pdf += &format!("{offset:010} 00000 n \n");
        }
        pdf += &format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
        pdf.into_bytes()
    }

    /// Real pdfium: `RP_PDFIUM=<library>`.
    #[tokio::test]
    async fn renders_three_coloured_pages() {
        if std::env::var_os("RP_PDFIUM").is_none() {
            return;
        }
        let dir = Dir::new();
        std::fs::write(dir.0.join("in.pdf"), three_page_pdf()).unwrap();
        let r = PdfRenderer::start(&dir.0).unwrap();
        let pages = run(&r, &dir, CancellationToken::new()).await.unwrap();
        assert_eq!(pages.len(), 3);
        for (page, want) in pages.iter().zip([[255, 0, 0], [0, 255, 0], [0, 0, 255]]) {
            let img = image::open(page).unwrap().to_rgb8();
            assert_eq!(img.dimensions(), (1920, 1080));
            for (x, y) in [(0, 0), (159, 159)] {
                assert_eq!(img.get_pixel(x, y).0, want, "{page:?}");
            }
        }
    }

    #[test]
    fn check_pdf_rejects_non_pdf() {
        let dir = std::env::temp_dir().join(format!("rp-pdf-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("a.pdf");
        std::fs::write(&good, b"%PDF-1.7\n").unwrap();
        assert_eq!(check_pdf(&good), Ok(()));
        let bad = dir.join("b.pdf");
        std::fs::write(&bad, b"hello").unwrap();
        assert_eq!(check_pdf(&bad), Err(NOT_PDF.to_string()));
        let short = dir.join("c.pdf");
        std::fs::write(&short, b"%PD").unwrap();
        assert!(check_pdf(&short).is_err());
        assert!(check_pdf(&dir.join("missing.pdf")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
