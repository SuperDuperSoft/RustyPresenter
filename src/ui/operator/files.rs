//! Which import a dropped or picked file takes, and the pure steps of "Make slides from pictures"
//! (`web/operator.js:535-598`). The steps that need the window arrive with Task 14's `add_files` and `images_to_deck`.
use super::strings as tr;
use crate::{app::ImportPurpose, media};
use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
};

const DECK: &[&str] = &["ppt", "pptx", "pps", "ppsx", "odp"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route { Song, Deck, Media }

impl Route {
    pub fn purpose(self) -> ImportPurpose {
        match self {
            Route::Song => ImportPurpose::Lyrics,
            Route::Deck => ImportPurpose::PowerPoint,
            Route::Media => ImportPurpose::Media,
        }
    }
}

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn route(path: &Path) -> Route {
    let e = ext(path);
    if e == "txt" { Route::Song } else if DECK.contains(&e.as_str()) { Route::Deck } else { Route::Media }
}

/// True when there is at least one file and every file is a picture, by extension.
pub fn all_images(paths: &[PathBuf]) -> bool {
    !paths.is_empty() && paths.iter().all(|p| media::IMAGE.contains(&ext(p).as_str()))
}

/// Sorts by file name the way `localeCompare(.., {numeric: true})` does for ordinary names, so `2.png` comes
/// before `10.png`. Names that compare equal (`7.png`, `007.png`) keep their order.
pub fn sort_by_name(paths: &mut [PathBuf]) {
    let name = |p: &PathBuf| p.file_name().unwrap_or(p.as_os_str()).to_string_lossy().into_owned();
    paths.sort_by(|a, b| natural_cmp(&name(a), &name(b)));
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        let (x, y) = match (a.peek(), b.peek()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) => (*x, *y),
        };
        let order = if x.is_ascii_digit() && y.is_ascii_digit() {
            // By length first, so no run can overflow an integer.
            let (m, n) = (digits(&mut a), digits(&mut b));
            m.len().cmp(&n.len()).then_with(|| m.cmp(&n))
        } else {
            a.next();
            b.next();
            rank(x).cmp(&rank(y))
        };
        if order != Ordering::Equal { return order; }
    }
}

/// A run of digits without its leading zeros.
fn digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut run = String::new();
    while let Some(c) = chars.next_if(char::is_ascii_digit) { run.push(c); }
    run.trim_start_matches('0').to_string()
}

/// Punctuation, then digits, then letters, ignoring case.
fn rank(c: char) -> (u8, char) {
    let class = if c.is_ascii_digit() { 1 } else if c.is_alphanumeric() { 2 } else { 0 };
    (class, c.to_lowercase().next().unwrap_or(c))
}

/// The name the "Name these slides" prompt starts with: the file name without its last extension.
pub fn suggested_title(path: &Path) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
    match name.rsplit_once('.') {
        Some((stem, e)) if !e.is_empty() => stem.to_string(),
        _ => name.into_owned(),
    }
}

/// The deck title from the prompt's answer. Cancel and a blank answer still make a deck.
pub fn deck_title(answer: Option<&str>) -> String {
    match answer {
        Some(a) if !a.trim().is_empty() => a.to_string(),
        _ => tr::DEFAULT_DECK_TITLE.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> { names.iter().map(PathBuf::from).collect() }

    fn sorted(names: &[&str]) -> Vec<PathBuf> {
        let mut v = paths(names);
        sort_by_name(&mut v);
        v
    }

    #[test]
    fn route_by_extension() {
        assert_eq!(route(Path::new("C:/songs/Amazing Grace.txt")), Route::Song);
        for f in ["a.ppt", "a.pptx", "a.pps", "a.ppsx", "a.odp"] { assert_eq!(route(Path::new(f)), Route::Deck, "{f}"); }
        for f in ["a.mp4", "a.png", "a.mp3", "a.pdf", "no-extension"] { assert_eq!(route(Path::new(f)), Route::Media, "{f}"); }
    }

    #[test]
    fn route_ignores_extension_case() {
        assert_eq!(route(Path::new("SONG.TXT")), Route::Song);
        assert_eq!(route(Path::new("Deck.PPTX")), Route::Deck);
    }

    #[test]
    fn each_route_imports_with_its_purpose() {
        assert_eq!(Route::Song.purpose(), ImportPurpose::Lyrics);
        assert_eq!(Route::Deck.purpose(), ImportPurpose::PowerPoint);
        assert_eq!(Route::Media.purpose(), ImportPurpose::Media);
    }

    #[test]
    fn all_images_needs_every_file() {
        assert!(all_images(&paths(&["a.png", "b.JPG", "c.webp"])));
        assert!(!all_images(&paths(&["a.png", "b.mp4"])));
        assert!(!all_images(&paths(&["no-extension"])));
    }

    #[test]
    fn all_images_is_false_with_no_files() { assert!(!all_images(&[])); }

    #[test]
    fn sort_puts_2_before_10() {
        assert_eq!(sorted(&["10.png", "2.png", "1.png"]), paths(&["1.png", "2.png", "10.png"]));
        assert_eq!(sorted(&["Slide10.png", "Slide9.png"]), paths(&["Slide9.png", "Slide10.png"]));
    }

    #[test]
    fn sort_uses_the_file_name_not_the_folder() {
        assert_eq!(sorted(&["a/2.png", "z/1.png"]), paths(&["z/1.png", "a/2.png"]));
    }

    #[test]
    fn sort_ignores_case() {
        assert_eq!(sorted(&["Banana.png", "apple.png", "cherry.png"]), paths(&["apple.png", "Banana.png", "cherry.png"]));
        assert_eq!(sorted(&["IMG_10.png", "img_2.png"]), paths(&["img_2.png", "IMG_10.png"]));
    }

    #[test]
    fn sort_puts_punctuation_before_digits_before_letters() {
        // `localeCompare` order: a shorter name wins only when it is a true prefix.
        assert_eq!(sorted(&["a1.png", "a_1.png", "ab.png"]), paths(&["a_1.png", "a1.png", "ab.png"]));
    }

    #[test]
    fn sort_handles_numbers_too_long_for_an_integer() {
        let (big, bigger) = ("99999999999999999999999998.png", "99999999999999999999999999.png");
        assert_eq!(sorted(&[bigger, big, "3.png"]), paths(&["3.png", big, bigger]));
    }

    #[test]
    fn sort_keeps_equal_numbers_in_their_given_order() {
        assert_eq!(sorted(&["007.png", "7.png"]), paths(&["007.png", "7.png"]));
        assert_eq!(sorted(&["7.png", "007.png"]), paths(&["7.png", "007.png"]));
    }

    #[test]
    fn suggested_title_drops_only_the_last_extension() {
        assert_eq!(suggested_title(Path::new("C:/pics/Easter Sunday.png")), "Easter Sunday");
        assert_eq!(suggested_title(Path::new("v1.2.final.jpg")), "v1.2.final");
        assert_eq!(suggested_title(Path::new("plain")), "plain");
    }

    #[test]
    fn deck_title_falls_back_when_cancelled_or_blank() {
        assert_eq!(deck_title(None), "Slides");
        assert_eq!(deck_title(Some("")), "Slides");
        assert_eq!(deck_title(Some("   ")), "Slides");
    }

    #[test]
    fn deck_title_keeps_what_was_typed() { assert_eq!(deck_title(Some("Easter Sunday")), "Easter Sunday"); }
}
