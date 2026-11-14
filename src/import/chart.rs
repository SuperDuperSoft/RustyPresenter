//! A PowerPoint chart drawn from its own numbers. A chart keeps no picture of itself in the file, only its data
//! (`ppt/charts/chartN.xml`: series, categories and the values PowerPoint last worked out), so it came over as a
//! grey box. Here the common kinds become plain objects the slide editor can change: columns and bars (side by
//! side or stacked) as rectangles, lines and areas as drawn lines, a pie or doughnut as one bar cut into its
//! shares, with the title, the category names and a legend as text on a dark panel (so the words read on any
//! background). Other kinds (scatter, radar, stock, surface, bubble) are left to the caller's grey box.
use super::notes::{attr, is};
use crate::design::{HAlign, Look, Obj, ObjKind, Para, PctRect, Run, Shape, TextStyle, VAlign};
use roxmltree::{Document, Node};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Columns, Bars, Lines, Pie }

#[derive(Clone, Debug, PartialEq)]
pub struct Series { pub name: String, pub color: Option<String>, pub values: Vec<f64> }

/// What a chart part says.
#[derive(Clone, Debug, PartialEq)]
pub struct Chart { pub kind: Kind, pub stacked: bool, pub title: String, pub categories: Vec<String>, pub series: Vec<Series> }

/// Office's own series colours, in order.
const PALETTE: [&str; 6] = ["#4472c4", "#ed7d31", "#a5a5a5", "#ffc000", "#5b9bd5", "#70ad47"];
const PANEL: &str = "#1f2937";
const WORDS: &str = "#ffffff";
/// The most series and categories drawn (more would be slivers).
const MAX_SERIES: usize = 12;
const MAX_CATEGORIES: usize = 40;

/// The cached points of a `c:cat`, `c:val` or `c:tx` (their `idx` says where each belongs).
fn points(holder: Node) -> Vec<(usize, String)> {
    holder.descendants().filter(|n| is(n, "pt")).filter_map(|pt| {
        let idx = attr(&pt, "idx")?.parse().ok()?;
        let v = pt.children().find(|n| is(n, "v"))?.text().unwrap_or("").trim().to_string();
        Some((idx, v))
    }).collect()
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> { n.children().find(|c| is(c, name)) }

/// Reads a chart part. `None`: not a chart we draw, or it has no numbers.
pub fn read(xml: &str) -> Option<Chart> {
    let doc = Document::parse(xml).ok()?;
    let chart = doc.descendants().find(|n| is(n, "chart"))?;
    let plot = child(chart, "plotArea")?;
    let (group, kind) = plot.children().find_map(|n| {
        let kind = match n.tag_name().name() {
            "barChart" | "bar3DChart" => if child(n, "barDir").and_then(|d| attr(&d, "val")) == Some("bar") { Kind::Bars } else { Kind::Columns },
            "lineChart" | "line3DChart" | "areaChart" | "area3DChart" => Kind::Lines,
            "pieChart" | "pie3DChart" | "doughnutChart" | "ofPieChart" => Kind::Pie,
            _ => return None,
        };
        Some((n, kind))
    })?;
    let stacked = kind != Kind::Lines && child(group, "grouping").and_then(|g| attr(&g, "val")).is_some_and(|v| v.contains("tacked"));
    let title = child(chart, "title").map(|t| t.descendants().filter(|n| is(n, "t")).filter_map(|n| n.text()).collect::<String>()).unwrap_or_default();
    let mut categories: Vec<String> = vec![];
    let mut series = vec![];
    for ser in group.children().filter(|n| is(n, "ser")).take(MAX_SERIES) {
        let name = child(ser, "tx").map(|t| points(t).into_iter().map(|(_, v)| v).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        let color = child(ser, "spPr").and_then(|sp| sp.descendants().find(|n| is(n, "srgbClr"))).and_then(|c| attr(&c, "val")).map(|v| format!("#{}", v.to_lowercase()));
        if let Some(cat) = child(ser, "cat") {
            for (i, v) in points(cat) {
                if i >= MAX_CATEGORIES { continue }
                if categories.len() <= i { categories.resize(i + 1, String::new()) }
                if categories[i].is_empty() { categories[i] = v }
            }
        }
        let mut values = vec![];
        if let Some(val) = child(ser, "val") {
            for (i, v) in points(val) {
                let Ok(v) = v.parse::<f64>() else { continue };
                if i >= MAX_CATEGORIES || !v.is_finite() { continue }
                if values.len() <= i { values.resize(i + 1, 0.0) }
                values[i] = v;
            }
        }
        if !values.is_empty() { series.push(Series { name, color, values }) }
    }
    let n = series.iter().map(|s| s.values.len()).max()?;
    for s in &mut series { s.values.resize(n, 0.0) }
    categories.resize(n, String::new());
    Some(Chart { kind, stacked, title: title.trim().to_string(), categories, series })
}

/// A number as a label: whole when it is, else up to two decimals.
fn label(v: f64) -> String {
    if v.fract().abs() < 1e-9 { return format!("{v:.0}") }
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn color_of(s: &Series, i: usize) -> String { s.color.clone().unwrap_or_else(|| PALETTE[i % PALETTE.len()].to_string()) }

fn rect(fill: &str, r: PctRect) -> Obj {
    let mut o = Obj::new(ObjKind::Shape { shape: Shape::Rect }, r);
    o.look = Look { fill: Some(fill.to_string()), ..Look::default() };
    o
}

fn words(text: impl Into<String>, size: f32, align: HAlign, r: PctRect) -> Obj {
    let style = TextStyle { size, color: WORDS.into(), align, valign: VAlign::Middle, shrink: true, ..TextStyle::default() };
    Obj::new(ObjKind::text(text, style), r)
}

/// The most objects one chart may become: a slide holds `design::MAX_OBJECTS` in all.
const MAX_PARTS: usize = 40;
/// The editor keeps no object thinner than this (% of the slide): thinner bars would run into each other.
const THIN: f32 = 2.0;

/// The chart as objects inside `at` (percent of the slide), back to front. Value labels are left out when they
/// would make it too many objects; `None` when it is too many even so, or its bars would be too thin to tell
/// apart (the caller's grey box says what was there).
pub fn draw(c: &Chart, at: PctRect) -> Option<Vec<Obj>> {
    [true, false].into_iter().find_map(|labels| build(c, at, labels).filter(|o| o.len() <= MAX_PARTS))
}

fn build(c: &Chart, at: PctRect, labels: bool) -> Option<Vec<Obj>> {
    let (x, y, w, h) = (at.x, at.y, at.w, at.h);
    // Text sizes follow the chart's height (1080 px is 100 %).
    let size = (h * 10.8 * 0.055).clamp(14.0, 48.0);
    let mut out = vec![rect(PANEL, at)];
    let pad = 0.03;
    let (mut top, mut bottom) = (y + h * pad, y + h * (1.0 - pad));
    if !c.title.is_empty() {
        out.push(words(c.title.clone(), size * 1.4, HAlign::Center, PctRect::new(x + w * pad, top, w * (1.0 - 2.0 * pad), h * 0.13)));
        top += h * 0.14;
    }
    // The legend: one line of coloured squares and names. A pie's is its categories with their shares.
    let pie_total: f64 = c.series.first().map(|s| s.values.iter().map(|v| v.max(0.0)).sum()).unwrap_or(0.0);
    let legend: Vec<(String, String)> = match c.kind {
        Kind::Pie => c.series.first().map(|s| s.values.iter().enumerate().map(|(i, v)| {
            let share = if pie_total > 0.0 { v.max(0.0) / pie_total * 100.0 } else { 0.0 };
            (PALETTE[i % PALETTE.len()].to_string(), format!("{} {}%", c.categories[i], label((share * 10.0).round() / 10.0)).trim().to_string())
        }).collect()).unwrap_or_default(),
        _ if c.series.len() > 1 || c.series.iter().any(|s| !s.name.is_empty()) => c.series.iter().enumerate().map(|(i, s)| (color_of(s, i), s.name.clone())).collect(),
        _ => vec![],
    };
    if !legend.is_empty() {
        let mut runs = vec![];
        for (i, (color, name)) in legend.iter().enumerate() {
            if i > 0 { runs.push(Run::plain("   ")) }
            runs.push(Run { text: "■ ".into(), color: Some(color.clone()), ..Run::default() });
            runs.push(Run::plain(name.clone()));
        }
        let style = TextStyle { size: size * 0.8, color: WORDS.into(), align: HAlign::Center, valign: VAlign::Middle, shrink: true, ..TextStyle::default() };
        let lh = h * 0.11;
        out.push(Obj::new(ObjKind::rich(vec![Para { runs, ..Para::default() }], style), PctRect::new(x + w * pad, bottom - lh, w * (1.0 - 2.0 * pad), lh)));
        bottom -= lh + h * 0.01;
    }
    let (left, right) = (x + w * 0.05, x + w * 0.95);
    let n = c.categories.len().max(1);
    match c.kind {
        Kind::Pie => {
            // One bar cut into the shares, in the middle of what is left.
            let s = c.series.first()?;
            let bar_h = (bottom - top) * 0.45;
            let by = top + (bottom - top - bar_h) / 2.0;
            let mut bx = left;
            for (i, v) in s.values.iter().enumerate() {
                let bw = if pie_total > 0.0 { (v.max(0.0) / pie_total) as f32 * (right - left) } else { 0.0 };
                if bw > 0.0 { out.push(rect(PALETTE[i % PALETTE.len()], PctRect::new(bx, by, bw, bar_h))) }
                bx += bw;
            }
        }
        Kind::Lines => {
            let names_h = h * 0.09;
            let plot_bottom = bottom - names_h;
            let all = c.series.iter().flat_map(|s| s.values.iter().copied());
            let (lo, hi) = all.fold((0.0f64, 0.0f64), |(lo, hi), v| (lo.min(v), hi.max(v)));
            let span = if hi > lo { hi - lo } else { 1.0 };
            let plot = PctRect::new(left, top, right - left, plot_bottom - top);
            for (i, s) in c.series.iter().enumerate() {
                let points: Vec<(f32, f32)> = s.values.iter().enumerate()
                    .map(|(k, v)| (if n > 1 { k as f32 / (n - 1) as f32 } else { 0.5 }, (1.0 - (v - lo) / span) as f32)).collect();
                out.push(Obj::new(ObjKind::Path { points, width: 6.0, color: color_of(s, i) }, plot));
            }
            let slot = (right - left) / n as f32;
            for (k, name) in c.categories.iter().enumerate().filter(|(_, name)| !name.is_empty()) {
                // A line's points sit on the ends, so the names are centred on them.
                let cx = if n > 1 { left + (right - left) * k as f32 / (n - 1) as f32 } else { (left + right) / 2.0 };
                out.push(words(name.clone(), size * 0.7, HAlign::Center, PctRect::new((cx - slot / 2.0).max(x), plot_bottom, slot.min(w), names_h)));
            }
        }
        Kind::Columns | Kind::Bars => {
            let upright = c.kind == Kind::Columns;
            // Along the categories (`a`) and along the values (`v`): for columns left→right and bottom→top.
            let names = if upright { h * 0.09 } else { w * 0.16 };
            let (a0, a1) = if upright { (left, right) } else { (top, bottom) };
            let (v0, v1) = if upright { (bottom - names, top + h * 0.06) } else { (left + names, right - w * 0.06) };
            let totals = |k: usize, pos: bool| -> f64 { c.series.iter().map(|s| s.values[k]).filter(|v| (*v >= 0.0) == pos).sum() };
            let (lo, hi) = if c.stacked {
                (0..n).fold((0.0f64, 0.0f64), |(lo, hi), k| (lo.min(totals(k, false)), hi.max(totals(k, true))))
            } else {
                c.series.iter().flat_map(|s| s.values.iter().copied()).fold((0.0f64, 0.0f64), |(lo, hi), v| (lo.min(v), hi.max(v)))
            };
            let span = if hi > lo { hi - lo } else { 1.0 };
            let at_value = |v: f64| v0 + (v1 - v0) * ((v - lo) / span) as f32;
            let slot = (a1 - a0) / n as f32;
            let per = if c.stacked { 1 } else { c.series.len().max(1) };
            let thick = slot * 0.8 / per as f32;
            if thick < THIN { return None }
            let few = labels && n * per <= 16;
            let put = |out: &mut Vec<Obj>, color: &str, a: f32, from: f64, to: f64, text: Option<String>| {
                let (p, q) = (at_value(from), at_value(to));
                let (start, len) = (p.min(q), (p - q).abs());
                let bar = if upright { PctRect::new(a, start, thick, len) } else { PctRect::new(start, a, len, thick) };
                if len > 0.0 { out.push(rect(color, bar)) }
                if let Some(t) = text {
                    let r = if upright { PctRect::new(a - thick * 0.25, start - h * 0.055, thick * 1.5, h * 0.05) } else { PctRect::new(start + len + w * 0.005, a, w * 0.055, thick) };
                    out.push(words(t, size * 0.6, if upright { HAlign::Center } else { HAlign::Left }, r));
                }
            };
            for k in 0..n {
                let a = a0 + slot * k as f32 + slot * 0.1;
                let (mut up, mut down) = (0.0f64, 0.0f64);
                for (i, s) in c.series.iter().enumerate() {
                    let v = s.values[k];
                    let color = color_of(s, i);
                    if c.stacked {
                        let from = if v >= 0.0 { up } else { down };
                        if v >= 0.0 { up += v } else { down += v }
                        put(&mut out, &color, a, from, from + v, None);
                    } else {
                        // A value's label sits past the end of a bar that goes up or right.
                        put(&mut out, &color, a + thick * i as f32, 0.0, v, (few && v >= 0.0).then(|| label(v)));
                    }
                }
                let name = &c.categories[k];
                if !name.is_empty() {
                    let r = if upright { PctRect::new(a0 + slot * k as f32, bottom - names, slot, names) } else { PctRect::new(left, a0 + slot * k as f32, names - w * 0.01, slot) };
                    out.push(words(name.clone(), size * 0.7, if upright { HAlign::Center } else { HAlign::Right }, r));
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
#[path = "chart_tests.rs"]
mod tests;
