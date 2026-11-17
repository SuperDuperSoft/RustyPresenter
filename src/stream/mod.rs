//! Livestreaming, step one (design: `docs/superpowers/specs/2026-11-09-livestreaming-design.md`, §8 "Go Live
//! basics"): where a stream goes, and the one FFmpeg output that writes the recording and sends the same picture
//! and sound to every destination that is on (`args::record` with `stream`). Pure, so all tested here.
//!
//! The recorder sends to local feeds and a sender for each destination passes them on and reconnects (`send`).
//! Keys are kept in the computer's password store (`keys`). Not built yet: the speed test, scenes and cameras.
//!
//! A stream key is a password: whoever has it can broadcast as the church. `Destination::shown` is the only
//! form of an address that may reach a log, a notice or the screen.
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

pub mod accounts;
pub mod board_feed;
pub mod camera_lost;
pub mod camera_feed;
pub mod chat;
pub mod compose;
pub mod delay;
pub mod keys;
pub mod level;
pub mod live_info;
pub mod lyrics;
pub mod ndi;
pub mod own_scenes;
pub mod pc_sound;
pub mod phone_camera;
pub mod pictures;
pub mod ptz;
pub mod scene;
pub mod send;
pub mod speed;
pub mod tally;
pub mod words_file;

/// A place a stream can be sent.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    #[default]
    YouTube,
    Facebook,
    Twitch,
    /// Any other server: its own address (RTMP, RTMPS or SRT) and, where it has one, a key.
    Custom,
}

impl Service {
    /// The service's own address for incoming streams; a custom one has none.
    pub fn ingest(self) -> Option<&'static str> {
        match self {
            Service::YouTube => Some("rtmp://a.rtmp.youtube.com/live2"),
            Service::Facebook => Some("rtmps://live-api-s.facebook.com:443/rtmp"),
            Service::Twitch => Some("rtmp://live.twitch.tv/app"),
            Service::Custom => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self { Service::YouTube => "YouTube", Service::Facebook => "Facebook", Service::Twitch => "Twitch", Service::Custom => "Custom" }
    }
}

/// One destination of the stream.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Destination {
    pub id: Uuid,
    /// The operator's own name for it ("Church YouTube"); empty: the service's.
    pub name: String,
    pub service: Service,
    /// A custom server's address; not used by the others.
    pub url: String,
    /// The stream key (a password). Never shown or logged (see `shown`) and never saved with the data: it is
    /// kept in the computer's password store and put here when it is needed (`keys::fill`).
    #[serde(skip)]
    pub key: String,
    pub on: bool,
    /// Gets the tall picture (9:16, for phones: Shorts, Reels, TikTok) instead of the wide one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tall: bool,
}

/// Why a destination cannot be streamed to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem { NoKey, NoAddress, BadAddress }

impl Destination {
    pub fn title(&self) -> &str { if self.name.trim().is_empty() { self.service.name() } else { self.name.trim() } }

    /// The address FFmpeg sends to: the server, then the key. A custom SRT address is used as it is (its
    /// "streamid" is part of the address).
    pub fn address(&self) -> Result<String, Problem> {
        let key = self.key.trim();
        let base = match self.service.ingest() {
            Some(b) => {
                if key.is_empty() { return Err(Problem::NoKey) }
                b.to_string()
            }
            None => {
                let url = self.url.trim();
                if url.is_empty() { return Err(Problem::NoAddress) }
                let ok = ["rtmp://", "rtmps://", "srt://"].iter().any(|s| url.len() > s.len() && url.to_ascii_lowercase().starts_with(s));
                if !ok || url.chars().any(|c| c.is_whitespace() || c.is_control()) { return Err(Problem::BadAddress) }
                url.to_string()
            }
        };
        if key.chars().any(|c| c.is_whitespace() || c.is_control()) { return Err(Problem::BadAddress) }
        if key.is_empty() || base.to_ascii_lowercase().starts_with("srt://") { return Ok(base) }
        Ok(format!("{}/{key}", base.trim_end_matches('/')))
    }

    /// The address as it may be shown or logged: the key is left out.
    pub fn shown(&self) -> String {
        let base = self.service.ingest().map(str::to_string).unwrap_or_else(|| {
            // A custom address may carry its key itself (SRT's streamid, a query): only the server is shown.
            let url = self.url.trim();
            let end = url.find(['?', '#']).unwrap_or(url.len());
            url[..end].to_string()
        });
        if self.key.trim().is_empty() { base } else { format!("{}/…", base.trim_end_matches('/')) }
    }
}

/// The addresses of the destinations that are on and can be streamed to, and the titles of those that are on
/// but cannot (with why), for the operator.
pub fn ready(all: &[Destination]) -> (Vec<String>, Vec<(String, Problem)>) {
    let mut good = vec![];
    let mut bad = vec![];
    for d in all.iter().filter(|d| d.on) {
        match d.address() {
            Ok(a) => good.push(a),
            Err(p) => bad.push((d.title().to_string(), p)),
        }
    }
    (good, bad)
}

/// A file name or address inside FFmpeg's `tee` output: `\`, `'` and the `|` between outputs are escaped.
fn tee_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if matches!(c, '\\' | '\'' | '|' | '[' | ']') { out.push('\\') }
        out.push(c);
    }
    out
}

/// FFmpeg's `tee` output: the recording `file` (Matroska, which survives a crash) and each address. A
/// destination that fails (a wrong key, the internet gone) is dropped and the others, and the recording, go on.
/// With `mix_only` (the file has more sound tracks than the mix), each address gets the picture and the first
/// sound only.
pub fn tee(file: &Path, addresses: &[String], mix_only: bool) -> String {
    let mut parts = vec![format!("[f=matroska]{}", tee_escape(&file.to_string_lossy().replace('\\', "/")))];
    parts.extend(addresses.iter().map(|a| tee_part(a, mix_only)));
    parts.join("|")
}

/// FFmpeg's `tee` output for `addresses` alone (the tall stream: no file).
pub fn tee_only(addresses: &[String]) -> String { addresses.iter().map(|a| tee_part(a, false)).collect::<Vec<_>>().join("|") }

/// One address in a `tee` output, with the format it carries.
fn tee_part(a: &str, mix_only: bool) -> String {
    {
        // FLV is what RTMP carries; SRT carries MPEG-TS.
        let lower = a.to_ascii_lowercase();
        let format = if lower.starts_with("srt://") || lower.starts_with("udp://") { "mpegts" } else { "flv" };
        // MPEG-TS has no header of its own: the picture's is put in again at every keyframe, so whoever
        // starts reading in the middle (a sender that reconnects) can show it within two seconds.
        let extra = if format == "mpegts" { ":bsfs/v=dump_extra" } else { "" };
        // The quotes are escaped once for the tee's own reading, so its options see `'v,a:0'` whole.
        let select = if mix_only { ":select=\\'v,a:0\\'" } else { "" };
        format!("[f={format}{extra}{select}:onfail=ignore]{}", tee_escape(a))
    }
}

/// `line` (something FFmpeg said) with every stream key of `addresses` taken out: the last part of an address,
/// and anything after a `?`. So a log or a notice never holds a key.
pub fn redact(line: &str, addresses: &[String]) -> String {
    let mut out = line.to_string();
    for a in addresses {
        let after_scheme = a.find("://").map_or(0, |i| i + 3);
        let mut secrets: Vec<&str> = vec![];
        if let Some(q) = a.find('?') { secrets.push(&a[q + 1..]) }
        let path = &a[after_scheme..a.find('?').unwrap_or(a.len())];
        if let Some(slash) = path.rfind('/') { secrets.push(&path[slash + 1..]) }
        for s in secrets.into_iter().filter(|s| s.len() >= 4) { out = out.replace(s, "…") }
    }
    out
}

/// What the operator is told about streaming.
pub mod tr {
    use crate::tr;
    /// After a stream: how long it ran, and how each destination did (`(name, drops, ever connected)`).
    pub fn summary(secs: u64, rows: &[(String, u32, bool)]) -> String {
        let mut parts = vec![tr!("The stream ran {time}.", time = crate::record::clock(secs))];
        for (name, drops, ever) in rows {
            parts.push(match (*ever, *drops) {
                (false, _) => tr!("{name}: never connected.", name = name),
                (true, 0) => tr!("{name}: connected the whole time.", name = name),
                (true, 1) => tr!("{name}: dropped once and came back.", name = name),
                (true, n) => tr!("{name}: dropped {n} times and came back.", name = name, n = n),
            });
        }
        parts.join(" ")
    }
    pub fn chat_line(name: &str, service: &str, text: &str) -> String { tr!("{name} · {service}: {text}", name = name, service = service, text = text) }
    pub fn live_ndi_only() -> String { tr!("Live over NDI only: no destinations are switched on. Recording too.") }
    pub fn live_on(names: &str) -> String { tr!("Going live on {names}. Recording too.", names = names) }
    pub fn connected(name: &str) -> String { tr!("{name}: the stream is going out.", name = name) }
    pub fn never_connected(name: &str) -> String { tr!("{name} has not connected after several tries. Check the stream key in Settings → Media → Livestream, and the internet. Still trying; the recording goes on.", name = name) }
    pub fn lost(name: &str) -> String { tr!("{name} lost its connection. Trying again by itself; the recording goes on.", name = name) }
    pub fn ndi_missing() -> String { tr!("Nothing goes out over NDI: it needs the free NDI Tools from ndi.video on this computer. The livestream goes on without it.") }
    pub fn nothing_ready() -> String { tr!("Go Live needs a destination that is switched on and has its stream key. Set one up in Settings → Media → Livestream.") }
    pub fn not_ready(name: &str, why: &str) -> String { tr!("{name} is not streamed to: {why}", name = name, why = why) }
    pub fn no_key() -> String { tr!("it has no stream key") }
    pub fn no_address() -> String { tr!("it has no address") }
    pub fn bad_address() -> String { tr!("its address or key is not one a stream can be sent to") }
    pub fn lighter(name: &str, kbps: u32) -> String {
        tr!("{name}: the internet there is slow, so it gets a lighter stream ({kbps} kbit/s). The recording and the other destinations keep the full picture.", name = name, kbps = kbps)
    }
    pub fn full_again(name: &str) -> String { tr!("{name} gets the full stream again.", name = name) }
    pub fn camera_lost() -> String { tr!("The camera stopped. The stream shows the slides until it is back; it is tried again by itself.") }
    pub fn camera_never() -> String { tr!("The camera is not sending pictures, so the stream shows the slides. Check that it is plugged in and that no other program is using it; it is tried again by itself.") }
    pub fn camera_back() -> String { tr!("The camera is back in the stream.") }
    pub fn too_slow(name: &str) -> String {
        tr!("{name}: the internet is not fast enough, so the stream skips. Choose 720p in Settings → Media → Recording, or switch a destination off.", name = name)
    }
    pub fn licence_note() -> String {
        tr!("You are live. Streaming worship songs needs a streaming licence (for example CCLI's Streaming License) as well as your church's usual copyright licence, and videos or music in the stream need their owners' permission too. This note is shown once.")
    }
    pub fn stop_recording_first() -> String { tr!("A recording is under way. Stop it, then press Go Live.") }
    /// macOS: the computer's own sound needs a loopback device (`pc_sound`).
    pub fn mac_needs_loopback() -> String {
        tr!("A Mac cannot send its own sound by itself. Install the free BlackHole, make a Multi-Output Device of the speakers and BlackHole in Audio MIDI Setup, and choose it as the app's sound output.")
    }
}

impl Problem {
    pub fn words(&self) -> String {
        match self { Problem::NoKey => tr::no_key(), Problem::NoAddress => tr::no_address(), Problem::BadAddress => tr::bad_address() }
    }
}

/// The four destinations Settings offers, each with an id of its own that never changes (its key is kept under
/// it): YouTube, Facebook, Twitch and one custom server.
pub const BUILT_IN: [(Service, u128); 4] = [(Service::YouTube, 0x5254_5354_5245_414d_0000_0000_0000_0001), (Service::Facebook, 0x5254_5354_5245_414d_0000_0000_0000_0002),
    (Service::Twitch, 0x5254_5354_5245_414d_0000_0000_0000_0003), (Service::Custom, 0x5254_5354_5245_414d_0000_0000_0000_0004)];

/// The id of Settings' row `i`.
pub fn built_in_id(i: usize) -> Uuid { Uuid::from_u128(BUILT_IN[i.min(3)].1) }

/// A stream's title from a pattern: `{date}` is the day ("November 15, 2026"), `{service}` the service's name.
pub fn title(pattern: &str, date: chrono::NaiveDate, service: &str) -> String {
    pattern.replace("{date}", &date.format("%B %-d, %Y").to_string()).replace("{service}", service).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dest(service: Service, url: &str, key: &str) -> Destination {
        Destination { id: Uuid::new_v4(), name: String::new(), service, url: url.into(), key: key.into(), on: true, tall: false }
    }

    #[test]
    fn a_services_address_is_its_server_and_the_key() {
        assert_eq!(dest(Service::YouTube, "", " abcd-1234 ").address().as_deref(), Ok("rtmp://a.rtmp.youtube.com/live2/abcd-1234"));
        assert_eq!(dest(Service::Facebook, "", "FB-9?s_bl=1").address().as_deref(), Ok("rtmps://live-api-s.facebook.com:443/rtmp/FB-9?s_bl=1"));
        assert_eq!(dest(Service::Twitch, "", "live_1").address().as_deref(), Ok("rtmp://live.twitch.tv/app/live_1"));
        assert_eq!(dest(Service::YouTube, "", "  ").address(), Err(Problem::NoKey));
        assert_eq!(dest(Service::YouTube, "", "two words").address(), Err(Problem::BadAddress));
    }

    #[test]
    fn a_custom_server_gives_its_own_address() {
        assert_eq!(dest(Service::Custom, "rtmp://stream.example.org/live/", "k1").address().as_deref(), Ok("rtmp://stream.example.org/live/k1"));
        assert_eq!(dest(Service::Custom, "RTMPS://stream.example.org/live", "").address().as_deref(), Ok("RTMPS://stream.example.org/live"));
        // SRT keeps its key in the address itself.
        assert_eq!(dest(Service::Custom, "srt://h.example.org:9000?streamid=abc", "ignored").address().as_deref(), Ok("srt://h.example.org:9000?streamid=abc"));
        assert_eq!(dest(Service::Custom, "", "k").address(), Err(Problem::NoAddress));
        for bad in ["http://example.org/live", "example.org", "rtmp://", "rtmp://a b/c", "file:///c:/x"] {
            assert_eq!(dest(Service::Custom, bad, "k").address(), Err(Problem::BadAddress), "{bad}");
        }
    }

    #[test]
    fn what_is_shown_never_has_the_key() {
        let d = dest(Service::YouTube, "", "SECRET-KEY");
        assert_eq!(d.shown(), "rtmp://a.rtmp.youtube.com/live2/…");
        let c = dest(Service::Custom, "srt://h.example.org:9000?streamid=SECRET-KEY", "");
        assert_eq!(c.shown(), "srt://h.example.org:9000");
        let k = dest(Service::Custom, "rtmp://stream.example.org/live", "SECRET-KEY");
        for s in [d.shown(), c.shown(), k.shown()] { assert!(!s.contains("SECRET"), "{s}") }
        assert_eq!((d.title(), Destination { name: " Church YouTube ".into(), ..d.clone() }.title()), ("YouTube", "Church YouTube"));
    }

    #[test]
    fn only_destinations_that_are_on_and_complete_are_streamed_to() {
        let off = Destination { on: false, ..dest(Service::Twitch, "", "t") };
        let (good, bad) = ready(&[dest(Service::YouTube, "", "y"), off, dest(Service::Facebook, "", ""), dest(Service::Custom, "ftp://x", "k")]);
        assert_eq!(good, ["rtmp://a.rtmp.youtube.com/live2/y"]);
        assert_eq!(bad, [("Facebook".to_string(), Problem::NoKey), ("Custom".to_string(), Problem::BadAddress)]);
    }

    #[test]
    fn the_tee_output_keeps_the_recording_and_lets_a_destination_fail() {
        let t = tee(Path::new(r"C:\Users\A B\Videos\RustyPresenter\2026-11-15 10.30.part.mkv"),
            &["rtmp://a.rtmp.youtube.com/live2/y|1".to_string(), "srt://h.example.org:9000?streamid=abc".to_string()], false);
        assert_eq!(t, "[f=matroska]C:/Users/A B/Videos/RustyPresenter/2026-11-15 10.30.part.mkv\
            |[f=flv:onfail=ignore]rtmp://a.rtmp.youtube.com/live2/y\\|1|[f=mpegts:bsfs/v=dump_extra:onfail=ignore]srt://h.example.org:9000?streamid=abc");
        assert_eq!(tee(Path::new("it's [1].mkv"), &[], false), "[f=matroska]it\\'s \\[1\\].mkv");
        // A file with sound tracks of its own: the destinations get the picture and the mix.
        assert_eq!(tee(Path::new("r.mkv"), &["rtmp://x.example/live/k".to_string(), "udp://127.0.0.1:20001?pkt_size=1316".to_string()], true),
            "[f=matroska]r.mkv|[f=flv:select=\\'v,a:0\\':onfail=ignore]rtmp://x.example/live/k|[f=mpegts:bsfs/v=dump_extra:select=\\'v,a:0\\':onfail=ignore]udp://127.0.0.1:20001?pkt_size=1316");
    }

    #[test]
    fn what_ffmpeg_says_is_logged_without_the_keys() {
        let to = ["rtmp://a.rtmp.youtube.com/live2/abcd-1234-SECRET".to_string(), "srt://h.example.org:9000?streamid=PASS-9876".to_string()];
        let said = "[tee @ 0x1] Slave '[f=flv:onfail=ignore]rtmp://a.rtmp.youtube.com/live2/abcd-1234-SECRET': error opening: I/O error; srt://h.example.org:9000?streamid=PASS-9876";
        let shown = redact(said, &to);
        assert!(!shown.contains("SECRET") && !shown.contains("PASS-9876"), "{shown}");
        assert!(shown.contains("rtmp://a.rtmp.youtube.com/live2/…") && shown.contains("srt://h.example.org:9000?…"), "{shown}");
        assert_eq!(redact("frame= 100 fps=30", &to), "frame= 100 fps=30");
        assert_eq!(built_in_id(0), built_in_id(0));
        assert_ne!(built_in_id(0), built_in_id(3));
    }

    #[test]
    fn a_title_from_a_pattern() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 5).unwrap();
        assert_eq!(title("{service} — {date}", day, "Sunday Service"), "Sunday Service — November 5, 2026");
        assert_eq!(title(" Live ", day, "x"), "Live");
    }

    #[test]
    fn destinations_are_saved_and_read_back() {
        let d = dest(Service::Custom, "rtmp://x.example/live", "k");
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"service\":\"custom\""), "{json}");
        // All of it but the key, which is not kept with the data (`keys`).
        assert_eq!(serde_json::from_str::<Destination>(&json).unwrap(), Destination { key: String::new(), ..d });
        assert_eq!(serde_json::from_str::<Destination>("{}").unwrap(), Destination::default());
    }
}
