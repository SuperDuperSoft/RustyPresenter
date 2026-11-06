//! The compositor: what every box of a stage layout shows right now (pure, no UI).
use super::{presets::resolve_style, strings as s, ElementKind, PctRect, ResolvedStyle, StageLayout, StageState, TimerRef};
use crate::{app::LiveView, model::{AssetId, Slide, SlideContent}, timers::{self, TimerSnapshot, TimerState, TimersView}};
use uuid::Uuid;

/// Everything the stage is built from.
pub struct StageInputs<'a> {
    pub live: &'a LiveView,
    /// The live item's slides, for "next section" (empty when unknown).
    pub slides: &'a [Slide],
    pub timers: &'a TimersView,
    pub state: &'a StageState,
    pub now: chrono::NaiveTime,
}

/// A stage layout filled in.
#[derive(Clone, Debug, PartialEq)]
pub struct StageView {
    pub layout_id: Uuid,
    pub background: String,
    pub font: String,
    pub cells: Vec<Cell>,
    /// A message is up but the layout has no message box: show it as a banner instead.
    pub message_fallback: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell { pub element: Uuid, pub rect: PctRect, pub style: ResolvedStyle, pub content: CellContent }

#[derive(Clone, Debug, PartialEq)]
pub enum CellContent {
    Empty,
    /// `red` = over time (drawn red whatever the box colour).
    Text { text: String, dim: bool, caption: Option<String>, red: bool },
    Picture { asset: AssetId, caption: Option<String> },
    Message { text: String },
}

impl CellContent {
    fn text(text: impl Into<String>, caption: Option<String>) -> Self { CellContent::Text { text: text.into(), dim: false, caption, red: false } }
}

pub fn build(inp: &StageInputs, layout: &StageLayout) -> StageView {
    let mut cells: Vec<Cell> = layout.elements.iter().filter_map(|e| e.known()).map(|e| {
        let mut style = resolve_style(&e.kind, &e.style);
        let content = content(inp, &e.kind, &style);
        if e.style.color.is_none() && matches!(e.kind, ElementKind::SectionName | ElementKind::NextSection) {
            if let CellContent::Text { text, .. } = &content { style.color = section_color(text).into(); }
        }
        Cell { element: e.id, rect: e.rect, style, content }
    }).collect();
    shared_rect_priority(&mut cells);
    let has_message_box = layout.elements.iter().filter_map(|e| e.known()).any(|e| e.kind == ElementKind::StageMessage);
    let message = &inp.live.stage_message;
    StageView {
        layout_id: layout.id,
        background: layout.background.clone(),
        font: layout.font.clone(),
        cells,
        message_fallback: (!message.is_empty() && !has_message_box).then(|| message.clone()),
    }
}

/// Boxes in exactly the same place: the first one with something to show wins, the rest go empty.
pub fn shared_rect_priority(cells: &mut [Cell]) {
    for i in 0..cells.len() {
        if cells[i].content == CellContent::Empty { continue; }
        let rect = cells[i].rect;
        if cells[..i].iter().any(|c| c.rect == rect && c.content != CellContent::Empty) { cells[i].content = CellContent::Empty; }
    }
}

/// A slide as the stage names it: its words, or what kind of picture or media it is.
pub fn label(slide: &Slide) -> String {
    match &slide.content {
        SlideContent::Text { text, parallel, .. } => crate::model::all_bibles_text(text, parallel.as_ref()),
        SlideContent::Image { .. } => s::SLIDE_PICTURE.into(),
        SlideContent::Video { .. } => s::VIDEO.into(),
        SlideContent::Still { .. } => s::PICTURE.into(),
        SlideContent::Audio { .. } => s::AUDIO.into(),
    }
}

/// A slide's words, or its picture when the box shows pictures.
fn slide_content(slide: &Slide, pictures: bool, caption: Option<String>) -> CellContent {
    match &slide.content {
        SlideContent::Image { asset } | SlideContent::Still { asset } if pictures => CellContent::Picture { asset: asset.clone(), caption },
        _ => CellContent::text(label(slide), caption),
    }
}

/// A song slide's section: its label without the page count ("Verse 1 (1/2)" → "Verse 1").
pub fn section_of(label: &str) -> &str {
    let Some(open) = label.strip_suffix(')').and_then(|l| l.rfind(" (").map(|i| (i, &l[i + 2..]))) else { return label };
    let (at, inside) = open;
    let pages = inside.split_once('/').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit()));
    if pages { &label[..at] } else { label }
}

/// The colour of a song section's name (a stand-in until FEAT-4's section colours).
pub fn section_color(label: &str) -> &'static str {
    let l = label.to_ascii_lowercase();
    if l.starts_with("pre-chorus") || l.starts_with("prechorus") { "#c792ea" }
    else if l.starts_with("chorus") { "#ff9f6b" }
    else if l.starts_with("verse") { "#6fb3ff" }
    else if l.starts_with("bridge") { "#5fd3a8" }
    else { "#9aa3b5" }
}

fn is_song(live: &LiveView) -> bool { live.item.as_ref().is_some_and(|i| i.kind == "song") }

/// `−m:ss`, never below zero.
pub fn countdown(left: f64) -> String { let s = left.max(0.0).round() as u64; format!("−{}:{:02}", s / 60, s % 60) }

pub fn clock(t: chrono::NaiveTime, seconds: bool, h24: bool) -> String {
    let f = match (h24, seconds) { (false, false) => "%-I:%M %p", (false, true) => "%-I:%M:%S %p", (true, false) => "%H:%M", (true, true) => "%H:%M:%S" };
    t.format(f).to_string()
}

fn content(inp: &StageInputs, kind: &ElementKind, style: &ResolvedStyle) -> CellContent {
    let live = inp.live;
    let caption = style.caption.clone();
    match kind {
        ElementKind::CurrentSlide { pictures, keep_after_clear } => {
            let f = &live.frame;
            let on_air = f.slide.is_some() || f.media.is_some();
            match &live.slide {
                Some(slide) if on_air => slide_content(slide, *pictures, caption),
                _ if f.logo => CellContent::Text { text: s::LOGO.into(), dim: true, caption, red: false },
                // Clear Slide leaves the background (or media) up; Clear All leaves nothing.
                Some(slide) if *keep_after_clear && f.background.is_some() => CellContent::Text { text: label(slide), dim: true, caption, red: false },
                _ => CellContent::Text { text: String::new(), dim: true, caption, red: false },
            }
        }
        ElementKind::NextSlide { pictures } => match &live.next {
            Some(n) => {
                let other_item = live.item.as_ref().is_none_or(|i| i.title != n.title);
                slide_content(&n.slide, *pictures, caption.map(|c| if other_item { s::next_item(&n.title) } else { c }))
            }
            None => CellContent::text(s::END_OF_SERVICE, caption),
        },
        ElementKind::SectionName => match &live.slide {
            Some(slide) if is_song(live) => CellContent::text(section_of(&slide.label), caption),
            _ => CellContent::Empty,
        },
        ElementKind::NextSection => {
            let at = live.cursor.as_ref().map(|c| c.slide);
            match (at, is_song(live)) {
                (Some(i), true) if i < inp.slides.len() => {
                    let here = section_of(&inp.slides[i].label);
                    inp.slides[i + 1..].iter().map(|s| section_of(&s.label)).find(|l| *l != here)
                        .map_or(CellContent::Empty, |l| CellContent::text(l, caption))
                }
                _ => CellContent::Empty,
            }
        }
        ElementKind::ItemTitle => CellContent::text(live.item.as_ref().map_or(s::NOTHING_LIVE, |i| i.title.as_str()), caption),
        ElementKind::SlidesLeft => match (&live.item, &live.cursor) {
            (Some(item), Some(c)) => CellContent::text(s::slides_left(item.count.saturating_sub(c.slide + 1)), caption),
            _ => CellContent::Empty,
        },
        ElementKind::Notes => match &live.slide {
            Some(slide) if !slide.notes.is_empty() => CellContent::text(slide.notes.clone(), caption),
            _ => CellContent::Empty,
        },
        ElementKind::Clock { seconds, h24 } => CellContent::text(clock(inp.now, *seconds, *h24), caption),
        ElementKind::Timer { timer } => {
            let found = match timer {
                TimerRef::Current => inp.state.current(inp.timers),
                TimerRef::Id { id } => match inp.timers.timers.iter().find(|t| t.id == *id) {
                    Some(t) => Some(t),
                    None => return CellContent::Text { text: s::TIMER_REMOVED.into(), dim: true, caption, red: false },
                },
            };
            found.map_or(CellContent::Empty, |t| timer_cell(t, caption))
        }
        ElementKind::MediaTimeLeft => live.media.as_ref().or(live.audio.as_ref()).filter(|m| m.duration > 0.0)
            .map_or(CellContent::Empty, |m| CellContent::text(countdown(m.duration - m.position), caption)),
        ElementKind::StageMessage if !live.stage_message.is_empty() => CellContent::Message { text: live.stage_message.clone() },
        ElementKind::StageMessage => CellContent::Empty,
        ElementKind::Text { text } => CellContent::text(text.clone(), caption),
    }
}

/// A timer's time from `timers::text`; red when over or finished, dim with "· paused" when paused.
/// An empty caption means "the timer's name".
fn timer_cell(t: &TimerSnapshot, caption: Option<String>) -> CellContent {
    let paused = t.state == TimerState::Paused;
    let caption = caption.map(|c| {
        let c = if c.is_empty() { t.name.clone() } else { c };
        if paused { s::paused(&c) } else { c }
    });
    CellContent::Text { text: timers::text(t.value_ms), dim: paused, caption, red: t.overrun || t.state == TimerState::Finished }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::{LiveItemSummary, NextUp}, live::*, model::{Background, EndAction, Theme}, stage::{presets, Align, ElementStyle, StageElement, BACKGROUND}, timers::TimerKind};
    use crate::lenient::Lenient;

    fn text(t: &str) -> Slide { Slide { label: t.into(), content: SlideContent::Text { text: t.into(), footer: None, reference: None, size: 0, parallel: None }, notes: String::new() } }
    fn labelled(label: &str, t: &str) -> Slide { Slide { label: label.into(), ..text(t) } }
    fn slide(content: SlideContent) -> Slide { Slide { label: String::new(), content, notes: String::new() } }
    fn on_air(s: &Slide) -> Option<SlideLayer> {
        match &s.content {
            SlideContent::Text { text, .. } => Some(SlideLayer::Text { text: text.clone(), footer: None, reference: None, scripture: false, size: 0, passage: vec![], parallel: None, theme: None }),
            _ => None,
        }
    }

    /// `slides[at]` of song "Amazing Grace" live, with the slide after it as Next.
    fn song_view(slides: &[Slide], at: usize) -> LiveView {
        let now = slides[at].clone();
        LiveView {
            version: 1, committed: 1, pending: false,
            frame: Frame { slide: on_air(&now), background: None, ..Frame::default() },
            cursor: Some(Cursor { item_id: Uuid::nil(), item_index: 0, slide: at }),
            item: Some(LiveItemSummary { item_id: Uuid::nil(), title: "Amazing Grace".into(), kind: "song".into(), count: slides.len() }),
            slide: Some(now),
            next: slides.get(at + 1).map(|s| NextUp { title: "Amazing Grace".into(), slide: s.clone() }),
            media: None, audio: None, notice: None, theme: Theme::default(), logo: None, stage_message: String::new(), clear_epoch: 0,
        }
    }
    fn empty_view() -> LiveView {
        let mut v = song_view(&[text("x")], 0);
        (v.frame, v.cursor, v.item, v.slide, v.next) = (Frame::default(), None, None, None, None);
        v
    }
    fn song() -> Vec<Slide> {
        vec![labelled("Verse 1 (1/2)", "Amazing grace"), labelled("Verse 1 (2/2)", "That saved a wretch"), labelled("Chorus", "My chains are gone"), labelled("Verse 2", "Twas grace")]
    }

    const NOON: fn() -> chrono::NaiveTime = || chrono::NaiveTime::from_hms_opt(9, 5, 30).unwrap();

    fn build_with(live: &LiveView, slides: &[Slide], timers: &TimersView, state: &StageState, layout: &StageLayout) -> StageView {
        build(&StageInputs { live, slides, timers, state, now: NOON() }, layout)
    }
    fn view_of(live: &LiveView, slides: &[Slide], layout: &StageLayout) -> StageView {
        build_with(live, slides, &TimersView::default(), &StageState::default(), layout)
    }
    /// A one-box layout.
    fn one(kind: ElementKind) -> StageLayout {
        let e = StageElement { id: Uuid::from_u128(1), kind, rect: PctRect::new(0.0, 0.0, 50.0, 50.0), style: ElementStyle::default() };
        StageLayout { id: Uuid::from_u128(100), name: "One".into(), background: BACKGROUND.into(), font: String::new(), elements: vec![Lenient::Known(e)] }
    }
    fn only(live: &LiveView, slides: &[Slide], kind: ElementKind) -> CellContent { view_of(live, slides, &one(kind)).cells.remove(0).content }
    fn words(c: &CellContent) -> (&str, bool) {
        match c { CellContent::Text { text, dim, .. } => (text.as_str(), *dim), other => panic!("not text: {other:?}") }
    }
    fn cell_of<'a>(v: &'a StageView, kind: &ElementKind, layout: &StageLayout) -> &'a CellContent {
        let id = layout.elements.iter().filter_map(|e| e.known()).find(|e| &e.kind == kind).unwrap().id;
        &v.cells.iter().find(|c| c.element == id).unwrap().content
    }
    const CUR: ElementKind = ElementKind::CurrentSlide { pictures: false, keep_after_clear: false };

    #[test]
    fn current_and_next_slide_follow_the_live_view() {
        let v = song_view(&song(), 0);
        assert_eq!(words(&only(&v, &song(), CUR)), ("Amazing grace", false));
        let next = only(&v, &song(), ElementKind::NextSlide { pictures: false });
        assert_eq!(next, CellContent::Text { text: "That saved a wretch".into(), dim: false, caption: Some("NEXT".into()), red: false });
    }

    #[test]
    fn classic_current_slide_empty_and_dim_after_clear_slide() {
        let mut v = song_view(&song(), 0);
        v.frame.slide = None; // Clear Slide: the cursor's slide is still known but nothing is on air
        assert_eq!(words(&only(&v, &song(), CUR)), ("", true));
        v.frame.logo = true;
        assert_eq!(words(&only(&v, &song(), CUR)), ("(logo)", true));
    }

    #[test]
    fn keep_after_clear_shows_dim_words_after_clear_slide_only() {
        let keep = ElementKind::CurrentSlide { pictures: false, keep_after_clear: true };
        let mut v = song_view(&song(), 0);
        v.frame.slide = None;
        v.frame.background = Some(Inst { inst: 1, v: Background::Color { color: "#000".into() } });
        assert_eq!(words(&only(&v, &song(), keep.clone())), ("Amazing grace", true));
        v.frame.background = None; // Clear All
        assert_eq!(words(&only(&v, &song(), keep)), ("", true));
    }

    #[test]
    fn clear_all_always_empties_current_slide() {
        let v = empty_view();
        assert_eq!(words(&only(&v, &[], CUR)), ("", true));
        assert_eq!(words(&only(&v, &[], ElementKind::ItemTitle)), ("Nothing live", false));
        assert_eq!(words(&only(&v, &[], ElementKind::NextSlide { pictures: false })), ("End of service", false));
    }

    #[test]
    fn classic_picture_slide_shows_label_and_pictures_option_shows_picture() {
        let pic = slide(SlideContent::Image { asset: "a.png".into() });
        let mut v = song_view(&[pic.clone()], 0);
        v.frame.media = Some(Inst { inst: 1, v: MediaLayer::Still { asset: "a.png".into() } });
        assert_eq!(words(&only(&v, &[], CUR)), ("(slide picture)", false));
        let with = only(&v, &[], ElementKind::CurrentSlide { pictures: true, keep_after_clear: false });
        assert_eq!(with, CellContent::Picture { asset: "a.png".into(), caption: None });
        let video = slide(SlideContent::Video { asset: "v.mp4".into(), autoplay: true, end: EndAction::Stop });
        v.slide = Some(video);
        assert_eq!(words(&only(&v, &[], ElementKind::CurrentSlide { pictures: true, keep_after_clear: false })), ("(video)", false));
    }

    #[test]
    fn next_slide_names_next_item_when_crossing() {
        let mut v = song_view(&song(), 3);
        v.next = Some(NextUp { title: "Welcome".into(), slide: text("Good morning") });
        let c = only(&v, &song(), ElementKind::NextSlide { pictures: false });
        assert_eq!(c, CellContent::Text { text: "Good morning".into(), dim: false, caption: Some("NEXT · Welcome".into()), red: false });
    }

    #[test]
    fn section_name_from_label_songs_only() {
        let v = song_view(&song(), 1);
        assert_eq!(words(&only(&v, &song(), ElementKind::SectionName)), ("Verse 1", false));
        let mut deck = v.clone();
        deck.item.as_mut().unwrap().kind = "text".into();
        assert_eq!(only(&deck, &song(), ElementKind::SectionName), CellContent::Empty);
        assert_eq!(section_of("Chorus (10/12)"), "Chorus");
        assert_eq!(section_of("John 3:16 (cont.)"), "John 3:16 (cont.)");
        assert_eq!(section_of("Tag (end)"), "Tag (end)");
    }

    #[test]
    fn section_name_uses_section_colour_unless_the_box_sets_one() {
        let v = song_view(&song(), 2);
        let c = &view_of(&v, &song(), &one(ElementKind::SectionName)).cells[0];
        assert_eq!(c.style.color, section_color("Chorus"));
        let mut own = one(ElementKind::SectionName);
        own.elements[0].known_mut().unwrap().style.color = Some("#123456".into());
        assert_eq!(view_of(&v, &song(), &own).cells[0].style.color, "#123456");
    }

    #[test]
    fn next_section_skips_pages_of_the_same_section() {
        let s = song();
        assert_eq!(words(&only(&song_view(&s, 0), &s, ElementKind::NextSection)), ("Chorus", false));
        assert_eq!(words(&only(&song_view(&s, 1), &s, ElementKind::NextSection)), ("Chorus", false));
        assert_eq!(words(&only(&song_view(&s, 2), &s, ElementKind::NextSection)), ("Verse 2", false));
        assert_eq!(only(&song_view(&s, 3), &s, ElementKind::NextSection), CellContent::Empty, "last section");
        assert_eq!(only(&song_view(&s, 0), &[], ElementKind::NextSection), CellContent::Empty, "slides unknown");
        let c = only(&song_view(&s, 0), &s, ElementKind::NextSection);
        assert!(matches!(c, CellContent::Text { caption: Some(ref c), .. } if c == "NEXT SECTION"));
    }

    #[test]
    fn slides_left_counts_down_the_item() {
        let s = song();
        assert_eq!(words(&only(&song_view(&s, 0), &s, ElementKind::SlidesLeft)), ("3 slides left", false));
        assert_eq!(words(&only(&song_view(&s, 2), &s, ElementKind::SlidesLeft)), ("1 slide left", false));
        assert_eq!(words(&only(&song_view(&s, 3), &s, ElementKind::SlidesLeft)), ("Last slide", false));
        assert_eq!(only(&empty_view(), &[], ElementKind::SlidesLeft), CellContent::Empty);
    }

    #[test]
    fn notes_show_the_slides_notes() {
        let mut s = song();
        s[0].notes = "Slow, hold the last line".into();
        assert_eq!(words(&only(&song_view(&s, 0), &s, ElementKind::Notes)), ("Slow, hold the last line", false));
        assert_eq!(only(&song_view(&s, 1), &s, ElementKind::Notes), CellContent::Empty);
    }

    #[test]
    fn clock_formats_12h_24h_seconds() {
        let t = chrono::NaiveTime::from_hms_opt(18, 5, 30).unwrap();
        assert_eq!(clock(t, false, false), "6:05 PM");
        assert_eq!(clock(t, true, false), "6:05:30 PM");
        assert_eq!(clock(t, false, true), "18:05");
        assert_eq!(clock(t, true, true), "18:05:30");
        assert_eq!(words(&only(&empty_view(), &[], ElementKind::Clock { seconds: false, h24: false })), ("9:05 AM", false));
    }

    fn snap(n: u128, state: TimerState, value_ms: i64, overrun: bool) -> TimerSnapshot {
        TimerSnapshot { id: Uuid::from_u128(n), name: "Sermon".into(), kind: TimerKind::Duration { secs: 1800 }, state, value_ms, overrun, on_stage: true, on_remote: false }
    }
    fn timer_content(t: TimerSnapshot, r: TimerRef) -> CellContent {
        let timers = TimersView { timers: vec![t], auto: None };
        build_with(&empty_view(), &[], &timers, &StageState::default(), &one(ElementKind::Timer { timer: r })).cells.remove(0).content
    }

    #[test]
    fn timer_text_comes_from_timers_text() {
        let c = timer_content(snap(1, TimerState::Running, 754_000, false), TimerRef::Current);
        assert_eq!(c, CellContent::Text { text: timers::text(754_000), dim: false, caption: Some("Sermon".into()), red: false });
    }

    #[test]
    fn current_timer_shows_overrun_and_finished_red() {
        let over = timer_content(snap(1, TimerState::Running, -65_000, true), TimerRef::Current);
        assert!(matches!(over, CellContent::Text { ref text, red: true, .. } if text == "+1:05"));
        let done = timer_content(snap(1, TimerState::Finished, 0, false), TimerRef::Current);
        assert!(matches!(done, CellContent::Text { red: true, .. }));
    }

    #[test]
    fn timer_paused_is_dim_with_caption() {
        let c = timer_content(snap(1, TimerState::Paused, 60_000, false), TimerRef::Current);
        assert_eq!(c, CellContent::Text { text: "1:00".into(), dim: true, caption: Some("Sermon · paused".into()), red: false });
    }

    #[test]
    fn missing_timer_says_removed_and_no_current_is_empty() {
        let c = timer_content(snap(1, TimerState::Running, 0, false), TimerRef::Id { id: Uuid::from_u128(7) });
        assert_eq!(words(&c), ("Timer removed", true));
        assert_eq!(timer_content(snap(1, TimerState::Stopped, 0, false), TimerRef::Current), CellContent::Empty);
        // By id, a stopped timer still shows its start value.
        assert_eq!(words(&timer_content(snap(1, TimerState::Stopped, 1_800_000, false), TimerRef::Id { id: Uuid::from_u128(1) })), ("30:00", false));
    }

    #[test]
    fn media_time_left_matches_todays_countdown() {
        let mut v = song_view(&[slide(SlideContent::Video { asset: "v.mp4".into(), autoplay: true, end: EndAction::Stop })], 0);
        let k = || ElementKind::MediaTimeLeft;
        v.media = Some(PlayStatus { inst: 7, state: PlayState::Playing, position: 10.4, duration: 90.0 });
        assert_eq!(words(&only(&v, &[], k())), ("−1:20", false));
        v.media.as_mut().unwrap().position = 95.0;
        assert_eq!(words(&only(&v, &[], k())), ("−0:00", false));
        v.media = None;
        v.audio = Some(PlayStatus { inst: 8, state: PlayState::Playing, position: 0.0, duration: 0.0 });
        assert_eq!(only(&v, &[], k()), CellContent::Empty);
        v.audio.as_mut().unwrap().duration = 125.0;
        assert_eq!(words(&only(&v, &[], k())), ("−2:05", false));
    }

    #[test]
    fn classic_media_wins_shared_rect_over_timer() {
        let classic = presets::classic();
        let timers = TimersView { timers: vec![snap(1, TimerState::Running, 60_000, false)], auto: None };
        let mut v = song_view(&song(), 0);
        let timer = ElementKind::Timer { timer: TimerRef::Current };
        let out = build_with(&v, &song(), &timers, &StageState::default(), classic);
        assert_eq!(words(cell_of(&out, &timer, classic)), ("1:00", false), "no media: the timer shows");
        v.media = Some(PlayStatus { inst: 7, state: PlayState::Playing, position: 0.0, duration: 30.0 });
        let out = build_with(&v, &song(), &timers, &StageState::default(), classic);
        assert_eq!(words(cell_of(&out, &ElementKind::MediaTimeLeft, classic)), ("−0:30", false));
        assert_eq!(cell_of(&out, &timer, classic), &CellContent::Empty);
    }

    #[test]
    fn message_fills_message_cell_over_notes() {
        let classic = presets::classic();
        let mut s = song();
        s[0].notes = "Slow".into();
        let mut v = song_view(&s, 0);
        v.stage_message = "Last chorus".into();
        let out = view_of(&v, &s, classic);
        assert_eq!(cell_of(&out, &ElementKind::StageMessage, classic), &CellContent::Message { text: "Last chorus".into() });
        assert_eq!(cell_of(&out, &ElementKind::Notes, classic), &CellContent::Empty, "the message takes the notes' place");
        assert_eq!(out.message_fallback, None);
        v.stage_message.clear();
        let out = view_of(&v, &s, classic);
        assert_eq!(cell_of(&out, &ElementKind::StageMessage, classic), &CellContent::Empty);
        assert_eq!(words(cell_of(&out, &ElementKind::Notes, classic)), ("Slow", false));
    }

    #[test]
    fn message_without_message_box_sets_fallback() {
        let mut v = song_view(&song(), 0);
        v.stage_message = "Wrap up".into();
        assert_eq!(view_of(&v, &song(), &one(CUR)).message_fallback.as_deref(), Some("Wrap up"));
        assert_eq!(view_of(&v, &song(), presets::by_id(presets::BAND).unwrap()).message_fallback, None);
    }

    #[test]
    fn clears_never_touch_message_timer_clock_notes() {
        let band = presets::by_id(presets::BAND).unwrap();
        let mut s = song();
        s[2].notes = "Big".into();
        let mut v = song_view(&s, 2);
        v.stage_message = "Go".into();
        let before = view_of(&v, &s, band);
        v.frame.slide = None;
        let after = view_of(&v, &s, band);
        for (a, b) in before.cells.iter().zip(&after.cells) {
            if a.element != band.elements[3].known().unwrap().id { assert_eq!(a, b); } // only the current slide changes
        }
        assert_eq!(words(cell_of(&after, &CUR, band)), ("", true));
    }

    #[test]
    fn unknown_element_draws_nothing() {
        let mut l = one(ElementKind::ItemTitle);
        l.elements.insert(0, Lenient::Unknown(serde_json::json!({"kind": {"type": "hologram"}})));
        let out = view_of(&empty_view(), &[], &l);
        assert_eq!(out.cells.len(), 1);
        assert_eq!(out.layout_id, l.id);
    }

    #[test]
    fn styles_reach_the_cells() {
        let classic = presets::classic();
        let out = view_of(&song_view(&song(), 0), &song(), classic);
        assert_eq!(out.cells.len(), classic.elements.len());
        assert_eq!(out.background, "#07080a");
        let title = &out.cells[0];
        assert_eq!((title.style.color.as_str(), title.style.align), ("#9aa3b5", Align::Left));
    }
}
