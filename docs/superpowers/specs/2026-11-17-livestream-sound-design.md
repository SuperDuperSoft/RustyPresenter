# The livestream's sound — design (November 17, 2026)

Branch `ls2-sound`. Livestreaming design §4 item 5: what was still missing from the stream's sound. Easy settings
stay as they are; everything new is either one "Sound setup" choice or under "More sound settings".

## 1. Sound setup (simple modes)

One list in Settings → Media → Livestream, "Sound setup":

| Choice | Inputs it sets | Filters it sets |
|---|---|---|
| Sound board or mixer | the first input that looks like a mixer or sound interface (USB Audio CODEC, Behringer, Yamaha, Focusrite, "Line"…) | nothing but the safety limiter: the sound person has done the rest |
| USB microphone | the first input that looks like a USB microphone (Yeti, Rode, Samson, "USB"…) | light gate, light compressor, a little less low and more high, hum and hiss out |
| Computer's microphone | the computer's own microphone (Realtek, "Microphone Array", built-in…) | strong gate, strong compressor, much less low, hum and hiss out |
| The app's sound only | no microphone; the recording takes the app's sound | the safety limiter |
| Custom | nothing (what is set below) | — |

Every choice also turns the computer's sound on (the output the app plays to) and the second and third inputs off.
When no input looks right, the one chosen stays and the help text says to pick it in "Sound in the recording". A
setting changed by hand afterwards makes the list say "Custom" (`stream::sound_setup::matches`).

## 2. More sound settings

A "More sound settings" check opens, for each sound (the microphone or line-in, the second and third ones, this
computer):
- **Gate** (Off / Light / Strong): quiets the microphone between words (`agate`).
- **Evener** (Off / Light / Strong): brings loud moments down and quiet ones up (`acompressor`).
- **Never too loud** (on by default): nothing goes over −1 dB (`alimiter=limit=0.89`).
- **Low, Middle, High** (−12 to +12 dB): `bass` at 100 Hz, `equalizer` at 1 kHz, `treble` at 8 kHz.

Order on each sound: hum and hiss out, gate, low/middle/high, evener, its level, limiter. Then everything is mixed,
held back (sound delay) and evened out as before. With two or more sounds and any limiter on, a last limiter sits
after the mix, so the sum of two loud sounds never clips. Saved as `RecordPrefs::stream_filters` (left out when all
are as they start). `SoundFx::default()` (a recording without the stream) still does nothing to the sound.

## 3. Up to three inputs

"Second microphone or line-in" and "Third…" (shown once the one before is chosen), each with its level and filters.
Each goes through `board_feed` like the first (Mute, unplugging, its own meter level). FFmpeg's inputs: 0 picture, 1
first input, then the computer's sound, then the others; the mix names them `[b]`, `[p]`, `[b2]`, `[b3]`. With
"each sound on its own track" every one gets a track, named.

## 4. The phone as a microphone

A small page of its own, `/stage-view/mic` (the same access and password as the stage view and the tally page). The
phone's browser asks for the microphone (`getUserMedia`), takes its sound at 48 kHz (`AudioContext` +
`ScriptProcessor`, which every phone browser has), turns it into 16-bit numbers and POSTs 100 ms at a time to
`/stage-view/mic/sound`. Not a WebSocket: the app's web server (axum without its WebSocket part) and the crates in
the lock file have none, and ten small posts a second on the church's own network are fine. The app keeps the
newest phone sound in one place (`stream::phone_mic`); a sound input named "Phone microphone" reads it like any
other input (`board_feed`, so silence by the clock when the phone stops, Mute and the meter work).

**The one-time step.** Phones only let a page use the microphone over a secure connection. The app also serves its
pages over `https://` on the next port (phone remote port + 1), with a certificate it makes itself the first time
(`network::secure`, ECDSA P-256 with `ring`, kept in the data folder). Nobody vouches for that certificate, so the
first time each phone opens the page it warns "not private"; the person taps Advanced (Android) or Show Details
(iPhone) and goes on. Settings says this in those words, and shows the address.

## 5. Listen to the stream

"Listen to the stream" in Settings: Off, the usual output, or any output. While live, the recorder's FFmpeg sends a
copy of the mix (after filters and the sound delay, as NDI gets it) to the app as raw 48 kHz stereo; the app plays
it on that output (`stream::listen`): Windows with WASAPI (the `windows` crate already used for the computer's
sound), Linux through `pacat` (pulseaudio-utils), macOS through FFmpeg's `audiotoolbox` output (the default output
only: FFmpeg cannot open a Core Audio output by its id). The app reads as fast as FFmpeg writes and keeps at most a
quarter second, so the stream never waits for the headphones.

## 6. Linux and macOS

- Auto-follow's own listening on Linux uses `record::devices::sound_reader` too (FFmpeg's PulseAudio, else `parec`).
- The `.deb` recommends `pulseaudio-utils` (cargo-packager has no Recommends field: a small script adds it to the
  package after it is built, `packaging/linux/deb-recommends.sh`).
- macOS 14.2+: the computer's sound through a Core Audio process tap (`stream::mac_tap`, `objc2` + plain Core Audio
  calls), no BlackHole needed. Older macOS, or a tap that cannot start: BlackHole as before. Written without a Mac:
  not compiled or tried.

## Tests

Arguments of every filter and of the mix with one, two, three and four sounds, tracks, NDI and Listen copies; the
setup choices and input picking; the prefs round trip; the phone sound's conversion and the hub; the phone page's
access; the certificate (parsed back with rustls/webpki); Listen's holding buffer and players' arguments; a real
FFmpeg run of the full filter chain; Settings read/write in the window.
