# Songs folder, chords in another key, intro chords (November 17, 2026)

Jake: "I need a toggleable folder where I can just store all of my songs, where I know where RustyPresenter stores
all the songs, but I can also upload songs too, unless you keep it in a different format." Then: "Make the chords
transposable as well." And: "Sometimes these have intro chords — will it show those before the song, like it plays
during the intro before anyone's singing?"

## 1. Songs folder

**Settings → Songs → Songs folder** (`ui/dialogs/settings-songs-folder.slint`, `src/ui/dialogs/songs_folder.rs`):
"Keep my songs in a folder" (off by default). When on: the folder's path, **Open the folder**, **Choose another
folder…**, **Use the usual folder** (only when another was chosen), a status line ("212 songs in the folder · last
change 3 minutes ago · 1 file could not be read: notes.txt", or what is wrong: "The songs folder cannot be found. Is
the drive connected?"), and **Tidy imported files into ChordPro** (off by default) with a line saying what it does.
Each choice saves at once (`songs-folder/set`); the Settings window's own Save never changes them. The usual folder
is Documents\RustyPresenter\Songs (the person's Documents folder on macOS and Linux).

The data file (`data.json`) stays the app's own record: services point at song ids, backups and Undo work as
before. The folder is the church's open copy, kept the same both ways by one thread (`songs_folder::start`) that
looks about every two seconds and at once when Settings changes. What it knows of the folder (which file is which
song, as last seen) is in the data folder (`songs-folder.json`), never in the songs folder.

### The file format

One file per song, `<title>.chordpro`, UTF-8, ChordPro: Notepad opens it, chords stay in the words, other worship
programs read it, and the app's importers already read ChordPro. Example:

```
# A RustyPresenter song (ChordPro). The {x_rustypresenter_…} lines are for RustyPresenter; other programs skip them.
{title: Amazing Grace}
{artist: Chris Tomlin}
{composer: John Newton}
{composer: Edwin Excell}
{year: 1779}
{copyright: Public Domain}
{ccli: 22025}
{key: G}
{transpose: 2}
{x_rustypresenter_order: Intro, Verse 1, Chorus, Verse 1}
{x_rustypresenter_lines_per_slide: 2}
{x_rustypresenter_id: 6f1c2a0e-3c51-4c55-9a43-2b2e8c0d9f11}

{start_of_verse: Intro}
[G] [D] [Em] [C]
{end_of_verse}

{start_of_verse: Verse 1}
[G]Amazing [C]grace, how [G]sweet the sound
That saved a wretch like me
{end_of_verse}

{start_of_chorus: Chorus}
My chains are gone
{end_of_chorus}
```

- Details are standard directives (`title`, `artist`, one `composer` per writer, `year`, `copyright`, `ccli`, `key`,
  `transpose`); what only the app uses is in `x_rustypresenter_…` directives (ChordPro lets other programs skip
  `x_` directives): publisher, order, lines per slide, text size, id.
- Each section name opens `{start_of_verse|chorus|bridge: <the name as written>}` and is closed before the blank
  lines that end it. Everything else in the words is written as it is (chords, `{Instrumental}` slide labels).
- Read back, a file is exactly the song it was written from (tested for odd titles, spacing, Unicode, blank lines,
  lines that look like directives or `#` comments, every combination of awkward lines). A words line another
  program would take for a directive or a comment is kept in `{x_rustypresenter_line: "…"}`; a detail that cannot
  sit on one line (a line break, a brace) is written tidied for other programs with its exact text beside it in
  `{x_rustypresenter_exact_<detail>: "…"}`. An edit made in Notepad wins over the exact text.
- File names come from the title, made safe for Windows, macOS and Linux (no `<>:"/\|?*`, no device names such as
  CON, no dots or spaces at the ends, at most 100 characters) and unique without regard to capitals:
  "Amazing Grace.chordpro", "Amazing Grace (2).chordpro".

### Both ways

- **Turning it on** writes every song. **Turning it off** leaves the files where they are and writes nothing more;
  turned on again, what changed meanwhile is written (and a file changed meanwhile too keeps both, below).
- **In the app** a song added, edited, renamed or removed changes its file at the next look: a new title renames the
  file; a removed song's file is moved into the folder's **Removed** subfolder (with any file it was brought in
  from), never deleted. A file put back from Removed brings its song back with its id, so old services find it.
- **A file of the app's edited outside** (Notepad, another program) changes the song; the id stays. When the song
  was also changed in the app since the last look, both are kept: the folder's version comes in as
  "<title> (from the folder)", the app's goes back into its file, and a message says so.
- **Files dropped into the folder** in any format the app reads (ChordPro `.chordpro/.cho/.chopro/.crd/.pro`,
  SongSelect `.txt` and `.usr`, OpenLyrics `.xml`, OpenSong with or without `.xml`, plain text with Verse/Chorus
  headings, its title on the first line or taken from the file name) come in by themselves, unless the library
  already has that song (same CCLI number, or same title when there is none) with the same words. **The clearest
  behaviour, chosen:** with Tidy off (the default) the dropped file stays exactly as it is and the song's own
  ChordPro file is written beside it; editing the dropped file later changes the song (its lines per slide, text
  size, key move and order stay as the app has them). With **Tidy** ticked the dropped file is moved into the
  **Imported files** subfolder once the song is in, so only ChordPro files stay. Nothing is ever deleted either way.
- **A file that cannot be read** is listed in the status line and in a message, never moved or changed, and read
  again only when it changes. Files bigger than 1 MB are not songs.
- **A file deleted from the folder by hand** is written again (songs are removed in the app, where services may
  use them; Settings says so). A file **renamed** in Explorer stays the song's file; a **copy** becomes a new song.
- **Another computer / lost state**: the files carry the song ids, so a library opening the same folder takes its
  own files back without doubling them; a file with other words than the library keeps both.

### Safety

- Writes are a new temp file (`.rp-….tmp`, ignored by the folder look) flushed to the disk and renamed over the old
  one (`store::write_atomic`): a crash leaves the old file.
- The app's own writes are recognised by their contents and stamps and never taken as changes.
- A file is read only once it stopped changing (same size and time on two looks, or last changed over three
  seconds ago), so a copy, a OneDrive/Dropbox download or a network copy in progress is never read half done.
- A busy or locked file (OneDrive, a network drive, an editor holding it) is tried again at the next look; the app
  keeps the song as it knew it until then. A file changed outside is never overwritten before it was looked at.
- A folder that cannot be made or is gone (unplugged drive) is said in Settings; nothing is lost and it carries on
  when it is back. Data opened read-only (saved by a newer version) writes nothing.

## 2. Chords in another key

- `Song` keeps `key` (as written: "G", "Bb", "F#m"; from ChordPro `{key}`, SongSelect `.usr` `Keys=`, or typed) and
  `transpose` (−11 … +11 semitones). Both are left out of `data.json` when empty/0, so old data loads unchanged.
- Only the stage screen's chords move (`crate::transpose`, applied in `slides::song_slides_in` to `slide.chords`):
  each chord's root and bass note move, the rest stays (`[F#m7/C#]` +2 → `[G#m7/D#]`); things in brackets that are
  not chords stay. Sharps or flats follow the key played in (F, B♭, E♭, A♭, D♭ major and D, G, C, F, B♭, E♭ minor
  are written with flats, all others with sharps), so B♭ never shows A♯. With no key typed, the first chord says
  the key. The stored words keep the chords as written; the audience screen never shows chords.
- **Song editor**: a Key row under Lines per slide: the key field, − / +, where it is now ("G → A, +2"), "As
  written" to go back, and a capo hint ("Capo 2 to play G shapes", for moves of 1–7 up).
- **Quick control during a service** (`ui/song-key.slint` `SongKeyControl`, `src/ui/song_key.rs`): Key − / now / +
  / 0 for the selected song, saved with `song/key`. For the Slides panel header (the lead places it). On the live
  song the stage screen changes at once: only the chords differ, so the live engine takes the new slides in quietly
  (no "Update screen" banner, the room unchanged).

## 3. Intro chords

- A line of chords and no words is kept in the song (ChordPro import, SongSelect ChordPro, the songs folder files,
  typed songs). A paragraph of chords alone outside any section is an "Intro" before the first words, an
  "Instrumental" after.
- Such a section is a slide of its own where the order puts it: the audience screen shows no words (the background
  only; no chord letters, no section name, and the song's credit line goes on the first and last slides with
  words); the stage screen shows "Intro ♪" and, in a box that shows chords, the chords (moved like the rest); its
  next box shows the first sung line. The slide list and the song editor's preview mark it "♪ chords only".
- Auto-follow already treats a slide with no words as instrumental: it stays up while the band plays and the first
  words come up as singing starts (tested with a real intro slide).

## Placing the two new components (for the lead)

- Settings → Songs page: `import { SongsFolderRows } from "settings-songs-folder.slint";` and put `SongsFolderRows {}`
  on the page; call `crate::ui::dialogs::songs_folder::connect(&op)` once when the operator window opens (it starts
  the status timer and answers the callbacks) and `songs_folder::fill(&op)` when Settings opens.
- Slides panel header: `import { SongKeyControl } from "../song-key.slint";` and put `SongKeyControl {}` beside the
  song's A− / A+ and lines controls; call `crate::ui::song_key::connect(&op)` once when the window opens and
  `crate::ui::song_key::render(&op)` wherever `render_text_size` runs (selection or library changed).
- `SongsFolderState` and `SongKeyState` are already exported from `ui/app.slint`.
