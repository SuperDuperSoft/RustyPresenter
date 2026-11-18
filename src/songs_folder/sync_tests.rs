use super::*;
use std::fs;

fn temp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("rp-test-songsfolder-{}", Uuid::new_v4()));
    fs::create_dir_all(&d).unwrap();
    d
}

fn song(title: &str, lyrics: &str) -> Song { Song { id: Uuid::new_v4(), title: title.into(), lyrics: lyrics.into(), ..Default::default() } }

/// A folder with its own state file, as the app opens it.
fn folder() -> Folder {
    let root = temp();
    Folder::open(root.join("Songs"), root.join("songs-folder.json"))
}

/// Two looks (a new file waits for the second), each brought into `songs`, then the library written.
fn settle(f: &mut Folder, songs: &mut Vec<Song>, tidy: bool) -> Vec<Note> {
    let mut notes = Vec::new();
    for _ in 0..2 {
        let (changes, n) = f.scan(songs, tidy);
        notes.extend(n);
        notes.extend(apply(songs, changes));
    }
    f.write(songs);
    notes
}

fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).map(|d| d.flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    v.sort();
    v
}

fn read(f: &Folder, name: &str) -> String { fs::read_to_string(f.dir.join(name)).unwrap() }

/// Writes a file as another program would, with a time a little in the past so it counts as finished.
fn drop_in(f: &Folder, name: &str, text: &str) {
    let p = f.dir.join(name);
    fs::write(&p, text).unwrap();
    let older = SystemTime::now() - Duration::from_secs(10);
    fs::File::options().write(true).open(&p).unwrap().set_modified(older).unwrap();
}

/// An edit in Notepad: new text, a new time.
fn edit(f: &Folder, name: &str, change: impl FnOnce(String) -> String) {
    let p = f.dir.join(name);
    let text = change(fs::read_to_string(&p).unwrap());
    fs::write(&p, text).unwrap();
    let later = SystemTime::now() - Duration::from_secs(5);
    fs::File::options().write(true).open(&p).unwrap().set_modified(later).unwrap();
}

#[test]
fn every_song_gets_a_file_named_after_it() {
    let mut f = folder();
    let mut songs = vec![song("Amazing Grace", "Verse 1\nAmazing grace"), song("Amazing Grace", "Verse 1\nOther words"), song("Who? What: <Why>", "x"), song("", "y")];
    assert!(settle(&mut f, &mut songs, false).is_empty());
    assert_eq!(files(&f.dir), ["Amazing Grace (2).chordpro", "Amazing Grace.chordpro", "Song.chordpro", "Who What Why.chordpro"]);
    assert_eq!(f.count(), 4);
    // Each file reads back as its song.
    let back = format::read(&read(&f, "Amazing Grace (2).chordpro")).unwrap();
    assert_eq!(back, songs[1]);
    // The app's own writes are not taken for changes, and nothing is written twice.
    let stamp = fs::metadata(f.dir.join("Amazing Grace.chordpro")).unwrap().modified().unwrap();
    for _ in 0..3 {
        let (changes, notes) = f.scan(&songs, false);
        assert!(changes.is_empty() && notes.is_empty());
        f.write(&songs);
    }
    assert_eq!(fs::metadata(f.dir.join("Amazing Grace.chordpro")).unwrap().modified().unwrap(), stamp);
    // No half-written file is left behind.
    assert!(fs::read_dir(&f.dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")));
}

#[test]
fn changes_in_the_app_follow_into_the_files() {
    let mut f = folder();
    let mut songs = vec![song("Grace", "Verse 1\nold"), song("Be Still", "Verse 1\nbe still")];
    settle(&mut f, &mut songs, false);
    songs[0].lyrics = "Verse 1\nnew".into();
    songs[0].transpose = 2;
    f.write(&songs);
    let text = read(&f, "Grace.chordpro");
    assert!(text.contains("\nnew\n") && text.contains("{transpose: 2}"), "{text}");
    // Renamed: the file follows the title; the old name is gone.
    songs[0].title = "Grace Again".into();
    f.write(&songs);
    assert_eq!(files(&f.dir), ["Be Still.chordpro", "Grace Again.chordpro"]);
    // Removed: its file goes into "Removed", nothing is deleted.
    songs.remove(1);
    f.write(&songs);
    assert_eq!(files(&f.dir), ["Grace Again.chordpro"]);
    assert_eq!(files(&f.dir.join(REMOVED)), ["Be Still.chordpro"]);
    // Removed again with the same name: kept beside the first.
    let mut again = vec![songs[0].clone(), song("Be Still", "Verse 1\nsecond")];
    settle(&mut f, &mut again, false);
    again.remove(1);
    f.write(&again);
    assert_eq!(files(&f.dir.join(REMOVED)), ["Be Still (2).chordpro", "Be Still.chordpro"]);
    // A look later finds nothing to do.
    let (changes, notes) = f.scan(&again, false);
    assert!(changes.is_empty() && notes.is_empty());
}

#[test]
fn a_file_edited_in_notepad_changes_the_song() {
    let mut f = folder();
    let mut songs = vec![song("Grace", "Verse 1\nold words")];
    let id = songs[0].id;
    settle(&mut f, &mut songs, false);
    edit(&f, "Grace.chordpro", |t| t.replace("old words", "new words").replace("{title: Grace}", "{title: Grace (Live)}"));
    let notes = settle(&mut f, &mut songs, false);
    assert_eq!(notes, [Note::Updated("Grace (Live)".into())]);
    assert_eq!((songs[0].id, songs[0].lyrics.as_str()), (id, "Verse 1\nnew words"), "the same song, so services still find it");
    // The new title gives the file a new name.
    assert_eq!(files(&f.dir), ["Grace (Live).chordpro"]);
    // Taken once.
    assert!(settle(&mut f, &mut songs, false).is_empty());
    // Edited and saved with Windows line endings: still read.
    edit(&f, "Grace (Live).chordpro", |t| t.replace("new words", "newer").replace('\n', "\r\n"));
    settle(&mut f, &mut songs, false);
    assert_eq!(songs[0].lyrics, "Verse 1\nnewer");
}

#[test]
fn changed_here_and_there_keeps_both() {
    let mut f = folder();
    let mut songs = vec![song("Grace", "Verse 1\nold")];
    settle(&mut f, &mut songs, false);
    songs[0].lyrics = "Verse 1\nthe app's words".into();
    edit(&f, "Grace.chordpro", |t| t.replace("\nold\n", "\nthe folder's words\n"));
    let notes = settle(&mut f, &mut songs, false);
    assert_eq!(notes, [Note::KeptBoth("Grace".into(), "Grace (from the folder)".into())]);
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].lyrics, "Verse 1\nthe app's words", "the app's song is as it was made");
    assert_eq!((songs[1].title.as_str(), songs[1].lyrics.as_str()), ("Grace (from the folder)", "Verse 1\nthe folder's words"));
    // Each has its file: the app's words back in Grace's, the folder's in a new one.
    assert!(read(&f, "Grace.chordpro").contains("the app's words"));
    assert!(read(&f, "Grace (from the folder).chordpro").contains("the folder's words"));
    assert!(settle(&mut f, &mut songs, false).is_empty(), "and then it is quiet");
    // `apply` checks again with the library locked: a song changed after the look is kept too.
    let mut lib = vec![song("X", "a")];
    let seen = song_hash(&lib[0]);
    lib[0].lyrics = "changed meanwhile".into();
    let from_file = Song { lyrics: "from the file".into(), ..lib[0].clone() };
    let notes = apply(&mut lib, vec![Change::Update { song: from_file, expect: seen }]);
    assert!(matches!(&notes[0], Note::KeptBoth(..)) && lib.len() == 2 && lib[0].lyrics == "changed meanwhile");
}

#[test]
fn files_dropped_in_come_into_the_library() {
    let mut f = folder();
    let mut songs = vec![song("Already Here", "Verse 1\nWe are walking")];
    settle(&mut f, &mut songs, false);
    drop_in(&f, "Hills.cho", crate::songselect::inbox::tests::CHORDPRO);
    drop_in(&f, "River.txt", crate::songselect::inbox::tests::LYRICS_TXT);
    drop_in(&f, "Morning.usr", crate::songselect::usr::tests::SAMPLE);
    drop_in(&f, "Own Words.txt", "Verse 1\nOur own words\n\nChorus\nSing it");
    drop_in(&f, "Open.xml", r#"<?xml version="1.0"?><song xmlns="http://openlyrics.info/namespace/2009/song" version="0.8"><properties><titles><title>Open Song</title></titles></properties><lyrics><verse name="v1"><lines>Open words</lines></verse></lyrics></song>"#);
    drop_in(&f, "OpenSong", "<song><title>Extensionless</title><lyrics>[V1]\n Some words</lyrics></song>");
    drop_in(&f, "notes.txt", "Shopping list\nmilk\neggs");
    drop_in(&f, "picture.jpg", "not looked at");
    let notes = settle(&mut f, &mut songs, false);
    let mut titles: Vec<&str> = songs.iter().map(|s| s.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["Already Here", "Extensionless", "Hills Of Test", "Morning Light Test Song", "Open Song", "Own Words", "River Of Test Songs"]);
    assert_eq!(notes.iter().filter(|n| matches!(n, Note::CameIn(_))).count(), 6);
    let hills = songs.iter().find(|s| s.title == "Hills Of Test").unwrap();
    assert!(hills.lyrics.contains("[D]Over the hills") && hills.ccli == "7000001", "chords and credits kept");
    // The file that is not a song: listed, untouched, not read again.
    assert!(notes.contains(&Note::Unreadable("notes.txt".into(), "This file has no Verse/Chorus headings. Use SongSelect's \"Lyrics (.txt)\" download.".into())), "{notes:?}");
    assert_eq!(f.unreadable.keys().collect::<Vec<_>>(), ["notes.txt"]);
    assert_eq!(read(&f, "notes.txt"), "Shopping list\nmilk\neggs");
    // The originals stay; each song has its ChordPro file beside them.
    let all = files(&f.dir);
    for want in ["Hills.cho", "River.txt", "Morning.usr", "Open.xml", "OpenSong", "picture.jpg", "Hills Of Test.chordpro", "River Of Test Songs.chordpro",
        "Own Words.chordpro", "Morning Light Test Song.chordpro", "Open Song.chordpro", "Extensionless.chordpro", "Already Here.chordpro"] {
        assert!(all.contains(&want.to_string()), "{want} in {all:?}");
    }
    assert!(settle(&mut f, &mut songs, false).is_empty(), "each comes in once");
    // The original edited later: the song follows (its own details stay).
    let river = songs.iter().position(|s| s.title == "River Of Test Songs").unwrap();
    songs[river].lines_per_slide = 3;
    f.write(&songs);
    edit(&f, "River.txt", |t| t.replace("Singing all the way", "Singing all the day"));
    settle(&mut f, &mut songs, false);
    assert!(songs[river].lyrics.contains("Singing all the day") && songs[river].lines_per_slide == 3, "{:?}", songs[river]);
    // The same song dropped in again with the same words: no second copy.
    drop_in(&f, "River again.txt", crate::songselect::inbox::tests::LYRICS_TXT.replace("Singing all the way", "Singing all the day").as_str());
    let before = songs.len();
    settle(&mut f, &mut songs, false);
    assert_eq!(songs.len(), before);
    // Removing the song in the app moves its files, the original too, into "Removed": it does not come back.
    songs.remove(river);
    settle(&mut f, &mut songs, false);
    let removed = files(&f.dir.join(REMOVED));
    assert!(removed.contains(&"River.txt".to_string()) && removed.contains(&"River Of Test Songs.chordpro".to_string()), "{removed:?}");
    assert!(settle(&mut f, &mut songs, false).is_empty());
    assert!(!songs.iter().any(|s| s.title == "River Of Test Songs"));
}

#[test]
fn tidy_moves_imported_files_aside() {
    let mut f = folder();
    let mut songs = vec![];
    settle(&mut f, &mut songs, true);
    drop_in(&f, "River.txt", crate::songselect::inbox::tests::LYRICS_TXT);
    settle(&mut f, &mut songs, true);
    assert_eq!(songs.len(), 1);
    assert_eq!(files(&f.dir), ["River Of Test Songs.chordpro"], "only the ChordPro file stays");
    assert_eq!(files(&f.dir.join(IMPORTED)), ["River.txt"], "the original is kept aside, not deleted");
    assert!(settle(&mut f, &mut songs, true).is_empty());
}

#[test]
fn a_file_still_arriving_waits() {
    let mut f = folder();
    let mut songs = vec![];
    settle(&mut f, &mut songs, false);
    // Just written (a copy or a download in progress): not read on the first look.
    fs::write(f.dir.join("New.chordpro"), "{title: New}\n[G]la").unwrap();
    let (changes, _) = f.scan(&songs, false);
    assert!(changes.is_empty());
    // Still growing at the next look: waits again.
    fs::write(f.dir.join("New.chordpro"), "{title: New}\n[G]la la la").unwrap();
    let t = SystemTime::now() + Duration::from_secs(1);
    fs::File::options().write(true).open(f.dir.join("New.chordpro")).unwrap().set_modified(t).unwrap();
    let (changes, _) = f.scan(&songs, false);
    assert!(changes.is_empty());
    // Unchanged since: taken.
    let (changes, _) = f.scan(&songs, false);
    assert_eq!(changes.len(), 1);
    // The app's half-written files and editors' lock files are never songs.
    drop_in(&f, ".rp-123.tmp", "{title: half}");
    drop_in(&f, "~$Lock.txt", "Verse 1\nx");
    let (changes, notes) = f.scan(&songs, false);
    assert!(changes.is_empty() && notes.is_empty());
}

#[test]
fn a_file_deleted_by_hand_is_written_again_and_a_rename_is_followed() {
    let mut f = folder();
    let mut songs = vec![song("Grace", "Verse 1\nx"), song("Hope", "Verse 1\ny")];
    settle(&mut f, &mut songs, false);
    fs::remove_file(f.dir.join("Grace.chordpro")).unwrap();
    settle(&mut f, &mut songs, false);
    settle(&mut f, &mut songs, false);
    assert!(f.dir.join("Grace.chordpro").is_file(), "songs are removed in the app, so the file comes back");
    assert_eq!(songs.len(), 2);
    // Renamed in Explorer: the same song, no copy.
    fs::rename(f.dir.join("Hope.chordpro"), f.dir.join("Hope renamed.chordpro")).unwrap();
    let older = SystemTime::now() - Duration::from_secs(10);
    fs::File::options().write(true).open(f.dir.join("Hope renamed.chordpro")).unwrap().set_modified(older).unwrap();
    let notes = settle(&mut f, &mut songs, false);
    settle(&mut f, &mut songs, false);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(songs.len(), 2);
    assert_eq!(f.state.songs[&songs[1].id].file, "Hope renamed.chordpro");
    // Copied in Explorer: a second song, under its own id.
    fs::copy(f.dir.join("Grace.chordpro"), f.dir.join("Grace - Copy.chordpro")).unwrap();
    fs::File::options().write(true).open(f.dir.join("Grace - Copy.chordpro")).unwrap().set_modified(older).unwrap();
    let notes = settle(&mut f, &mut songs, false);
    assert_eq!(notes, [Note::CameIn("Grace".into())]);
    assert_eq!(songs.len(), 3);
    assert_ne!(songs[2].id, songs[0].id);
    settle(&mut f, &mut songs, false);
    let ids: Vec<Uuid> = fs::read_dir(&f.dir).unwrap().flatten().filter_map(|e| format::read(&fs::read_to_string(e.path()).ok()?).ok()).map(|s| s.id).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&songs[2].id), "the copy was written again with its own id");
}

#[test]
fn a_file_put_back_from_removed_brings_its_song_back() {
    let mut f = folder();
    let mut songs = vec![song("Grace", "Verse 1\nx")];
    let id = songs[0].id;
    settle(&mut f, &mut songs, false);
    songs.clear();
    f.write(&songs);
    fs::rename(f.dir.join(REMOVED).join("Grace.chordpro"), f.dir.join("Grace.chordpro")).unwrap();
    let older = SystemTime::now() - Duration::from_secs(10);
    fs::File::options().write(true).open(f.dir.join("Grace.chordpro")).unwrap().set_modified(older).unwrap();
    let notes = settle(&mut f, &mut songs, false);
    assert_eq!(notes, [Note::BroughtBack("Grace".into())]);
    assert_eq!(songs[0].id, id, "with its id, so an old service finds it");
}

#[test]
fn a_restart_or_another_computer_knows_the_files() {
    let root = temp();
    let dir = root.join("Songs");
    let state = root.join("songs-folder.json");
    let mut songs = vec![song("Grace", "Verse 1\nx")];
    let mut f = Folder::open(dir.clone(), state.clone());
    settle(&mut f, &mut songs, false);
    f.save_state();
    // Restarted: nothing to do.
    let mut f = Folder::open(dir.clone(), state.clone());
    assert!(settle(&mut f, &mut songs, false).is_empty());
    assert_eq!(files(&dir), ["Grace.chordpro"]);
    // The same folder used with no state (another computer with the same library, the state file lost): the files are
    // the songs' own, nothing is doubled.
    let mut f = Folder::open(dir.clone(), root.join("other.json"));
    assert!(settle(&mut f, &mut songs, false).is_empty());
    assert_eq!(files(&dir), ["Grace.chordpro"]);
    // …and when the library has other words, both are kept.
    songs[0].lyrics = "Verse 1\nchanged on this computer".into();
    let mut f = Folder::open(dir.clone(), root.join("third.json"));
    let notes = settle(&mut f, &mut songs, false);
    assert_eq!(notes, [Note::KeptBoth("Grace".into(), "Grace (from the folder)".into())]);
    // A state for another folder is not used.
    let f = Folder::open(root.join("Elsewhere"), state);
    assert!(f.state.songs.is_empty());
}

#[test]
fn a_crash_while_writing_leaves_the_old_file() {
    let mut f = folder();
    let mut songs = vec![song("Grace", "Verse 1\nx")];
    settle(&mut f, &mut songs, false);
    let before = read(&f, "Grace.chordpro");
    // What a crash halfway through a write leaves: the new text in a temp file that was never renamed.
    fs::write(f.dir.join(".rp-00000000.tmp"), "{title: Gr").unwrap();
    assert_eq!(read(&f, "Grace.chordpro"), before);
    let (changes, notes) = f.scan(&songs, false);
    assert!(changes.is_empty() && notes.is_empty(), "the leftover is not a song");
}

#[cfg(windows)]
#[test]
fn a_busy_file_is_tried_again_and_nothing_is_lost() {
    use std::os::windows::fs::OpenOptionsExt;
    let mut f = folder();
    let mut songs = vec![song("Grace", "Verse 1\nold")];
    settle(&mut f, &mut songs, false);
    // Another program (OneDrive, an editor) holds the file without sharing it.
    let held = fs::OpenOptions::new().read(true).share_mode(0).open(f.dir.join("Grace.chordpro")).unwrap();
    songs[0].lyrics = "Verse 1\nnew".into();
    f.write(&songs);
    assert_eq!(f.retrying.len(), 1, "busy: tried again later");
    assert_eq!(f.state.songs[&songs[0].id].hash, song_hash(&Song { lyrics: "Verse 1\nold".into(), ..songs[0].clone() }), "still known as the old song");
    drop(held);
    f.write(&songs);
    assert!(f.retrying.is_empty());
    assert!(read(&f, "Grace.chordpro").contains("\nnew\n"));
}

#[test]
fn a_big_file_is_not_read() {
    let mut f = folder();
    let mut songs = vec![];
    settle(&mut f, &mut songs, false);
    drop_in(&f, "huge.txt", &"a".repeat(MAX_BYTES as usize + 1));
    settle(&mut f, &mut songs, false);
    assert!(songs.is_empty());
    assert_eq!(f.unreadable.get("huge.txt").map(|(_, w)| w.as_str()), Some("it is too big to be a song file"));
}
