//! Service planning (FEAT-6a, `docs/superpowers/specs/2026-11-01-feat6-service-planning-design.md` §5): service
//! dates, Copy last week, templates, blanks and headers. Everything here is pure; `actions` runs it on `Data`.
#[path = "../strings/planning.rs"]
pub mod strings;
pub mod actions;

use crate::model::{BlankKind, DateOrder, ItemKind, Paper, Service, ServiceItem, ServiceTemplate, Settings};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use uuid::Uuid;

/// The colours a header can have, in the order the colour button steps through them.
pub const HEADER_COLORS: [&str; 6] = ["#4aa3ff", "#3ecf8e", "#f5a524", "#e5484d", "#a78bfa", "#8b95a5"];

/// A header's colour: its own when it is one of `HEADER_COLORS`, else the first.
pub fn header_color(color: &str) -> &'static str {
    HEADER_COLORS.iter().find(|c| c.eq_ignore_ascii_case(color)).copied().unwrap_or(HEADER_COLORS[0])
}

/// The colour after `color`, for the header's colour button.
pub fn next_header_color(color: &str) -> &'static str {
    let i = HEADER_COLORS.iter().position(|c| c.eq_ignore_ascii_case(color)).map_or(0, |i| i + 1);
    HEADER_COLORS[i % HEADER_COLORS.len()]
}

pub fn blank_title(label: &str, want: BlankKind) -> String {
    let asked;
    strings::blank_title(if label.trim().is_empty() { asked = strings::blank_label(want); &asked } else { label })
}

// ---------- region ----------

/// Date order and paper from a locale such as "en-US" or "de_DE.UTF-8" (§5.2). No region: ISO and Letter.
pub fn region_defaults(locale: Option<&str>) -> (DateOrder, Paper) {
    let region = locale.and_then(|l| {
        let l = l.split(['.', '@']).next().unwrap_or("");
        l.split(['-', '_']).skip(1).find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic())).map(|p| p.to_ascii_uppercase())
    });
    let Some(r) = region else { return (DateOrder::Ymd, Paper::Letter) };
    let order = match r.as_str() {
        "US" | "PH" | "FM" | "MH" | "PW" => DateOrder::Mdy,
        "CA" | "CN" | "JP" | "KR" | "TW" | "HU" | "LT" | "MN" | "IR" => DateOrder::Ymd,
        _ => DateOrder::Dmy,
    };
    let paper = if matches!(r.as_str(), "US" | "CA" | "MX" | "PH" | "CL" | "CO" | "VE") { Paper::Letter } else { Paper::A4 };
    (order, paper)
}

/// This computer's defaults.
pub fn system_defaults() -> (DateOrder, Paper) { region_defaults(sys_locale::get_locale().as_deref()) }

/// The date order to use: Settings' own, else the computer's.
pub fn date_order(settings: &Settings) -> DateOrder { settings.date_order.unwrap_or_else(|| system_defaults().0) }

/// The paper to use: the Print dialog's last choice, else the computer's.
pub fn paper(settings: &Settings) -> Paper { settings.print.paper.unwrap_or_else(|| system_defaults().1) }

// ---------- dates ----------

/// 11/8/2026, 8/11/2026 or 2026-11-08.
pub fn format_date(d: NaiveDate, order: DateOrder) -> String {
    match order {
        DateOrder::Mdy => format!("{}/{}/{}", d.month(), d.day(), d.year()),
        DateOrder::Dmy => format!("{}/{}/{}", d.day(), d.month(), d.year()),
        DateOrder::Ymd => d.format("%Y-%m-%d").to_string(),
    }
}

/// "Sun 11/8/2026".
pub fn format_day_date(d: NaiveDate, order: DateOrder) -> String {
    format!("{} {}", strings::days_short()[d.weekday().num_days_from_monday() as usize], format_date(d, order))
}

/// "Sunday, November 8, 2026", "Sunday 8 November 2026" or "Sunday, 2026-11-08".
pub fn format_date_long(d: NaiveDate, order: DateOrder) -> String {
    let day = strings::days_long()[d.weekday().num_days_from_monday() as usize].clone();
    let month = strings::months()[d.month0() as usize].clone();
    match order {
        DateOrder::Mdy => format!("{day}, {month} {}, {}", d.day(), d.year()),
        DateOrder::Dmy => format!("{day} {} {month} {}", d.day(), d.year()),
        DateOrder::Ymd => format!("{day}, {}", d.format("%Y-%m-%d")),
    }
}

fn year(y: &str) -> Option<i32> {
    let n: i32 = y.parse().ok()?;
    match y.len() { 2 => Some(2000 + n), 4 => Some(n), _ => None }
}

/// Every date `token` can be read as: "a/b/y" as month/day and as day/month, "y-m-d" as ISO.
fn readings(token: &str) -> Vec<(DateOrder, NaiveDate)> {
    let num = |s: &str| (!s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse::<u32>().ok()).flatten();
    let mut out = vec![];
    let dash: Vec<&str> = token.split('-').collect();
    if dash.len() == 3 && dash[0].len() == 4 && let (Some(y), Some(m), Some(d)) = (year(dash[0]), num(dash[1]), num(dash[2])) {
        if let Some(x) = NaiveDate::from_ymd_opt(y, m, d) { out.push((DateOrder::Ymd, x)) }
    }
    let parts: Vec<&str> = token.split(['/', '.']).collect();
    if parts.len() == 3 && let (Some(a), Some(b), Some(y)) = (num(parts[0]), num(parts[1]), year(parts[2])) {
        if let Some(x) = NaiveDate::from_ymd_opt(y, a, b) { out.push((DateOrder::Mdy, x)) }
        if let Some(x) = NaiveDate::from_ymd_opt(y, b, a) { out.push((DateOrder::Dmy, x)) }
    }
    out
}

/// A date typed by the operator, read in `order` (or as ISO, which is never ambiguous).
pub fn parse_date(text: &str, order: DateOrder) -> Option<NaiveDate> {
    let r = readings(text.trim());
    r.iter().find(|(o, _)| *o == order).or_else(|| r.iter().find(|(o, _)| *o == DateOrder::Ymd)).map(|(_, d)| *d)
}

/// `name` without its trailing date (§5.2). The date text is taken off only when it reads as exactly `old`; with
/// no `old`, when it reads as a date in `order` or ISO. A name that was only a date becomes "Service".
pub fn strip_date(name: &str, old: Option<NaiveDate>, order: DateOrder) -> String {
    let trimmed = name.trim_end();
    let (head, token) = match trimmed.rfind(char::is_whitespace) {
        Some(i) => (&trimmed[..i], &trimmed[i..]),
        None => ("", trimmed),
    };
    let r = readings(token.trim());
    let strip = match old {
        Some(old) => r.iter().any(|(_, d)| *d == old),
        None => r.iter().any(|(o, _)| *o == order || *o == DateOrder::Ymd),
    };
    let base = if strip { head.trim_end().trim_end_matches(['-', '–']).trim_end() } else { trimmed };
    if base.is_empty() { strings::service().into() } else { base.into() }
}

/// `name` with its trailing date replaced by `new`, or with `new` added.
pub fn dated_name(name: &str, old: Option<NaiveDate>, new: NaiveDate, order: DateOrder) -> String {
    format!("{} {}", strip_date(name, old, order), format_date(new, order))
}

/// The service "Copy last week" copies (§5.2): the latest one dated today or earlier (the later one in the list on
/// a tie), else the open one.
pub fn week_source(services: &[Service], current: Option<Uuid>, today: NaiveDate) -> Option<Uuid> {
    let mut best: Option<&Service> = None;
    for s in services {
        if let Some(d) = s.date.filter(|d| *d <= today) && best.is_none_or(|b| b.date.is_some_and(|bd| d >= bd)) { best = Some(s) }
    }
    best.map(|s| s.id).or(current.filter(|c| services.iter().any(|s| s.id == *c))).or(services.first().map(|s| s.id))
}

/// The first `day` on or after `today`.
pub fn default_date(day: Option<Weekday>, today: NaiveDate) -> NaiveDate {
    let Some(day) = day else { return today };
    let ahead = (7 + day.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64) % 7;
    today + Duration::days(ahead)
}

/// The new service's date: a week after the source's, or the same weekday from today when that has passed. An
/// undated source: the next Sunday (today if Sunday).
pub fn next_week_date(source: Option<NaiveDate>, today: NaiveDate) -> NaiveDate {
    match source {
        Some(d) if d + Duration::days(7) >= today => d + Duration::days(7),
        Some(d) => default_date(Some(d.weekday()), today),
        None => default_date(Some(Weekday::Sun), today),
    }
}

/// The service already dated `date`, if any.
pub fn find_on_date(services: &[Service], date: NaiveDate) -> Option<Uuid> {
    services.iter().find(|s| s.date == Some(date)).map(|s| s.id)
}

/// Services newest first; undated ones after, in the order they are stored (§3.1).
pub fn sorted<'a>(services: &'a [Service]) -> Vec<&'a Service> {
    let mut v: Vec<&Service> = services.iter().collect();
    v.sort_by(|a, b| match (a.date, b.date) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    v
}

// ---------- items ----------

/// What a blank in place of this item asks for. `None` for blanks and headers.
pub fn blank_kind(kind: &ItemKind) -> Option<BlankKind> {
    Some(match kind {
        ItemKind::Song { .. } => BlankKind::Song,
        ItemKind::Scripture { .. } => BlankKind::Scripture,
        ItemKind::Media { .. } => BlankKind::Media,
        ItemKind::Deck { .. } => BlankKind::Deck,
        ItemKind::Text { .. } | ItemKind::Design { .. } => BlankKind::Text,
        ItemKind::Blank { .. } | ItemKind::Header { .. } => return None,
    })
}

/// `item` turned into a blank of its kind, with its notes and background (§5.6).
pub fn blanked(item: &ServiceItem) -> ServiceItem {
    let want = blank_kind(&item.kind).unwrap_or_default();
    ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Blank { label: strings::blank_label(want).into(), want }, background: item.background.clone(), notes: item.notes.clone(), theme: item.theme.clone() }
}

pub fn is_blank(item: &ServiceItem) -> bool { matches!(item.kind, ItemKind::Blank { .. }) }

pub fn blank_count(items: &[ServiceItem]) -> usize { items.iter().filter(|i| is_blank(i)).count() }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyMode { LastWeek { blanks: bool }, ThisService }

/// The items of a copy with new ids, and how many blanks it has (§5.6). With blanks on, songs and Bible readings
/// become blanks. Copy last week drops a song's order for that service (FEAT-4 D16); Copy this service keeps it.
pub fn copy_items(items: &[ServiceItem], mode: CopyMode) -> (Vec<ServiceItem>, usize) {
    let last_week = matches!(mode, CopyMode::LastWeek { .. });
    let blanks = matches!(mode, CopyMode::LastWeek { blanks: true });
    let out: Vec<ServiceItem> = items.iter().map(|i| {
        if blanks && matches!(i.kind, ItemKind::Song { .. } | ItemKind::Scripture { .. }) { return blanked(i) }
        let mut copy = ServiceItem { id: Uuid::new_v4(), ..i.clone() };
        if last_week && let ItemKind::Song { arrangement, .. } = &mut copy.kind { *arrangement = None }
        copy
    }).collect();
    let n = blank_count(&out);
    (out, n)
}

/// A new dated service copied from `source` (Copy last week).
pub fn copy_week(source: &Service, date: NaiveDate, blanks: bool, order: DateOrder) -> Service {
    let (items, _) = copy_items(&source.items, CopyMode::LastWeek { blanks });
    Service { id: Uuid::new_v4(), name: dated_name(&source.name, source.date, date, order), items, date: Some(date), template: source.template, theme: source.theme.clone() }
}

/// A template's name from a service's: its trailing date taken off.
pub fn template_name(service: &Service, order: DateOrder) -> String { strip_date(&service.name, service.date, order) }

/// A template from a service (§5.3): songs and Bible readings become blanks; everything else is kept.
pub fn template_from_service(service: &Service, name: &str) -> ServiceTemplate {
    let items = service.items.iter().map(|i| match i.kind {
        ItemKind::Song { .. } | ItemKind::Scripture { .. } => blanked(i),
        _ => ServiceItem { id: Uuid::new_v4(), ..i.clone() },
    }).collect();
    ServiceTemplate { id: Uuid::new_v4(), name: name.trim().to_string(), day: Some(service.date.map_or(Weekday::Sun, |d| d.weekday())), items }
}

/// A new service from a template (§5.3), with fresh ids.
pub fn service_from_template(t: &ServiceTemplate, name: &str, date: NaiveDate) -> Service {
    let items = t.items.iter().map(|i| ServiceItem { id: Uuid::new_v4(), ..i.clone() }).collect();
    Service { id: Uuid::new_v4(), name: name.to_string(), items, date: Some(date), template: Some(t.id), theme: None }
}

/// "Sundays" or "Any day".
pub fn day_label(day: Option<Weekday>) -> String {
    day.map_or_else(strings::any_day, |d| strings::days_plural()[d.num_days_from_monday() as usize].clone())
}

/// The item that fills `blank` (§5.6): the blank's id; its notes, then the chosen item's after a blank line; the
/// chosen item's background, else the blank's.
pub fn fill(blank: &ServiceItem, chosen: ServiceItem) -> ServiceItem {
    let notes = match (blank.notes.trim().is_empty(), chosen.notes.trim().is_empty()) {
        (true, _) => chosen.notes,
        (false, true) => blank.notes.clone(),
        (false, false) => format!("{}\n\n{}", blank.notes, chosen.notes),
    };
    ServiceItem { id: blank.id, kind: chosen.kind, background: chosen.background.or_else(|| blank.background.clone()), notes, theme: chosen.theme.or_else(|| blank.theme.clone()) }
}

#[cfg(test)]
mod tests;
