# Livestream scenes (1.3b) — design

Date: November 15, 2026. Owner: Jake. Status: approved in chat ("Yes, build it").
Parent design: `2026-11-09-livestreaming-design.md` §4 parts 1–3, §8 step 2.

## Why

One volunteer runs the slides and the stream together, so the stream has to look after itself. Viewers at home see the
camera with the words while the church sings, the camera with the verse during a reading, presentations and videos
in full, a "Starting soon" screen before the service and a closing screen after. When the volunteer wants something
else, one click does it.

**Success:** Go Live before the service and touch nothing else; the stream shows the right thing at every point of a
normal Sunday. **Anti-goals:** a scene change that stutters or blanks the stream; anything that can stop the stream or
the recording (a camera unplugged, a picture that will not draw); setup work before the first use.

## Today (1.3a, on `follow-sunday`)

- The room's picture reaches the recorder through `record::pump::FrameSink`; the writer (`record::run::write`) sends
  RGBA frames at 30 fps into one FFmpeg, which encodes once for the file and every destination.
- One camera layout is picked in Settings before going live (`RecordPrefs::camera_layout`: camera + screen corner,
  screen + camera corner, camera + words). FFmpeg puts it together (`args::picture_in_picture`, `camera_with_words`)
  from extra inputs: the camera through `stream::camera_feed` (yuva420p over local TCP) and the words through
  `stream::lyrics` (RGBA over local TCP). FFmpeg's filter graph cannot change while running, so the layout is fixed.
- Freeze (`Controls::hold`) and Mute work and stay as they are.

## Approaches

1. **The app puts the picture together itself (chosen).** The camera and words workers keep their newest picture in
   memory instead of sending it to FFmpeg; the writer composes the scene into one RGBA frame each tick and FFmpeg gets
   a single plain input. Scenes can change at any frame, fades are a blend, and every scene is a pure function that
   tests can check pixel by pixel. Cost: one frame copy plus a small blend per frame on the CPU (camera scenes), cheap
   at 720p/1080p.
2. **FFmpeg keeps composing; every layer always present, the app switches by making layers see-through.** Less new
   code, but the screen-in-a-corner comes from the same input as the full screen (cannot be hidden), cards need yet
   another input, and fades mean rewriting alpha planes in three feeds in step. Rejected.
3. **Restart FFmpeg at each scene change.** Drops the stream for seconds. Rejected.

## Scenes

| Scene | Picture |
| --- | --- |
| Camera | Camera fills the frame (shape kept, black bars). |
| Camera + words | Camera, with the live text slide's words as a lower third (`lyrics::lower_third` look). |
| Full screen | What the room shows (today's stream without a camera). |
| Screen + camera | Room screen fills the frame, camera a quarter as wide in the bottom-right corner. |
| Starting soon | A card: the church's picture, or words ("Starting soon") with the pre-service countdown under them while it runs. |
| Ending | A card: picture or words ("Thanks for joining us"). |
| Hold | A card: picture or words ("We'll be right back"). Different from Freeze, which keeps the last picture. |

A scene change fades over 0.5 s (the old composed frame blended into the new one). Freeze still freezes whatever is
going out, mid-fade included.

**No camera set up, or the camera has stopped:** camera scenes show Full screen instead (the camera scene with the
room underneath, as today's "the stream shows the slides until it is back"). Camera + words with the camera gone shows
Full screen too, since the room already has the words.

## Automatic choice (`stream::scene::choose`, pure)

Looked at ten times a second, from what is live:

1. Pre-service countdown running → **Starting soon**.
2. Nothing has been taken live since Go Live (no cursor) → **Starting soon**.
3. The service's last item is the live item and nothing shows (slide, media cleared) → **Ending**.
4. A video playing (media layer) → **Full screen**.
5. A text slide with words: song → **Camera + words**; Bible (scripture) → **Camera + words**; other text (sermon
   notes, announcements as text) → **Camera + words** as well (the words matter to viewers).
6. A picture slide or a designed slide (presentations, imported decks, announcement pictures) → **Full screen**, or
   **Screen + camera** when the setting "Presentations: with the camera in the corner" is on.
7. Nothing on the slide layer (cleared, or background only) → **Camera**.

Without a camera every camera scene falls back as above, so auto on a camera-less setup is: cards before/after,
Full screen in between.

## Taking over by hand

- Scene buttons beside Go Live / Freeze / Mute, shown while live: **Auto**, Camera, Camera + words, Full screen,
  Screen + camera, Starting soon, Ending, Hold. The scene going out is lit; Auto is lit while automatic is in charge.
- A click on a scene holds it **until the live item changes** (another service item taken live), then Auto takes over
  again. A click on Auto gives control back at once.
- **Slide actions and macros** gain a step "Stream scene: <scene or Auto>". It acts like a click (same until-next-item
  rule), so a sermon's first slide can say "Camera".
- Setting **Automatic scenes: off**: the stream stays on the scene last chosen (Full screen at Go Live) until a click.

## Settings (Settings → Media → Livestream)

Replaces the "Camera layout" picker (its saved value is ignored from now on):

- **Automatic scenes** on/off (default on).
- **Presentations** Full screen / With the camera in the corner (default Full screen).
- **Starting soon**, **Ending**, **Hold**: each words (editable, default text above, translated) or a picture chosen
  from a file. The card's words use the church default theme's font and colours on its background colour.

## Parts

- `stream::scene` — `Scene` (serde: snake_case names, kept in `RecordPrefs` and macro steps), `LiveInfo` (what
  `choose` needs, made from `Live` + automation state), `choose`, `Choice` (manual override with the item it was made
  on), `effective` (camera fallback).
- `stream::compose` — pure: `Inputs { room, camera, words, card }` as RGBA slices; `compose(scene, &Inputs, out)`;
  `blend(from, to, t, out)`; `corner_rect`; nearest-pixel downscale for the corner. Words and cards are premultiplied
  RGBA over the base.
- `stream::pictures` — one worker (replaces the words TCP sender): every 100 ms asks the app for `LiveInfo`, picks the
  scene, draws the words picture and the card when they change (card once a second while a countdown shows), and
  publishes `{scene, words, card}` in a shared slot.
- `stream::camera_feed` — keeps the newest camera picture as RGBA in a shared slot (no TCP, no own pacing). Keeps its
  reconnect and `seeing` behaviour.
- `record::args::record` — no camera inputs or filter graph; one RGBA input as without a camera.
  `picture_in_picture`, `camera_with_words`, `Layout`, `Camera::{words_port, feed_port}` go.
- `record::run::write` — each tick: take the room picture, read the camera slot and the pictures slot, compose into the
  output buffer (only when an input changed or a fade runs), blend during a fade, write.
- App: `App::stream_choice` (manual override, shared by buttons and macro steps); `MacroStep::StreamScene`.
- UI: scene buttons in the operator's live bar; Settings fields.

## Not in this step

Several cameras and switching between them (1.3d), a scene editor (1.3f), separate tall canvas, stinger transitions,
different scenes per destination.

## Testing

- `choose`: every rule above, including fallbacks and manual override expiry.
- `compose`: each scene's pixels (camera fill with bars, corner position and size, words over camera, card), blend
  halfway, camera missing.
- Recorder: the existing real-FFmpeg tests (`run_tests.rs`) changed to the single-input form; a new one switches scene
  mid-recording and checks the file's pictures before and after.
- UI: the scene buttons light the scene going out; a click holds until the next item.

## Left to the builder

Exact button look (follow the Freeze/Mute buttons), whether composition skips unchanged frames, card layout details.
