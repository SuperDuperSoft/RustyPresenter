//! FEAT-4 on the live engine: Jump to a song section, Refresh the live item from the library, and `reconcile`,
//! which compares the live snapshot with the library after every data change (spec §5.2, §5.4, §5.5).
use super::{Cursor, Live, Plan};
use crate::sections::{self, Decision, JumpTarget, LiveChange, strings};
use uuid::Uuid;

impl Live {
    /// Jump the live song to a section. Walks the snapshot, like Next; errors change nothing.
    pub(super) fn jump(&mut self, item_id: Uuid, t: JumpTarget) -> Result<(), String> {
        let (Some(cursor), Some(item)) = (self.cursor.clone(), self.item.clone()) else { return Err(strings::nothing_live().into()) };
        if cursor.item_id != item_id { return Err(strings::live_song_changed().into()) }
        let target = sections::jump_target(&item.slides, cursor.slide, &t).map_err(|e| e.message())?;
        self.go(item, cursor.item_index, target);
        Ok(())
    }

    /// Update screen: the library's version of the live item, at the same place in it. With the text cleared the
    /// clear stays; the new words show on the next Next, jump or click.
    pub(super) fn refresh(&mut self, plan: &dyn Plan, item_id: Uuid) -> Result<(), String> {
        let (Some(cursor), Some(old)) = (self.cursor.clone().filter(|c| c.item_id == item_id), self.item.clone()) else {
            return Err(strings::no_longer_live().into());
        };
        let new = plan.build(item_id).filter(|i| !i.slides.is_empty()).ok_or(strings::no_slides_now())?;
        let slide = sections::refresh_slide(sections::map_position(&old.slides, cursor.slide, &new.slides)).min(new.slides.len() - 1);
        self.changed = None;
        if self.frame.slide.is_some() {
            self.go(new, cursor.item_index, slide);
        } else {
            self.item = Some(new);
            self.cursor = Some(Cursor { slide, ..cursor });
            self.commit_change(super::Change::Silent);
        }
        Ok(())
    }

    /// Compares the live snapshot with the library. The room never changes here: an edit that leaves the slide on
    /// screen looking the same is taken in quietly (a new version with the same frame, so an older Next is stale);
    /// anything else sets `changed` for the banner. True when the live state changed.
    pub fn reconcile(&mut self, plan: &dyn Plan) -> bool {
        let (Some(cursor), Some(old)) = (self.cursor.clone(), self.item.clone()) else { return false };
        let before = self.changed.clone();
        let Some(new) = plan.build(cursor.item_id) else {
            self.changed = None;
            return before.is_some();
        };
        if cursor.slide >= old.slides.len() { return false }
        let look_equal = new.background == old.background;
        match sections::decide(&old.slides, cursor.slide, &new.slides, look_equal, |i| sections::map_position(&old.slides, i, &new.slides)) {
            Decision::Same => self.changed = None,
            Decision::NotesOnly => {
                if let Some(item) = self.item.as_mut() { item.slides = new.slides }
                self.changed = None;
                return true;
            }
            Decision::Adopt { slide, map } => {
                // FEAT-2: the App hands the old → new map to auto-advance (`AutomationRuntime::on_adopt`).
                if let Some(item) = self.item.as_mut() { item.slides = new.slides }
                self.cursor = Some(Cursor { slide, ..cursor });
                self.changed = None;
                self.adopted = Some((cursor.item_id, map));
                self.commit_change(super::Change::Silent);
                return true;
            }
            Decision::Banner(reason) => self.changed = Some(LiveChange { item_id: cursor.item_id, reason }),
        }
        self.changed != before
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        live::{Command, Envelope, LiveItem, Plan, RoomEvent, SlideLayer},
        model::{Background, Settings, Slide, SlideContent, Song, Theme},
        sections::{ChangeReason, SectionKind},
    };
    use super::Live;
    use std::{cell::RefCell, collections::HashMap};
    use uuid::Uuid;

    /// A service of songs and text items whose slides the test can change, as the library would.
    #[derive(Default)]
    struct Lib { order: Vec<Uuid>, items: RefCell<HashMap<Uuid, LiveItem>> }

    impl Plan for Lib {
        fn order(&self) -> Vec<Uuid> { self.order.clone() }
        fn build(&self, id: Uuid) -> Option<LiveItem> { self.items.borrow().get(&id).cloned() }
    }

    const L: &str = "Verse 1\nv1\nChorus\nc\nVerse 2\nv2\nBridge\nb";

    fn song_slides(lyrics: &str, order: &[&str]) -> Vec<Slide> {
        let song = Song { lyrics: lyrics.into(), arrangement: order.iter().map(|s| s.to_string()).collect(), ..Default::default() };
        crate::slides::song_slides(&song, &Settings { credits: "none".into(), lines_per_slide: 1, ..Default::default() })
    }

    fn item(id: Uuid, kind: &str, slides: Vec<Slide>) -> LiveItem {
        LiveItem { item_id: id, title: "T".into(), kind: kind.into(), slides, background: Background::Color { color: "#000".into() }, theme: Theme::default() }
    }

    impl Lib {
        fn with(items: Vec<LiveItem>) -> Self {
            Lib { order: items.iter().map(|i| i.item_id).collect(), items: RefCell::new(items.into_iter().map(|i| (i.item_id, i)).collect()) }
        }
        fn set_slides(&self, id: Uuid, slides: Vec<Slide>) { self.items.borrow_mut().get_mut(&id).unwrap().slides = slides }
        fn remove(&self, id: Uuid) { self.items.borrow_mut().remove(&id); }
    }

    fn env(cmd: Command, base: Option<u64>) -> Envelope { Envelope { id: Uuid::new_v4().to_string(), base, cmd } }
    fn run(l: &mut Live, p: &Lib, cmd: Command) -> Result<(), String> {
        let b = l.version;
        let o = l.apply(env(cmd, Some(b)), p);
        if o.ok { Ok(()) } else { Err(o.error.unwrap_or_default()) }
    }
    fn shown(l: &Live) -> Option<String> { match &l.frame.slide { Some(SlideLayer::Text { text, .. }) => Some(text.clone()), _ => None } }
    fn jump(id: Uuid, kind: SectionKind, number: Option<u8>) -> Command { Command::Jump { item_id: id, kind, number, occurrence: None } }

    fn live_song(order: &[&str], slide: usize) -> (Live, Lib, Uuid) {
        let id = Uuid::new_v4();
        let p = Lib::with(vec![item(id, "song", song_slides(L, order))]);
        let mut l = Live::new();
        run(&mut l, &p, Command::Show { item_id: id, slide }).unwrap();
        (l, p, id)
    }

    #[test]
    fn jump_goes_to_the_section_and_walks_the_snapshot() {
        let (mut l, p, id) = live_song(&["Verse 1", "Chorus", "Verse 2", "Chorus"], 0);
        run(&mut l, &p, jump(id, SectionKind::Verse, Some(2))).unwrap();
        assert_eq!(shown(&l).as_deref(), Some("v2"));
        run(&mut l, &p, jump(id, SectionKind::Chorus, None)).unwrap();
        assert_eq!(l.cursor.as_ref().unwrap().slide, 3, "the chorus after verse 2");
    }

    #[test]
    fn jump_requires_base() {
        let (mut l, p, id) = live_song(&[], 0);
        let o = l.apply(env(jump(id, SectionKind::Chorus, None), None), &p);
        assert!(!o.ok);
        assert!(o.error.unwrap().starts_with("stale"));
    }

    #[test]
    fn jump_queued_before_clear_all_is_stale() {
        let (mut l, p, id) = live_song(&[], 0);
        let base = l.version;
        run(&mut l, &p, Command::ClearAll).unwrap();
        let o = l.apply(env(jump(id, SectionKind::Chorus, None), Some(base)), &p);
        assert!(!o.ok);
        assert_eq!(l.frame.slide, None);
    }

    #[test]
    fn jump_after_clear_slide_brings_text_back() {
        let (mut l, p, id) = live_song(&[], 0);
        run(&mut l, &p, Command::ClearSlide).unwrap();
        run(&mut l, &p, jump(id, SectionKind::Bridge, None)).unwrap();
        assert_eq!(shown(&l).as_deref(), Some("b"));
    }

    #[test]
    fn jump_on_non_song_errors_and_keeps_version() {
        let id = Uuid::new_v4();
        let text = vec![Slide::new("a", SlideContent::Text { text: "a".into(), footer: None, reference: None, size: 0, parallel: None })];
        let p = Lib::with(vec![item(id, "text", text)]);
        let mut l = Live::new();
        run(&mut l, &p, Command::Show { item_id: id, slide: 0 }).unwrap();
        let v = l.version;
        assert_eq!(run(&mut l, &p, jump(id, SectionKind::Chorus, None)), Err("Section keys work on songs.".into()));
        assert_eq!(l.version, v);
    }

    #[test]
    fn jump_wrong_item_or_nothing_live_errors() {
        let (mut l, p, _) = live_song(&[], 0);
        assert_eq!(run(&mut l, &p, jump(Uuid::new_v4(), SectionKind::Chorus, None)), Err("The live song changed — try again.".into()));
        let mut empty = Live::new();
        assert_eq!(run(&mut empty, &p, jump(Uuid::new_v4(), SectionKind::Chorus, None)), Err("Nothing is live yet.".into()));
    }

    #[test]
    fn jump_missing_section_says_so() {
        let (mut l, p, id) = live_song(&[], 0);
        assert_eq!(run(&mut l, &p, jump(id, SectionKind::Tag, None)), Err("This song has no Tag.".into()));
    }

    #[test]
    fn duplicate_jump_id_applies_once() {
        let (mut l, p, id) = live_song(&[], 0);
        let e = env(jump(id, SectionKind::Verse, None), Some(l.version));
        assert!(l.apply(e.clone(), &p).ok);
        let v = l.version;
        assert!(l.apply(e, &p).duplicate);
        assert_eq!(l.version, v);
    }

    #[test]
    fn edit_other_section_adopts_quietly_frame_unchanged() {
        let (mut l, p, id) = live_song(&[], 0);
        let (frame, v) = (l.frame.clone(), l.version);
        p.set_slides(id, song_slides(&L.replace("v2", "v2 fixed"), &[]));
        assert!(l.reconcile(&p));
        assert_eq!(l.frame, frame);
        assert_eq!(l.version, v + 1);
        assert_eq!(l.changed, None);
        assert_eq!(l.next_preview(&p).map(|(_, s)| s.label), Some("Chorus".into()));
        run(&mut l, &p, Command::Next).unwrap();
        run(&mut l, &p, Command::Next).unwrap();
        assert_eq!(shown(&l).as_deref(), Some("v2 fixed"));
    }

    #[test]
    fn adoption_makes_in_flight_next_stale() {
        let (mut l, p, id) = live_song(&[], 0);
        let base = l.version;
        p.set_slides(id, song_slides(&L.replace("v2", "v2 fixed"), &[]));
        l.reconcile(&p);
        assert!(!l.apply(env(Command::Next, Some(base)), &p).ok);
    }

    #[test]
    fn arrangement_change_while_live_keeps_position() {
        let (mut l, p, id) = live_song(&["Verse 1", "Verse 2"], 1);
        p.set_slides(id, song_slides(L, &["Verse 1", "Chorus", "Verse 2"]));
        l.reconcile(&p);
        assert_eq!(l.cursor.as_ref().unwrap().slide, 2);
        assert_eq!(shown(&l).as_deref(), Some("v2"));
    }

    #[test]
    fn notes_only_change_adopts_with_no_version_change() {
        let (mut l, p, id) = live_song(&[], 0);
        let v = l.version;
        let mut s = song_slides(L, &[]);
        s[0].notes = "softly".into();
        p.set_slides(id, s);
        assert!(l.reconcile(&p));
        assert_eq!(l.version, v);
        assert_eq!(l.item.as_ref().unwrap().slides[0].notes, "softly");
    }

    #[test]
    fn edit_live_slide_sets_words_on_screen_and_keeps_frame_and_version() {
        let (mut l, p, id) = live_song(&[], 0);
        let (frame, v) = (l.frame.clone(), l.version);
        p.set_slides(id, song_slides(&L.replace("v1", "v1 fixed"), &[]));
        assert!(l.reconcile(&p));
        assert_eq!(l.changed.as_ref().map(|c| c.reason), Some(ChangeReason::WordsOnScreen));
        assert_eq!((l.frame.clone(), l.version), (frame, v));
        assert!(!l.reconcile(&p), "nothing new the second time");
    }

    #[test]
    fn background_change_gives_look_reason() {
        let (mut l, p, id) = live_song(&[], 0);
        p.items.borrow_mut().get_mut(&id).unwrap().background = Background::Color { color: "#123456".into() };
        l.reconcile(&p);
        assert_eq!(l.changed.as_ref().map(|c| c.reason), Some(ChangeReason::LookOnScreen));
    }

    #[test]
    fn undoing_edit_by_hand_clears_changed() {
        let (mut l, p, id) = live_song(&[], 0);
        p.set_slides(id, song_slides(&L.replace("v1", "v1 fixed"), &[]));
        l.reconcile(&p);
        p.set_slides(id, song_slides(L, &[]));
        assert!(l.reconcile(&p));
        assert_eq!(l.changed, None);
    }

    #[test]
    fn take_live_other_item_clears_changed() {
        let (mut l, p, id) = live_song(&[], 0);
        p.set_slides(id, song_slides(&L.replace("v1", "v1 fixed"), &[]));
        l.reconcile(&p);
        run(&mut l, &p, Command::Next).unwrap();
        assert!(l.changed.is_some(), "Next walks the old words and keeps the banner");
        run(&mut l, &p, Command::Show { item_id: id, slide: 0 }).unwrap();
        assert_eq!(l.changed, None, "a click shows the new version");
        assert_eq!(shown(&l).as_deref(), Some("v1 fixed"));
    }

    #[test]
    fn removed_live_item_has_no_banner() {
        let (mut l, p, id) = live_song(&[], 0);
        p.set_slides(id, song_slides(&L.replace("v1", "v1 fixed"), &[]));
        l.reconcile(&p);
        p.remove(id);
        assert!(l.reconcile(&p));
        assert_eq!(l.changed, None);
    }

    #[test]
    fn refresh_shows_new_words_at_same_position() {
        let (mut l, p, id) = live_song(&[], 2);
        p.set_slides(id, song_slides(&L.replace("v2", "v2 fixed"), &[]));
        l.reconcile(&p);
        p.set_slides(id, song_slides(&L.replace("v2", "v2 fixed").replace("Bridge\nb", ""), &["Chorus", "Verse 1", "Verse 2"]));
        l.reconcile(&p);
        run(&mut l, &p, Command::Refresh { item_id: id }).unwrap();
        assert_eq!(shown(&l).as_deref(), Some("v2 fixed"));
        assert_eq!(l.cursor.as_ref().unwrap().slide, 2);
        assert_eq!(l.changed, None);
    }

    #[test]
    fn refresh_lost_place_goes_to_slide_zero() {
        let (mut l, p, id) = live_song(&[], 3);
        p.set_slides(id, song_slides("Verse 1\nv1\nChorus\nc", &[]));
        l.reconcile(&p);
        assert_eq!(l.changed.as_ref().map(|c| c.reason), Some(ChangeReason::PlaceLost));
        run(&mut l, &p, Command::Refresh { item_id: id }).unwrap();
        assert_eq!(shown(&l).as_deref(), Some("v1"));
    }

    #[test]
    fn refresh_with_no_slides_errors_and_keeps_changed() {
        let (mut l, p, id) = live_song(&[], 0);
        p.set_slides(id, vec![]);
        l.reconcile(&p);
        assert_eq!(l.changed.as_ref().map(|c| c.reason), Some(ChangeReason::NoSlides));
        assert_eq!(run(&mut l, &p, Command::Refresh { item_id: id }), Err("That song has no slides now. The screen keeps the old words.".into()));
        assert!(l.changed.is_some());
    }

    #[test]
    fn refresh_wrong_item_errors() {
        let (mut l, p, _) = live_song(&[], 0);
        assert_eq!(run(&mut l, &p, Command::Refresh { item_id: Uuid::new_v4() }), Err("That song is no longer live.".into()));
    }

    #[test]
    fn refresh_while_cleared_keeps_clear() {
        let (mut l, p, id) = live_song(&[], 0);
        run(&mut l, &p, Command::ClearSlide).unwrap();
        p.set_slides(id, song_slides(&L.replace("v1", "v1 fixed").replace("c\n", "c fixed\n"), &[]));
        l.reconcile(&p);
        let (frame, v) = (l.frame.clone(), l.version);
        run(&mut l, &p, Command::Refresh { item_id: id }).unwrap();
        assert_eq!((l.frame.clone(), l.version), (frame, v + 1));
        assert_eq!(l.changed, None);
        run(&mut l, &p, Command::Next).unwrap();
        assert_eq!(shown(&l).as_deref(), Some("c fixed"));
    }

    #[test]
    fn failed_refresh_prepare_reverts_snapshot_and_banner() {
        let (mut l, p, id) = live_song(&[], 0);
        let v = l.version;
        l.report(RoomEvent::Committed { version: v });
        p.set_slides(id, song_slides(&L.replace("v1", "v1 fixed"), &[]));
        l.reconcile(&p);
        run(&mut l, &p, Command::Refresh { item_id: id }).unwrap();
        let now = l.version;
        assert!(l.report(RoomEvent::Failed { version: now, reason: "x".into() }));
        assert_eq!(shown(&l).as_deref(), Some("v1"));
        l.reconcile(&p);
        assert_eq!(l.changed.as_ref().map(|c| c.reason), Some(ChangeReason::WordsOnScreen));
    }
}
