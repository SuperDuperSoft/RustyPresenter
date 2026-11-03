use super::*;
use crate::{
    app::{Imported, ImportPurpose, test_app},
    import::{libreoffice, pdf::{PageError, PdfBackend, PdfDoc}, powerpoint},
    platform::tools::ToolEnv,
};
use std::{io::{Cursor, Write}, time::{Duration, Instant}};

const PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";

/// pdfium's stand-in: `n` 16:9 pages, each a small PNG.
struct Pages(usize);
struct Doc(usize);
impl PdfBackend for Pages {
    fn open<'a>(&'a mut self, _: &Path) -> Result<Box<dyn PdfDoc + 'a>, String> {
        Ok(Box::new(Doc(self.0)))
    }
}
impl PdfDoc for Doc {
    fn page_sizes(&self) -> Vec<(f32, f32)> {
        vec![(960.0, 540.0); self.0]
    }
    fn render_page(&self, index: usize, _: (u32, u32), out: &Path) -> Result<(), PageError> {
        // Each page differs, or the store would keep one file for all of them.
        image::RgbImage::from_pixel(4, 4, image::Rgb([index as u8, 0, 0])).save(out).unwrap();
        Ok(())
    }
}

fn pages(n: usize) -> Option<Arc<PdfRenderer>> {
    Some(Arc::new(PdfRenderer::with_backend(Box::new(Pages(n)), Duration::from_secs(5))))
}

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("rp-deck-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn no_soffice() -> FindSoffice {
    Box::new(|| None)
}

fn converters(lo: Option<Arc<LibreOffice>>, pdf: Option<Arc<PdfRenderer>>, ppt: Option<PowerPoint>) -> Arc<Converters> {
    let c = Converters::new(no_soffice(), pdf, ppt, dir());
    *c.libreoffice.lock().unwrap() = lo;
    Arc::new(c)
}

fn app_with(c: Arc<Converters>) -> Arc<App> {
    let app = test_app();
    app.set_converters(c);
    app
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn odp() -> Vec<u8> {
    zip_of(&[("mimetype", b"application/vnd.oasis.opendocument.presentation")])
}

/// A 16:9 PowerPoint file that says it has `slides` slides, of which the first `hidden` are hidden.
fn pptx(slides: usize, hidden: usize) -> Vec<u8> {
    let mut overrides = format!(r#"<Override PartName="/ppt/presentation.xml" ContentType="{PPTX}"/>"#);
    let (mut ids, mut rels) = (String::new(), String::new());
    let mut parts = vec![];
    for i in 1..=slides {
        overrides += &format!(r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#);
        ids += &format!(r#"<p:sldId id="{}" r:id="rId{i}"/>"#, 255 + i);
        rels += &format!(r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{i}.xml"/>"#);
        let show = if i <= hidden { r#" show="0""# } else { "" };
        parts.push((format!("ppt/slides/slide{i}.xml"), format!(r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"{show}/>"#)));
    }
    let types = format!(r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">{overrides}</Types>"#);
    let root = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#;
    let presentation = format!(r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/></p:presentation>"#);
    let rels = format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#);
    let mut all: Vec<(&str, &[u8])> = vec![("[Content_Types].xml", types.as_bytes()), ("_rels/.rels", root.as_bytes()), ("ppt/presentation.xml", presentation.as_bytes()), ("ppt/_rels/presentation.xml.rels", rels.as_bytes())];
    all.extend(parts.iter().map(|(n, p)| (n.as_str(), p.as_bytes())));
    zip_of(&all)
}

/// `bytes` as a staged file, as `import_staged` hands it over.
fn staged(app: &Arc<App>, bytes: &[u8]) -> PathBuf {
    let tmp = app.store.tmp().join(format!("{}.part", Uuid::new_v4()));
    std::fs::create_dir_all(app.store.tmp()).unwrap();
    std::fs::write(&tmp, bytes).unwrap();
    tmp
}

fn job(app: &App, id: Uuid) -> Option<crate::app::Job> {
    app.jobs.lock().unwrap().iter().find(|j| j.id == id).cloned()
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let began = Instant::now();
    while !done() {
        assert!(began.elapsed() < Duration::from_secs(20), "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Waits until the job has stopped running: it is gone, or it failed, or it stays for its note.
async fn finished(app: &App, id: Uuid) {
    until("the job", || job(app, id).is_none_or(|j| j.error.is_some() || j.done)).await;
}

fn tmp_empty(app: &App) -> bool {
    std::fs::read_dir(app.store.tmp()).map_or(true, |mut d| d.next().is_none())
}

fn decks(app: &App) -> usize {
    app.data.lock().unwrap().decks.len()
}

fn ooxml() -> Kind {
    Kind::Ooxml { variant: sniff::Variant::Pptx, slide_size: None, slides: None, hidden: 0 }
}

#[test]
fn route_table() {
    let snap = || Err(LO_SNAP.to_string());
    for ppt in [false, true] {
        for lo in [Lo::None, Lo::Snap, Lo::Ok] {
            assert_eq!(route(&Kind::Encrypted, ppt, lo), Err(ENCRYPTED.to_string()));
            assert_eq!(route(&Kind::NotPresentation, ppt, lo), Err(NOT_PRESENTATION.to_string()));
            assert_eq!(route(&Kind::Pdf, ppt, lo), Ok(Route::Pdf));
            let by_lo = match lo { Lo::None => Ok(Route::Missing), Lo::Snap => snap(), Lo::Ok => Ok(Route::LibreOffice) };
            assert_eq!(route(&Kind::Keynote, ppt, lo), by_lo, "PowerPoint can't open Keynote");
            for kind in [ooxml(), Kind::Ppt, Kind::Odp] {
                let want = if ppt { Ok(Route::PowerPoint { fallback: lo == Lo::Ok }) } else { by_lo.clone() };
                assert_eq!(route(&kind, ppt, lo), want, "{kind:?} {ppt} {lo:?}");
            }
        }
    }
}

#[test]
fn every_deck_extension_is_listed_once() {
    let mut sorted = DECK_EXT.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), DECK_EXT.len());
    assert_eq!(sorted, ["key", "odp", "pdf", "pot", "potx", "pps", "ppsx", "ppt", "pptm", "pptx"]);
}

#[tokio::test]
async fn not_presentation_adds_no_deck_and_ends_job_with_message() {
    let app = test_app();
    let file = staged(&app, b"just some text");
    let id = start(app.clone(), file, "Notes".into());
    finished(&app, id).await;
    assert_eq!(job(&app, id).unwrap().error.as_deref(), Some(NOT_PRESENTATION));
    assert_eq!(decks(&app), 0);
    assert!(tmp_empty(&app));
}

#[tokio::test]
async fn keynote_package_directory_gives_plain_message() {
    let app = test_app();
    let package = dir().join("Talk.key");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(package.join("Index.zip"), b"x").unwrap();
    let Imported::Job(id) = app.import_path(&package, ImportPurpose::PowerPoint, |_| {}).await.unwrap() else { panic!("no job") };
    finished(&app, id).await;
    assert_eq!(job(&app, id).unwrap().error.as_deref(), Some(KEYNOTE_PACKAGE));
    assert!(package.join("Index.zip").exists(), "the operator's folder stays");
}

#[tokio::test]
async fn pdf_is_rendered_and_published_as_one_deck() {
    let app = app_with(converters(None, pages(3), None));
    let source = dir().join("Sermon.pdf");
    std::fs::write(&source, b"%PDF-1.7\n").unwrap();
    let Imported::Job(id) = app.import_path(&source, ImportPurpose::PowerPoint, |_| {}).await.unwrap() else { panic!("no job") };
    finished(&app, id).await;
    assert!(job(&app, id).is_none(), "nothing to say, so the card goes");
    let d = app.data.lock().unwrap();
    assert_eq!((d.decks.len(), d.decks[0].title.as_str(), d.decks[0].slides.len()), (1, "Sermon", 3));
    assert!(d.decks[0].slides.iter().all(|a| a.ends_with(".png") && app.store.asset_path(a).exists()));
    drop(d);
    assert!(tmp_empty(&app), "the work folder and the staged file go");
    assert!(source.exists(), "only our copy is moved");
}

#[tokio::test]
async fn pdf_route_without_pdfium_gives_reinstall_message() {
    for app in [test_app(), app_with(converters(None, None, None))] {
        let file = staged(&app, b"%PDF-1.7\n");
        let id = start(app.clone(), file, "x".into());
        finished(&app, id).await;
        assert_eq!(job(&app, id).unwrap().error.as_deref(), Some(PDFIUM_MISSING));
        assert!(tmp_empty(&app));
    }
}

#[tokio::test]
async fn no_converter_says_how_to_install_one() {
    let app = app_with(converters(None, pages(1), None));
    let file = staged(&app, &odp());
    let id = start(app.clone(), file, "x".into());
    finished(&app, id).await;
    let error = job(&app, id).unwrap().error.unwrap();
    assert!(error.starts_with("To import presentations, install LibreOffice. ") && error.contains(tools::install_hint(tools::OS)), "{error}");
}

#[tokio::test]
async fn libreoffice_import_adds_note() {
    let d = dir();
    let app = app_with(converters(Some(libreoffice::tests::fake("pdf", &d)), pages(2), None));
    let file = staged(&app, &odp());
    let id = start(app.clone(), file, "Talk".into());
    finished(&app, id).await;
    let j = job(&app, id).expect("a note keeps the card");
    assert_eq!((j.note.as_deref(), j.error), (Some(strings::LIBREOFFICE_NOTE), None));
    let deck = app.data.lock().unwrap().decks[0].clone();
    assert_eq!((j.result, deck.slides.len()), (Some(deck.id), 2));
    assert!(tmp_empty(&app));
}

#[tokio::test]
async fn a_slide_count_that_differs_is_noted_beside_the_program() {
    let d = dir();
    let app = app_with(converters(Some(libreoffice::tests::fake("pdf", &d)), pages(2), None));
    let file = staged(&app, &pptx(4, 1));
    let id = start(app.clone(), file, "Talk".into());
    finished(&app, id).await;
    assert_eq!(job(&app, id).unwrap().note.as_deref(), Some("Imported with LibreOffice — check the slides. 3 slides were expected but 2 were imported."));
}

#[tokio::test]
async fn hidden_slides_are_not_expected() {
    let d = dir();
    let app = app_with(converters(Some(libreoffice::tests::fake("pdf", &d)), pages(3), None));
    let file = staged(&app, &pptx(4, 1));
    let id = start(app.clone(), file, "Talk".into());
    finished(&app, id).await;
    assert_eq!(job(&app, id).unwrap().note.as_deref(), Some(strings::LIBREOFFICE_NOTE));
}

#[tokio::test]
async fn cancel_removes_job_and_work_dir() {
    let d = dir();
    let app = app_with(converters(Some(libreoffice::tests::fake("sleep:30000,pdf", &d)), pages(1), None));
    let file = staged(&app, &odp());
    let id = start(app.clone(), file, "Talk".into());
    until("the fake to start", || std::fs::read_to_string(d.join("mark")).is_ok_and(|m| m.contains("start"))).await;
    app.job_cancel(id);
    until("the job to go", || job(&app, id).is_none()).await;
    assert_eq!(decks(&app), 0);
    assert!(tmp_empty(&app));
    let marks = std::fs::read_to_string(d.join("mark")).unwrap();
    assert!(!marks.contains("end"), "the fake never finished: {marks}");
}

#[tokio::test]
async fn waiting_label_shows_then_restores() {
    let d = dir();
    let app = app_with(converters(Some(libreoffice::tests::fake("sleep:1000,pdf", &d)), pages(1), None));
    let (first, second) = (staged(&app, &odp()), staged(&app, &odp()));
    let a = start(app.clone(), first, "One".into());
    until("the first to start", || std::fs::read_to_string(d.join("mark")).is_ok_and(|m| m.contains("start"))).await;
    let b = start(app.clone(), second, "Two".into());
    until("the wait", || job(&app, b).is_some_and(|j| j.label == strings::WAITING_FOR_LIBREOFFICE)).await;
    until("the label to return", || job(&app, b).is_some_and(|j| j.label == "Importing “Two”")).await;
    finished(&app, a).await;
    finished(&app, b).await;
    assert_eq!(decks(&app), 2);
}

#[tokio::test]
async fn overlapping_imports_set_their_own_results() {
    let d = dir();
    let app = app_with(converters(Some(libreoffice::tests::fake("pdf", &d)), pages(1), None));
    let (first, second) = (staged(&app, &odp()), staged(&app, &odp()));
    let a = start(app.clone(), first, "One".into());
    let b = start(app.clone(), second, "Two".into());
    finished(&app, a).await;
    finished(&app, b).await;
    let d = app.data.lock().unwrap().decks.clone();
    let of = |title: &str| d.iter().find(|x| x.title == title).unwrap().id;
    assert_eq!(job(&app, a).unwrap().result, Some(of("One")));
    assert_eq!(job(&app, b).unwrap().result, Some(of("Two")));
}

#[cfg(windows)]
#[tokio::test]
async fn powerpoint_comes_first_and_needs_no_note() {
    let d = dir();
    let good = powerpoint::tests::good(2);
    let app = app_with(converters(Some(libreoffice::tests::fake("pdf", &d)), pages(9), Some(powerpoint::tests::fake(&good, &d))));
    let file = staged(&app, &pptx(2, 0));
    let id = start(app.clone(), file, "Talk".into());
    finished(&app, id).await;
    assert_eq!(job(&app, id).map(|j| j.error), None);
    assert_eq!(app.data.lock().unwrap().decks[0].slides.len(), 2);
    assert!(!d.join("mark").exists(), "LibreOffice was not needed");
}

#[cfg(windows)]
#[tokio::test]
async fn powerpoint_works_without_pdfium() {
    let d = dir();
    let good = powerpoint::tests::good(1);
    let app = app_with(converters(None, None, Some(powerpoint::tests::fake(&good, &d))));
    let file = staged(&app, &pptx(1, 0));
    let id = start(app.clone(), file, "Talk".into());
    finished(&app, id).await;
    assert_eq!(job(&app, id).map(|j| j.error), None);
    assert_eq!(decks(&app), 1);
}

#[cfg(windows)]
#[tokio::test]
async fn failed_powerpoint_falls_back_to_libreoffice_and_says_so() {
    let d = dir();
    let app = app_with(converters(Some(libreoffice::tests::fake("pdf", &d)), pages(2), Some(powerpoint::tests::fake("fail", &d))));
    let file = staged(&app, &pptx(2, 0));
    let id = start(app.clone(), file, "Talk".into());
    finished(&app, id).await;
    let j = job(&app, id).unwrap();
    assert_eq!((j.note.as_deref(), j.error), (Some(FALLBACK_NOTE), None));
    assert_eq!(decks(&app), 1);
}

#[cfg(windows)]
#[tokio::test]
async fn a_fallback_still_reports_a_slide_count_that_differs() {
    let d = dir();
    let app = app_with(converters(Some(libreoffice::tests::fake("pdf", &d)), pages(1), Some(powerpoint::tests::fake("fail", &d))));
    let file = staged(&app, &pptx(2, 0));
    let id = start(app.clone(), file, "Talk".into());
    finished(&app, id).await;
    assert_eq!(job(&app, id).unwrap().note.unwrap(), format!("{FALLBACK_NOTE} 2 slides were expected but 1 were imported."));
}

#[cfg(windows)]
#[tokio::test]
async fn failed_powerpoint_without_libreoffice_shows_its_own_message() {
    let d = dir();
    let app = app_with(converters(None, pages(1), Some(powerpoint::tests::fake("fail", &d))));
    let file = staged(&app, &pptx(1, 0));
    let id = start(app.clone(), file, "Talk".into());
    finished(&app, id).await;
    assert_eq!(job(&app, id).unwrap().error.as_deref(), Some(strings::PPT_CANNOT_READ));
    assert_eq!(decks(&app), 0);
}

#[cfg(windows)]
#[tokio::test]
async fn failed_powerpoint_does_not_fall_back_without_pdfium() {
    let d = dir();
    let app = app_with(converters(Some(libreoffice::tests::fake("pdf", &d)), None, Some(powerpoint::tests::fake("fail", &d))));
    let file = staged(&app, &pptx(1, 0));
    let id = start(app.clone(), file, "Talk".into());
    finished(&app, id).await;
    assert_eq!(job(&app, id).unwrap().error.as_deref(), Some(strings::PPT_CANNOT_READ));
    assert!(!d.join("mark").exists(), "LibreOffice's PDF could not have been rendered");
}

#[cfg(windows)]
#[tokio::test]
async fn cancelling_powerpoint_does_not_start_libreoffice() {
    let d = dir();
    let app = app_with(converters(Some(libreoffice::tests::fake("pdf", &d)), pages(1), Some(powerpoint::tests::fake("sleep:30000", &d))));
    let file = staged(&app, &pptx(1, 0));
    let id = start(app.clone(), file, "Talk".into());
    until("the fake to start", || std::fs::read_to_string(d.join("ppt-mark")).is_ok_and(|m| m.contains("export"))).await;
    app.job_cancel(id);
    until("the job to go", || job(&app, id).is_none()).await;
    assert_eq!(decks(&app), 0);
    assert!(!d.join("mark").exists());
}

#[tokio::test]
async fn a_pdf_without_pages_adds_no_deck() {
    let app = app_with(converters(None, pages(0), None));
    let file = staged(&app, b"%PDF-1.7\n");
    let id = start(app.clone(), file, "x".into());
    finished(&app, id).await;
    assert_eq!(job(&app, id).unwrap().error.as_deref(), Some(PDF_NO_SLIDES));
    assert_eq!(decks(&app), 0);
}

#[test]
fn status_says_what_this_computer_has() {
    let d = dir();
    let c = converters(Some(libreoffice::tests::fake("pdf", &d)), pages(1), None);
    assert_eq!(c.status(), ConverterStatus { powerpoint: false, libreoffice: Lo::Ok, pdfium: true });
    assert_eq!(converters(None, None, None).status(), ConverterStatus { powerpoint: false, libreoffice: Lo::None, pdfium: false });
    let snap = Converters::new(Box::new(|| Some(Soffice { path: "/snap/bin/libreoffice".into(), snap: true })), None, None, dir());
    assert_eq!(snap.status().libreoffice, Lo::Snap);
}

#[test]
fn libreoffice_found_after_install_without_restart() {
    let folder = dir();
    let cache = Mutex::new(None);
    let searched = folder.clone();
    let env = move |at: &Path| ToolEnv { os: tools::Os::Windows, var: Box::new(|_| None), home: None, path_dirs: vec![], registry: vec![at.to_path_buf()], opt_dirs: vec![] };
    let look = env.clone();
    let c = Converters::new(Box::new(move || tools::find_soffice_in(&cache, &look(&searched))), None, None, dir());
    assert!(c.libreoffice().is_none());
    let path = tools::soffice_candidates(&env(&folder)).into_iter().next().unwrap();
    std::fs::write(&path, b"").unwrap();
    assert!(c.libreoffice().is_some(), "found without a restart");
    assert_eq!(c.status().libreoffice, Lo::Ok);
}

#[test]
fn the_ratio_and_count_come_from_the_file() {
    let kind = Kind::Ooxml { variant: sniff::Variant::Pptx, slide_size: Some((1600, 900)), slides: Some(5), hidden: 2 };
    assert!((expected_ratio(&kind).unwrap() - 16.0 / 9.0).abs() < 1e-3);
    assert_eq!(expected_slides(&kind), Some(3));
    assert_eq!((expected_ratio(&ooxml()), expected_slides(&ooxml())), (None, None));
    assert_eq!(expected_ratio(&Kind::Ooxml { variant: sniff::Variant::Pptx, slide_size: Some((1, 0)), slides: None, hidden: 0 }), None);
}

/// `import::init` and a real import. `RP_TEST_SOFFICE=1`, `RP_PDFIUM=<library>`, `RP_TEST_DECK=<a .pptx>`, whose slides
/// are counted by the sniff.
#[tokio::test]
async fn real_converters_make_a_deck() {
    let Some(deck) = std::env::var_os("RP_TEST_DECK").filter(|_| std::env::var_os("RP_TEST_SOFFICE").is_some() && std::env::var_os("RP_PDFIUM").is_some()) else { return };
    let app = test_app();
    tokio::spawn(crate::import::init(&app));
    let Imported::Job(id) = app.import_path(Path::new(&deck), ImportPurpose::PowerPoint, |_| {}).await.unwrap() else { panic!("no job") };
    let began = Instant::now();
    while job(&app, id).is_some_and(|j| j.error.is_none() && !j.done) {
        assert!(began.elapsed() < Duration::from_secs(180), "timed out");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let j = job(&app, id).expect("LibreOffice leaves a note");
    assert_eq!((j.error, j.note.as_deref()), (None, Some(strings::LIBREOFFICE_NOTE)));
    let deck = app.data.lock().unwrap().decks[0].clone();
    let Kind::Ooxml { slides: Some(n), .. } = sniff::sniff(Path::new(&std::env::var_os("RP_TEST_DECK").unwrap())).unwrap() else { panic!("not a pptx") };
    assert_eq!(deck.slides.len(), n);
    let first = image::open(app.store.asset_path(&deck.slides[0])).unwrap();
    assert_eq!((first.width(), first.height()), (1920, 1080));
    assert!(tmp_empty(&app));
}
