//! N15: what a slide does when it is taken live (never when it is only previewed or drawn as a thumbnail). Each
//! slide keeps a short list of macro steps, shown as badges on its thumbnail; a "Run macro" step runs that macro's
//! steps in its place. The action palette lists every action there is, as tiles to drag onto a slide.
use crate::automation::Automation;
use crate::automation::macros::{ClearWhich, ItemTarget, Macro, MacroStep};
use crate::automation::preservice::PRE_SERVICE_MACRO_ID;
use crate::lenient::Lenient;
use crate::model::Data;
use crate::timers::Builtin;
use crate::tr;
use std::collections::BTreeMap;
use uuid::Uuid;

/// A slide can do this many things at the most.
pub const MAX_PER_SLIDE: usize = 8;
/// The palette offers "Go to slide" for this many slides at the most.
const MAX_GO_TO: usize = 20;

/// One item's slide actions, by 0-based slide.
pub type ItemActions = BTreeMap<usize, Vec<Lenient<MacroStep>>>;

pub fn too_many() -> String { tr!("A slide can do up to 8 things.") }
pub fn gone() -> String { tr!("That action was already taken off.") }
pub fn playlists_not_ready() -> String { tr!("Playlists aren't available yet.") }
/// The name a slide's actions run under, in toasts and notices.
pub fn run_name(slide: usize) -> String { tr!("Slide {n}", n = slide + 1) }

/// Slide `slide` of service item `item`'s actions, in order.
pub fn of(a: &Automation, item: Uuid, slide: usize) -> &[Lenient<MacroStep>] {
    a.slide_actions.get(&item).and_then(|m| m.get(&slide)).map_or(&[][..], Vec::as_slice)
}

/// Adds `steps` (one palette tile) to the end of a slide's list.
pub fn add(a: &mut Automation, item: Uuid, slide: usize, steps: Vec<MacroStep>) -> Result<(), String> {
    for s in &steps {
        if let MacroStep::RunMacro { macro_id } = s && !a.macros.iter().any(|m| m.id == *macro_id) { return Err(super::strings::macro_gone()) }
    }
    if of(a, item, slide).len() + steps.len() > MAX_PER_SLIDE { return Err(too_many()) }
    if steps.is_empty() { return Ok(()) }
    a.slide_actions.entry(item).or_default().entry(slide).or_default().extend(steps.into_iter().map(Lenient::Known));
    Ok(())
}

/// Takes action `index` off a slide.
pub fn remove(a: &mut Automation, item: Uuid, slide: usize, index: usize) -> Result<(), String> {
    let m = a.slide_actions.get_mut(&item).ok_or_else(gone)?;
    let list = m.get_mut(&slide).filter(|l| index < l.len()).ok_or_else(gone)?;
    list.remove(index);
    if list.is_empty() { m.remove(&slide); }
    if m.is_empty() { a.slide_actions.remove(&item); }
    Ok(())
}

/// The macro a slide runs: each "Run macro" step becomes that macro's own steps (one level only). With the
/// Pre-service macro in it, it is marked as that one, so the countdown's end works as from its button.
pub fn as_macro(a: &Automation, name: String, steps: &[Lenient<MacroStep>]) -> Macro {
    let mut out = vec![];
    let mut builtin = None;
    for s in steps {
        let inner = match s.known() { Some(MacroStep::RunMacro { macro_id }) => a.macros.iter().find(|m| m.id == *macro_id), _ => None };
        match inner {
            Some(m) => {
                if m.id == PRE_SERVICE_MACRO_ID { builtin = Some(Builtin::PreService) }
                out.extend(m.steps.iter().filter(|s| !matches!(s.known(), Some(MacroStep::RunMacro { .. }))).cloned());
            }
            None => out.push(s.clone()),
        }
    }
    Macro { id: Uuid::nil(), name, steps: out, builtin, ..Macro::default() }
}

/// The audio playlists the palette offers, as (id, name). N13 adds playlists; until then there are none.
pub fn playlists(_data: &Data) -> Vec<(Uuid, String)> { vec![] }

/// The palette's groups, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group { Clear, Play, Timers, Messages, Overlays, Macros, Stage, Themes, Slides }

pub const GROUPS: [Group; 9] =
    [Group::Clear, Group::Play, Group::Timers, Group::Messages, Group::Overlays, Group::Macros, Group::Stage, Group::Themes, Group::Slides];

impl Group {
    pub fn label(self) -> String {
        match self {
            Group::Clear => tr!("Clear"),
            Group::Play => tr!("Play"),
            Group::Timers => tr!("Timers"),
            Group::Messages => tr!("Messages"),
            Group::Overlays => tr!("Overlays"),
            Group::Macros => tr!("Macros"),
            Group::Stage => tr!("Stage screen"),
            Group::Themes => tr!("Themes"),
            Group::Slides => tr!("Slides"),
        }
    }
}

/// One palette tile: what it adds to a slide. `off` says why it can't be used yet (the tile is greyed).
#[derive(Clone, Debug, PartialEq)]
pub struct Tile { pub group: Group, pub label: String, pub steps: Vec<MacroStep>, pub off: Option<String> }

fn tile(group: Group, label: String, steps: Vec<MacroStep>) -> Tile { Tile { group, label, steps, off: None } }

/// Every action a slide can do, by group. `slides` is the selected item's slide count (for "Go to slide").
pub fn palette(data: &Data, slides: usize) -> Vec<Tile> {
    let a = &data.automation;
    let mut t = vec![];
    for (label, step) in [
        (tr!("Clear everything"), MacroStep::Clear { which: ClearWhich::All }),
        (tr!("Clear words"), MacroStep::Clear { which: ClearWhich::Slide }),
        (tr!("Clear video and pictures"), MacroStep::Clear { which: ClearWhich::Media }),
        (tr!("Clear audio"), MacroStep::Clear { which: ClearWhich::Audio }),
        (tr!("Clear overlays"), MacroStep::Clear { which: ClearWhich::Overlays }),
        (tr!("Hide the message"), MacroStep::MessageHide),
        (tr!("Show the logo"), MacroStep::Logo),
    ] {
        t.push(tile(Group::Clear, label, vec![step]));
    }
    if let Some(s) = data.service() {
        for i in &s.items {
            let (title, kind, _) = crate::slides::build_item(data, i);
            if matches!(kind, "video" | "audio" | "image") {
                t.push(tile(Group::Play, tr!("Play “{title}”", title = title), vec![MacroStep::PlayMedia { target: ItemTarget::Titled { title } }]));
            }
        }
    }
    let lists = playlists(data);
    if lists.is_empty() {
        t.push(Tile { group: Group::Play, label: tr!("Play a playlist"), steps: vec![], off: Some(tr!("No playlists yet.")) });
    }
    for (id, name) in lists {
        t.push(tile(Group::Play, tr!("Play playlist “{name}”", name = name), vec![MacroStep::PlayPlaylist { playlist: id }]));
    }
    for d in &a.timers {
        t.push(tile(Group::Timers, tr!("Start “{name}”", name = d.name), vec![MacroStep::TimerRestart { timer: d.id }, MacroStep::TimerShow { timer: d.id }]));
        t.push(tile(Group::Timers, tr!("Stop “{name}”", name = d.name), vec![MacroStep::TimerHide { timer: d.id }, MacroStep::TimerReset { timer: d.id }]));
    }
    for m in &a.messages {
        let fields = crate::automation::messages::parse(&m.text).fields;
        let mut x = tile(Group::Messages, tr!("Show “{name}”", name = m.name), vec![MacroStep::MessageShow { template: m.id, values: BTreeMap::new() }]);
        if !fields.is_empty() { x.off = Some(tr!("This message has blanks to fill in. Put it in a macro instead.")) }
        t.push(x);
    }
    for o in &data.overlays {
        t.push(tile(Group::Overlays, tr!("“{name}” on", name = o.name), vec![MacroStep::Overlay { id: o.id, on: true }]));
        t.push(tile(Group::Overlays, tr!("“{name}” off", name = o.name), vec![MacroStep::Overlay { id: o.id, on: false }]));
    }
    for m in &a.macros {
        t.push(tile(Group::Macros, tr!("Run “{name}”", name = m.name), vec![MacroStep::RunMacro { macro_id: m.id }]));
    }
    for l in crate::stage::presets::builtins().iter().chain(&data.stage_layouts) {
        t.push(tile(Group::Stage, tr!("Stage: {name}", name = l.name), vec![MacroStep::StageLayout { layout: l.id }]));
    }
    t.push(tile(Group::Stage, tr!("Let the stage layout follow the slides"), vec![MacroStep::StageLayoutAuto]));
    t.push(tile(Group::Stage, tr!("Hide the stage message"), vec![MacroStep::StageMessageHide]));
    for r in data.catalog().rows() {
        t.push(tile(Group::Themes, tr!("Theme: {name}", name = r.name), vec![MacroStep::UseTheme { theme: r.id }]));
    }
    t.push(Tile { group: Group::Themes, label: tr!("Change what a screen shows"), steps: vec![], off: Some(super::strings::screen_views_not_yet()) });
    for n in 0..slides.clamp(1, MAX_GO_TO) {
        t.push(tile(Group::Slides, tr!("Go to slide {n}", n = n + 1), vec![MacroStep::GoToSlide { slide: n }]));
    }
    for (label, step) in [
        (tr!("Next slide"), MacroStep::Next),
        (tr!("Pause automatic slides"), MacroStep::AutoAdvance { on: false }),
        (tr!("Resume automatic slides"), MacroStep::AutoAdvance { on: true }),
    ] {
        t.push(tile(Group::Slides, label, vec![step]));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timers::SERMON_ID;

    const ITEM: Uuid = Uuid::from_u128(0x1);

    #[test]
    fn add_and_remove_keep_the_map_tidy() {
        let mut a = Automation::seeded();
        add(&mut a, ITEM, 2, vec![MacroStep::Logo, MacroStep::Next]).unwrap();
        add(&mut a, ITEM, 2, vec![MacroStep::MessageHide]).unwrap();
        assert_eq!(of(&a, ITEM, 2).len(), 3);
        assert!(of(&a, ITEM, 0).is_empty());
        remove(&mut a, ITEM, 2, 1).unwrap();
        assert_eq!(of(&a, ITEM, 2), [Lenient::Known(MacroStep::Logo), Lenient::Known(MacroStep::MessageHide)]);
        assert_eq!(remove(&mut a, ITEM, 2, 5), Err(gone()));
        remove(&mut a, ITEM, 2, 0).unwrap();
        remove(&mut a, ITEM, 2, 0).unwrap();
        assert!(a.slide_actions.is_empty(), "an empty slide and item go");
    }

    #[test]
    fn a_slide_holds_eight_at_most_and_only_real_macros() {
        let mut a = Automation::seeded();
        add(&mut a, ITEM, 0, vec![MacroStep::Logo; 7]).unwrap();
        assert_eq!(add(&mut a, ITEM, 0, vec![MacroStep::Logo; 2]), Err(too_many()));
        assert_eq!(of(&a, ITEM, 0).len(), 7, "nothing of a refused tile is added");
        let e = add(&mut a, ITEM, 1, vec![MacroStep::RunMacro { macro_id: Uuid::from_u128(9) }]);
        assert_eq!(e, Err(super::super::strings::macro_gone()));
    }

    #[test]
    fn run_macro_steps_become_the_macros_steps() {
        let mut a = Automation::seeded();
        crate::automation::apply_preservice(&mut a, Default::default());
        let walk = Macro { id: Uuid::from_u128(0xa), name: "Walk".into(), steps: vec![Lenient::Known(MacroStep::Logo)], ..Macro::default() };
        a.macros.push(walk);
        let steps = [MacroStep::Next, MacroStep::RunMacro { macro_id: Uuid::from_u128(0xa) }, MacroStep::RunMacro { macro_id: Uuid::from_u128(0xdead) }]
            .map(Lenient::Known);
        let m = as_macro(&a, "Slide 1".into(), &steps);
        assert_eq!(m.steps, [MacroStep::Next, MacroStep::Logo, MacroStep::RunMacro { macro_id: Uuid::from_u128(0xdead) }].map(Lenient::Known));
        assert_eq!(m.builtin, None);
        let m = as_macro(&a, "Slide 1".into(), &[Lenient::Known(MacroStep::RunMacro { macro_id: PRE_SERVICE_MACRO_ID })]);
        assert_eq!(m.builtin, Some(Builtin::PreService));
        assert!(m.steps.contains(&Lenient::Known(MacroStep::TimerShow { timer: crate::timers::PRE_SERVICE_TIMER_ID })));
    }

    #[test]
    fn palette_has_every_kind_of_action() {
        let mut d = Data::default();
        crate::automation::apply_preservice(&mut d.automation, Default::default());
        let tiles = palette(&d, 3);
        for g in GROUPS.iter().filter(|g| **g != Group::Overlays) {
            assert!(tiles.iter().any(|t| t.group == *g), "{g:?} has tiles");
        }
        let start = tiles.iter().find(|t| t.label == "Start “Sermon”").unwrap();
        assert_eq!(start.steps, [MacroStep::TimerRestart { timer: SERMON_ID }, MacroStep::TimerShow { timer: SERMON_ID }]);
        assert!(tiles.iter().any(|t| t.label == "Run “Start countdown”"));
        assert!(tiles.iter().any(|t| t.label == "Stage: Classic"), "{:?}", tiles.iter().filter(|t| t.group == Group::Stage).map(|t| &t.label).collect::<Vec<_>>());
        assert!(tiles.iter().any(|t| t.label == "Theme: Standard"));
        let playlist = tiles.iter().find(|t| t.label == "Play a playlist").unwrap();
        assert!(playlist.off.is_some() && playlist.steps.is_empty(), "a stub until N13");
        assert!(tiles.iter().any(|t| t.steps == [MacroStep::StageLayoutAuto]));
        let go: Vec<_> = tiles.iter().filter(|t| matches!(t.steps[..], [MacroStep::GoToSlide { .. }])).map(|t| t.label.as_str()).collect();
        assert_eq!(go, ["Go to slide 1", "Go to slide 2", "Go to slide 3"], "one for each slide of the item");
        assert_eq!(palette(&d, 0).iter().filter(|t| t.label.starts_with("Go to slide")).count(), 1);
        let look = tiles.iter().find(|t| t.label == "Change what a screen shows").unwrap();
        assert!(look.off.is_some(), "a stub until screen views");
        let nursery = tiles.iter().find(|t| t.label == "Show “Nursery”").unwrap();
        assert!(nursery.off.is_some(), "a message with blanks needs a macro");
    }

    #[test]
    fn new_steps_round_trip() {
        for s in [MacroStep::PlayMedia { target: ItemTarget::Titled { title: "Walk-in".into() } }, MacroStep::PlayPlaylist { playlist: ITEM },
            MacroStep::UseTheme { theme: ITEM }, MacroStep::GoToSlide { slide: 3 }, MacroStep::RunMacro { macro_id: ITEM }] {
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(serde_json::from_str::<MacroStep>(&json).unwrap(), s, "{json}");
        }
    }
}
