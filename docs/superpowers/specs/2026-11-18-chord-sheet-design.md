# Chords above the words (chord sheet on the stage screen)

November 18, 2026. Jake imported SongSelect's ChordPro download of a song and compared the stage screen with
SongSelect's chord sheet PDF: "We want the chords to appear more like the PDF. That is much more readable."

## What the band sees

Before: a stage box with "Show chords" showed the chords typed in the words, brackets and all:
`[D]  Sometimes the [A/D]night is beautiful`.

Now it shows a chord sheet, like the PDF:

- Each line with chords is two rows: the chords on top, each starting right above the letter it falls on, and the
  words below without brackets. A chord inside a word sits over that part of the word.
- Chords are bold, a little smaller than the words (90 %), in a colour of their own that reads well on the stage
  background: gold on a dark screen, blue when the box's words are already gold (the Next box), darker colours on a
  light background (`stage::chords::chord_color`).
- Chords close together never run into each other: a chord's column is as wide as the wider of the chord (plus a
  small gap) and its words, so the words get a little extra room instead, as on a chord sheet.
- A line of chords alone (an intro, an ending such as `[D]      [G/D]    [A/D]`) is a chord row alone, spaced as
  written. A line without chords is words alone (no empty chord row). Notes like "(1st Ending)" and "(To Verse 2)"
  stay words. `[*Softly]` (a ChordPro note for the band) sits in the chord row and is no longer shown in the room.
- The lines are left-aligned with each other (it is a sheet); the sheet as a whole is placed in its box by the box's
  alignment.
- The size still fits the box: the sheet (both rows of every line) gets the biggest size from the box's own down at
  which it fits. Long lines wrap between words, a chord staying with the word under it; a word too wide for the box
  makes the size smaller. Never cut off (same rules as `fit_between`).
- The song's Key −/+ still moves the chords. The audience screen never shows chords.

## How it is built

- `src/stage/chords.rs` (pure, tested): `split_line` turns `[D]  Sometimes the [A/D]night` into segments
  `[("D", "  Sometimes the "), ("A/D", "night")]` (words before the first chord as `("", …)`, brackets that are not
  chords left as words, an unclosed `[` kept). `items` cuts a line into the pieces that stay together when it wraps
  (each word with its chord; the parts of a word with a chord inside it). `lay_out` places every chord and every
  piece of words in pixels from a `width(text, chord)` function and the row heights, wrapping at a width.
- `stage::view::CellContent::Chords { lines, caption, chord_color }` replaces the bracketed text for slide boxes
  with chords on. `build` sets `chord_color` from the box's colour and the layout background.
- Stage window (`src/ui/stage.rs`, `ui/stage.slint`): Rust measures each piece once with a hidden Text in the
  window (the same font engine that draws it; between two bars so spaces count), lays the sheet out at each size it
  tries, and hands the window the placed pieces (`ChordPieceVm`) and the sheet's size. Each chord is drawn at the
  same x as the words under it, so it is over the right letter by construction; measuring only decides where the
  words after it go. The pieces are cached, so the clock's redraw every second does not rebuild them.
- The dock's Stage preview panel is a picture of the stage window, so it shows the same.
- Phone stage page (`web/stage-view.js`): cells of kind `"chords"` carry `lines` (arrays of `[chord, words]`) and
  `chord_color`; the page draws each piece as an inline column (chord above words) in a wrapping row, so the
  browser places them, and shrinks words until they fit as before. The cell also keeps `text` (the chords in
  brackets), so a phone still showing an older page draws the words as before.
