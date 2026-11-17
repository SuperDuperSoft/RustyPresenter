# Livestream and recording on macOS and Linux — design (November 17, 2026)

Branch `ls-macos-linux`. The livestream and recording were built Windows-first. This makes every part that was
Windows-only right for macOS and Linux too, as far as it can be done and checked from the Windows development PC.
No new native code and no new crates: what the app already builds with, plus programs every Linux desktop has.

## What was Windows-only, and what each becomes

| Part | Windows (as before) | Linux | macOS |
|---|---|---|---|
| The computer's own sound (`stream::pc_sound`) | WASAPI loopback of the chosen output | The output's **monitor** source in PulseAudio/PipeWire (`<sink>.monitor`; the default output: `@DEFAULT_MONITOR@`), read like a sound input | No loopback without new native code (ScreenCaptureKit / Core Audio taps). The app reads a **loopback device** (BlackHole, free) as a sound input: the one whose name matches the chosen output, else the first one found. None: a plain message, and Settings says what to install |
| Reading a sound input (`stream::board_feed`) | FFmpeg `dshow` | FFmpeg `pulse`; when that FFmpeg was built without PulseAudio (static builds), `parec` (pulseaudio-utils, on every desktop with PulseAudio or PipeWire's pulse layer) | FFmpeg `avfoundation` |
| Listing sound inputs | FFmpeg `-list_devices` | FFmpeg `-sources pulse`; none listed: `pactl list sources` | FFmpeg `-list_devices` (avfoundation) |
| Listing cameras | FFmpeg `-list_devices` | `/sys/class/video4linux` (each camera's capture node, not its metadata node), read by FFmpeg `v4l2` | FFmpeg `-list_devices` (avfoundation) |
| Sound outputs (from libmpv) | `wasapi/…` only | `pipewire/…`, else `pulse/…` (each output is listed again under ALSA, JACK, SDL) | `coreaudio/…` |
| Stream keys and sign-ins (`stream::keys`) | Credential Manager | The desktop's password store over D-Bus (Secret Service: GNOME Keyring, KWallet, KeePassXC), with `zbus`, which the app already uses on Linux. No store running: keys last until the app closes, as before | The login Keychain (`security-framework`, already in the lock file for macOS) |
| Video encoder | x264, NVIDIA, Intel, AMD, Windows' own | the same list (NVIDIA/Intel work on Linux too) | the Mac's own (`h264_videotoolbox`), new in the list and used when x264 is missing |
| NDI runtime | `Processing.NDI.Lib.x64.dll` | `libndi.so.6` (also `libndi.so.5`), from `NDI_RUNTIME_DIR_V6`/`V5`, `/usr/local/lib`, `/usr/lib` | `libndi.dylib` from `/usr/local/lib` and the NDI SDK folder |
| Camera and microphone permission | none | none | `NSCameraUsageDescription` / `NSMicrophoneUsageDescription` in the app's Info.plist, and the hardened runtime's camera and audio-input entitlements; FFmpeg runs as the app's child, so the app is what asks |

## Linux: the computer's sound

mpv names an output `pipewire/<node>` or `pulse/<sink>`; the sink's monitor is `<sink>.monitor`. "The output the
app plays its sound to" with no output chosen is `@DEFAULT_MONITOR@`, which PulseAudio and pipewire-pulse both
understand. The capture is a `board_feed` of that source, so the gap filling by the clock, Mute and the level meter
are the same code as for the sound board. (PulseAudio sends silence from a monitor rather than nothing, but a
stopped or missing reader is still filled by the clock.)

## macOS: the computer's sound

macOS has no way to hear an output without new native code. BlackHole (free, open source) is a sound output that is
also a sound input. The operator plays the app's sound to a Multi-Output Device made of the speakers and BlackHole
(Audio MIDI Setup), and the app reads BlackHole. The choice is a pure function of the chosen output's name and the
inputs FFmpeg lists. Later, a native Core Audio tap (macOS 14.2+) could replace it; that needs a Mac to build and try.

## Tests

Pure functions, tested on Windows: the monitor source name; reading `ffmpeg -devices`; `parec` arguments;
`pactl list sources` parsing; the sysfs camera list; the macOS loopback choice; the output list per platform; the
NDI places per platform; the Secret Service's attributes. The cross checks (`cargo check --target`) show how far
the macOS/Linux code compiles here; nothing of it can run here.
