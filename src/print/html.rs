//! `PrintDoc` → one self-contained web page (spec §6.4): black on white, no scripts, nothing fetched from elsewhere.
use super::doc::{Block, PrintDoc};
use crate::model::Paper;

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const STYLE: &str = r#"
  body { font-family: "Segoe UI", "Noto Sans", sans-serif; color: #000; background: #fff; font-size: 11pt; line-height: 1.35; margin: 0 auto; max-width: 7in; padding: 0.25in; }
  h1 { font-size: 20pt; margin: 0 0 2pt; }
  .sub { color: #333; margin: 0 0 14pt; }
  h2 { font-size: 15pt; margin: 16pt 0 6pt; border-bottom: 1pt solid #000; padding-bottom: 2pt; }
  h3 { font-size: 13pt; margin: 14pt 0 2pt; }
  table { border-collapse: collapse; width: 100%; }
  td { vertical-align: top; padding: 4pt 6pt 4pt 0; border-bottom: 0.5pt solid #bbb; }
  td.n { width: 2em; text-align: right; color: #444; }
  td.k { width: 7em; color: #444; }
  .t { font-weight: 600; }
  .d { color: #333; }
  .notes { font-style: italic; color: #333; margin-top: 2pt; white-space: pre-wrap; }
  tr.blank .t, tr.blank .d { font-style: italic; color: #666; font-weight: normal; }
  tr.hdr td { font-weight: 700; text-transform: uppercase; letter-spacing: 0.05em; padding-top: 10pt; border-bottom: 1pt solid #000; }
  .credit { color: #333; font-size: 8.5pt; margin: 0; }
  .order { font-size: 9.5pt; margin: 4pt 0 6pt; }
  .sec { break-inside: avoid; page-break-inside: avoid; margin: 6pt 0; }
  .sec b { display: block; font-size: 9.5pt; }
  .sec p { margin: 0; white-space: pre-wrap; }
  .para { margin: 4pt 0 8pt; }
  .brk { break-before: page; page-break-before: always; }
  footer { margin-top: 24pt; color: #555; font-size: 8.5pt; }
  @media print { body { padding: 0; max-width: none; } }
"#;

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "song" => "Song",
        "scripture" => "Scripture",
        "media" => "Media",
        "deck" => "Slides",
        "text" => "Announcement",
        _ => "",
    }
}

pub fn render(doc: &PrintDoc, paper: Paper, footer: &str) -> String {
    let size = match paper { Paper::Letter => "letter", Paper::A4 => "A4" };
    let mut h = format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>{}</title>\n<style>@page {{ size: {size}; margin: 0.75in; }}{STYLE}</style></head><body>\n<h1>{}</h1>\n",
        escape(&doc.title), escape(&doc.title));
    if !doc.subtitle.is_empty() { h += &format!("<p class=\"sub\">{}</p>\n", escape(&doc.subtitle)) }
    let mut table = false;
    let mut brk = false;
    let close = |h: &mut String, table: &mut bool| if *table { h.push_str("</table>\n"); *table = false };
    for b in &doc.blocks {
        match b {
            Block::OrderRow { n, kind, title, detail, notes, blank } => {
                if std::mem::take(&mut brk) { h.push_str("<div class=\"brk\"></div>\n") }
                if !table { h.push_str("<table>\n"); table = true }
                h += &format!("<tr{}><td class=\"n\">{n}</td><td class=\"k\">{}</td><td><div class=\"t\">{}</div>",
                    if *blank { " class=\"blank\"" } else { "" }, kind_label(kind), escape(title));
                if !detail.is_empty() { h += &format!("<div class=\"d\">{}</div>", escape(detail)) }
                if !notes.is_empty() { h += &format!("<div class=\"notes\">{}</div>", escape(notes)) }
                h += "</td></tr>\n";
            }
            Block::OrderHeader(t) => {
                if !table { h.push_str("<table>\n"); table = true }
                h += &format!("<tr class=\"hdr\"><td colspan=\"3\">{}</td></tr>\n", escape(t));
            }
            Block::PageBreak => { close(&mut h, &mut table); brk = true }
            other => {
                close(&mut h, &mut table);
                if std::mem::take(&mut brk) { h.push_str("<div class=\"brk\"></div>\n") }
                h += &match other {
                    Block::Heading(t) if t == super::strings::ORDER_HEADING || t == super::strings::LYRICS_HEADING => format!("<h2>{}</h2>\n", escape(t)),
                    Block::Heading(t) => format!("<h3>{}</h3>\n", escape(t)),
                    Block::Credit(t) => format!("<p class=\"credit\">{}</p>\n", escape(t)),
                    Block::Order(t) => format!("<p class=\"order\">{}</p>\n", escape(t)),
                    Block::Para(t) => format!("<p class=\"para\">{}</p>\n", escape(t)),
                    Block::Section { label, lines } => format!("<div class=\"sec\"><b>{}</b><p>{}</p></div>\n", escape(label), escape(&lines.join("\n"))),
                    _ => String::new(),
                };
            }
        }
    }
    close(&mut h, &mut table);
    h += &format!("<footer>{}</footer>\n</body></html>\n", escape(footer));
    h
}
