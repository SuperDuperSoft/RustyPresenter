//! N32: the words of a PowerPoint file (.pptx and its kin), slide by slide, for editable text slides. Only the text
//! comes over: pictures, shapes and layout stay behind, which is why pictures (`deck`) stay the usual way in.
use quick_xml::{Reader, events::Event};
use std::{collections::HashMap, io::Read, path::Path};

/// The most of one part that is read: slide XML is small.
const MAX_PART: u64 = 16 << 20;
pub const EXT: &[&str] = &["pptx", "ppsx", "potx", "pptm"];

fn not_pptx() -> String { crate::tr!("Only PowerPoint files saved as .pptx can be read as words. Save it as .pptx in PowerPoint, or add it as pictures.") }

fn part(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut s = String::new();
    zip.by_name(name).ok()?.take(MAX_PART).read_to_string(&mut s).ok()?;
    Some(s)
}

/// Each slide's words, in the order the presentation shows them: paragraphs on lines of their own. A slide with
/// no words is an empty string.
pub fn slide_texts(path: &Path) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| not_pptx())?;
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    let order = part(&mut zip, "ppt/presentation.xml").zip(part(&mut zip, "ppt/_rels/presentation.xml.rels"))
        .map(|(p, r)| slide_order(&p, &r)).unwrap_or_default();
    let order = if order.is_empty() { numbered(&names) } else { order };
    if order.is_empty() { return Err(not_pptx()) }
    Ok(order.iter().map(|name| part(&mut zip, name).map(|x| words(&x)).unwrap_or_default()).collect())
}

/// `ppt/slides/slideN.xml` parts by number, for a file whose slide list cannot be read.
fn numbered(names: &[String]) -> Vec<String> {
    let mut slides: Vec<(u32, String)> = names.iter().filter_map(|n| {
        let num = n.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse().ok()?;
        Some((num, n.clone()))
    }).collect();
    slides.sort();
    slides.into_iter().map(|(_, n)| n).collect()
}

fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == key || a.key.local_name().as_ref() == key).and_then(|a| a.unescape_value().ok()).map(|v| v.into_owned())
}

/// The slide parts in show order: `p:sldIdLst` in presentation.xml, through its relationships.
pub fn slide_order(presentation: &str, rels: &str) -> Vec<String> {
    let mut targets = HashMap::new();
    let mut r = Reader::from_str(rels);
    loop {
        match r.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == b"Relationship" => {
                if let (Some(id), Some(t)) = (attr(&e, b"Id"), attr(&e, b"Target")) { targets.insert(id, t); }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let mut out = vec![];
    let mut r = Reader::from_str(presentation);
    loop {
        match r.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == b"sldId" => {
                let Some(t) = e.attributes().flatten().find(|a| a.key.as_ref() == b"r:id").and_then(|a| a.unescape_value().ok()).and_then(|id| targets.get(id.as_ref()).cloned()) else { continue };
                let t = t.trim_start_matches('/');
                out.push(if t.starts_with("ppt/") { t.to_string() } else { format!("ppt/{t}") });
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// A slide part's words: each `a:p` a line (`a:br` breaks it), empty lines left out.
pub fn words(xml: &str) -> String {
    let mut r = Reader::from_str(xml);
    let (mut lines, mut line, mut in_text) = (Vec::<String>::new(), String::new(), false);
    let end_line = |line: &mut String, lines: &mut Vec<String>| {
        let t = line.trim();
        if !t.is_empty() { lines.push(t.to_string()) }
        line.clear();
    };
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"t" => in_text = true,
            Ok(Event::End(e)) if e.local_name().as_ref() == b"t" => in_text = false,
            Ok(Event::Text(t)) if in_text => if let Ok(t) = t.xml10_content() { line.push_str(&t) },
            Ok(Event::GeneralRef(e)) if in_text => if let Ok(Some(c)) = e.resolve_char_ref() { line.push(c) } else {
                line.push_str(match e.decode().as_deref() { Ok("amp") => "&", Ok("lt") => "<", Ok("gt") => ">", Ok("quot") => "\"", Ok("apos") => "'", _ => "" })
            },
            Ok(Event::Empty(e)) if e.local_name().as_ref() == b"br" => end_line(&mut line, &mut lines),
            Ok(Event::End(e)) if e.local_name().as_ref() == b"p" => end_line(&mut line, &mut lines),
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    end_line(&mut line, &mut lines);
    lines.join("\n")
}

/// The words as an announcement's body: one slide each, empty slides left out. With the number left out.
pub fn announcement_body(slides: &[String]) -> (String, usize) {
    let kept: Vec<&str> = slides.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    (kept.join("\n\n"), slides.len() - kept.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SLIDE: &str = r#"<p:sld xmlns:a="a" xmlns:p="p"><p:cSld><p:spTree>
        <p:sp><p:txBody><a:p><a:r><a:t>Welcome</a:t></a:r><a:r><a:t> home</a:t></a:r></a:p><a:p></a:p></p:txBody></p:sp>
        <p:sp><p:txBody><a:p><a:r><a:t>Coffee &amp; cake</a:t></a:r><a:br/><a:r><a:t>after church</a:t></a:r></a:p></p:txBody></p:sp>
        </p:spTree></p:cSld></p:sld>"#;

    #[test]
    fn a_slides_words_are_its_paragraphs() {
        assert_eq!(words(SLIDE), "Welcome home\nCoffee & cake\nafter church");
        assert_eq!(words("<p:sld/>"), "");
    }

    #[test]
    fn slides_come_in_show_order() {
        let pres = r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="257" r:id="rId3"/><p:sldId id="256" r:id="rId2"/></p:sldIdLst></p:presentation>"#;
        let rels = r#"<Relationships><Relationship Id="rId2" Target="slides/slide1.xml"/><Relationship Id="rId3" Target="slides/slide2.xml"/></Relationships>"#;
        assert_eq!(slide_order(pres, rels), ["ppt/slides/slide2.xml", "ppt/slides/slide1.xml"]);
        assert_eq!(numbered(&["ppt/slides/slide10.xml".into(), "ppt/slides/slide2.xml".into(), "ppt/media/x.png".into()]), ["ppt/slides/slide2.xml", "ppt/slides/slide10.xml"]);
    }

    #[test]
    fn a_pptx_file_reads_and_empty_slides_drop_out() {
        let dir = std::env::temp_dir().join(format!("v1-screens-pptx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("talk.pptx");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (name, body) in [("ppt/slides/slide1.xml", SLIDE), ("ppt/slides/slide2.xml", "<p:sld/>"), ("ppt/slides/slide3.xml", "<p:sld><a:p><a:t>Amen</a:t></a:p></p:sld>")] {
            z.start_file(name, o).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        let slides = slide_texts(&path).unwrap();
        assert_eq!(slides.len(), 3);
        assert_eq!(announcement_body(&slides), ("Welcome home\nCoffee & cake\nafter church\n\nAmen".to_string(), 1));
        std::fs::write(dir.join("old.ppt"), b"not a zip").unwrap();
        assert!(slide_texts(&dir.join("old.ppt")).unwrap_err().contains(".pptx"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
