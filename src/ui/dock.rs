//! Dock panels (design `docs/superpowers/specs/2026-11-17-dock-panels-design.md`): the operator window's areas as
//! panels in rows, columns and tabs, in the main window and in windows of their own on other monitors. Pure: nothing
//! here touches a window; `ui::operator::docking` draws what `place` works out and calls the changes below.
//!
//! A window's panels are a tree: a `Split` lays its children side by side (`Row`) or one under the other (`Column`),
//! each with a `Size` (a share of what is left, a number of pixels, or as tall/wide as its content wants); a `Tabs`
//! holds one panel or several shown one at a time. Panels never overlap: every panel gets a rectangle of its own.
use serde::{Deserialize, Deserializer, Serialize};

/// The room between panels: the strip a volunteer drags to resize (today's 8 px gaps).
pub const GAP: f32 = 8.0;
/// The room around the panels inside the window.
pub const PAD: f32 = 8.0;
/// A tab bar, or a panel's title bar in arrange mode.
pub const HEAD: f32 = 30.0;

/// Every panel of the operator window. The order is the order of the window's placement list (`index`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum PanelId { Service, Slides, Library, Bible, Live, UpNext, Cameras, Automation, Livestream, Stage, Follow }

impl PanelId {
    pub const ALL: [PanelId; 11] = [
        PanelId::Service, PanelId::Slides, PanelId::Library, PanelId::Bible, PanelId::Live, PanelId::UpNext,
        PanelId::Cameras, PanelId::Automation, PanelId::Livestream, PanelId::Stage, PanelId::Follow,
    ];

    pub fn index(self) -> usize { self as usize }

    pub fn from_index(i: usize) -> Option<PanelId> { Self::ALL.get(i).copied() }

    /// The least room the panel needs to stay usable (width, height). Smaller windows scroll instead.
    pub fn min_size(self) -> (f32, f32) {
        match self {
            PanelId::Service => (250.0, 160.0),
            PanelId::Slides => (240.0, 160.0),
            PanelId::Library => (300.0, 44.0),
            PanelId::Bible => (300.0, 50.0),
            PanelId::Live => (360.0, 300.0),
            PanelId::UpNext => (200.0, 80.0),
            PanelId::Cameras => (300.0, 100.0),
            PanelId::Automation => (200.0, 44.0),
            PanelId::Livestream => (240.0, 56.0),
            PanelId::Stage => (200.0, 130.0),
            PanelId::Follow => (240.0, 44.0),
        }
    }

    /// The size a panel gets when it is put beside another along `dir`: the strips are as tall as their content.
    fn default_size(self, dir: Dir) -> Size {
        match (self, dir) {
            (PanelId::Service, Dir::Row) => Size::Px(264.0),
            (PanelId::Live, Dir::Row) => Size::Px(440.0),
            (PanelId::Bible | PanelId::Library | PanelId::UpNext | PanelId::Automation | PanelId::Livestream
                | PanelId::Follow | PanelId::Cameras, Dir::Column) => Size::Fit,
            _ => Size::Weight(1.0),
        }
    }

    /// Where the panel goes back to when it is shown again: beside the first of these that is in the main window.
    fn homes(self) -> &'static [(PanelId, Zone)] {
        use PanelId::*;
        use Zone::*;
        match self {
            Service => &[(Slides, Left), (Library, Top), (Bible, Bottom), (Live, Left)],
            Slides => &[(Service, Right), (Library, Top), (Bible, Bottom), (Live, Left)],
            Library => &[(Slides, Bottom), (Service, Bottom), (Live, Left)],
            Bible => &[(Slides, Top), (Service, Top), (Library, Top), (Live, Left)],
            Live => &[(Slides, Right), (Library, Right), (Service, Right), (Bible, Right)],
            UpNext | Cameras | Automation | Livestream | Stage | Follow => &[(Live, Bottom), (Slides, Right), (Service, Right)],
        }
    }
}

/// `Row`: children left to right. `Column`: top to bottom.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Dir { Row, Column }

/// How much room a child of a split takes along the split.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Size {
    /// A share of the room the fixed children leave.
    Weight(f32),
    /// This many (logical) pixels.
    Px(f32),
    /// As much as its content wants (a strip of buttons).
    Fit,
}

impl Default for Size {
    fn default() -> Self { Size::Weight(1.0) }
}

/// Where a dragged panel is dropped on another: beside it on one side, or in the middle (a tab with it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone { Left, Right, Top, Bottom, Center }

impl Zone {
    fn dir(self) -> Option<Dir> {
        match self { Zone::Left | Zone::Right => Some(Dir::Row), Zone::Top | Zone::Bottom => Some(Dir::Column), Zone::Center => None }
    }
    fn after(self) -> bool { matches!(self, Zone::Right | Zone::Bottom) }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Node {
    Split { dir: Dir, children: Vec<Child> },
    Tabs { #[serde(deserialize_with = "known_panels")] panels: Vec<PanelId>, #[serde(default)] active: usize },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Child {
    pub node: Node,
    #[serde(default)]
    pub size: Size,
}

/// A window on the screen, in physical pixels (`place` of a window of its own).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WinRect { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }

/// One window's panels. The first window of a layout is the main window; the others are windows of their own.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DockWindow {
    /// The monitor it was on (the name Windows gives it). Built-in layouts say `SECOND_MONITOR`.
    #[serde(default)]
    pub monitor: String,
    /// Where it was, if known.
    #[serde(default)]
    pub place: Option<WinRect>,
    pub root: Node,
}

/// A built-in layout's window goes to the second monitor if there is one.
pub const SECOND_MONITOR: &str = "#2";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Layout {
    /// The church's name for a saved layout (empty for the built-ins and the arrangement in use).
    #[serde(default)]
    pub name: String,
    pub windows: Vec<DockWindow>,
    /// The panels closed in this layout (View → Panels brings them back).
    #[serde(default, deserialize_with = "known_panels")]
    pub closed: Vec<PanelId>,
}

/// The layouts that come with the app.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Builtin { Standard, BigSlides, Livestream, TwoMonitors }

impl Builtin {
    pub const ALL: [Builtin; 4] = [Builtin::Standard, Builtin::BigSlides, Builtin::Livestream, Builtin::TwoMonitors];
}

/// Which layout the operator chose last (View → Layouts ticks it).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Chosen { Builtin(Builtin), Saved(String) }

impl Default for Chosen {
    fn default() -> Self { Chosen::Builtin(Builtin::Standard) }
}

/// What Settings keeps (`Settings::dock`): the arrangement in use, which layout it came from, and the church's own.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct DockPrefs {
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub chosen: Chosen,
    #[serde(deserialize_with = "lenient_layout")]
    pub current: Layout,
    #[serde(deserialize_with = "known_layouts")]
    pub saved: Vec<Layout>,
}

/// Panels this version knows; a newer version's panel is left out.
fn known_panels<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PanelId>, D::Error> {
    let v = Vec::<serde_json::Value>::deserialize(d)?;
    Ok(v.into_iter().filter_map(|p| serde_json::from_value(p).ok()).collect())
}

/// Saved layouts this version can read, each repaired; one it cannot read is left out.
fn known_layouts<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Layout>, D::Error> {
    let v = Vec::<serde_json::Value>::deserialize(d)?;
    Ok(v.into_iter().filter_map(|l| serde_json::from_value::<Layout>(l).ok()).map(|mut l| { l.repair(); l }).collect())
}

/// The arrangement in use, repaired; Standard if it cannot be read.
fn lenient_layout<'de, D: Deserializer<'de>>(d: D) -> Result<Layout, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let mut l = serde_json::from_value::<Layout>(v).unwrap_or_default();
    l.repair();
    Ok(l)
}

fn tabs(panels: &[PanelId]) -> Node { Node::Tabs { panels: panels.to_vec(), active: 0 } }
fn child(node: Node, size: Size) -> Child { Child { node, size } }
fn row(children: Vec<Child>) -> Node { Node::Split { dir: Dir::Row, children } }
fn column(children: Vec<Child>) -> Node { Node::Split { dir: Dir::Column, children } }

/// The work area of Standard: the Bible box over the service list and the slides, the library under them.
fn work_area() -> Node {
    use PanelId::*;
    column(vec![
        child(tabs(&[Bible]), Size::Fit),
        child(row(vec![child(tabs(&[Service]), Size::Px(264.0)), child(tabs(&[Slides]), Size::Weight(1.0))]), Size::Weight(1.0)),
        child(tabs(&[Library]), Size::Fit),
    ])
}

impl Default for Layout {
    fn default() -> Self { Layout::builtin(Builtin::Standard) }
}

impl Layout {
    pub fn builtin(b: Builtin) -> Layout {
        use PanelId::*;
        let main = |root| DockWindow { monitor: String::new(), place: None, root };
        let (windows, closed) = match b {
            // Today's window: the work area beside the live column.
            Builtin::Standard => (
                vec![main(row(vec![child(work_area(), Size::Weight(1.0)), child(tabs(&[Live]), Size::Px(440.0))]))],
                vec![UpNext, Cameras, Automation, Livestream, Stage, Follow],
            ),
            // The slides take the room: the service list and the library share one place as tabs.
            Builtin::BigSlides => (
                vec![main(row(vec![
                    child(tabs(&[Service, Library]), Size::Px(300.0)),
                    child(column(vec![child(tabs(&[Bible]), Size::Fit), child(tabs(&[Slides]), Size::Weight(1.0))]), Size::Weight(1.0)),
                    child(tabs(&[Live]), Size::Px(400.0)),
                ]))],
                vec![UpNext, Cameras, Automation, Livestream, Stage, Follow],
            ),
            // Live, the cameras and the stream larger, on the right.
            Builtin::Livestream => (
                vec![main(row(vec![
                    child(work_area(), Size::Weight(1.0)),
                    child(column(vec![
                        child(tabs(&[Live]), Size::Weight(3.0)),
                        child(tabs(&[Cameras]), Size::Weight(2.0)),
                        child(tabs(&[Livestream]), Size::Fit),
                    ]), Size::Px(520.0)),
                ]))],
                vec![UpNext, Automation, Stage, Follow],
            ),
            // The live panels in a window of their own on the second monitor.
            Builtin::TwoMonitors => (
                vec![
                    main(work_area()),
                    DockWindow {
                        monitor: SECOND_MONITOR.into(),
                        place: None,
                        root: row(vec![
                            child(tabs(&[Live]), Size::Weight(1.0)),
                            child(column(vec![
                                child(tabs(&[Cameras]), Size::Weight(1.0)),
                                child(tabs(&[Livestream]), Size::Fit),
                                child(tabs(&[Automation]), Size::Fit),
                            ]), Size::Weight(1.0)),
                        ]),
                    },
                ],
                vec![UpNext, Stage, Follow],
            ),
        };
        Layout { name: String::new(), windows, closed }
    }

    /// The window a panel is in, if it is shown.
    pub fn window_of(&self, p: PanelId) -> Option<usize> { self.windows.iter().position(|w| w.root.contains(p)) }

    pub fn is_shown(&self, p: PanelId) -> bool { self.window_of(p).is_some() }

    /// Makes the layout valid: every panel at most once, a panel this layout has never heard of (a newer panel)
    /// at its Standard place, no empty tabs or splits, sizes that make sense, a main window with a panel in it.
    pub fn repair(&mut self) {
        let mut seen = Vec::new();
        for w in &mut self.windows { w.root.dedupe(&mut seen); }
        self.closed.retain(|p| if seen.contains(p) { false } else { seen.push(*p); true });
        self.tidy();
        if self.windows.is_empty() || self.windows[0].root.panels().is_empty() {
            *self = Layout::builtin(Builtin::Standard);
            return;
        }
        let standard = Layout::builtin(Builtin::Standard);
        for p in PanelId::ALL {
            if seen.contains(&p) { continue }
            if standard.closed.contains(&p) { self.closed.push(p) } else { self.restore(p) }
        }
    }

    /// Drops empty tabs and splits and windows of their own with nothing left; the main window stays.
    fn tidy(&mut self) {
        let mut i = 0;
        self.windows.retain_mut(|w| {
            let main = i == 0;
            i += 1;
            match std::mem::replace(&mut w.root, tabs(&[])).normalized() {
                Some(n) => { w.root = n; true }
                None => main,
            }
        });
    }

    /// Takes a panel out of wherever it is (closed included). False if it was the main window's last panel.
    fn take(&mut self, p: PanelId) -> bool {
        if self.windows.first().is_some_and(|w| w.root.panels() == [p]) { return false }
        for w in &mut self.windows { w.root.remove(p); }
        self.closed.retain(|c| *c != p);
        self.tidy();
        true
    }

    /// Closes a panel (View → Panels shows it again). The main window keeps at least one panel.
    pub fn close(&mut self, p: PanelId) -> bool {
        if !self.is_shown(p) { return false }
        if !self.take(p) { return false }
        self.closed.push(p);
        true
    }

    /// Shows a closed panel again, beside where it is in Standard (in the main window).
    pub fn restore(&mut self, p: PanelId) {
        if self.is_shown(p) { return }
        self.closed.retain(|c| *c != p);
        let main = &mut self.windows[0].root;
        for (anchor, zone) in p.homes() {
            if main.contains(*anchor) && main.insert(*anchor, *zone, p) { return }
        }
        // Nothing to go beside: at the right of the main window.
        let old = std::mem::replace(main, tabs(&[]));
        *main = match old {
            Node::Split { dir: Dir::Row, mut children } => { children.push(child(tabs(&[p]), p.default_size(Dir::Row))); row(children) }
            other => row(vec![child(other, Size::Weight(1.0)), child(tabs(&[p]), p.default_size(Dir::Row))]),
        };
    }

    /// Drops panel `p` on `target` (any window): beside it, or as a tab with it. False when nothing changes or the
    /// main window would be left empty.
    pub fn drop_on(&mut self, p: PanelId, target: PanelId, zone: Zone) -> bool {
        let Some(tw) = self.window_of(target) else { return false };
        let group = self.windows[tw].root.group_of(target).unwrap_or_default();
        if group.contains(&p) {
            // Into its own group: nothing to do. Beside its own group: out of the group, beside the others.
            if zone == Zone::Center || group.len() == 1 { return false }
        }
        let target = if p == target { *group.iter().find(|g| **g != p).expect("a group of two or more") } else { target };
        let before = self.clone();
        if !self.take(p) { return false }
        let Some(tw) = self.window_of(target) else { *self = before; return false };
        if !self.windows[tw].root.insert(target, zone, p) { *self = before; return false }
        true
    }

    /// Picks a tab: `p` is shown in its group.
    pub fn activate(&mut self, p: PanelId) {
        for w in &mut self.windows { w.root.activate(p); }
    }

    /// Moves `panels` (one panel, or a whole tab group) to a new window of their own. Returns its index, or `None`
    /// if that would leave the main window empty.
    pub fn pop_out(&mut self, panels: &[PanelId], place: Option<WinRect>, monitor: &str) -> Option<usize> {
        if panels.is_empty() { return None }
        let main = self.windows[0].root.panels();
        if main.iter().all(|m| panels.contains(m)) { return None }
        for p in panels { if !self.take(*p) { return None } }
        self.windows.push(DockWindow { monitor: monitor.into(), place, root: tabs(panels) });
        Some(self.windows.len() - 1)
    }

    /// A window of its own closed: its panels go back into the main window, never lost.
    pub fn pop_in(&mut self, win: usize) {
        if win == 0 || win >= self.windows.len() { return }
        let w = self.windows.remove(win);
        for p in w.root.panels() { self.restore(p) }
    }

    /// Moves `p` into window `win` (at its right). False if it is there already or the main window would be empty.
    pub fn move_to(&mut self, p: PanelId, win: usize) -> bool {
        if win >= self.windows.len() || self.window_of(p) == Some(win) { return false }
        // Taking the panel can remove its old window, which moves the later ones down by one.
        let from = self.window_of(p);
        let before = self.clone();
        if !self.take(p) { return false }
        let win = match from { Some(f) if f < win && self.windows.len() < before.windows.len() => win - 1, _ => win };
        let Some(w) = self.windows.get_mut(win) else { *self = before; return false };
        let old = std::mem::replace(&mut w.root, tabs(&[]));
        w.root = match old {
            Node::Split { dir: Dir::Row, mut children } => { children.push(child(tabs(&[p]), Size::Weight(1.0))); row(children) }
            other => row(vec![child(other, Size::Weight(1.0)), child(tabs(&[p]), Size::Weight(1.0))]),
        };
        true
    }
}

impl Node {
    /// Every panel in the tree, in order.
    pub fn panels(&self) -> Vec<PanelId> {
        let mut out = Vec::new();
        self.each_panel(&mut |p| out.push(p));
        out
    }

    fn each_panel(&self, f: &mut impl FnMut(PanelId)) {
        match self {
            Node::Tabs { panels, .. } => panels.iter().for_each(|p| f(*p)),
            Node::Split { children, .. } => children.iter().for_each(|c| c.node.each_panel(f)),
        }
    }

    pub fn contains(&self, p: PanelId) -> bool {
        match self {
            Node::Tabs { panels, .. } => panels.contains(&p),
            Node::Split { children, .. } => children.iter().any(|c| c.node.contains(p)),
        }
    }

    /// The panels of the tab group holding `p`.
    pub fn group_of(&self, p: PanelId) -> Option<Vec<PanelId>> {
        match self {
            Node::Tabs { panels, .. } => panels.contains(&p).then(|| panels.clone()),
            Node::Split { children, .. } => children.iter().find_map(|c| c.node.group_of(p)),
        }
    }

    fn dedupe(&mut self, seen: &mut Vec<PanelId>) {
        match self {
            Node::Tabs { panels, .. } => panels.retain(|p| if seen.contains(p) { false } else { seen.push(*p); true }),
            Node::Split { children, .. } => children.iter_mut().for_each(|c| c.node.dedupe(seen)),
        }
    }

    fn remove(&mut self, p: PanelId) {
        match self {
            Node::Tabs { panels, active } => {
                if let Some(i) = panels.iter().position(|x| *x == p) {
                    panels.remove(i);
                    if *active > i || *active >= panels.len() { *active = active.saturating_sub(1) }
                }
            }
            Node::Split { children, .. } => children.iter_mut().for_each(|c| c.node.remove(p)),
        }
    }

    fn activate(&mut self, p: PanelId) {
        match self {
            Node::Tabs { panels, active } => if let Some(i) = panels.iter().position(|x| *x == p) { *active = i },
            Node::Split { children, .. } => children.iter_mut().for_each(|c| c.node.activate(p)),
        }
    }

    /// No empty tabs or splits, a split of one is its child, sizes that make sense. `None` when nothing is left.
    pub fn normalized(self) -> Option<Node> {
        match self {
            Node::Tabs { panels, active } if !panels.is_empty() => { let active = active.min(panels.len() - 1); Some(Node::Tabs { panels, active }) }
            Node::Tabs { .. } => None,
            Node::Split { dir, children } => {
                let mut kept: Vec<Child> = children.into_iter().filter_map(|c| {
                    let size = match c.size {
                        Size::Weight(w) if !(w.is_finite() && w > 0.0) => Size::Weight(1.0),
                        Size::Px(p) if !(p.is_finite() && p >= 0.0) => Size::Fit,
                        s => s,
                    };
                    c.node.normalized().map(|node| Child { node, size })
                }).collect();
                match kept.len() {
                    0 => None,
                    1 => Some(kept.remove(0).node),
                    _ => Some(Node::Split { dir, children: kept }),
                }
            }
        }
    }

    /// Puts `p` beside the group holding `target` (or into it, `Center`). False if `target` is not here.
    fn insert(&mut self, target: PanelId, zone: Zone, p: PanelId) -> bool {
        match self {
            Node::Tabs { panels, active } if panels.contains(&target) => {
                match zone.dir() {
                    None => { panels.push(p); *active = panels.len() - 1; }
                    Some(dir) => {
                        let old = std::mem::replace(self, tabs(&[]));
                        let new = child(tabs(&[p]), p.default_size(dir));
                        let old = child(old, Size::Weight(1.0));
                        let children = if zone.after() { vec![old, new] } else { vec![new, old] };
                        *self = Node::Split { dir, children };
                    }
                }
                true
            }
            Node::Tabs { .. } => false,
            Node::Split { dir, children } => {
                let Some(i) = children.iter().position(|c| c.node.contains(target)) else { return false };
                let leaf = matches!(children[i].node, Node::Tabs { .. });
                if leaf && zone.dir() == Some(*dir) {
                    let size = share_with(&mut children[i].size, p, *dir);
                    let at = if zone.after() { i + 1 } else { i };
                    children.insert(at, child(tabs(&[p]), size));
                    return true
                }
                children[i].node.insert(target, zone, p)
            }
        }
    }

    fn at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        match path.split_first() {
            None => Some(self),
            Some((i, rest)) => match self {
                Node::Split { children, .. } => children.get_mut(*i)?.node.at_mut(rest),
                Node::Tabs { .. } => None,
            },
        }
    }

    /// Whether this node draws a head: a tab bar (two panels or more) or arrange mode's title bar.
    fn has_head(&self, arrange: bool) -> bool {
        matches!(self, Node::Tabs { panels, .. } if arrange || panels.len() > 1)
    }

    /// The least room this node needs (width, height).
    pub fn min_size(&self, arrange: bool) -> (f32, f32) {
        match self {
            Node::Tabs { panels, .. } => {
                let (w, h) = panels.iter().map(|p| p.min_size()).fold((0.0f32, 0.0f32), |a, b| (a.0.max(b.0), a.1.max(b.1)));
                (w, h + if self.has_head(arrange) { HEAD } else { 0.0 })
            }
            Node::Split { dir, children } => {
                let gaps = GAP * (children.len().saturating_sub(1)) as f32;
                let mins = children.iter().map(|c| c.node.min_size(arrange));
                match dir {
                    Dir::Row => mins.fold((gaps, 0.0f32), |a, b| (a.0 + b.0, a.1.max(b.1))),
                    Dir::Column => mins.fold((0.0f32, gaps), |a, b| (a.0.max(b.0), a.1 + b.1)),
                }
            }
        }
    }

    /// The room this node's content wants along `dir` (for `Size::Fit`).
    fn wanted(&self, dir: Dir, arrange: bool, fit: &dyn Fn(PanelId) -> (f32, f32)) -> f32 {
        let along = |s: (f32, f32)| match dir { Dir::Row => s.0, Dir::Column => s.1 };
        match self {
            Node::Tabs { panels, .. } => {
                let want = panels.iter().map(|p| along(fit(*p)).max(along(p.min_size()))).fold(0.0, f32::max);
                want + if dir == Dir::Column && self.has_head(arrange) { HEAD } else { 0.0 }
            }
            Node::Split { dir: d, children } => {
                let each = children.iter().map(|c| match c.size {
                    Size::Px(p) if *d == dir => p,
                    Size::Weight(_) if *d == dir => along(c.node.min_size(arrange)),
                    _ => c.node.wanted(dir, arrange, fit),
                });
                if *d == dir { each.sum::<f32>() + GAP * children.len().saturating_sub(1) as f32 } else { each.fold(0.0, f32::max) }
            }
        }
    }
}

/// The size a new panel `p` gets beside a child of size `target` (which may give up half of its own).
fn share_with(target: &mut Size, p: PanelId, dir: Dir) -> Size {
    let min = match dir { Dir::Row => p.min_size().0, Dir::Column => p.min_size().1 };
    match (p.default_size(dir), *target) {
        (Size::Weight(_), Size::Weight(w)) => { *target = Size::Weight(w / 2.0); Size::Weight(w / 2.0) }
        (Size::Weight(_), Size::Px(px)) => { let half = ((px - GAP) / 2.0).max(min); *target = Size::Px(half); Size::Px(half) }
        (Size::Weight(_), Size::Fit) => Size::Px(min * 1.5),
        (own, _) => own,
    }
}

/// A rectangle in a window, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool { x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h }
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
    pub fn right(&self) -> f32 { self.x + self.w }
    pub fn bottom(&self) -> f32 { self.y + self.h }
}

/// Where a panel is drawn. `shown` is false for a tab that is not picked (it keeps its place, hidden).
#[derive(Clone, Debug, PartialEq)]
pub struct PanelPlace { pub panel: PanelId, pub rect: Rect, pub shown: bool }

/// A tab bar or title bar over a group: its panels and the one shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Head { pub rect: Rect, pub panels: Vec<PanelId>, pub active: usize }

/// A strip between two children of a split, to drag. `before` and `after` are the two children's sizes now.
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    pub rect: Rect,
    pub dir: Dir,
    pub path: Vec<usize>,
    pub index: usize,
    pub before: f32,
    pub after: f32,
    pub min_before: f32,
    pub min_after: f32,
}

/// Everything a window draws for its tree.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Placement {
    pub panels: Vec<PanelPlace>,
    pub heads: Vec<Head>,
    pub dividers: Vec<Divider>,
    /// The room the panels take: the window, or more when the window is smaller than the panels need (it scrolls).
    pub width: f32,
    pub height: f32,
}

impl Placement {
    pub fn panel(&self, p: PanelId) -> Option<&PanelPlace> { self.panels.iter().find(|x| x.panel == p) }

    /// The group (head and body) shown at a point, with the panel picked in it.
    fn group_at(&self, x: f32, y: f32) -> Option<(PanelId, Rect)> {
        self.panels.iter().filter(|p| p.shown).find_map(|p| {
            let head = self.heads.iter().find(|h| h.panels.contains(&p.panel));
            let r = match head { Some(h) => Rect { x: p.rect.x, y: h.rect.y, w: p.rect.w, h: p.rect.bottom() - h.rect.y }, None => p.rect };
            r.contains(x, y).then_some((p.panel, r))
        })
    }

    /// Where a panel dragged to (`x`, `y`) would go: the panel under it, the side (the outer quarter nearest an
    /// edge) or the middle, and the part of the window to light up.
    pub fn drop_target(&self, x: f32, y: f32) -> Option<(PanelId, Zone, Rect)> {
        let (p, r) = self.group_at(x, y)?;
        let (u, v) = ((x - r.x) / r.w.max(1.0), (y - r.y) / r.h.max(1.0));
        let edges = [(u, Zone::Left), (1.0 - u, Zone::Right), (v, Zone::Top), (1.0 - v, Zone::Bottom)];
        let (d, zone) = edges.into_iter().fold((f32::MAX, Zone::Center), |a, b| if b.0 < a.0 { b } else { a });
        let zone = if d < 0.25 { zone } else { Zone::Center };
        let lit = match zone {
            Zone::Left => Rect { w: r.w / 2.0, ..r },
            Zone::Right => Rect { x: r.x + r.w / 2.0, w: r.w / 2.0, ..r },
            Zone::Top => Rect { h: r.h / 2.0, ..r },
            Zone::Bottom => Rect { y: r.y + r.h / 2.0, h: r.h / 2.0, ..r },
            Zone::Center => r,
        };
        Some((p, zone, lit))
    }

    /// The parts of the window a menu or list must never cover: the audience screen's and the stage screen's
    /// previews (the Live and Stage preview panels).
    pub fn keep_clear(&self) -> Vec<Rect> {
        self.panels.iter().filter(|p| p.shown && matches!(p.panel, PanelId::Live | PanelId::Stage)).map(|p| p.rect).collect()
    }
}

/// Works out every panel's rectangle in a window `w` × `h` (logical pixels). `fit` gives a panel's content's
/// wanted size, for `Size::Fit`. A window smaller than the panels need gets a larger area (it scrolls).
pub fn place(root: &Node, w: f32, h: f32, arrange: bool, fit: &dyn Fn(PanelId) -> (f32, f32)) -> Placement {
    let (mw, mh) = root.min_size(arrange);
    let (width, height) = (w.max(mw + 2.0 * PAD), h.max(mh + 2.0 * PAD));
    let mut out = Placement { width, height, ..Default::default() };
    let area = Rect { x: PAD, y: PAD, w: width - 2.0 * PAD, h: height - 2.0 * PAD };
    place_node(root, area, arrange, fit, &mut Vec::new(), &mut out);
    out
}

fn place_node(node: &Node, r: Rect, arrange: bool, fit: &dyn Fn(PanelId) -> (f32, f32), path: &mut Vec<usize>, out: &mut Placement) {
    match node {
        Node::Tabs { panels, active } => {
            let body = if node.has_head(arrange) {
                out.heads.push(Head { rect: Rect { h: HEAD, ..r }, panels: panels.clone(), active: *active });
                Rect { y: r.y + HEAD, h: r.h - HEAD, ..r }
            } else { r };
            for (i, p) in panels.iter().enumerate() { out.panels.push(PanelPlace { panel: *p, rect: body, shown: i == *active }) }
        }
        Node::Split { dir, children } => {
            let along = |s: (f32, f32)| match dir { Dir::Row => s.0, Dir::Column => s.1 };
            let total = along((r.w, r.h)) - GAP * children.len().saturating_sub(1) as f32;
            let mins: Vec<f32> = children.iter().map(|c| along(c.node.min_size(arrange))).collect();
            let sizes = distribute(children, &mins, total, |c| c.node.wanted(*dir, arrange, fit));
            // Whole pixels: each edge rounded, so neighbours meet exactly one gap apart.
            let start = along((r.x, r.y));
            let mut pos = start;
            let mut edges = Vec::with_capacity(children.len());
            for s in &sizes { let a = pos.round(); pos += s; let b = pos.round(); edges.push((a, b)); pos += GAP; }
            for (i, (c, (a, b))) in children.iter().zip(&edges).enumerate() {
                let cr = match dir { Dir::Row => Rect { x: *a, w: b - a, ..r }, Dir::Column => Rect { y: *a, h: b - a, ..r } };
                path.push(i);
                place_node(&c.node, cr, arrange, fit, path, out);
                path.pop();
                if i + 1 < children.len() {
                    let next = edges[i + 1].0;
                    let rect = match dir { Dir::Row => Rect { x: *b, w: next - b, ..r }, Dir::Column => Rect { y: *b, h: next - b, ..r } };
                    out.dividers.push(Divider {
                        rect, dir: *dir, path: path.clone(), index: i, before: sizes[i], after: sizes[i + 1],
                        min_before: mins[i], min_after: mins[i + 1],
                    });
                }
            }
        }
    }
}

/// Shares `total` among children: fixed and fitted ones first, then the shares, every child at least its minimum.
/// With no shares, the last child takes what is left over.
fn distribute(children: &[Child], mins: &[f32], total: f32, wanted: impl Fn(&Child) -> f32) -> Vec<f32> {
    let n = children.len();
    let mut sizes: Vec<f32> = children.iter().zip(mins).map(|(c, m)| match c.size {
        Size::Px(p) => p.max(*m),
        Size::Fit => wanted(c).max(*m),
        Size::Weight(_) => *m,
    }).collect();
    let weighted: Vec<usize> = (0..n).filter(|i| matches!(children[*i].size, Size::Weight(_))).collect();
    let used: f32 = sizes.iter().sum();
    if used > total {
        // Too little room: the fixed ones give way down to their minimums, in proportion to what they can give.
        let give: f32 = (0..n).map(|i| sizes[i] - mins[i]).sum();
        let need = used - total;
        if give > 0.0 {
            let k = (need / give).min(1.0);
            for i in 0..n { sizes[i] -= (sizes[i] - mins[i]) * k; }
        }
        return sizes;
    }
    if weighted.is_empty() {
        if let Some(last) = sizes.last_mut() { *last += total - used; }
        return sizes;
    }
    // The shares: room left over by the fixed children, split by weight; a share below its minimum keeps its
    // minimum and the rest is split again among the others.
    let fixed: f32 = (0..n).filter(|i| !weighted.contains(i)).map(|i| sizes[i]).sum();
    let mut free = weighted.clone();
    let mut room = total - fixed;
    loop {
        let wsum: f32 = free.iter().map(|i| weight(&children[*i])).sum();
        let low: Vec<usize> = free.iter().copied().filter(|i| room * weight(&children[*i]) / wsum < mins[*i]).collect();
        if low.is_empty() {
            for i in &free { sizes[*i] = room * weight(&children[*i]) / wsum; }
            return sizes;
        }
        for i in low { sizes[i] = mins[i]; room -= mins[i]; free.retain(|f| *f != i); }
        if free.is_empty() { return sizes }
    }
}

fn weight(c: &Child) -> f32 { match c.size { Size::Weight(w) => w, _ => 0.0 } }

/// Dragging divider `d` by `delta` pixels (from where the drag started): the two children change size, each kept
/// at its minimum or more, and the change is kept in the tree (a share stays a share, the others become pixels).
pub fn drag_divider(root: &mut Node, d: &Divider, delta: f32) {
    let total = d.before + d.after;
    let a = (d.before + delta).min(total - d.min_after).max(d.min_before.min(total));
    let b = total - a;
    let Some(Node::Split { children, .. }) = root.at_mut(&d.path) else { return };
    if d.index + 1 >= children.len() { return }
    let (sa, sb) = (children[d.index].size, children[d.index + 1].size);
    let (na, nb) = match (sa, sb) {
        (Size::Weight(x), Size::Weight(y)) if total > 0.0 => { let s = x + y; (Size::Weight(s * a / total), Size::Weight(s * b / total)) }
        (Size::Weight(_), _) => (sa, Size::Px(b)),
        (_, Size::Weight(_)) => (Size::Px(a), sb),
        _ => (Size::Px(a), Size::Px(b)),
    };
    children[d.index].size = na;
    children[d.index + 1].size = nb;
}

#[cfg(test)]
mod tests;
