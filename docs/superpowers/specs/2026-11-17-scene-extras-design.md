# Livestream scenes: the rest — design (November 17, 2026)

Follows `2026-11-17-own-scenes-design.md` (the church's own scenes). This adds everything that design left out:
dragging layers, crops, colour and brightness keys, clock / countdown / video / web page layers, transitions,
tall-stream scenes and keyboard keys for scenes.

## What a volunteer sees (Settings → Media → Livestream → Your scenes…)

- **Drag on the picture.** A press on the picture chooses the layer in front there and dragging moves it; the chosen
  layer has an accent frame with a square on its bottom right corner, and dragging that square makes it bigger or
  smaller. Either way the layer becomes "A box you set" (whole percent, kept inside the picture). The picture is
  drawn again as it moves (picture files are read once and kept, so this stays quick).
- **Wide picture / Tall picture (phones)** buttons over the picture. The tall picture of a scene starts as the wide
  one fitted across (what tall destinations got until now). "Place the layers for phones" gives the scene places of
  its own there: each layer starts where it is in the wide picture (the same percent), and Place, the box sliders and
  dragging then change the tall place only. "Use the wide picture fitted across again" forgets them.
- **Cut off the edges** (cameras, the room screen, the words, pictures, videos, web pages): left, top, right, bottom
  in percent of the layer's own picture. At least 1% is always left.
- **Take out** (cameras, pictures, videos): Nothing / A colour (green screen) / The dark parts / The light parts.
  A colour: swatches (green-screen green, blue-screen blue, black, white, gold) or `#rrggbb`, "How close a colour must
  be" and "Soft edge". Dark / light: "Darker than" / "Lighter than" and "Soft edge". A camera's room-screen stand-in is
  never keyed.
- **New things a layer shows:** a video (chosen file, played again and again, no sound), a clock (the time now;
  seconds and 24-hour switches), a countdown (to a time typed as `10:30`, or, left empty, to the service's start time
  from Services → Starts at; "–:––" when there is none, "0:00" once it has passed), and a web page (an address).
  Text, clock and countdown layers have a size (0 = like the church's words) and a colour (empty = the church's).
- **When this scene comes in:** Like the other scenes (the default, named) / Cut / Fade / Slide / Stinger video (with
  "Choose a video…"). **Changing scenes** under the scene list sets the default for every scene, built-in ones too
  (`RecordPrefs::scene_transition`, saved with the scenes; Fade as before). A stinger with no video fades.
- **Keys for scenes:** Settings → Help lists "Livestream scene: …" for each built-in scene and each of the church's
  scenes, with no key until the church gives one (so nothing clashes with the presentation's keys). A key works like
  a clear key (anywhere, unless it is a typing key in a text box) and chooses that scene as the scene button does;
  when not live it says "Scene keys work while the stream is live."

## Rules and technical choices

- **Model** (`stream::own_scenes`): `Layer` gains `crop: Crop`, `key: key_out::Key`, `tall: Option<Place>`;
  `OwnScene` gains `transition: Option<Transition>` and `tall: bool`; `Shows` gains `Video`, `Clock`, `Countdown`,
  `Web`, and `Text` gains a `TextLook`. Everything new is left out of the saved settings when unused, so older
  settings read the same and newer kinds still read as "Unknown".
- **Keys** (`stream::key_out`): a colour key compares the two colour parts of YCbCr (so a shadow on the green screen
  is still green); "how close" and "soft" are distances up to 128. Each layer's `Cutout` holds a table of how much of
  a pixel stays (by squared colour distance, or by brightness), so a 1080p camera costs one table look-up a pixel,
  and only for the pixels in the layer's place. Pictures are keyed once when the scene is made ready.
- **Crop** is applied to the source picture before the place's shape is cut from its middle (cameras, videos) or
  before it is fitted (words, pages, pictures).
- **Clocks and countdowns** are patches drawn by the scene worker, not the writer: `Prepared::retick` makes a copy of
  the prepared scene (pictures shared, `Arc`) with only the changed clock drawn again, at most once a second; the
  writer sees a new prepared scene and puts the picture together again.
- **Videos** (`stream::scene_media`): one FFmpeg per distinct video file of the church's scenes, started when the
  stream goes live, through `camera_feed` (`-stream_loop -1 -re`, scaled to the stream's size and frame rate, RGBA).
  The writer only takes the newest picture (`Inputs::more`). A layer whose video has no picture yet shows what is
  behind it.
- **Web pages** (`stream::web_page`): the app has no browser of its own (the old webview went with Tauri, and
  `tests/cleanup.rs` keeps it out). The computer's Microsoft Edge (else Chrome / Chromium) runs headless with a clear
  background, the stream's size, a profile of its own in the temp folder, in a process tree that dies with the app.
  Its DevTools screencast sends a PNG whenever the page changes; a thread of its own decodes it into premultiplied
  RGBA. The DevTools WebSocket is a small client written here (no new crate). A page whose browser stops is opened
  again every 5 s. A PC without Edge/Chrome logs why and the layer shows what is behind it.
- **Transitions** (`stream::transition`): `Running` holds the picture that was going out (and its tall one) and draws
  each frame: fade (as before), slide (the new scene pushes the old one out to the left, eased, half a second) or a
  stinger. The scene worker sends the incoming scene's transition in `Shown::transition`. The camera switch's Fade
  button still fades.
- **Stingers:** for each stinger video FFmpeg is started ahead (scaled to fill the picture, `format=rgba`; a WebM
  through `libvpx-vp9` when FFmpeg has it, to keep its see-through). It opens the file, decodes and then waits,
  blocked on its full output, so the stinger starts at once; the next one is started ahead after each use. Its length
  is counted once at go-live (`-progress`); the scene changes at its middle frame (half a second when not yet
  counted). Frames are taken by the clock, at most two waiting; a stinger whose first picture does not come within
  1.5 s is dropped (the scene changes at once); no stinger runs over 10 s.
- **Tall scenes** (`stream::tall`): only when a destination is tall and some scene has tall places. The writer then
  puts the tall picture together too (the scene's tall places, else the wide picture fitted across, again only when
  something changed) and FFmpeg reads it as raw RGBA from a local port (`read_from` swaps the tall graph's input). A
  task of its own writes the newest tall picture at the frame rate, so FFmpeg starting (which reads its inputs one
  after another) never waits on the writer. Without tall scenes nothing changes: FFmpeg fits the wide picture.
- **Keys** (`shortcuts::Action::Scene(Scene)`, stored as `scene:camera_words` or `scene:<id>`): `Keymap::new` lists
  the built-in scenes and any of the church's scenes with a key; `Keymap::with_scenes` (Settings → Help) lists all.

## Speed on an old PC (1080p, 30 a second)

The writer still only puts the picture together when something in it changed. Heavy work is elsewhere: video and web
page decoding, stingers (decode and premultiply), clocks (text drawing), pictures and keys of pictures (once). Per
frame the writer does the same nearest-pixel scaling as before plus, for a keyed layer, one table look-up per pixel
of its place. A tall picture adds one more composition (or a fit) when something changed, one copy of the tall
picture for its feed, and a second encode in FFmpeg (as before).
