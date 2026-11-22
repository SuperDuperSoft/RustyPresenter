//! RTF → the text it shows. For song files that keep a slide's words as an RTF document (ProPresenter 6). Reads
//! `\uN` Unicode escapes with their `\ucN` fallback characters, `\'xx` bytes in the code page of the font in use
//! (`\fcharsetN`, `\ansicpg`, two-byte code pages included), the usual symbol words, and leaves out the font
//! table, colour table and the other destinations that hold no text. Formatting is ignored.
use encoding_rs::Encoding;
use std::collections::HashMap;

/// Paragraph properties and skip state, saved at `{` and restored at `}`.
#[derive(Clone)]
struct Group { skip: bool, font_table: bool, uc: usize, font: Option<i32> }

// Destinations whose text is never shown.
const SKIP_DESTINATIONS: &[&str] = &["colortbl", "stylesheet", "info", "pict", "object", "header", "footer", "headerl", "headerr", "footerl",
    "footerr", "listtable", "listoverridetable", "rsidtbl", "generator", "xmlnstbl", "themedata", "colorschememapping", "latentstyles",
    "datastore", "filetbl", "revtbl", "nonshppict", "shppict"];

struct Rtf {
    group: Group,
    stack: Vec<Group>,
    line: String,
    /// Consecutive `\'xx` bytes, decoded together in the active code page (two-byte code pages need the pair).
    bytes: Vec<u8>,
    high_surrogate: Option<u32>,
    /// `\ucN` fallback characters still to skip after a `\uN`.
    fallback: usize,
    ansi_cp: u32,
    /// `\deffN`: the font used where no `\fN` is given.
    deff: Option<i32>,
    font_cp: HashMap<i32, u32>,
    defining_font: Option<i32>,
    /// The text so far, one entry per paragraph or line break.
    lines: Vec<String>,
}

impl Rtf {
    fn new() -> Self {
        Self { group: Group { skip: false, font_table: false, uc: 1, font: None }, stack: vec![], line: String::new(), bytes: vec![],
            high_surrogate: None, fallback: 0, ansi_cp: 1252, deff: None, font_cp: HashMap::new(), defining_font: None, lines: vec![] }
    }

    fn encoding(&self) -> &'static Encoding {
        code_page(self.group.font.or(self.deff).and_then(|f| self.font_cp.get(&f).copied()).unwrap_or(self.ansi_cp))
    }

    fn flush_bytes(&mut self) {
        if self.bytes.is_empty() { return; }
        let bytes = std::mem::take(&mut self.bytes);
        self.line.push_str(&self.encoding().decode_without_bom_handling(&bytes).0);
    }

    /// The buffered bytes end with the first half of a two-byte character. Writers put a trail byte in the
    /// ASCII range as a plain letter (`\'83A` is Shift-JIS "ア"), so the next character belongs to it.
    fn awaiting_trail(&self) -> bool {
        if self.bytes.is_empty() { return false; }
        let enc = self.encoding();
        let lead: fn(u8) -> bool = if enc == encoding_rs::SHIFT_JIS { |b| matches!(b, 0x81..=0x9F | 0xE0..=0xFC) }
            else if [encoding_rs::GBK, encoding_rs::EUC_KR, encoding_rs::BIG5].contains(&enc) { |b| matches!(b, 0x81..=0xFE) }
            else { return false };
        let mut i = 0;
        while i < self.bytes.len() { i += if lead(self.bytes[i]) { 2 } else { 1 }; }
        i > self.bytes.len()
    }

    fn text(&mut self, c: char) {
        if !self.group.skip { self.line.push(c); }
    }

    fn end_line(&mut self) {
        self.flush_bytes();
        if self.group.skip { self.line.clear(); return; }
        self.lines.push(std::mem::take(&mut self.line));
    }

    fn unicode(&mut self, n: i32) {
        let u = if n < 0 { n + 65536 } else { n } as u32;
        match u {
            0xD800..=0xDBFF => { self.high_surrogate = Some(u); return; }
            0xDC00..=0xDFFF => {
                if let Some(h) = self.high_surrogate.take() {
                    if let Some(c) = char::from_u32(0x10000 + ((h - 0xD800) << 10) + (u - 0xDC00)) { self.text(c); }
                }
                return;
            }
            _ => {}
        }
        self.high_surrogate = None;
        if let Some(c) = char::from_u32(u) { self.text(c); }
    }

    fn word(&mut self, name: &str, param: Option<i32>) {
        if self.group.font_table {
            // Inside the font table only the font → code page map matters.
            match name {
                "f" => self.defining_font = param,
                "fcharset" => if let (Some(f), Some(cp)) = (self.defining_font, param.and_then(charset_cp)) { self.font_cp.insert(f, cp); },
                "cpg" => if let (Some(f), Some(cp)) = (self.defining_font, param) { self.font_cp.insert(f, cp as u32); },
                _ => {}
            }
            return;
        }
        if self.group.skip { return; }
        match name {
            "par" | "line" => self.end_line(),
            "u" => if let Some(n) = param { self.unicode(n); self.fallback = self.group.uc; },
            "uc" => self.group.uc = param.unwrap_or(1).max(0) as usize,
            "ansicpg" => if let Some(cp) = param { self.ansi_cp = cp as u32; },
            "deff" => self.deff = param,
            "f" => self.group.font = param,
            "tab" => self.text(' '),
            "emdash" => self.text('\u{2014}'),
            "endash" => self.text('\u{2013}'),
            "lquote" => self.text('\u{2018}'),
            "rquote" => self.text('\u{2019}'),
            "ldblquote" => self.text('\u{201C}'),
            "rdblquote" => self.text('\u{201D}'),
            "bullet" => self.text('\u{2022}'),
            "fonttbl" => { self.group.skip = true; self.group.font_table = true; }
            d if SKIP_DESTINATIONS.contains(&d) => self.group.skip = true,
            // \pard, \qc, \plain, \fsN … are formatting.
            _ => {}
        }
    }

    fn symbol(&mut self, c: char) {
        match c {
            '~' => self.text('\u{a0}'),
            '_' => self.text('\u{2011}'),
            '\\' | '{' | '}' => self.text(c),
            '*' => self.group.skip = true,
            '\n' | '\r' => if !self.group.skip { self.end_line(); },
            // `\-` (optional hyphen) and the rest carry no text.
            _ => {}
        }
    }

    fn plain(&mut self, c: char) { self.text(c); }

    fn parse(mut self, rtf: &str) -> Self {
        let chars: Vec<char> = rtf.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' && chars.get(i + 1) == Some(&'\'') {
                // \'xx: a byte in the active code page; buffered so runs decode together.
                let Some(hex) = chars.get(i + 2..i + 4) else { break };
                i += 4;
                if self.fallback > 0 { self.fallback -= 1; continue; }
                if let Ok(b) = u8::from_str_radix(&hex.iter().collect::<String>(), 16) { if !self.group.skip { self.bytes.push(b); } }
                continue;
            }
            if self.fallback == 0 && c.is_ascii_graphic() && !matches!(c, '\\' | '{' | '}') && self.awaiting_trail() {
                self.bytes.push(c as u8);
                i += 1;
                continue;
            }
            self.flush_bytes();
            match c {
                '{' | '}' => {
                    i += 1;
                    self.fallback = 0;
                    if c == '{' { self.stack.push(self.group.clone()); continue; }
                    if let Some(outer) = self.stack.pop() { self.group = outer; }
                }
                '\\' => {
                    let Some(&n) = chars.get(i + 1) else { break };
                    if n.is_ascii_alphabetic() {
                        let mut j = i + 1;
                        while j < chars.len() && chars[j].is_ascii_alphabetic() { j += 1; }
                        let name: String = chars[i + 1..j].iter().collect();
                        let num_start = j;
                        if j < chars.len() && chars[j] == '-' && chars.get(j + 1).is_some_and(char::is_ascii_digit) { j += 1; }
                        while j < chars.len() && chars[j].is_ascii_digit() { j += 1; }
                        let param = chars[num_start..j].iter().collect::<String>().parse::<i32>().ok();
                        if chars.get(j) == Some(&' ') { j += 1; }
                        i = j;
                        if self.fallback > 0 { self.fallback -= 1; continue; }
                        self.word(&name, param);
                    } else {
                        i += 2;
                        if self.fallback > 0 { self.fallback -= 1; continue; }
                        self.symbol(n);
                    }
                }
                '\r' | '\n' => i += 1,
                _ => {
                    i += 1;
                    if self.fallback > 0 { self.fallback -= 1; continue; }
                    self.plain(c);
                }
            }
        }
        self.flush_bytes();
        if !self.line.is_empty() { self.end_line(); }
        self
    }
}

/// RTF `\fcharsetN` → Windows code page. 0 (ANSI), 1 (default) and unknown charsets use `\ansicpg`.
fn charset_cp(charset: i32) -> Option<u32> {
    Some(match charset {
        77 => 10000, 128 => 932, 129 => 949, 134 => 936, 136 => 950, 161 => 1253, 162 => 1254, 163 => 1258, 177 => 1255,
        178 => 1256, 186 => 1257, 204 => 1251, 222 => 874, 238 => 1250,
        _ => return None,
    })
}

/// Windows code page → decoder. Anything unknown reads as cp1252, the usual Windows default.
fn code_page(cp: u32) -> &'static Encoding {
    use encoding_rs::*;
    match cp {
        874 => WINDOWS_874, 932 => SHIFT_JIS, 936 => GBK, 949 => EUC_KR, 950 => BIG5, 1250 => WINDOWS_1250, 1251 => WINDOWS_1251,
        1253 => WINDOWS_1253, 1254 => WINDOWS_1254, 1255 => WINDOWS_1255, 1256 => WINDOWS_1256, 1257 => WINDOWS_1257, 1258 => WINDOWS_1258,
        10000 => MACINTOSH, 20866 => KOI8_R, 65001 => UTF_8,
        _ => WINDOWS_1252,
    }
}

/// The text an RTF document shows, one entry per paragraph or line break.
pub(crate) fn rtf_text(rtf: &str) -> Vec<String> { Rtf::new().parse(rtf).lines }

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines with something on them, tidied.
    fn text(rtf: &str) -> String {
        rtf_text(rtf).iter().map(|l| l.replace('\t', " ").trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
    }

    const ENCODINGS: &str = r"{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0 Tahoma;}{\f1\fcharset204 Arial;}}It\u8217?s caf\'e9 \u-10179?\u-8704?\par \uc1 na\u239\'efve\par Nbsp\~here a\_b \{brace\} back\\slash\par\f1\'cf\'f0\'e8\'e2\'e5\'f2\par{\*\generator Riched20;}\emdash\tab x\par}";

    const CYRILLIC: &str = r"{\rtf1\ansi\ansicpg1251\deff0{\fonttbl{\f0\fcharset0 Arial;}}\'cf\'f0\'e8\'e2\'e5\'f2\par}";

    #[test]
    fn paragraphs_and_line_breaks_are_lines() {
        assert_eq!(rtf_text(r"{\rtf1\ansi\deff0{\fonttbl{\f0 Tahoma;}}\f0\fs20 First line\par Second line\line Same paragraph\par}"),
            ["First line", "Second line", "Same paragraph"]);
    }

    #[test]
    fn unicode_escapes_code_pages_and_symbols() {
        assert_eq!(text(ENCODINGS), "It\u{2019}s caf\u{e9} \u{1F600}\nna\u{ef}ve\nNbsp\u{a0}here a\u{2011}b {brace} back\\slash\n\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}\n\u{2014} x");
        assert_eq!(text(CYRILLIC), "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}");
    }

    #[test]
    fn empty_rtf_gives_no_text() {
        assert_eq!(text(r"{\rtf1\ansi{\fonttbl{\f0 Tahoma;}}\par\par {\pard\plain}\par}"), "");
    }

    #[test]
    fn central_european_greek_turkish_hebrew_and_japanese_code_pages() {
        // \fcharset238 = cp1250 (Polish), reached through \deff when no \f is given.
        assert_eq!(text(r"{\rtf1\ansi\deff1{\fonttbl{\f0 Arial;}{\f1\fcharset238 Arial;}}Za\'bf\'f3\'b3\'e6\par}"), "Za\u{17c}\u{f3}\u{142}\u{107}");
        assert_eq!(text(r"{\rtf1\ansi\ansicpg1253{\fonttbl{\f0\fcharset0 Arial;}}\'e1\'e3\'e1\'f0\'e7\par}"), "\u{3b1}\u{3b3}\u{3b1}\u{3c0}\u{3b7}");
        assert_eq!(text(r"{\rtf1\ansi{\fonttbl{\f0\fcharset162 Arial;}}\f0 \'f0\'fd\par}"), "\u{11f}\u{131}");
        assert_eq!(text(r"{\rtf1\ansi{\fonttbl{\f0\fcharset177 Arial;}}\f0 \'f9\'ec\'e5\'ed\par}"), "\u{5e9}\u{5dc}\u{5d5}\u{5dd}");
        // Shift-JIS: two-byte characters, one with an ASCII trail byte written as a plain letter.
        assert_eq!(text(r"{\rtf1\ansi{\fonttbl{\f0\fcharset128 MS Gothic;}}\f0 \'82\'a0\'82\'a2\'83A\par}"), "\u{3042}\u{3044}\u{30a2}");
    }

    #[test]
    fn uc_skip_counts_and_their_restore_at_group_end() {
        assert_eq!(text(r"{\rtf1\ansi\uc0\u20320 x\par}"), "\u{4f60}x");
        assert_eq!(text(r"{\rtf1\ansi{\uc2\u20320\'a0\'a0}\u20320?y\par}"), "\u{4f60}\u{4f60}y");
    }

    #[test]
    fn malformed_rtf_does_not_panic() {
        for rtf in [r"{\rtf1\ansi {\b bold\par", r"{\rtf1\ansi x}}}} y\par", r"{\rtf1\ansi z\'4", r"{\rtf1\ansi\u", r"{\rtf1\ansi\u-99999999999 q\par}", r"{\rtf1\ansi\'zz w\par}"] {
            let _ = rtf_text(rtf);
        }
        assert_eq!(text(r"{\rtf1\ansi {\b bold\par"), "bold");
    }
}
