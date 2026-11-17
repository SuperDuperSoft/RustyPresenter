# The church's own livestream scenes — design (November 17, 2026)

Part of the livestream design (`2026-11-09-livestreaming-design.md` §4 items 1–3). Until now the stream had only the
built-in scenes (`stream::scene::Scene`). This lets a church make its own scenes from layers and use them like the
built-in ones.

## What a volunteer sees

- **Settings → Media → Livestream → "Your scenes…"** opens an editor (with unsaved Settings changes it first asks
  to save them, as the Song use report does). Closing the editor brings Settings back on the Media tab.
- **Left:** the list of the church's scenes. Add (a new scene starts as the room screen with the camera on air in
  the bottom right corner), Copy, Rename…, Delete…, Move up, Move down.
- **Middle:** a picture of how the chosen scene looks, drawn by the same code that draws the stream, with stand-in
  pictures: a grey "Camera 1" picture, a blue "Room screen" picture and sample words.
- **Right:** the layers, the one in front at the top. Add layer, Remove, In front, Behind. For the chosen layer:
  - **Shows:** the camera on air, camera 1–4, the room screen, the words (as over the camera today), a picture
    file, a colour, or a short text.
  - **Place:** fills the picture; top left / top right / bottom left / bottom right, each small, medium or large
    (the same sizes as the built-in corner camera); left half; right half; or a box set in percent (left, top,
    width, height).
  - **See-through:** 0% (solid) to 100%.
  - Picture: "Choose a picture…". Colour: the same swatches and `#rrggbb` field as elsewhere. Text: the words.
- Under the list: **"Use it by itself in place of:"** Never, or one built-in scene (for example "Camera + words",
  so every song and verse uses this scene). Kept simple: one choice per scene; when two scenes claim the same
  built-in, the first in the list wins.
- **Save** keeps the scenes in Settings; **Cancel** leaves them as they were.
- **While live** the scene button lists the church's scenes after the built-in ones. Slide actions and macros'
  "Livestream: …" step offer them too.

## Rules

- A camera layer whose camera sends no picture (unplugged, or not set up) shows the room screen in its place —
  the same rule as the built-in camera scenes.
- Camera and room screen pictures fill their place and keep their shape: what does not fit is cut off equally on
  both sides. Words and picture files keep their shape and fit inside their place, see-through around them.
- A text layer is drawn in the church's default font and colour, centred in its place.
- The automatic choice is unchanged; afterwards a built-in scene that an own scene stands in for is replaced by it.
  A scene chosen by hand (scene button, slide action, macro) is never replaced.
- A scene the operator or a slide action chose that was deleted since shows the room screen.

## Technical choices

- `Scene` gains `Own(Uuid)`. It stays `Copy`; built-in scenes still save as `"camera_words"` etc., an own scene as
  `{"own": "<id>"}`, so slide actions and macros keep it by its id (renaming never breaks them). `Scene::ALL` stays
  the built-in ones.
- New module `stream::own_scenes`: the model (`OwnScene { id, name, layers, instead_of }`, `Layer { shows,
  place, see_through }`), names and labels, the stand-in rule, and `Prepared`: the scene made ready for one
  picture size. Everything that does not move (picture files, colours, text) is drawn once into patches the size
  of its place, with its see-through already applied; places are worked out once. Composing a frame is then: black,
  then for each layer either a patch laid over its rectangle, or a live picture (camera, room, words) scaled into its
  rectangle (nearest pixel, the same as the corner camera; full-size solid layers are a plain copy).
- `RecordPrefs::own_scenes` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`): old settings load as no
  scenes; settings without any own scenes save as before.
- `stream::pictures` (the scene worker) gets the scenes in `Look::own`, applies the stand-ins, prepares the chosen
  own scene (again only when it changes) and passes it in `Shown::own`; it draws the words when the scene has a
  words layer.
- `stream::compose::Inputs` gains `cameras` (every camera's newest picture) and `own`; `compose` draws an own
  scene with `own_scenes::compose`. The recorder's `Mixer` puts the picture together again only when something
  the scene shows changed (its cameras, the room only when shown, the words, the prepared scene).
- The editor's preview is drawn at 640×360 by the same `Prepared` and `compose`, so what the volunteer sees is
  what goes out.

## Not in this step

Dragging layers on the picture, cropping, colour keys (green screen), a clock or countdown layer, video files,
scene transitions other than the fade, a different scene set for the tall stream.
