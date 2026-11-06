//! The report as a spreadsheet (CSV) and as a page to print (FEAT-7 §5.4). Both are pure.
use super::{ReportRow, strings};
use chrono::NaiveDate;

/// The file name the save dialog starts from: `song-use-<from>-to-<to>.<ext>`.
pub fn file_name(from: NaiveDate, to: NaiveDate, ext: &str) -> String { format!("song-use-{}-to-{}.{ext}", day(from), day(to)) }

pub fn day(d: NaiveDate) -> String { d.format("%Y-%m-%d").to_string() }

/// One CSV cell. Cells that would start a formula in a spreadsheet get a leading `'`; cells with a comma, quote,
/// line break or edge space are quoted, with quotes doubled.
fn cell(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@']) { format!("'{text}") } else { text.to_string() };
    if text.contains([',', '"', '\n', '\r']) || text.starts_with(' ') || text.ends_with(' ') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

fn line(cells: &[String]) -> String { cells.iter().map(|c| cell(c)).collect::<Vec<_>>().join(",") + "\r\n" }

/// `Title,CCLI Song #,Writers,Times shown,Times streamed,Last used`, after a licence line when there is a licence
/// number. Starts with a byte-order mark so Excel reads accented titles right; lines end in CRLF.
pub fn to_csv(rows: &[ReportRow], license: &str) -> String {
    let mut out = String::from("\u{feff}");
    if !license.trim().is_empty() {
        out += &line(&[strings::CSV_LICENSE.into(), license.trim().into()]);
        out += "\r\n";
    }
    out += &line(&strings::CSV_HEADER.map(String::from));
    for r in rows {
        out += &line(&[r.title.clone(), r.ccli.clone(), r.writers.clone(), r.shown.to_string(), r.streamed.to_string(), day(r.last_used)]);
    }
    out
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// A page to print or save as PDF from the browser: title, dates, licence, the table and the CCLI note.
pub fn to_html(rows: &[ReportRow], license: &str, range: (NaiveDate, NaiveDate)) -> String {
    let license_line = if license.trim().is_empty() { strings::NO_LICENSE.to_string() } else { strings::license_line(license.trim()) };
    let mut body = String::new();
    if rows.is_empty() {
        body += &format!("<p class=\"none\">{}</p>\n", esc(strings::PAGE_NO_SONGS));
    } else {
        body += "<table>\n<thead><tr>";
        for (i, h) in strings::CSV_HEADER.iter().enumerate() {
            body += &format!("<th{}>{}</th>", if (3..5).contains(&i) { " class=\"n\"" } else { "" }, esc(h));
        }
        body += "</tr></thead>\n<tbody>\n";
        for r in rows {
            body += &format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td>{}</td></tr>\n",
                esc(&r.title), esc(&r.ccli), esc(&r.writers), r.shown, r.streamed, day(r.last_used),
            );
        }
        body += "</tbody>\n</table>\n";
    }
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n<style>\n\
body {{ font-family: system-ui, sans-serif; color: #111; margin: 32px; }}\n\
h1 {{ font-size: 22px; margin: 0 0 4px; }}\n\
.meta {{ color: #444; margin: 0 0 16px; }}\n\
table {{ border-collapse: collapse; width: 100%; font-size: 14px; }}\n\
th, td {{ border-bottom: 1px solid #ccc; padding: 6px 8px; text-align: left; vertical-align: top; }}\n\
th {{ border-bottom: 2px solid #111; }}\n\
.n {{ text-align: right; }}\n\
.note {{ color: #444; font-size: 13px; margin-top: 18px; }}\n\
@media print {{ .hint {{ display: none; }} body {{ margin: 0; }} }}\n\
</style>\n</head>\n<body>\n<h1>{title}</h1>\n<p class=\"meta\">{range}<br>{license}</p>\n<p class=\"hint note\">{hint}</p>\n{body}<p class=\"note\">{footer}</p>\n</body>\n</html>\n",
        title = esc(strings::TITLE),
        range = esc(&strings::page_range(&day(range.0), &day(range.1))),
        license = esc(&license_line),
        hint = esc(strings::PAGE_PRINT_HINT),
        footer = esc(strings::FOOTER),
    )
}
