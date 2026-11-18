//! What kind of file is this? Decided by content, never by name (Plan 4 Task 3). LibreOffice opens a plain-text
//! or empty file named `.pptx` as a text document and makes a one-page PDF with exit 0 (check C5 gotcha 2).
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::io::{self, Read, Seek};
use std::path::Path;

pub use super::strings::{encrypted, not_presentation};

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
pub(super) const MAX_PART: u64 = 50 * 1024 * 1024;
/// At most this much of one slide is inflated to find its root, which comes right after the XML prolog.
const SLIDE_HEAD: u64 = 64 * 1024;
const OLE_MAGIC: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";
const ODP_MIME: &[u8] = b"application/vnd.oasis.opendocument.presentation";
const PRESENTATION_PART: &str = "/ppt/presentation.xml";
/// `headerToken` of the `Current User` stream's CurrentUserAtom in an encrypted `.ppt` (MS-PPT 2.3.2).
const PPT_ENCRYPTED_TOKEN: [u8; 4] = 0xF3D1_C4DF_u32.to_le_bytes();

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

/// A password-protected OOXML file is a compound file holding the encrypted package. Word and Excel files look
/// the same until the password is known, so they get the password message too.
fn sniff_cfb<R: Read + Seek>(r: R) -> Kind {
    let Ok(mut c) = cfb::CompoundFile::open(r) else { return Kind::NotPresentation };
    if c.is_stream("/EncryptionInfo") && c.is_stream("/EncryptedPackage") {
        Kind::Encrypted
    } else if c.is_stream("/PowerPoint Document") {
        if c.is_stream("/EncryptedSummary") || ppt_encrypted(&mut c) { Kind::Encrypted } else { Kind::Ppt }
    } else {
        Kind::NotPresentation
    }
}

/// A password-protected `.ppt` keeps its streams; the `Current User` header token says so. The token follows
/// the 8-byte record header and the 4-byte size.
fn ppt_encrypted<F: Read + Seek>(c: &mut cfb::CompoundFile<F>) -> bool {
    let mut head = Vec::with_capacity(16);
    c.open_stream("/Current User").is_ok_and(|s| s.take(16).read_to_end(&mut head).is_ok()) && head.get(12..16) == Some(&PPT_ENCRYPTED_TOKEN[..])
}

fn sniff_zip<R: Read + Seek>(r: R) -> Kind {
    let Ok(mut zip) = zip::ZipArchive::new(r) else { return Kind::NotPresentation };
    if read_part(&mut zip, "mimetype", 256).is_some_and(|m| m.trim_ascii() == ODP_MIME) {
        // A password leaves `mimetype` readable; the manifest says how each encrypted entry was encrypted.
        let encrypted = read_part(&mut zip, "META-INF/manifest.xml", MAX_PART).is_some_and(|m| odf_encrypted(&m));
        return if encrypted { Kind::Encrypted } else { Kind::Odp };
    }
    if let Some(variant) = read_part(&mut zip, "[Content_Types].xml", MAX_PART).and_then(|t| ooxml_variant(&t)) {
        return ooxml(&mut zip, variant);
    }
    // Pages and Numbers keep `Index/Document.iwa` too, and LibreOffice opens those as well. Only Keynote has slides.
    if zip.index_for_name("Index/Document.iwa").is_some() && zip.file_names().flatten().any(|n| n.starts_with("Index/Slide") || n.starts_with("Index/MasterSlide")) {
        return Kind::Keynote;
    }
    Kind::NotPresentation
}

/// A whole zip entry, or `None` when it is missing, unreadable or over `limit` bytes.
pub(super) fn read_part<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str, limit: u64) -> Option<Vec<u8>> {
    let f = zip.by_name(name).ok()?;
    if f.size() > limit {
        return None;
    }
    let mut buf = Vec::new();
    // The declared size may lie; never read past the limit. (`zip` also fails a read that passes the declared
    // size, so today this is a second guard.)
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
    // Part names ignore case (OPC); zip entry names don't. Lower-case name → entry name.
    let names: HashMap<String, String> = zip.file_names().flatten().map(|n| (n.to_ascii_lowercase(), n.into_owned())).collect();
    let part = |zip: &mut zip::ZipArchive<R>, name: &str| names.get(name).and_then(|n| read_part(zip, n, MAX_PART));
    let (slide_size, ids) = part(zip, "ppt/presentation.xml").and_then(|p| presentation(&p)).unwrap_or_default();
    let hidden = match &ids {
        Some(ids) => {
            let rels = part(zip, "ppt/_rels/presentation.xml.rels").map(|r| relationships(&r)).unwrap_or_default();
            hidden_slides(zip, &names, ids, &rels)
        }
        None => 0,
    };
    Kind::Ooxml { variant, slide_size, slides: ids.as_ref().map(Vec::len), hidden }
}

/// How many of `ids` point at a hidden slide. Each slide is read once however often it is listed, and all of
/// them together inflate at most `MAX_PART`; past that the count gives up and says 0.
fn hidden_slides<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, names: &HashMap<String, String>, ids: &[String], rels: &HashMap<String, String>) -> usize {
    let (mut budget, mut known, mut count) = (MAX_PART, HashMap::<&str, bool>::new(), 0);
    for target in ids.iter().filter_map(|id| rels.get(id)) {
        let hidden = match known.get(target.as_str()) {
            Some(&hidden) => hidden,
            None => {
                let hidden = match names.get(target) {
                    Some(entry) => match slide_hidden(zip, entry, &mut budget) {
                        Some(hidden) => hidden,
                        None => return 0,
                    },
                    None => false,
                };
                *known.entry(target).or_insert(hidden)
            }
        };
        count += usize::from(hidden);
    }
    count
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

/// `Id → lower-case zip entry` for every relationship of `presentation.xml`. Targets are relative to `ppt/`.
fn relationships(xml: &[u8]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let _ = each_element(xml, |name, e| {
        if name == b"Relationship" && let (Some(id), Some(target)) = (attr(e, b"Id"), attr(e, b"Target")) {
            let entry = match target.strip_prefix('/') {
                Some(abs) => abs.to_string(),
                None => format!("ppt/{target}"),
            };
            out.entry(id).or_insert(entry.to_ascii_lowercase());
        }
        true
    });
    out
}

/// A slide is hidden when its root is `<p:sld show="0">`. At most `SLIDE_HEAD` bytes are read, taken from
/// `budget`. `None` when the budget ran out before the root was found.
fn slide_hidden<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, entry: &str, budget: &mut u64) -> Option<bool> {
    let Ok(f) = zip.by_name(entry) else { return Some(false) };
    let given = SLIDE_HEAD.min(*budget);
    let mut r = quick_xml::Reader::from_reader(io::BufReader::new(f.take(given)));
    let mut buf = Vec::new();
    let hidden = loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                break Some(e.local_name().as_ref() == b"sld" && attr(&e, b"show").is_some_and(|v| matches!(v.trim(), "0" | "false")));
            }
            Ok(Event::Eof) | Err(_) => break None,
            _ => buf.clear(),
        }
    };
    *budget -= given - r.into_inner().into_inner().limit();
    match hidden {
        None if *budget == 0 => None,
        hidden => Some(hidden.unwrap_or(false)),
    }
}

/// A manifest with a `<manifest:encryption-data>` belongs to a password-protected ODF document.
fn odf_encrypted(manifest: &[u8]) -> bool {
    let mut found = false;
    let _ = each_element(manifest, |name, _| {
        found = name == b"encryption-data";
        !found
    });
    found
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
        let k = kind(zip_of(&[("Index/Document.iwa", b"\x00\x01iwa"), ("Index/Slide.iwa", b"\x00\x01iwa"), ("Metadata/Properties.plist", b"plist")]));
        assert_eq!(k, Kind::Keynote);
        assert_eq!(staged_ext(&k), Some("key"));
        let master = kind(zip_of(&[("Index/Document.iwa", b"iwa"), ("Index/MasterSlide-12.iwa", b"iwa")]));
        assert_eq!(master, Kind::Keynote);
    }

    #[test]
    fn pages_and_numbers_are_not_keynote() {
        let pages = kind(zip_of(&[("Index/Document.iwa", b"iwa"), ("Index/DocumentStylesheet.iwa", b"iwa"), ("Metadata/Properties.plist", b"plist")]));
        assert_eq!(pages, Kind::NotPresentation);
        let numbers = kind(zip_of(&[("Index/Document.iwa", b"iwa"), ("Index/CalculationEngine.iwa", b"iwa"), ("Index/Tables/Tile.iwa", b"iwa")]));
        assert_eq!(numbers, Kind::NotPresentation);
    }

    /// `Current User` starts with a CurrentUserAtom: record header (recType 0x0FF6), size 0x14, header token.
    fn current_user(token: u32) -> Vec<u8> {
        let mut atom = vec![0, 0, 0xF6, 0x0F, 0x1A, 0, 0, 0, 0x14, 0, 0, 0];
        atom.extend(token.to_le_bytes());
        atom.extend([0; 18]);
        atom
    }

    fn cfb_with(streams: &[(&str, &[u8])]) -> Vec<u8> {
        let mut c = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        for (s, data) in streams {
            c.create_stream(s).unwrap().write_all(data).unwrap();
        }
        c.flush().unwrap();
        c.into_inner().into_inner()
    }

    #[test]
    fn encrypted_legacy_ppt_cfb() {
        let plain = cfb_with(&[("/PowerPoint Document", b"records"), ("/Current User", &current_user(0xE391_C05F))]);
        assert_eq!(kind(plain), Kind::Ppt);
        let encrypted = cfb_with(&[("/PowerPoint Document", b"records"), ("/Current User", &current_user(0xF3D1_C4DF))]);
        assert_eq!(kind(encrypted), Kind::Encrypted);
        let summary = cfb_with(&[("/PowerPoint Document", b"records"), ("/Current User", b"short"), ("/EncryptedSummary", b"x")]);
        assert_eq!(kind(summary), Kind::Encrypted);
    }

    #[test]
    fn encrypted_odp() {
        let manifest = |extra: &str| {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.presentation"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml" manifest:size="1234">{extra}</manifest:file-entry></manifest:manifest>"#
            )
        };
        let odp = |m: String| kind(zip_of(&[("mimetype", ODP_MIME), ("META-INF/manifest.xml", m.as_bytes()), ("content.xml", b"\x8f\x12")]));
        assert_eq!(odp(manifest("")), Kind::Odp);
        let enc = r#"<manifest:encryption-data manifest:checksum-type="SHA1/1K" manifest:checksum="x"><manifest:algorithm manifest:algorithm-name="http://www.w3.org/2001/04/xmlenc#aes256-cbc" manifest:initialisation-vector="y"/></manifest:encryption-data>"#;
        assert_eq!(odp(manifest(enc)), Kind::Encrypted);
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
        assert!(encrypted().contains("File → Info → Protect"));
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
        assert_eq!(not_presentation(), "This doesn't look like a PowerPoint file.");
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

    /// Sets the uncompressed size both zip headers declare for every entry.
    fn declare_size(mut bytes: Vec<u8>, size: u32) -> Vec<u8> {
        for (sig, at) in [(b"PK\x03\x04", 22), (b"PK\x01\x02", 24)] {
            let starts: Vec<usize> = bytes.windows(4).enumerate().filter(|(_, w)| w == sig).map(|(i, _)| i).collect();
            for i in starts {
                bytes[i + at..i + at + 4].copy_from_slice(&size.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn part_bigger_than_its_declared_size_is_refused() {
        let honest = zip_of(&[("a.xml", &[b'x'; 1000])]);
        let lying = declare_size(honest, 10);
        let mut zip = zip::ZipArchive::new(Cursor::new(lying)).unwrap();
        assert_eq!(zip.by_name("a.xml").unwrap().size(), 10);
        assert_eq!(read_part(&mut zip, "a.xml", 100), None);
        assert_eq!(read_part(&mut zip, "a.xml", 2000), None);
        assert_eq!(read_part(&mut zip, "a.xml", 10), None);
    }

    #[test]
    fn part_names_ignore_case() {
        let types = content_types("/PPT/Presentation.xml", PPTX_MAIN);
        let (pres, rels) = (presentation(12192000, 6858000, 2), rels(2));
        let k = kind(zip_of(&[
            ("[Content_Types].xml", types.as_bytes()),
            ("PPT/Presentation.xml", pres.as_bytes()),
            ("PPT/_rels/Presentation.xml.rels", rels.as_bytes()),
            ("PPT/Slides/Slide1.xml", slide(false).as_bytes()),
            ("PPT/Slides/Slide2.xml", slide(true).as_bytes()),
        ]));
        assert_eq!(k, Kind::Ooxml { variant: Variant::Pptx, slide_size: Some((12192000, 6858000)), slides: Some(2), hidden: 1 });
    }

    /// `ids` slide ids, each with its own relationship, pointing at `slides` in turn.
    fn pptx_with_slides(ids: usize, slides: &[(String, Vec<u8>)]) -> Vec<u8> {
        let types = content_types("/ppt/presentation.xml", PPTX_MAIN);
        let pres = presentation(12192000, 6858000, ids);
        let r: String = (0..ids)
            .map(|i| format!(r#"<Relationship Id="rId{}" Target="slides/{}"/>"#, i + 2, slides[i % slides.len()].0))
            .collect();
        let rels = format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{r}</Relationships>"#);
        let mut entries: Vec<(String, &[u8])> = vec![
            ("[Content_Types].xml".into(), types.as_bytes()),
            ("ppt/presentation.xml".into(), pres.as_bytes()),
            ("ppt/_rels/presentation.xml.rels".into(), rels.as_bytes()),
        ];
        entries.extend(slides.iter().map(|(n, x)| (format!("ppt/slides/{n}"), x.as_slice())));
        zip_of(&entries.iter().map(|(n, x)| (n.as_str(), *x)).collect::<Vec<_>>())
    }

    /// A hidden slide root after `pad` bytes of whitespace.
    fn padded_hidden_slide(pad: usize) -> Vec<u8> {
        let mut s = vec![b' '; pad];
        s.extend(slide(false).bytes().skip_while(|&b| b != b'>').skip(1));
        s
    }

    #[test]
    fn slide_listed_many_times_is_read_once_and_counted_each_time() {
        // Read per id, 5,000 × ~62 KB would run past the budget and the count would give up.
        let slide = padded_hidden_slide(SLIDE_HEAD as usize - 4096);
        let k = kind(pptx_with_slides(5000, &[("slide1.xml".into(), slide)]));
        assert_eq!(k, Kind::Ooxml { variant: Variant::Pptx, slide_size: Some((12192000, 6858000)), slides: Some(5000), hidden: 5000 });
    }

    #[test]
    fn slide_root_is_looked_for_only_near_the_start() {
        let near = padded_hidden_slide(1000);
        let far = padded_hidden_slide(SLIDE_HEAD as usize);
        let k = kind(pptx_with_slides(2, &[("slide1.xml".into(), near), ("slide2.xml".into(), far)]));
        assert!(matches!(k, Kind::Ooxml { slides: Some(2), hidden: 1, .. }), "{k:?}");
    }

    #[test]
    fn hidden_count_gives_up_when_slides_inflate_too_much() {
        let slides = |n: usize| (0..n).map(|i| (format!("slide{i}.xml"), padded_hidden_slide(SLIDE_HEAD as usize - 2048))).collect::<Vec<_>>();
        assert!(matches!(kind(pptx_with_slides(10, &slides(10))), Kind::Ooxml { slides: Some(10), hidden: 10, .. }));
        // 900 × ~62 KB is past MAX_PART.
        assert!(matches!(kind(pptx_with_slides(900, &slides(900))), Kind::Ooxml { slides: Some(900), hidden: 0, .. }));
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
