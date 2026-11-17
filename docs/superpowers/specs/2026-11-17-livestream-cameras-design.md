# Livestream cameras: phone cameras, lost-camera picture, moving by hand, all cameras at once, camera keys

November 17, 2026 (branch `ls2-cameras`). Builds on the Program/Preview design (2026-11-17-program-preview-design.md)
and NDI extras (2026-11-17-ndi-extras-design.md).

## 1. A phone as a camera

- **Page:** `/camera` on the app's phone server (`web/camera.html`, routes in `src/network/phone_camera.rs`). Words
  come from the computer in its language (`GET /camera/info`, `stream::phone_camera::tr::page_words`).
- **Private page (https).** Browsers open the camera only on https or localhost. The phone server is plain http
  (port 4318), so the same router is also served over **https on port 4320** (`network::https`, started in
  `startup::serve`). The certificate is self-signed, made by the app with `ring` (ECDSA P-256, written out in DER by
  hand: no certificate crate in the build), for this computer's network address, 127.0.0.1 and `localhost`, with
  server-auth key use and 800 days (Apple phones refuse more than 825). Kept in the data folder (`https.json`) so the
  phone's one-time "go on" lasts; made again only when the address changes or 30 days before it runs out.
- **The one-time step**, in plain words on the plain page and in Settings → Media → Livestream → "Phone as a
  camera": the phone warns the connection is not private; that is expected (the page comes from the church's own
  computer); tap Advanced / Show Details, then Proceed / Visit this website. The plain page has a button to the
  private page. If the https server could not start, the page says so.
- **Access:** the same as the phone remote: the phone must be paired (6-digit code from Phone remote → Pair a phone,
  plus the remote password when set). The page pairs itself with `/remote/pair`. Pictures from another site are
  refused (Origin checked; `same_origin` now also accepts `https://<host>` in the remote and stage view).
- **Pictures:** the phone draws its camera into the stream's size (black bars when its shape differs; "Turn the phone
  sideways") and sends JPEG (quality 0.7) over a WebSocket `/camera/ws?name=…`. The app answers each picture with
  the camera's tally light (`on_air`, `next`, `off`), which also paces the phone (next picture only after the answer,
  at most the stream's 30 a second). If the WebSocket fails twice (some phones refuse wss with a self-signed
  certificate) the page posts one picture a request to `/camera/frame`. Front/back camera switch, the phone kept
  awake, red/green frame for its tally.
- **WebSocket:** written in `network::websocket` (handshake with ring's SHA-1, frames, pieces, ping/pong, close, size
  limit 8 MB) on hyper's upgrade: the WebSocket crates are not in the build.
- **In the app:** each phone camera has a name typed on the phone ("Balcony"), saved as `phone://Balcony`.
  `stream::phone_camera` keeps the newest JPEG per name; the camera feed (`camera_feed::start`, input `<phone>`)
  decodes each new one into the stream's picture, so the lost-camera light, switching, tally, scenes and own scenes
  work as for any camera. Phones that sent pictures since the app started are listed in Settings' camera lists as
  "Phone camera: Balcony"; a saved one not sending shows "(not connected)". Several phones work at once.

## 2. A picture for a lost camera

Settings → Media → Livestream → "When a camera sends nothing": the room screen (as before, default), the church logo
(the Logo button's picture, centred on black; black when there is none), or black. Prepared once when the stream
starts (`stream::camera_lost::prepare`) and put in the camera's place by the writer while the camera sends nothing,
in camera scenes and own scenes; not in the room screen's corner (Screen + camera keeps the plain room). The camera
comes back by itself when it sends again; the lost-camera light still warns. `RecordPrefs::camera_lost`.

## 3. Moving cameras by hand

Under the Next camera in the Cameras box (with one camera: the one going out, so a one-camera church can move it):
◀ ▲ ▼ ▶ and − + move the camera while held (press: move, let go: stop), then "Position 1–6" calls a saved position,
and "Save…" turns the numbers into "Save this shot as 1–6".
- VISCA over IP: Pan-tiltDrive `81 01 06 01 VV WW XX YY FF` (pan speed 6, tilt 5), Zoom `81 01 04 07 2p/3p/00 FF`
  (speed 3), Memory Set `81 01 04 3F 01 pp FF`; Sony's header on port 52381 as before.
- NDI (SDK v6): `NDIlib_recv_ptz_pan_tilt_speed(p_instance, float pan_speed, float tilt_speed)` (pan + left, tilt +
  up), `NDIlib_recv_ptz_zoom_speed(p_instance, float zoom_speed)` (+ in), `NDIlib_recv_ptz_store_preset(p_instance,
  int preset_no)`; carried out by the feed's receiving thread (`ndi::Remote`). Saving also works with a short
  connection of its own; moving by hand needs the feed (the box shows only while live, when it runs).
- Orders go through one worker thread (`ptz::send`) so a quick press and release reach the camera in order. If the
  controls go away while held (another camera chosen, stream stopped) the camera is told to stop.

## 4. All cameras at once

Three or four cameras: the Cameras box shows every camera small in a row (in place of the name buttons), red outline
going out, green next, "No picture" when it sends nothing; a click chooses it as next. The writer shrinks each
camera to 160 wide every 250 ms (`record::peek::Peek::cameras`).

## 5. Camera keys

Settings → Help (the keyboard table): "Livestream: camera 1–4 next", "switch to the next camera", "fade to the next
camera". No keys by default; the church sets them like any shortcut, and the editor's clash warnings (section keys,
other shortcuts) apply. They work only outside text boxes and dialogs, and only while live with cameras (otherwise
the key is left to the presentation).
