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

/// Opens the data folder behind a port already held. Left-over work folders go on every start, so a killed
/// import's go too.
pub fn open(listener: TcpListener, root: PathBuf, port: u16) -> Result<Started, String> {
    let store = Store::open(root.clone()).map_err(|e| crate::tr!("Cannot use the data folder {path}: {reason}", path = root.display(), reason = e))?;
    store.clean_tmp();
    // New data names its first service in the computer's language, so that language is set before it is made.
    if !store.data_path().exists() { let _ = crate::i18n::set_language(&crate::i18n::resolve("")); }
    let crate::store::Loaded { mut data, read_only } = store.load_full()?;
    let removed = crate::bible::blocklist::strip(&mut data);
    if !read_only && (removed > 0 || !store.data_path().exists()) { let _ = store.save(&data); }

    let lan_ip = UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| { s.connect("192.0.2.1:80").ok()?; s.local_addr().ok().map(|a| a.ip()) });
    let lan_url = lan_ip.map(|ip| format!("http://{ip}:{port}/remote"));
    let app = App::new(store, data, port, lan_url);
    app.set_read_only(read_only);
    // 1.3: song auto-follow's worker (idle until a song follows the singers).
    app.start_follower();
    if removed > 0 { app.live.lock().unwrap().notice = Some(crate::scripture::strings::passages_removed(removed)); }
    Ok(Started { app, listener, root })
}

/// The console banner; `quit` says how to quit.
pub fn banner(s: &Started, quit: &str) {
    println!("\n  RustyPresenter {}\n", env!("CARGO_PKG_VERSION"));
    if let Some(u) = &s.app.lan_url { println!("  Phone remote:  {u}"); }
    println!("  Your data:     {}", s.root.display());
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
    // The same pages over https on the next port, for the phone's microphone (`network::secure`).
    tokio::spawn(crate::network::secure::serve(app.clone()));
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

/// A complete backup at every start, then daily. A restart after a crash (`restored`) waits the full day:
/// another copy of the same data each time would push older good backups out of the ten kept.
pub async fn backups(app: Arc<App>, restored: bool) {
    if restored { tokio::time::sleep(BACKUP_EVERY).await; }
    loop {
        let data = app.data.lock().unwrap().clone();
        if !data.songs.is_empty() || data.services.iter().any(|s| !s.items.is_empty()) {
            if let Err(e) = app.store.backup(&data) { eprintln!("Backup failed: {e}"); }
        }
        tokio::time::sleep(BACKUP_EVERY).await;
    }
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
        Store::open(root.clone()).unwrap().save(&Data { services: vec![service], ..Data::default() }).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let started = open(listener, root, 0).unwrap();
        assert_eq!(started.app.data.lock().unwrap().services[0].items.len(), 1);
        assert_eq!(started.app.store.load().unwrap().services[0].items.len(), 1, "saved as well");
        assert_eq!(started.app.live.lock().unwrap().notice.as_deref(), Some(passages_removed(1).as_str()));
    }

    const XX: &str = "msgid \"\"\nmsgstr \"\"\n\"Language: xx\\n\"\n\nmsgid \"Sunday Service\"\nmsgstr \"Culto dominical\"\n";

    #[test]
    fn only_new_data_names_its_first_service_in_the_app_language() {
        crate::i18n::testing::with_catalog(XX, || {
            assert_eq!(Data::default().services[0].name, "Culto dominical");
            let root = test_dir();
            Store::open(root.clone()).unwrap().save(&Data { services: vec![Service::new("Sunday Service")], ..Data::default() }).unwrap();
            assert_eq!(Store::open(root).unwrap().load().unwrap().services[0].name, "Sunday Service", "a saved service keeps its name");
        });
        assert_eq!(Data::default().services[0].name, "Sunday Service");
    }
}
