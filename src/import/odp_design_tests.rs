use super::*;
use crate::design::{HAlign, Transition, VAlign};
use std::io::{Cursor, Write};

const NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:smil="urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0" xmlns:anim="urn:oasis:names:tc:opendocument:xmlns:animation:1.0""#;

/// LibreOffice's styles.xml: a 28 × 15.75 cm (16:9) page, white master background, the title and outline styles.
/// `master` is what the master page holds besides its title placeholder; `size` the page width and height.
fn styles_xml(master: &str, size: (&str, &str)) -> String {
    format!(r##"<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles {NS} office:version="1.3">
<office:font-face-decls><style:font-face style:name="Liberation Sans" svg:font-family="&apos;Liberation Sans&apos;" style:font-family-generic="roman"/><style:font-face style:name="DejaVu Serif" svg:font-family="&apos;DejaVu Serif&apos;"/></office:font-face-decls>
<office:styles>
<draw:gradient draw:name="Sky" draw:style="linear" draw:start-color="#000000" draw:end-color="#0000ff"/>
<draw:fill-image draw:name="Paper" xlink:href="Pictures/paper.png" xlink:type="simple"/>
<style:default-style style:family="graphic"><style:graphic-properties svg:stroke-color="#3465a4" draw:fill-color="#729fcf"/><style:text-properties style:font-name="Liberation Sans" fo:font-size="18pt"/></style:default-style>
<style:style style:name="standard" style:family="graphic"><style:graphic-properties draw:stroke="solid" svg:stroke-width="0cm" svg:stroke-color="#3465a4" draw:fill="solid" draw:fill-color="#729fcf"/><style:text-properties fo:font-size="18pt"/></style:style>
<style:style style:name="Default-title" style:family="presentation"><style:graphic-properties draw:stroke="none" draw:fill="none" draw:textarea-vertical-align="middle"/><style:paragraph-properties fo:text-align="center"/><style:text-properties fo:font-size="44pt" fo:color="#1c1c1c" style:font-name="DejaVu Serif"/></style:style>
<style:style style:name="Default-outline1" style:family="presentation"><style:graphic-properties draw:stroke="none" draw:fill="none"/><style:text-properties fo:font-size="32pt" fo:color="#000000"/></style:style>
</office:styles>
<office:automatic-styles>
<style:page-layout style:name="PM1"><style:page-layout-properties fo:margin-top="0cm" fo:page-width="{}" fo:page-height="{}" style:print-orientation="landscape"/></style:page-layout>
<style:style style:name="Mdp1" style:family="drawing-page"><style:drawing-page-properties draw:background-size="full" draw:fill="solid" draw:fill-color="#ffffff"/></style:style>
<style:style style:name="Mgr1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill-color="#ff0000" draw:stroke="none"/></style:style>
</office:automatic-styles>
<office:master-styles>
<style:master-page style:name="Default" style:page-layout-name="PM1" draw:style-name="Mdp1">
<draw:frame presentation:style-name="Default-title" presentation:class="title" presentation:placeholder="true" svg:x="1.4cm" svg:y="1.575cm" svg:width="25.2cm" svg:height="3.15cm"><draw:text-box/></draw:frame>
{master}
</style:master-page>
</office:master-styles>
</office:document-styles>"##, size.0, size.1)
}

/// content.xml with these automatic styles and pages.
fn content_xml(auto: &str, pages: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content {NS} office:version="1.3">
<office:automatic-styles>
<style:style style:name="dp1" style:family="drawing-page"><style:drawing-page-properties presentation:background-visible="true" presentation:background-objects-visible="true"/></style:style>
{auto}
</office:automatic-styles>
<office:body><office:presentation>
{pages}
</office:presentation></office:body></office:document-content>"#)
}

fn zip_of(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        w.start_file(*name, opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn odp_file(content: &str, styles: &str, pictures: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries = vec![
        ("mimetype", b"application/vnd.oasis.opendocument.presentation".to_vec()),
        ("content.xml", content.as_bytes().to_vec()),
        ("styles.xml", styles.as_bytes().to_vec()),
    ];
    entries.extend(pictures.iter().map(|(n, b)| (*n, b.to_vec())));
    zip_of(&entries)
}

/// Converts, keeping each picture handed to the store as (bytes, ext); assets are `img<n>.<ext>`.
fn convert_all(file: Vec<u8>) -> (OdpDesigns, Vec<(Vec<u8>, String)>) {
    let mut put_in = Vec::new();
    let mut put = |b: &[u8], ext: &str| {
        put_in.push((b.to_vec(), ext.to_string()));
        Some(format!("img{}.{ext}", put_in.len()))
    };
    let out = convert(Cursor::new(file), &mut put).expect("converts");
    (out, put_in)
}

/// One 16:9 page with these automatic styles and page markup.
fn one_page(auto: &str, page: &str) -> Design {
    let file = odp_file(&content_xml(auto, &format!(r#"<draw:page draw:name="p1" draw:style-name="dp1" draw:master-page-name="Default">{page}</draw:page>"#)), &styles_xml("", ("28cm", "15.75cm")), &[]);
    let (mut out, _) = convert_all(file);
    assert_eq!(out.designs.len(), 1);
    out.designs.remove(0)
}

fn close(a: f32, b: f32) -> bool { (a - b).abs() < 0.05 }

fn rect_is(r: PctRect, x: f32, y: f32, w: f32, h: f32) -> bool { close(r.x, x) && close(r.y, y) && close(r.w, w) && close(r.h, h) }

fn text_of(o: &Obj) -> Option<&str> { match &o.kind { ObjKind::Text { text, .. } => Some(text), _ => None } }

/// The objects after the background.
fn drawn(d: &Design) -> &[Obj] { &d.objects[1..] }

const SHAPE_STYLES: &str = r##"
<style:style style:name="gr1" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:fill="solid" draw:fill-color="#00ff00" draw:opacity="50%" draw:stroke="solid" svg:stroke-color="#ff0000" svg:stroke-width="0.1cm"/></style:style>
<style:style style:name="gr2" style:family="graphic" style:parent-style-name="standard"><style:graphic-properties draw:stroke="none"/></style:style>
<style:style style:name="gr3" style:family="graphic"><style:graphic-properties draw:stroke="solid" svg:stroke-color="#0000ff" svg:stroke-width="0.2cm" draw:marker-end="Arrow"/></style:style>
"##;

#[test]
fn shapes_take_their_place_kind_fill_and_outline() {
    let d = one_page(SHAPE_STYLES, r#"
<draw:custom-shape draw:style-name="gr1" draw:id="id1" svg:x="2.8cm" svg:y="1.575cm" svg:width="14cm" svg:height="3.15cm"><draw:enhanced-geometry svg:viewBox="0 0 21600 21600" draw:type="rectangle" draw:enhanced-path="M 0 0 L 21600 0 21600 21600 0 21600 0 0 Z N"/></draw:custom-shape>
<draw:custom-shape draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"><draw:enhanced-geometry draw:type="ellipse"/></draw:custom-shape>
<draw:custom-shape draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"><draw:enhanced-geometry draw:type="isosceles-triangle"/></draw:custom-shape>
<draw:custom-shape draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="5.6cm" svg:height="1.575cm"><draw:enhanced-geometry draw:type="round-rectangle" draw:modifiers="5400"/></draw:custom-shape>
<draw:custom-shape draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="5.6cm" svg:height="3.15cm"><draw:enhanced-geometry draw:type="right-arrow"/></draw:custom-shape>
<draw:rect draw:style-name="gr2" svg:x="28mm" svg:y="1in" svg:width="72pt" svg:height="1.575cm"/>
<draw:ellipse draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"/>
<draw:line draw:style-name="gr3" svg:x1="2.8cm" svg:y1="7.875cm" svg:x2="16.8cm" svg:y2="7.875cm"/>
<draw:custom-shape svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"><draw:enhanced-geometry draw:type="star5"/></draw:custom-shape>
"#);
    let o = drawn(&d);
    assert_eq!(o.len(), 9);
    let shape = |i: usize| match &o[i].kind { ObjKind::Shape { shape } => *shape, k => panic!("{k:?}") };
    assert_eq!([0, 1, 2, 3, 4, 5, 6, 7, 8].map(shape), [Shape::Rect, Shape::Ellipse, Shape::Triangle, Shape::Rect, Shape::Arrow, Shape::Rect, Shape::Ellipse, Shape::Arrow, Shape::Rect]);
    assert!(rect_is(o[0].rect, 10.0, 10.0, 50.0, 20.0), "{:?}", o[0].rect);
    // The fill keeps its see-through share; 0.1 cm of a 28 cm page is 1920 / 280 px.
    assert_eq!(o[0].look.fill.as_deref(), Some("#00ff0080"));
    let ol = o[0].look.outline.as_ref().unwrap();
    assert_eq!(ol.color, "#ff0000");
    assert!(close(ol.width, 1920.0 / 280.0), "{}", ol.width);
    // No stroke: no outline; the fill comes from the parent style.
    assert_eq!((o[1].look.fill.as_deref(), o[1].look.outline.is_none()), (Some("#729fcf"), true));
    // A quarter of the shorter side, in 1080p pixels: 1.575 cm is 108 px.
    assert!(close(o[3].look.radius, 27.0), "{}", o[3].look.radius);
    // 28 mm, 1 inch (2.54 cm), 72 pt (1 inch).
    assert!(rect_is(o[5].rect, 10.0, 2.54 / 15.75 * 100.0, 2.54 / 28.0 * 100.0, 10.0), "{:?}", o[5].rect);
    // A line: as long as it is, across the middle of its box, drawn in the stroke colour.
    assert!(close(o[7].rect.x, 10.0) && close(o[7].rect.w, 50.0) && close(o[7].rect.y + o[7].rect.h / 2.0, 50.0), "{:?}", o[7].rect);
    assert_eq!(o[7].look.outline.as_ref().unwrap().color, "#0000ff");
    assert!(close(o[7].rotation, 0.0));
    // A block arrow is drawn in its fill colour.
    assert_eq!(o[4].look.outline.as_ref().unwrap().color, "#729fcf");
}

#[test]
fn a_line_going_down_is_turned() {
    let d = one_page(SHAPE_STYLES, r#"<draw:line draw:style-name="gr3" svg:x1="2.8cm" svg:y1="1.575cm" svg:x2="2.8cm" svg:y2="14.175cm"/>"#);
    let l = &drawn(&d)[0];
    assert!(close(l.rotation, 90.0), "{}", l.rotation);
    // 12.6 cm of a 28 cm-wide slide, centred on its middle.
    assert!(close(l.rect.w, 45.0) && close(l.rect.x + l.rect.w / 2.0, 10.0) && close(l.rect.y + l.rect.h / 2.0, 50.0), "{:?}", l.rect);
}

#[test]
fn a_turned_shape_keeps_its_middle() {
    // 30° to the left, as LibreOffice writes it: rotate first, then move.
    let d = one_page(SHAPE_STYLES, r#"<draw:custom-shape draw:style-name="gr2" svg:width="5.6cm" svg:height="3.15cm" draw:transform="rotate (0.523598775598299) translate (5.6cm 3.15cm)"><draw:enhanced-geometry draw:type="rectangle"/></draw:custom-shape>"#);
    let o = &drawn(&d)[0];
    assert!(close(o.rotation, -30.0), "{}", o.rotation);
    assert!(rect_is(o.rect, 6.0124 / 28.0 * 100.0, 1.5386 / 15.75 * 100.0, 20.0, 20.0), "{:?}", o.rect);
}

const TEXT_STYLES: &str = r##"
<style:style style:name="pr1" style:family="presentation" style:parent-style-name="Default-title"><style:graphic-properties draw:fill-color="#ffffff"/></style:style>
<style:style style:name="pr2" style:family="presentation" style:parent-style-name="Default-outline1"><style:graphic-properties draw:fill-color="#ffffff"/></style:style>
<style:style style:name="gr4" style:family="graphic"><style:graphic-properties draw:stroke="none" draw:fill="solid" draw:fill-color="#222222" draw:textarea-vertical-align="bottom"/></style:style>
<style:style style:name="P1" style:family="paragraph"><style:paragraph-properties fo:text-align="end"/><style:text-properties fo:font-size="24pt"/></style:style>
<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold" fo:font-style="italic" fo:color="#ff0000" style:font-name="DejaVu Serif" style:text-underline-style="solid"/></style:style>
<text:list-style style:name="L2"><text:list-level-style-number text:level="1" style:num-format="1" style:num-suffix="."/></text:list-style>
"##;

#[test]
fn text_frames_bring_their_words_and_look() {
    let d = one_page(TEXT_STYLES, r#"
<draw:frame presentation:style-name="pr1" draw:layer="layout" svg:width="25.2cm" svg:height="3.15cm" svg:x="1.4cm" svg:y="1.575cm" presentation:class="title"><draw:text-box><text:p>Amazing Grace</text:p></draw:text-box></draw:frame>
<draw:frame presentation:style-name="pr2" svg:width="25.2cm" svg:height="9.45cm" svg:x="1.4cm" svg:y="4.725cm" presentation:class="outline"><draw:text-box><text:list><text:list-item><text:p>How sweet</text:p></text:list-item><text:list-item><text:p>the sound</text:p><text:list><text:list-item><text:p>that saved</text:p></text:list-item></text:list></text:list-item></text:list></draw:text-box></draw:frame>
<draw:frame presentation:style-name="pr2" svg:width="25.2cm" svg:height="9.45cm" svg:x="1.4cm" svg:y="4.725cm" presentation:class="outline" presentation:placeholder="true"><draw:text-box/></draw:frame>
<draw:frame draw:style-name="gr4" svg:width="14cm" svg:height="3.15cm" svg:x="0cm" svg:y="12.6cm"><draw:text-box><text:p text:style-name="P1"><text:span text:style-name="T1">a wretch</text:span> like me</text:p><text:list text:style-name="L2"><text:list-item><text:p>one</text:p></text:list-item><text:list-item><text:p>two</text:p></text:list-item></text:list></draw:text-box></draw:frame>
<draw:custom-shape draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"><text:p>In a box</text:p><draw:enhanced-geometry draw:type="rectangle"/></draw:custom-shape>
<draw:custom-shape draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"><text:p>In a ring</text:p><draw:enhanced-geometry draw:type="ellipse"/></draw:custom-shape>
"#);
    let o = drawn(&d);
    let texts: Vec<_> = o.iter().map(text_of).collect();
    assert_eq!(texts, [Some("Amazing Grace"), Some("• How sweet\n• the sound\n    • that saved"), Some("a wretch like me\n1. one\n2. two"), Some("In a box"), None, Some("In a ring")]);
    // The title: the master's title style. 44 pt of a 15.75 cm (446.5 pt) page is 44 / 446.5 of 1080 px.
    let t = o[0].text_style().unwrap();
    assert!(close(t.size, 44.0 / 446.4567 * 1080.0), "{}", t.size);
    assert_eq!((t.font.as_str(), t.color.as_str(), t.align, t.valign, t.bold), ("DejaVu Serif", "#1c1c1c", HAlign::Center, VAlign::Middle, false));
    assert!(rect_is(o[0].rect, 5.0, 10.0, 90.0, 20.0), "{:?}", o[0].rect);
    assert!(o[0].look.fill.is_none() && o[0].look.outline.is_none());
    // An outline: top, left, the outline size and colour.
    let t = o[1].text_style().unwrap();
    assert_eq!((t.align, t.valign, t.color.as_str(), t.font.as_str()), (HAlign::Left, VAlign::Top, "#000000", "Liberation Sans"));
    assert!(close(t.size, 32.0 / 446.4567 * 1080.0));
    // A text box: its first words' style, the paragraph's size and side, the frame's fill and lower edge.
    let t = o[2].text_style().unwrap();
    assert_eq!((t.bold, t.italic, t.color.as_str(), t.font.as_str(), t.align, t.valign), (true, true, "#ff0000", "DejaVu Serif", HAlign::Right, VAlign::Bottom));
    assert!(close(t.size, 24.0 / 446.4567 * 1080.0));
    assert_eq!(o[2].look.fill.as_deref(), Some("#222222"));
    // A rectangle with words is one text box with the rectangle's fill; other shapes get a text box on top.
    assert_eq!((o[3].look.fill.as_deref(), o[3].text_style().unwrap().valign), (Some("#729fcf"), VAlign::Middle));
    assert!(matches!(o[4].kind, ObjKind::Shape { shape: Shape::Ellipse }) && o[5].rect == o[4].rect && o[5].look.fill.is_none());
}

#[test]
fn pictures_go_to_the_store_once_and_odd_objects_become_named_boxes() {
    let content = content_xml(SHAPE_STYLES, r#"<draw:page draw:name="p1" draw:style-name="dp1" draw:master-page-name="Default">
<draw:frame draw:name="Cross" svg:width="14cm" svg:height="7.875cm" svg:x="0cm" svg:y="0cm"><draw:image xlink:href="Pictures/cross.svg"/><draw:image xlink:href="Pictures/cross.png"/></draw:frame>
<draw:g draw:id="g1">
  <draw:frame svg:width="2.8cm" svg:height="1.575cm" svg:x="14cm" svg:y="7.875cm"><draw:image xlink:href="./Pictures/cross.png"/></draw:frame>
  <draw:custom-shape draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"><draw:enhanced-geometry draw:type="ellipse"/></draw:custom-shape>
</draw:g>
<draw:frame draw:name="Prices" svg:width="14cm" svg:height="7.875cm" svg:x="0cm" svg:y="0cm"><table:table><table:table-row><table:table-cell><text:p>1</text:p></table:table-cell></table:table-row></table:table></draw:frame>
<draw:frame presentation:class="chart" svg:width="14cm" svg:height="7.875cm" svg:x="0cm" svg:y="0cm"><draw:object xlink:href="./Object 1"/><draw:image xlink:href="./ObjectReplacements/Object 1"/></draw:frame>
<draw:frame draw:name="Gone" svg:width="2.8cm" svg:height="1.575cm" svg:x="0cm" svg:y="0cm"><draw:image xlink:href="Pictures/missing.png"/></draw:frame>
</draw:page>"#);
    let file = odp_file(&content, &styles_xml("", ("28cm", "15.75cm")), &[("Pictures/cross.png", b"PNGDATA"), ("Pictures/cross.svg", b"<svg/>")]);
    let (out, put) = convert_all(file);
    assert_eq!(put, [(b"PNGDATA".to_vec(), "png".to_string())]);
    let o = drawn(&out.designs[0]);
    assert_eq!(o.len(), 6);
    assert!(matches!(&o[0].kind, ObjKind::Picture { asset, fit: Fit::Stretch } if asset == "img1.png"));
    assert!(matches!(&o[1].kind, ObjKind::Picture { asset, .. } if asset == "img1.png"));
    assert!(rect_is(o[1].rect, 50.0, 50.0, 10.0, 10.0));
    assert!(matches!(o[2].kind, ObjKind::Shape { shape: Shape::Ellipse }));
    assert_eq!((text_of(&o[3]), text_of(&o[4]), text_of(&o[5])), (Some("Prices"), Some("Chart"), Some("Gone")));
    assert!(o[3].look.fill.is_some() && o[3].look.outline.is_some());
    assert_eq!(out.placeholders, 3);
}

#[test]
fn backgrounds_come_from_the_page_or_its_master() {
    let auto = r##"
<style:style style:name="dp2" style:family="drawing-page"><style:drawing-page-properties draw:fill="solid" draw:fill-color="#102030"/></style:style>
<style:style style:name="dp3" style:family="drawing-page"><style:drawing-page-properties draw:fill="bitmap" draw:fill-image-name="Paper" presentation:background-objects-visible="false"/></style:style>
<style:style style:name="dp4" style:family="drawing-page"><style:drawing-page-properties draw:fill="gradient" draw:fill-gradient-name="Sky"/></style:style>
"##;
    let master = r#"<draw:custom-shape draw:style-name="Mgr1" svg:x="0cm" svg:y="14.175cm" svg:width="28cm" svg:height="1.575cm"><draw:enhanced-geometry draw:type="rectangle"/></draw:custom-shape>
<draw:frame presentation:class="footer" presentation:placeholder="true" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"><draw:text-box/></draw:frame>"#;
    let pages = r#"<draw:page draw:style-name="dp1" draw:master-page-name="Default"/>
<draw:page draw:style-name="dp2" draw:master-page-name="Default"/>
<draw:page draw:style-name="dp3" draw:master-page-name="Default"/>
<draw:page draw:style-name="dp4" draw:master-page-name="Default"/>"#;
    let file = odp_file(&content_xml(auto, pages), &styles_xml(master, ("28cm", "15.75cm")), &[("Pictures/paper.png", b"PAPER")]);
    let (out, put) = convert_all(file);
    let d = &out.designs;
    let bg = |i: usize| (&d[i].objects[0].kind, d[i].objects[0].look.fill.clone(), d[i].objects[0].rect);
    assert!(matches!(bg(0), (ObjKind::Shape { shape: Shape::Rect }, Some(f), r) if f == "#ffffff" && rect_is(r, 0.0, 0.0, 100.0, 100.0)));
    assert_eq!(bg(1).1.as_deref(), Some("#102030"));
    assert!(matches!(bg(2).0, ObjKind::Picture { asset, fit: Fit::Stretch } if asset == "img1.png"));
    assert_eq!(put.len(), 1);
    assert_eq!(bg(3).1.as_deref(), Some("#000080"), "a gradient: halfway between its colours");
    // The master's own shapes sit on the background; its placeholders don't. Page 3 hides them.
    assert_eq!(d[0].objects.len(), 2);
    assert_eq!(d[0].objects[1].look.fill.as_deref(), Some("#ff0000"));
    assert_eq!(d[2].objects.len(), 1);
}

#[test]
fn a_four_by_three_page_sits_in_the_middle() {
    let content = content_xml(SHAPE_STYLES, r#"<draw:page draw:style-name="dp1" draw:master-page-name="Default"><draw:custom-shape draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="28cm" svg:height="10.5cm"><draw:enhanced-geometry draw:type="rectangle"/></draw:custom-shape></draw:page>"#);
    let (out, _) = convert_all(odp_file(&content, &styles_xml("", ("28cm", "21cm")), &[]));
    let d = &out.designs[0];
    assert!(rect_is(d.objects[0].rect, 12.5, 0.0, 75.0, 100.0), "{:?}", d.objects[0].rect);
    assert!(rect_is(d.objects[1].rect, 12.5, 0.0, 75.0, 50.0), "{:?}", d.objects[1].rect);
}

#[test]
fn notes_transitions_and_times_follow_the_shown_pages() {
    let auto = r#"
<style:style style:name="dp2" style:family="drawing-page"><style:drawing-page-properties presentation:visibility="hidden"/></style:style>
<style:style style:name="dp3" style:family="drawing-page"><style:drawing-page-properties presentation:transition-type="automatic" presentation:duration="PT00H00M05S" smil:type="barWipe" smil:subtype="leftToRight" smil:dur="2s"/></style:style>"#;
    let pages = r#"<draw:page draw:style-name="dp1" draw:master-page-name="Default"><presentation:notes><draw:frame presentation:class="notes"><draw:text-box><text:p>Pray first</text:p></draw:text-box></draw:frame></presentation:notes></draw:page>
<draw:page draw:style-name="dp2" draw:master-page-name="Default"/>
<draw:page draw:style-name="dp3" draw:master-page-name="Default"/>"#;
    let (out, _) = convert_all(odp_file(&content_xml(auto, pages), &styles_xml("", ("28cm", "15.75cm")), &[]));
    assert_eq!(out.designs.len(), 2);
    assert_eq!((out.designs[0].notes.as_str(), out.designs[1].notes.as_str()), ("Pray first", ""));
    assert_eq!(out.designs[0].transition, Transition::default());
    assert_eq!(out.designs[1].transition, Transition { kind: crate::design::TransitionKind::Wipe, ms: 2000, dir: crate::design::Dir::Right });
    assert_eq!(out.advance_ms, [None, Some(5000)]);
}

/// One effect node as LibreOffice writes it: `node` the node-type, `class` and `id` the preset, `sub` its sub-type,
/// `body` the animate elements.
fn effect(node: &str, begin: &str, class: &str, id: &str, sub: &str, body: &str) -> String {
    let sub = if sub.is_empty() { String::new() } else { format!(r#" presentation:preset-sub-type="{sub}""#) };
    format!(r#"<anim:par smil:begin="{begin}" smil:fill="hold" presentation:node-type="{node}" presentation:preset-class="{class}" presentation:preset-id="{id}"{sub}>{body}</anim:par>"#)
}

fn show(target: &str) -> String { format!(r#"<anim:set smil:begin="0s" smil:dur="0.001s" smil:fill="hold" smil:targetElement="{target}" smil:attributeName="visibility" smil:to="visible"/>"#) }

#[test]
fn animations_become_steps() {
    let fly = effect("on-click", "0s", "entrance", "ooo-entrance-fly-in", "from-left", &format!(r#"{}<anim:animate smil:dur="0.5s" smil:fill="hold" smil:targetElement="id1" smil:attributeName="x" smil:values="0-width/2;x" smil:keyTimes="0;1"/>"#, show("id1")));
    let fade = effect("with-previous", "0.5s", "entrance", "ooo-entrance-fade-in", "", &format!(r#"{}<anim:transitionFilter smil:dur="1s" smil:targetElement="id2" smil:type="fade" smil:subtype="crossfade"/>"#, show("id2")));
    let wipe = effect("after-previous", "0s", "exit", "ooo-exit-wipe", "from-top", r#"<anim:transitionFilter smil:dur="0.75s" smil:targetElement="id1" smil:type="barWipe" smil:subtype="topToBottom" smil:mode="out"/>"#);
    let spin = effect("on-click", "0s", "emphasis", "ooo-emphasis-spin", "", r#"<anim:animateTransform smil:dur="2s" smil:fill="hold" smil:targetElement="id2" smil:by="180" svg:type="rotate"/>"#);
    let grow = effect("with-previous", "0s", "emphasis", "ooo-emphasis-grow-and-shrink", "", r#"<anim:animateTransform smil:dur="1s" smil:fill="hold" smil:targetElement="id3" smil:to="1.25,1.25" svg:type="scale"/>"#);
    let motion = effect("on-click", "0s", "motion-path", "ooo-motionpath-right", "", r#"<anim:animateMotion smil:dur="2s" smil:fill="hold" smil:targetElement="id3" svg:path="M 0 0 L 0.25 0.1"/>"#);
    let odd = effect("on-click", "0s", "entrance", "ooo-entrance-swish", "", &format!(r#"{}<anim:animate smil:dur="1.5s" smil:targetElement="id3" smil:attributeName="x"/>"#, show("id3")));
    let appear = effect("after-previous", "0s", "entrance", "ooo-entrance-appear", "", &show("xml1"));
    let ghost = effect("on-click", "0s", "entrance", "ooo-entrance-appear", "", &show("nobody"));
    let tap = effect("on-click", "0s", "entrance", "ooo-entrance-appear", "", &show("id1"));
    let group = |effects: &[&String]| format!(r#"<anim:par smil:begin="indefinite"><anim:par smil:begin="0s">{}</anim:par></anim:par>"#, effects.iter().map(|s| s.as_str()).collect::<String>());
    let timing = format!(r#"<anim:par presentation:node-type="timing-root"><anim:seq presentation:node-type="main-sequence">{}{}{}{}{}</anim:seq><anim:seq smil:begin="id2.click" presentation:node-type="interactive-sequence">{}</anim:seq></anim:par>"#,
        group(&[&fly, &fade, &wipe]), group(&[&spin, &grow]), group(&[&motion]), group(&[&odd, &appear]), group(&[&ghost]), group(&[&tap]));
    let d = one_page(SHAPE_STYLES, &format!(r#"
<draw:custom-shape draw:style-name="gr2" draw:id="id1" xml:id="id1" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"><draw:enhanced-geometry draw:type="rectangle"/></draw:custom-shape>
<draw:custom-shape draw:style-name="gr2" xml:id="id2" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"><text:p>Joy</text:p><draw:enhanced-geometry draw:type="ellipse"/></draw:custom-shape>
<draw:custom-shape draw:style-name="gr2" draw:id="id3" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"><draw:enhanced-geometry draw:type="rectangle"/></draw:custom-shape>
<draw:g xml:id="xml1"><draw:rect draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"/><draw:ellipse draw:style-name="gr2" svg:x="0cm" svg:y="0cm" svg:width="2.8cm" svg:height="1.575cm"/></draw:g>
{timing}"#));
    let o = drawn(&d);
    let (s1, s2, s2t, s3, g1, g2) = (o[0].id, o[1].id, o[2].id, o[3].id, o[4].id, o[5].id);
    let a = &d.anims;
    let summary: Vec<_> = a.iter().map(|a| (a.obj, a.kind, a.effect, a.start, a.delay_ms, a.dur_ms, a.dir)).collect();
    use crate::design::Dir;
    assert_eq!(summary, [
        (s1, AnimKind::Entrance, Effect::Fly, Start::OnClick, 0, 500, Some(Dir::Left)),
        // A shape with words is two objects: the words' step goes with the shape's.
        (s2, AnimKind::Entrance, Effect::Fade, Start::WithPrevious, 500, 1000, None),
        (s2t, AnimKind::Entrance, Effect::Fade, Start::WithPrevious, 500, 1000, None),
        (s1, AnimKind::Exit, Effect::Wipe, Start::AfterPrevious, 0, 750, Some(Dir::Up)),
        (s2, AnimKind::Emphasis, Effect::Spin, Start::OnClick, 0, 2000, None),
        (s2t, AnimKind::Emphasis, Effect::Spin, Start::WithPrevious, 0, 2000, None),
        (s3, AnimKind::Emphasis, Effect::Grow, Start::WithPrevious, 0, 1000, None),
        (s3, AnimKind::Motion, Effect::Path, Start::OnClick, 0, 2000, None),
        // An effect we don't have: a fade, on the same timing.
        (s3, AnimKind::Entrance, Effect::Fade, Start::OnClick, 0, 1500, None),
        (g1, AnimKind::Entrance, Effect::Appear, Start::AfterPrevious, 0, 0, None),
        (g2, AnimKind::Entrance, Effect::Appear, Start::WithPrevious, 0, 0, None),
    ]);
    assert_eq!((a[4].amount, a[6].amount), (Some(180.0), Some(125.0)));
    let moved = a[7].path.as_ref().unwrap().moved();
    assert!(close(moved.0, 25.0) && close(moved.1, 10.0), "{moved:?}");
    assert_eq!(d.steps(), 4);
}

#[test]
fn not_a_presentation_converts_to_nothing() {
    let mut put = |_: &[u8], _: &str| None;
    assert!(convert(Cursor::new(b"nope".to_vec()), &mut put).is_none());
    assert!(convert(Cursor::new(zip_of(&[("mimetype", b"x".to_vec())])), &mut put).is_none());
}

#[test]
fn a_file_converts_into_the_asset_store() {
    let dir = crate::store::test_dir();
    let store = crate::store::Store::open(dir.clone()).unwrap();
    let content = content_xml("", r#"<draw:page draw:style-name="dp1" draw:master-page-name="Default"><draw:frame svg:width="2.8cm" svg:height="1.575cm" svg:x="0cm" svg:y="0cm"><draw:image xlink:href="Pictures/a.png"/></draw:frame></draw:page>"#);
    let path = dir.join("talk.odp");
    std::fs::write(&path, odp_file(&content, &styles_xml("", ("28cm", "15.75cm")), &[("Pictures/a.png", b"PICTURE")])).unwrap();
    let out = odp_to_designs(&path, &store).unwrap();
    let ObjKind::Picture { asset, .. } = &out.designs[0].objects[1].kind else { panic!() };
    assert!(asset.ends_with(".png"));
    assert_eq!(std::fs::read(store.asset_path(asset)).unwrap(), b"PICTURE");
    std::fs::write(&path, b"not a zip").unwrap();
    assert!(odp_to_designs(&path, &store).is_err());
    let _ = std::fs::remove_dir_all(dir);
}
