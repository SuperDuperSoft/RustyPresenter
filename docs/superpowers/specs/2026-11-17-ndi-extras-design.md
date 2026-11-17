# NDI extras: cameras that move, their tally lights, and the words alone in a file

Follows `2026-11-16-ndi-in-and-out-design.md` ("Not now": NDI PTZ, NDI tally back to the camera, alpha
recording). Branch `ls-ndi-extras`, November 17, 2026.

## 1. NDI cameras that can move

Many church PTZ cameras send NDI (PTZOptics, BirdDog, Panasonic, Avonic and others). Over NDI the camera is moved
through the same connection that brings its picture, so no control address is needed.

- `stream::ptz::Control` becomes "how this camera is moved": `Visca { host, port }` (as today) or `Ndi(source)`.
  `Control::of(camera, address)`: a control address in Settings wins (VISCA, as today); otherwise a camera that is
  an NDI source is moved over NDI. `control_of`, `moves` and `recall` go through it, so the operator's position
  list while live, macro steps and slide actions ("Camera 2: position 3") work the same for NDI cameras.
- Positions are the camera's own NDI presets: position 1 is preset 0 (as VISCA, which also counts from 0), called
  back at full speed (`NDIlib_recv_ptz_recall_preset(recv, n - 1, 1.0)`). The app only calls positions back; they
  are saved with the camera's remote or web page, as for VISCA. The app moves no camera by hand, so no pan, tilt
  or zoom speed calls are declared.
- While live, the camera's receiver (`camera_feed` → `ndi::receive`) carries the calls: a `Remote` per NDI camera
  feed (kept in a small list in `ndi.rs` by source name) holds the position asked for and the tally, and the
  receiving thread sends them between pictures (at most 200 ms later). It also notes whether the camera says it can
  move (`NDIlib_recv_ptz_is_supported`, asked twice a second).
- The operator's position list shows only when the camera in the picture can move: for an NDI camera, when its
  receiver says so (an NDI source from OBS, say, never shows positions). The list is looked at again with the
  scene button (four times a second), as the NDI connection takes a moment.
- Not live (a macro or slide action before the service): a short NDI connection of its own, metadata only (no
  pictures), waits up to 5 s for the camera to say it can move, calls the position back, and closes. Off the UI
  thread, as VISCA.
- Macro and slide-action lists offer an NDI camera's positions whether or not it can move: whether it can is only
  known once connected. A camera that cannot move ignores them (as a VISCA camera that is off).

## 2. The NDI camera's own tally light

- While live, each NDI camera is told whether it is on air (`on_program`) or next (`on_preview`) with
  `NDIlib_recv_set_tally`, following `App::tally` through `Tally::light(i)` only (so a new meaning of "next", the
  Program/Preview panel, carries over by itself). Cameras are matched to `Tally`'s numbers by their place in the
  operator's camera list.
- The operator's scene button (four times a second) hands the tally to `ndi::show_tally`; the receiving thread sends
  it when it changes and again every 2 s (NDI may connect again by itself). When the stream stops, the receivers
  close, which turns the camera's light off.

## 3. The words alone in a file, see-through around them

For a church that edits the service video afterwards: the words over the camera (the lower third, exactly what
"RustyPresenter Words" sends over NDI) as a second video beside the recording, to lay over the edit.

- Settings → Media → Livestream, after the NDI rows: "Words alone", a check "Also record the words alone, on a
  see-through background (for video editing)", off to start with. It does not need NDI.
- While live (it uses the stream's scenes), the file "<recording name> words.mov" is made next to the recording
  (never over another file: " words (2).mov" and so on).
- Format: QuickTime Animation (`qtrle`, `argb`) in MOV. It keeps see-through edges exactly (lossless, straight
  alpha) and is read by the common editors (Premiere Pro, Final Cut, Shotcut, Kdenlive and others that use FFmpeg).
  Unlike ProRes 4444 it only stores what changed from the picture before, so with words that change a few times a
  minute an hour is around 100 MB, where ProRes 4444 would be several GB. A full picture every 10 s
  (`-g 300` at 30 fps) keeps it quick to jump around in.
- Same size and frame rate as the recording. It starts with the stream's picture-writer and stops with it, so it
  lines up with the recording to within a fraction of a second (an editor slides it into place if needed).
- Never slows the recording or the stream: the writer only hands over the words picture when it changes (a pointer
  compare, then a short lock); a thread of its own turns it to straight alpha and feeds its own FFmpeg. FFmpeg puts
  the time on each picture as it arrives (`-use_wallclock_as_timestamps 1`) and fills the gaps itself
  (`-fps_mode cfr`), so the app sends a picture only when the words change and once a second, not 30 full
  pictures a second. If that FFmpeg fails, only the words file is lost.
- When the stream stops, the last picture is sent, its FFmpeg is told to finish, and the writer waits for it beside
  the recording's own finishing (at most 30 s).
- If the app stops suddenly, the words file is lost (a MOV is only readable once finished); the recording itself
  is recovered as before. Accepted: the words can be made again from the service.

## Tests

- `stream::ptz`: NDI cameras are moved over NDI unless a control address is given; `moves` lists them.
- `stream::ndi`, with the NDI runtime (skipped without it): a test source says it can move (connection metadata
  `<ndi_capabilities ntk_ptz="true"/>`); the camera's receiver sees that, and a position asked for arrives at the
  source as a preset recall; the tally set through `show_tally` arrives at the source (`NDIlib_send_get_tally`).
  These extra SDK calls are looked up in the test only.
- `stream::words_file`, with FFmpeg (skipped without it): a short real file has `qtrle` / `argb`, the right size and
  length, clear where there are no words and solid where there are.
- `record::run_tests`: going live with the words file on makes "Recording test words.mov" beside the recording.

## Not verified by a person

A real NDI PTZ camera (preset numbering differs between makers: some may count positions from 1); a real camera's
tally light; the words file opened in Premiere, Final Cut and DaVinci Resolve.
