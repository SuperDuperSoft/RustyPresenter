use super::*;
use std::sync::{Arc, Mutex};
use std::time::Instant;

type PageFn = Box<dyn FnMut(usize, (u32, u32), &Path) -> Result<(), String> + Send>;

struct Fake {
    sizes: Vec<(f32, f32)>,
    page: PageFn,
}

impl Backend for Fake {
    fn open(&mut self, _: &Path, work: &mut dyn FnMut(&mut dyn Doc) -> Result<(), String>) -> Result<(), String> {
        work(self)
    }
}

impl Doc for Fake {
    fn sizes(&self) -> Vec<(f32, f32)> {
        self.sizes.clone()
    }
    fn render(&mut self, page: usize, size: (u32, u32), out: &Path) -> Result<(), String> {
        (self.page)(page, size, out)
    }
}

fn fake(pages: usize, timeout: Duration, page: impl FnMut(usize, (u32, u32), &Path) -> Result<(), String> + Send + 'static) -> PdfRenderer {
    let page: PageFn = Box::new(page);
    PdfRenderer::spawn(move || Ok(Fake { sizes: vec![(1600.0, 900.0); pages], page }), timeout).unwrap()
}

fn none() -> Box<dyn Fn(usize, usize) + Send> {
    Box::new(|_, _| ())
}

async fn go(r: &PdfRenderer, cancel: CancellationToken) -> Result<Vec<PathBuf>, String> {
    r.render("x.pdf".into(), std::env::temp_dir(), None, none(), cancel).await
}

fn file(name: &str, bytes: &[u8]) -> (std::path::PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("rp-pdf-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    (dir, path)
}

#[test]
fn output_size_16x9_is_1920x1080() {
    assert_eq!(output_size(16.0 / 9.0), (1920, 1080));
    assert_eq!(output_size(793.8 / 446.5), (1920, 1080));
}

#[test]
fn output_size_4x3_is_1920x1440() {
    assert_eq!(output_size(4.0 / 3.0), (1920, 1440));
}

#[test]
fn output_size_tall_caps_height_and_scales_width() {
    assert_eq!(output_size(1.0 / 2.0), (1080, 2160));
    assert_eq!(output_size(0.8), (1728, 2160));
}

#[test]
fn output_size_heights_are_even() {
    for ratio in [1.0f32, 1.5, 1.6, 1.85, 2.0, 2.35, 3.1, 0.6, 7.3] {
        let (w, h) = output_size(ratio);
        assert_eq!((w % 2, h % 2), (0, 0), "{ratio}");
        assert!(w <= 1920 && h <= 2160, "{ratio}");
    }
    assert_eq!(output_size(1920.0 / 1081.0).1, 1082);
}

#[test]
fn no_pages_rejected() {
    assert_eq!(check_pages(&[], None).unwrap_err(), "This presentation has no slides.");
}

#[test]
fn mixed_page_sizes_rejected() {
    let e = check_pages(&[(1600.0, 900.0), (1024.0, 768.0)], None).unwrap_err();
    assert_eq!(e, "This presentation mixes slide sizes, which RustyPresenter can't show.");
}

#[test]
fn pages_within_one_percent_count_as_the_same_size() {
    assert!(check_pages(&[(1600.0, 900.0), (1600.0, 906.0)], None).is_ok());
    assert!(check_pages(&[(1600.0, 900.0), (1600.0, 912.0)], None).is_err());
}

#[test]
fn a4_without_expected_ratio_rejected() {
    for (w, h) in [(842.0, 595.0), (792.0, 612.0)] {
        assert_eq!(check_pages(&[(w, h)], None).unwrap_err(), "LibreOffice couldn't read this presentation.");
    }
}

#[test]
fn a4_with_expected_ratio_accepted() {
    assert!(check_pages(&[(842.0, 595.0)], Some(842.0 / 595.0)).is_ok());
}

#[test]
fn portrait_rejected() {
    assert_eq!(check_pages(&[(900.0, 1600.0)], None).unwrap_err(), "LibreOffice couldn't read this presentation.");
}

#[test]
fn ratio_must_match_expected() {
    assert_eq!(check_pages(&[(1024.0, 768.0)], Some(16.0 / 9.0)).unwrap_err(), "LibreOffice couldn't read this presentation.");
    let ratio = check_pages(&[(1600.0, 900.0)], Some(16.0 / 9.0)).unwrap();
    assert!((ratio - 16.0 / 9.0).abs() < 0.001);
}

#[test]
fn zero_sized_page_rejected() {
    assert!(check_pages(&[(0.0, 0.0)], None).is_err());
    assert!(check_pages(&[(1600.0, 900.0), (0.0, 900.0)], None).is_err());
}

#[test]
fn check_pdf_rejects_non_pdf() {
    let (_, good) = file("a.pdf", b"%PDF-1.7 rest");
    let (_, bad) = file("b.pdf", b"PK\x03\x04 not a pdf");
    let (_, short) = file("c.pdf", b"%PD");
    assert!(check_pdf(&good).is_ok());
    assert!(check_pdf(&bad).is_err());
    assert!(check_pdf(&short).is_err());
    assert!(check_pdf(Path::new("no-such-file.pdf")).is_err());
}

#[tokio::test]
async fn pages_arrive_in_order_with_progress() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let r = fake(3, Duration::from_secs(5), |_, size, _| {
        assert_eq!(size, (1920, 1080));
        Ok(())
    });
    let progress: Box<dyn Fn(usize, usize) + Send> = Box::new(move |done, all| log.lock().unwrap().push((done, all)));
    let files = r.render("x.pdf".into(), std::env::temp_dir(), None, progress, CancellationToken::new()).await.unwrap();
    let names: Vec<_> = files.iter().map(|f| f.file_name().unwrap().to_str().unwrap().to_string()).collect();
    assert_eq!(names, ["slide-001.png", "slide-002.png", "slide-003.png"]);
    assert_eq!(*seen.lock().unwrap(), [(1, 3), (2, 3), (3, 3)]);
}

#[tokio::test]
async fn a_bad_page_size_stops_before_any_page_renders() {
    let page: PageFn = Box::new(|_, _, _| panic!("no page should render"));
    let r = PdfRenderer::spawn(move || Ok(Fake { sizes: vec![(595.0, 842.0)], page }), Duration::from_secs(5)).unwrap();
    assert_eq!(go(&r, CancellationToken::new()).await.unwrap_err(), s::LO_UNREADABLE);
    assert_eq!(go(&r, CancellationToken::new()).await.unwrap_err(), s::LO_UNREADABLE, "a bad file does not end the renderer");
}

#[tokio::test]
async fn stuck_page_marks_renderer_dead() {
    let r = fake(2, Duration::from_millis(200), |_, _, _| {
        std::thread::sleep(Duration::from_secs(60));
        Ok(())
    });
    assert_eq!(go(&r, CancellationToken::new()).await.unwrap_err(), s::PDFIUM_STOPPED);
    let start = Instant::now();
    assert_eq!(go(&r, CancellationToken::new()).await.unwrap_err(), s::PDFIUM_STOPPED);
    assert!(start.elapsed() < Duration::from_millis(100));
}

#[tokio::test]
async fn panic_on_the_thread_marks_renderer_dead() {
    let r = fake(1, Duration::from_secs(5), |_, _, _| panic!("page"));
    assert_eq!(go(&r, CancellationToken::new()).await.unwrap_err(), s::PDFIUM_STOPPED);
    assert_eq!(go(&r, CancellationToken::new()).await.unwrap_err(), s::PDFIUM_STOPPED);
}

#[tokio::test]
async fn cancel_takes_effect_between_pages() {
    let cancel = CancellationToken::new();
    let hit = cancel.clone();
    let rendered = Arc::new(Mutex::new(0));
    let count = rendered.clone();
    let r = fake(5, Duration::from_secs(5), move |page, _, _| {
        *count.lock().unwrap() += 1;
        if page == 1 {
            hit.cancel();
        }
        Ok(())
    });
    assert_eq!(go(&r, cancel).await.unwrap_err(), CANCELLED);
    assert_eq!(*rendered.lock().unwrap(), 2);
}

#[tokio::test]
async fn waiting_behind_another_import_does_not_time_out() {
    // Each import takes three timeouts in all, but every page is inside the limit.
    let r = Arc::new(fake(3, Duration::from_millis(300), |_, _, _| {
        std::thread::sleep(Duration::from_millis(120));
        Ok(())
    }));
    let (a, b) = (r.clone(), r.clone());
    let first = tokio::spawn(async move { go(&a, CancellationToken::new()).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    let second = tokio::spawn(async move { go(&b, CancellationToken::new()).await });
    assert_eq!(first.await.unwrap().unwrap().len(), 3);
    assert_eq!(second.await.unwrap().unwrap().len(), 3);
}

#[tokio::test]
async fn cancel_while_waiting_for_a_turn() {
    let r = Arc::new(fake(2, Duration::from_secs(5), |_, _, _| {
        std::thread::sleep(Duration::from_millis(200));
        Ok(())
    }));
    let a = r.clone();
    let first = tokio::spawn(async move { go(&a, CancellationToken::new()).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    let cancel = CancellationToken::new();
    let hit = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        hit.cancel();
    });
    let start = Instant::now();
    assert_eq!(go(&r, cancel).await.unwrap_err(), CANCELLED);
    assert!(start.elapsed() < Duration::from_millis(200));
    assert_eq!(first.await.unwrap().unwrap().len(), 2);
}

/// A 3-page PDF of filled #ff0000, #00ff00 and #0000ff rectangles at 16:9.
fn three_page_pdf() -> Vec<u8> {
    let colours = ["1 0 0", "0 1 0", "0 0 1"];
    let mut objects = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_string(), "<< /Type /Pages /Kids [3 0 R 5 0 R 7 0 R] /Count 3 >>".to_string()];
    for (i, colour) in colours.iter().enumerate() {
        let stream = format!("{colour} rg 0 0 1600 900 re f");
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1600 900] /Contents {} 0 R >>", 4 + i * 2));
        objects.push(format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()));
    }
    let mut pdf = String::from("%PDF-1.7\n");
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

#[tokio::test]
async fn renders_three_coloured_pages() {
    let Ok(library) = std::env::var("RP_PDFIUM") else { return };
    let (dir, pdf) = file("three.pdf", &three_page_pdf());
    let r = PdfRenderer::spawn(move || bind(&[PathBuf::from(library)]), PAGE_TIMEOUT).unwrap();
    let files = r.render(pdf, dir.join("out"), None, none(), CancellationToken::new()).await.unwrap();
    assert_eq!(files.len(), 3);
    for (file, colour) in files.iter().zip([[255, 0, 0], [0, 255, 0], [0, 0, 255]]) {
        let image = image::open(file).unwrap().to_rgb8();
        assert_eq!(image.dimensions(), (1920, 1080));
        for x in 0..160 {
            for y in 0..160 {
                assert_eq!(image.get_pixel(x, y).0, colour);
            }
        }
    }
}

#[tokio::test]
async fn a_broken_pdf_reports_and_the_renderer_lives() {
    let Ok(library) = std::env::var("RP_PDFIUM") else { return };
    let (dir, bad) = file("bad.pdf", b"%PDF-1.7\nthis is not a pdf");
    let (_, good) = file("good.pdf", &three_page_pdf());
    let r = PdfRenderer::spawn(move || bind(&[PathBuf::from(library)]), PAGE_TIMEOUT).unwrap();
    let e = r.render(bad, dir.join("out"), None, none(), CancellationToken::new()).await.unwrap_err();
    assert_eq!(e, s::PDF_UNREADABLE);
    assert!(r.render(good, dir.join("out"), None, none(), CancellationToken::new()).await.is_ok());
}
