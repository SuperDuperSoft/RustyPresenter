//! Songs from SongSelect, driven inline: files appear in a stand-in Downloads folder and the window's watcher looks.
use super::*;
use crate::{
    app::test_app,
    model::{ItemKind, Song},
    songselect::{inbox::tests::{CHORDPRO, LYRICS_TXT}, usr::tests::SAMPLE as USR, watch::testing::fake_downloads},
    ui::{AddState, SettingsDlg, dialogs::{AddTab, SettingsTab, add, settings}, operator::window::testing::{find, open as open_window, refresh}},
};
use slint::Model;
use std::fs;

fn opened(app: &Arc<App>) -> Rc<Operator> {
    let op = open_window(app);
    op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
    slint::platform::update_timers_and_animations();
    refresh(&op);
    op
}

/// Two looks: a new file is taken once it stayed the same for two.
fn look_twice(op: &Rc<Operator>) { poll(op); poll(op); }

fn rows(op: &Operator) -> Vec<SongSelectRow> { op.ui.global::<SongSelectDlg>().get_rows().iter().collect() }

fn songs(app: &App) -> Vec<Song> { app.data.lock().unwrap().songs.clone() }

fn toasts(op: &Operator) -> Vec<(String, bool)> { op.toast_log() }

#[test]
fn downloads_come_in_while_the_window_is_open() {
    let dir = fake_downloads();
    fs::write(dir.join("Downloaded Earlier.txt"), LYRICS_TXT.replace("1234567", "999")).unwrap();
    let app = test_app();
    let op = opened(&app);
    open(&op);
    assert_eq!(op.dialog(), Some(DialogKind::SongSelect));
    assert_eq!(op.ui.get_dialog_title(), "Songs from SongSelect");
    let g = op.ui.global::<SongSelectDlg>();
    assert!(g.get_watching().contains(&dir.display().to_string()) && !g.get_watching_problem(), "{}", g.get_watching());
    assert!(find(&op, "SongSelectDialog::nothing-yet").is_some());
    // Downloads from SongSelect, and other files the person downloads at the same time.
    fs::write(dir.join("River Of Test Songs.txt"), LYRICS_TXT).unwrap();
    fs::write(dir.join("hills-of-test.chordpro"), CHORDPRO).unwrap();
    fs::write(dir.join("song.usr"), USR).unwrap();
    fs::write(dir.join("shopping list.txt"), "milk\neggs").unwrap();
    fs::write(dir.join("broken.txt"), "Title only\nCCLI Song # 5\n").unwrap();
    fs::write(dir.join("half.txt.crdownload"), "being downloaded").unwrap();
    poll(&op);
    assert!(rows(&op).is_empty(), "a new file waits for a second look");
    poll(&op);
    let got: Vec<(String, String, String, bool)> = rows(&op).iter().map(|r| (r.title.to_string(), r.ccli.to_string(), r.note.to_string(), r.can_add)).collect();
    assert_eq!(got, [
        ("River Of Test Songs".into(), "CCLI #1234567".into(), "Added to your song library".into(), true),
        ("broken.txt".into(), "".into(), "Could not read it: This file has no Verse/Chorus headings. Use SongSelect's \"Lyrics (.txt)\" download.".into(), false),
        ("Hills Of Test".into(), "CCLI #7000001".into(), "Added to your song library".into(), true),
        ("Morning Light Test Song".into(), "CCLI #7654321".into(), "Added to your song library".into(), true),
    ], "SongSelect's songs only, the one that could not be read said so; the one downloaded earlier is left");
    assert!(rows(&op)[1].problem);
    let lib = songs(&app);
    assert_eq!(lib.len(), 3);
    let river = lib.iter().find(|s| s.ccli == "1234567").unwrap();
    assert_eq!((river.writers.as_str(), river.copyright.as_str()), ("Sample Writer | Another Writer", "© 2026 Example Worship Music (Admin. by Example Publishing)"), "credits kept for the credit line");
    look_twice(&op);
    assert_eq!(rows(&op).len(), 4, "each file once");
    // Add to the service: the window stays open for more.
    let river_row = rows(&op).into_iter().find(|r| r.title == "River Of Test Songs").unwrap();
    g.invoke_add(river_row.id.clone());
    let items: Vec<ItemKind> = app.data.lock().unwrap().services[0].items.iter().map(|i| i.kind.clone()).collect();
    assert!(matches!(&items[..], [ItemKind::Song { song_id, .. }] if *song_id == river.id), "{items:?}");
    assert_eq!(op.dialog(), Some(DialogKind::SongSelect));
    assert!(rows(&op).iter().find(|r| r.id == river_row.id).unwrap().in_service);
    assert!(toasts(&op).contains(&("Added to the service".into(), false)));
    // Nothing in Downloads was moved or removed.
    for f in ["Downloaded Earlier.txt", "River Of Test Songs.txt", "hills-of-test.chordpro", "song.usr", "shopping list.txt", "broken.txt", "half.txt.crdownload"] {
        assert!(dir.join(f).is_file(), "{f} is still there");
    }
    // Closed: watching stops (the setting is off), and a download now waits for next time.
    g.invoke_close();
    assert_eq!(op.dialog(), None);
    assert!(op.songselect.watch.borrow().is_none());
    fs::write(dir.join("later.usr"), USR.replace("7654321", "1")).unwrap();
    look_twice(&op);
    assert_eq!(songs(&app).len(), 3);
}

#[test]
fn the_churchs_own_copy_is_never_changed() {
    let dir = fake_downloads();
    let app = test_app();
    let mine = Song { id: Uuid::new_v4(), title: "River (our words)".into(), ccli: "1234567".into(), lyrics: "Verse 1\nOur own changed words".into(), ..Default::default() };
    let same = Song { id: Uuid::new_v4(), title: "Hills".into(), ccli: "7000001".into(), lyrics: "Verse 1\nOver the hills the morning comes\nLight on the fields\n\nChorus\nSing, sing, sing\nAll the hills will ring".into(), ..Default::default() };
    app.data.lock().unwrap().songs.extend([mine.clone(), same.clone()]);
    let op = opened(&app);
    open(&op);
    fs::write(dir.join("river.txt"), LYRICS_TXT).unwrap();
    fs::write(dir.join("hills.chordpro"), CHORDPRO).unwrap();
    look_twice(&op);
    let notes: Vec<(String, String)> = rows(&op).iter().map(|r| (r.title.to_string(), r.note.to_string())).collect();
    assert_eq!(notes, [
        ("Hills Of Test".into(), "Already in your song library".into()),
        ("River Of Test Songs".into(), "Your copy has other words, so this one is kept as “River Of Test Songs (from SongSelect)”".into()),
    ]);
    let lib = songs(&app);
    assert_eq!(lib.len(), 3);
    assert_eq!(lib[0], mine, "the church's edits are kept");
    assert_eq!(lib[1], same);
    assert!(lib.iter().any(|s| s.title == "River Of Test Songs (from SongSelect)" && s.ccli == "1234567"));
    // "Add to the service" on "already there" adds the church's own song.
    let hills = rows(&op)[0].id.clone();
    assert_eq!(hills, same.id.to_string());
}

#[test]
fn with_the_setting_on_downloads_come_in_while_the_window_is_closed() {
    let dir = fake_downloads();
    let app = test_app();
    let op = opened(&app);
    assert!(op.songselect.watch.borrow().is_none(), "off by default");
    // Settings → Songs: the choice is saved and watching starts.
    settings::open(&op, SettingsTab::Songs);
    let f = op.ui.global::<SettingsDlg>();
    assert!(!f.get_songselect_inbox());
    assert!(find(&op, "SettingsSongsGeneral::songselect-inbox").is_some());
    f.set_songselect_inbox(true);
    find(&op, "SettingsDialog::save").unwrap().mock_single_click(slint::platform::PointerEventButton::Left);
    assert!(app.data.lock().unwrap().settings.songselect_inbox);
    refresh(&op);
    assert!(op.songselect.watch.borrow().is_some(), "watching whenever the app is open");
    fs::write(dir.join("river.txt"), LYRICS_TXT).unwrap();
    fs::write(dir.join("broken.usr"), "[S A12]\nTitle=Only a title\n").unwrap();
    look_twice(&op);
    assert_eq!(songs(&app).len(), 1);
    let t = toasts(&op);
    assert!(t.contains(&("“River Of Test Songs” came in from SongSelect.".into(), false)), "{t:?}");
    assert!(t.contains(&("broken.usr could not be read: No lyrics were found in this file.".into(), true)), "{t:?}");
    // The window opened now lists only what comes in from now on, from the same watcher (nothing twice).
    open(&op);
    assert!(rows(&op).is_empty());
    look_twice(&op);
    assert!(rows(&op).is_empty() && songs(&app).len() == 1);
    fs::write(dir.join("again.txt"), LYRICS_TXT).unwrap();
    look_twice(&op);
    assert_eq!(rows(&op)[0].note, "Already in your song library");
    op.close_dialog();
    assert!(op.songselect.watch.borrow().is_some(), "still watching: the setting is on");
    // Turned off: watching stops.
    app.data.lock().unwrap().settings.songselect_inbox = false;
    refresh(&op);
    assert!(op.songselect.watch.borrow().is_none());
}

#[test]
fn no_downloads_folder_says_so_and_choose_a_file_still_works() {
    crate::songselect::watch::testing::DOWNLOADS.with(|d| *d.borrow_mut() = None);
    let app = test_app();
    let op = opened(&app);
    open(&op);
    let g = op.ui.global::<SongSelectDlg>();
    assert!(g.get_watching_problem(), "{}", g.get_watching());
    let dir = std::env::temp_dir().join(format!("rp-songselect-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let own = dir.join("Our Own.cho");
    fs::write(&own, "{title: Our Own}\n[G]la la").unwrap();
    let usr = dir.join("x.usr");
    fs::write(&usr, USR).unwrap();
    files::testing::will_pick(vec![own, usr]);
    g.invoke_choose_file();
    let got: Vec<(String, String)> = rows(&op).iter().map(|r| (r.title.to_string(), r.note.to_string())).collect();
    assert_eq!(got, [("Our Own".into(), "Added to your song library".into()), ("Morning Light Test Song".into(), "Added to your song library".into())],
        "a picked file needs no CCLI number");
}

#[test]
fn open_songselect_only_opens_the_browser() {
    let app = test_app();
    let op = opened(&app);
    let before = crate::songselect::testing::OPENED.with(|o| o.get());
    open(&op);
    op.ui.global::<SongSelectDlg>().invoke_open_songselect();
    assert_eq!(crate::songselect::testing::OPENED.with(|o| o.get()), before + 1);
    assert_eq!(crate::songselect::HOME, "https://songselect.ccli.com/");
}

#[test]
fn a_song_search_that_finds_nothing_offers_songselect() {
    let _dir = fake_downloads();
    let app = test_app();
    add::tests::song(&app, "Amazing Grace", "", "", "Verse 1\nAmazing grace");
    let op = opened(&app);
    add::open_add(&op, AddTab::Song);
    add::tests::settle(&op);
    assert!(find(&op, "AddSongTab::from-songselect").is_some());
    let st = op.ui.global::<AddState>();
    assert!(!st.get_offer_songselect(), "nothing searched yet");
    add::tests::type_text(&op, "grace");
    add::tests::settle(&op);
    assert!(!st.get_offer_songselect(), "found");
    add::tests::type_text(&op, "zzz");
    add::tests::settle(&op);
    assert!(st.get_offer_songselect());
    add::tests::click(&op, "AddList::look-songselect");
    assert_eq!(op.dialog(), Some(DialogKind::SongSelect));
    assert_eq!(testing::COPIED.with(|c| c.borrow().clone()).as_deref(), Some("gracezzz"));
    assert!(toasts(&op).iter().any(|(t, _)| t.contains("gracezzz") && t.contains("paste")), "{:?}", toasts(&op));
    // The Bible tab never offers it.
    add::open_add(&op, AddTab::Scripture);
    assert!(!st.get_offer_songselect());
    // "From SongSelect…" opens the window too, and the File menu has it.
    add::open_add(&op, AddTab::Song);
    add::tests::settle(&op);
    add::tests::click(&op, "AddSongTab::from-songselect");
    assert_eq!(op.dialog(), Some(DialogKind::SongSelect));
    op.close_dialog();
    op.ui.invoke_menu("songselect".into());
    assert_eq!(op.dialog(), Some(DialogKind::SongSelect));
    op.close_dialog();
    // Quick search (Ctrl+F) offers it too.
    crate::ui::dialogs::quick_search::open(&op);
    let q = op.ui.global::<crate::ui::QuickSearchDlg>();
    q.set_query("grace".into());
    q.invoke_edited("grace".into());
    assert!(!q.get_offer_songselect());
    q.set_query("Way Maker".into());
    q.invoke_edited("Way Maker".into());
    assert!(q.get_offer_songselect());
    q.invoke_look_on_songselect();
    assert_eq!(op.dialog(), Some(DialogKind::SongSelect));
    assert_eq!(testing::COPIED.with(|c| c.borrow().clone()).as_deref(), Some("Way Maker"));
}

#[test]
fn dragging_songselect_files_brings_them_in() {
    use crate::ui::operator::files::{Route, route};
    for name in ["a.txt", "a.usr", "a.chordpro", "a.cho", "a.chopro", "a.pro", "A.USR"] {
        assert_eq!(route(std::path::Path::new(name)), Route::Song, "{name}");
    }
    let dir = std::env::temp_dir().join(format!("rp-songselect-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let (usr, cho) = (dir.join("song.usr"), dir.join("hills.chordpro"));
    fs::write(&usr, USR).unwrap();
    fs::write(&cho, CHORDPRO).unwrap();
    let app = test_app();
    let op = opened(&app);
    files::add_files(&op, vec![usr, cho]);
    let lib = songs(&app);
    let cclis: Vec<&str> = lib.iter().map(|s| s.ccli.as_str()).collect();
    assert_eq!(cclis, ["7654321", "7000001"]);
    assert_eq!(lib[1].lyrics, "Verse 1\nOver the hills the morning comes\nLight on the fields\n\nChorus\nSing, sing, sing\nAll the hills will ring");
    assert_eq!(app.data.lock().unwrap().services[0].items.len(), 2, "both go into the service, as a dropped lyric file does");
    assert!(!toasts(&op).iter().any(|t| t.1), "{:?}", toasts(&op));
}
