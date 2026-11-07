# ProPresenter 7 — hands-on notes (Nov 6, 2026)

Claude opened ProPresenter 7 (Windows trial) on Jake's PC and looked at the main window, Stage Editor, Bible panel, Screens menu and Looks.
Most ideas were already in PROJECT_PLAN §10c. These are the **new** ones (not in §10c yet).

| # | Idea seen in ProPresenter | Goes in | Done looks like |
|---|---|---|---|
| N1 | **Identify screens**: one button puts a big number on every display | Configure screens (V1 / FEAT-7) | Operator can tell which monitor is "Screen 2" in one click |
| N2 | **Status button** top-right with a warning badge (screen missing, media offline, Bible offline) | Preflight / Help | Badge shows before service if anything is wrong; click lists the problems |
| N3 | **Timer types**: countdown (segment), countdown-to-a-time (pre-service), count-up (elapsed), time left in current video, time left in current audio | FEAT-2 / FEAT-3 | Each can be placed on the stage layout and in slides |
| N4 | **Video transport**: back 15 s, forward 15 s, "jump to 10 s before end" (to test what happens when a video ends) | Media playback (V1) | Buttons under the preview; work during a live video |
| N5 | **Preview picks the screen**: dropdown under the live preview chooses which output you see (room, stage, stream) | FEAT-1 / FEAT-7 | Operator can check the stage screen without walking to it |
| N6 | **Bible panel extras**: reference box keeps a history dropdown; whole passage shows at once as a numbered slide grid; Prev/Next verse buttons; "Save as presentation"; licence/info button per translation | FEAT-5 (adds to §10c #15) | Typing "Ruth 4:5-22" shows all slides instantly; last 10 references one click away |
| N7 | **Friendly empty states**: an empty list shows one big button ("+ New presentation", "Import media", "Import audio") instead of blank space | UI polish (all lists) | No empty panel is ever just blank |
| N8 | **Filter box under every list** and a **thumbnail size slider** | UI polish | Every library/playlist/media list can be filtered by typing |
| N9 | **Look change fade time** (slider, 0–N s) | FEAT-1 (adds to §10c #7) | Switching looks fades instead of cutting |
| N10 | **Stage layout objects**: lock and hide per object, "Next slide: text + chords", slide notes, next-slide picture, simple shapes, rulers/grid | FEAT-3 layout editor | Objects can be locked so a volunteer can't move them by mistake |
| N11 | **Camera/video input as a background** (Video Inputs bin in media) | FEAT-1 (after 1.0) | A webcam or capture card can sit behind lyrics |
| N12 | **Props** (saved logo/bug overlays, a collection you toggle) and **Masks** (crop or round the output shape) | FEAT-1 overlays | A logo bug can be turned on/off from a side panel |
| N13 | **Separate audio playlist** with shuffle and its own volume, independent of slides | FEAT-2 (walk-in music, §10c #9) | Walk-in music plays from its own list and keeps going while slides change |

Not copied: ProContent store (paid), MultiTracks chords integration (paid).

## Second pass (Nov 6, 21:35–21:45) — every menu, Settings, editors

Seen this time: all menus (ProPresenter, File + Import/Export, Edit, Presentation, View, Window, Help), Settings (General, Updates, Screens, Audio, Network, Inputs, Devices, Media, Import, Integrations, Groups, Slide Labels), Configure Screens, Capture Settings, Timers, Stage tab, Macros tab, Search, Action Palette, Calendar, Timecode, Reflow editor, slide Editor, Theme/Props/Mask/CCLI editors.
Not seen (clicks on the right-hand side didn't land, or the window wouldn't open): Messages panel, Text and Build inspectors, Media inspector, Planning Center, Test Patterns, Easy/Table views, Status, Bible Options, Key Mappings window.

| # | Idea seen in ProPresenter | Goes in | Done looks like |
|---|---|---|---|
| N14 | **Quick search box** (Ctrl+F): one pop-up searches songs, library and song services; Enter opens, Ctrl+Enter adds to the service | FEAT-4 / FEAT-6 | Any song is two keystrokes from the service |
| N15 | **Action palette**: every action (clear any layer, play media/audio/playlist, camera, timer, message, macro, look, stage layout, "go to slide") shown as a tile you drag onto a slide | FEAT-2 (bigger §10c #6) | Every action a slide can do is in one place |
| N16 | **Macros**: one button runs several actions (e.g. "Pre-service" = start loop + walk-in music + countdown + stage layout) | FEAT-2 | One click sets up pre-service |
| N17 | **Calendar**: run an action or macro at a set time, repeating on chosen weekdays | FEAT-2 | Countdown starts by itself at 10:45 every Sunday |
| N18 | **Clear keys per layer** (F2 slide, F3 media, F4 props, F5 audio, F6 messages, F7 announcements) and **clear groups** (your own mix of clears) | FEAT-1 (with §10c #1) | Each layer has its own key; a "clear all but background" group works |
| N19 | **Section names and colours in Settings**: colour families (verses = blues, choruses = reds, bridges = purples), add/rename, key per section; **slide labels** (Blank, Instrumental, Repeat, Spoken Word) | FEAT-4 | Colours are editable; labels show on thumbnails |
| N20 | **Screen setup extras**: Mirror / Grouped screens, colour fix per screen (R, G, B, gamma, black), corner pin (keystone), video delay in frames, test patterns, placeholder screen (set up with no projector) | V1 screens | A crooked projector can be squared up in the app |
| N21 | **Audio**: separate "listen" output for the operator's headphones (preview audio), main output delay (ms), per-channel routing | V1 sound | Operator can cue a video in headphones without the room hearing |
| N22 | **Record to disk**: record the room output to a video file (H.264, 720p/1080p presets, choose folder and audio) | FEAT-7 (before §10d streaming) | One button records the service to an MP4 |
| N23 | **Media safety**: copy imported media into the app's own media folder (no "missing file" on Sunday); "Clean up unused media" | Media library (V1) | Moving or deleting the original file doesn't break a slide |
| N24 | **Import defaults**: scale (fit / fill / stretch), picture duration, video end behaviour (loop, hold last frame, clear, go to next) | Media (V1) | Settings decide what new media does without editing each one |
| N25 | **Themes as a set of slide layouts** (Title, Title + subtitle, Quote, Main point, Scripture, Picture, Points, Text, Lyrics, Lower-third lyrics) and ~15 ready-made themes; snap guides in the editor | FEAT-1 | Picking a theme styles songs, verses and sermon slides together |
| N26 | **Slide editor**: text, shapes, drawn paths, pictures, video, web page; align, size, rotate, opacity, fill, outline, shadow, soft edges; **builds** (objects fade in one by one); transitions per slide; slide notes; Live Edit | FEAT-1 editor | A sermon point slide can be made without PowerPoint |
| N27 | **Copy / paste text style** between slides | FEAT-1 editor | Formatting one slide like another is two keystrokes |
| N28 | **Shortcut editor**: change any keyboard shortcut | Settings | A church can match the keys they already know |
| N29 | **Show slide on stage screen only** (send a note to the band without the room seeing it) | FEAT-3 | Works from the operator and the phone |
| N30 | **Windowed output**: show the room screen in a normal window (testing with no projector, or for screen capture) | FEAT-7 (with §10c #11) | The room output can be opened as a window |
| N31 | **Network**: phone remote with password; **stage view on a phone/tablet** (password optional); simple TCP control for other gear | Phone remote (V1) / FEAT-3 | A singer can see the stage screen on an iPad |
| N32 | **More import/export**: text from clipboard, PowerPoint as editable slides or as pictures; export as text, slide pictures, playlist, themes | V1 import / FEAT-6 | Lyrics can be exported as plain text for the bulletin |
| N33 | **Show performance on screen** (frames per second, video memory) and "turn screens on at launch" | Settings / preflight | Operator can tell if the laptop is struggling |
| N34 | **Reflow editor details**: slide preview beside the text as you type, "Insert slide break" button | FEAT-4 (with §10c #4) | Changes show in the preview as you type |
| N35 | **Find my mouse** (Alt+M flashes the pointer) and Help links (tutorials, report a problem) | Help menu | Works on a dual-screen setup |
| N36 | **Copyright on screen**: per-song title/artist/author/publisher/CCLI #/year/artwork, one template for the copyright line, on/off in Settings | FEAT-7 | CCLI line shows on the first and last slide automatically |
| N37 | **Other slide views**: Easy view and Table view, not just the grid | Operator UI | Big-text view for small laptops |

Left out (needs special hardware or paid service): edge blending, SDI, SMPTE timecode, MIDI devices, AirPlay receiver, Resi, Planning Center, MultiTracks, SongSelect sign-in, ProContent.
