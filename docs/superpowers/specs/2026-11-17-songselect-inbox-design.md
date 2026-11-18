# SongSelect downloads come in by themselves (November 17, 2026)

Jake: "how can we use SongSelect as an easy way to grab songs from our account without API access and without
breaking any terms with them, and have it built into RustyPresenter".

## What SongSelect's terms allow (read November 17, 2026)

SongSelect's Terms of Use: the subscription is "non-exclusive, non-transferable ... for your own non-commercial
ministry purposes"; the church "will not rent, lease, sublicense, transmit, distribute, transfer, copy, ... or allow
any third party to use the Service"; passwords are only for church staff or volunteers. There is no public API
without a CCLI partnership.

So RustyPresenter **never**: signs in to SongSelect, asks for or keeps a SongSelect password, reads browser cookies,
fetches or scrapes any SongSelect page, downloads anything on the church's behalf, or sends a song anywhere outside
this computer. It **only**: opens SongSelect's home page in the church's own web browser, and reads files the person
downloaded themselves from SongSelect (that is what SongSelect's Download button is for). The person signs in, finds
the song and presses Download, exactly as they would without the app.

SongSelect's search address could not be checked (its site refuses automated reading), so the app opens the home
page and, for "Look for it on SongSelect", copies the song's name so it can be pasted into SongSelect's search box.

## What the volunteer sees

- **Add → Song → "From SongSelect…"** (also File → Add to service → "Songs from SongSelect…") opens a small window:
  1. Sign in to SongSelect in your web browser. 2. Find the song and choose Download → Lyrics (or ChordPro).
  3. It comes in here by itself. Buttons: **Open SongSelect**, **Choose a file…** (for a song downloaded earlier),
  **Close**. Under the steps: the songs that came in while the window was open, each with its title, CCLI number,
  what happened (added / already in your library / kept both as "… (from SongSelect)" / could not read) and
  **Add to the service**.
- **A song search that finds nothing** (Add → Song) shows "Look for it on SongSelect": the SongSelect window opens,
  the browser opens SongSelect, and the words searched for are copied to paste into SongSelect's search.
- **Settings → Songs → "Bring in SongSelect downloads whenever RustyPresenter is open"** (off by default): for
  churches that download during the week. Each song that comes in gets a short message at the bottom.
- **Dragging** a SongSelect file (.txt, .usr, .chordpro, .cho, .chopro, .pro) onto the window, or picking it with
  "Import SongSelect files", brings it in as before (asking about a song with the same CCLI number).

## How it works

- `songselect::watch::Watcher` looks at the top of the computer's real Downloads folder (`dirs::download_dir`, which
  is the Windows Known Folder, `~/Downloads` on macOS and the XDG folder on Linux) once a second, off the window's
  thread. Files already there when it started are ignored. A file is taken once it is not empty, its size and time
  stayed the same for two looks in a row, and no unfinished download of the same name is next to it
  (`.crdownload`, `.part`, `.partial`, `.download`, `.tmp`). Only song file names are looked at; files bigger than
  1 MB are skipped. Files are only read: never moved, renamed or deleted.
- `songselect::inbox::read_export` decides if a file is a SongSelect export: the Lyrics `.txt` with a
  "CCLI Song #" line, a ChordPro file (`.chordpro`, `.cho`, `.chopro`, `.pro`, or a `.txt` with ChordPro tags) with
  a CCLI number, or SongSelect's `.usr` file (`[S A<number>]`, `Title=`, `Author=`, `Copyright=`, `Admin=`,
  `Fields=` and `Words=` with `/t` between parts and `/n` between lines). Anything else is left alone without a word.
  A file that is clearly from SongSelect but cannot be read is listed as "could not read" with the reason.
- `songselect::inbox::place` keeps the church's edits: a song whose CCLI number is not in the library is added; one
  whose number is there with the same words (ignoring case, spacing, punctuation and section names) is "already in
  your library" and nothing changes; one with different words is added beside it as "<title> (from SongSelect)".
  Every song keeps its CCLI number, writers and copyright, so the credit line and the song use report work.
- One watcher per window serves both the SongSelect window and the Settings choice, so a file is never brought in
  twice.
