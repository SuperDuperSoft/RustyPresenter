//! The phone itself: the window (`ui/app.slint`), and the work behind each tab on threads of its own, so the
//! screen never waits for the Wi-Fi. Each tab's thread stops when the tab is left (its "turn" number moves on);
//! the microphone and camera go on until Stop is tapped.
mod camera;
mod jni;
mod mic;

use crate::{
    address, discovery,
    fit,
    http::{Client, StreamEnd},
    protocol::{self, CommandResult, NowShowing, PairCode, PairResult, RemoteMsg, RemoteView, StageView},
    saved::Saved,
    ws,
};
use serde_json::json;
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}},
    time::{Duration, Instant},
};

use crate::screens::*;

type C = Arc<Ctx>;

/// What the threads share.
pub struct Ctx {
    ui: slint::Weak<AppWindow>,
    saved: Mutex<Saved>,
    saved_path: PathBuf,
    client: Mutex<Option<Arc<Client>>>,
    view: Mutex<Option<RemoteView>>,
    /// The item whose slides are open in the sheet.
    sheet_item: Mutex<Option<usize>>,
    /// Turn numbers: a thread whose turn has passed stops.
    main_turn: AtomicU64,
    tab_turn: AtomicU64,
    find_turn: AtomicU64,
    tab: AtomicU64,
    mic_on: Mutex<Option<Arc<AtomicBool>>>,
    cam_on: Mutex<Option<Arc<AtomicBool>>>,
    scan_on: Mutex<Option<Arc<AtomicBool>>>,
    /// The QR code's one-time secret, until pairing uses it.
    secret: Mutex<Option<String>>,
    /// The stage tab's size in pixels, and the last stage view and its pictures (live/next, by version).
    stage_size: Mutex<(f32, f32)>,
    stage: Mutex<Option<StageView>>,
    stage_pics: Mutex<HashMap<String, (u64, SharedPixelBuffer<Rgba8Pixel>)>>,
    /// The live slide's picture already asked for (its version).
    picture_version: AtomicU64,
    /// A permission being asked: which, what to do once given, and what when not.
    asking: Mutex<Option<Asking>>,
    toast_turn: AtomicU64,
}

/// `since`: when Android was asked; `left`: Android's question took the screen (when it does not, Android said no
/// without asking: the permission was refused for good, or by the phone's rules).
struct Asking { permission: &'static str, then: Box<dyn FnOnce(&C) + Send>, denied: Box<dyn FnOnce(&C) + Send>, since: Option<Instant>, left: bool }

/// Runs `f` on the window's thread.
fn ui(c: &C, f: impl FnOnce(&AppWindow) + Send + 'static) { let _ = c.ui.upgrade_in_event_loop(move |w| f(&w)); }


fn spawn(f: impl FnOnce() + Send + 'static) { let _ = std::thread::Builder::new().stack_size(1 << 20).spawn(f); }


fn decode_jpeg(bytes: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg).ok()?.into_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(img.as_raw(), img.width(), img.height()))
}

#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    jni::set_app(app.clone());
    let pending: Arc<Mutex<Option<C>>> = Arc::new(Mutex::new(None));
    let p2 = pending.clone();
    // Back from Android's permission question: carry on with what asked.
    slint::android::init_with_event_listener(app.clone(), move |e| {
        use slint::android::android_activity::{MainEvent, PollEvent};
        let Some(c) = p2.lock().unwrap().clone() else { return };
        match e {
            PollEvent::Main(MainEvent::LostFocus) => { if let Some(a) = c.asking.lock().unwrap().as_mut() && a.since.is_some() { a.left = true } }
            PollEvent::Main(MainEvent::GainedFocus) => permission_answered(&c),
            _ => {}
        }
    }).expect("Slint on Android");
    let w = AppWindow::new().expect("the window");
    // The words in the phone's language (after the window is made: its words are bundled with it).
    let config = app.config();
    if let Some(l) = crate::lang::pick(&config.language().unwrap_or_default(), &config.country().unwrap_or_default()) {
        let _ = slint::select_bundled_translation(l);
    }
    let saved_path = app.internal_data_path().unwrap_or_else(std::env::temp_dir).join("remote.json");
    let saved = Saved::load(&saved_path);
    let c: C = Arc::new(Ctx {
        ui: w.as_weak(), saved: Mutex::new(saved.clone()), saved_path, client: Mutex::new(None), view: Mutex::new(None), sheet_item: Mutex::new(None),
        main_turn: AtomicU64::new(0), tab_turn: AtomicU64::new(0), find_turn: AtomicU64::new(0), tab: AtomicU64::new(0),
        mic_on: Mutex::new(None), cam_on: Mutex::new(None), scan_on: Mutex::new(None), secret: Mutex::new(None),
        stage_size: Mutex::new((0.0, 0.0)), stage: Mutex::new(None), stage_pics: Mutex::new(HashMap::new()), picture_version: AtomicU64::new(u64::MAX),
        asking: Mutex::new(None), toast_turn: AtomicU64::new(0),
    });
    *pending.lock().unwrap() = Some(c.clone());
    let st = w.global::<S>();
    st.set_notes_on(saved.notes);
    st.set_tally_camera(saved.tally_camera.clamp(1, 9) as i32);
    st.set_cam_back(saved.back_camera);
    st.set_cam_name(s(saved.camera_name.clone()));
    if !saved.host.is_empty() { st.set_typed(s(address::shown(&saved.host))) }
    wire(&w, &c);
    start(&c);
    w.run().expect("the window runs");
}

// ---------- wiring the taps ----------

fn wire(w: &AppWindow, c: &C) {
    let st = w.global::<S>();
    macro_rules! on {
        ($name:ident, |$c:ident $(, $a:ident : $t:ty)*| $body:block) => {{
            let cc = c.clone();
            st.$name(move |$($a: $t),*| { let $c = &cc; $body });
        }};
    }
    on!(on_choose, |c, i: i32| {
        let host = c.ui.upgrade().and_then(|w| w.global::<S>().get_found().row_data(i as usize)).map(|f| f.address.to_string());
        if let Some(t) = host.as_deref().and_then(address::parse) { connect(c, t) }
    });
    on!(on_connect_typed, |c, text: SharedString| {
        match address::parse(&text) {
            Some(t) => connect(c, t),
            None => ui(c, |w| { let st = w.global::<S>(); st.set_find_note(w.global::<Say>().invoke_bad_address()); st.set_find_problem(true); }),
        }
    });
    on!(on_search_again, |c| { search(c) });
    on!(on_scan_qr, |c| { scan_qr(c) });
    on!(on_scan_cancel, |c| { stop_scan(c); ui(c, |w| w.global::<S>().set_page(s("find"))); });
    on!(on_pair, |c, code: SharedString, password: SharedString| { pair(c, code.to_string(), password.to_string()) });
    on!(on_other_computer, |c| { other_computer(c) });
    on!(on_tab_chosen, |c, t: i32| { tab_chosen(c, t) });
    on!(on_next, |c| { let base = version(c); send(c, protocol::command("next", json!({}), base)) });
    on!(on_back, |c| { let base = version(c); send(c, protocol::command("prev", json!({}), base)) });
    on!(on_clear, |c, which: SharedString| { send(c, protocol::command(&which, json!({}), None)) });
    on!(on_jump, |c, i: i32| {
        let v = c.view.lock().unwrap().clone();
        if let Some(v) = v && let (Some(cur), Some(sec)) = (v.cursor.as_ref(), v.sections.get(i as usize)) {
            send(c, protocol::command("jump", json!({ "item_id": cur.item_id, "kind": sec.kind, "occurrence": sec.occurrence }), Some(v.version)));
        }
    });
    on!(on_open_item, |c, i: i32| { open_item(c, i as usize) });
    on!(on_show_slide, |c, i: i32| {
        let v = c.view.lock().unwrap().clone();
        let item = *c.sheet_item.lock().unwrap();
        if let (Some(v), Some(item)) = (v, item) && let Some(it) = v.items.get(item) {
            send(c, protocol::command("show", json!({ "item_id": it.id, "slide": i }), Some(v.version)));
        }
        ui(c, |w| w.global::<S>().set_sheet_open(false));
    });
    on!(on_media_toggle, |c| {
        let m = c.view.lock().unwrap().as_ref().and_then(|v| v.media.clone());
        if let Some(m) = m { send(c, protocol::command("media_pause", json!({ "inst": m.inst, "paused": !m.paused }), None)) }
    });
    on!(on_notes_toggled, |c, on: bool| { remember(c, |s| s.notes = on) });
    on!(on_stage_login, |c, pw: SharedString| { stage_login(c, pw.to_string()) });
    on!(on_stage_size, |c, w: f32, h: f32| {
        *c.stage_size.lock().unwrap() = (w, h);
        draw_stage(c);
    });
    on!(on_tally_camera_chosen, |c, n: i32| {
        let n = n.clamp(1, 9);
        remember(c, |s| s.tally_camera = n as u32);
        ui(c, move |w| w.global::<S>().set_tally_camera(n));
    });
    on!(on_mic_toggle, |c| { mic_toggle(c) });
    on!(on_cam_toggle, |c, name: SharedString| { cam_toggle(c, name.to_string()) });
    on!(on_cam_flip, |c, back: bool| {
        remember(c, |s| s.back_camera = back);
        ui(c, move |w| w.global::<S>().set_cam_back(back));
        // A running camera starts again with the other one.
        let running = c.cam_on.lock().unwrap().clone();
        if let Some(flag) = running {
            flag.store(false, Ordering::Relaxed);
            let c2 = c.clone();
            spawn(move || { std::thread::sleep(Duration::from_millis(400)); let name = c2.saved.lock().unwrap().camera_name.clone(); cam_start(&c2, name) });
        }
    });
    on!(on_ask_go, |c| { ask_go(c) });
    on!(on_ask_cancel, |c| {
        c.asking.lock().unwrap().take();
        ui(c, |w| w.global::<S>().set_ask_why(s("")));
    });
    let cc = c.clone();
    st.on_back_key(move || back_key(&cc));
}

/// A setting changed: kept on the phone.
fn remember(c: &C, f: impl FnOnce(&mut Saved)) {
    let mut sv = c.saved.lock().unwrap();
    f(&mut sv);
    let _ = sv.save(&c.saved_path);
}

fn toast(c: &C, text: String) {
    let turn = c.toast_turn.fetch_add(1, Ordering::Relaxed) + 1;
    ui(c, move |w| w.global::<S>().set_toast(s(text)));
    let c2 = c.clone();
    spawn(move || {
        std::thread::sleep(Duration::from_millis(2600));
        if c2.toast_turn.load(Ordering::Relaxed) == turn { ui(&c2, |w| w.global::<S>().set_toast(s(""))) }
    });
}

/// A word from `Say`, worked out on the window's thread (where the translations are) and handed to `then`.
fn say(c: &C, word: impl FnOnce(&Say) -> SharedString + Send + 'static, then: impl FnOnce(&C, String) + Send + 'static) {
    let c2 = c.clone();
    ui(c, move |w| { let t = word(&w.global::<Say>()).to_string(); spawn(move || then(&c2, t)) });
}

fn back_key(c: &C) -> bool {
    let Some(w) = c.ui.upgrade() else { return false };
    let st = w.global::<S>();
    if st.get_ask_why() != "" { c.asking.lock().unwrap().take(); st.set_ask_why(s("")); return true }
    if st.get_sheet_open() { st.set_sheet_open(false); return true }
    match st.get_page().as_str() {
        "scan" => { stop_scan(c); st.set_page(s("find")); true }
        "pair" => { other_computer(c); true }
        "main" if st.get_tab() == 1 && !st.get_stage_bar() => { st.set_stage_bar(true); true }
        "main" if st.get_tab() != 0 => { st.set_tab(0); tab_chosen(c, 0); true }
        _ => false,
    }
}

// ---------- finding and pairing the computer ----------

fn start(c: &C) {
    let saved = c.saved.lock().unwrap().clone();
    if saved.host.is_empty() { search(c); return }
    let client = Arc::new(Client::new(&saved.host));
    client.set_cookie("rp_session", saved.session.as_deref());
    client.set_cookie("rp_stage", saved.stage.as_deref());
    *c.client.lock().unwrap() = Some(client.clone());
    let c = c.clone();
    spawn(move || {
        let name = if saved.name.is_empty() { address::shown(&saved.host) } else { saved.name.clone() };
        match client.get("/remote/status").map(|r| r.json()) {
            Ok(v) if v["paired"] == true => open_main(&c),
            Ok(v) if v["paired"] == false => show_pair(&c, name, v["password"] == true, false),
            _ => {
                let shown = address::shown(&saved.host);
                ui(&c, move |w| { let st = w.global::<S>(); st.set_find_note(w.global::<Say>().invoke_cannot_reach(s(shown))); st.set_find_problem(true); });
                search_keeping_note(&c);
            }
        }
    });
}

fn search(c: &C) {
    ui(c, |w| { let st = w.global::<S>(); st.set_find_note(s("")); st.set_find_problem(false); });
    search_keeping_note(c);
}

/// Looks for computers on the Wi-Fi (`discovery`), listing each as it is found.
fn search_keeping_note(c: &C) {
    let turn = c.find_turn.fetch_add(1, Ordering::Relaxed) + 1;
    ui(c, |w| { let st = w.global::<S>(); st.set_page(s("find")); st.set_searching(true); st.set_found(ModelRc::new(VecModel::<Found>::default())); });
    let c = c.clone();
    spawn(move || {
        let own = discovery::own_ipv4s();
        if own.is_empty() {
            ui(&c, |w| { let st = w.global::<S>(); st.set_searching(false); st.set_find_note(w.global::<Say>().invoke_no_wifi()); st.set_find_problem(true); });
            return;
        }
        let c2 = c.clone();
        let found = discovery::scan(&own, address::PORT, move |f| {
            if c2.find_turn.load(Ordering::Relaxed) != turn { return }
            let row = Found { name: s(f.name.clone()), address: s(address::shown(&f.host)) };
            ui(&c2, move |w| {
                let st = w.global::<S>();
                let mut rows: Vec<Found> = st.get_found().iter().collect();
                if !rows.iter().any(|r| r.address == row.address) { rows.push(row) }
                st.set_found(ModelRc::new(VecModel::from(rows)));
            });
        });
        if c.find_turn.load(Ordering::Relaxed) != turn { return }
        let none = found.is_empty();
        ui(&c, move |w| {
            let st = w.global::<S>();
            st.set_searching(false);
            if none && !st.get_find_problem() { st.set_find_note(w.global::<Say>().invoke_none_found()) }
        });
    });
}

/// Connects to the computer at `t`: paired already → the tabs; else pairing (straight away with the QR code's secret).
fn connect(c: &C, t: address::Target) {
    c.find_turn.fetch_add(1, Ordering::Relaxed);
    ui(c, |w| w.global::<S>().set_busy(true));
    let c = c.clone();
    spawn(move || {
        let same = c.saved.lock().unwrap().host == t.host;
        let client = Arc::new(Client::new(&t.host));
        if same {
            let sv = c.saved.lock().unwrap().clone();
            client.set_cookie("rp_session", sv.session.as_deref());
            client.set_cookie("rp_stage", sv.stage.as_deref());
        }
        let shown = address::shown(&t.host);
        let r = client.get("/remote/status");
        ui(&c, |w| w.global::<S>().set_busy(false));
        let r = match r {
            Ok(r) => r,
            Err(_) => {
                ui(&c, move |w| { let st = w.global::<S>(); st.set_page(s("find")); st.set_find_note(w.global::<Say>().invoke_cannot_reach(s(shown))); st.set_find_problem(true); });
                return;
            }
        };
        let Some(found) = discovery::recognise(&t.host, r.status, &r.body) else {
            ui(&c, move |w| { let st = w.global::<S>(); st.set_page(s("find")); st.set_find_note(w.global::<Say>().invoke_not_rustypresenter(s(shown))); st.set_find_problem(true); });
            return;
        };
        remember(&c, |sv| sv.choose(&t.host, &found.name));
        *c.client.lock().unwrap() = Some(client);
        let status: protocol::RemoteStatus = serde_json::from_slice(&r.body).unwrap_or_default();
        let name = if found.name.is_empty() { shown } else { format!("{} · {shown}", found.name) };
        *c.secret.lock().unwrap() = t.secret.clone();
        if status.paired && t.secret.is_none() { open_main(&c); return }
        show_pair(&c, name, status.password, t.secret.is_some());
        // The QR code without a password needs no typing at all.
        if t.secret.is_some() && !status.password { pair(&c, String::new(), String::new()) }
    });
}

fn show_pair(c: &C, name: String, password: bool, secret: bool) {
    ui(c, move |w| {
        let st = w.global::<S>();
        st.set_computer(s(name));
        st.set_needs_password(password);
        st.set_has_secret(secret);
        st.set_pair_error(s(""));
        st.set_page(s("pair"));
    });
}

fn pair(c: &C, code: String, password: String) {
    let Some(client) = c.client.lock().unwrap().clone() else { return };
    let secret = c.secret.lock().unwrap().clone();
    let code = match secret { Some(sec) => PairCode::Secret(sec), None => PairCode::Pin(code) };
    ui(c, |w| { let st = w.global::<S>(); st.set_busy(true); st.set_pair_error(s("")); });
    let c2 = c.clone();
    say(c, |w| w.invoke_phone_name(), move |_, fallback| {
        let c = c2;
        let name = jni::device_name().unwrap_or(fallback);
        let r = client.post_json("/remote/pair", &protocol::pair_request(&code, &password, &name));
        ui(&c, |w| w.global::<S>().set_busy(false));
        match r.as_ref().map(protocol::pair_result) {
            Ok(PairResult::Paired(token)) => {
                c.secret.lock().unwrap().take();
                remember(&c, |sv| sv.session = Some(token));
                open_main(&c);
            }
            Ok(PairResult::Error { text, retry_with_pin }) => {
                if retry_with_pin { c.secret.lock().unwrap().take(); }
                ui(&c, move |w| {
                    let st = w.global::<S>();
                    if retry_with_pin { st.set_has_secret(false) }
                    st.set_pair_error(if text.is_empty() { w.global::<Say>().invoke_could_not_connect() } else { s(text) });
                });
            }
            Err(_) => ui(&c, |w| { let st = w.global::<S>(); st.set_pair_error(w.global::<Say>().invoke_not_connected()); }),
        }
    });
}

fn other_computer(c: &C) {
    c.main_turn.fetch_add(1, Ordering::Relaxed);
    c.tab_turn.fetch_add(1, Ordering::Relaxed);
    stop_flag(&c.mic_on);
    stop_flag(&c.cam_on);
    c.secret.lock().unwrap().take();
    keep_awake(c);
    search(c);
}

fn stop_flag(m: &Mutex<Option<Arc<AtomicBool>>>) { if let Some(f) = m.lock().unwrap().take() { f.store(false, Ordering::Relaxed) } }

fn client(c: &C) -> Option<Arc<Client>> { c.client.lock().unwrap().clone() }

/// Paired: the tabs, and the remote's live updates.
fn open_main(c: &C) {
    ui(c, |w| w.global::<S>().set_page(s("main")));
    let turn = c.main_turn.fetch_add(1, Ordering::Relaxed) + 1;
    let tab = c.tab.load(Ordering::Relaxed) as i32;
    tab_chosen(c, tab);
    let c = c.clone();
    spawn(move || remote_events(&c, turn));
}

/// Back on the pairing page: this phone was unpaired on the computer.
fn pair_again(c: &C) {
    c.main_turn.fetch_add(1, Ordering::Relaxed);
    remember(c, |sv| sv.session = None);
    if let Some(cl) = client(c) { cl.set_cookie("rp_session", None) }
    let name = { let sv = c.saved.lock().unwrap(); if sv.name.is_empty() { address::shown(&sv.host) } else { sv.name.clone() } };
    show_pair(c, name, false, false);
    let c2 = c.clone();
    spawn(move || {
        if let Some(cl) = client(&c2) && let Ok(r) = cl.get("/remote/status") {
            let pw = r.json()["password"] == true;
            ui(&c2, move |w| w.global::<S>().set_needs_password(pw));
        }
    });
    ui(c, |w| { let st = w.global::<S>(); st.set_pair_error(w.global::<Say>().invoke_pair_again()); });
}

// ---------- Remote ----------

fn version(c: &C) -> Option<u64> { c.view.lock().unwrap().as_ref().map(|v| v.version) }

fn remote_events(c: &C, turn: u64) {
    let mine = || c.main_turn.load(Ordering::Relaxed) == turn;
    while mine() {
        let Some(cl) = client(c) else { return };
        let end = cl.events("/remote/events", Duration::from_secs(25), &mine, |data| {
            if let RemoteMsg::View(v) = protocol::remote_msg(data) { show_view(c, *v) }
        });
        if !mine() { return }
        if matches!(end, Err(StreamEnd::Status(401))) { pair_again(c); return }
        ui(c, |w| w.global::<S>().set_connected(false));
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn show_view(c: &C, v: RemoteView) {
    let now = v.now_showing();
    let live = v.live_item().map(str::to_string);
    let items: Vec<ItemRow> = v.items.iter().map(|it| ItemRow { title: s(it.title.clone()), count: s(it.count.clone()), live: live.as_deref() == Some(&it.id) }).collect();
    let sections: Vec<SectionChip> = v.sections.iter().map(|x| SectionChip { short: s(x.short.clone()), label: s(x.label.clone()), color: color(&x.color, slint::Color::from_rgb_u8(0x5d, 0x65, 0x76)), live: x.live }).collect();
    let next = v.next.clone();
    let service = v.service.clone();
    let media = v.media.clone();
    let logo = v.on_air.logo;
    let version = v.version;
    // A picture slide: its picture, asked for once per screen change.
    if let NowShowing::Slide { picture: true, .. } = now && c.picture_version.swap(version, Ordering::Relaxed) != version {
        let c2 = c.clone();
        spawn(move || {
            let Some(cl) = client(&c2) else { return };
            if let Ok(r) = cl.get(&format!("/remote/live-slide?w=960&v={version}")) && r.ok() && let Some(px) = decode_jpeg(&r.body) {
                ui(&c2, move |w| w.global::<S>().set_now_image(Image::from_rgba8(px)));
            }
        });
    }
    // The open sheet follows the live slide.
    let sheet = c.sheet_item.lock().unwrap().and_then(|i| v.items.get(i).cloned());
    let cursor = v.cursor.clone();
    *c.view.lock().unwrap() = Some(v);
    ui(c, move |w| {
        let st = w.global::<S>();
        let say = w.global::<Say>();
        st.set_connected(true);
        st.set_service(service.map_or_else(|| say.invoke_service(), s));
        let (label, text, pic, notes) = match now {
            NowShowing::Logo => (String::new(), say.invoke_logo().to_string(), false, String::new()),
            NowShowing::Clear => (String::new(), say.invoke_clear().to_string(), false, String::new()),
            NowShowing::Music => (String::new(), say.invoke_music().to_string(), false, String::new()),
            NowShowing::Slide { label, text, picture, notes } => (label, if picture { String::new() } else { text }, picture, notes),
        };
        st.set_now_label(s(label));
        st.set_now_text(s(text));
        st.set_now_picture(pic);
        st.set_notes(s(notes));
        st.set_sections(ModelRc::new(VecModel::from(sections)));
        match next {
            Some(n) => { st.set_next_label(s(format!("{} · {}", n.title, n.label))); st.set_next_text(s(n.text)); }
            None => { st.set_next_label(s("")); st.set_next_text(say.invoke_end_of_service()); }
        }
        st.set_logo_on(logo);
        st.set_has_media(media.is_some());
        st.set_media_paused(media.is_some_and(|m| m.paused));
        st.set_items(ModelRc::new(VecModel::from(items)));
        if let Some(it) = sheet {
            let rows: Vec<SlideRow> = it.slides.iter().enumerate().map(|(i, sl)| SlideRow {
                label: s(sl.label.clone()), text: s(sl.text.clone()),
                live: cursor.as_ref().is_some_and(|cu| cu.item_id == it.id && cu.slide == i),
            }).collect();
            st.set_sheet_slides(ModelRc::new(VecModel::from(rows)));
        }
    });
}

fn open_item(c: &C, i: usize) {
    let v = c.view.lock().unwrap().clone();
    let Some(v) = v else { return };
    let Some(it) = v.items.get(i).cloned() else { return };
    *c.sheet_item.lock().unwrap() = Some(i);
    let rows: Vec<SlideRow> = it.slides.iter().enumerate().map(|(n, sl)| SlideRow {
        label: s(sl.label.clone()), text: s(sl.text.clone()),
        live: v.cursor.as_ref().is_some_and(|cu| cu.item_id == it.id && cu.slide == n),
    }).collect();
    ui(c, move |w| {
        let st = w.global::<S>();
        st.set_sheet_title(s(it.title));
        st.set_sheet_slides(ModelRc::new(VecModel::from(rows)));
        st.set_sheet_open(true);
    });
}

/// A command to the computer; its answer is shown when it is a problem.
fn send(c: &C, env: serde_json::Value) {
    let Some(cl) = client(c) else { return };
    let c = c.clone();
    spawn(move || match cl.post_json("/remote/command", &env) {
        Ok(r) => match protocol::command_result(r.status, &r.json()) {
            CommandResult::Done => {}
            CommandResult::PairAgain => pair_again(&c),
            CommandResult::Stale => say(&c, |w| w.invoke_screen_changed(), |c, t| toast(c, t)),
            CommandResult::Error(e) if !e.is_empty() => toast(&c, e),
            CommandResult::Error(_) => {}
        },
        Err(_) => say(&c, |w| w.invoke_not_connected(), |c, t| toast(c, t)),
    });
}

// ---------- the tabs ----------

fn tab_chosen(c: &C, t: i32) {
    c.tab.store(t.max(0) as u64, Ordering::Relaxed);
    let turn = c.tab_turn.fetch_add(1, Ordering::Relaxed) + 1;
    keep_awake(c);
    ui(c, |w| w.global::<S>().set_stage_bar(true));
    let c = c.clone();
    match t {
        1 => spawn(move || stage_loop(&c, turn)),
        2 => spawn(move || tally_loop(&c, turn)),
        3 => spawn(move || gate_loop(&c, turn)),
        _ => {}
    }
}

/// The screen stays on while the stage screen or the tally light shows, and while the microphone or camera sends.
fn keep_awake(c: &C) {
    use slint::android::android_activity::WindowManagerFlags as F;
    let tab = c.tab.load(Ordering::Relaxed);
    let on = tab == 1 || tab == 2 || c.mic_on.lock().unwrap().is_some() || c.cam_on.lock().unwrap().is_some() || c.scan_on.lock().unwrap().is_some();
    if on { jni::app().set_window_flags(F::KEEP_SCREEN_ON, F::empty()) } else { jni::app().set_window_flags(F::empty(), F::KEEP_SCREEN_ON) }
}

/// Whether this phone may see the stage screen (and so the tally light and the microphone): "" yes, else why not.
fn stage_access(c: &C) -> (&'static str, String) {
    let Some(cl) = client(c) else { return ("lost", String::new()) };
    match cl.get("/stage-view/status") {
        Ok(r) if r.ok() => {
            let st: protocol::StageStatus = serde_json::from_slice(&r.body).unwrap_or_default();
            if !st.on { ("off", st.off_text) } else if st.allowed { ("", String::new()) } else { ("password", String::new()) }
        }
        _ => ("lost", String::new()),
    }
}

fn set_gate(c: &C, gate: &'static str, text: String) {
    ui(c, move |w| { let st = w.global::<S>(); st.set_gate(s(gate)); st.set_gate_text(s(text)); });
}

/// Waits `secs`, stopping early when the tab is left.
fn wait(c: &C, turn: u64, secs: f32) -> bool {
    let until = Instant::now() + Duration::from_secs_f32(secs);
    while Instant::now() < until {
        if c.tab_turn.load(Ordering::Relaxed) != turn { return false }
        std::thread::sleep(Duration::from_millis(100));
    }
    c.tab_turn.load(Ordering::Relaxed) == turn
}

fn stage_loop(c: &C, turn: u64) {
    let mine = || c.tab_turn.load(Ordering::Relaxed) == turn;
    while mine() {
        let (gate, text) = stage_access(c);
        set_gate(c, gate, text);
        if !gate.is_empty() { if !wait(c, turn, if gate == "password" { 60.0 } else { 3.0 }) { return } continue }
        let Some(cl) = client(c) else { return };
        let _ = cl.events("/stage-view/events", Duration::from_secs(25), &mine, |data| {
            if let Some(v) = protocol::stage_view(data) {
                fetch_stage_pictures(c, &v);
                *c.stage.lock().unwrap() = Some(v);
                draw_stage(c);
            }
        });
        if !wait(c, turn, 1.0) { return }
    }
}

/// The live and next slides' pictures the stage shows, asked for again when the screen changes.
fn fetch_stage_pictures(c: &C, v: &StageView) {
    for cell in v.cells.iter().filter(|x| x.kind == "picture") {
        let src = if cell.src == "live" { "live" } else { "next" }.to_string();
        let have = c.stage_pics.lock().unwrap().get(&src).map(|(ver, _)| *ver);
        if have == Some(v.version) { continue }
        let (c2, version) = (c.clone(), v.version);
        // Marked as asked for, so it is asked once.
        if let Some(e) = c.stage_pics.lock().unwrap().get_mut(&src) { e.0 = version }
        spawn(move || {
            let Some(cl) = client(&c2) else { return };
            if let Ok(r) = cl.get(&format!("/stage-view/{src}.jpg?w=640&v={version}")) && r.ok() && let Some(px) = decode_jpeg(&r.body) {
                c2.stage_pics.lock().unwrap().insert(src, (version, px));
                draw_stage(&c2);
            }
        });
    }
}

/// The stage view laid out for the tab's size (`fit`).
fn draw_stage(c: &C) {
    let (w, h) = *c.stage_size.lock().unwrap();
    let Some(v) = c.stage.lock().unwrap().clone() else { return };
    if w < 10.0 || h < 10.0 { return }
    let pics: HashMap<String, SharedPixelBuffer<Rgba8Pixel>> = c.stage_pics.lock().unwrap().iter().map(|(k, (_, p))| (k.clone(), p.clone())).collect();
    let cells = fit::place(&v.cells, w, h);
    let bg = v.background.clone();
    let (banner, follow) = (v.banner.clone().unwrap_or_default(), v.follow.clone().unwrap_or_default());
    ui(c, move |win| {
        let st = win.global::<S>();
        let rows = stage_cells(cells, &pics);
        st.set_stage_bg(color(&bg, slint::Color::from_rgb_u8(0, 0, 0)));
        st.set_cells(ModelRc::new(VecModel::from(rows)));
        st.set_banner(s(banner));
        st.set_follow(s(follow));
    });
}

fn stage_login(c: &C, password: String) {
    let Some(cl) = client(c) else { return };
    ui(c, |w| w.global::<S>().set_gate_error(s("")));
    let c = c.clone();
    spawn(move || match cl.post_json("/stage-view/login", &json!({ "password": password })) {
        Ok(r) if r.ok() => {
            let stage = cl.cookie("rp_stage");
            remember(&c, |sv| sv.stage = stage);
            let tab = c.tab.load(Ordering::Relaxed) as i32;
            tab_chosen(&c, tab);
        }
        Ok(r) => { let e = r.json()["error"].as_str().unwrap_or_default().to_string(); ui(&c, move |w| w.global::<S>().set_gate_error(s(e))) }
        Err(_) => ui(&c, |w| { let st = w.global::<S>(); st.set_gate_error(w.global::<Say>().invoke_not_connected()); }),
    });
}

fn tally_loop(c: &C, turn: u64) {
    loop {
        if c.tab_turn.load(Ordering::Relaxed) != turn { return }
        let Some(cl) = client(c) else { return };
        let camera = c.saved.lock().unwrap().tally_camera.clamp(1, 9);
        match cl.get(&format!("/stage-view/tally.json?camera={camera}")) {
            Ok(r) if r.ok() => {
                let t: protocol::Tally = serde_json::from_slice(&r.body).unwrap_or_default();
                if t.allowed {
                    ui(c, move |w| { let st = w.global::<S>(); st.set_gate(s("")); st.set_light(s(t.light)); st.set_light_text(s(t.text)); st.set_light_sub(s(t.sub)); });
                } else {
                    let (gate, text) = stage_access(c);
                    set_gate(c, if gate.is_empty() { "off" } else { gate }, text);
                    if !wait(c, turn, if gate == "password" { 60.0 } else { 3.0 }) { return }
                    continue;
                }
            }
            _ => ui(c, |w| { let st = w.global::<S>(); st.set_gate(s("")); st.set_light(s("off")); st.set_light_sub(w.global::<Say>().invoke_not_connected()); }),
        }
        if !wait(c, turn, 0.5) { return }
    }
}

/// The Microphone tab: keeps its stage-screen access shown (a password to type, or turned off).
fn gate_loop(c: &C, turn: u64) {
    loop {
        let (gate, text) = stage_access(c);
        set_gate(c, gate, text);
        if !wait(c, turn, if gate.is_empty() { 5.0 } else { 3.0 }) { return }
    }
}

// ---------- permissions ----------

/// Runs `then` once Android allows `permission`, after saying why (`why`); `denied` when it does not.
fn with_permission(c: &C, permission: &'static str, why: impl FnOnce(&Say) -> SharedString + Send + 'static,
    then: impl FnOnce(&C) + Send + 'static, denied: impl FnOnce(&C) + Send + 'static) {
    if jni::has_permission(permission) { then(c); return }
    *c.asking.lock().unwrap() = Some(Asking { permission, then: Box::new(then), denied: Box::new(denied), since: None, left: false });
    ui(c, move |w| { let t = why(&w.global::<Say>()); w.global::<S>().set_ask_why(t) });
}

fn ask_go(c: &C) {
    ui(c, |w| w.global::<S>().set_ask_why(s("")));
    let perm = {
        let mut a = c.asking.lock().unwrap();
        let Some(a) = a.as_mut() else { return };
        a.since = Some(Instant::now());
        a.permission
    };
    jni::request_permission(perm);
    // No question came up: Android said no by itself.
    let c2 = c.clone();
    spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        let silent = c2.asking.lock().unwrap().as_ref().is_some_and(|a| !a.left);
        if silent && let Some(a) = c2.asking.lock().unwrap().take() {
            if jni::has_permission(a.permission) { (a.then)(&c2) } else { (a.denied)(&c2) }
        }
    });
}

/// The app is back in front after Android's question: allowed or not.
fn permission_answered(c: &C) {
    let asked = c.asking.lock().unwrap().as_ref().is_some_and(|a| a.left);
    if !asked { return }
    let Some(a) = c.asking.lock().unwrap().take() else { return };
    let c = c.clone();
    spawn(move || if jni::has_permission(a.permission) { (a.then)(&c) } else { (a.denied)(&c) });
}

// ---------- Microphone ----------

fn mic_toggle(c: &C) {
    if c.mic_on.lock().unwrap().is_some() { stop_flag(&c.mic_on); keep_awake(c); ui(c, |w| { let st = w.global::<S>(); st.set_mic_on(false); st.set_mic_level(0.0); }); return }
    with_permission(c, jni::MICROPHONE, |w| w.invoke_why_microphone(), |c| {
        let c = c.clone();
        spawn(move || mic_run(&c));
    }, |c| ui(c, |w| { let st = w.global::<S>(); st.set_mic_note(w.global::<Say>().invoke_no_microphone()); }));
}

fn mic_run(c: &C) {
    let flag = Arc::new(AtomicBool::new(true));
    *c.mic_on.lock().unwrap() = Some(flag.clone());
    keep_awake(c);
    let m = match mic::Mic::open() {
        Ok(m) => m,
        Err(_) => {
            c.mic_on.lock().unwrap().take();
            keep_awake(c);
            ui(c, |w| { let st = w.global::<S>(); st.set_mic_on(false); st.set_mic_note(w.global::<Say>().invoke_microphone_failed()); });
            return;
        }
    };
    ui(c, |w| { let st = w.global::<S>(); st.set_mic_on(true); st.set_mic_note(w.global::<Say>().invoke_waiting()); });
    while flag.load(Ordering::Relaxed) {
        let Ok(samples) = m.read() else { break };
        if samples.is_empty() { continue }
        let level = protocol::peak(&samples);
        let Some(cl) = client(c) else { break };
        let path = format!("/stage-view/mic/sound?rate={}", m.rate);
        let answer = cl.post_bytes(&path, "application/octet-stream", &protocol::pcm_bytes(&samples));
        let note = answer.ok().map(|r| serde_json::from_slice::<protocol::MicAnswer>(&r.body).unwrap_or_default());
        ui(c, move |w| {
            let st = w.global::<S>();
            st.set_mic_level(level);
            match note {
                Some(a) => st.set_mic_note(s(a.text)),
                None => st.set_mic_note(w.global::<Say>().invoke_not_connected()),
            }
        });
    }
    drop(m);
    let mut cur = c.mic_on.lock().unwrap();
    if cur.as_ref().is_some_and(|f| Arc::ptr_eq(f, &flag)) { cur.take(); }
    drop(cur);
    keep_awake(c);
    ui(c, |w| { let st = w.global::<S>(); st.set_mic_on(false); st.set_mic_level(0.0); });
}

// ---------- Camera ----------

fn cam_toggle(c: &C, name: String) {
    if c.cam_on.lock().unwrap().is_some() { stop_flag(&c.cam_on); return }
    with_permission(c, jni::CAMERA, |w| w.invoke_why_camera(), move |c| cam_start(c, name),
        |c| ui(c, |w| { let st = w.global::<S>(); st.set_cam_note(w.global::<Say>().invoke_no_camera()); }));
}

fn cam_start(c: &C, typed: String) {
    let c2 = c.clone();
    say(c, |w| w.invoke_camera_name(), move |_, fallback| {
        let c = c2;
        let Some(cl) = client(&c) else { return };
        let info: protocol::CameraInfo = cl.get("/camera/info").ok().and_then(|r| serde_json::from_slice(&r.body).ok()).unwrap_or_default();
        if !info.paired { pair_again(&c); return }
        let name = protocol::camera_name(&typed, info.name.as_deref().unwrap_or(&fallback));
        remember(&c, |sv| sv.camera_name = name.clone());
        let shown = name.clone();
        ui(&c, move |w| w.global::<S>().set_cam_name(s(shown)));
        let want = if info.width > 0 { (info.width, info.height) } else { (1280, 720) };
        cam_run(&c, &cl, name, want, info.fps.clamp(1, 60));
    });
}

fn cam_run(c: &C, cl: &Client, name: String, want: (u32, u32), fps: u32) {
    let flag = Arc::new(AtomicBool::new(true));
    *c.cam_on.lock().unwrap() = Some(flag.clone());
    keep_awake(c);
    let back = c.saved.lock().unwrap().back_camera;
    let cam = match camera::Camera::open(back, want) {
        Ok(cam) => cam,
        Err(_) => {
            c.cam_on.lock().unwrap().take();
            keep_awake(c);
            ui(c, |w| { let st = w.global::<S>(); st.set_cam_on(false); st.set_cam_note(w.global::<Say>().invoke_camera_failed()); });
            return;
        }
    };
    ui(c, |w| { let st = w.global::<S>(); st.set_cam_on(true); st.set_cam_note(s("")); st.set_cam_light(s("")); st.set_cam_text(w.global::<Say>().invoke_waiting()); });
    let gap = Duration::from_secs_f32(1.0 / fps as f32);
    let path = protocol::camera_path(&name);
    let mut sock: Option<ws::Socket> = None;
    let mut ws_fails = 0;
    let (mut last_sent, mut last_preview, mut last_try) = (Instant::now() - gap, Instant::now(), Instant::now() - Duration::from_secs(5));
    let mut rotation_at = Instant::now() - Duration::from_secs(5);
    let mut display = 0;
    while flag.load(Ordering::Relaxed) && !cam.lost() {
        if sock.is_none() && ws_fails < 2 && last_try.elapsed() > Duration::from_secs(1) {
            last_try = Instant::now();
            match ws::Socket::connect(&cl.host, &path, cl.cookie_header().as_deref(), Duration::from_secs(4)) {
                Ok(s_) => { sock = Some(s_); ws_fails = 0; }
                Err(e) if e.to_string().contains("401") => { flag.store(false, Ordering::Relaxed); pair_again(c); break }
                Err(_) => { ws_fails += 1; ui(c, |w| { let st = w.global::<S>(); st.set_cam_text(w.global::<Say>().invoke_lost()); }); }
            }
        }
        if last_sent.elapsed() < gap { std::thread::sleep(Duration::from_millis(3)); continue }
        if rotation_at.elapsed() > Duration::from_secs(1) { display = jni::display_rotation(); rotation_at = Instant::now(); }
        let rot = crate::frames::rotation(cam.sensor, display, cam.front);
        let Some((nv21, w, h)) = cam.latest(|f| crate::frames::to_nv21(f, rot)) else { std::thread::sleep(Duration::from_millis(4)); continue };
        if last_preview.elapsed() > Duration::from_millis(120) {
            last_preview = Instant::now();
            let (px, pw, ph) = crate::frames::preview_rgba(&nv21, w, h, 360);
            ui(c, move |win| {
                let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&px, pw as u32, ph as u32);
                win.global::<S>().set_cam_preview(Image::from_rgba8(buf));
            });
        }
        let Some(jpeg) = jni::jpeg(&nv21, w, h, 70) else { continue };
        last_sent = Instant::now();
        // Paced by the computer's answers: the next picture only once this one arrived.
        let answer = if let Some(sk) = sock.as_mut() {
            match sk.send_binary(&jpeg).and_then(|_| sk.recv()) {
                Ok(ws::Message::Text(t)) => serde_json::from_str::<protocol::CameraLight>(&t).ok(),
                _ => { sock = None; None }
            }
        } else if ws_fails >= 2 {
            match cl.post_bytes(&format!("/camera/frame?name={}", crate::http::encode(&name)), "image/jpeg", &jpeg) {
                Ok(r) if r.status == 401 => { flag.store(false, Ordering::Relaxed); pair_again(c); break }
                Ok(r) => serde_json::from_slice::<protocol::CameraLight>(&r.body).ok(),
                Err(_) => None,
            }
        } else { None };
        match answer {
            Some(l) => ui(c, move |w| { let st = w.global::<S>(); st.set_cam_light(s(l.light)); st.set_cam_text(if l.text.is_empty() { w.global::<Say>().invoke_sending() } else { s(l.text) }); }),
            None => ui(c, |w| { let st = w.global::<S>(); st.set_cam_light(s("")); st.set_cam_text(w.global::<Say>().invoke_lost()); }),
        }
    }
    if let Some(mut sk) = sock { sk.close() }
    let lost = cam.lost();
    drop(cam);
    let mut cur = c.cam_on.lock().unwrap();
    if cur.as_ref().is_some_and(|f| Arc::ptr_eq(f, &flag)) { cur.take(); }
    drop(cur);
    keep_awake(c);
    ui(c, move |w| {
        let st = w.global::<S>();
        st.set_cam_on(false); st.set_cam_light(s("")); st.set_cam_text(s(""));
        if lost { st.set_cam_note(w.global::<Say>().invoke_camera_failed()) }
    });
}

// ---------- the QR code ----------

fn scan_qr(c: &C) {
    with_permission(c, jni::CAMERA, |w| w.invoke_why_scan(), |c| {
        let c = c.clone();
        spawn(move || scan_run(&c));
    }, |c| ui(c, |w| { let st = w.global::<S>(); st.set_find_note(w.global::<Say>().invoke_no_camera()); st.set_find_problem(true); }));
}

fn stop_scan(c: &C) { stop_flag(&c.scan_on); keep_awake(c); }

fn scan_run(c: &C) {
    let flag = Arc::new(AtomicBool::new(true));
    *c.scan_on.lock().unwrap() = Some(flag.clone());
    keep_awake(c);
    c.find_turn.fetch_add(1, Ordering::Relaxed);
    ui(c, |w| { let st = w.global::<S>(); st.set_scan_note(w.global::<Say>().invoke_point_at_qr()); st.set_page(s("scan")); });
    let Ok(cam) = camera::Camera::open(true, (1280, 720)) else {
        stop_scan(c);
        ui(c, |w| { let st = w.global::<S>(); st.set_page(s("find")); st.set_find_note(w.global::<Say>().invoke_camera_failed()); st.set_find_problem(true); });
        return;
    };
    let mut wrong_until = Instant::now();
    let mut display = jni::display_rotation();
    let mut n = 0u32;
    while flag.load(Ordering::Relaxed) && !cam.lost() {
        n += 1;
        if n.is_multiple_of(30) { display = jni::display_rotation() }
        let rot = crate::frames::rotation(cam.sensor, display, cam.front);
        let Some((nv21, w, h)) = cam.latest(|f| crate::frames::to_nv21(f, rot)) else { std::thread::sleep(Duration::from_millis(10)); continue };
        let (px, pw, ph) = crate::frames::preview_rgba(&nv21, w, h, 480);
        ui(c, move |win| win.global::<S>().set_scan_preview(Image::from_rgba8(SharedPixelBuffer::clone_from_slice(&px, pw as u32, ph as u32))));
        // The brightness alone, at most 800 wide, is enough to read the code.
        let step = w.div_ceil(800).max(1);
        let (gw, gh) = (w / step, h / step);
        let mut grid = rqrr::PreparedImage::prepare_from_greyscale(gw, gh, |x, y| nv21[(y * step) * w + x * step]);
        let text = grid.detect_grids().into_iter().find_map(|g| g.decode().ok().map(|(_, t)| t));
        if let Some(t) = text {
            match address::parse(&t).filter(|t| t.secret.is_some()) {
                Some(target) => {
                    stop_scan(c);
                    drop(cam);
                    connect(c, target);
                    return;
                }
                None if Instant::now() > wrong_until => {
                    wrong_until = Instant::now() + Duration::from_secs(3);
                    ui(c, |w| { let st = w.global::<S>(); st.set_scan_note(w.global::<Say>().invoke_not_a_pair_code()); });
                    let c2 = c.clone();
                    spawn(move || { std::thread::sleep(Duration::from_secs(3)); ui(&c2, |w| { let st = w.global::<S>(); st.set_scan_note(w.global::<Say>().invoke_point_at_qr()); }) });
                }
                None => {}
            }
        }
    }
    stop_scan(c);
}
