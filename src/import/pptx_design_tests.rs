use super::*;
use std::io::{Cursor, Write};
use crate::design::{Transition, TransitionKind};

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const RT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn zip_of(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        w.start_file(name.as_str(), opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn rels(list: &[(&str, &str, &str)]) -> String {
    let r: String = list.iter().map(|(id, ty, target)| format!(r#"<Relationship Id="{id}" Type="{RT}/{ty}" Target="{target}"/>"#)).collect();
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{r}</Relationships>"#)
}

const THEME: &str = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office"><a:themeElements>
<a:clrScheme name="Church"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
<a:dk2><a:srgbClr val="1F2A44"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2>
<a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6>
<a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme>
<a:fontScheme name="Office"><a:majorFont><a:latin typeface="Georgia"/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/></a:minorFont></a:fontScheme>
</a:themeElements></a:theme>"#;

/// A master like PowerPoint's: dark-blue background, title (44 pt, major font, tx2) and body (bullets, 28 pt).
fn master() -> String {
    format!(r#"<p:sldMaster {NS}><p:cSld><p:bg><p:bgPr><a:solidFill><a:schemeClr val="tx2"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title Placeholder 1"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="838200" y="365125"/><a:ext cx="10515600" cy="1325563"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>
<p:txBody><a:bodyPr anchor="ctr"><a:normAutofit/></a:bodyPr><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master title style</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="3" name="Text Placeholder 2"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="838200" y="1825625"/><a:ext cx="10515600" cy="4351338"/></a:xfrm></p:spPr>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Edit Master text styles</a:t></a:r></a:p></p:txBody></p:sp>
<p:pic><p:nvPicPr><p:cNvPr id="7" name="Logo" descr="Church logo"/><p:cNvPicPr/><p:nvPr userDrawn="1"/></p:nvPicPr>
<p:blipFill><a:blip r:embed="rId3"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>
<p:spPr><a:xfrm><a:off x="11277600" y="6096000"/><a:ext cx="609600" cy="609600"/></a:xfrm><a:prstGeom prst="rect"/></p:spPr></p:pic>
</p:spTree></p:cSld>
<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>
<p:txStyles>
<p:titleStyle><a:lvl1pPr algn="l"><a:defRPr sz="4400" kern="1200"><a:solidFill><a:schemeClr val="bg1"/></a:solidFill><a:latin typeface="+mj-lt"/></a:defRPr></a:lvl1pPr></p:titleStyle>
<p:bodyStyle><a:lvl1pPr marL="228600" indent="-228600" algn="l"><a:buFont typeface="Arial"/><a:buChar char="&#8226;"/><a:defRPr sz="2800"><a:solidFill><a:schemeClr val="bg1"><a:lumMod val="85000"/></a:schemeClr></a:solidFill><a:latin typeface="+mn-lt"/></a:defRPr></a:lvl1pPr>
<a:lvl2pPr marL="685800" indent="-228600" algn="l"><a:buChar char="&#8226;"/><a:defRPr sz="2400"><a:solidFill><a:schemeClr val="bg1"/></a:solidFill><a:latin typeface="+mn-lt"/></a:defRPr></a:lvl2pPr></p:bodyStyle>
<p:otherStyle><a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr></p:otherStyle>
</p:txStyles></p:sldMaster>"#)
}

/// "Title and Content", which moves the title down a little; it shows the master's logo.
fn layout() -> String {
    format!(r#"<p:sldLayout {NS} type="obj" preserve="1"><p:cSld name="Title and Content"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="838200" y="685800"/><a:ext cx="10515600" cy="1325563"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Click to edit</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="3" name="Content Placeholder 2"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr><p:spPr/>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr lvl="0"/><a:r><a:t>Edit</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#)
}

fn sp(id: u32, name: &str, nv_pr: &str, sp_pr: &str, body: &str) -> String {
    let tx = if body.is_empty() { String::new() } else { format!("<p:txBody><a:bodyPr/><a:lstStyle/>{body}</p:txBody>") };
    format!(r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr/><p:nvPr>{nv_pr}</p:nvPr></p:nvSpPr><p:spPr>{sp_pr}</p:spPr>{tx}</p:sp>"#)
}

fn xf(x: i64, y: i64, cx: i64, cy: i64) -> String { format!(r#"<a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#) }

fn slide_xml(shapes: &str, extra: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>{extra}</p:sld>"#)
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, _| if x < w / 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

/// A presentation of `slides` (each: slide XML, extra slide rels), shown in the given order, `size` in EMU.
fn pptx(slides: &[(String, Vec<(&str, &str, &str)>)], order: &[usize], size: (i64, i64)) -> Vec<u8> { pptx_with(slides, order, size, vec![]) }

/// `pptx` with more parts.
fn pptx_with(slides: &[(String, Vec<(&str, &str, &str)>)], order: &[usize], size: (i64, i64), more: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let list: String = order.iter().map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 256 + i, 10 + i)).collect();
    let pres = format!(r#"<p:presentation {NS}><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{list}</p:sldIdLst><p:sldSz cx="{}" cy="{}"/><p:notesSz cx="6858000" cy="9144000"/>
<p:defaultTextStyle><a:lvl1pPr algn="l"><a:defRPr sz="1800"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle></p:presentation>"#, size.0, size.1);
    let mut pres_rels = vec![("rId1".to_string(), "slideMaster", "slideMasters/slideMaster1.xml".to_string())];
    for i in 0..slides.len() { pres_rels.push((format!("rId{}", 10 + i), "slide", format!("slides/slide{}.xml", i + 1))) }
    let pr: Vec<(&str, &str, &str)> = pres_rels.iter().map(|(a, b, c)| (a.as_str(), *b, c.as_str())).collect();
    let mut all: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/></Types>"#.to_vec()),
        ("ppt/presentation.xml".into(), pres.into_bytes()),
        ("ppt/_rels/presentation.xml.rels".into(), rels(&pr).into_bytes()),
        ("ppt/slideMasters/slideMaster1.xml".into(), master().into_bytes()),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels".into(), rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"), ("rId2", "theme", "../theme/theme1.xml"), ("rId3", "image", "../media/logo.png")]).into_bytes()),
        ("ppt/slideLayouts/slideLayout1.xml".into(), layout().into_bytes()),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels".into(), rels(&[("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")]).into_bytes()),
        ("ppt/theme/theme1.xml".into(), THEME.as_bytes().to_vec()),
        ("ppt/media/logo.png".into(), png(4, 4)),
        ("ppt/media/photo.png".into(), png(100, 50)),
        ("ppt/media/clip.mp4".into(), b"not really a video".to_vec()),
        ("ppt/media/old.emf".into(), b"emf".to_vec()),
    ];
    for (i, (xml, extra)) in slides.iter().enumerate() {
        all.push((format!("ppt/slides/slide{}.xml", i + 1), xml.clone().into_bytes()));
        let mut r = vec![("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")];
        r.extend(extra.iter().copied());
        all.push((format!("ppt/slides/_rels/slide{}.xml.rels", i + 1), rels(&r).into_bytes()));
    }
    all.extend(more);
    zip_of(&all)
}

/// Runs `convert`, keeping what was put in the store as (asset, bytes).
fn run(file: Vec<u8>) -> (Converted, Vec<(AssetId, Vec<u8>)>) {
    let mut stored = Vec::new();
    let mut put = |b: &[u8], ext: &str| { let id = format!("asset{}.{ext}", stored.len()); stored.push((id.clone(), b.to_vec())); Some(id) };
    let c = convert(Cursor::new(file), &mut put).expect("reads");
    (c, stored)
}

fn texts(d: &Design) -> Vec<(&str, &TextStyle, PctRect)> {
    d.objects.iter().filter_map(|o| match &o.kind { ObjKind::Text { text, style } => Some((text.as_str(), style, o.rect)), _ => None }).collect()
}

fn near(a: f32, b: f32) -> bool { (a - b).abs() < 0.05 }

#[test]
fn placeholders_take_place_and_style_from_layout_and_master() {
    let title = sp(2, "Title 1", r#"<p:ph type="title"/>"#, "", r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>Welcome Home</a:t></a:r></a:p>"#);
    let body = sp(3, "Content Placeholder 2", r#"<p:ph idx="1"/>"#, "", r#"<a:p><a:r><a:rPr lang="en-US"/><a:t>Coffee after the service</a:t></a:r></a:p><a:p><a:pPr lvl="1"/><a:r><a:rPr lang="en-US"/><a:t>in the hall</a:t></a:r></a:p><a:p><a:pPr><a:buNone/></a:pPr><a:r><a:t>All welcome</a:t></a:r></a:p>"#);
    let (c, stored) = run(pptx(&[(slide_xml(&(title + &body), ""), vec![])], &[0], (12_192_000, 6_858_000)));
    let d = &c.slides[0];
    // The master's background (tx2 → dk2), then its logo, then the slide's words.
    assert_eq!(d.objects[0].look.fill.as_deref(), Some("#1f2a44"));
    assert_eq!(d.objects[0].rect, PctRect::new(0.0, 0.0, 100.0, 100.0));
    assert!(matches!(&d.objects[1].kind, ObjKind::Picture { asset, fit: Fit::Stretch } if asset == &stored[0].0), "the master's logo");
    let t = texts(d);
    assert_eq!(t.len(), 2);
    let (words, style, rect) = t[0];
    assert_eq!(words, "Welcome Home");
    // The layout's place: 838200/12192000 = 6.875 %, 685800/6858000 = 10 %.
    assert!(near(rect.x, 6.875) && near(rect.y, 10.0) && near(rect.w, 86.25), "{rect:?}");
    assert_eq!((style.font.as_str(), style.size, style.color.as_str()), ("Georgia", 88.0, "#ffffff"), "44 pt is 88 px on a 1080 slide");
    assert_eq!((style.align, style.valign, style.shrink), (HAlign::Left, VAlign::Middle, true));
    let (words, style, rect) = t[1];
    assert_eq!(words, "• Coffee after the service\n• in the hall\nAll welcome");
    assert!(near(rect.y, 1825625.0 / 6858000.0 * 100.0), "the master's place when the layout has none: {rect:?}");
    assert_eq!((style.font.as_str(), style.size, style.color.as_str(), style.valign), ("Calibri", 56.0, "#d9d9d9", VAlign::Top), "white at 85 % brightness");
    assert_eq!(c.lost, 0);
}

#[test]
fn runs_bring_their_own_look_and_the_box_takes_the_most_used() {
    let body = r#"<a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US" sz="3200" b="1"><a:solidFill><a:srgbClr val="FFC000"/></a:solidFill><a:latin typeface="Montserrat"/></a:rPr><a:t>Sunday Service</a:t></a:r><a:r><a:rPr lang="en-US" sz="3200" i="1"/><a:t> 10am</a:t></a:r></a:p>"#;
    let tb = sp(4, "TextBox 3", "", &(xf(0, 0, 6_096_000, 3_429_000) + r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/>"#), body);
    let (c, _) = run(pptx(&[(slide_xml(&tb.replace("<p:cNvSpPr/>", r#"<p:cNvSpPr txBox="1"/>"#), ""), vec![])], &[0], (12_192_000, 6_858_000)));
    let t = texts(&c.slides[0]);
    let (words, st, rect) = t[0];
    assert_eq!(words, "Sunday Service 10am");
    assert_eq!(rect, PctRect::new(0.0, 0.0, 50.0, 50.0));
    assert_eq!((st.font.as_str(), st.size, st.color.as_str(), st.bold, st.italic, st.align), ("Montserrat", 64.0, "#ffc000", true, false, HAlign::Center));
    assert!(!st.shrink, "PowerPoint's text box does not shrink its words");
    assert!(c.slides[0].objects.iter().find(|o| matches!(o.kind, ObjKind::Text { .. })).unwrap().look.fill.is_none());
}

#[test]
fn shapes_lines_and_their_fills_come_over() {
    let shapes = [
        sp(5, "Oval", "", &(xf(1_219_200, 685_800, 2_438_400, 1_371_600) + r#"<a:prstGeom prst="ellipse"/><a:solidFill><a:srgbClr val="FF0000"><a:alpha val="50000"/></a:srgbClr></a:solidFill><a:ln w="25400"><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:ln>"#), r#"<a:p><a:r><a:t>Joy</a:t></a:r></a:p>"#),
        // PowerPoint's default shape: its look comes from the style references.
        format!(r#"<p:sp><p:nvSpPr><p:cNvPr id="6" name="Rounded Rectangle 5"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst="roundRect"><a:avLst/></a:prstGeom></p:spPr><p:style><a:lnRef idx="2"><a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent1"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></p:style><p:txBody><a:bodyPr rtlCol="0" anchor="ctr"/><a:lstStyle/><a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US"/><a:t>Give</a:t></a:r></a:p></p:txBody></p:sp>"#, xf(0, 0, 1_219_200, 685_800)),
        format!(r#"<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="7" name="Straight Connector 6"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr><a:xfrm flipV="1"><a:off x="0" y="0"/><a:ext cx="6096000" cy="3429000"/></a:xfrm><a:prstGeom prst="line"/><a:ln w="38100"><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></a:ln></p:spPr></p:cxnSp>"#),
        format!(r#"<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="8" name="Arrow 7"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr>{}<a:prstGeom prst="straightConnector1"/><a:ln w="12700"><a:solidFill><a:srgbClr val="000000"/></a:solidFill><a:tailEnd type="triangle"/></a:ln></p:spPr></p:cxnSp>"#, xf(0, 3_429_000, 6_096_000, 0)),
        sp(9, "Heart", "", &(xf(0, 0, 1_219_200, 1_219_200) + r#"<a:prstGeom prst="heart"/><a:solidFill><a:srgbClr val="FF00FF"/></a:solidFill>"#), ""),
        sp(10, "Rotated", "", &(r#"<a:xfrm rot="2700000"><a:off x="0" y="0"/><a:ext cx="1219200" cy="1219200"/></a:xfrm><a:prstGeom prst="triangle"/><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill>"#.to_string()), ""),
    ].concat();
    let (c, _) = run(pptx(&[(slide_xml(&shapes, ""), vec![])], &[0], (12_192_000, 6_858_000)));
    let objs = &c.slides[0].objects[2..];
    // The oval and its words over it.
    assert!(matches!(objs[0].kind, ObjKind::Shape { shape: Shape::Ellipse }));
    assert_eq!(objs[0].look.fill.as_deref(), Some("#ff000080"), "half see-through");
    assert_eq!(objs[0].look.outline, Some(Outline { color: "#4472c4".into(), width: 4.0 }));
    assert!(matches!(&objs[1].kind, ObjKind::Text { text, .. } if text == "Joy") && objs[1].rect == objs[0].rect);
    // The rounded box: one text object with the style's fill, outline and font colour.
    let ObjKind::Text { text, style } = &objs[2].kind else { panic!("{:?}", objs[2].kind) };
    assert_eq!((text.as_str(), style.color.as_str(), style.valign), ("Give", "#ffffff", VAlign::Middle));
    assert_eq!(objs[2].look.fill.as_deref(), Some("#4472c4"));
    assert_eq!(objs[2].look.outline.as_ref().map(|o| o.color.as_str()), Some("#223962"));
    assert!(objs[2].look.radius > 10.0);
    // The line runs from bottom left to top right (flipped), 3 pt wide.
    assert_eq!(objs[3].kind, ObjKind::Path { points: vec![(0.0, 1.0), (1.0, 0.0)], width: 6.0, color: "#00ff00".into() });
    // A flat line with an arrow head is the room's arrow.
    assert!(matches!(objs[4].kind, ObjKind::Shape { shape: Shape::Arrow }) && objs[4].look.outline.is_some());
    // A heart has no match: a box in its colour, and counted.
    assert!(matches!(objs[5].kind, ObjKind::Shape { shape: Shape::Rect }) && objs[5].look.fill.as_deref() == Some("#ff00ff"));
    assert_eq!(objs[6].rotation, 45.0);
    assert_eq!(c.lost, 1);
}

#[test]
fn pictures_are_stored_cropped_and_unknown_things_become_grey_boxes() {
    let pic = |id: u32, rid: &str, crop: &str, nv: &str| format!(r#"<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="Picture {id}" descr="Pastor Dave"/><p:cNvPicPr/><p:nvPr>{nv}</p:nvPr></p:nvPicPr><p:blipFill><a:blip r:embed="{rid}"><a:alphaModFix amt="60000"/></a:blip>{crop}<a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst="rect"/></p:spPr></p:pic>"#, xf(0, 0, 6_096_000, 3_429_000));
    let table = format!(r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="20" name="Table 19"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="6096000" y="3429000"/><a:ext cx="6096000" cy="3429000"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tr h="370840"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Time</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Event</a:t></a:r></a:p></a:txBody></a:tc></a:tr><a:tr h="370840"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>9am</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Prayer</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#);
    let video = r#"<a:videoFile r:link="rId4"/><p:extLst><p:ext uri="{DAA4B4D4-6D71-4841-9C94-3DE7FCFB9230}"><p14:media xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main" r:embed="rId4"/></p:ext></p:extLst>"#;
    let shapes = [pic(30, "rId2", r#"<a:srcRect l="50000"/>"#, ""), pic(31, "rId3", "", ""), table, pic(32, "rId2", "", video)].concat();
    let slide = slide_xml(&shapes, "");
    let (c, stored) = run(pptx(&[(slide, vec![("rId2", "image", "../media/photo.png"), ("rId3", "image", "../media/old.emf"), ("rId4", "video", "../media/clip.mp4")])], &[0], (12_192_000, 6_858_000)));
    let objs = &c.slides[0].objects[2..];
    let ObjKind::Picture { asset, .. } = &objs[0].kind else { panic!() };
    let cut = image::load_from_memory(&stored.iter().find(|(a, _)| a == asset).unwrap().1).unwrap();
    assert_eq!((cut.width(), cut.height()), (50, 50), "the left half cut off");
    assert_eq!(cut.to_rgba8().get_pixel(0, 0).0, [0, 0, 255, 255]);
    assert_eq!(objs[0].opacity, 60);
    // A picture we can't draw (EMF) and the table: grey boxes with the description, and the table's words.
    assert!(matches!(&objs[1].kind, ObjKind::Text { text, .. } if text == "Pastor Dave"), "{:?}", objs[1].kind);
    assert_eq!(objs[1].look.fill.as_deref(), Some("#4b5563"), "the same grey box as the .odp import's");
    assert!(matches!(&objs[2].kind, ObjKind::Text { text, .. } if text == "Time  |  Event\n9am  |  Prayer"));
    assert!(near(objs[2].rect.x, 50.0) && near(objs[2].rect.y, 50.0));
    // The video, with its picture as the still.
    let ObjKind::Video { asset, thumb: Some(_), .. } = &objs[3].kind else { panic!("{:?}", objs[3].kind) };
    assert!(asset.ends_with(".mp4"));
    assert_eq!(c.lost, 2);
}

#[test]
fn groups_are_flattened_through_their_frame() {
    // The group's children are drawn at half size: their 0..2438400 frame is 0..1219200 on the slide, at 1219200.
    let group = format!(r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="40" name="Group 39"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="1219200" y="685800"/><a:ext cx="1219200" cy="685800"/><a:chOff x="0" y="0"/><a:chExt cx="2438400" cy="1371600"/></a:xfrm></p:grpSpPr>{}{}</p:grpSp>"#,
        sp(41, "A", "", &(xf(0, 0, 1_219_200, 685_800) + r#"<a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="111111"/></a:solidFill>"#), ""),
        sp(42, "B", "", &(xf(1_219_200, 685_800, 1_219_200, 685_800) + r#"<a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="222222"/></a:solidFill>"#), ""));
    let (c, _) = run(pptx(&[(slide_xml(&group, ""), vec![])], &[0], (12_192_000, 6_858_000)));
    let objs = &c.slides[0].objects[2..];
    let r = |i: usize| objs[i].rect;
    assert!(near(r(0).x, 10.0) && near(r(0).y, 10.0) && near(r(0).w, 5.0) && near(r(0).h, 5.0), "{:?}", r(0));
    assert!(near(r(1).x, 15.0) && near(r(1).y, 15.0), "{:?}", r(1));
}

#[test]
fn a_four_by_three_slide_sits_in_the_middle() {
    let tb = sp(4, "TextBox", "", &(xf(0, 0, 9_144_000, 6_858_000) + r#"<a:prstGeom prst="rect"/>"#), r#"<a:p><a:r><a:rPr sz="2400"/><a:t>Hi</a:t></a:r></a:p>"#);
    let (c, _) = run(pptx(&[(slide_xml(&tb, ""), vec![])], &[0], (9_144_000, 6_858_000)));
    let (_, st, rect) = texts(&c.slides[0])[0];
    assert!(near(rect.x, 12.5) && near(rect.w, 75.0) && near(rect.h, 100.0), "{rect:?}");
    assert_eq!(st.size, 48.0, "24 pt on a 7.5 inch high slide shown 1080 high");
}

#[test]
fn slides_come_in_show_order_with_notes_backgrounds_and_transitions() {
    let notes = r#"<p:notes xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="3" name="Notes"/><p:cNvSpPr/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:t>Pray first</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:notes>"#;
    let picture_bg = format!(r#"<?xml version="1.0"?><p:sld {NS} showMasterSp="0"><p:cSld><p:bg><p:bgPr><a:blipFill><a:blip r:embed="rId2"/><a:stretch/></a:blipFill></p:bgPr></p:bg><p:spTree/></p:cSld><p:transition spd="slow"><p:cover dir="l"/></p:transition></p:sld>"#);
    let hidden = format!(r#"<?xml version="1.0"?><p:sld {NS} show="0"><p:cSld><p:spTree/></p:cSld></p:sld>"#);
    let first = slide_xml("", "");
    let more = vec![("ppt/notesSlides/notesSlide1.xml".to_string(), notes.as_bytes().to_vec())];
    let file = pptx_with(&[(first, vec![("rId3", "notesSlide", "../notesSlides/notesSlide1.xml")]), (picture_bg, vec![("rId2", "image", "../media/photo.png")]), (hidden, vec![])], &[1, 2, 0], (12_192_000, 6_858_000), more);
    let (c, _) = run(file);
    assert_eq!(c.slides.len(), 2, "the hidden slide is left out");
    let (a, b) = (&c.slides[0], &c.slides[1]);
    assert_eq!(a.objects.len(), 1, "no master shapes on this one");
    assert!(matches!(a.objects[0].kind, ObjKind::Picture { fit: Fit::Fill, .. }));
    assert_eq!(a.transition, Transition { kind: TransitionKind::Cover, ms: 1000, dir: Dir::Left });
    assert_eq!(b.notes, "Pray first");
    assert_eq!(b.transition, Transition::default(), "no transition in the file: the normal crossfade");
}

/// One effect as PowerPoint writes it, inside its click group.
fn effect(ctn_id: u32, spid: u32, node: &str, class: &str, preset: u32, sub: u32, delay: u32, behaviour: &str) -> String {
    let outer_delay = if node == "clickEffect" { "indefinite" } else { "0" };
    format!(r#"<p:par><p:cTn id="{}" fill="hold"><p:stCondLst><p:cond delay="{outer_delay}"/></p:stCondLst><p:childTnLst><p:par><p:cTn id="{}" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst><p:childTnLst>
<p:par><p:cTn id="{ctn_id}" presetID="{preset}" presetClass="{class}" presetSubtype="{sub}" fill="hold" nodeType="{node}"><p:stCondLst><p:cond delay="{delay}"/></p:stCondLst><p:childTnLst>
<p:set><p:cBhvr><p:cTn id="{}" dur="1" fill="hold"><p:stCondLst><p:cond delay="0"/></p:stCondLst></p:cTn><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl><p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val="visible"/></p:to></p:set>
{behaviour}</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>"#, ctn_id + 100, ctn_id + 200, ctn_id + 300)
}

fn behave(spid: u32, dur: u32, extra: &str) -> String {
    format!(r#"<p:animEffect transition="in" filter="fade"><p:cBhvr><p:cTn id="999" dur="{dur}"/><p:tgtEl><p:spTgt spid="{spid}"/></p:tgtEl></p:cBhvr></p:animEffect>{extra}"#)
}

#[test]
fn animations_become_steps_on_the_objects_made() {
    let shapes = (2..=8).map(|i| sp(i, &format!("S{i}"), "", &(xf(0, 0, 1_219_200, 685_800) + r#"<a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="333333"/></a:solidFill>"#), "")).collect::<String>();
    let oval = sp(9, "Oval", "", &(xf(0, 0, 1_219_200, 685_800) + r#"<a:prstGeom prst="ellipse"/><a:solidFill><a:srgbClr val="333333"/></a:solidFill>"#), r#"<a:p><a:r><a:t>Hi</a:t></a:r></a:p>"#);
    let steps = [
        effect(5, 2, "clickEffect", "entr", 10, 0, 0, &behave(2, 750, "")),
        effect(10, 3, "withEffect", "entr", 2, 8, 250, &behave(3, 500, "")),
        effect(15, 4, "afterEffect", "emph", 8, 0, 0, r#"<p:animRot by="-10800000"><p:cBhvr><p:cTn id="998" dur="2000"/><p:tgtEl><p:spTgt spid="4"/></p:tgtEl></p:cBhvr></p:animRot>"#),
        effect(20, 5, "clickEffect", "exit", 22, 4, 0, &behave(5, 500, "")),
        effect(25, 6, "clickEffect", "path", 0, 0, 0, r#"<p:animMotion origin="layout" path="M 0 0 L 0.25 0.1 E" pathEditMode="relative"><p:cBhvr><p:cTn id="997" dur="2000"/><p:tgtEl><p:spTgt spid="6"/></p:tgtEl></p:cBhvr></p:animMotion>"#),
        effect(30, 7, "clickEffect", "entr", 37, 0, 0, &behave(7, 1000, "")),
        effect(35, 8, "withEffect", "entr", 1, 0, 0, ""),
        effect(40, 9, "clickEffect", "emph", 6, 0, 0, r#"<p:animScale><p:cBhvr><p:cTn id="996" dur="1000"/><p:tgtEl><p:spTgt spid="9"/></p:tgtEl></p:cBhvr><p:by x="200000" y="200000"/></p:animScale>"#),
    ].concat();
    let timing = format!(r#"<p:timing><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst><p:seq concurrent="1" nextAc="seek"><p:cTn id="2" dur="indefinite" nodeType="mainSeq"><p:childTnLst>{steps}</p:childTnLst></p:cTn><p:prevCondLst/><p:nextCondLst/></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>"#);
    let (c, _) = run(pptx(&[(slide_xml(&(shapes + &oval), &timing), vec![])], &[0], (12_192_000, 6_858_000)));
    let d = &c.slides[0];
    let id = |name_index: usize| d.objects[2 + name_index].id;
    let a = &d.anims;
    assert_eq!(a.len(), 9, "{a:#?}");
    let row = |x: &Anim| (x.kind, x.effect, x.start, x.delay_ms, x.dur_ms, x.dir);
    assert_eq!((a[0].obj, row(&a[0])), (id(0), (AnimKind::Entrance, Effect::Fade, Start::OnClick, 0, 750, None)));
    assert_eq!((a[1].obj, row(&a[1])), (id(1), (AnimKind::Entrance, Effect::Fly, Start::WithPrevious, 250, 500, Some(Dir::Left))));
    assert_eq!((row(&a[2]), a[2].amount), ((AnimKind::Emphasis, Effect::Spin, Start::AfterPrevious, 0, 2000, None), Some(-180.0)));
    assert_eq!(row(&a[3]), (AnimKind::Exit, Effect::Wipe, Start::OnClick, 0, 500, Some(Dir::Down)));
    assert_eq!(row(&a[4]).0, AnimKind::Motion);
    let moved = a[4].path.as_ref().unwrap().moved();
    assert!(near(moved.0, 25.0) && near(moved.1, 10.0), "{moved:?}");
    assert_eq!(row(&a[5]), (AnimKind::Entrance, Effect::Fade, Start::OnClick, 0, 1000, None), "a preset we don't have fades");
    assert_eq!(row(&a[6]), (AnimKind::Entrance, Effect::Appear, Start::WithPrevious, 0, 0, None));
    // The oval made a shape and its words: both grow, together.
    assert_eq!((a[7].obj, a[8].obj, a[7].effect, a[7].amount, a[8].start), (id(7), id(8), Effect::Grow, Some(200.0), Start::WithPrevious));
    assert_eq!(d.steps(), 5);
}

#[test]
fn powerpoint_preset_table() {
    let cv_preset = |class: &str, id: u32, sub: u32| {
        let doc = Document::parse("<x/>").unwrap();
        let geo = Geo { s: 1.0, ox: 0.0, oy: 0.0, cx: W, cy: H };
        let cv = Conv { geo: &geo, major: String::new(), minor: String::new(), title: vec![], body: vec![], other: vec![], deflt: vec![], layout: None, master: None, colors: Colors::from_theme(None) };
        cv.preset(doc.root_element(), class, id, sub).map(|a| (a.kind, a.effect, a.dir))
    };
    use AnimKind::*;
    let e = |id, sub| cv_preset("entr", id, sub).map(|(_, e, d)| (e, d));
    assert_eq!(e(1, 0), Some((Effect::Appear, None)));
    assert_eq!(e(10, 0), Some((Effect::Fade, None)));
    assert_eq!(e(2, 4), Some((Effect::Fly, Some(Dir::Down))));
    assert_eq!(e(2, 1), Some((Effect::Fly, Some(Dir::Up))));
    assert_eq!(e(2, 8), Some((Effect::Fly, Some(Dir::Left))));
    assert_eq!(e(2, 2), Some((Effect::Fly, Some(Dir::Right))));
    assert_eq!(e(22, 8), Some((Effect::Wipe, Some(Dir::Left))));
    assert_eq!(e(16, 21), Some((Effect::Split, None)));
    assert_eq!(e(53, 16), Some((Effect::Zoom, None)));
    assert_eq!(e(23, 16), Some((Effect::Zoom, None)));
    assert_eq!(e(42, 0), Some((Effect::Float, None)));
    assert_eq!(e(47, 0), Some((Effect::Float, Some(Dir::Up))));
    assert_eq!(e(26, 0), Some((Effect::Fade, None)), "Bounce: fades");
    assert_eq!(cv_preset("exit", 10, 0), Some((Exit, Effect::Fade, None)));
    assert_eq!(cv_preset("emph", 8, 0), Some((Emphasis, Effect::Spin, None)));
    assert_eq!(cv_preset("emph", 6, 0), Some((Emphasis, Effect::Grow, None)));
    assert_eq!(cv_preset("emph", 26, 0), Some((Emphasis, Effect::Pulse, None)));
    assert_eq!(cv_preset("emph", 32, 0), Some((Emphasis, Effect::Pulse, None)), "Teeter pulses");
    assert_eq!(cv_preset("path", 1, 0), None, "a path with no path is left out");
    assert_eq!(cv_preset("verb", 1, 0), None);
}

#[test]
fn not_a_presentation_reads_as_none() {
    let mut put = |_: &[u8], _: &str| None;
    assert!(convert(Cursor::new(b"nope".to_vec()), &mut put).is_none());
}

#[tokio::test]
async fn the_job_puts_the_slides_in_the_service_and_says_what_was_lost() {
    use crate::app::{test_app, ImportPurpose, Imported};
    let app = test_app();
    let heart = sp(9, "Heart", "", &(xf(0, 0, 1_219_200, 1_219_200) + r#"<a:prstGeom prst="heart"/><a:solidFill><a:srgbClr val="FF00FF"/></a:solidFill>"#), "");
    let dir = std::env::temp_dir().join(format!("v11-pptx-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Welcome.pptx");
    std::fs::write(&file, pptx(&[(slide_xml(&heart, ""), vec![])], &[0], (12_192_000, 6_858_000))).unwrap();
    let Imported::Job(job) = app.import_path(&file, ImportPurpose::Slides, |_| {}).await.unwrap() else { panic!("no job") };
    let began = std::time::Instant::now();
    while !app.job_ends.lock().unwrap().iter().any(|(j, _)| *j == job) {
        assert!(began.elapsed() < std::time::Duration::from_secs(20), "timed out");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let made = app.job_ends.lock().unwrap().iter().find(|(j, _)| *j == job).unwrap().1.expect("made an item");
    let d = app.data.lock().unwrap().clone();
    let item = d.service().unwrap().items.iter().find(|i| i.id == made).unwrap().clone();
    let ItemKind::Design { title, slides } = item.kind else { panic!() };
    assert_eq!((title.as_str(), slides.len(), slides[0].objects.len()), ("Welcome", 1, 3));
    assert!(slides[0].assets().iter().all(|a| app.store.asset_path(a).exists()), "the logo is in the store");
    let card = app.jobs.lock().unwrap().iter().find(|j| j.id == job).cloned().expect("the card stays for its note");
    assert_eq!(card.note.as_deref(), Some(lost_note(1).as_str()));
    assert_eq!(lost_note(1), "1 object couldn't be converted; use Pictures for an exact look.");
}

#[tokio::test]
async fn an_odp_file_comes_in_through_the_same_job() {
    use crate::app::{test_app, ImportPurpose, Imported};
    let app = test_app();
    let content = r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0"><office:body><office:presentation><draw:page draw:name="1"/><draw:page draw:name="2"/></office:presentation></office:body></office:document-content>"#;
    let file = std::env::temp_dir().join(format!("v11-pptx-{}.odp", Uuid::new_v4()));
    std::fs::write(&file, zip_of(&[("mimetype".into(), b"application/vnd.oasis.opendocument.presentation".to_vec()), ("content.xml".into(), content.as_bytes().to_vec())])).unwrap();
    let Imported::Job(job) = app.import_path(&file, ImportPurpose::Slides, |_| {}).await.unwrap() else { panic!("no job") };
    let began = std::time::Instant::now();
    while !app.job_ends.lock().unwrap().iter().any(|(j, _)| *j == job) {
        assert!(began.elapsed() < std::time::Duration::from_secs(20), "timed out");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let made = app.job_ends.lock().unwrap().iter().find(|(j, _)| *j == job).unwrap().1.expect("made an item");
    let d = app.data.lock().unwrap().clone();
    let item = d.service().unwrap().items.iter().find(|i| i.id == made).unwrap().clone();
    assert!(matches!(item.kind, ItemKind::Design { ref slides, .. } if slides.len() == 2), "{:?}", item.kind);
}
