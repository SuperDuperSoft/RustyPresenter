# Syllables and slide editing (November 18, 2026)

Jake, with a screenshot of "You've Already Won" from SongSelect's ChordPro: "notice there's a hyphen in 'defense'.
There should not be." Then: "Editing a song should be MUCH easier. There should be a way to just edit slides." And:
"It should be as easy as humanly possible to rearrange the song in any order you like."

## 1. Words SongSelect splits into syllables

SongSelect's ChordPro breaks a word where a chord falls in it and writes the break as space, hyphen, space:
`my [Dbsus]de - [Db]fense`, `righteous - [F#m]ness`, sometimes with no chord at the break: `a - gain`.

- **Rule** (`src/import/syllables.rs`): a letter (and any chords after it), one or more spaces, a plain `-`, one or
  more spaces, any chords, then a small letter is one word: the spaces and the hyphen go, the chords stay where
  they fall (`de[Db]fense`). Real dashes stay: a capital or anything that is not a letter after the dash, en and em
  dashes, a hyphen without spaces, a dash at the start or end of a line.
- **Where**: the ChordPro reader joins every lyric line before anything else, so the importer, the SongSelect inbox
  and the songs folder (all read ChordPro through it) get whole words.
- **Songs already in the library**: a tidy at load (`Data::after_load`) joins the breaks only in songs that still
  show SongSelect's sign: chords in the words and at least one break with a chord at it (`de - [Db]fense`), which
  nobody types by hand. A song the church typed, or one without such a break, is never touched. Each changed line
  is written to the log (`Joined split words in "<title>": "<was>" → "<now>"`). It is idempotent: once joined there
  is nothing left to find, so it runs at every load without changing anything again (a song restored from an old
  backup is tidied too).
- **Stage chord sheet** (`stage::chords::lay_out`): inside a word the parts sit side by side ("de" then "fense"),
  so no space is added inside the word; each chord starts over its part, or just after the chord before it when
  that one is wider. The phone's stage page does the same (`web/stage-view.js`, `spaceChords`).

## 2. Editing a song from its slides

The song stays one text (section names, blank lines between slides, `[G]` chords): the slides panel only finds the
lines a slide came from and changes those (`src/song_edit`). Everything else in the text stays as the church wrote it.

- **Slide → lines** (`Doc::find`): the slide's `SectionRef` (played place and page) → the written section the order
  plays there → its blank-line group and page (lines per slide), including a `{Label}` line on top. Tested for every
  slide at 1, 2 and 4 lines per slide, with repeats, labels and chord-only slides.
- **In place**: double-click a song slide (or its pencil, or Edit words in its menu) and a small editor opens over
  the slides: the part's name in its colour, "Chorus plays 3 times in this song. Your change shows every time." when
  it repeats, the words without chords, and under them the chords over the words as the band reads them. Enter
  saves, Shift+Enter starts a new line, Esc cancels. A blank line typed in it starts another slide.
- **Chords**: by default the box shows only the words; on Save each line whose words did not change stays exactly as
  it was, and a changed line gets its old chords back over the same letters (a character alignment of the old and
  new words), lines of chords alone stay where they were. "Edit chords too" shows them as `[G]` marks to edit, and is
  remembered for the session. A slide of chords alone always opens with its chords.
- **The slide's menu** (right-click, or ⋯ on the picture): Edit words, Split here… (the editor opens; the cursor's
  line starts the new slide), Join with next (the next slide of the same part; when the two are one part shown a few
  lines at a time it says to use Lines +), Add a slide after, Duplicate, Delete slide (asks first when the part
  plays more than once; a part with no slides left goes, and leaves the orders), Change section… (from this slide on
  the part is the chosen name: on its first slide the part is renamed and the orders follow; further in it is split
  and the new part plays after it wherever it played; the list offers each kind with a free number, in its colour),
  Move this part earlier / later, Edit the whole song… (the song editor with the cursor at this slide).
- **Live**: Edit words in the Live panel shows the live song's slides and opens the live slide in the same editor,
  with Save and show now. A change to the words on screen shows the usual "Update screen" banner; nothing on screen
  jumps.
- **Undo**: every change is one `song/slides` action, one `App::edit`, so Edit → Undo takes it back in one step.

## 3. The order, as easy as it can be

- **On the Slides panel** (any selected song): the order as coloured chips. Drag a chip onto another to move it,
  onto End for last, onto ✕ Remove (or ×) to take it out, ⧉ (or D) plays it twice, drag a `+ <part>` button in
  between chips to play that part there (a click: at the end). Keys: ←/→ choose, Ctrl+←/→ move, Delete removes, D
  plays the chosen one twice (⧉ on the chosen chip too). Quick order ▾ (on the strip's row, to keep the bar short):
  Chorus after every verse, Chorus again at the end. A slide dragged onto another slide moves its part there too.
- **This service or always**: a change made on the panel is this service's own order of the song (the song itself
  is untouched); then "Just for this service" shows with "Save as the usual order" (the song plays this way in every
  service, and this one follows it again) and "Reset to the song's order". The song editor's chips change the song's
  usual order; Item edit's change the service's, as before. All three use the same chips.
- **While live**: a new order never changes the room. The app knows where each part went (`order::apply` gives old →
  new played places) and hands it to the live engine (`Live::order_hint`), so the live part stays live where it went
  and Next follows the new order (without it, two identical choruses could swap).

## 4. The song editor

- **Add a part** buttons (Verse, Chorus, Pre-chorus, Bridge, Tag, Ending, Intro (chords only)): the name with the next
  free number goes after the part the cursor is in, the cursor on the line for its words.
- **Preview**: click a slide to put the cursor at its words.
- **Order**: the chips with `+ <part>` buttons that drag in (they replace the separate rows of buttons); typing the
  order is still there, labelled "Or type the order here".
- Key row, lines per slide, text size and chords-only marks were there already and stay.

## Not done / limits

- The editor shows over the Slides panel: if that panel is closed (View → Panels), double-click and Edit words have
  nowhere to show it.
- Join cannot join two slides that are one part shown a few lines at a time (they are one block of words); it says so.
