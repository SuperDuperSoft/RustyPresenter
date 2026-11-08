//! What the move to one native app left behind must stay gone (Plan 4 Tasks 12 and 13).
use std::{fs, path::{Path, PathBuf}};

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")) }

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() { files_under(&path, out) } else { out.push(path) }
    }
}

/// The places that matter, as UTF-8 text (a binary file has nothing to say here).
fn text_of(places: &[&str]) -> Vec<(PathBuf, String)> {
    let mut files = vec![];
    for place in places {
        let path = root().join(place);
        if path.is_dir() { files_under(&path, &mut files) } else { files.push(path) }
    }
    files.into_iter().filter_map(|f| fs::read_to_string(&f).ok().map(|text| (f, text))).collect()
}

#[test]
fn no_tauri_or_webview_left() {
    let words = ["tauri", "webview"];
    let found: Vec<String> = text_of(&["src", "build.rs", "Cargo.toml"]).iter()
        .flat_map(|(file, text)| {
            let lower = text.to_lowercase();
            words.iter().filter(move |w| lower.contains(**w)).map(move |w| format!("{w} in {}", file.display()))
        })
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn old_app_files_are_gone() {
    for gone in ["src/displays.rs", "tauri.conf.json", "gen", "src/bin/native.rs", "RustyPresenter-Native.cmd"] {
        assert!(!root().join(gone).exists(), "{gone} is still here");
    }
}

#[test]
fn no_ffmpeg_left() {
    // The word itself stays: About reads mpv's `ffmpeg-version` and shows the LGPL notice.
    let uses = ["find_tool", "Command::new(\"ffmpeg\"", "Command::new(\"ffprobe\"", "media::ffmpeg", "plan_video", "plan_audio", "spawn_conversion", "Update::Convert", "Gyan.FFmpeg"];
    let found: Vec<String> = text_of(&["src", "ui"]).iter()
        .flat_map(|(file, text)| uses.iter().filter(move |u| text.contains(**u)).map(move |u| format!("{u} in {}", file.display())))
        .collect();
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn web_folder_has_only_remote() {
    let mut names: Vec<String> = fs::read_dir(root().join("web")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    // The phone remote and its drawing view, and the stage screen for phones and tablets (N31).
    assert_eq!(names, ["draw.js", "remote.html", "remote.js", "stage-view.html", "stage-view.js"]);
}
