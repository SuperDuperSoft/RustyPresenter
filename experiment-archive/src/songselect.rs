//! Conversion of a user-supplied SongSelect lyric TXT export. This is not API access.
use crate::model::{ServiceItem, Slide};
use uuid::Uuid;

fn section(line: &str) -> bool {
    let lower = line.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    let head = lower.split_whitespace().next().unwrap_or("");
    ["verse", "chorus", "bridge", "intro", "outro", "ending", "tag", "interlude", "refrain", "pre-chorus", "prechorus", "instrumental"].contains(&head)
        || lower.starts_with("pre chorus")
}

pub fn paginate(text: &str, max_lines: usize, max_chars: usize) -> Vec<String> {
    let mut pages = Vec::new();
    let mut page = String::new();
    let mut lines = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        // Split exceptionally long source lines at word boundaries as well.
        let mut wrapped = String::new();
        for word in line.split_whitespace() {
            if !wrapped.is_empty() && wrapped.chars().count() + word.chars().count() + 1 > max_chars {
                push_line(&mut pages, &mut page, &mut lines, &wrapped, max_lines, max_chars);
                wrapped.clear();
            }
            if !wrapped.is_empty() { wrapped.push(' '); }
            wrapped.push_str(word);
        }
        if !wrapped.is_empty() {
            push_line(&mut pages, &mut page, &mut lines, &wrapped, max_lines, max_chars);
        }
    }
    if !page.is_empty() { pages.push(page); }
    pages
}

fn push_line(pages: &mut Vec<String>, page: &mut String, lines: &mut usize, line: &str, max_lines: usize, max_chars: usize) {
    if !page.is_empty() && (*lines >= max_lines || page.chars().count() + line.chars().count() + 1 > max_chars) {
        pages.push(std::mem::take(page));
        *lines = 0;
    }
    if !page.is_empty() { page.push('\n'); }
    page.push_str(line);
    *lines += 1;
}

pub fn import_txt(text: &str, lines_per_slide: usize) -> Result<ServiceItem, String> {
    if !(1..=8).contains(&lines_per_slide) || text.len() > 256_000 {
        return Err("Choose 1–8 lines per slide and a lyric file smaller than 256 KB.".into());
    }
    let normalized = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<_> = normalized.lines().map(str::trim_end).collect();
    let title_index = lines.iter().position(|l| !l.trim().is_empty()).ok_or("The lyric file is empty.")?;
    let title = lines[title_index].trim().trim_matches('"').to_owned();
    if title.len() > 500 { return Err("The song title is too long.".into()); }
    let first_section = lines.iter().enumerate().skip(title_index + 1).find(|(_, l)| section(l)).map(|(i, _)| i)
        .ok_or("This TXT file has no recognized lyric sections. Use SongSelect's lyric TXT export with Verse / Chorus headings.")?;
    let footer = lines.iter().enumerate().skip(first_section + 1).find(|(_, l)| {
        let lower = l.trim().to_ascii_lowercase();
        lower.starts_with("ccli song") || lower.starts_with("copyright") || lower.starts_with('©')
    }).map(|(i, _)| i).unwrap_or(lines.len());
    let mut credits: Vec<String> = lines[title_index + 1..first_section].iter().filter(|l| !l.trim().is_empty()).map(|l| l.trim().to_string()).collect();
    credits.extend(lines[footer..].iter().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.to_ascii_lowercase().starts_with("ccli license") && !l.to_ascii_lowercase().contains("for use solely") && !l.contains("www.ccli.com")).map(str::to_string));
    let mut slides = Vec::new();
    let mut label = String::new();
    let mut lyrics = String::new();
    let flush = |slides: &mut Vec<Slide>, label: &str, lyrics: &mut String| {
        let pages = paginate(lyrics, lines_per_slide, 280);
        let count = pages.len();
        for (index, page) in pages.into_iter().enumerate() {
            let label = if count > 1 { format!("{label} · {}/{}", index + 1, count) } else { label.to_owned() };
            slides.push(Slide::new(label, page));
        }
        lyrics.clear();
    };
    for line in &lines[first_section..footer] {
        if section(line) {
            flush(&mut slides, &label, &mut lyrics);
            label = line.trim().trim_matches(['[', ']']).to_string();
        } else if !line.trim().is_empty() {
            lyrics.push_str(line.trim());
            lyrics.push('\n');
        }
    }
    flush(&mut slides, &label, &mut lyrics);
    if slides.is_empty() { return Err("No lyrics were found in this file.".into()); }
    Ok(ServiceItem { id: Uuid::new_v4(), title, kind: "song".into(), credits: credits.join("\n"), slides })
}

#[cfg(test)]
mod tests {
    use super::*;
    const SONG: &str = "A Test Song\r\nTest Writer\r\n\r\nVerse 1\r\nOriginal test line one\r\nOriginal test line two\r\nOriginal test line three\r\n\r\nChorus\r\nA repeated test line\r\n\r\nCCLI Song # 12345\r\n© 2026 Test Writer\r\nFor use solely with the SongSelect Terms of Use.\r\nCCLI License # 98765";

    #[test]
    fn imports_sections_and_credits_without_projecting_footer() {
        let item = import_txt(SONG, 2).unwrap();
        assert_eq!(item.title, "A Test Song");
        assert_eq!(item.slides.len(), 3);
        assert_eq!(item.slides[0].label, "Verse 1 · 1/2");
        assert!(item.credits.contains("12345"));
        assert!(item.credits.contains("Test Writer"));
        assert!(!item.credits.contains("98765"));
        assert!(item.slides.iter().all(|s| !s.text.contains("CCLI") && !s.text.contains('©')));
    }

    #[test]
    fn pagination_preserves_every_word() {
        let text = "a b c d e f g h i j k l m n o p q r s t u v w x y z";
        let pages = paginate(text, 2, 12);
        assert!(pages.len() > 1);
        assert_eq!(pages.join(" ").split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>());
    }

    #[test]
    fn does_not_guess_lyrics_from_unrecognized_files() {
        assert!(import_txt("Song Title\nA writer\nCopyright text", 4).is_err());
        assert!(import_txt(SONG, 0).is_err());
    }
}
