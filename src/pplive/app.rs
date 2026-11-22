//! The App's side of "Run in PowerPoint": which show the live item asks for, Next/Back as PowerPoint clicks, and a
//! thread that keeps the show in line with the live engine and notices what PowerPoint does by itself.
//! Lock order: the driver first, then `data` → `live`. Nothing takes the driver while holding `data` or `live`.
use super::{After, Driver, Rect, SlideShowHost, Want};
use crate::{app::{App, DataPlan}, live::{Change, Command, Envelope, Outcome}, model::{Data, ItemKind}};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc};
use std::time::Duration;
use uuid::Uuid;

/// How often the thread looks at PowerPoint when nothing woke it.
const TICK: Duration = Duration::from_millis(500);

/// Held by the App. Empty unless `App::set_show_host` was called (Windows with PowerPoint, or a test).
#[derive(Default)]
pub struct Hub {
    driver: Mutex<Option<Driver>>,
    wake: Mutex<Option<mpsc::Sender<()>>>,
    /// Clear All: end the show on the next tick.
    close: AtomicBool,
}

impl Hub {
    fn wake(&self) {
        if let Some(tx) = self.wake.lock().unwrap().as_ref() { let _ = tx.send(()); }
    }
}

impl App {
    /// Lets decks run in PowerPoint through `host`, and starts the thread that keeps the show in line.
    pub fn set_show_host(self: &Arc<Self>, host: Box<dyn SlideShowHost>) {
        self.install_show_host(host);
        let (tx, rx) = mpsc::channel();
        *self.pplive.wake.lock().unwrap() = Some(tx);
        let me = Arc::downgrade(self);
        let spawned = std::thread::Builder::new().name("powerpoint-show".into()).spawn(move || loop {
            match rx.recv_timeout(TICK) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            let Some(app) = me.upgrade() else { return };
            app.show_tick();
        });
        if let Err(e) = spawned { eprintln!("The PowerPoint show thread could not start: {e}") }
    }

    /// The driver without its thread: tests call `show_tick` themselves.
    pub(crate) fn install_show_host(&self, host: Box<dyn SlideShowHost>) {
        *self.pplive.driver.lock().unwrap() = Some(Driver::new(host));
    }

    /// Whether this computer can run decks in PowerPoint.
    pub fn can_run_powerpoint(&self) -> bool { self.pplive.driver.lock().unwrap().is_some() }

    /// What PowerPoint should show for the live item: `None` unless it is a deck set to run in PowerPoint.
    pub(crate) fn show_want(&self) -> Option<Want> {
        let data = self.data.lock().unwrap();
        let live = self.live.lock().unwrap();
        let cursor = live.cursor.as_ref()?;
        let item = data.service()?.items.iter().find(|i| i.id == cursor.item_id)?;
        let ItemKind::Deck { deck_id } = item.kind else { return None };
        let deck = data.decks.iter().find(|d| d.id == deck_id)?;
        let file = deck.runs_in_powerpoint()?;
        Some(Want {
            item_id: item.id, title: deck.title.clone(), file: self.store.asset_path(file), slide: cursor.slide,
            black: live.frame.slide.is_none(), covered: live.frame.logo || !live.frame.overlays.is_empty(), screen: self.room_screen(&data),
        })
    }

    /// The room monitor, when the audience screen is on and set to one.
    fn room_screen(&self, data: &Data) -> Option<Rect> {
        let screens = &data.settings.screens;
        if !screens.audience.on { return None }
        let id = screens.monitors("audience").first()?.to_string();
        self.monitors().into_iter().find(|m| m.id == id).map(|m| Rect { x: m.x, y: m.y, width: m.width, height: m.height })
    }

    /// Next/Back while a show runs: a click in PowerPoint (an animation, or the next slide, which the live slide
    /// follows). `None`: the live engine handles the command as always (no show, a cleared slide, a stale or
    /// missing base, or past either end of the show, where it takes the next or previous item).
    pub(crate) fn show_command(&self, env: &Envelope) -> Option<Outcome> {
        let forward = match env.cmd {
            Command::Next => true,
            Command::Prev => false,
            Command::Show { item_id, .. } => {
                // A click on a slide of a show PowerPoint ended starts it again.
                if let Some(d) = self.pplive.driver.lock().unwrap().as_mut() { d.retry(item_id) }
                return None;
            }
            _ => return None,
        };
        let mut guard = self.pplive.driver.lock().unwrap();
        let driver = guard.as_mut()?;
        let item_id = {
            let live = self.live.lock().unwrap();
            if let Some(o) = live.seen_outcome(&env.id) { return Some(o) }
            if env.base != Some(live.version) || live.frame.slide.is_none() { return None }
            let item_id = live.cursor.as_ref()?.item_id;
            if !driver.is_open_for(item_id) { return None }
            item_id
        };
        let after = driver.step(forward);
        let out = match after {
            After::Beyond => return None,
            After::Ended(_) | After::Failed(_) => { self.show_after(item_id, after); return None }
            After::Nothing => {
                // An animation played: the room is PowerPoint's, but a Next sent on the old state is stale now.
                let mut live = self.live.lock().unwrap();
                live.commit_change(Change::Silent);
                let out = Outcome { ok: true, version: live.version, error: None, duplicate: false };
                live.note_seen(env.id.clone(), out.clone());
                out
            }
            After::Slide(n) => {
                let data = self.data.lock().unwrap();
                let mut live = self.live.lock().unwrap();
                let shown = live.apply(Envelope::new(Command::Show { item_id, slide: n }), &DataPlan(&data));
                live.note_seen(env.id.clone(), shown.clone());
                shown
            }
        };
        drop(guard);
        self.broadcast_live();
        self.automation_after(None);
        Some(out)
    }

    /// After a command the engine applied: the thread brings PowerPoint in line at once.
    pub(crate) fn show_after_command(&self, cmd: &Command) {
        if matches!(cmd, Command::ClearAll) { self.pplive.close.store(true, Ordering::SeqCst) }
        self.pplive.wake();
    }

    /// One look: end the show after Clear All, bring it in line with the live item, and follow PowerPoint.
    pub(crate) fn show_tick(&self) {
        let mut guard = self.pplive.driver.lock().unwrap();
        let Some(driver) = guard.as_mut() else { return };
        if self.pplive.close.swap(false, Ordering::SeqCst) { driver.close() }
        let want = self.show_want();
        let Some(item_id) = want.as_ref().map(|w| w.item_id) else { driver.apply(None); return };
        let after = driver.apply(want.as_ref());
        let after = if after == After::Nothing { driver.poll() } else { after };
        drop(guard);
        self.show_after(item_id, after);
    }

    /// Acts on what the driver said: the live slide follows PowerPoint, and a failure is told to the operator.
    fn show_after(&self, item_id: Uuid, after: After) {
        match after {
            After::Nothing | After::Beyond => {}
            After::Slide(n) => {
                let out = {
                    let data = self.data.lock().unwrap();
                    let mut live = self.live.lock().unwrap();
                    // Only while that item is still live, and only to another slide.
                    if live.cursor.as_ref().is_none_or(|c| c.item_id != item_id || c.slide == n) { return }
                    live.apply(Envelope::new(Command::Show { item_id, slide: n }), &DataPlan(&data))
                };
                if out.ok {
                    self.broadcast_live();
                    self.automation_after(None);
                }
            }
            After::Ended(title) => self.set_notice(&super::strings::ended(&title)),
            After::Failed(why) => self.set_notice(&why),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake::FakeHost;
    use crate::{app::{App, DataPlan, Monitor, Screens, test_app}, live::{Command, Envelope, Plan}, model::{Deck, DeckShow, ItemKind, ScreenTarget, Service, ServiceItem}};
    use std::sync::Arc;
    use uuid::Uuid;

    struct Desk;
    impl Screens for Desk {
        fn open(&self, _: &str, _: Option<&str>) -> Result<(), String> { Ok(()) }
        fn close(&self, _: &str) {}
        fn open_roles(&self) -> Vec<String> { vec![] }
        fn monitors(&self) -> Vec<Monitor> {
            let m = |id: &str, x, primary| Monitor { id: id.into(), label: id.into(), x, y: 0, width: 1920, height: 1080, primary };
            vec![m("LAPTOP", 0, true), m("PROJECTOR", 1920, false)]
        }
    }

    /// A service with a 3-slide deck that runs in PowerPoint, then a text item; the room on the projector.
    fn setup(clicks: &[usize]) -> (Arc<App>, FakeHost, Uuid, Uuid) {
        let app = test_app();
        app.set_screens(Arc::new(Desk));
        let deck = Deck {
            id: Uuid::new_v4(), title: "Talk".into(), slides: vec!["a.png".into(), "b.png".into(), "c.png".into()], notes: vec![], transitions: vec![],
            advance_ms: vec![Some(5000)], show: Some(DeckShow { file: "talk.pptx".into(), run: true }),
        };
        let (deck_item, text_item) = (Uuid::new_v4(), Uuid::new_v4());
        app.edit(|d| {
            d.decks.push(deck.clone());
            d.settings.screens.assign("audience", ScreenTarget::Monitor("PROJECTOR".into()));
            d.settings.screens.audience.on = true;
            if d.service().is_none() { d.services.push(Service::new("Sunday")) }
            let s = d.service_mut().unwrap();
            let item = |id, kind| ServiceItem { id, kind, background: None, notes: String::new(), theme: None };
            s.items.push(item(deck_item, ItemKind::Deck { deck_id: deck.id }));
            s.items.push(item(text_item, ItemKind::Text { title: "After".into(), body: "Amen".into(), layout: Default::default() }));
            Ok(())
        }).unwrap();
        let host = FakeHost::with_clicks(clicks);
        app.install_show_host(Box::new(host.clone()));
        (app, host, deck_item, text_item)
    }

    fn send(app: &App, cmd: Command) -> crate::live::Outcome {
        let base = Some(app.live.lock().unwrap().version);
        app.command(Envelope { id: Uuid::new_v4().to_string(), base, cmd })
    }

    fn slide(app: &App) -> Option<(Uuid, usize)> { app.live.lock().unwrap().cursor.as_ref().map(|c| (c.item_id, c.slide)) }

    #[test]
    fn a_deck_that_runs_in_powerpoint_is_wanted_on_the_room_monitor() {
        let (app, _, deck_item, text_item) = setup(&[0, 0, 0]);
        assert!(app.can_run_powerpoint());
        assert!(app.show_want().is_none(), "nothing live");
        assert!(send(&app, Command::Show { item_id: deck_item, slide: 1 }).ok);
        let w = app.show_want().unwrap();
        assert_eq!((w.item_id, w.slide, w.black, w.title.as_str()), (deck_item, 1, false, "Talk"));
        assert_eq!(w.file, app.store.asset_path("talk.pptx"));
        assert_eq!(w.screen, Some(super::Rect { x: 1920, y: 0, width: 1920, height: 1080 }));
        assert!(send(&app, Command::ClearSlide).ok);
        assert!(app.show_want().unwrap().black);
        assert!(send(&app, Command::Show { item_id: text_item, slide: 0 }).ok);
        assert!(app.show_want().is_none());
        // The room off: no monitor to run on.
        assert!(send(&app, Command::Show { item_id: deck_item, slide: 0 }).ok);
        app.edit(|d| { d.settings.screens.audience.on = false; Ok(()) }).unwrap();
        assert_eq!(app.show_want().unwrap().screen, None);
        // Shown as pictures: PowerPoint is not asked for.
        app.edit(|d| { d.decks[0].show.as_mut().unwrap().run = false; Ok(()) }).unwrap();
        assert!(app.show_want().is_none());
    }

    #[test]
    fn its_slides_say_where_they_are_and_leave_timings_to_powerpoint() {
        let (app, _, deck_item, _) = setup(&[0, 0, 0]);
        let live = DataPlan(&app.data.lock().unwrap()).build(deck_item).unwrap();
        assert_eq!(live.slides[1].label, "Slide 2 of 3");
        assert_eq!(live.slides[0].advance_ms, None, "PowerPoint's own timings run the show");
    }

    #[test]
    fn next_and_back_click_through_powerpoint_and_the_live_slide_follows() {
        let (app, host, deck_item, text_item) = setup(&[1, 0, 0]);
        assert!(send(&app, Command::Show { item_id: deck_item, slide: 0 }).ok);
        app.show_tick();
        assert_eq!(host.take_log(), ["open ".to_string() + &app.store.asset_path("talk.pptx").display().to_string() + " 1920,0 1920x1080", "pos".into()]);
        let v = app.live.lock().unwrap().version;
        // The build on slide 1: PowerPoint clicks, the live slide stays, the version moves (stale Nexts stay stale).
        let out = send(&app, Command::Next);
        assert!(out.ok && out.version > v);
        assert_eq!(slide(&app), Some((deck_item, 0)));
        assert!(!app.command(Envelope { id: "old".into(), base: Some(v), cmd: Command::Next }).ok, "a Next sent before the click is stale");
        // Slide 2: the live slide follows.
        assert!(send(&app, Command::Next).ok);
        assert_eq!(slide(&app), Some((deck_item, 1)));
        // The same command delivered twice clicks once.
        let base = Some(app.live.lock().unwrap().version);
        let env = Envelope { id: "twice".into(), base, cmd: Command::Next };
        assert!(app.command(env.clone()).ok);
        assert!(app.command(env).duplicate);
        assert_eq!(slide(&app), Some((deck_item, 2)));
        assert_eq!(host.take_log(), ["pos", "next", "pos", "next", "pos", "next"]);
        app.show_tick();
        assert_eq!(host.take_log(), ["pos"], "already there: no goto");
        // The end of the show: Next takes the next item, and the show closes.
        assert!(send(&app, Command::Next).ok);
        assert_eq!(slide(&app), Some((text_item, 0)));
        app.show_tick();
        assert_eq!(host.take_log(), ["pos", "close"]);
        // Back into the deck lands on its last slide.
        assert!(send(&app, Command::Prev).ok);
        assert_eq!(slide(&app), Some((deck_item, 2)));
        app.show_tick();
        assert_eq!(host.take_log()[1..], ["goto 3".to_string(), "pos".into()]);
        // Back: slide 2, then slide 1 with its build played, then that build undone.
        assert!(send(&app, Command::Prev).ok);
        assert_eq!(slide(&app), Some((deck_item, 1)));
        assert!(send(&app, Command::Prev).ok);
        assert_eq!(slide(&app), Some((deck_item, 0)));
        assert!(send(&app, Command::Prev).ok);
        assert_eq!(slide(&app), Some((deck_item, 0)));
        assert_eq!(host.0.lock().unwrap().click, 0);
    }

    #[test]
    fn the_logo_is_seen_over_a_powerpoint_show() {
        let (app, host, deck_item, _) = setup(&[0, 0, 0]);
        assert!(send(&app, Command::Show { item_id: deck_item, slide: 0 }).ok);
        app.show_tick();
        host.take_log();
        // Logo: PowerPoint's window steps behind the room window, which shows the logo (it stayed in front, and
        // the room saw PowerPoint's black screen).
        assert!(send(&app, Command::Logo).ok);
        assert!(app.show_want().unwrap().covered);
        app.show_tick();
        assert_eq!(host.take_log(), ["cover 1", "black 1", "pos"]);
        // A slide of the deck again: the show is back in front.
        assert!(send(&app, Command::Show { item_id: deck_item, slide: 1 }).ok);
        assert!(!app.show_want().unwrap().covered);
        app.show_tick();
        assert_eq!(host.take_log(), ["cover 0", "black 0", "goto 2", "pos"]);
    }

    #[test]
    fn clears_black_out_powerpoint_and_clear_all_ends_the_show() {
        let (app, host, deck_item, _) = setup(&[0, 0, 0]);
        assert!(send(&app, Command::Show { item_id: deck_item, slide: 0 }).ok);
        app.show_tick();
        host.take_log();
        assert!(send(&app, Command::ClearSlide).ok);
        app.show_tick();
        assert_eq!(host.take_log(), ["black 1", "pos"]);
        // Next on a cleared slide is the engine's: it shows slide 2, and PowerPoint follows.
        assert!(send(&app, Command::Next).ok);
        app.show_tick();
        assert_eq!(host.take_log(), ["black 0", "goto 2", "pos"]);
        assert!(send(&app, Command::ClearAll).ok);
        app.show_tick();
        assert_eq!(host.take_log(), ["close"]);
    }

    #[test]
    fn powerpoint_moving_on_or_ending_is_followed() {
        let (app, host, deck_item, _) = setup(&[0, 0, 0]);
        assert!(send(&app, Command::Show { item_id: deck_item, slide: 0 }).ok);
        app.show_tick();
        host.set_slide(3);
        app.show_tick();
        assert_eq!(slide(&app), Some((deck_item, 2)));
        host.end();
        app.show_tick();
        let notice = app.live.lock().unwrap().notice.clone().unwrap();
        assert!(notice.contains("PowerPoint ended the slide show of “Talk”"), "{notice}");
        // Next now walks the pictures, which the room shows.
        host.take_log();
        assert!(send(&app, Command::Prev).ok);
        assert_eq!(slide(&app), Some((deck_item, 1)));
        app.show_tick();
        assert!(host.take_log().is_empty(), "not started again by itself");
        // A click on one of its slides starts it again.
        assert!(send(&app, Command::Show { item_id: deck_item, slide: 0 }).ok);
        app.show_tick();
        assert!(host.take_log()[0].starts_with("open "));
    }
}
