# Dock panels: the operator window rearranged, resized, saved, and spread over monitors

Jake, November 17, 2026: "I want the whole UI to be modular … multiple screens and custom layouts … right now the
layouts can look kind of bad because the Bible thing can go over the room thing at the top and they're overlapping …
custom savable layouts and modular like I can move around the size of each thing." He chose **dock panels** (like
OBS) and **pop panels out into their own window** on another monitor.

## What a volunteer gets

- The operator window is made of **panels**: Service, Slides, Library (songs, media), Bible, Live (the audience screen
  preview and its controls: next/previous, clear, transport), Up next / notes, Cameras, Message & macros (automation
  strip), Livestream (the Go Live panel and its health), Stage preview, Follow (auto-follow). Each existing area of
  today's window becomes one panel, nothing is lost. The menu bar and the thin top bar (title, Go Live, search) stay
  where they are; things that today float over other areas (the Bible box at the top) become panels, so **nothing
  overlaps anything ever again**.
- **Panels never overlap.** The window is split into rows and columns; dividers between them can be dragged to resize
  (each panel keeps a sensible minimum size; a too-small window scrolls rather than squashes past it).
- **Arrange mode** (View → Arrange panels, and a button in the layouts menu): each panel shows a title bar with a
  grip and a ⋯ menu. Drag a panel by its title onto another panel: drop zones show left / right / top / bottom (split
  beside it) or centre (become a tab with it). Tabs show their panels' names; click to switch. The ⋯ menu: Close panel,
  Move to a new window, Move back to the main window. Closed panels come back from View → Panels (a tick list).
  Outside arrange mode, layouts are **locked**: no title bars and no grips beyond the dividers, so a volunteer cannot
  break the layout on a Sunday by accident (dividers still resize).
- **Layouts**: View → Layouts: the built-in ones (Standard = today's arrangement; Big slides; Livestream (Live,
  Cameras and Livestream panels larger); Two monitors (Live + Cameras + Livestream popped out)), the church's own saved
  ones, "Save this layout as…", "Rename…", "Delete", "Reset to Standard". The chosen layout is remembered and opens next
  time. Keyboard: none by default (the presentation owns the keys); can be given in Settings → Keys like other actions.
- **Pop-out windows**: a panel (or a tab group) moved to a new window gets its own window (title "RustyPresenter —
  <panel>"), which can be dragged to another monitor, resized, and holds its own dock area (more panels can be dropped
  into it). A saved layout remembers each window's monitor and place; if that monitor isn't plugged in when it opens,
  the window comes to the main monitor. Closing a pop-out window puts its panels back into the main window (never
  lost). The audience/stage output windows are unchanged and separate.
- Small screens (1024×768 laptop): Standard fits as today; the panel minimums prevent unreadable layouts.

## How it is built (technical)

- **Model (Rust, pure, tested)**: `ui::dock` — `Layout { name, windows: Vec<DockWindow> }`, `DockWindow { monitor,
  rect, root: Node }`, `Node = Split { dir: Row|Column, children: Vec<(Node, f32 share)> } | Tabs { panels: Vec<PanelId>,
  active }`. Pure functions: place every panel's rectangle for a window size (respecting minimums), the divider
  rectangles, drop (panel onto target zone → new tree), close/restore, pop out/in, normalise (no empty splits, shares
  sum to 1), validity (every panel at most once). Saved in settings (`serde`, lenient: unknown panels dropped, missing
  ones restored to Standard's place). Built-in layouts are code; the church's are data.
- **Rendering (Slint)**: a `DockArea` component draws each visible panel at the rectangle the model gives (absolute
  x/y/width/height from a model of placements), dividers as drag handles (dragging calls back with the new share),
  tab bars, and in arrange mode the title bars, ⋯ menus and drop-zone highlights. Each panel component is created once
  per window and placed by `if`/position, so its state is not rebuilt when the layout changes.
- **Panel state in globals**: today panels take their data from `OperatorWindow`'s own properties. For pop-out windows
  every panel's data and callbacks move to Slint `global`s (one per panel, e.g. `ServicePanelState`), and the Rust side
  writes to the globals of **every open operator window** (main + pop-outs) through one helper
  (e.g. `Operator::each_window(|ui| …)`), and wires callbacks on each window as it opens. Slint creates the globals per
  window instance (checked in slint 1.18.1's generated code: `SharedGlobals` per component instance), so mirroring is
  required. A pop-out window is a second instance of the operator window component (or a lighter `PanelWindow` that
  imports the same panels and globals), showing only its dock tree. Reads (`get_`) come from the main window.
- **Tests**: the dock model thoroughly (drops in every zone, closing the last panel of a tab group, minimums, shares,
  pop out/in, lenient loading); the window driven inline (arrange mode, a divider drag, saving and choosing a layout,
  a pop-out window opened and closed and its panels coming back, a panel's state shown in the pop-out window).
