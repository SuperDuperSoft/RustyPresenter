# See the next camera before it goes out (Program/Preview) — design

Livestreaming design §4 item 4: "Switching: Program/Preview pair, Cut/Fade, number keys, phone tally". Branch
`ls-preview`. Words on screen are for volunteers: "Live" and "Next", never "program" or "preview".

## What the volunteer sees

While live with one camera or more, a **Cameras** box sits in the live column, under the automation strip:

```
 Live                       Next
 [ small live picture ]     [ small picture of the next camera ]
 ● Camera 1 · Pulpit         ● Camera 2 · Wide
 [1 Pulpit] [2 Wide] [3 Side]          (only with three cameras or more)
 [ Switch ]  [ Fade ]
```

- **Live** is the stream's picture as it is sent (scenes, words and all), so the operator sees exactly what people
  at home see. **Next** is the camera chosen to go out next. Both refresh four times a second.
- A camera that sends no picture (unplugged, not started) shows **No picture** in its box.
- **Switch** puts the next camera out at once; **Fade** puts it out with the half-second fade the scenes use. The
  camera that was out becomes the next one, so pressing again swaps back. With two cameras that is all there is.
- With three or four cameras, a button per camera ("1 Pulpit") chooses which is next; the one out now is marked red
  and cannot be chosen as next.
- With one camera the box shows the Live picture only (no Next, no buttons): still useful to see what goes out.
- When the scene going out shows no camera (a video, a card), Switch and Fade still choose the camera that comes
  in with the next camera scene; the Live caption says the camera is not in the picture now.
- A PTZ camera chosen as next shows "Position 1–6" buttons under the Next picture, so its shot can be set up before
  it goes out (the scene button keeps the positions of the camera that is out).
- No keyboard shortcuts: the number keys and letters belong to the presentation. Buttons only.
- The scene button's camera rows (from the two-camera work) stay: choosing one there switches at once, and the
  camera that was out becomes next, as with Switch.

## Phone tally

Red: the camera in the stream's picture. Green: the camera chosen as next. With one camera and a scene that shows
no camera, that camera is green (it comes in with the next camera scene), as before.

## Up to four cameras

Settings → Media → Livestream gets "Third camera" and "Fourth camera" rows, shown once the camera before is
chosen, the same list and the same kinds as the first two (USB, network address, NDI), each with its own
"Camera control address". Saved as `camera3`, `camera4`, `camera3_control`, `camera4_control`. A camera row with
none before it, or the same camera again, is not kept. `RecordPrefs::cameras()` gives the chosen ones in order;
Go Live and the PTZ slide actions ("Camera 3: position 2") use it.

## How it works

- `record::run::Controls` gains `next` (the camera chosen as next) and `fade` (the next camera change fades).
  `Recording::switch_camera(i, fade)` sets them; the writer sees the camera change and, with `fade`, starts the
  mixer's fade from the picture going out (`Mixer::fade_from`), the same blend the scene changes use.
- The camera feeds keep their places (`Vec<Option<Feed>>`), so camera *n* is always Settings' camera *n* even if one
  could not start.
- **The small pictures** (`record::peek`): every 250 ms the writer shrinks the picture it just wrote and the next
  camera's newest picture to 320 pixels wide (an average of four samples per pixel, well under a millisecond) and
  hands them over behind a mutex as `Arc`s. It never copies a full picture. The operator window's 250 ms ticker
  takes them and makes Slint images only when they changed. Nothing is made while not live or without cameras.
- The operator keeps which camera is out (`camera-active`, as before) and which is next; the Cameras box is a
  global (`CameraSwitch`, `ui/operator/cameras.slint`) like the automation strip, so the main window changes by two
  lines.

## Tests

- `record::peek`: shrinking keeps colours and shape; a picture of another size is no picture.
- Mixer: a fade asked for a camera change blends from the old camera to the new one.
- Real FFmpeg: two stand-in cameras (blue, green); the small Live picture is blue and Next green; Fade swaps them;
  the recording shows the second camera after.
- Operator (inline): Switch swaps live and next; Fade too; choosing next with three cameras; the camera out cannot
  be next; the tally follows; the Cameras box hides when the stream stops; one camera shows Live only.
- Settings: cameras 3 and 4 and their control addresses are kept; none without the one before.
- Tally: green follows the camera chosen as next.
