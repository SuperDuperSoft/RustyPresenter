use super::*;
use std::sync::{Arc, Mutex};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener};

/// A cut of the real file's shape: BOM, the real header, a quoted description with a comma, a doubled quote and a
/// line break, and rows for each filter.
const CSV: &str = "\u{feff}languageCode,translationId,languageName,languageNameInEnglish,dialect,homeDomain,title,description,Redistributable,Copyright,UpdateDate,publicationURL,OTbooks,OTchapters,OTverses,NTbooks,NTchapters,NTverses,DCbooks,DCchapters,DCverses,FCBHID,Certified,inScript,swordName,rodCode,textDirection,downloadable,font,shortTitle,PODISBN,script,sourceDate\r\n\
eng,engwebp,English,English,,ebible.org,World English Bible,\"The World English Bible, a \"\"modern\"\" update\nof the ASV\",True,public domain,2026-03-01,,39,929,23145,27,260,7957,0,0,0,,True,,,,ltr,True,,World English Bible,,Latn,\r\n\
spa,spaRV1909,Español,Spanish,,ebible.org,Santa Biblia Reina-Valera 1909,,True,public domain,2025-01-02,,39,929,23145,27,260,7957,0,0,0,,True,,,,ltr,True,,Reina-Valera 1909,,Latn,\r\n\
tpt,tptNT,Tepehua,Tlachichilco Tepehua,,ebible.org,Tlachichilco Tepehua New Testament,,True,Copyright © 2010 Wycliffe,2024-05-05,,0,0,0,27,260,7957,0,0,0,,True,,,,ltr,True,,Tepehua NT,,Latn,\r\n\
eng,enggw,English,English,,ebible.org,GOD'S WORD,,False,Copyright © God's Word to the Nations,2024-01-01,,39,929,23145,27,260,7957,0,0,0,,True,,,,ltr,True,,GOD'S WORD,,Latn,\r\n\
eng,engx,English,English,,ebible.org,Not downloadable,,True,public domain,2024-01-01,,39,929,23145,27,260,7957,0,0,0,,True,,,,ltr,False,,X,,Latn,\r\n\
hbo,hboWLC,עברית,Ancient Hebrew,,ebible.org,Westminster Leningrad Codex,,True,public domain,2024-01-01,,39,929,23145,0,0,0,0,0,0,,True,,,,rtl,True,,WLC,,Hebr,\r\n\
eng,engnwt,English,English,,ebible.org,New World Translation,,True,Watch Tower,2024-01-01,,39,929,23145,27,260,7957,0,0,0,,True,,,,ltr,True,,NWT,,Latn,\r\n\
grc,grcbrent,Ελληνικά,Ancient Greek,,ebible.org,Brenton Septuagint,,True,public domain,2024-01-01,,39,929,23145,0,0,0,15,100,1000,,True,,,,ltr,True,,Brenton,,Grek,\r\n\
eng,engnet,English,English,,ebible.org,NET Bible,,True,Copyright © Biblical Studies Press,2024-01-01,,39,929,23145,27,260,7957,0,0,0,,True,,,,ltr,True,,NET,,Latn,\r\n\
eng,engps,English,English,,ebible.org,A Psalter,,True,public domain,2024-01-01,,1,150,2461,0,0,0,0,0,0,,True,,,,ltr,True,,Psalter,,Latn,\r\n";

fn offered(csv: &str, root: &Path) -> Vec<CatalogRow> {
    let l = Licences::load(root);
    parse(csv.as_bytes()).unwrap().iter().filter_map(|r| offer(r, &l)).collect()
}

fn ids(rows: &[CatalogRow]) -> Vec<&str> { rows.iter().map(|r| r.source_id()).collect() }

#[test]
fn reads_the_header_by_name_with_quotes_line_breaks_and_a_bom() {
    let rows = parse(CSV.as_bytes()).unwrap();
    assert_eq!(rows.len(), 10);
    let web = &rows[0];
    assert_eq!(web.translation_id, "engwebp");
    assert_eq!(web.description, "The World English Bible, a \"modern\" update\nof the ASV");
    assert_eq!((web.redistributable, web.downloadable, web.ot_books, web.nt_books, web.dc_books), (true, true, 39, 27, 0));
    assert_eq!((web.update_date.as_str(), web.text_direction.as_str(), web.script.as_str()), ("2026-03-01", "ltr", "Latn"));
    assert_eq!(rows[1].language_name, "Español");
    // Another column order, LF only, no trailing line break.
    let moved = parse(b"translationId,downloadable,title,languageCode,Redistributable\nabc,True,A B C,abc,True").unwrap();
    assert_eq!((moved[0].translation_id.as_str(), moved[0].title.as_str(), moved[0].downloadable), ("abc", "A B C", true));
    assert!(parse(b"languageCode,title\neng,x").is_err(), "a file without the columns the filter needs");
}

#[test]
fn offers_only_redistributable_downloadable_and_unblocked_rows() {
    let root = crate::store::test_dir();
    let rows = offered(CSV, &root);
    assert_eq!(ids(&rows), ["engwebp", "spaRV1909", "tptNT", "hboWLC", "grcbrent", "engnet", "engps"]);
    let by = |id: &str| rows.iter().find(|r| r.source_id() == id).unwrap();
    assert_eq!(by("engwebp").id(), "ebible:engwebp");
    assert_eq!(by("engwebp").scope(), Scope::Full);
    assert_eq!(by("tptNT").scope(), Scope::NewTestament);
    assert_eq!(by("grcbrent").scope(), Scope::OldTestament);
    assert!(by("grcbrent").has_dc());
    assert_eq!(by("engps").scope(), Scope::Portions);
    assert_eq!(by("tptNT").abbr(), "NT");
    // A right-to-left Bible is offered, and says which way it is written.
    assert_eq!((by("hboWLC").dir(), by("engwebp").dir()), (Dir::Rtl, Dir::Ltr));
}

#[test]
fn abbreviations_follow_the_spec_examples() {
    for (id, lang, abbr) in [("spaRV1909", "spa", "RV1909"), ("hboWLC", "hbo", "WLC"), ("grcbrent", "grc", "BRENT"),
        ("eng-Brenton", "eng", "BRENTON"), ("tptNT", "tpt", "NT"), ("eng", "eng", "ENG"), ("engkjv2006extra", "eng", "KJV2006EXT"),
        ("abc", "", "ABC"), ("x_y", "zz", "X_Y")] {
        assert_eq!(abbreviation(id, lang), abbr, "{id}");
    }
}

#[test]
fn licences_come_from_the_cache_for_the_same_edition_then_the_seed() {
    let root = crate::store::test_dir();
    let rows = offered(CSV, &root);
    let by = |rows: &[CatalogRow], id: &str| rows.iter().find(|r| r.source_id() == id).map(|r| (r.licence().clone(), r.abbr().to_string()));
    assert_eq!(by(&rows, "engnet"), Some((LicenceState::Known(Licence::Net), "NET".into())), "from the seed");
    assert_eq!(by(&rows, "engwebp").unwrap().0, LicenceState::OnDownload);

    Licences::record(&root, "engwebp", Checked { update_date: "2026-03-01".into(), licence: Some(Licence::Pd) }).unwrap();
    Licences::record(&root, "spaRV1909", Checked { update_date: "2025-01-02".into(), licence: None }).unwrap();
    Licences::record(&root, "engnet", Checked { update_date: "2020-01-01".into(), licence: None }).unwrap();
    let rows = offered(CSV, &root);
    assert_eq!(by(&rows, "engwebp").unwrap().0, LicenceState::Known(Licence::Pd));
    assert_eq!(by(&rows, "spaRV1909"), None, "refused for this edition: hidden");
    assert_eq!(by(&rows, "engnet").unwrap().0, LicenceState::Known(Licence::Net), "refused for an older edition only: checked again");

    let newer = CSV.replace("2025-01-02", "2026-06-06");
    assert_eq!(by(&offered(&newer, &root), "spaRV1909").unwrap().0, LicenceState::OnDownload, "a new edition is checked again");
}

#[test]
fn the_shipped_seed_reads() {
    let seed: Seed = serde_json::from_str(SEED).unwrap();
    assert_eq!(seed.v, 1);
    assert!(seed.bibles.values().all(|e| e.licence.as_ref().is_some_and(|l| *l != Licence::Unknown)), "every seed entry names a licence");
}

/// A one-request-at-a-time HTTP server that answers like Cloudflare: 304 when the ETag matches.
struct Server { base: String, hits: Arc<Mutex<Vec<String>>>, body: Arc<Mutex<(u16, String)>> }

async fn server(body: &str) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(Mutex::new(Vec::new()));
    let body = Arc::new(Mutex::new((200u16, body.to_string())));
    let (h, b) = (hits.clone(), body.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut req = Vec::new();
            let mut buf = [0u8; 4096];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                match sock.read(&mut buf).await { Ok(0) | Err(_) => break, Ok(n) => req.extend_from_slice(&buf[..n]) }
            }
            let req = String::from_utf8_lossy(&req).to_lowercase();
            h.lock().unwrap().push(req.clone());
            let (status, text) = b.lock().unwrap().clone();
            let reply = if req.contains("if-none-match: \"v1\"") && status == 200 {
                "HTTP/1.1 304 Not Modified\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_string()
            } else {
                format!("HTTP/1.1 {status} X\r\netag: \"v1\"\r\nlast-modified: Wed, 01 Oct 2026 00:00:00 GMT\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}", text.len())
            };
            let _ = sock.write_all(reply.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    Server { base, hits, body }
}

fn ctx(root: &Path, base: &str, now: i64) -> Ctx { Ctx { root: root.to_path_buf(), now, force: false, base: base.into() } }

#[tokio::test]
async fn fetches_once_a_day_with_a_conditional_get_and_keeps_the_list_offline() {
    let root = crate::store::test_dir();
    let s = server(CSV).await;
    let t0 = 1_800_000_000;

    let first = refresh(&ctx(&root, &s.base, t0)).await.unwrap();
    assert_eq!((first.rows.len(), first.offline), (7, false));
    assert!(root.join("bibles/catalog/ebible-translations.csv").exists());
    assert!(s.hits.lock().unwrap()[0].starts_with("get /translations.csv "));
    assert!(!s.hits.lock().unwrap()[0].contains("if-none-match"), "nothing saved yet, so ask plainly");

    refresh(&ctx(&root, &s.base, t0 + 3600)).await.unwrap();
    assert_eq!(s.hits.lock().unwrap().len(), 1, "less than a day later: the saved list, no request");

    let next_day = refresh(&ctx(&root, &s.base, t0 + DAY + 1)).await.unwrap();
    assert_eq!(next_day.rows.len(), 7);
    let hits = s.hits.lock().unwrap().clone();
    assert_eq!(hits.len(), 2);
    assert!(hits[1].contains("if-none-match: \"v1\"") && hits[1].contains("if-modified-since: wed, 01 oct 2026"), "{}", hits[1]);

    refresh(&ctx(&root, &s.base, t0 + DAY + 2)).await.unwrap();
    assert_eq!(s.hits.lock().unwrap().len(), 2, "the 304 counts as today's check");

    let mut forced = ctx(&root, &s.base, t0 + DAY + 3);
    forced.force = true;
    refresh(&forced).await.unwrap();
    assert_eq!(s.hits.lock().unwrap().len(), 3, "force asks even within the day");

    // The source is gone: the saved list shows, marked offline.
    let gone = refresh(&ctx(&root, "http://127.0.0.1:9", t0 + 3 * DAY)).await.unwrap();
    assert_eq!((gone.rows.len(), gone.offline), (7, true));
}

#[tokio::test]
async fn a_broken_answer_never_replaces_the_saved_list() {
    let root = crate::store::test_dir();
    let s = server(CSV).await;
    let t0 = 1_800_000_000;
    refresh(&ctx(&root, &s.base, t0)).await.unwrap();

    *s.body.lock().unwrap() = (200, "<html>maintenance</html>".into());
    // A new ETag would be needed for a 200 here; the server only says 304 to "v1", so drop the saved ETag.
    let state = root.join("bibles/catalog/ebible.json");
    fs::write(&state, br#"{"etag":"","last_modified":"","checked_at":0}"#).unwrap();
    let r = refresh(&ctx(&root, &s.base, t0 + 2 * DAY)).await.unwrap();
    assert_eq!((r.rows.len(), r.offline), (7, true), "the saved list, with Add turned off");
    assert_eq!(fs::read_to_string(root.join("bibles/catalog/ebible-translations.csv")).unwrap(), CSV, "the good list stays");

    *s.body.lock().unwrap() = (503, String::new());
    let r = refresh(&ctx(&root, &s.base, t0 + 3 * DAY)).await;
    assert!(matches!(r, Ok(Listing { offline: true, .. })), "a server error with a saved list shows the saved list");
}

#[tokio::test]
async fn no_internet_and_nothing_saved_says_offline() {
    let root = crate::store::test_dir();
    assert_eq!(refresh(&ctx(&root, "http://127.0.0.1:9", 1_800_000_000)).await.unwrap_err(), CatalogError::Offline);
    let s = server("<html>maintenance</html>").await;
    assert!(matches!(refresh(&ctx(&root, &s.base, 1_800_000_000)).await, Err(CatalogError::Bad(_))));
    assert!(!root.join("bibles/catalog/ebible-translations.csv").exists());
}
