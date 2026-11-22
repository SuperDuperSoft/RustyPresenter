//! Startup: the port (single instance), the data folder,
//! `App`, the phone/web server and the daily backup.
use crate::{app::App, server, store::Store};
use std::{net::{SocketAddr, TcpListener, UdpSocket}, path::PathBuf, sync::Arc, time::Duration};

/// The core of a running RustyPresenter, before any window.
pub struct Started { pub app: Arc<App>, pub listener: TcpListener, pub root: PathBuf }

/// The port the server listens on.
pub fn port() -> u16 { std::env::var("RUSTYPRESENTER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(4318) }

/// The data folder, without creating it.
pub fn data_root() -> Result<PathBuf, String> {
    server::data_dir().ok_or_else(|| crate::tr!("Cannot find a folder to keep your data in. Set RUSTYPRESENTER_DATA."))
}

/// Opens the data folder behind a port already held, with nobody to ask: data that cannot be read is an error.
pub fn open(listener: TcpListener, root: PathBuf, port: u16) -> Result<Started, String> { open_asking(listener, root, port, &|_| false) }

/// Opens the data folder behind a port already held. Left-over work folders go on every start, so a killed
/// import's go too. When `data.json` cannot be read and there is a complete backup, `ask` is shown a question:
/// on yes the unreadable file is set aside and the app starts from the newest backup. Never without asking.
pub fn open_asking(listener: TcpListener, root: PathBuf, port: u16, ask: &dyn Fn(&str) -> bool) -> Result<Started, String> {
    let store = Store::open(root.clone()).map_err(|e| crate::tr!("Cannot use the data folder {path}: {reason}", path = root.display(), reason = e))?;
    store.clean_tmp();
    // New data names its first service in the computer's language, so that language is set before it is made.
    if !store.data_path().exists() { let _ = crate::i18n::set_language(&crate::i18n::resolve("")); }
    let (loaded, from_backup) = match store.load_full() {
        Ok(loaded) => (loaded, None),
        Err(problem) => { let (loaded, said) = start_from_backup(&store, &problem, ask)?; (loaded, Some(said)) }
    };
    let crate::store::Loaded { mut data, read_only } = loaded;
    let removed = crate::bible::blocklist::strip(&mut data);
    // A save that fails here is said once the operator window is up: the passages are gone from memory, not yet
    // from the file.
    let unsaved = if !read_only && (removed > 0 || !store.data_path().exists()) { store.save(&data).err() } else { None };

    let lan_ip = UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| { s.connect("192.0.2.1:80").ok()?; s.local_addr().ok().map(|a| a.ip()) });
    let lan_url = lan_ip.map(|ip| format!("http://{ip}:{port}/remote"));
    let app = App::new(store, data, port, lan_url);
    app.set_read_only(read_only);
    // 1.3: song auto-follow's worker (idle until a song follows the singers).
    app.start_follower();
    if removed > 0 { app.live.lock().unwrap().notice = Some(crate::scripture::strings::passages_removed(removed)); }
    if let Some(e) = unsaved { app.live.lock().unwrap().notice = Some(crate::tr!("Could not save: {reason}", reason = e)); }
    if let Some(said) = from_backup { app.live.lock().unwrap().notice = Some(said); }
    Ok(Started { app, listener, root })
}

/// When a backup was made, in this computer's time ("2026-11-06 10:15"), from its stamp.
fn backup_when(created: &str) -> String {
    match chrono::NaiveDateTime::parse_from_str(created, "%Y-%m-%d_%H%M%S") {
        Ok(utc) => utc.and_utc().with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string(),
        Err(_) => created.to_string(),
    }
}

/// `data.json` could not be read (`problem`). With a complete backup to offer, asks whether to start from the
/// newest one; on yes the unreadable file is moved aside (`data.unreadable-<stamp>.json`) and the backup restored.
/// Gives the data and the line the operator window shows. On no, or with no backup, the problem is the error.
fn start_from_backup(store: &Store, problem: &str, ask: &dyn Fn(&str) -> bool) -> Result<(crate::store::Loaded, String), String> {
    let Some(newest) = store.backups().into_iter().next() else { return Err(problem.into()) };
    let when = backup_when(&newest.created);
    let aside = format!("data.unreadable-{}.json", crate::store::now_stamp());
    let question = crate::tr!(
        "Your saved songs and services could not be read.\n\nStart with the newest backup, made {when}?\n\nThe file that could not be read is kept as {name}. Choose No to close RustyPresenter and change nothing.\n\n({problem})",
        when = when, name = aside, problem = problem);
    if !ask(&question) { return Err(problem.into()) }
    // Set aside first, so nothing is ever written over the file that could not be read.
    std::fs::rename(store.data_path(), store.root.join(&aside)).map_err(|e| format!("{problem}\n{e}"))?;
    match store.restore(&newest.name) {
        Ok(data) => Ok((crate::store::Loaded { data, read_only: false }, crate::tr!("Started from the backup made {when}. The file that could not be read is kept as {name}.", when = when, name = aside))),
        Err(e) => {
            let _ = std::fs::rename(store.root.join(&aside), store.data_path());
            Err(crate::tr!("{problem}\n\nThe newest backup could not be used either: {reason}", problem = problem, reason = e))
        }
    }
}

/// The console banner; `quit` says how to quit.
pub fn banner(s: &Started, quit: &str) {
    println!("\n  RustyPresenter {}\n", env!("CARGO_PKG_VERSION"));
    if let Some(u) = &s.app.lan_url { println!("  Remote:        {u}"); }
    let portable = if crate::platform::data_folder::portable_dir().is_some() { "  (portable: beside the app)" } else { "" };
    println!("  Your data:     {}{portable}", s.root.display());
    println!("\n  {quit}\n");
}

/// Spec §6.4: pauses what can be heard when the chosen sound output goes away.
pub async fn watch_sound(app: Arc<App>) {
    let mut every = tokio::time::interval(WATCH_SOUND_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        every.tick().await;
        let app = app.clone();
        let _ = tokio::task::spawn_blocking(move || app.watch_sound()).await;
    }
}

/// The operator page, phone remote and room/stage pages. Runs on a tokio runtime.
pub async fn serve(app: Arc<App>, listener: TcpListener) {
    // N31: the TCP control, when it is turned on.
    if let Err(e) = crate::network::tcp::sync(&app) { eprintln!("{e}"); }
    // The phone pages over https too, for the phone camera (a browser opens the camera only on a private page).
    tokio::spawn(crate::network::https::serve(app.clone(), crate::network::https::port_for(app.port)));
    let svc = server::router(app).into_make_service_with_connect_info::<SocketAddr>();
    let listener = match tokio::net::TcpListener::from_std(listener) { Ok(l) => l, Err(e) => { eprintln!("Server could not start: {e}"); return; } };
    if let Err(e) = axum::serve(listener, svc).await { eprintln!("Server stopped: {e}"); }
}

const BACKUP_EVERY: Duration = Duration::from_secs(24 * 3600);
/// While something plays, its position is saved this often: the room's reports alone don't save it.
const SAVE_PLAYING_EVERY: Duration = Duration::from_secs(5);
/// How long a video can play on with its sound output gone. Each look starts a small mpv, but only while something
/// can be heard and a sound output is chosen.
const WATCH_SOUND_EVERY: Duration = Duration::from_secs(3);

/// A complete backup at every start, then daily, when something changed since the newest one. A restart after a
/// crash (`restored`) waits the full day.
pub async fn backups(app: Arc<App>, restored: bool) {
    if restored { tokio::time::sleep(BACKUP_EVERY).await; }
    loop {
        backup_once(&app).await;
        tokio::time::sleep(BACKUP_EVERY).await;
    }
}

/// One automatic backup, off the async workers (it copies files and waits for the disk). A backup that could not
/// be made is said in the operator window: the release build has no console to print to.
async fn backup_once(app: &Arc<App>) {
    let data = app.data.lock().unwrap().clone();
    if data.songs.is_empty() && data.services.iter().all(|s| s.items.is_empty()) { return }
    let worker = app.clone();
    // Nothing changed since the newest backup: another copy would only crowd the list.
    let made = tokio::task::spawn_blocking(move || if worker.store.backed_up(&data) { Ok(String::new()) } else { worker.store.backup(&data) }).await;
    let failed = match made { Ok(Ok(_)) => return, Ok(Err(e)) => e.to_string(), Err(e) => e.to_string() };
    eprintln!("Backup failed: {failed}");
    app.set_notice(&crate::tr!("The automatic backup could not be made: {reason}", reason = failed));
}

/// Keeps the saved frame's playback positions fresh while video or audio is on air.
pub async fn save_playing(app: Arc<App>) {
    let mut every = tokio::time::interval(SAVE_PLAYING_EVERY);
    loop {
        every.tick().await;
        if app.playing() { app.save_live_state(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::{Data, ItemKind, Service, ServiceItem}, scripture::strings::passages_removed, store::test_dir};

    #[test]
    fn a_saved_service_opens_without_blocked_passages() {
        let root = test_dir();
        let mut service = Service::new("Sunday");
        let passage = |translation: &str| ServiceItem { id: uuid::Uuid::new_v4(), background: None, notes: String::new(), theme: None,
            kind: ItemKind::Scripture { reference: "John 3:16".into(), translation: translation.into(), verses: vec![], per_slide: 0, text_size: 0, also: vec![], layout: Default::default(), breaks: Default::default() } };
        service.items = vec![passage("KJV"), passage("NWT")];
        Store::open(root.clone()).unwrap().save(&Data { services: vec![service].into(), ..Data::default() }).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let started = open(listener, root, 0).unwrap();
        assert_eq!(started.app.data.lock().unwrap().services[0].items.len(), 1);
        assert_eq!(started.app.store.load().unwrap().services[0].items.len(), 1, "saved as well");
        assert_eq!(started.app.live.lock().unwrap().notice.as_deref(), Some(passages_removed(1).as_str()));
    }

    /// B2: a backup that fails is said where the operator looks.
    #[tokio::test]
    async fn a_failed_automatic_backup_sets_the_live_notice() {
        let root = test_dir();
        let mut data = Data::default();
        data.songs.push(crate::model::Song { title: "Amazing Grace".into(), ..Default::default() });
        Store::open(root.clone()).unwrap().save(&data).unwrap();
        let started = open(TcpListener::bind("127.0.0.1:0").unwrap(), root.clone(), 0).unwrap();
        backup_once(&started.app).await;
        assert_eq!(started.app.live.lock().unwrap().notice, None, "a backup that works says nothing");
        assert_eq!(started.app.store.backups().len(), 1);
        // B8: nothing changed, so the next one makes no second copy; a change does.
        backup_once(&started.app).await;
        assert_eq!(started.app.store.backups().len(), 1);
        started.app.data.lock().unwrap().songs[0].title = "Amazing Grace (My Chains Are Gone)".into();
        backup_once(&started.app).await;
        assert_eq!(started.app.store.backups().len(), 2);
        // The backups folder is in the way as a file: nothing can be written there.
        std::fs::remove_dir_all(root.join("backups")).unwrap();
        std::fs::write(root.join("backups"), b"not a folder").unwrap();
        backup_once(&started.app).await;
        let notice = started.app.live.lock().unwrap().notice.clone().unwrap_or_default();
        assert!(notice.starts_with("The automatic backup could not be made"), "{notice}");
    }

    /// B15: data that cannot be read offers the newest backup, and never takes it without a yes.
    #[test]
    fn unreadable_data_starts_from_the_newest_backup_only_after_a_yes() {
        let root = test_dir();
        let store = Store::open(root.clone()).unwrap();
        let mut data = Data::default();
        data.songs.push(crate::model::Song { title: "Amazing Grace".into(), ..Default::default() });
        store.save(&data).unwrap();
        let backup = store.backup(&data).unwrap();
        std::fs::write(store.data_path(), b"{ half a file").unwrap();
        let asked = std::cell::RefCell::new(vec![]);
        let listener = || TcpListener::bind("127.0.0.1:0").unwrap();
        // With nobody to ask, and on No: an error, and nothing on the disk changes.
        assert!(open(listener(), root.clone(), 0).is_err_and(|e| e.contains("could not be read")));
        let no = open_asking(listener(), root.clone(), 0, &|q| { asked.borrow_mut().push(q.to_string()); false });
        assert!(no.is_err_and(|e| e.contains("could not be read")));
        assert_eq!(std::fs::read(store.data_path()).unwrap(), b"{ half a file");
        assert!(asked.borrow()[0].contains("Start with the newest backup") && asked.borrow()[0].contains("data.unreadable-"), "{:?}", asked.borrow());
        // On Yes: the backup's songs, the unreadable file kept beside it, and the operator is told.
        let started = open_asking(listener(), root.clone(), 0, &|_| true).unwrap();
        assert_eq!(started.app.data.lock().unwrap().songs[0].title, "Amazing Grace");
        assert_eq!(started.app.store.load().unwrap().songs.len(), 1, "data.json is readable again");
        let kept: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("data.unreadable-")).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read(kept[0].path()).unwrap(), b"{ half a file");
        assert!(started.app.live.lock().unwrap().notice.as_deref().is_some_and(|n| n.starts_with("Started from the backup made")));
        assert!(started.app.store.backups().iter().any(|b| b.name == backup));
        // With no backup at all there is nothing to offer: the error as before, without a question.
        let empty = test_dir();
        std::fs::write(Store::open(empty.clone()).unwrap().data_path(), b"garbage").unwrap();
        assert!(open_asking(listener(), empty, 0, &|_| panic!("nothing to ask about")).is_err());
    }

    const XX: &str = "msgid \"\"\nmsgstr \"\"\n\"Language: xx\\n\"\n\nmsgid \"Sunday Service\"\nmsgstr \"Culto dominical\"\n";

    #[test]
    fn only_new_data_names_its_first_service_in_the_app_language() {
        crate::i18n::testing::with_catalog(XX, || {
            assert_eq!(Data::default().services[0].name, "Culto dominical");
            let root = test_dir();
            Store::open(root.clone()).unwrap().save(&Data { services: vec![Service::new("Sunday Service")].into(), ..Data::default() }).unwrap();
            assert_eq!(Store::open(root).unwrap().load().unwrap().services[0].name, "Sunday Service", "a saved service keeps its name");
        });
        assert_eq!(Data::default().services[0].name, "Sunday Service");
    }
}
