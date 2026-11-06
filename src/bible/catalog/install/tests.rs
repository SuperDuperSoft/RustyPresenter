use super::*;
use crate::bible::catalog::{ebible, Scope};
use std::sync::{Arc, Mutex};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener};

const XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<vpl>
<v b="GEN" c="1" v="1">In the beginning God created the heavens and the earth.</v>
<v b="JHN" c="3" v="16">For God so loved the world &amp; gave his one and only Son.</v>
<v b="JHN" c="3" v="17">For God didn't send his Son.</v>
</vpl>"#;

const CC_BY_SA: &str = r#"<html><body><p>Copyright © 2020 Someone.</p><p>Licensed under <a href="https://creativecommons.org/licenses/by-sa/4.0/">CC BY-SA 4.0</a>.</p></body></html>"#;

fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    for (name, text) in files {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(text.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn row(id: &str, update: &str) -> CatalogRow {
    let r = ebible::Row {
        language_code: "eng".into(), translation_id: id.into(), language_name: "English".into(), language_name_en: "English".into(),
        title: format!("Test Bible {id}"), redistributable: true, downloadable: true, copyright: "Copyright © 2020 Someone".into(),
        update_date: update.into(), ot_books: 39, nt_books: 27, script: "Latn".into(), text_direction: "ltr".into(), ..Default::default()
    };
    ebible::offer(&r, &Licences::default()).expect("the test row is offered")
}

/// What the test server answers: status, the body, and a `Content-Length` that may lie. `stall` sends the headers and
/// then nothing.
#[derive(Clone)]
struct Answer { status: u16, body: Vec<u8>, length: Option<u64>, stall: bool }

impl Answer {
    fn ok(body: Vec<u8>) -> Self { Answer { status: 200, length: Some(body.len() as u64), body, stall: false } }
}

struct Server { base: String, hits: Arc<Mutex<Vec<String>>>, answer: Arc<Mutex<Answer>> }

async fn server(answer: Answer) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (hits, answer) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(answer)));
    let (h, a) = (hits.clone(), answer.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let (h, a) = (h.clone(), a.clone());
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf).await { Ok(0) | Err(_) => break, Ok(n) => req.extend_from_slice(&buf[..n]) }
                }
                h.lock().unwrap().push(String::from_utf8_lossy(&req).to_string());
                let a = a.lock().unwrap().clone();
                let length = a.length.map(|l| format!("content-length: {l}\r\n")).unwrap_or_default();
                let head = format!("HTTP/1.1 {} X\r\ncontent-type: application/zip\r\n{length}connection: close\r\n\r\n", a.status);
                let _ = sock.write_all(head.as_bytes()).await;
                if a.stall { tokio::time::sleep(Duration::from_secs(30)).await; return; }
                let _ = sock.write_all(&a.body).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Server { base, hits, answer }
}

fn ctx(root: &Path, base: &str) -> Ctx { Ctx { root: root.to_path_buf(), now: 1_800_000_000, force: false, base: base.into() } }

fn partials_left(root: &Path) -> usize { fs::read_dir(root.join(PARTIAL)).map(|d| d.count()).unwrap_or(0) }

async fn install(root: &Path, base: &str, row: &CatalogRow) -> Result<BibleMeta, InstallError> {
    ebible(&ctx(root, base), row, &CancellationToken::new(), &|_| {}).await
}

#[tokio::test]
async fn a_download_is_checked_saved_listed_and_its_licence_remembered() {
    let root = crate::store::test_dir();
    let s = server(Answer::ok(zip_of(&[("engtest_vpl.xml", XML), ("engtest_about.htm", CC_BY_SA)]))).await;
    let r = row("engtest", "2026-03-01");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    let m = ebible(&ctx(&root, &s.base), &r, &CancellationToken::new(), &move |p| s2.lock().unwrap().push(p)).await.unwrap();

    assert_eq!((m.id.as_str(), m.abbr.as_str(), m.name.as_str(), m.lang.as_str()), ("ebible:engtest", "TEST", "Test Bible engtest", "eng"));
    assert_eq!(m.licence, crate::bible::library::Licence::Cc { kind: "by-sa".into(), version: "4.0".into(), url: "https://creativecommons.org/licenses/by-sa/4.0/".into() });
    assert_eq!(m.source, Source::Ebible { update_date: "2026-03-01".into() });
    assert_eq!(m.books, vec![0, 42]);
    assert!(m.bytes > 0);

    let hit = s.hits.lock().unwrap()[0].clone();
    assert!(hit.starts_with("GET /engtest_vpl.zip "), "{hit}");
    assert!(hit.to_lowercase().contains("user-agent: rustypresenter/") && hit.contains("(+https://github.com/SuperDuperSoft/RustyPresenter)"), "{hit}");

    let seen = seen.lock().unwrap();
    assert_eq!(seen.last(), Some(&100));
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "progress only goes up: {seen:?}");

    assert!(root.join("bibles/ebible~engtest.json").is_file());
    assert_eq!(partials_left(&root), 0, "the partial file is gone");
    assert!(import::list(&root).iter().any(|b| b.id == "ebible:engtest"), "listed at once");
    let b = import::get(&root, "ebible:engtest").unwrap();
    assert_eq!(b.lookup("John 3:16").unwrap().1[0].text, "For God so loved the world & gave his one and only Son.");
    assert_eq!(crate::bible::library::read_index(&root)["ebible:engtest"].licence, m.licence, "the index has its licence");

    let cached = ebible::Licences::load(&root);
    let again = ebible::offer(&ebible::Row { translation_id: "engtest".into(), redistributable: true, downloadable: true,
        language_code: "eng".into(), title: "T".into(), update_date: "2026-03-01".into(), ..Default::default() }, &cached).unwrap();
    assert!(matches!(again.licence(), crate::bible::catalog::LicenceState::Known(_)), "the licence is known next time");
    assert_eq!(again.scope(), Scope::Portions);
}

#[tokio::test]
async fn no_licence_means_nothing_kept_and_the_row_is_hidden() {
    let root = crate::store::test_dir();
    let s = server(Answer::ok(zip_of(&[("engx_vpl.xml", XML), ("engx_about.htm", "<p>All rights reserved.</p>")]))).await;
    let e = install(&root, &s.base, &row("engx", "2026-01-01")).await.unwrap_err();
    assert_eq!(e, InstallError::Licence);
    assert_eq!(e.message("X", false), strings::LICENCE_REFUSED);
    assert!(!root.join("bibles/ebible~engx.json").exists());
    assert_eq!(partials_left(&root), 0);
    let r = ebible::Row { translation_id: "engx".into(), redistributable: true, downloadable: true, language_code: "eng".into(),
        title: "T".into(), update_date: "2026-01-01".into(), ..Default::default() };
    assert!(ebible::offer(&r, &ebible::Licences::load(&root)).is_none(), "refused for this edition");

    // No notice page at all is no licence too.
    let s = server(Answer::ok(zip_of(&[("engy_vpl.xml", XML)]))).await;
    assert_eq!(install(&root, &s.base, &row("engy", "2026-01-01")).await.unwrap_err(), InstallError::Licence);
}

#[tokio::test]
async fn blocked_text_is_refused_even_under_another_name() {
    let root = crate::store::test_dir();
    let xml = r#"<vpl><v b="PSA" c="23" v="1">The Lord is my best friend and my shepherd.</v></vpl>"#;
    let s = server(Answer::ok(zip_of(&[("engz_vpl.xml", xml), ("engz_about.htm", CC_BY_SA)]))).await;
    let e = install(&root, &s.base, &row("engz", "2026-01-01")).await.unwrap_err();
    assert_eq!(e, InstallError::Blocked);
    assert_eq!(e.message("X", true), crate::scripture::strings::BLOCKED, "blocked says so, even for an update");
    assert!(!root.join("bibles/ebible~engz.json").exists());
    assert_eq!(partials_left(&root), 0);
}

#[tokio::test]
async fn damaged_or_empty_downloads_are_not_kept() {
    let root = crate::store::test_dir();
    for body in [b"not a zip at all".to_vec(), zip_of(&[("other.xml", XML), ("engd_about.htm", CC_BY_SA)]),
        zip_of(&[("engd_vpl.xml", "<vpl></vpl>"), ("engd_about.htm", CC_BY_SA)])] {
        let s = server(Answer::ok(body)).await;
        let e = install(&root, &s.base, &row("engd", "2026-01-01")).await.unwrap_err();
        assert!(matches!(e, InstallError::Unusable(_)), "{e:?}");
        assert_eq!(e.message("Test", false), "Test couldn't be used, so it wasn't added.");
    }
    assert!(!root.join("bibles/ebible~engd.json").exists());
    assert_eq!(partials_left(&root), 0);
}

#[tokio::test]
async fn server_errors_and_cut_off_downloads_say_check_the_internet() {
    let root = crate::store::test_dir();
    let s = server(Answer { status: 404, body: vec![], length: Some(0), stall: false }).await;
    let e = install(&root, &s.base, &row("engn", "2026-01-01")).await.unwrap_err();
    assert!(matches!(e, InstallError::Download(_)), "{e:?}");
    assert_eq!(e.message("Test", false), "Couldn't download Test. Check the internet and try again.");

    let body = zip_of(&[("engn_vpl.xml", XML), ("engn_about.htm", CC_BY_SA)]);
    *s.answer.lock().unwrap() = Answer { length: Some(body.len() as u64 + 100), ..Answer::ok(body) };
    assert!(matches!(install(&root, &s.base, &row("engn", "2026-01-01")).await, Err(InstallError::Download(_))));

    assert!(matches!(install(&root, "http://127.0.0.1:9", &row("engn", "2026-01-01")).await, Err(InstallError::Download(_))));
    assert_eq!(partials_left(&root), 0);
}

#[tokio::test]
async fn too_large_stops_at_the_header_or_at_the_cap() {
    let root = crate::store::test_dir();
    let s = server(Answer { status: 200, body: vec![0; 10], length: Some(MAX_ZIP + 1), stall: false }).await;
    let e = install(&root, &s.base, &row("engl", "2026-01-01")).await.unwrap_err();
    assert_eq!(e, InstallError::TooLarge);
    assert_eq!(e.message("X", false), strings::TOO_LARGE);
    assert_eq!(s.hits.lock().unwrap().len(), 1);

    // No length given: stops once more than the cap has arrived.
    *s.answer.lock().unwrap() = Answer { status: 200, body: vec![0; (MAX_ZIP + (1 << 20)) as usize], length: None, stall: false };
    assert_eq!(install(&root, &s.base, &row("engl", "2026-01-01")).await.unwrap_err(), InstallError::TooLarge);
    assert_eq!(partials_left(&root), 0);
}

#[test]
fn an_entry_larger_than_its_cap_is_refused_and_found_in_any_folder() {
    let bytes = zip_of(&[("Some/Folder/ENGQ_VPL.XML", "0123456789"), ("small.htm", "abc")]);
    let mut zip = zip::ZipArchive::new(io::Cursor::new(bytes)).unwrap();
    assert_eq!(entry(&mut zip, "engq_vpl.xml", 10).unwrap(), Some(b"0123456789".to_vec()));
    assert_eq!(entry(&mut zip, "engq_vpl.xml", 9), Err(InstallError::TooLarge));
    assert_eq!(entry(&mut zip, "missing.xml", 9), Ok(None));
}

#[tokio::test]
async fn cancel_stops_the_download_and_leaves_nothing() {
    let root = crate::store::test_dir();
    let s = server(Answer { status: 200, body: vec![], length: Some(1000), stall: true }).await;
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move { tokio::time::sleep(Duration::from_millis(300)).await; c.cancel(); });
    let r = row("engc", "2026-01-01");
    let e = tokio::time::timeout(Duration::from_secs(10), ebible(&ctx(&root, &s.base), &r, &cancel, &|_| {})).await
        .expect("cancel ends the job at once").unwrap_err();
    assert_eq!(e, InstallError::Cancelled);
    assert_eq!(e.message("X", false), crate::import::CANCELLED);
    assert_eq!(partials_left(&root), 0);
    assert!(!root.join("bibles/ebible~engc.json").exists());
}

#[tokio::test]
async fn a_failed_update_keeps_the_installed_copy() {
    let root = crate::store::test_dir();
    let s = server(Answer::ok(zip_of(&[("engu_vpl.xml", XML), ("engu_about.htm", CC_BY_SA)]))).await;
    install(&root, &s.base, &row("engu", "2026-01-01")).await.unwrap();
    let before = fs::read(root.join("bibles/ebible~engu.json")).unwrap();

    for bad in [zip_of(&[("engu_vpl.xml", "<vpl/>"), ("engu_about.htm", CC_BY_SA)]), zip_of(&[("engu_vpl.xml", XML), ("engu_about.htm", "none")])] {
        *s.answer.lock().unwrap() = Answer::ok(bad);
        let e = install(&root, &s.base, &row("engu", "2026-09-09")).await.unwrap_err();
        assert_eq!(e.message("Test Bible", true), "The update for Test Bible couldn't be used. The copy you have still works.");
        assert_eq!(fs::read(root.join("bibles/ebible~engu.json")).unwrap(), before, "the installed file is untouched");
        assert!(import::get(&root, "ebible:engu").unwrap().lookup("John 3:16").is_ok());
        assert_eq!(import::meta(&root, "ebible:engu").unwrap().source, Source::Ebible { update_date: "2026-01-01".into() });
    }

    *s.answer.lock().unwrap() = Answer::ok(zip_of(&[("engu_vpl.xml", &XML.replace("didn't send", "did not send")), ("engu_about.htm", CC_BY_SA)]));
    install(&root, &s.base, &row("engu", "2026-09-09")).await.unwrap();
    assert_eq!(import::get(&root, "ebible:engu").unwrap().lookup("John 3:17").unwrap().1[0].text, "For God did not send his Son.", "a good update replaces it");
    assert_eq!(import::meta(&root, "ebible:engu").unwrap().source, Source::Ebible { update_date: "2026-09-09".into() });
}

#[test]
fn hostile_ids_are_refused() {
    for ok in ["spaRV1909", "eng-Brenton", "grc_x", "a", &"x".repeat(40)] { assert!(valid_source_id(ok), "{ok}"); }
    for bad in ["", "../x", "a/b", "a\\b", "a b", "a.b", "CON", "con", "Nul", "COM1", "lpt9", "é", &"x".repeat(41)] {
        assert!(!valid_source_id(bad), "{bad:?}");
    }
    assert!(valid_source_id("COM10") && valid_source_id("CONX"), "only the device names themselves");
    let r = ebible::Row { translation_id: "../x".into(), redistributable: true, downloadable: true, title: "T".into(), ..Default::default() };
    assert!(ebible::offer(&r, &Licences::default()).is_none(), "the catalog skips such a row");
}

#[test]
fn start_up_empties_the_partial_folder() {
    let root = crate::store::test_dir();
    fs::create_dir_all(root.join(PARTIAL)).unwrap();
    fs::write(root.join(PARTIAL).join("ebible~x.zip"), b"half").unwrap();
    clean_partials(&root);
    assert_eq!(partials_left(&root), 0);
    clean_partials(&root);
}

#[test]
fn messages() {
    assert_eq!(InstallError::NoSpace(25).message("X", false), "Not enough disk space for this Bible (needs about 25 MB).");
    assert_eq!(InstallError::NoSpace(25).message("X", true), "Not enough disk space for this Bible (needs about 25 MB).");
    assert_eq!(InstallError::TooLarge.message("X", true), "The update for X couldn't be used. The copy you have still works.");
    assert_eq!(megabytes(1), 1);
    assert_eq!(megabytes(25 << 20), 25);
    assert!(room_for(&std::env::temp_dir(), u64::MAX).is_err_and(|e| matches!(e, InstallError::NoSpace(_))));
    assert!(room_for(&std::env::temp_dir(), 1).is_ok());
}

/// Library spec §14, "Net (gated)": the live eBible list and one real download. Run with `RP_NET_TESTS=1`; it checks
/// the CSV's column names and the zip's entry names and licence page, which the other tests only assume.
#[tokio::test]
async fn net_the_live_catalog_lists_bibles_and_the_greek_sblgnt_downloads_under_cc_by() {
    if std::env::var_os("RP_NET_TESTS").is_none() { return; }
    let root = crate::store::test_dir();
    let mut c = Ctx::new(root.clone(), CatalogSource::Ebible);
    c.force = true;
    let listing = CatalogSource::Ebible.refresh(&c).await.expect("the eBible list loads");
    assert!(!listing.offline, "answered by eBible.org, not the saved list");
    assert!(listing.rows.len() > 300, "only {} Bibles offered; the CSV's columns may have changed", listing.rows.len());
    assert!(listing.rows.iter().any(|r| r.source_id() == "engwebp"), "the World English Bible is offered");
    let row = listing.rows.iter().find(|r| r.source_id() == "grcsbl").expect("the SBL Greek New Testament is offered");
    let m = CatalogSource::Ebible.install(&c, row, &CancellationToken::new(), &|_| {}).await.expect("grcsbl downloads");
    assert!(matches!(&m.licence, crate::bible::library::Licence::Cc { kind, version, .. } if kind == "by" && version == "4.0"), "{:?}", m.licence);
    let b = import::get(&root, "ebible:grcsbl").expect("installed");
    assert!(!b.lookup("John 1:1").unwrap().1[0].text.is_empty());
    assert_eq!(partials_left(&root), 0);
}
