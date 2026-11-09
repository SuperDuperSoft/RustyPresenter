use super::*;
use std::io::{Cursor, Write};

const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const P14: &str = "http://schemas.microsoft.com/office/powerpoint/2010/main";
const SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";

fn zip_of(entries: &[(String, String)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        w.start_file(name.as_str(), opts).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// A slide whose `<p:transition…>` markup (or "") sits after the shapes, as PowerPoint writes it.
fn slide(shown: bool, transition: &str) -> String {
    let show = if shown { "" } else { r#" show="0""# };
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:p="{P}" xmlns:mc="{MC}" xmlns:p14="{P14}"{show}><p:cSld><p:spTree/></p:cSld>{transition}</p:sld>"#)
}

/// A .pptx with one slide per entry: (shown, transition markup).
fn pptx_of(slides: &[(bool, &str)]) -> Vec<u8> {
    let rels: String = (0..slides.len()).map(|i| format!(r#"<Relationship Id="rId{i}" Type="{SLIDE}" Target="slides/slide{i}.xml"/>"#)).collect();
    let list: String = (0..slides.len()).map(|i| format!(r#"<p:sldId id="{}" r:id="rId{i}"/>"#, 256 + i)).collect();
    let types = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/></Types>"#);
    let mut all = vec![
        ("[Content_Types].xml".to_string(), types),
        ("ppt/presentation.xml".to_string(), format!(r#"<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldIdLst>{list}</p:sldIdLst></p:presentation>"#)),
        ("ppt/_rels/presentation.xml.rels".to_string(), format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#)),
    ];
    for (i, (shown, t)) in slides.iter().enumerate() {
        all.push((format!("ppt/slides/slide{i}.xml"), slide(*shown, t)));
    }
    zip_of(&all)
}

/// Three slides: push up, none, wipe right (and a hidden one between).
pub(crate) fn sample_pptx() -> Vec<u8> {
    pptx_of(&[
        (true, r#"<p:transition spd="med" advClick="0" advTm="5000"><p:push dir="u"/></p:transition>"#),
        (true, ""),
        (false, r#"<p:transition advTm="1000"><p:fade/></p:transition>"#),
        (true, r#"<p:transition p14:dur="1500"><p:wipe dir="r"/></p:transition>"#),
    ])
}

/// `sample_pptx`'s auto-advance times.
pub(crate) fn sample_advance() -> Vec<Option<u32>> { vec![Some(5000), None, None] }

pub(crate) fn sample_transitions() -> Vec<Option<Transition>> {
    vec![t(TransitionKind::Push, 750, Dir::Up), None, t(TransitionKind::Wipe, 1500, Dir::Right)]
}

fn t(kind: TransitionKind, ms: u32, dir: Dir) -> Option<Transition> { Some(Transition { kind, ms, dir }) }
fn one(markup: &str) -> Option<Transition> { slide_transition(&Document::parse(&slide(true, markup)).unwrap()) }

#[test]
fn each_shown_slide_brings_its_powerpoint_transition() {
    let file = pptx_of(&[
        (true, r#"<p:transition spd="slow"><p:fade/></p:transition>"#),
        (true, ""),
        (false, r#"<p:transition><p:push dir="u"/></p:transition>"#),
        (true, r#"<p:transition spd="fast"><p:wipe dir="r"/></p:transition>"#),
    ]);
    assert_eq!(pptx(Cursor::new(file)), Some(vec![
        t(TransitionKind::Fade, 1000, Dir::Left),
        None,
        t(TransitionKind::Wipe, 500, Dir::Right),
    ]), "the hidden slide makes no picture, so it has no entry");
}

#[test]
fn newer_powerpoint_choice_wins_over_its_fallback() {
    let alt = format!(r#"<mc:AlternateContent><mc:Choice Requires="p14"><p:transition spd="slow" p14:dur="1250"><p:push dir="d"/></p:transition></mc:Choice><mc:Fallback><p:transition spd="slow"><p:fade/></p:transition></mc:Fallback></mc:AlternateContent>"#);
    assert_eq!(one(&alt), t(TransitionKind::Push, 1250, Dir::Down));
    // A PowerPoint 2010 effect the room has no match for fades for the file's time.
    let alt = r#"<mc:AlternateContent><mc:Choice Requires="p14"><p:transition spd="slow" p14:dur="2000"><p14:prism/></p:transition></mc:Choice><mc:Fallback><p:transition spd="slow"><p:fade/></p:transition></mc:Fallback></mc:AlternateContent>"#;
    assert_eq!(one(alt), t(TransitionKind::Fade, 2000, Dir::Left));
    // Only a fallback.
    let alt = r#"<mc:AlternateContent><mc:Fallback><p:transition spd="med"><p:cover dir="r"/></p:transition></mc:Fallback></mc:AlternateContent>"#;
    assert_eq!(one(alt), t(TransitionKind::Cover, 750, Dir::Right));
}

#[test]
fn powerpoint_kinds_map_to_what_the_room_draws() {
    let k = |el: &str| one(&format!(r#"<p:transition>{el}</p:transition>"#));
    assert_eq!(k("<p:fade/>"), t(TransitionKind::Fade, 500, Dir::Left));
    assert_eq!(k(r#"<p:fade thruBlk="1"/>"#), t(TransitionKind::Black, 500, Dir::Left));
    assert_eq!(k("<p:cut/>"), t(TransitionKind::Cut, 500, Dir::Left));
    assert_eq!(k(r#"<p:cut thruBlk="true"/>"#), t(TransitionKind::Black, 500, Dir::Left));
    assert_eq!(k("<p:dissolve/>"), t(TransitionKind::Dissolve, 500, Dir::Left));
    assert_eq!(k("<p:push/>"), t(TransitionKind::Push, 500, Dir::Left), "PowerPoint's default side");
    assert_eq!(k(r#"<p:pull dir="ru"/>"#), t(TransitionKind::Uncover, 500, Dir::Right));
    assert_eq!(k(r#"<p:cover/>"#), t(TransitionKind::Cover, 500, Dir::Left));
    assert_eq!(k(r#"<p:split orient="vert" dir="in"/>"#), t(TransitionKind::Split, 500, Dir::Left), "opens sideways");
    assert_eq!(k(r#"<p:split/>"#), t(TransitionKind::Split, 500, Dir::Up), "opens up and down");
    assert_eq!(k(r#"<p:strips dir="ld"/>"#), t(TransitionKind::Wipe, 500, Dir::Left), "another kind with a side wipes");
    assert_eq!(k(r#"<p:zoom dir="in"/>"#), t(TransitionKind::Zoom, 500, Dir::Left));
    assert_eq!(k(r#"<p:randomBar dir="vert"/>"#), t(TransitionKind::Fade, 500, Dir::Left));
    assert_eq!(k(r#"<p14:vortex dir="u"/>"#), t(TransitionKind::Wipe, 500, Dir::Up));
    // Never longer than the editor offers.
    assert_eq!(one(r#"<p:transition p14:dur="60000"><p:fade/></p:transition>"#), t(TransitionKind::Fade, TRANSITION_MAX_MS, Dir::Left));
    // A sound only: no effect.
    assert_eq!(one(r#"<p:transition><p:sndAc><p:stSnd/></p:sndAc></p:transition>"#), t(TransitionKind::Cut, 500, Dir::Left));
}

/// LibreOffice: four pages, the third hidden, with transitions in their page styles.
fn sample_odp() -> Vec<u8> {
    let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:smil="urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0">
<office:automatic-styles>
<style:style style:name="dp1" style:family="drawing-page"><style:drawing-page-properties presentation:transition-speed="slow" smil:type="pushWipe" smil:subtype="fromRight"/></style:style>
<style:style style:name="dp2" style:family="drawing-page"><style:drawing-page-properties smil:type="barWipe" smil:subtype="leftToRight" smil:direction="reverse" smil:dur="2s"/></style:style>
<style:style style:name="dp3" style:family="drawing-page"><style:drawing-page-properties presentation:visibility="hidden" smil:type="fade"/></style:style>
<style:style style:name="dp4" style:family="drawing-page"><style:drawing-page-properties smil:type="fade" smil:subtype="fadeOverColor" smil:dur="0.5s"/></style:style>
<style:style style:name="dp5" style:family="drawing-page"><style:drawing-page-properties presentation:background-visible="true"/></style:style>
</office:automatic-styles>
<office:body><office:presentation>
<draw:page draw:name="1" draw:style-name="dp1"/>
<draw:page draw:name="2" draw:style-name="dp2"/>
<draw:page draw:name="3" draw:style-name="dp3"/>
<draw:page draw:name="4" draw:style-name="dp4"/>
<draw:page draw:name="5" draw:style-name="dp5"/>
</office:presentation></office:body></office:document-content>"#;
    zip_of(&[("mimetype".into(), "application/vnd.oasis.opendocument.presentation".into()), ("content.xml".into(), content.into())])
}

#[test]
fn libreoffice_page_styles_bring_their_transitions() {
    assert_eq!(odp(Cursor::new(sample_odp())), Some(vec![
        t(TransitionKind::Push, 1000, Dir::Left),
        t(TransitionKind::Wipe, 2000, Dir::Left),
        t(TransitionKind::Black, 500, Dir::Left),
        None,
    ]));
}

#[test]
fn powerpoint_after_times_are_read_per_shown_slide() {
    assert_eq!(pptx_advance(Cursor::new(sample_pptx())), Some(sample_advance()));
    let one = |markup: &str| slide_advance(&Document::parse(&slide(true, markup)).unwrap());
    assert_eq!(one(r#"<p:transition advTm="0"/>"#), Some(0));
    assert_eq!(one(r#"<p:transition advTm="x"/>"#), None);
    assert_eq!(one(""), None);
    let alt = r#"<mc:AlternateContent><mc:Choice Requires="p14"><p:transition p14:dur="1250" advTm="3000"><p:push dir="d"/></p:transition></mc:Choice><mc:Fallback><p:transition advTm="2000"><p:fade/></p:transition></mc:Fallback></mc:AlternateContent>"#;
    assert_eq!(one(alt), Some(3000));
}

#[test]
fn libreoffice_automatic_pages_bring_their_time() {
    let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:smil="urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0">
<office:automatic-styles>
<style:style style:name="dp1" style:family="drawing-page"><style:drawing-page-properties presentation:transition-type="automatic" presentation:duration="PT00H00M05S" smil:type="fade" smil:dur="1s"/></style:style>
<style:style style:name="dp2" style:family="drawing-page"><style:drawing-page-properties presentation:transition-type="manual" presentation:duration="PT00H00M09S"/></style:style>
<style:style style:name="dp3" style:family="drawing-page"><style:drawing-page-properties presentation:transition-type="automatic" presentation:duration="PT1M2.5S"/></style:style>
</office:automatic-styles>
<office:body><office:presentation>
<draw:page draw:name="1" draw:style-name="dp1"/>
<draw:page draw:name="2" draw:style-name="dp2"/>
<draw:page draw:name="3" draw:style-name="dp3"/>
<draw:page draw:name="4"/>
</office:presentation></office:body></office:document-content>"#;
    let file = zip_of(&[("mimetype".into(), "application/vnd.oasis.opendocument.presentation".into()), ("content.xml".into(), content.into())]);
    assert_eq!(odp_advance(Cursor::new(file)), Some(vec![Some(5000), None, Some(62_500), None]));
    assert_eq!(iso_duration("PT01H00M00S"), Some(3_600_000));
    assert_eq!(iso_duration("5s"), None);
    assert_eq!(line_up(Some(vec![Some(5000u32), None]), 2), vec![Some(5000), None]);
}

#[test]
fn transitions_kept_only_when_they_line_up() {
    let fade = t(TransitionKind::Fade, 500, Dir::Left);
    assert_eq!(line_up(Some(vec![fade, None]), 2), vec![fade, None]);
    assert_eq!(line_up(Some(vec![fade, None]), 3), vec![]);
    assert_eq!(line_up::<Transition>(Some(vec![None, None]), 2), vec![], "none at all: nothing to keep");
    assert_eq!(line_up::<Transition>(None, 2), vec![]);
    assert_eq!(pptx(Cursor::new(b"not a zip".to_vec())), None);
}

#[test]
fn smil_clock_values() {
    assert_eq!(seconds("2s"), Some(2000));
    assert_eq!(seconds("0.5s"), Some(500));
    assert_eq!(seconds("750ms"), Some(750));
    assert_eq!(seconds("1.25"), Some(1250));
    assert_eq!(seconds("soon"), None);
}
