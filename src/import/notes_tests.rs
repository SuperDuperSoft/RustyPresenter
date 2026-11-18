use super::*;
use std::io::{Cursor, Write};

const PPTX_MAIN: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const NOTES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
const LAYOUT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";

fn zip_of(entries: &[(String, String)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        w.start_file(name.as_str(), opts).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn rels(list: &[(&str, &str, &str)]) -> String {
    let r: String = list.iter().map(|(id, ty, target)| format!(r#"<Relationship Id="{id}" Type="{ty}" Target="{target}"/>"#)).collect();
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{r}</Relationships>"#)
}

fn slide(shown: bool) -> String {
    let show = if shown { "" } else { r#" show="0""# };
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:p="{P}" xmlns:a="{A}"{show}><p:cSld><p:spTree/></p:cSld></p:sld>"#)
}

/// A shape: `ph` is the placeholder element (or ""), `paras` its paragraphs of runs.
fn sp(ph: &str, paras: &[&[&str]]) -> String {
    let body: String = paras.iter().map(|runs| {
        let r: String = runs.iter().map(|t| if *t == "<br>" { "<a:br/>".to_string() } else { format!("<a:r><a:rPr lang=\"en-US\"/><a:t>{t}</a:t></a:r>") }).collect();
        format!("<a:p>{r}</a:p>")
    }).collect();
    format!(r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Shape"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{body}</p:txBody></p:sp>"#)
}

fn notes_slide(shapes: &[String]) -> String {
    let image = r#"<p:sp><p:nvSpPr><p:cNvPr id="1" name="Slide Image"/><p:cNvSpPr/><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp>"#;
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:notes xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}"><p:cSld><p:spTree>{image}{}</p:spTree></p:cSld></p:notes>"#, shapes.concat())
}

fn package(presentation_rels: &[(&str, &str, &str)], ids: &[&str], parts: Vec<(String, String)>) -> Vec<u8> {
    let types = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="{PPTX_MAIN}"/></Types>"#);
    let list: String = ids.iter().enumerate().map(|(i, id)| format!(r#"<p:sldId id="{}" rel:id="{id}"/>"#, 256 + i)).collect();
    // Any prefix may stand for the relationships namespace.
    let pres = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation xmlns:p="{P}" xmlns:rel="{R}"><p:sldIdLst>{list}</p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/></p:presentation>"#);
    let mut all = vec![
        ("[Content_Types].xml".to_string(), types),
        ("ppt/presentation.xml".to_string(), pres),
        ("ppt/_rels/presentation.xml.rels".to_string(), rels(presentation_rels)),
    ];
    all.extend(parts);
    zip_of(&all)
}

/// PowerPoint's way: four slides in the file, the second hidden, shown in the order 1, 2 (hidden), 4, 3; notes on
/// the first and the last shown slide. Each notes slide also has the slide number.
pub(crate) fn sample_pptx() -> Vec<u8> {
    let num = |n: &str| sp(r#"<p:ph type="sldNum" sz="quarter" idx="5"/>"#, &[&[n]]);
    let body = r#"<p:ph type="body" idx="1"/>"#;
    package(
        &[("rId2", SLIDE, "slides/slide1.xml"), ("rId3", SLIDE, "slides/slide2.xml"), ("rId4", SLIDE, "slides/slide3.xml"), ("rId5", SLIDE, "slides/slide4.xml")],
        &["rId2", "rId3", "rId5", "rId4"],
        vec![
            ("ppt/slides/slide1.xml".into(), slide(true)),
            ("ppt/slides/slide2.xml".into(), slide(false)),
            ("ppt/slides/slide3.xml".into(), slide(true)),
            ("ppt/slides/slide4.xml".into(), slide(true)),
            ("ppt/slides/_rels/slide1.xml.rels".into(), rels(&[("rId1", LAYOUT, "../slideLayouts/slideLayout1.xml"), ("rId2", NOTES, "../notesSlides/notesSlide1.xml")])),
            ("ppt/slides/_rels/slide2.xml.rels".into(), rels(&[("rId2", NOTES, "../notesSlides/notesSlide2.xml")])),
            ("ppt/slides/_rels/slide3.xml.rels".into(), rels(&[("rId2", NOTES, "../notesSlides/notesSlide3.xml")])),
            ("ppt/slides/_rels/slide4.xml.rels".into(), rels(&[("rId1", LAYOUT, "../slideLayouts/slideLayout1.xml")])),
            ("ppt/notesSlides/notesSlide1.xml".into(), notes_slide(&[sp(body, &[&["Welcome everyone"]]), num("1")])),
            ("ppt/notesSlides/notesSlide2.xml".into(), notes_slide(&[sp(body, &[&["Hidden slide notes"]]), num("2")])),
            ("ppt/notesSlides/notesSlide3.xml".into(), notes_slide(&[num("4"), sp(body, &[&["Read ", "John 3:16"], &["Then pray", "<br>", "slowly"], &[]])])),
        ],
    )
}

/// What `sample_pptx` gives for the three slides the export makes.
pub(crate) fn sample_notes() -> Vec<String> {
    vec!["Welcome everyone".into(), String::new(), "Read John 3:16\nThen pray\nslowly".into()]
}

/// Google Slides' "Download → Microsoft PowerPoint": the notes placeholder has only an `idx`, the text is cut into
/// many runs, the rels point at absolute part names whose case differs from the zip's, and the slide number sits
/// in a plain text box.
fn google_pptx() -> Vec<u8> {
    let notes = |runs: &[&[&str]]| notes_slide(&[sp("", &[&["7"]]), sp(r#"<p:ph idx="1"/>"#, runs)]);
    package(
        &[("g1", SLIDE, "/ppt/slides/slide1.xml"), ("g2", SLIDE, "/ppt/slides/slide2.xml"), ("g3", SLIDE, "/ppt/slides/slide3.xml")],
        &["g1", "g2", "g3"],
        vec![
            ("ppt/slides/slide1.xml".into(), slide(true)),
            ("ppt/slides/slide2.xml".into(), slide(false)),
            ("ppt/slides/slide3.xml".into(), slide(true)),
            ("ppt/slides/_rels/slide1.xml.rels".into(), rels(&[("kix1", NOTES, "/ppt/notesSlides/NotesSlide_g1.xml")])),
            ("ppt/slides/_rels/slide2.xml.rels".into(), rels(&[("kix1", NOTES, "/ppt/notesSlides/notesSlide_g2.xml")])),
            ("ppt/slides/_rels/slide3.xml.rels".into(), rels(&[("kix1", NOTES, "../notesSlides/notesSlide_g3.xml")])),
            ("ppt/notesSlides/notesslide_g1.xml".into(), notes(&[&["Ann", "ounce", "ments"], &["Bake ", "sale ", "Sunday"]])),
            ("ppt/notesSlides/notesSlide_g2.xml".into(), notes(&[&["Skipped"]])),
            ("ppt/notesSlides/notesSlide_g3.xml".into(), notes(&[&["Close in prayer"]])),
        ],
    )
}

#[test]
fn powerpoint_notes_follow_the_show_order_and_skip_hidden_slides() {
    assert_eq!(pptx(Cursor::new(sample_pptx())), Some(sample_notes()));
}

#[test]
fn google_slides_notes_without_a_body_type() {
    assert_eq!(pptx(Cursor::new(google_pptx())), Some(vec!["Announcements\nBake sale Sunday".to_string(), "Close in prayer".into()]));
}

#[test]
fn body_placeholder_wins_over_a_longer_text_box() {
    let doc = notes_slide(&[sp("", &[&["A much longer text box that is not the notes"]]), sp(r#"<p:ph type="body"/>"#, &[&["The notes"]])]);
    assert_eq!(notes_text(&Document::parse(&doc).unwrap()), "The notes");
    // Only the slide number and the picture: no notes.
    let doc = notes_slide(&[sp(r#"<p:ph type="sldNum"/>"#, &[&["3"]])]);
    assert_eq!(notes_text(&Document::parse(&doc).unwrap()), "");
}

#[test]
fn not_a_presentation_has_no_notes() {
    assert_eq!(pptx(Cursor::new(b"not a zip".to_vec())), None);
    assert_eq!(pptx(Cursor::new(zip_of(&[("a.txt".into(), "x".into())]))), None);
    assert_eq!(odp(Cursor::new(zip_of(&[("mimetype".into(), "application/vnd.oasis.opendocument.presentation".into())]))), None);
}

#[test]
fn part_names_resolve() {
    assert_eq!(resolve("ppt/slides", "../notesSlides/notesSlide1.xml"), "ppt/notesSlides/notesSlide1.xml");
    assert_eq!(resolve("ppt", "slides/slide1.xml"), "ppt/slides/slide1.xml");
    assert_eq!(resolve("ppt/slides", "/ppt/notesSlides/n.xml"), "ppt/notesSlides/n.xml");
}

pub(crate) fn sample_odp() -> Vec<u8> {
    let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0">
<office:automatic-styles>
<style:style style:name="dp1" style:family="drawing-page"><style:drawing-page-properties presentation:display-header="false"/></style:style>
<style:style style:name="dp2" style:family="drawing-page"><style:drawing-page-properties presentation:visibility="hidden"/></style:style>
</office:automatic-styles>
<office:body><office:presentation>
<draw:page draw:name="page1" draw:style-name="dp1"><presentation:notes><draw:page-thumbnail presentation:class="page"/><draw:frame presentation:class="notes"><draw:text-box><text:p>Hello<text:s text:c="2"/><text:span>world</text:span></text:p></draw:text-box></draw:frame></presentation:notes></draw:page>
<draw:page draw:name="page2" draw:style-name="dp2"><presentation:notes><draw:frame presentation:class="notes"><draw:text-box><text:p>Hidden</text:p></draw:text-box></draw:frame></presentation:notes></draw:page>
<draw:page draw:name="page3" draw:style-name="dp1"><presentation:notes><draw:frame presentation:class="notes"><draw:text-box><text:p>First</text:p><text:list><text:list-item><text:p>Second<text:line-break/>line</text:p></text:list-item></text:list></draw:text-box></draw:frame></presentation:notes></draw:page>
<draw:page draw:name="page4" draw:style-name="dp1"/>
</office:presentation></office:body></office:document-content>"#;
    zip_of(&[("mimetype".into(), "application/vnd.oasis.opendocument.presentation".into()), ("content.xml".into(), content.into())])
}

#[test]
fn odp_notes_skip_hidden_slides() {
    assert_eq!(odp(Cursor::new(sample_odp())), Some(vec!["Hello  world".to_string(), "First\nSecond\nline".into(), String::new()]));
}

#[test]
fn read_picks_the_reader_by_kind() {
    let d = crate::store::test_dir();
    let p = d.join("talk.bin");
    std::fs::write(&p, sample_pptx()).unwrap();
    let kind = super::super::sniff::sniff(&p).unwrap();
    assert_eq!(format(&kind), Some(Format::Pptx));
    assert_eq!(read(&p, Format::Pptx), Some(sample_notes()));
    assert_eq!(format(&Kind::Pdf), None);
    assert_eq!(format(&Kind::Odp), Some(Format::Odp));
}

#[test]
fn notes_kept_only_when_they_line_up() {
    assert_eq!(line_up(Some(sample_notes()), 3), sample_notes());
    assert_eq!(line_up(Some(sample_notes()), 2), Vec::<String>::new(), "a different count would put notes on the wrong slides");
    assert_eq!(line_up(Some(vec![String::new(); 3]), 3), Vec::<String>::new(), "no notes at all");
    assert_eq!(line_up(None, 3), Vec::<String>::new());
}
