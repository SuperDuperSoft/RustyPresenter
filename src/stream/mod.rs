//! Livestreaming, step one (design: `docs/superpowers/specs/2026-11-09-livestreaming-design.md`, §8 "Go Live
//! basics"): where a stream goes, and the one FFmpeg output that writes the recording and sends the same picture
//! and sound to every destination that is on (`args::record` with `stream`). Pure, so all tested here.
//!
//! Not built yet: the Go Live button and the Settings list, reconnecting, the speed test, scenes and cameras.
//! Nothing in the app starts a stream. Keys are kept in the computer's password store (`keys`).
//!
//! A stream key is a password: whoever has it can broadcast as the church. `Destination::shown` is the only
//! form of an address that may reach a log, a notice or the screen.
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

pub mod keys;

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
pub fn tee(file: &Path, addresses: &[String]) -> String {
    let mut parts = vec![format!("[f=matroska]{}", tee_escape(&file.to_string_lossy().replace('\\', "/")))];
    for a in addresses {
        // FLV is what RTMP carries; SRT carries MPEG-TS.
        let format = if a.to_ascii_lowercase().starts_with("srt://") { "mpegts" } else { "flv" };
        parts.push(format!("[f={format}:onfail=ignore]{}", tee_escape(a)));
    }
    parts.join("|")
}

/// A stream's title from a pattern: `{date}` is the day ("November 15, 2026"), `{service}` the service's name.
pub fn title(pattern: &str, date: chrono::NaiveDate, service: &str) -> String {
    pattern.replace("{date}", &date.format("%B %-d, %Y").to_string()).replace("{service}", service).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dest(service: Service, url: &str, key: &str) -> Destination {
        Destination { id: Uuid::new_v4(), name: String::new(), service, url: url.into(), key: key.into(), on: true }
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
            &["rtmp://a.rtmp.youtube.com/live2/y|1".to_string(), "srt://h.example.org:9000?streamid=abc".to_string()]);
        assert_eq!(t, "[f=matroska]C:/Users/A B/Videos/RustyPresenter/2026-11-15 10.30.part.mkv\
            |[f=flv:onfail=ignore]rtmp://a.rtmp.youtube.com/live2/y\\|1|[f=mpegts:onfail=ignore]srt://h.example.org:9000?streamid=abc");
        assert_eq!(tee(Path::new("it's [1].mkv"), &[]), "[f=matroska]it\\'s \\[1\\].mkv");
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
