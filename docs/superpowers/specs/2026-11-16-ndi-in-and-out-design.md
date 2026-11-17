# NDI in and out (livestream design §4.4 and §4.10)

Jake, November 16, 2026: "Yes, install NDI". The NDI 6 Tools (runtime, Test Patterns, Studio Monitor) are installed
on the development PC.

## Runtime

- NDI is proprietary. The app never links it and never ships it: `stream::ndi` loads `Processing.NDI.Lib.x64.dll`
  at run time with `libloading`, as `video::ffi` loads libmpv. It looks in the folder named by `NDI_RUNTIME_DIR_V6`
  (set by the NDI Tools / Runtime installer), then `NDI_RUNTIME_DIR_V5`, then the system search path (`libndi.so.6`,
  `libndi.dylib` on Linux and macOS).
- Without the runtime nothing breaks. NDI cameras are not listed, and turning NDI out on says at Go Live that the
  free NDI Tools from ndi.video are needed.
- Only the few calls needed are declared (find, receive, send), with the SDK's v2 frame structs.

## In: NDI cameras

- Settings → Media → Livestream → Camera lists the NDI sources found on the network ("NDI: PC (OBS)"), beside the
  USB cameras. They are looked for for 1.5 s on the same background thread that lists the cameras.
- A chosen source is saved as `ndi://<source name>`. `record::args::camera_input` turns it into a marker that
  `stream::camera_feed::start` recognises. It then runs an NDI receiver instead of FFmpeg, into the same `Latest`
  slot, scaled to the stream's size with black bars (`record::pump::fit`). Unplugging, retrying, the "camera lost"
  light, switching and the tally all work as for any camera.

## Out: the stream and the words

Settings → Media → Livestream, two checks, both off to start with:

1. **"Send the stream over NDI too (for OBS, vMix or another computer)"**: an NDI source named
   `RustyPresenter Stream`. It carries the stream's picture as the writer makes it, before encoding, so there is
   no encoding delay. It also carries the stream's sound mix: the recorder's FFmpeg sends a copy of the mix as raw
   48 kHz float to a local connection, and the app passes it on. The livestream's delay does not apply.
2. **"Send the words alone over NDI, on a see-through background (for keying)"**: an NDI source named
   `RustyPresenter Words`. It carries the words over the camera (the lower third) with an alpha channel, and is
   clear when there are none. It is sent when it changes and once a second otherwise.

Both run while the livestream runs: they use the stream's picture and scenes. NDI out alone is enough to Go
Live (Jake, November 16, 2026: "whatever is best long term"): with no destinations switched on, Go Live runs
the scenes and NDI out and makes the recording, for a church that streams from OBS.

## Not now

- NDI PTZ, NDI tally back to the camera, alpha recording.
