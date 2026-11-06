# Classic menu bar (UI-MENU) — design

**Date:** November 6, 2026 · **Asked by:** Jake (PROJECT_PLAN §10c, "UI-MENU — classic menu bar") · **Board id:** UI-MENU

## 1. Goal

The usual menu bar along the top of the operator window: **File, Edit, View, Window, Settings, Help**. Every item runs
an action the window already has (a button, a key or a dialog). Buttons stay; the menu is an easier-to-find way in.

## 2. How it is built

- Slint 1.18's built-in `MenuBar` as the first child of `OperatorWindow` (`ui/operator.slint`), with `Menu`,
  `MenuItem` and `MenuSeparator`. Sub-menus are `Menu` inside `Menu`. Every title is `@tr("…")`.
- Windows and macOS get a native menu (Slint uses `muda`); Linux and the testing backend get Slint's own drawn bar
  (the Fluent style, 40 px). On Windows the native bar is outside the client area; on Linux the window's height
  includes it. Slint moves the window's children under the bar, so `root.height` in the window is the client height.
- Slint does not call `TranslateAcceleratorW`; on every platform the window itself matches menu shortcuts **before** the
  focused element sees the key (`i_slint_core::window::process_menubar_shortcuts`), and only for enabled items.
  So `shortcut:` is set **only where the menu's handling is the same as `keys::route` in every focus state**:
  F1–F5 (route clears from anywhere, dialogs and text boxes included) and Ctrl+B (route focuses the Bible box unless a
  dialog or question is up; the item is disabled then, so the key falls through to route, which ignores it).
  Keys whose meaning depends on focus (arrows, Space, Page Up/Down, Enter, Ctrl+V, C B P E 1–9, Alt+1–5, Esc) get
  **no** `shortcut:`: a bound shortcut would take the arrow or Space away from a text box. They are listed in
  Help → Keyboard shortcuts (Settings → Help, which a test already keeps in step with `keys::route`).
- An item calls the same window callback its button uses. Items with no window-level callback today (Add on a given
  tab, Settings on a given tab, Exit) go through one new callback `menu(string)`; `src/ui/operator/menu.rs` turns the
  id into a `MenuAction` (pure, unit-tested) and runs the existing function: `add::open_add(op, tab)`,
  `settings::open(op, tab)`, or a `CloseRequested` window event (the same path as the window's X: `main.rs` quits).

## 3. Contents

`ready` = the first state read has arrived (`loaded`) and no dialog or question covers the window (`covered`).
While covered, the on-screen buttons are under the dialog's backdrop, so their menu items are greyed too.

| Menu | Item | Shortcut | Runs | Enabled |
| --- | --- | --- | --- | --- |
| File | New service… | | `ServicesDialogState.new-service()` (Services → New) | ready |
| | Services… | | `services()` (top-bar service picker) | ready |
| | Open a service file… | | `ServicesDialogState.open-file()` | ready |
| | Save this service to a file… | | `ServicesDialogState.save-file()` | ready and a current service |
| | Add to service ▸ Song… / Bible passage… / Video & media… / Slides… / Announcement… | | `menu("add-…")` → `open_add(tab)` | ready |
| | Import media… | | `bin-import()` (media library Import) | ready |
| | Exit | | `menu("exit")` → close request | always |
| Edit | Edit the selected item… | | `slides-edit()` (slides header Edit) | ready and an item selected |
| | Bible box | Ctrl+B | `focus-bible()` (what route's FocusBible runs) | not covered |
| View | Next | | `live-next()` | not covered |
| | Back | | `live-prev()` | not covered |
| | Clear all / Clear text / Clear media & background / Stop audio / Logo | F1–F5 | `live-clear("clear_all" …)` | always (F-keys work over dialogs too) |
| | Hide media library / Show media library | | toggles the media library's open state (its header click) | not covered |
| Window | Screens… | | `screens()` | ready |
| | Phone remote… | | `phone()` | ready |
| | Stage message… | | `stage-message()` | ready and the stage button is shown |
| Settings | Settings… | | `settings()` (top-bar gear, Look tab) | ready |
| | Songs… / Bible… / Video… / Backups… | | `menu("settings-…")` → `settings::open(tab)` | ready |
| Help | Keyboard shortcuts | | `menu("settings-help")` | ready |
| | Check the service | | `check-service()` (service column Check) | ready |
| | About RustyPresenter | | `menu("settings-about")` | ready |

Item words reuse the app's existing words (Settings → Help table, the clear buttons, the Add tiles, Settings tabs).
The clears sit under View because Jake's list has no Live menu; moving them to a Live menu later is a title change.
Clicking a dialog item first gives the keyboard back to the window (`focus-keys()`), as the top-bar buttons do. The
clear items do not, matching the F-keys (F1 in the Bible box clears and leaves the box focused).

## 4. Later (no action exists yet, so not in the menu)

- File: Open recent ▸, Save / Save as (the app saves by itself), Import ▸ Songs from EasyWorship and SongSelect lyrics
  (today these buttons live inside the Add dialog and need its state; a window-level action first), Export ▸ Song,
  Print service order (FEAT-6).
- Edit: Undo, Redo, Cut/Copy/Paste/Delete (Slint has no "focused text box" action for a menu), Find… (Ctrl+F),
  Edit arrangement (FEAT-4).
- View: Grid / Big-text / Outline views, action icons, Zoom (FEAT-4, FEAT-2).
- Window: Room / Stage screen on-off toggles (Screens dialog only today), Looks…, Stage layouts…, Practice mode
  (FEAT-1/3/6).
- Settings: Media folders, Themes (FEAT-1), a Language tab (I18N).
- Help: Open logs folder (no action yet).
- Showing → / Space next to Next and Back: Slint 1.18 has no display-only shortcut text.

## 5. Layout at 1366×768

The bar is one row: 40 px drawn by Slint on Linux, a native ~20 px row on Windows (outside the client area), none on
macOS (top of the screen). `ui::operator::fit` adds 1366×768 to its sizes; columns, top bar, Next and the clear
buttons must stay inside the window there and at 1280×720. Checked by eye under Xvfb (`/root/shots/menu-*.png`).

## 6. Tests (each written first, seen failing)

Rust (`src/ui/operator/menu.rs`):
- `every_menu_id_in_the_window_has_an_action`: every `root.menu("…")` id in `ui/operator.slint` parses to a `MenuAction`.
- `menu_ids_map_to_their_tabs`: `add-song` → `Add(Song)` … `settings-about` → `Settings(About)`, `exit` → `Exit`, unknown → `None`.
- `every_menu_title_is_translatable`: every `title:` inside the `MenuBar` block is `@tr(…)`.
- `shortcuts_match_the_keys`: each `shortcut: @keys(…)` in the bar is one of F1–F5 or Control+B, and its item calls the
  callback `keys::route` gives that key (F1 → `clear_all` … F5 → `logo`, Ctrl+B → `focus-bible`).
Window (testing backend, `src/ui/operator/menu.rs` `window_tests`):
- `menu_opens_settings_on_a_tab`, `menu_opens_add_on_a_tab`, `menu_exit_closes_the_window`.
- `menu_items_greyed_until_ready_and_while_covered` (via the window's `menu-ready`, `menu-can-edit`, `menu-can-save` out properties).
- `f2_runs_through_the_menu_bar_and_clears_text`; the existing F1/Ctrl+B key tests keep passing through the bar.
- `fit` tests at 1366×768.
