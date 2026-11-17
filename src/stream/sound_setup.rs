//! The stream's sound set up with one choice (livestreaming design §4 item 5, "simple modes for small setups"):
//! a sound board, a USB microphone, the computer's own microphone, or the app's sound only. Each choice picks the
//! input that looks right and the filters that suit it; anything changed by hand afterwards is "Custom". Also each
//! sound's filters as Settings keeps them (More sound settings).
use crate::record::{RecordPrefs, Sound, args::{Filters, Input, Strength}};
use serde::{Deserialize, Serialize};

/// The choices in Settings' "Sound setup", in their order there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// What is set by hand.
    #[default]
    Custom,
    Board,
    UsbMic,
    PcMic,
    AppOnly,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Custom, Mode::Board, Mode::UsbMic, Mode::PcMic, Mode::AppOnly];
    pub fn index(self) -> i32 { Mode::ALL.iter().position(|m| *m == self).unwrap_or(0) as i32 }
    pub fn from_index(i: i32) -> Mode { usize::try_from(i).ok().and_then(|i| Mode::ALL.get(i)).copied().unwrap_or_default() }
    pub fn is_custom(&self) -> bool { *self == Mode::Custom }
}

/// Each sound's filters (More sound settings). Every one starts with only the limiter on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamFilters {
    /// The microphone or line-in (the recording's sound input).
    pub board: Filters,
    /// The second and third microphone or line-in.
    pub input2: Filters,
    pub input3: Filters,
    /// This computer's sound.
    pub pc: Filters,
}

impl Default for StreamFilters {
    fn default() -> StreamFilters { StreamFilters { board: Filters::SAFE, input2: Filters::SAFE, input3: Filters::SAFE, pc: Filters::SAFE } }
}

impl StreamFilters {
    pub fn is_default(&self) -> bool { *self == StreamFilters::default() }
}

/// What a setup sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    /// It wants a microphone or line-in (else the app's sound only).
    pub input: bool,
    pub board: Filters,
    pub pc: Filters,
    /// Hum and hiss taken out of the microphone.
    pub quiet_noise: bool,
    /// Loud and quiet moments evened out (the whole mix).
    pub even: bool,
}

/// What `mode` sets; `None` for Custom.
pub fn setup(mode: Mode) -> Option<Setup> {
    let safe = Filters::SAFE;
    Some(match mode {
        Mode::Custom => return None,
        // The sound person has already gated, evened and balanced it: only kept from crackling.
        Mode::Board => Setup { input: true, board: safe, pc: safe, quiet_noise: false, even: true },
        // Close to the mouth: a little less boom, a little more clarity, quiet between words.
        Mode::UsbMic => Setup { input: true, board: Filters { gate: Strength::Light, even: Strength::Light, low_db: -3, high_db: 2, ..safe }, pc: safe, quiet_noise: true, even: true },
        // Far from the people speaking, near the fan and the room: much less boom, strong gate and evener.
        Mode::PcMic => Setup { input: true, board: Filters { gate: Strength::Strong, even: Strength::Strong, low_db: -6, mid_db: 2, ..safe }, pc: safe, quiet_noise: true, even: true },
        Mode::AppOnly => Setup { input: false, board: safe, pc: safe, quiet_noise: false, even: true },
    })
}

/// Words in an input's name that say what it is (lower case).
const VIRTUAL: [&str; 7] = ["stereo mix", "what u hear", "cable output", "blackhole", "voicemeeter", ".monitor", "loopback"];
const BOARD: [&str; 22] = ["codec", "line in", "line-in", "linein", "line (", "mixer", "behringer", "yamaha", "mackie", "focusrite", "scarlett",
    "presonus", "soundcraft", "allen", "zoom", "umc", "x32", "xr18", "x-usb", "audient", "motu", "tascam"];
const USB_MIC: [&str; 18] = ["yeti", "snowball", "blue ", "rode", "røde", "samson", "audio-technica", "at2020", "atr2100", "shure", "hyperx",
    "quadcast", "fifine", "elgato", "seiren", "maono", "tonor", "usb mic"];
const BUILT_IN: [&str; 13] = ["realtek", "microphone array", "mic array", "internal", "built-in", "builtin", "integrated", "macbook", "imac",
    "intel", "conexant", "high definition audio", "alsa_input.pci"];

/// The input among `inputs` that looks like what `mode` uses; `None`: none does (or the mode uses none).
pub fn pick_input(mode: Mode, inputs: &[Input]) -> Option<&Input> {
    let text = |i: &Input| format!("{} {}", i.name, i.label).to_lowercase();
    let has = |i: &Input, words: &[&str]| { let t = text(i); words.iter().any(|w| t.contains(w)) };
    let real: Vec<&Input> = inputs.iter().filter(|i| !has(i, &VIRTUAL) && i.name != super::phone_mic::INPUT).collect();
    let first = |ok: &dyn Fn(&Input) -> bool| real.iter().copied().find(|i| ok(i));
    let usb = |i: &Input| text(i).contains("usb");
    match mode {
        Mode::Board => first(&|i| has(i, &BOARD)).or_else(|| first(&|i| usb(i) && !has(i, &USB_MIC) && !has(i, &BUILT_IN))),
        Mode::UsbMic => first(&|i| has(i, &USB_MIC)).or_else(|| first(&|i| usb(i) && !has(i, &BOARD))),
        Mode::PcMic => first(&|i| has(i, &BUILT_IN) && !has(i, &BOARD) && !usb(i))
            .or_else(|| first(&|i| text(i).contains("mic") && !usb(i) && !has(i, &BOARD) && !has(i, &USB_MIC))),
        Mode::Custom | Mode::AppOnly => None,
    }
}

/// Puts `mode` into `p`, picking its input from `inputs` (when none looks right the one chosen stays). The levels
/// stay as they are. True when it wanted an input and found none.
pub fn apply(mode: Mode, p: &mut RecordPrefs, inputs: &[Input]) -> bool {
    p.stream_sound_mode = mode;
    let Some(s) = setup(mode) else { return false };
    let mut missing = false;
    if s.input {
        match pick_input(mode, inputs) {
            Some(i) => p.sound = Sound::Input(i.name.clone()).key(),
            None => missing = !matches!(p.sound(), Sound::Input(_)),
        }
    } else {
        p.sound = Sound::App.key();
    }
    // The computer's sound goes out too (the output the app plays to, unless one is chosen).
    if p.stream_sound == "off" { p.stream_sound = String::new() }
    p.stream_input2.clear();
    p.stream_input3.clear();
    let filters = StreamFilters { board: s.board, pc: s.pc, ..StreamFilters::default() };
    p.stream_filters = (!filters.is_default()).then_some(filters);
    p.stream_quiet_noise = s.quiet_noise;
    p.stream_even = if s.even { None } else { Some(false) };
    missing
}

/// Whether `p` is still as `mode` set it (else Settings shows Custom). The input itself and the levels may differ.
pub fn matches(mode: Mode, p: &RecordPrefs) -> bool {
    let Some(s) = setup(mode) else { return true };
    let f = p.filters();
    let input_ok = if s.input { matches!(p.sound(), Sound::Input(_)) } else { p.sound() == Sound::App };
    input_ok && f.board == s.board && f.pc == s.pc && p.stream_quiet_noise == s.quiet_noise && p.stream_even.unwrap_or(true) == s.even
        && p.stream_sound != "off" && p.stream_input2.is_empty() && p.stream_input3.is_empty()
}

pub mod tr {
    use crate::tr;
    /// "Sound setup"'s choices, in `Mode::ALL`'s order.
    pub fn modes() -> Vec<String> {
        vec![tr!("Custom (as set below)"), tr!("Sound board or mixer"), tr!("USB microphone"), tr!("Computer's microphone"), tr!("The app's sound only")]
    }
    /// The recording's track of the second or third microphone (`n` is 2 or 3).
    pub fn track_input(n: usize) -> String {
        if n == 2 { tr!("Second microphone or line-in") } else { tr!("Third microphone or line-in") }
    }
    pub fn no_input_found() -> String {
        tr!("No sound input here looks like one. Choose it under \"Sound in the recording\" above.")
    }
    pub fn no_extra() -> String { tr!("None") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i(name: &str) -> Input { Input { name: name.into(), label: name.into() } }

    #[test]
    fn each_setup_picks_the_input_that_looks_right() {
        let inputs = [i("Stereo Mix (Realtek(R) Audio)"), i("Microphone Array (Realtek(R) Audio)"), i("Microphone (Yeti Stereo Microphone)"),
            i("Line In (Behringer UMC202HD 192k)"), i("CABLE Output (VB-Audio Virtual Cable)")];
        let name = |m| pick_input(m, &inputs).map(|i| i.name.as_str());
        assert_eq!(name(Mode::Board), Some("Line In (Behringer UMC202HD 192k)"));
        assert_eq!(name(Mode::UsbMic), Some("Microphone (Yeti Stereo Microphone)"));
        assert_eq!(name(Mode::PcMic), Some("Microphone Array (Realtek(R) Audio)"), "never Stereo Mix");
        assert_eq!((name(Mode::AppOnly), name(Mode::Custom)), (None, None));
        // A mixer's USB sound shows as "USB Audio CODEC"; a plain USB microphone as "USB PnP Sound Device".
        let usb = [i("Microphone (USB PnP Sound Device)"), i("Microphone (2- USB Audio CODEC )")];
        assert_eq!(pick_input(Mode::Board, &usb).map(|i| i.name.as_str()), Some("Microphone (2- USB Audio CODEC )"));
        assert_eq!(pick_input(Mode::UsbMic, &usb).map(|i| i.name.as_str()), Some("Microphone (USB PnP Sound Device)"));
        // Linux names: the description says what it is.
        let linux = [Input { name: "alsa_input.pci-0000_00_1f.3.analog-stereo".into(), label: "Built-in Audio Analog Stereo".into() }];
        assert_eq!(pick_input(Mode::PcMic, &linux).map(|i| i.label.as_str()), Some("Built-in Audio Analog Stereo"));
        assert_eq!(pick_input(Mode::Board, &linux), None);
        assert_eq!(pick_input(Mode::UsbMic, &[i(crate::stream::phone_mic::INPUT)]), None, "never the phone");
    }

    #[test]
    fn a_setup_sets_the_inputs_and_filters_and_stays_until_changed_by_hand() {
        let inputs = [i("Microphone Array (Realtek(R) Audio)"), i("Microphone (Yeti Stereo Microphone)")];
        let mut p = RecordPrefs { stream_sound: "off".into(), stream_input2: "Line 2".into(), stream_board_db: 5, ..RecordPrefs::default() };
        assert!(!apply(Mode::UsbMic, &mut p, &inputs));
        assert_eq!(p.sound(), Sound::Input("Microphone (Yeti Stereo Microphone)".into()));
        assert_eq!((p.stream_sound.as_str(), p.stream_input2.as_str(), p.stream_board_db), ("", "", 5), "the PC's sound on, the second input off, the level kept");
        assert_eq!(p.filters().board, setup(Mode::UsbMic).unwrap().board);
        assert!(p.stream_quiet_noise && matches(Mode::UsbMic, &p));
        // Changed by hand: no longer that setup.
        let mut q = p.clone();
        q.stream_filters.as_mut().unwrap().board.high_db = 5;
        assert!(!matches(Mode::UsbMic, &q));
        let mut q = p.clone();
        q.stream_quiet_noise = false;
        assert!(!matches(Mode::UsbMic, &q));
        // The app's sound only: no microphone; the filters as they start, so nothing is saved for them.
        apply(Mode::AppOnly, &mut p, &inputs);
        assert_eq!((p.sound(), p.stream_filters), (Sound::App, None));
        assert!(matches(Mode::AppOnly, &p) && !matches(Mode::Board, &p) && matches(Mode::Custom, &p));
        // No input looks like a sound board: the one chosen stays, and nothing is said while there is one.
        let mut p = RecordPrefs::default();
        assert!(apply(Mode::Board, &mut p, &inputs), "said: none found");
        assert_eq!(p.sound(), Sound::App);
        let mut p = RecordPrefs { sound: "input:Mic".into(), ..RecordPrefs::default() };
        assert!(!apply(Mode::Board, &mut p, &inputs));
        assert_eq!(p.sound(), Sound::Input("Mic".into()));
    }

    #[test]
    fn modes_have_a_place_in_settings() {
        for m in Mode::ALL { assert_eq!(Mode::from_index(m.index()), m) }
        assert_eq!(Mode::from_index(-1), Mode::Custom);
        assert_eq!(tr::modes().len(), Mode::ALL.len());
        assert_eq!(serde_json::to_string(&Mode::UsbMic).unwrap(), "\"usb_mic\"");
    }
}
