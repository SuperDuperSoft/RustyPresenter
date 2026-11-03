//! What kind of file is this? Decided by content, never by name (Plan 4 Task 3). LibreOffice opens a plain-text
//! or empty file named `.pptx` as a text document and makes a one-page PDF with exit 0 (check C5 gotcha 2).
use quick_xml::events::{BytesStart, Event};
use std::io::{self, Read, Seek};
use std::path::Path;

pub use super::strings::{ENCRYPTED, NOT_PRESENTATION};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Pptx,
    Ppsx,
    Potx,
    Pptm,
}

#[derive(Debug, PartialEq)]
pub enum Kind {
    Pdf,
    /// `slide_size` is `<p:sldSz cx cy>` in EMU; `slides` counts `<p:sldIdLst>`. Both are `None` when
    /// `presentation.xml` could not be read.
    Ooxml { variant: Variant, slide_size: Option<(u64, u64)>, slides: Option<usize>, hidden: usize },
    Ppt,
    Odp,
    Keynote,
    Encrypted,
    NotPresentation,
}

/// Parts bigger than this uncompressed are not parsed (zip-bomb guard).
const MAX_PART: u64 = 50 * 1024 * 1024;
const OLE_MAGIC: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";
const ODP_MIME: &[u8] = b"application/vnd.oasis.opendocument.presentation";
const PRESENTATION_PART: &str = "/ppt/presentation.xml";

/// Only opening and reading the file are errors. A broken zip or compound file is `NotPresentation`.
pub fn sniff(path: &Path) -> io::Result<Kind> {
    sniff_reader(io::BufReader::new(std::fs::File::open(path)?))
}

pub fn sniff_reader<R: Read + Seek>(mut r: R) -> io::Result<Kind> {
    r.seek(io::SeekFrom::Start(0))?;
    let mut magic = Vec::with_capacity(8);
    r.by_ref().take(8).read_to_end(&mut magic)?;
    r.seek(io::SeekFrom::Start(0))?;
    Ok(if magic.starts_with(b"%PDF-") {
        Kind::Pdf
    } else if magic.starts_with(b"PK\x03\x04") {
        sniff_zip(r)
    } else if magic == OLE_MAGIC {
        sniff_cfb(r)
    } else {
        Kind::NotPresentation
    })
}

/// The extension a staged copy gets, so converters that do look at names see the right one.
pub fn staged_ext(kind: &Kind) -> Option<&'static str> {
    Some(match kind {
        Kind::Pdf => "pdf",
        Kind::Ooxml { variant: Variant::Pptx, .. } => "pptx",
        Kind::Ooxml { variant: Variant::Ppsx, .. } => "ppsx",
        Kind::Ooxml { variant: Variant::Potx, .. } => "potx",
        Kind::Ooxml { variant: Variant::Pptm, .. } => "pptm",
        Kind::Ppt => "ppt",
        Kind::Odp => "odp",
        Kind::Keynote => "key",
        Kind::Encrypted | Kind::NotPresentation => return None,
    })
}

/// A password-protected OOXML file is a compound file holding the encrypted package.
fn sniff_cfb<R: Read + Seek>(r: R) -> Kind {
    let Ok(c) = cfb::CompoundFile::open(r) else { return Kind::NotPresentation };
    if c.is_stream("/EncryptionInfo") && c.is_stream("/EncryptedPackage") {
        Kind::Encrypted
    } else if c.is_stream("/PowerPoint Document") {
        Kind::Ppt
    } else {
        Kind::NotPresentation
    }
}

fn sniff_zip<R: Read + Seek>(r: R) -> Kind {
    let Ok(mut zip) = zip::ZipArchive::new(r) else { return Kind::NotPresentation };
    if read_part(&mut zip, "mimetype", 256).is_some_and(|m| m.trim_ascii() == ODP_MIME) {
        return Kind::Odp;
    }
    if let Some(variant) = read_part(&mut zip, "[Content_Types].xml", MAX_PART).and_then(|t| ooxml_variant(&t)) {
        return ooxml(&mut zip, variant);
    }
    if zip.index_for_name("Index/Document.iwa").is_some() {
        return Kind::Keynote;
    }
    Kind::NotPresentation
}

/// A whole zip entry, or `None` when it is missing, unreadable or over `limit` bytes.
fn read_part<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str, limit: u64) -> Option<Vec<u8>> {
    let f = zip.by_name(name).ok()?;
    if f.size() > limit {
        return None;
    }
    let mut buf = Vec::new();
    // The declared size may lie; never read past the limit.
    f.take(limit + 1).read_to_end(&mut buf).ok()?;
    (buf.len() as u64 <= limit).then_some(buf)
}

/// The variant from the content type `[Content_Types].xml` gives `/ppt/presentation.xml`.
fn ooxml_variant(types: &[u8]) -> Option<Variant> {
    let mut found = None;
    each_element(types, |name, e| {
        if name == b"Override" && attr(e, b"PartName").is_some_and(|p| p.eq_ignore_ascii_case(PRESENTATION_PART)) {
            found = attr(e, b"ContentType");
        }
        found.is_none()
    })?;
    match found?.as_str() {
        "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml" => Some(Variant::Pptx),
        "application/vnd.openxmlformats-officedocument.presentationml.slideshow.main+xml" => Some(Variant::Ppsx),
        "application/vnd.openxmlformats-officedocument.presentationml.template.main+xml" => Some(Variant::Potx),
        // Macro-enabled shows and templates are treated like `.pptm`: they carry macros too.
        "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml"
        | "application/vnd.ms-powerpoint.slideshow.macroEnabled.main+xml"
        | "application/vnd.ms-powerpoint.template.macroEnabled.main+xml" => Some(Variant::Pptm),
        _ => None,
    }
}

fn ooxml<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, variant: Variant) -> Kind {
    let (slide_size, ids) = read_part(zip, "ppt/presentation.xml", MAX_PART).and_then(|p| presentation(&p)).unwrap_or_default();
    let hidden = match &ids {
        Some(ids) => {
            let rels = read_part(zip, "ppt/_rels/presentation.xml.rels", MAX_PART).map(|r| relationships(&r)).unwrap_or_default();
            ids.iter().filter_map(|id| rels.iter().find(|(rid, _)| rid == id)).filter(|(_, target)| slide_hidden(zip, target)).count()
        }
        None => 0,
    };
    Kind::Ooxml { variant, slide_size, slides: ids.as_ref().map(Vec::len), hidden }
}

/// `<p:sldSz cx cy>` and the relationship ids of `<p:sldIdLst>`, in order.
fn presentation(xml: &[u8]) -> Option<(Option<(u64, u64)>, Option<Vec<String>>)> {
    let (mut size, mut ids, mut in_list) = (None, Vec::new(), false);
    each_event(xml, |ev| {
        match ev {
            Ev::Open(b"sldIdLst", _) => in_list = true,
            Ev::Close(b"sldIdLst") => in_list = false,
            Ev::Open(b"sldId", e) if in_list => ids.extend(rel_id(e)),
            Ev::Open(b"sldSz", e) => {
                let num = |k| attr(e, k).and_then(|v| v.trim().parse::<u64>().ok());
                size = num(b"cx").zip(num(b"cy"));
            }
            _ => {}
        }
        true
    })?;
    Some((size, Some(ids)))
}

/// `Id → zip entry` for every relationship of `presentation.xml`. Targets are relative to `ppt/`.
fn relationships(xml: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let _ = each_element(xml, |name, e| {
        if name == b"Relationship" && let (Some(id), Some(target)) = (attr(e, b"Id"), attr(e, b"Target")) {
            let entry = match target.strip_prefix('/') {
                Some(abs) => abs.to_string(),
                None => format!("ppt/{target}"),
            };
            out.push((id, entry));
        }
        true
    });
    out
}

/// A slide is hidden when its root is `<p:sld show="0">`. Only the root element is read.
fn slide_hidden<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, entry: &str) -> bool {
    let Ok(f) = zip.by_name(entry) else { return false };
    let mut r = quick_xml::Reader::from_reader(io::BufReader::new(f.take(MAX_PART)));
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                return e.local_name().as_ref() == b"sld" && attr(&e, b"show").is_some_and(|v| matches!(v.trim(), "0" | "false"));
            }
            Ok(Event::Eof) | Err(_) => return false,
            _ => buf.clear(),
        }
    }
}

enum Ev<'a, 'b> {
    Open(&'a [u8], &'a BytesStart<'b>),
    Close(&'a [u8]),
}

/// Calls `f` for each opening and closing tag (by local name, without the namespace prefix) until it returns
/// `false`. `None` when the XML is broken.
fn each_event(xml: &[u8], mut f: impl FnMut(Ev) -> bool) -> Option<()> {
    let mut r = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    loop {
        let go = match r.read_event_into(&mut buf).ok()? {
            Event::Start(e) => f(Ev::Open(e.local_name().as_ref(), &e)),
            Event::Empty(e) => f(Ev::Open(e.local_name().as_ref(), &e)) && f(Ev::Close(e.local_name().as_ref())),
            Event::End(e) => f(Ev::Close(e.local_name().as_ref())),
            Event::Eof => return Some(()),
            _ => true,
        };
        if !go {
            return Some(());
        }
        buf.clear();
    }
}

fn each_element(xml: &[u8], mut f: impl FnMut(&[u8], &BytesStart) -> bool) -> Option<()> {
    each_event(xml, |ev| match ev {
        Ev::Open(name, e) => f(name, e),
        Ev::Close(_) => true,
    })
}

/// An attribute by local name. Values here are names, numbers and content types, which never need unescaping.
fn attr(e: &BytesStart, name: &[u8]) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.local_name().as_ref() == name).map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

/// `r:id` of a `<p:sldId>`, which also has a plain `id`.
fn rel_id(e: &BytesStart) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.prefix().is_some() && a.key.local_name().as_ref() == b"id").map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    const PPTX_MAIN: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
    const PPSX_MAIN: &str = "application/vnd.openxmlformats-officedocument.presentationml.slideshow.main+xml";
    const PPTM_MAIN: &str = "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml";

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn content_types(part: &str, ct: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="{part}" ContentType="{ct}"/></Types>"#
        )
    }

    fn presentation(cx: u64, cy: u64, slides: usize) -> String {
        let ids: String = (0..slides).map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + i, i + 2)).collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{cx}" cy="{cy}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#
        )
    }

    fn rels(slides: usize) -> String {
        let r: String = (0..slides)
            .map(|i| format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{}.xml"/>"#, i + 2, i + 1))
            .collect();
        format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>{r}</Relationships>"#)
    }

    fn slide(shown: bool) -> String {
        let show = if shown { "" } else { r#" show="0""# };
        format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"{show}><p:cSld/></p:sld>"#)
    }

    /// A package with one slide per entry of `shown`.
    fn pptx(ct: &str, cx: u64, cy: u64, shown: &[bool]) -> Vec<u8> {
        let types = content_types("/ppt/presentation.xml", ct);
        let pres = presentation(cx, cy, shown.len());
        let rels = rels(shown.len());
        let slides: Vec<(String, String)> = shown.iter().enumerate().map(|(i, s)| (format!("ppt/slides/slide{}.xml", i + 1), slide(*s))).collect();
        let mut entries: Vec<(&str, &[u8])> = vec![
            ("[Content_Types].xml", types.as_bytes()),
            ("ppt/presentation.xml", pres.as_bytes()),
            ("ppt/_rels/presentation.xml.rels", rels.as_bytes()),
        ];
        entries.extend(slides.iter().map(|(n, x)| (n.as_str(), x.as_bytes())));
        zip_of(&entries)
    }

    fn cfb_of(streams: &[&str]) -> Vec<u8> {
        let mut c = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        for s in streams {
            c.create_stream(s).unwrap().write_all(b"stream data").unwrap();
        }
        c.flush().unwrap();
        c.into_inner().into_inner()
    }

    fn kind(bytes: Vec<u8>) -> Kind {
        sniff_reader(Cursor::new(bytes)).unwrap()
    }

    #[test]
    fn pdf_magic() {
        assert_eq!(kind(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n1 0 obj".to_vec()), Kind::Pdf);
        assert_eq!(staged_ext(&Kind::Pdf), Some("pdf"));
    }

    #[test]
    fn minimal_pptx_with_16x9_size_and_3_slides() {
        let k = kind(pptx(PPTX_MAIN, 12192000, 6858000, &[true, true, true]));
        assert_eq!(k, Kind::Ooxml { variant: Variant::Pptx, slide_size: Some((12192000, 6858000)), slides: Some(3), hidden: 0 });
        assert_eq!(staged_ext(&k), Some("pptx"));
        // The same bytes through a file whose name says nothing.
        let d = crate::store::test_dir();
        let p = d.join("upload.bin");
        std::fs::write(&p, pptx(PPTX_MAIN, 12192000, 6858000, &[true, true, true])).unwrap();
        assert_eq!(sniff(&p).unwrap(), k);
    }

    #[test]
    fn ppsx_and_pptm_variants() {
        let variant = |ct| match kind(pptx(ct, 9144000, 6858000, &[true])) {
            Kind::Ooxml { variant, slide_size, .. } => {
                assert_eq!(slide_size, Some((9144000, 6858000)));
                variant
            }
            k => panic!("{k:?}"),
        };
        assert_eq!(variant(PPSX_MAIN), Variant::Ppsx);
        assert_eq!(variant(PPTM_MAIN), Variant::Pptm);
        assert_eq!(variant("application/vnd.openxmlformats-officedocument.presentationml.template.main+xml"), Variant::Potx);
        // Macro-enabled shows and templates carry macros too.
        assert_eq!(variant("application/vnd.ms-powerpoint.slideshow.macroEnabled.main+xml"), Variant::Pptm);
        assert_eq!(variant("application/vnd.ms-powerpoint.template.macroEnabled.main+xml"), Variant::Pptm);
        let ext = |variant| staged_ext(&Kind::Ooxml { variant, slide_size: None, slides: None, hidden: 0 });
        assert_eq!([ext(Variant::Pptx), ext(Variant::Ppsx), ext(Variant::Potx), ext(Variant::Pptm)], [Some("pptx"), Some("ppsx"), Some("potx"), Some("pptm")]);
    }

    #[test]
    fn hidden_slides_counted() {
        let k = kind(pptx(PPTX_MAIN, 12192000, 6858000, &[true, false, true, false, false]));
        assert_eq!(k, Kind::Ooxml { variant: Variant::Pptx, slide_size: Some((12192000, 6858000)), slides: Some(5), hidden: 3 });
    }

    #[test]
    fn odp_mimetype() {
        let k = kind(zip_of(&[
            ("mimetype", b"application/vnd.oasis.opendocument.presentation"),
            ("content.xml", b"<office:document-content/>"),
        ]));
        assert_eq!(k, Kind::Odp);
        assert_eq!(staged_ext(&k), Some("odp"));
        let text = kind(zip_of(&[("mimetype", b"application/vnd.oasis.opendocument.text")]));
        assert_eq!(text, Kind::NotPresentation);
    }

    #[test]
    fn keynote_index() {
        let k = kind(zip_of(&[("Index/Document.iwa", b"\x00\x01iwa"), ("Metadata/Properties.plist", b"plist")]));
        assert_eq!(k, Kind::Keynote);
        assert_eq!(staged_ext(&k), Some("key"));
    }

    #[test]
    fn ppt_cfb() {
        let k = kind(cfb_of(&["/PowerPoint Document", "/Current User", "/\u{5}SummaryInformation"]));
        assert_eq!(k, Kind::Ppt);
        assert_eq!(staged_ext(&k), Some("ppt"));
    }

    #[test]
    fn encrypted_cfb() {
        let k = kind(cfb_of(&["/EncryptionInfo", "/EncryptedPackage"]));
        assert_eq!(k, Kind::Encrypted);
        assert_eq!(staged_ext(&k), None);
        assert!(ENCRYPTED.contains("File → Info → Protect"));
        // Word and Excel files are compound files too.
        assert_eq!(kind(cfb_of(&["/WordDocument"])), Kind::NotPresentation);
    }

    #[test]
    fn docx_is_not_presentation() {
        let ct = content_types("/word/document.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml");
        let k = kind(zip_of(&[("[Content_Types].xml", ct.as_bytes()), ("word/document.xml", b"<w:document/>")]));
        assert_eq!(k, Kind::NotPresentation);
        assert_eq!(staged_ext(&k), None);
    }

    #[test]
    fn xlsx_is_not_presentation() {
        let ct = content_types("/xl/workbook.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml");
        assert_eq!(kind(zip_of(&[("[Content_Types].xml", ct.as_bytes()), ("xl/workbook.xml", b"<workbook/>")])), Kind::NotPresentation);
    }

    #[test]
    fn plain_text_named_pptx_is_not_presentation() {
        let d = crate::store::test_dir();
        let p = d.join("Sunday.pptx");
        std::fs::write(&p, "Amazing grace, how sweet the sound\r\n").unwrap();
        assert_eq!(sniff(&p).unwrap(), Kind::NotPresentation);
        assert_eq!(NOT_PRESENTATION, "This doesn't look like a PowerPoint file.");
    }

    #[test]
    fn empty_file_is_not_presentation() {
        let d = crate::store::test_dir();
        let p = d.join("empty.pptx");
        std::fs::write(&p, b"").unwrap();
        assert_eq!(sniff(&p).unwrap(), Kind::NotPresentation);
        // Shorter than any magic number.
        assert_eq!(kind(b"PK".to_vec()), Kind::NotPresentation);
        assert!(sniff(&d.join("missing.pptx")).is_err());
    }

    #[test]
    fn empty_zip_is_not_presentation() {
        let empty = zip_of(&[]);
        assert!(empty.starts_with(b"PK\x05\x06"), "{:?}", &empty[..4]);
        assert_eq!(kind(empty), Kind::NotPresentation);
        // A zip signature on a broken archive.
        assert_eq!(kind(b"PK\x03\x04 not really a zip".to_vec()), Kind::NotPresentation);
        assert_eq!(kind(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1 not really a compound file".to_vec()), Kind::NotPresentation);
    }

    #[test]
    fn oversized_presentation_xml_gives_no_size() {
        let types = content_types("/ppt/presentation.xml", PPTX_MAIN);
        let mut pres = presentation(12192000, 6858000, 1).into_bytes();
        pres.resize(50 * 1024 * 1024 + 1, b' ');
        let k = kind(zip_of(&[("[Content_Types].xml", types.as_bytes()), ("ppt/presentation.xml", &pres)]));
        assert_eq!(k, Kind::Ooxml { variant: Variant::Pptx, slide_size: None, slides: None, hidden: 0 });
    }

    /// Real files written by LibreOffice, plus the committed check C5 sample. Needs LibreOffice installed.
    #[tokio::test]
    async fn libreoffice_files_sniff_by_content() {
        if std::env::var("RP_TEST_SOFFICE").as_deref() != Ok("1") { return }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments").join("media");
        let size_16x9 = Some((10080625, 5670550));
        let sample = sniff(&root.join("sample.pptx")).unwrap();
        assert_eq!(sample, Kind::Ooxml { variant: Variant::Pptx, slide_size: size_16x9, slides: Some(3), hidden: 0 });

        let soffice = crate::platform::tools::find_soffice().expect("LibreOffice not found");
        let d = crate::store::test_dir();
        let profile = crate::platform::tools::file_url(&d.join("profile"), true).unwrap();
        for (fmt, want) in [("pptx", sample), ("odp", Kind::Odp), ("ppt", Kind::Ppt), ("pdf", Kind::Pdf)] {
            let mut cmd = tokio::process::Command::new(&soffice.path);
            cmd.args(["--headless", "--norestore", "--nologo", "--nodefault", "--nolockcheck"])
                .arg(format!("-env:UserInstallation={profile}"))
                .args(["--convert-to", fmt, "--outdir"])
                .arg(&d)
                .arg(root.join("sample.fodp"))
                .env_remove("PYTHONHOME")
                .env_remove("PYTHONPATH")
                .stdout(std::process::Stdio::null());
            let mut tree = crate::platform::proc::spawn_tree(cmd).unwrap();
            let status = tree.wait(std::time::Duration::from_secs(180), &tokio_util::sync::CancellationToken::new()).await.unwrap();
            assert!(status.success(), "{fmt}: {status}");
            let got = sniff(&d.join(format!("sample.{fmt}"))).unwrap();
            assert_eq!(got, want, "{fmt}");
        }
    }
}
