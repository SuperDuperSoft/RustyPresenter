# ProPresenter 7 "Complete Beginner's Guide" — notes + ideas for RustyPresenter

Source: Renewed Vision, https://www.youtube.com/watch?v=tb7UatDBGAI (73 min). Section summary, not a verbatim transcript.

## What the video covers (by timestamp)
- **0:00 Intro / setup** — free download, watermark until licensed. First-run wizard: migrate old data (they say: rebuild library slowly instead), screen setup, "House of Worship" toggle (Bibles, Planning Center, SongSelect), sample content.
- **4:39 Interface** — left→right flow: Library/Playlist → slides grid → preview. Multiple libraries (e.g. Songs, Presentations), drag between them. One presentation per part of the service; playlists per event, with coloured **headers** (Pre-show, Music, Communication, Post-show). Transitions per layer (dissolve/cut/fancy). Views: grid, **easy view** (big text; hold `~` to peek), outline. "Continuous playlist" scrolls through every item.
- **11:22 Layers** — stacked: Video input → Media → Slide → Announcements → Props → Messages, plus Audio (non-visual). Each layer has its own clear button that lights up when in use. A slide can carry media, so one click = text + background.
- **15:30 Show controls** — audio bin, stage controls, timers, messages, props. Media bin with playlists/folders and **smart playlists** (watch a disk folder). Floating, persistent search window (Ctrl+F) that also searches SongSelect. Non-linear: click anything, anytime.
- **20:11 Screens** — audience screens (single, mirrored, grouped, edge-blended) and stage screens. Outputs: displays, SDI, **NDI** (to OBS/vMix), Syphon, **placeholder** screen (e.g. 1920x1080 livestream with no physical display).
- **23:19 Songs** — SongSelect import or paste text (File → Import text from clipboard). Import dialog: lines per slide, break on line vs paragraph, size, **theme applied on import**, target library, add to playlist. **Reflow editor**: one text box for the whole song, Enter = new line, "insert slide break", Backspace merges slides, group labels (Verse/Chorus/Bridge). Tip: add an **Intro** slide (holds background + actions) and a **Blank** slide to every song. **Arrangements**: drag groups into an order, several arrangements per song; editing a group updates every use.
- **34:50 Slideshow / announcement loop** — drag a folder of images in → one slide each; slideshow timer (e.g. 5 s), auto-loop, countdown indicator you can click to pause. Drop an **audio playlist** on slide 1; looping doesn't restart the music.
- **38:24 Teaching notes + scripture** — themes with layouts (title, statement, list, quote). Full editor: objects (text, shapes, media, video input, web page), text/shape properties, **builds** (bullet-by-bullet animation, multi-click slides). Bible panel: theme + which box gets verse vs reference, verses per slide, then "copy to selected presentation".
- **44:54 Export** — presentation vs **bundle** (includes media). Fonts don't travel.
- **45:52 Playlists** — new playlist, drag from search, headers with colours.
- **48:10 Navigation** — Up/Down = next item, Left/Right/Space = slides. Hotkeys: C chorus, B bridge, A/S/D/F verses 1–4, I intro, type a number + Enter to jump to that slide.
- **49:46 Stage screens** — stage layouts built in an editor: current/next slide text, preview image, notes, live screen preview, chord charts, stage message, timers, clock, current/next group, current/next playlist item, slides remaining, stream/record status. Pre-built layouts; "scale text to fit".
- **56:42 Actions** — slides can trigger: clear layers, media, audio playlist, video input, **stage layout change**, **look change**. Right-click → Add action. Icons on slides show what's attached.
- **59:57 Looks** — per audience screen: which layers show, plus a **theme override** (e.g. centred lyrics in room, lower-third lyrics on livestream). Presets ("Music", "Default"), switched by slide actions. Messages for in-room, props for online.
- **1:06:33 Announcements layer** — second presentation lane above slides; route it to only one screen (lobby signage while the room rehearses).
- **1:08:44 Video input + streaming** — camera/NDI input with custom thumbnail; looks decide which screen shows it. Capture: source screen → record to disk / RTMP / Resi, bitrate, local copy, audio routing.

## Ideas for RustyPresenter (ranked: biggest win for a church volunteer first)
1. **Layer model with per-layer clear buttons** (Media / Slide / Announcement / Messages / Audio). Buttons light up when that layer is live. Simple to explain, stops "why is the background gone?" moments.
2. **Song Intro + Blank slides by default** on every imported song (EasyWorship import too). Intro holds the background and actions; Blank = instrumental.
3. **Arrangements**: song = groups; arrangement = ordered list of group refs. Edit once, every repeat updates. Fits the existing `song.rs` model.
4. **Reflow editor**: one big text box per song; Enter = line, Ctrl+Enter = slide break, Backspace at start merges. Faster than per-slide editing.
5. **Import dialog with theme + lines-per-slide**, and "Paste lyrics" import that detects "Verse 1 / Chorus" headings.
6. **Slide actions** (background, audio playlist, stage layout, look). Start with background + stage layout; show small icons on the slide thumbnail.
7. **Looks** = per-screen layer on/off + theme override. Big win for livestream: lower-third lyrics, no backgrounds online.
8. **Stage layouts** with live fields (current/next slide, next group, clock, countdown, slides remaining, message). Ship 2–3 presets; editing can wait.
9. **Announcement loop**: drop a folder of images → timed looping slideshow, with walk-in music that doesn't restart on loop. Plus an announcements lane routable to a lobby/livestream screen only.
10. **Keyboard nav**: you already have C/B/P/E/1–9. Add: type number + Enter to jump to slide N; Up/Down for next service item; hold a key for big-text "easy view".
11. **Placeholder / NDI output** for livestream (later): a virtual 1920x1080 screen OBS can grab. Big for churches already using OBS.
12. **Service headers with colours** (Pre-service, Worship, Message, Post).
13. **Smart media folders**: point at a disk folder, library auto-updates (fits the new bottom media library).
14. **Bundle export** (.zip with media) so a pastor can build notes at home and send them in.
15. **Bible panel**: choose which text box gets verse vs reference, verses per slide, "add to service".

Skip / later: SongSelect + Planning Center (paid APIs), edge blending, SDI, Resi, built-in RTMP streaming (let OBS do it).
