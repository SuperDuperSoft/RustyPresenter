//! The built-in stage layouts (made in code, never stored) and every box kind's default look.
use super::{strings as s, Align, ElementKind, ElementStyle, PctRect, ResolvedStyle, StageElement, StageLayout, TimerRef, VAlign, BACKGROUND};
use crate::lenient::Lenient;
use std::sync::OnceLock;
use uuid::Uuid;

pub const CLASSIC: Uuid = Uuid::from_u128(0x5ea6e_0001_0000_0000_0000_0000_0000);
pub const BAND: Uuid = Uuid::from_u128(0x5ea6e_0002_0000_0000_0000_0000_0000);
pub const SPEAKER: Uuid = Uuid::from_u128(0x5ea6e_0003_0000_0000_0000_0000_0000);
pub const LYRICS_ONLY: Uuid = Uuid::from_u128(0x5ea6e_0004_0000_0000_0000_0000_0000);
pub const CLOCK_COUNTDOWN: Uuid = Uuid::from_u128(0x5ea6e_0005_0000_0000_0000_0000_0000);

/// Today's colours, shared by the presets.
const GREY: &str = "#9aa3b5";
const RED: &str = "#ff6b6b";
const GOLD: &str = "#f5c66b";
const NOTE_BLUE: &str = "#cfe3ff";
const NOTE_FILL: &str = "#12151b";

/// Every kind's defaults: white, centred, bold, 3 % floor, no caption except Next, Next section and Timer.
pub fn kind_defaults(kind: &ElementKind) -> ResolvedStyle {
    use ElementKind::*;
    let max_size = match kind {
        CurrentSlide { .. } => 9.0,
        NextSlide { .. } | StageMessage => 6.0,
        Clock { .. } => 8.0,
        Timer { .. } => 10.0,
        Text { .. } => 5.0,
        SectionName | NextSection | ItemTitle | SlidesLeft | Notes | MediaTimeLeft => 4.0,
    };
    let caption = match kind {
        NextSlide { .. } => Some(s::next()),
        NextSection => Some(s::next_section()),
        Timer { .. } => Some(String::new()), // the timer's name, filled in by the view
        _ => None,
    };
    ResolvedStyle { color: "#ffffff".into(), max_size, min_size: 3.0, align: Align::Center, valign: VAlign::Middle, bold: true, caption, fill: None }
}

/// A style with the kind's defaults under the box's own choices.
pub fn resolve_style(kind: &ElementKind, st: &ElementStyle) -> ResolvedStyle {
    let d = kind_defaults(kind);
    let caption = match st.caption {
        Some(false) => None,
        Some(true) => Some(st.caption_text.clone().or(d.caption).unwrap_or_default()),
        None => d.caption.map(|c| st.caption_text.clone().unwrap_or(c)),
    };
    let max_size = st.max_size.unwrap_or(d.max_size);
    ResolvedStyle {
        color: st.color.clone().unwrap_or(d.color),
        max_size,
        min_size: st.min_size.unwrap_or(d.min_size).min(max_size),
        align: st.align.unwrap_or(d.align),
        valign: st.valign.unwrap_or(d.valign),
        bold: st.bold.unwrap_or(d.bold),
        caption,
        fill: st.fill.clone().or(d.fill),
    }
}

/// The built-ins in menu order: Classic first.
pub fn builtins() -> &'static [StageLayout] {
    static ALL: OnceLock<Vec<StageLayout>> = OnceLock::new();
    ALL.get_or_init(|| vec![classic_layout(), lyrics_only_layout(), band_layout(), speaker_layout(), clock_countdown_layout()])
}

/// A built-in layout by id.
pub fn by_id(id: Uuid) -> Option<&'static StageLayout> { builtins().iter().find(|l| l.id == id) }

pub fn is_builtin(id: Uuid) -> bool { by_id(id).is_some() }

/// A layout by id: the built-ins first, then the church's own. A church layout with a built-in's id is ignored.
pub fn find_layout(church: &[StageLayout], id: Uuid) -> Option<&StageLayout> {
    by_id(id).or_else(|| church.iter().find(|l| l.id == id))
}

/// Every layout for a menu: the built-ins, then the church's own.
pub fn all_layouts(church: &[StageLayout]) -> impl Iterator<Item = &StageLayout> {
    builtins().iter().chain(church.iter().filter(|l| !is_builtin(l.id)))
}

/// One box of every kind, in `ElementKind` order (the editor's Add menu; names in `strings::kind_names`).
pub fn all_kinds() -> [ElementKind; 12] {
    use ElementKind::*;
    [CurrentSlide { pictures: false, keep_after_clear: false }, NextSlide { pictures: false }, SectionName, NextSection, ItemTitle,
     SlidesLeft, Notes, Clock { seconds: false, h24: false }, current_timer(), MediaTimeLeft, StageMessage, Text { text: String::new() }]
}

/// Where `kind` sits in `all_kinds` (and `strings::kind_names`).
pub fn kind_index(kind: &ElementKind) -> usize {
    use ElementKind::*;
    match kind {
        CurrentSlide { .. } => 0, NextSlide { .. } => 1, SectionName => 2, NextSection => 3, ItemTitle => 4, SlidesLeft => 5,
        Notes => 6, Clock { .. } => 7, Timer { .. } => 8, MediaTimeLeft => 9, StageMessage => 10, Text { .. } => 11,
    }
}

/// A new box of `kind`, 30 % × 15 % in the middle of the window, with the kind's look.
pub fn new_box(kind: ElementKind, id: Uuid) -> StageElement {
    StageElement { id, kind, rect: super::geom::new_box_rect(), style: ElementStyle::default() }
}

pub fn classic() -> &'static StageLayout { &builtins()[0] }

/// A box builder for the tables below.
struct B { layout: Uuid, n: u128, out: Vec<Lenient<StageElement>> }

impl B {
    fn new(layout: Uuid) -> Self { B { layout, n: 0, out: vec![] } }
    fn add(&mut self, kind: ElementKind, (x, y, w, h): (f32, f32, f32, f32), style: ElementStyle) -> &mut Self {
        self.n += 1;
        let id = Uuid::from_u128(self.layout.as_u128() + self.n);
        self.out.push(Lenient::Known(StageElement { id, kind, rect: PctRect::new(x, y, w, h), style }));
        self
    }
    fn done(&mut self, name: &str) -> StageLayout {
        StageLayout { id: self.layout, name: name.into(), background: BACKGROUND.into(), font: String::new(), elements: std::mem::take(&mut self.out) }
    }
}

fn st(f: impl FnOnce(&mut ElementStyle)) -> ElementStyle { let mut s = ElementStyle::default(); f(&mut s); s }
fn color(c: &str) -> Option<String> { Some(c.into()) }
fn current_timer() -> ElementKind { ElementKind::Timer { timer: TimerRef::Current } }

/// Today's stage: title and section on top, media time or timer and the clock on the right, the slide,
/// the message (or the notes) and Next below. Boxes in one place: the first with something to show wins.
fn classic_layout() -> StageLayout {
    use ElementKind::*;
    B::new(CLASSIC)
        .add(ItemTitle, (3.0, 2.0, 44.0, 6.0), st(|s| { s.color = color(GREY); s.align = Some(Align::Left); s.bold = Some(false); }))
        .add(SectionName, (48.0, 2.0, 15.0, 6.0), st(|s| s.align = Some(Align::Left)))
        .add(MediaTimeLeft, (64.0, 2.0, 13.0, 6.0), st(|s| { s.color = color(RED); s.align = Some(Align::Right); }))
        .add(current_timer(), (64.0, 2.0, 13.0, 6.0), st(|s| { s.color = color(RED); s.align = Some(Align::Right); s.max_size = Some(4.0); s.caption = Some(false); }))
        .add(Clock { seconds: false, h24: false }, (78.0, 1.0, 19.0, 8.0), st(|s| s.align = Some(Align::Right)))
        .add(CurrentSlide { pictures: false, keep_after_clear: false }, (3.0, 10.0, 94.0, 52.0), ElementStyle::default())
        .add(StageMessage, (3.0, 63.0, 94.0, 11.0), ElementStyle::default())
        .add(Notes, (3.0, 63.0, 94.0, 11.0), st(|s| { s.color = color(NOTE_BLUE); s.fill = color(NOTE_FILL); s.align = Some(Align::Left); s.bold = Some(false); }))
        .add(NextSlide { pictures: false }, (3.0, 76.0, 94.0, 22.0), st(|s| { s.color = color(GOLD); s.align = Some(Align::Left); s.valign = Some(VAlign::Top); s.bold = Some(false); }))
        .done(&s::classic())
}

/// Just the words, big, and what comes next.
fn lyrics_only_layout() -> StageLayout {
    use ElementKind::*;
    B::new(LYRICS_ONLY)
        .add(CurrentSlide { pictures: false, keep_after_clear: false }, (3.0, 3.0, 94.0, 62.0), st(|s| s.max_size = Some(12.0)))
        .add(NextSlide { pictures: false }, (3.0, 68.0, 94.0, 29.0), st(|s| s.color = color(GOLD)))
        .add(StageMessage, (3.0, 3.0, 94.0, 14.0), ElementStyle::default())
        .done(&s::lyrics_only())
}

/// A big clock and the current countdown, for the back of the room or a speaker's timer screen.
fn clock_countdown_layout() -> StageLayout {
    use ElementKind::*;
    B::new(CLOCK_COUNTDOWN)
        .add(Clock { seconds: false, h24: false }, (5.0, 8.0, 90.0, 40.0), st(|s| s.max_size = Some(30.0)))
        .add(current_timer(), (5.0, 52.0, 90.0, 30.0), st(|s| s.max_size = Some(22.0)))
        .add(StageMessage, (5.0, 84.0, 90.0, 13.0), ElementStyle::default())
        .done(&s::clock_countdown())
}

/// For singers and the band: where they are in the song and what comes next, big words, the clock.
fn band_layout() -> StageLayout {
    use ElementKind::*;
    B::new(BAND)
        .add(SectionName, (3.0, 2.0, 30.0, 9.0), st(|s| { s.align = Some(Align::Left); s.max_size = Some(6.0); }))
        .add(NextSection, (35.0, 2.0, 30.0, 9.0), st(|s| { s.color = color(GOLD); s.max_size = Some(5.0); }))
        .add(Clock { seconds: false, h24: false }, (70.0, 2.0, 27.0, 9.0), st(|s| { s.align = Some(Align::Right); s.max_size = Some(6.0); }))
        .add(CurrentSlide { pictures: false, keep_after_clear: false }, (3.0, 13.0, 94.0, 50.0), st(|s| s.max_size = Some(10.0)))
        .add(NextSlide { pictures: false }, (3.0, 65.0, 62.0, 22.0), st(|s| { s.color = color(GOLD); s.align = Some(Align::Left); s.valign = Some(VAlign::Top); }))
        .add(SlidesLeft, (68.0, 65.0, 29.0, 10.0), st(|s| { s.color = color(GREY); s.align = Some(Align::Right); }))
        .add(MediaTimeLeft, (68.0, 76.0, 29.0, 11.0), st(|s| { s.color = color(RED); s.align = Some(Align::Right); s.max_size = Some(6.0); }))
        .add(current_timer(), (68.0, 76.0, 29.0, 11.0), st(|s| { s.color = color(RED); s.align = Some(Align::Right); s.max_size = Some(6.0); s.caption = Some(false); }))
        .add(StageMessage, (3.0, 88.0, 94.0, 10.0), ElementStyle::default())
        .done(&s::band())
}

/// For the preacher: the clock, a big countdown, the room's slide (pictures too) and the notes.
fn speaker_layout() -> StageLayout {
    use ElementKind::*;
    B::new(SPEAKER)
        .add(Clock { seconds: false, h24: false }, (3.0, 2.0, 30.0, 14.0), st(|s| s.align = Some(Align::Left)))
        .add(current_timer(), (60.0, 2.0, 37.0, 20.0), st(|s| { s.align = Some(Align::Right); s.max_size = Some(18.0); }))
        .add(CurrentSlide { pictures: true, keep_after_clear: false }, (3.0, 24.0, 45.0, 40.0), st(|s| s.max_size = Some(6.0)))
        .add(Notes, (51.0, 24.0, 46.0, 56.0), st(|s| { s.color = color(NOTE_BLUE); s.fill = color(NOTE_FILL); s.align = Some(Align::Left); s.valign = Some(VAlign::Top); s.max_size = Some(6.0); s.bold = Some(false); }))
        .add(SlidesLeft, (3.0, 66.0, 45.0, 14.0), st(|s| s.color = color(GREY)))
        .add(StageMessage, (3.0, 82.0, 94.0, 16.0), ElementStyle::default())
        .done(&s::speaker())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(l: &StageLayout) -> Vec<(&ElementKind, PctRect)> { l.elements.iter().filter_map(|e| e.known()).map(|e| (&e.kind, e.rect)).collect() }

    #[test]
    fn builtin_ids_are_stable() {
        assert_eq!(CLASSIC.to_string(), "0005ea6e-0001-0000-0000-000000000000");
        assert_eq!(builtins().iter().map(|l| l.id).collect::<Vec<_>>(), vec![CLASSIC, LYRICS_ONLY, BAND, SPEAKER, CLOCK_COUNTDOWN]);
        assert_eq!(CLOCK_COUNTDOWN.to_string(), "0005ea6e-0005-0000-0000-000000000000");
        assert_eq!(classic().id, CLASSIC);
        // Box ids are fixed too, and unique across all layouts.
        let mut ids: Vec<Uuid> = builtins().iter().flat_map(|l| l.elements.iter().map(|e| e.known().unwrap().id)).collect();
        assert_eq!(ids[0], Uuid::from_u128(CLASSIC.as_u128() + 1));
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn builtin_names_are_unique() {
        let names: Vec<&str> = builtins().iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["Classic", "Lyrics only", "Band", "Speaker", "Clock and countdown"]);
    }

    #[test]
    fn every_builtin_passes_sanitise_unchanged() {
        for l in builtins() {
            for (k, r) in kinds(l) { assert_eq!(r.sanitised(), r, "{} {k:?}", l.name); }
            assert!(l.elements.len() <= 24);
        }
    }

    #[test]
    fn classic_matches_todays_stage_regions() {
        let k = kinds(classic());
        let at = |want: &ElementKind| k.iter().find(|(kind, _)| *kind == want).map(|(_, r)| *r).unwrap();
        assert_eq!(at(&ElementKind::CurrentSlide { pictures: false, keep_after_clear: false }), PctRect::new(3.0, 10.0, 94.0, 52.0));
        assert_eq!(at(&ElementKind::Notes), PctRect::new(3.0, 63.0, 94.0, 11.0));
        assert_eq!(at(&ElementKind::NextSlide { pictures: false }), PctRect::new(3.0, 76.0, 94.0, 22.0));
        // Media time is listed before the timer in the same box, so media wins it.
        let media = k.iter().position(|(kind, _)| **kind == ElementKind::MediaTimeLeft).unwrap();
        let timer = k.iter().position(|(kind, _)| matches!(kind, ElementKind::Timer { .. })).unwrap();
        assert!(media < timer && k[media].1 == k[timer].1);
        let slide = resolve_style(&ElementKind::CurrentSlide { pictures: false, keep_after_clear: false }, &ElementStyle::default());
        assert_eq!((slide.max_size, slide.min_size), (9.0, 3.0));
    }

    #[test]
    fn band_and_speaker_have_the_live_fields() {
        let band = kinds(by_id(BAND).unwrap());
        for want in [ElementKind::SectionName, ElementKind::NextSection, ElementKind::SlidesLeft, ElementKind::StageMessage] {
            assert!(band.iter().any(|(k, _)| **k == want), "Band lacks {want:?}");
        }
        let speaker = kinds(by_id(SPEAKER).unwrap());
        assert!(speaker.iter().any(|(k, _)| matches!(k, ElementKind::Timer { timer: TimerRef::Current })));
        assert!(speaker.iter().any(|(k, _)| matches!(k, ElementKind::CurrentSlide { pictures: true, .. })));
        assert!(speaker.iter().any(|(k, _)| **k == ElementKind::Notes));
    }

    #[test]
    fn lyrics_only_and_clock_countdown_match_the_spec() {
        let lyrics = kinds(by_id(LYRICS_ONLY).unwrap());
        assert_eq!(lyrics.iter().map(|(_, r)| *r).collect::<Vec<_>>(),
            [PctRect::new(3.0, 3.0, 94.0, 62.0), PctRect::new(3.0, 68.0, 94.0, 29.0), PctRect::new(3.0, 3.0, 94.0, 14.0)]);
        assert!(matches!(lyrics[0].0, ElementKind::CurrentSlide { .. }) && *lyrics[2].0 == ElementKind::StageMessage);
        let big = known_style(LYRICS_ONLY, 0);
        assert_eq!(big.max_size, 12.0);
        let clock = kinds(by_id(CLOCK_COUNTDOWN).unwrap());
        assert_eq!(clock.iter().map(|(_, r)| *r).collect::<Vec<_>>(),
            [PctRect::new(5.0, 8.0, 90.0, 40.0), PctRect::new(5.0, 52.0, 90.0, 30.0), PctRect::new(5.0, 84.0, 90.0, 13.0)]);
        assert!(matches!(clock[1].0, ElementKind::Timer { timer: TimerRef::Current }));
        assert_eq!((known_style(CLOCK_COUNTDOWN, 0).max_size, known_style(CLOCK_COUNTDOWN, 1).max_size), (30.0, 22.0));
    }

    fn known_style(layout: Uuid, i: usize) -> ResolvedStyle {
        let e = by_id(layout).unwrap().elements[i].known().unwrap();
        resolve_style(&e.kind, &e.style)
    }

    #[test]
    fn find_layout_prefers_builtins_then_church() {
        let mut fake = classic().clone();
        fake.name = "Fake".into();
        let mut ours = classic().clone();
        (ours.id, ours.name) = (Uuid::from_u128(77), "Ours".into());
        let church = [fake, ours.clone()];
        assert_eq!(find_layout(&church, CLASSIC).unwrap().name, "Classic");
        assert_eq!(find_layout(&church, ours.id), Some(&ours));
        assert_eq!(find_layout(&church, Uuid::from_u128(5)), None);
        assert_eq!(all_layouts(&church).map(|l| l.name.as_str()).collect::<Vec<_>>(),
            ["Classic", "Lyrics only", "Band", "Speaker", "Clock and countdown", "Ours"]);
        assert!(is_builtin(SPEAKER) && !is_builtin(ours.id));
    }

    #[test]
    fn every_kind_has_a_name_and_help() {
        let all = all_kinds();
        assert_eq!(all.len(), s::kind_names().len());
        assert_eq!(all.len(), s::kind_help().len());
        assert_eq!(kind_index(&all[8]), 8);
        assert_eq!(s::kind_names()[kind_index(&ElementKind::Notes)], "Notes");
        let b = new_box(ElementKind::Clock { seconds: false, h24: false }, Uuid::from_u128(9));
        assert_eq!((b.id, b.rect, b.style.clone()), (Uuid::from_u128(9), PctRect::new(35.0, 42.5, 30.0, 15.0), ElementStyle::default()));
    }

    #[test]
    fn styles_resolve_over_kind_defaults() {
        let next = resolve_style(&ElementKind::NextSlide { pictures: false }, &ElementStyle::default());
        assert_eq!((next.caption.as_deref(), next.max_size, next.color.as_str()), (Some("NEXT"), 6.0, "#ffffff"));
        let off = resolve_style(&ElementKind::NextSlide { pictures: false }, &st(|s| s.caption = Some(false)));
        assert_eq!(off.caption, None);
        let own = resolve_style(&ElementKind::Notes, &st(|s| { s.caption = Some(true); s.caption_text = Some("FOR YOU".into()); s.max_size = Some(2.5); }));
        assert_eq!(own.caption.as_deref(), Some("FOR YOU"));
        assert_eq!((own.max_size, own.min_size), (2.5, 2.5), "the floor never goes above the biggest size");
    }
}
