# One word for each thing (November 17, 2026)

Jake: "Make sure the app has consistency. For example: does it use the same wording? Is stage screen always called
stage screen?"

Every text a person reads was collected (the 3,235 texts in `translations/rustypresenter.pot`, the phone pages in
`web/`, the plain names in `translations/allow-bare.txt`, menu items, buttons, Settings tab names and the default
names the code gives new screens) and each thing named more than one way was given one name: the one most used
already, unless it was unclear to a volunteer. 97 texts changed; their translations were kept (renamed in place in
every `.po`) for the translators to check. `tests/words.rs` fails when a new text uses a retired word.

## The glossary

Counts are texts (msgids) before the change.

| Thing | Word to use | Retired (texts that used it) | Notes |
|---|---|---|---|
| The screen the people in the room see | **audience screen** (Jake chose it, November 17, 2026: "I like Stage Screen and audience screen better") | room screen, Audience / Audience *n* as a bare screen name, "what the audience sees" | "projector" or "TV" stays for the device you plug in ("Plug in the projector/TV"). "Output" stays for the monitor a screen goes to (Screens → Output) and for a sound output. |
| The screen the band and speaker see | **stage screen** (24) | stage display (5), Stage Display *n*, stage view (4) | On a phone or tablet: "the stage screen for phones" (Settings → Network…). A **stage layout** is what it shows. |
| The window that sets them up | **Screens** (Window → Screens…) | Screen Configuration (window title) | The title now matches the menu item. |
| Sending the service to YouTube etc. | **livestream** (27); **stream** as the short noun ("the stream", "stream key") | live stream (3) | **Go Live** is the button's name (capitals when naming it); "go live" as a verb. **broadcast** only for YouTube's and Facebook's own event ("Broadcast title"); "can broadcast as you" became "can stream as you". |
| Where the stream goes | **destination** (14) | "places" (1) | YouTube, Facebook… as companies: **streaming service** (only in the setup help). |
| Saving the service as a video | **recording** / **record** | — | Already one word. |
| This Sunday's list | **service** (159), **service list** for the list on screen | order of service (1) → **service order** (the printed list) | "Plan" is only API.Bible's plan (another company's word). |
| A thing in the service | **item** (50) | — | **presentation** is a PowerPoint, Keynote or PDF file; its item kind is **Slides**. |
| A song's or verse's text on screen | **words** (song words) | lyrics (27), lyric (1) | Kept: SongSelect's own names ("SongSelect lyrics (.txt)", "Download → Lyrics", "Lyrics (.txt)"). **text** stays for type in general ("text size", "text box", Clear text). Layout names: "Words", "Lower-third words". Theme rows: "Song text size", "Bible text size", "ALL CAPS song words". |
| The Bible | **Bible**, **Bible passage** (a range), **verse** | Scripture (2 texts, 3 places: item kind, layout name, print), Bible reading(s) (3) | Item kind "Bible passage · 3 slides", blank "Bible passage". |
| Livestream scenes | **scene** (37) | — | Already one word. |
| Cameras | **Camera *n*** | — | A camera's saved aim is a **position** ("Save this shot as position 2"); where a layer sits in a scene is its **place**. Kept apart on purpose. "On air" is kept for the camera the stream shows now (tally), as TV crews say. |
| What you hear | **sound** (77) | — | **audio** (19) stays for audio files and items (the Audio chip, "Import audio…", Clear audio). |
| Sound equipment | **microphone or line-in**, **sound board** | sound desk (1) | The preset "Sound board or mixer" keeps "mixer" on purpose, to help people who call it that. |
| The phone that runs the slides | **phone remote** (7) | Remote password, "This remote…", "RustyPresenter Remote" | "Phone remote password", "This phone remote needs a password too.", page title "Phone remote — RustyPresenter". |
| Clear keys (F1–F8, View menu, Live column, macros, phone) | **Clear all**, **Clear text**, **Clear media & background**, **Clear audio**, **Logo**, **Clear overlays** | Clear All (3), Clear everything (1), Clear words (1), Clear video and pictures (1), Stop audio (1), "Clear Audio (F4)" | The Live column and phone show the short chips Text, Media, Audio, Logo under Clear all. |
| Seeing through a layer | **see-through** (8) | Opacity (1) | |
| The church's logo | **logo** / **church logo** | — | It is chosen in **Settings → Look** (two texts sent people to Settings → Themes or just "Settings"). |
| The computer | **this computer** (41) | this PC (6) | "the presentation computer" on the phone pages. |
| The next slide | **Up next** (the Live column's box), **Next** (the button) | "the chips under Next" → under Up next | **Preview** is the small picture of a screen. |
| The keyboard | **key**, **keyboard shortcuts** | — | Also **stream key** (a password from YouTube…) and an API **key** for a Bible service: always with their own word in front or clear from the sentence. |
| Mail | **email** (the guest pages already said it) | e-mail (1) | |

## Rules for new texts

- **Capitals:** sentence case for everything: buttons, titles, menu items, Settings rows, chips ("Clear all", "New
  service", "Stage screen 2"). Capitals only for names (RustyPresenter, SongSelect, YouTube, Go Live, Wi-Fi) and
  for ALL CAPS on purpose (LIVE badge).
- **Spelling:** British, as most of the app already was: colour (21 vs 7 "color"), centre (4 vs 1), licence for the
  noun (licensed for the verb), grey, favourite, recognise. Other people's names keep theirs (CCLI's "Streaming
  License", TikTok's "LIVE Center"); `{color}` and `{{license}}` are gaps the code fills, not words.
- **Settings paths:** the tab, then the section, joined by " → " (the arrow character, with spaces): "Settings →
  Media → Livestream", "Settings → Bible", "Settings → Look". A Settings menu item that is not a tab is written as
  the menu shows it, with its "…": "Settings → Network…", "Settings → Keys and clear groups…". Other menus the
  same way: "Window → Screens…", "File → New service…", "Edit → Find a verse…".
- **Ellipsis:** the one character "…", never three dots. On a menu item or button that opens a window where you
  choose or type something before anything happens ("Screens…", "Choose a video…"); not on one that acts at once
  or only shows something (Keyboard shortcuts, About RustyPresenter, Check the service).
- **Numbers:** in sentences, words: "10 seconds", "1 minute". In tight buttons and chips, short forms: "+15 s",
  "5 min".

## Left on purpose

- "projector"/"TV" for the device, "Output" for a monitor choice, "audio" for files, "on air" for the camera tally,
  "broadcast" for YouTube's event, "mixer" in one preset name, SongSelect's "Lyrics": see the table.
- Choose / pick and could not / couldn't are both everyday words used in ordinary sentences, not names of things;
  they were not changed.
- Saved names: screens still called "Audience", "Audience 2", "Stage Display", "Stage Display 2" (the old default
  names, and "Room screen" from a few days of test builds) are renamed once to "Audience screen", "Audience screen 2", "Stage screen", "Stage screen 2" when the data loads
  (`ScreenSetup::rename_old_defaults`); a name typed by hand stays. Saved blank items keep the label they were given
  ("Bible reading"); new ones say "Bible passage".
- `web/guest.html` and `web/guest-host.html` (untranslated, for the guest speaker) already followed the glossary.

## Keeping it

`tests/words.rs` reads the template and fails when a text uses a retired word (whole words; capitals matter only for
names like "Clear All"), names the word to use, and allows other people's names listed in `ALLOWED` (SongSelect's
"Lyrics", CCLI's "Streaming License", TikTok's "LIVE Center", Twitch's own "Settings → Stream"). It also fails on
three dots.

The list of every renamed text (old → new, with the things it changed) was handed to the translators as
`renamed-msgids.json`; their translations were kept, so nothing became untranslated, and they check them next.
