//! 1.2 "Run in PowerPoint": a deck set to run in PowerPoint plays as PowerPoint's own slide show, full screen on
//! the room monitor, so every animation, transition and video plays as it does there. RustyPresenter stays in
//! charge: Next/Back, the phone, the clears and the service order drive the show through a `SlideShowHost`.
//! The deck's slide pictures stay the live item, so the operator, stage and phone show them, and the room window
//! (behind PowerPoint's) shows them if PowerPoint cannot run.
//!
//! PowerPoint's window covers the room window, and with it the logo and the overlays (a message, a timer, a logo
//! bug), which only the room window draws. So while the logo is on or an overlay is up, PowerPoint's window
//! steps behind the room window (`SlideShowHost::cover`): the room shows the slide's picture with the logo or
//! the overlays on it, and the show comes back in front, where it was, when they are gone.
//!
//! `Driver` holds the rules and is tested with `FakeHost`; `host::PsHost` is the real one (PowerShell driving
//! PowerPoint through COM, Windows only at run time).
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub mod app;
pub mod host;
#[path = "../strings/pplive.rs"]
pub mod strings;

/// A monitor's rectangle in physical pixels, as `app::Monitor` has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect { pub x: i32, pub y: i32, pub width: i32, pub height: i32 }

/// PowerPoint's `SlideShowView.State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowState { Running, Paused, Black, White, Done }

impl ShowState {
    /// From `PpSlideShowState` (1 running … 5 done).
    pub fn from_code(n: u32) -> Option<ShowState> {
        Some(match n { 1 => Self::Running, 2 => Self::Paused, 3 => Self::Black, 4 => Self::White, 5 => Self::Done, _ => return None })
    }
}

/// Where the show is. `slide` counts the shown slides from 1 (hidden ones are skipped, as in the pictures);
/// `click` is how many of the slide's `clicks` animations have played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos { pub slide: usize, pub slides: usize, pub click: usize, pub clicks: usize, pub state: ShowState }

impl Pos {
    /// Next would leave the show: the last slide with every click played, or the black end screen.
    pub fn at_end(&self) -> bool { self.state == ShowState::Done || (self.slide >= self.slides && self.click >= self.clicks) }
    /// Back would leave the show: the first slide before any click.
    pub fn at_start(&self) -> bool { self.state != ShowState::Done && self.slide <= 1 && self.click == 0 }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HostError {
    /// The show is gone: ended in PowerPoint, or PowerPoint closed.
    Ended,
    /// Said to the operator.
    Failed(String),
}

/// One running slide show. Every call waits for PowerPoint's answer.
pub trait SlideShowHost: Send {
    /// Opens `file` and runs its slide show full screen on `screen`, in front of everything. The shown slides.
    fn open(&mut self, file: &Path, screen: Rect) -> Result<usize, HostError>;
    /// One click on: the next animation, or the next slide.
    fn next(&mut self) -> Result<Pos, HostError>;
    fn prev(&mut self) -> Result<Pos, HostError>;
    /// Shown slide `slide` (from 1), from its start.
    fn goto(&mut self, slide: usize) -> Result<Pos, HostError>;
    fn current(&mut self) -> Result<Pos, HostError>;
    /// PowerPoint's black screen on or off.
    fn black(&mut self, on: bool) -> Result<Pos, HostError>;
    /// The show's window behind every other window (`on`), so the room window is seen, or back in front of
    /// everything. The show goes on as it is: nothing is paused or ended.
    fn cover(&mut self, on: bool) -> Result<Pos, HostError>;
    /// Ends the show and closes the presentation. Nothing else of PowerPoint's is touched.
    fn close(&mut self);
}

/// What the live engine asks of PowerPoint (`App::show_want`).
#[derive(Clone, Debug, PartialEq)]
pub struct Want {
    pub item_id: Uuid,
    pub title: String,
    pub file: PathBuf,
    /// The live slide, from 0.
    pub slide: usize,
    /// The slide was cleared: PowerPoint shows black.
    pub black: bool,
    /// The logo is on or an overlay is up: the room window must be seen, so PowerPoint's steps behind it.
    pub covered: bool,
    /// The room monitor; `None` when the room screen is not on a monitor.
    pub screen: Option<Rect>,
}

/// What the App should do after the driver talked to PowerPoint.
#[derive(Clone, Debug, PartialEq)]
pub enum After {
    Nothing,
    /// PowerPoint is on this slide (from 0) now: the live slide follows.
    Slide(usize),
    /// Next past the end, or Back past the start: the live engine takes the next or previous item.
    Beyond,
    /// PowerPoint ended the show by itself (its title, for the notice).
    Ended(String),
    /// The show could not start or answer (the notice).
    Failed(String),
}

struct Open { item_id: Uuid, title: String, file: PathBuf, slides: usize, slide: usize, black: bool, covered: bool }

/// Keeps PowerPoint's show in line with the live item, and turns Next/Back into clicks.
pub struct Driver {
    host: Box<dyn SlideShowHost>,
    open: Option<Open>,
    /// The item whose show failed or was ended in PowerPoint: not started again until another item goes live or
    /// the operator clicks one of its slides (`retry`).
    stopped: Option<Uuid>,
}

impl Driver {
    pub fn new(host: Box<dyn SlideShowHost>) -> Driver { Driver { host, open: None, stopped: None } }

    /// A show is running for this item (black or not).
    pub fn is_open_for(&self, item_id: Uuid) -> bool { self.open.as_ref().is_some_and(|o| o.item_id == item_id) }

    /// The operator asked for one of this item's slides: a stopped show may start again.
    pub fn retry(&mut self, item_id: Uuid) { if self.stopped == Some(item_id) { self.stopped = None } }

    /// Brings PowerPoint in line with `want`: starts, moves, blacks out or ends the show.
    pub fn apply(&mut self, want: Option<&Want>) -> After {
        let Some(w) = want else {
            self.close();
            self.stopped = None;
            return After::Nothing;
        };
        if self.open.as_ref().is_some_and(|o| o.item_id != w.item_id || o.file != w.file) { self.close() }
        if self.stopped.is_some_and(|id| id != w.item_id) { self.stopped = None }
        if self.open.is_none() {
            // A cleared slide never starts a show just to show black.
            if w.black || self.stopped == Some(w.item_id) { return After::Nothing }
            let Some(screen) = w.screen else {
                self.stopped = Some(w.item_id);
                return After::Failed(strings::no_room_screen());
            };
            match self.host.open(&w.file, screen) {
                Ok(slides) => self.open = Some(Open { item_id: w.item_id, title: w.title.clone(), file: w.file.clone(), slides, slide: 1, black: false, covered: false }),
                Err(e) => {
                    self.host.close();
                    self.stopped = Some(w.item_id);
                    return After::Failed(strings::failed(&w.title, &message(e)));
                }
            }
        }
        let open = self.open.as_ref().expect("opened above");
        let (black, slide, slides) = (open.black, open.slide, open.slides);
        if open.covered != w.covered {
            if let Err(e) = self.host.cover(w.covered) { return self.lost(e) }
            if let Some(o) = &mut self.open { o.covered = w.covered }
        }
        if black != w.black {
            if let Err(e) = self.host.black(w.black) { return self.lost(e) }
            if let Some(o) = &mut self.open { o.black = w.black }
        }
        // Going to the slide it is on would play its builds again.
        let to = (w.slide + 1).min(slides.max(1));
        if !w.black && to != slide {
            match self.host.goto(to) {
                Ok(p) => if let Some(o) = &mut self.open { o.slide = p.slide },
                Err(e) => return self.lost(e),
            }
        }
        After::Nothing
    }

    /// Next (`forward`) or Back on a running show: one click in PowerPoint, or `Beyond` at either end.
    pub fn step(&mut self, forward: bool) -> After {
        if self.open.is_none() { return After::Beyond }
        let pos = match self.host.current() { Ok(p) => p, Err(e) => return self.lost(e) };
        if (forward && pos.at_end()) || (!forward && pos.at_start()) { return After::Beyond }
        let moved = if forward { self.host.next() } else { self.host.prev() };
        match moved {
            Ok(p) => self.moved(p),
            Err(e) => self.lost(e),
        }
    }

    /// A look at the show: a slide PowerPoint moved to by itself (timings, a click in the show), or its end.
    pub fn poll(&mut self) -> After {
        if self.open.is_none() { return After::Nothing }
        match self.host.current() {
            Ok(p) => self.moved(p),
            Err(e) => self.lost(e),
        }
    }

    /// Ends the show (Clear All, or the item left).
    pub fn close(&mut self) {
        if self.open.take().is_some() { self.host.close() }
    }

    fn moved(&mut self, p: Pos) -> After {
        let Some(o) = &mut self.open else { return After::Nothing };
        if p.state == ShowState::Done || p.slide == o.slide { return After::Nothing }
        o.slide = p.slide;
        After::Slide(p.slide.saturating_sub(1))
    }

    fn lost(&mut self, e: HostError) -> After {
        let Some(o) = self.open.take() else { return After::Nothing };
        self.host.close();
        self.stopped = Some(o.item_id);
        match e {
            HostError::Ended => After::Ended(o.title),
            HostError::Failed(why) => After::Failed(strings::failed(&o.title, &why)),
        }
    }
}

fn message(e: HostError) -> String {
    match e {
        HostError::Ended => strings::no_answer(),
        HostError::Failed(why) => why,
    }
}

/// A slide show in memory, for tests: each slide has its clicks, and every call is logged.
#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    pub struct Show {
        pub log: Vec<String>,
        /// Clicks on each slide.
        pub clicks: Vec<usize>,
        pub open: bool,
        pub slide: usize,
        pub click: usize,
        pub black: bool,
        pub covered: bool,
        pub done: bool,
        /// The next call finds the show ended (closed in PowerPoint).
        pub gone: bool,
        /// `open` fails with this.
        pub fail: Option<String>,
    }

    #[derive(Clone, Default)]
    pub struct FakeHost(pub Arc<Mutex<Show>>);

    impl FakeHost {
        pub fn with_clicks(clicks: &[usize]) -> FakeHost {
            FakeHost(Arc::new(Mutex::new(Show { clicks: clicks.to_vec(), ..Show::default() })))
        }
        pub fn log(&self) -> Vec<String> { self.0.lock().unwrap().log.clone() }
        pub fn take_log(&self) -> Vec<String> { std::mem::take(&mut self.0.lock().unwrap().log) }
        /// PowerPoint moved on by itself (a slide timing).
        pub fn set_slide(&self, slide: usize) { let mut s = self.0.lock().unwrap(); s.slide = slide; s.click = 0; }
        pub fn end(&self) { self.0.lock().unwrap().gone = true }
        fn pos(s: &Show) -> Pos {
            let state = if s.done { ShowState::Done } else if s.black { ShowState::Black } else { ShowState::Running };
            Pos { slide: s.slide, slides: s.clicks.len(), click: s.click, clicks: s.clicks[s.slide - 1], state }
        }
        fn call(&self, what: String, f: impl FnOnce(&mut Show)) -> Result<Pos, HostError> {
            let mut s = self.0.lock().unwrap();
            s.log.push(what);
            if s.gone || !s.open { return Err(HostError::Ended) }
            f(&mut s);
            Ok(Self::pos(&s))
        }
    }

    impl SlideShowHost for FakeHost {
        fn open(&mut self, file: &Path, screen: Rect) -> Result<usize, HostError> {
            let mut s = self.0.lock().unwrap();
            s.log.push(format!("open {} {},{} {}x{}", file.display(), screen.x, screen.y, screen.width, screen.height));
            if let Some(e) = s.fail.clone() { return Err(HostError::Failed(e)) }
            *s = Show { log: std::mem::take(&mut s.log), clicks: s.clicks.clone(), open: true, slide: 1, ..Show::default() };
            Ok(s.clicks.len())
        }
        fn next(&mut self) -> Result<Pos, HostError> {
            self.call("next".into(), |s| {
                if s.click < s.clicks[s.slide - 1] { s.click += 1 } else if s.slide < s.clicks.len() { s.slide += 1; s.click = 0 } else { s.done = true }
            })
        }
        fn prev(&mut self) -> Result<Pos, HostError> {
            self.call("prev".into(), |s| {
                if s.done { s.done = false } else if s.click > 0 { s.click -= 1 } else if s.slide > 1 { s.slide -= 1; s.click = s.clicks[s.slide - 1] }
            })
        }
        fn goto(&mut self, slide: usize) -> Result<Pos, HostError> {
            self.call(format!("goto {slide}"), |s| { s.slide = slide; s.click = 0; s.done = false })
        }
        fn current(&mut self) -> Result<Pos, HostError> { self.call("pos".into(), |_| {}) }
        fn black(&mut self, on: bool) -> Result<Pos, HostError> { self.call(format!("black {}", on as u8), |s| s.black = on) }
        fn cover(&mut self, on: bool) -> Result<Pos, HostError> { self.call(format!("cover {}", on as u8), |s| s.covered = on) }
        fn close(&mut self) {
            let mut s = self.0.lock().unwrap();
            s.log.push("close".into());
            s.open = false;
            s.gone = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fake::FakeHost, *};

    const ROOM: Rect = Rect { x: 1920, y: 0, width: 1920, height: 1080 };

    fn want(item: Uuid, slide: usize) -> Want {
        Want { item_id: item, title: "Talk".into(), file: PathBuf::from("talk.pptx"), slide, black: false, covered: false, screen: Some(ROOM) }
    }

    fn driver(clicks: &[usize]) -> (Driver, FakeHost) {
        let host = FakeHost::with_clicks(clicks);
        (Driver::new(Box::new(host.clone())), host)
    }

    #[test]
    fn going_live_opens_the_show_on_the_room_monitor_at_the_live_slide() {
        let (mut d, host) = driver(&[0, 0, 0]);
        let item = Uuid::new_v4();
        assert_eq!(d.apply(Some(&want(item, 0))), After::Nothing);
        assert_eq!(host.take_log(), ["open talk.pptx 1920,0 1920x1080"], "slide 1 is where a show starts: no goto");
        assert!(d.is_open_for(item));
        // Taken live at slide 3 of an item already open: only a goto.
        assert_eq!(d.apply(Some(&want(item, 2))), After::Nothing);
        assert_eq!(host.take_log(), ["goto 3"]);
        // The same slide again never replays its builds.
        d.apply(Some(&want(item, 2)));
        assert!(host.take_log().is_empty());
    }

    #[test]
    fn next_plays_each_click_then_the_next_slide_then_leaves_at_the_end() {
        let (mut d, host) = driver(&[2, 0]);
        let item = Uuid::new_v4();
        d.apply(Some(&want(item, 0)));
        host.take_log();
        assert_eq!(d.step(true), After::Nothing, "first build");
        assert_eq!(d.step(true), After::Nothing, "second build");
        assert_eq!(d.step(true), After::Slide(1));
        assert_eq!(host.take_log(), ["pos", "next", "pos", "next", "pos", "next"]);
        // Slide 2 of 2, nothing left to click: Next takes the next item, without touching PowerPoint.
        assert_eq!(d.step(true), After::Beyond);
        assert_eq!(host.take_log(), ["pos"]);
        // Back walks the builds of slide 1 backwards, then leaves at its start.
        assert_eq!(d.step(false), After::Slide(0));
        assert_eq!(d.step(false), After::Nothing);
        assert_eq!(d.step(false), After::Nothing);
        assert_eq!(d.step(false), After::Beyond);
    }

    #[test]
    fn the_live_slide_following_a_click_does_not_send_powerpoint_back() {
        let (mut d, host) = driver(&[0, 1, 0]);
        let item = Uuid::new_v4();
        d.apply(Some(&want(item, 0)));
        assert_eq!(d.step(true), After::Slide(1));
        host.take_log();
        // The App shows slide 2 live, and asks again: PowerPoint is already there.
        d.apply(Some(&want(item, 1)));
        assert!(host.take_log().is_empty());
    }

    #[test]
    fn the_show_steps_behind_the_room_for_the_logo_and_overlays() {
        let host = FakeHost::with_clicks(&[0, 0, 0]);
        let mut d = Driver::new(Box::new(host.clone()));
        let item = Uuid::new_v4();
        d.apply(Some(&want(item, 0)));
        host.take_log();
        // The logo goes on (or a message comes up): PowerPoint's window goes behind, once.
        d.apply(Some(&Want { covered: true, ..want(item, 0) }));
        d.apply(Some(&Want { covered: true, ..want(item, 0) }));
        assert_eq!(host.take_log(), ["cover 1"]);
        // The operator goes on a slide meanwhile: the show follows behind the room window.
        d.apply(Some(&Want { covered: true, ..want(item, 1) }));
        assert_eq!(host.take_log(), ["goto 2"]);
        // Gone again: back in front, on the slide it is on.
        d.apply(Some(&want(item, 1)));
        assert_eq!(host.take_log(), ["cover 0"]);
        // A deck that goes live with the logo already on starts behind the room window.
        let host = FakeHost::with_clicks(&[0]);
        let mut d = Driver::new(Box::new(host.clone()));
        d.apply(Some(&Want { covered: true, ..want(item, 0) }));
        assert_eq!(host.log().last().map(String::as_str), Some("cover 1"));
        assert!(host.0.lock().unwrap().covered);
    }

    #[test]
    fn a_cleared_slide_is_black_and_comes_back() {
        let (mut d, host) = driver(&[0, 0]);
        let item = Uuid::new_v4();
        d.apply(Some(&want(item, 1)));
        host.take_log();
        d.apply(Some(&Want { black: true, ..want(item, 1) }));
        assert_eq!(host.take_log(), ["black 1"]);
        d.apply(Some(&want(item, 1)));
        assert_eq!(host.take_log(), ["black 0"]);
        // After Clear All the show is closed; a cleared slide never opens it again.
        d.close();
        d.apply(Some(&Want { black: true, ..want(item, 1) }));
        assert_eq!(host.take_log(), ["close"]);
        // The next slide shown does.
        d.apply(Some(&want(item, 1)));
        assert_eq!(host.take_log(), ["open talk.pptx 1920,0 1920x1080", "goto 2"]);
    }

    #[test]
    fn another_item_or_nothing_live_closes_the_show() {
        let (mut d, host) = driver(&[0, 0]);
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        d.apply(Some(&want(a, 0)));
        d.apply(Some(&Want { file: PathBuf::from("other.pptx"), ..want(b, 0) }));
        assert_eq!(host.take_log(), ["open talk.pptx 1920,0 1920x1080", "close", "open other.pptx 1920,0 1920x1080"]);
        d.apply(None);
        assert_eq!(host.take_log(), ["close"]);
        assert!(!d.is_open_for(b));
        // Next with no show open is the live engine's own.
        assert_eq!(d.step(true), After::Beyond);
    }

    #[test]
    fn powerpoint_moving_by_itself_moves_the_live_slide() {
        let (mut d, host) = driver(&[0, 0, 0]);
        let item = Uuid::new_v4();
        d.apply(Some(&want(item, 0)));
        assert_eq!(d.poll(), After::Nothing);
        host.set_slide(3);
        assert_eq!(d.poll(), After::Slide(2));
        assert_eq!(d.poll(), After::Nothing);
    }

    #[test]
    fn a_show_ended_in_powerpoint_is_not_started_again_until_a_slide_is_clicked() {
        let (mut d, host) = driver(&[0, 0]);
        let item = Uuid::new_v4();
        d.apply(Some(&want(item, 0)));
        host.end();
        assert_eq!(d.poll(), After::Ended("Talk".into()));
        assert!(!d.is_open_for(item));
        host.take_log();
        d.apply(Some(&want(item, 1)));
        assert!(host.take_log().is_empty(), "the room shows the pictures meanwhile");
        d.retry(item);
        d.apply(Some(&want(item, 1)));
        assert_eq!(host.take_log(), ["open talk.pptx 1920,0 1920x1080", "goto 2"]);
        // Ended in the middle of a Next: the same.
        host.end();
        assert_eq!(d.step(true), After::Ended("Talk".into()));
    }

    #[test]
    fn a_show_that_cannot_start_says_why_once() {
        let (mut d, host) = driver(&[0]);
        host.0.lock().unwrap().fail = Some("PowerPoint could not be started.".into());
        let item = Uuid::new_v4();
        let After::Failed(why) = d.apply(Some(&want(item, 0))) else { panic!() };
        assert!(why.contains("PowerPoint could not be started.") && why.contains("“Talk”"), "{why}");
        assert_eq!(d.apply(Some(&want(item, 0))), After::Nothing, "not tried every half second");
        // Without a room monitor nothing is tried at all.
        let other = Uuid::new_v4();
        host.take_log();
        assert_eq!(d.apply(Some(&Want { screen: None, ..want(other, 0) })), After::Failed(strings::no_room_screen()));
        assert!(host.take_log().is_empty());
    }

    #[test]
    fn the_end_screen_counts_as_the_end() {
        let p = Pos { slide: 2, slides: 3, click: 0, clicks: 0, state: ShowState::Done };
        assert!(p.at_end() && !p.at_start());
        let p = Pos { slide: 3, slides: 3, click: 1, clicks: 2, state: ShowState::Running };
        assert!(!p.at_end());
        assert_eq!(ShowState::from_code(3), Some(ShowState::Black));
        assert_eq!(ShowState::from_code(0), None);
    }
}
