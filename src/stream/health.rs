//! The Go Live panel's health light and status list (livestreaming design §4 item 9): one light for the whole
//! stream, and a line for each destination and for the camera, the sound, the computer and the captions, in plain
//! words. Pure: the operator's ticker gathers what the recording knows (`Inputs`) once a second.
use super::send::Link;
use crate::tr;

/// How a part of the stream is doing, and the whole stream's light (the worst of its parts).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Light {
    #[default]
    Good,
    /// Going out, but something wants looking at (a destination reconnecting or on a lighter stream, no sound).
    Look,
    /// Nothing is reaching the viewers.
    Bad,
}

impl Light {
    /// For the screen: 0 green, 1 amber, 2 red.
    pub fn index(self) -> i32 { self as i32 }
}

/// One line of the status list.
#[derive(Clone, Debug, PartialEq)]
pub struct Line { pub light: Light, pub text: String }

/// What the recording knows, once a second.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    /// Each destination's name and its connection, in order.
    pub links: Vec<(String, Link)>,
    /// Going out over NDI (`None`: NDI out is off).
    pub ndi: bool,
    /// The camera in the picture is sending pictures (`None`: no camera).
    pub camera: Option<bool>,
    /// The sound input (its name) and whether it has gone quiet; `None`: no sound input.
    pub sound: Option<(String, bool)>,
    /// The computer is not keeping up.
    pub slow: bool,
    /// Hold and Mute are on.
    pub held: bool,
    pub muted: bool,
    /// Live captions: `None` off; else their trouble, if any.
    pub captions: Option<Option<String>>,
    /// Tries a destination may fail before it is said not to connect (`ui::operator::record::NEVER_TRIES`).
    pub never_after: u32,
}

/// One destination's line.
pub fn link_line(name: &str, l: &Link, never_after: u32) -> Line {
    if l.up && l.lightest && l.skips > 0 {
        Line { light: Light::Look, text: tr!("{name}: going out, but the internet is too slow for it", name = name) }
    } else if l.up && l.kbps > 0 {
        Line { light: Light::Look, text: tr!("{name}: going out, lighter ({kbps} kbit/s) for a slow internet", name = name, kbps = l.kbps.to_string()) }
    } else if l.up {
        Line { light: Light::Good, text: tr!("{name}: going out", name = name) }
    } else if l.failed >= never_after.max(1) {
        Line { light: Light::Bad, text: tr!("{name}: not connecting. Check its stream key and the internet; still trying", name = name) }
    } else {
        Line { light: Light::Look, text: tr!("{name}: connecting…", name = name) }
    }
}

/// The status list and the whole stream's light.
pub fn status(i: &Inputs) -> (Light, Vec<Line>) {
    let mut lines: Vec<Line> = i.links.iter().map(|(n, l)| link_line(n, l, i.never_after)).collect();
    if i.ndi { lines.push(Line { light: Light::Good, text: tr!("NDI: going out") }) }
    match i.camera {
        Some(true) => lines.push(Line { light: Light::Good, text: tr!("Camera: picture coming in") }),
        Some(false) => lines.push(Line { light: Light::Look, text: tr!("Camera: no picture (the room screen is shown instead)") }),
        None => {}
    }
    match &i.sound {
        Some((name, false)) => lines.push(Line { light: Light::Good, text: tr!("Sound: {name}", name = name) }),
        Some((name, true)) => lines.push(Line { light: Light::Look, text: tr!("Sound: nothing from {name} for 10 seconds", name = name) }),
        None => {}
    }
    if i.slow { lines.push(Line { light: Light::Look, text: tr!("This computer is not keeping up: the picture is jerky") }) }
    if i.held { lines.push(Line { light: Light::Look, text: tr!("Frozen: viewers see a still picture") }) }
    if i.muted { lines.push(Line { light: Light::Look, text: tr!("Muted: viewers hear nothing") }) }
    match &i.captions {
        Some(None) => lines.push(Line { light: Light::Good, text: tr!("Live captions: on") }),
        Some(Some(why)) => lines.push(Line { light: Light::Look, text: why.clone() }),
        None => {}
    }
    // The whole light: red only when nothing reaches anyone (every destination down, and no NDI).
    let any_up = i.links.iter().any(|(_, l)| l.up) || i.ndi;
    let worst = lines.iter().map(|l| l.light).filter(|l| *l != Light::Bad).max().unwrap_or_default();
    let light = if !any_up && i.links.iter().any(|(_, l)| l.failed >= i.never_after.max(1)) { Light::Bad }
        else if !any_up { Light::Look }
        else if lines.iter().any(|l| l.light == Light::Bad) { Light::Look }
        else { worst };
    (light, lines)
}

/// The words beside the light.
pub fn summary(light: Light) -> String {
    match light {
        Light::Good => tr!("All good"),
        Light::Look => tr!("Check the stream"),
        Light::Bad => tr!("Not going out"),
    }
}

/// A broadcast's title or description from its template: `{date}` the day ("November 15, 2026"), `{service}` the
/// service's name, `{time}` the start time ("10:30 AM"), `{weekday}` the day's name ("Sunday").
pub fn fill(template: &str, at: chrono::NaiveDateTime, service: &str) -> String {
    let t = super::title(template, at.date(), service);
    t.replace("{time}", &at.format("%-I:%M %p").to_string()).replace("{weekday}", &at.format("%A").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(up: bool, failed: u32, kbps: u32) -> Link { Link { up, failed, kbps, ..Link::default() } }

    #[test]
    fn each_destination_says_how_it_is_doing() {
        assert_eq!(link_line("YouTube", &link(true, 0, 0), 3), Line { light: Light::Good, text: "YouTube: going out".into() });
        assert_eq!(link_line("YouTube", &link(false, 1, 0), 3).light, Light::Look);
        assert_eq!(link_line("YouTube", &link(false, 3, 0), 3).light, Light::Bad);
        let lighter = link_line("Facebook", &link(true, 0, 2500), 3);
        assert_eq!(lighter.light, Light::Look);
        assert!(lighter.text.contains("2500"), "{}", lighter.text);
        let slow = Link { up: true, lightest: true, skips: 2, ..Link::default() };
        assert!(link_line("Twitch", &slow, 3).text.contains("too slow"));
    }

    #[test]
    fn the_light_is_red_only_when_nothing_reaches_anyone() {
        let all_good = Inputs { links: vec![("YouTube".into(), link(true, 0, 0)), ("Facebook".into(), link(true, 0, 0))], camera: Some(true), never_after: 3, ..Inputs::default() };
        assert_eq!(status(&all_good).0, Light::Good);
        assert_eq!(status(&all_good).1.len(), 3);
        let one_down = Inputs { links: vec![("YouTube".into(), link(true, 0, 0)), ("Facebook".into(), link(false, 5, 0))], never_after: 3, ..Inputs::default() };
        assert_eq!(status(&one_down).0, Light::Look, "YouTube still gets it");
        let none = Inputs { links: vec![("YouTube".into(), link(false, 5, 0))], never_after: 3, ..Inputs::default() };
        assert_eq!(status(&none).0, Light::Bad);
        let starting = Inputs { links: vec![("YouTube".into(), link(false, 0, 0))], never_after: 3, ..Inputs::default() };
        assert_eq!(status(&starting).0, Light::Look, "still connecting");
        let ndi_only = Inputs { ndi: true, ..Inputs::default() };
        assert_eq!(status(&ndi_only).0, Light::Good);
        let quiet = Inputs { links: vec![("YouTube".into(), link(true, 0, 0))], sound: Some(("Line In".into(), true)), never_after: 3, ..Inputs::default() };
        assert_eq!(status(&quiet).0, Light::Look);
        assert_eq!(summary(Light::Bad), "Not going out");
    }

    #[test]
    fn templates_take_the_date_time_and_service() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap().and_hms_opt(10, 30, 0).unwrap();
        assert_eq!(fill("Sunday Service — {date}", at, "YouTube"), "Sunday Service — November 15, 2026");
        assert_eq!(fill("{weekday} {time} on {service}", at, "Facebook"), "Sunday 10:30 AM on Facebook");
        assert_eq!(fill("Welcome! No placeholders.", at, "YouTube"), "Welcome! No placeholders.");
    }
}
