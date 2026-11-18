//! The Media tab's Download from YouTube panel: a link box (filled from the clipboard when it holds a YouTube
//! link), the video's title, length and picture as soon as the link is in, and one Download button. The download
//! then runs as a job card with Cancel (`youtube::start_download`) and the dialog closes. Ctrl+V with a YouTube link
//! anywhere in the operator opens the panel with it (`open_with_link`).
use super::{AddTab, DialogKind, add::AddCtx};
use crate::{
    ui::{AddState, operator::window::Operator},
    youtube::{self, Found, Quality, Video, strings as tr},
};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel};
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}, sync::{Arc, Mutex}, time::Duration};

/// How often the panel shows how far getting the helpers ready has got (it is reported off the Slint thread).
const SETUP_EVERY: Duration = Duration::from_millis(150);

/// The open dialog's panel: the video found, the link it was found for, and the lookup in flight.
#[derive(Default)]
pub(crate) struct Panel {
    video: RefCell<Option<Video>>,
    /// The cleaned link of the last lookup, so typing or pasting the same link again does nothing.
    looked: RefCell<String>,
    /// Numbers lookups; an answer for an older one is dropped.
    seq: Cell<u32>,
    setup: Arc<Mutex<Option<youtube::Setup>>>,
    timer: Timer,
}

/// Every field as a fresh dialog has it; "Add to service when done" is ticked, as the dialog opens from the service.
pub(crate) fn reset(st: &AddState) {
    st.set_yt_open(false);
    st.set_yt_link("".into());
    st.set_yt_busy(false);
    st.set_yt_status("".into());
    st.set_yt_error(false);
    st.set_yt_found(false);
    st.set_yt_title("".into());
    st.set_yt_detail("".into());
    st.set_yt_note("".into());
    st.set_yt_thumb_shown(false);
    st.set_yt_thumb(Image::default());
    let qualities: Vec<SharedString> = tr::qualities().into_iter().map(SharedString::from).collect();
    st.set_yt_qualities(ModelRc::new(VecModel::from(qualities)));
    st.set_yt_quality(0);
    st.set_yt_add(true);
}

/// Download from YouTube: opens the panel. A YouTube link on the clipboard is filled in and looked up at once.
pub(crate) fn show(op: &Rc<Operator>, ctx: &Rc<AddCtx>, link: Option<String>) {
    let st = op.ui.global::<AddState>();
    st.set_yt_open(true);
    let text = link.or_else(|| super::settings::paste_text().filter(|t| youtube::link(t).is_some()));
    if let Some(text) = text.filter(|_| st.get_yt_link().trim().is_empty()) {
        st.set_yt_link(text.trim().into());
        look_up(op, ctx, false);
    }
}

/// Ctrl+V with a YouTube link outside a text field: the Add dialog on Video & media with the panel looking it up.
pub(crate) fn open_with_link(op: &Rc<Operator>, link: &str) {
    let ctx = super::add::open_add_ctx(op, AddTab::Media);
    show(op, &ctx, Some(link.to_string()));
}

/// The clipboard holds a YouTube link (Ctrl+V outside a text field).
pub(crate) fn pasted_link() -> Option<String> {
    super::settings::paste_text().filter(|t| youtube::link(t).is_some())
}

/// The link box changed: a whole YouTube link is looked up straight away; anything else waits for Enter.
fn edited(op: &Rc<Operator>, ctx: &Rc<AddCtx>, text: &str) {
    if youtube::link(text).is_some() { look_up(op, ctx, false) }
}

/// Looks the link up: title, length and picture, or what is wrong, in plain words. `again`: Enter, which looks
/// up even the same link (after an error, say).
fn look_up(op: &Rc<Operator>, ctx: &Rc<AddCtx>, again: bool) {
    let st = op.ui.global::<AddState>();
    let text = st.get_yt_link().to_string();
    let Some(link) = youtube::link(&text) else {
        if !text.trim().is_empty() { show_error(&st, &tr::not_a_link()) }
        return;
    };
    let p = &ctx.yt;
    if !again && *p.looked.borrow() == link.url { return }
    *p.looked.borrow_mut() = link.url.clone();
    p.seq.set(p.seq.get().wrapping_add(1));
    let seq = p.seq.get();
    *p.video.borrow_mut() = None;
    st.set_yt_found(false);
    st.set_yt_error(false);
    st.set_yt_busy(true);
    st.set_yt_status(tr::looking_up().into());
    *p.setup.lock().unwrap() = None;
    let (shown, me) = (p.setup.clone(), Rc::downgrade(op));
    p.timer.start(TimerMode::Repeated, SETUP_EVERY, move || {
        let (Some(op), Some(s)) = (me.upgrade(), *shown.lock().unwrap()) else { return };
        op.ui.global::<AddState>().set_yt_status(s.text().into());
    });
    let setup = p.setup.clone();
    let (me, weak_ctx): (Weak<Operator>, Weak<AddCtx>) = (Rc::downgrade(op), Rc::downgrade(ctx));
    let quality = Quality::from_index(st.get_yt_quality());
    op.ops().youtube_look_up(link, quality, move |s| *setup.lock().unwrap() = Some(s), move |r| {
        let (Some(op), Some(ctx)) = (me.upgrade(), weak_ctx.upgrade()) else { return };
        // A newer lookup, or the dialog was closed or moved on.
        if ctx.yt.seq.get() != seq || op.dialog() != Some(DialogKind::Add(AddTab::Media)) { return }
        ctx.yt.timer.stop();
        let st = op.ui.global::<AddState>();
        st.set_yt_busy(false);
        match r {
            Ok(found) => show_found(&st, &ctx, found),
            Err(e) => {
                // Enter on the same link tries again.
                ctx.yt.looked.borrow_mut().clear();
                show_error(&st, &e);
            }
        }
    });
}

fn show_error(st: &AddState, text: &str) {
    st.set_yt_found(false);
    st.set_yt_error(true);
    st.set_yt_status(text.into());
}

fn show_found(st: &AddState, ctx: &AddCtx, found: Found) {
    let Found { video, picture } = found;
    st.set_yt_status("".into());
    st.set_yt_error(false);
    st.set_yt_title(video.title.as_str().into());
    st.set_yt_detail(tr::detail(video.duration, &video.channel).into());
    st.set_yt_note(video.playlist.as_ref().map(|(_, n)| tr::playlist(*n)).unwrap_or_default().into());
    match picture {
        Some(p) => {
            let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(p.as_raw(), p.width(), p.height());
            st.set_yt_thumb(Image::from_rgba8(buf));
            st.set_yt_thumb_shown(true);
        }
        None => st.set_yt_thumb_shown(false),
    }
    *ctx.yt.video.borrow_mut() = Some(video);
    st.set_yt_found(true);
}

/// Download: starts the job card, says so, and closes the dialog so the card is in view.
fn download(op: &Rc<Operator>, ctx: &Rc<AddCtx>) {
    let Some(video) = ctx.yt.video.borrow().clone() else { return };
    let st = op.ui.global::<AddState>();
    let quality = Quality::from_index(st.get_yt_quality());
    let service = st.get_yt_add().then(|| op.state.borrow().snap.as_ref().and_then(|s| s.data.service().map(|s| s.id))).flatten();
    let title = youtube::library_name(&video.title);
    let me = Rc::downgrade(op);
    op.ops().youtube_download(video, quality, service, move |_| {
        let Some(op) = me.upgrade() else { return };
        op.toast(tr::started(&title), false);
        op.close_dialog();
    });
}

/// The panel's callbacks on `AddState`.
pub(crate) fn wire(op: &Rc<Operator>, ctx: &Rc<AddCtx>) {
    let st = op.ui.global::<AddState>();
    let c = ctx.clone();
    st.on_yt_show(move || if let Some(op) = c.op.upgrade() { show(&op, &c, None) });
    let c = ctx.clone();
    st.on_yt_link_edited(move |t| if let Some(op) = c.op.upgrade() { edited(&op, &c, &t) });
    let c = ctx.clone();
    st.on_yt_look_up(move || if let Some(op) = c.op.upgrade() { look_up(&op, &c, true) });
    let c = ctx.clone();
    st.on_yt_download(move || if let Some(op) = c.op.upgrade() { download(&op, &c) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::{App, test_app},
        ui::{
            dialogs::add::{open_add, tests::*},
            operator::window::testing::find,
        },
        youtube::testing_fake::Fake,
    };
    use std::{path::PathBuf, sync::Arc};

    const JSON: &str = r#"{"id":"aqz-KE-bpKQ","title":"Big Buck Bunny","channel":"Blender","duration":634,"live_status":"not_live","requested_formats":[{"format_id":"137","filesize":9},{"format_id":"140","filesize":1}]}"#;

    /// An app whose yt-dlp is the fake, answering lookups with `JSON` (or `info` steps).
    fn faked(info: Option<&str>) -> (Arc<App>, PathBuf) {
        let app = test_app();
        let dir = std::env::temp_dir().join(format!("rp-ytui-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let json = dir.join("info.json");
        std::fs::write(&json, JSON).unwrap();
        let log = dir.join("log");
        let info = info.map(str::to_string).unwrap_or_else(|| format!("out:{}", json.display()));
        *app.youtube.fake.lock().unwrap() = Some(Fake::new(&info, "hang", &log));
        (app, log)
    }

    fn runs(log: &std::path::Path) -> usize { std::fs::read_to_string(log).map_or(0, |t| t.lines().count()) }

    fn media_tab(app: &Arc<App>) -> Rc<Operator> {
        let op = opened(app);
        open_add(&op, AddTab::Media);
        settle(&op);
        op
    }

    #[test]
    fn the_button_opens_the_panel_and_takes_a_link_from_the_clipboard() {
        let (app, log) = faked(None);
        let op = media_tab(&app);
        let st = op.ui.global::<AddState>();
        assert!(!st.get_yt_open(), "closed until asked for");
        assert!(find(&op, "AddMediaTab::youtube").is_some());
        super::super::settings::testing::set_clipboard(Some("https://youtu.be/aqz-KE-bpKQ?si=x"));
        click(&op, "AddMediaTab::youtube");
        assert!(st.get_yt_open());
        assert_eq!(st.get_yt_link(), "https://youtu.be/aqz-KE-bpKQ?si=x");
        // Looked up at once: the title and the length with the channel.
        assert!(st.get_yt_found(), "status: {}", st.get_yt_status());
        assert_eq!((st.get_yt_title().as_str(), st.get_yt_detail().as_str()), ("Big Buck Bunny", "10:34 · Blender"));
        assert_eq!(st.get_yt_status(), "");
        assert!(st.get_yt_add(), "Add to service is ticked");
        assert_eq!(st.get_yt_quality(), 0);
        assert_eq!(runs(&log), 1);
        // The notice sits right under the button.
        let notice = find(&op, "YouTubePanel::notice").unwrap();
        assert_eq!(notice.accessible_label().unwrap_or_default(), "Only download videos your church has the right to use (your own, licensed, or Creative Commons).");
        super::super::settings::testing::set_clipboard(None);
    }

    #[test]
    fn a_clipboard_without_a_link_leaves_the_box_empty() {
        let (app, log) = faked(None);
        let op = media_tab(&app);
        super::super::settings::testing::set_clipboard(Some("hello"));
        click(&op, "AddMediaTab::youtube");
        let st = op.ui.global::<AddState>();
        assert_eq!(st.get_yt_link(), "");
        assert!(!st.get_yt_found());
        assert_eq!(runs(&log), 0);
        // Typing a link looks it up when it is whole; the same link again does nothing new.
        type_text(&op, "https://www.youtube.com/watch?v=aqz-KE-bp");
        settle(&op);
        assert_eq!(runs(&log), 0, "not a whole link yet");
        type_text(&op, "KQ");
        settle(&op);
        assert!(st.get_yt_found());
        assert_eq!(runs(&log), 1);
        st.invoke_yt_link_edited(st.get_yt_link());
        assert_eq!(runs(&log), 1);
        super::super::settings::testing::set_clipboard(None);
    }

    #[test]
    fn not_a_link_and_errors_are_plain() {
        let dir = std::env::temp_dir().join(format!("rp-ytui-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = dir.join("err.txt");
        std::fs::write(&err, "ERROR: [youtube] aqz-KE-bpKQ: Private video. Sign in if you've been granted access\n").unwrap();
        let (app, log) = faked(Some(&format!("err:{},exit:1", err.display())));
        let op = media_tab(&app);
        click(&op, "AddMediaTab::youtube");
        let st = op.ui.global::<AddState>();
        type_text(&op, "vimeo.com/123");
        press(&op, char::from(slint::platform::Key::Return));
        settle(&op);
        assert!(st.get_yt_error());
        assert_eq!(st.get_yt_status(), tr::not_a_link());
        assert_eq!(runs(&log), 0);
        st.set_yt_link("https://youtu.be/aqz-KE-bpKQ".into());
        st.invoke_yt_look_up();
        assert!(st.get_yt_error() && !st.get_yt_found());
        assert_eq!(st.get_yt_status(), "This video is private, so only its owner can download it.");
        // Enter tries the same link again.
        st.invoke_yt_look_up();
        assert_eq!(runs(&log), 2);
        assert!(find(&op, "YouTubePanel::download").is_none(), "nothing to download");
    }

    #[test]
    fn download_starts_a_job_card_and_closes_the_dialog() {
        let (app, _) = faked(None);
        let op = media_tab(&app);
        let st = op.ui.global::<AddState>();
        click(&op, "AddMediaTab::youtube");
        st.set_yt_link("https://youtu.be/aqz-KE-bpKQ".into());
        st.invoke_yt_look_up();
        settle(&op);
        st.set_yt_quality(1);
        click(&op, "YouTubePanel::download");
        assert_eq!(op.dialog(), None);
        assert_eq!(last_toast(&op), Some(("Downloading “Big Buck Bunny”. It will be in Video & media when it's done.".into(), false)));
        let jobs = app.jobs.lock().unwrap().clone();
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].cancellable);
        assert_eq!(jobs[0].label, "Downloading “Big Buck Bunny”");
        app.job_cancel(jobs[0].id);
    }

    #[test]
    fn ctrl_v_with_a_youtube_link_opens_the_panel() {
        let (app, log) = faked(None);
        let op = opened(&app);
        super::super::settings::testing::set_clipboard(Some("https://www.youtube.com/watch?v=aqz-KE-bpKQ"));
        op.window().dispatch_event(slint::platform::WindowEvent::KeyPressed { text: char::from(slint::platform::Key::Control).into() });
        press(&op, "v");
        op.window().dispatch_event(slint::platform::WindowEvent::KeyReleased { text: char::from(slint::platform::Key::Control).into() });
        settle(&op);
        assert_eq!(op.dialog(), Some(DialogKind::Add(AddTab::Media)));
        let st = op.ui.global::<AddState>();
        assert!(st.get_yt_open() && st.get_yt_found());
        assert_eq!(runs(&log), 1);
        op.close_dialog();
        // Anything else on the clipboard: Ctrl+V does nothing.
        super::super::settings::testing::set_clipboard(Some("just words"));
        op.window().dispatch_event(slint::platform::WindowEvent::KeyPressed { text: char::from(slint::platform::Key::Control).into() });
        press(&op, "v");
        op.window().dispatch_event(slint::platform::WindowEvent::KeyReleased { text: char::from(slint::platform::Key::Control).into() });
        assert_eq!(op.dialog(), None);
        super::super::settings::testing::set_clipboard(None);
    }
}
