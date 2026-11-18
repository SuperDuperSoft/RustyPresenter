# Dragging panels between windows, like browser tabs

Jake, November 18, 2026: the dock panels (design `2026-11-17-dock-panels-design.md`) could only move to another
window through the ⋯ menu, because a drag stayed inside its window. He asked for real dragging, like browser tabs.
And: "It needs to let us save arrangements and remember arrangements, or layouts."

## What a volunteer gets

- In arrange mode (View → Arrange panels), drag a panel by the grip on its title bar **out of its window**. While the
  button is held the drag follows the pointer anywhere on the screen.
- Over any of the app's operator windows (the main window or a window of its own), that window lights up where the
  panel would go (left, right, top, bottom of a panel, or its middle to share its place as a tab), exactly as a drag
  inside one window does, and shows the panel's name next to the pointer.
- **Let go on a panel of another window**: the panel moves there. **Let go back in its own window**: an ordinary drop.
  **Let go over no window of the app** (the desktop, or another program's window in front of ours): the panel opens in
  a new window of its own at the pointer, as large as the panel was, wholly on the monitor under the pointer. A panel
  that is alone in a window of its own just moves that window there.
- **Esc** or a **right click** stops the drag; so does another program taking the keyboard. Nothing changes until the
  button is let go.
- A window of its own left with no panels closes (its panels went elsewhere). The main window always keeps a panel.
- The ⋯ menu's "Move to …" and "Move to a new window" stay, for touch screens and for anyone who prefers them.
- **Remembered**: every change made by dragging, and every window's monitor, place and size, is kept for next time
  (as before: Settings, a moment after each change; window moves every few seconds). "Save this layout as…" saves all
  of it, windows included, and choosing a saved layout opens each window on its monitor again.
- **"(changed)"**: when the arrangement in use is no longer the saved layout it came from (a panel moved, a divider
  dragged, a window moved more than a few pixels), View → Layouts shows "Sunday (changed)" and offers
  **Update “Sunday”** above "Save this layout as…": one click saves the changes into it.

## How it is built

- **Following the pointer outside the window.** Checked in the sources the app builds with: winit 0.30.13 on Windows
  calls `SetCapture` on every button press (`capture_mouse` in `platform_impl/windows/event_loop.rs`) and keeps sending
  `CursorMoved` with points outside the window (it sends `CursorLeft` first, which Slint 1.18.1's winit backend
  ignores while a button is held, `winitwindowadapter.rs`); Slint's `TouchArea` that took the press keeps the mouse
  (`grabbed`) and gets `moved` with `mouse-x`/`mouse-y` outside its window. So the title bar's grip already reports
  the whole drag; no polling, no timers. macOS: winit sends drags outside the view while a button is down
  (`view.rs`, `mouse_motion` checks `pressedMouseButtons`). X11: the server gives the pressing window the pointer
  until release. Wayland: the moves come too, but Wayland never tells a program where its windows are
  (`inner_position` fails), so there a drag stays in its window as before and the ⋯ menu moves panels between windows.
- **Where on the screen.** Each window's inside on the screen comes from winit (`inner_position`, `inner_size`,
  `scale_factor`), and where its dock area starts inside it from Slint (`DockArea.origin-x/-y`: under the top bar in
  the main window, with the scroll). A point in the source window's dock area becomes a screen point and then a point
  in the dock area of the window under it (`ui::dock::drag::OnScreen`).
- **Which window is in front.** On Windows, `WindowFromPoint` + `GetAncestor(GA_ROOT)` (windows-sys, already a
  dependency) says which top-level window is under the pointer, ours or another program's: exact, whatever overlaps.
  Elsewhere, the order our windows last came to the front (winit `Focused` events) decides
  (`drag::front_to_back`).
- **The name at the pointer** is drawn inside the window under the pointer (`DockArea.ghost`), set from Rust. A
  floating window of its own for it was left out: opening a window during a drag can take the keyboard from the
  source window, which would stop the drag. Over the desktop only the pointer shows.
- **Nothing changes until the release**; the drop uses the model's `drop_on` (any window) and the tear-off its
  `pop_out` with the place `drag::tear_off_place` works out (the monitor under the pointer, else the nearest; at least
  the smallest window of its own; never larger than that monitor). The change is applied after the release event is
  over (a zero timer), so a window that empties closes outside its own event handler.
- **Cancel**: Esc reaches `Operator::key` first (it does not clear the screens then); a right click or Slint's
  `cancel` (the window lost the mouse) arrive at the grip; a window losing the keyboard (winit `Focused(false)`, the
  main window's shared event filter and one per window of its own) cancels; a cancelled drag ignores its moves until
  the button is let go, or until one of the app's windows comes to the front again.
- **"(changed)"**: `Layout::same_arrangement` compares the panels, sizes, closed panels and the windows' places (16
  physical pixels either way count as the same; names and ids do not).
- **Tests**: the pure parts (`src/ui/dock/drag_tests.rs`: screen point → window and dock-area point, the front window
  where windows overlap, what Windows says is in front, tear-off placement on the right monitor, never larger than it,
  the smallest window, off every monitor; `same_arrangement`; the model across windows). The windows driven inline
  (`docking_tests.rs`, windows given screen places since the testing backend has none): a drag from the main window
  onto a window of its own, back from a window of its own (which closes), onto the desktop (a new window), a lone
  panel's window moved, a drag out and back, Esc / right click / losing the keyboard, a restart keeping a tear-off and a
  cross-window drop and "Save this layout as…" saving them, and "(changed)" with "Update “Sunday”".
