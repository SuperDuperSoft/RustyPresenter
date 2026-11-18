# Native App — Plan 4: Imports, Crash Recovery, Packaging, Tauri Removal

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the native app so a volunteer on Windows, macOS or Linux installs one package and runs a service. PowerPoint, Keynote, ODP and PDF decks import on every platform: through PowerPoint on Windows when it is installed, otherwise LibreOffice, then pdfium. Media is probed and thumbnailed by headless mpv, so FFmpeg is no longer needed. A supervisor process restarts a crashed app with `--restore` and puts the last committed frame back. A second launch focuses the running app. Installers bundle the LGPL libmpv, pdfium and licences. Tauri, WebView2, the web operator/output/stage pages, `render.js` and FFmpeg are removed.

**Architecture:** New modules `import` (sniff, pdf, libreoffice, powerpoint, deck, probe, media) and `recovery` (state, supervisor, panic), plus new `platform` files: `resources` (where the package put our files), `tools` (finding LibreOffice, PowerPoint and pdfium), `proc` (killing whole process trees) and `instance` (single instance). The executable starts as a tiny supervisor (std only, no Slint, no tokio, no port) that launches itself with `--app`. Imports run on tokio and report through the existing `App::job_*` jobs. pdfium lives on one dedicated thread. Probes use short-lived libmpv cores. Packaging uses cargo-packager driven by `Packager.toml` and a staging script. A separate cached CI workflow builds the LGPL libmpv. When Tasks 12–13 are done, the native binary is the only binary, named `rustypresenter`.

**Tech Stack:** Rust 2024, Slint 1.18, libmpv client API 2.x (`libloading`), `pdfium-render` 0.8.37 (pdfium bound at runtime), `zip` (current major, `default-features = false, features = ["deflate"]`), `cfb` 0.10, `quick-xml` ≥ 0.37, `url` 2.5, `tokio-util` 0.7 (`CancellationToken`), `image` 0.25 (features `png, jpeg, gif, webp, bmp`, exactly as in `Cargo.toml` today; no `avif`), `rfd` 0.17 (from Plan 3), `windows-sys` 0.61 (extra features below), `libc` 0.2 + `signal-hook` 0.3 (Unix only), `winresource` (build-dep, Windows only), cargo-packager **0.11.8**, cargo-about, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-29-native-slint-design.md` §2, §5 (thumbnails and probing), §8.3–8.4, §9, §10, §11, §13, §14 steps 8–11. The v1 spec (`2026-10-29-rustypresenter-v1-design.md`) still governs import behaviour (§9) and backups (§11). **Check results:** `docs/superpowers/checks/2026-native-checks.md`. Gotchas 1–6 from C5 are requirements for Tasks 1–6. Gotchas 1–7 from C1 still apply to probing.

## Global Constraints

- **Preconditions:** Plan 2 is finished, including Task 8 (`rustypresenter-native`, `ui::screens::NativeScreens`, panic log). Plan 3 is finished: the operator window, its dialogs, the job/notice display, and Settings → About with `AboutSlint`. Where this plan changes Plan 3 files, it names the behaviour, not line numbers. Read the file first.
- All Plan 1 and Plan 2 constraints still apply. Until Task 12 the Tauri binary must still build and `cargo test --locked --release` must pass. From Task 12 on there is one binary and CI runs `cargo test --locked` in debug.
- After Task 8, new imports are no longer converted. The Tauri web room may fail to play unusual formats from then on. That is acceptable because the native room is the one in use.
- **No blocking work on the UI thread.** Imports and probes run on tokio (`spawn_blocking` for blocking parts). pdfium runs on one dedicated thread, because it is not thread-safe. At most 2 probes run at once, each with `hwdec=no` so they never take the GPU decoder from the live room. Nothing an import does may slow Next or the clears (v1 §14 responsiveness row: under 200 ms during an import).
- **Imports publish atomically.** A deck or media entry is added in one `App::edit` only after every asset is in the store. A `WorkDir` drop guard always removes the work folder under `store.tmp()`. A failed or cancelled import adds nothing.
- **Process trees die together.** Every converter is spawned through `platform::proc::spawn_tree`: a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` on Windows, `process_group(0)` plus `kill(-pgid, SIGKILL)` on Unix. Windows children are spawned with `CREATE_NO_WINDOW`. On Windows the job handle closes when the app dies for any reason, so the tree dies with it. On Unix a process group does **not** die with a SIGKILLed or crashed app, so every LibreOffice run records its pgid in `<data>/deps/libreoffice.pgid` and start-up kills a leftover group before anything touches the profile (Task 5).
- **The core owns the converters.** `App` gains `converters: OnceLock<Arc<Converters>>` and `probe_mpv: OnceLock<Option<Arc<Mpv>>>`, set once at start-up by **both** binaries through `import::init(&app)` (Task 6c). `server::upload` and `App::import_path` only have `Arc<App>`, and reach them through `App::converters()` / `App::probe_mpv()`. When they are not set, imports fail with "The slide converter (pdfium) is missing. Reinstall RustyPresenter." or `ffi::MISSING`.
- **Imports keep Plan 3's entry points.** `App::import_path(path, purpose: ImportPurpose) -> Result<Imported, ActionError>` and the shared `App::import_staged(tmp, name, sha, purpose)` (Plan 3 Task 1) stay the only entry points. Plan 4 changes the body of `import_staged`, not `server.rs`. Purpose mapping:
  - `ImportPurpose::PowerPoint` → `deck::start` → `Imported::Job(job_id)`.
  - `ImportPurpose::Media` → `.txt` goes to the SongSelect import (Task 8a) → `Imported::Song{..}`; anything else → `import::media::import_media` → `Imported::Media(MediaFile)`.
  - `ImportPurpose::Asset` → `probe_image` check, then `store.publish` → `Imported::Asset(AssetId)`.
- **One extension list.** `import::media::{VIDEO, AUDIO, IMAGE}` and `import::deck::DECK_EXT` (`pptx ppt pps ppsx pot potx pptm odp key pdf`) are the only lists. Plan 3's `files::route`, its `pick` filters and the drop batcher read them; no other list of extensions exists after Task 8.
- **Timeouts:**

  | Converter | Timeout |
  |---|---|
  | LibreOffice | 120 s + 30 s per 50 MB of input, capped at 600 s |
  | PowerPoint | 60 s + 10 s per slide (after `COUNT`), capped at 600 s |
  | pdfium | 5 s per page |
  | mpv probe | 20 s in total |

- **Slide image size, the same for every path:** width 1920, height = `round(1920 / ratio)` rounded to an even number, capped at 2160. If the height is capped, the width is scaled down. 16:9 gives 1920×1080, 4:3 gives 1920×1440.
- **User-facing messages** are plain English and are quoted exactly in the tasks. Show the core's message verbatim. Never show a raw OS error alone.
- **Exit codes** (shared `recovery::exitcode`):

  | Code | Name | Meaning | Supervisor |
  |---|---|---|---|
  | 0 | `OK` | normal quit | exit 0 |
  | 2 | `CONFIG` | `fail()` | exit, no restart |
  | 3 | `ALREADY_RUNNING` | already running, focus sent | exit 0 |
  | 4 | `PORT_TAKEN` | port taken by another program | exit, no restart |
  | 5 | `GAVE_UP` | (supervisor's own exit) three crashes within 60 s | — |
  | 101 | `PANIC` | panic | restart |
  | anything else, or a signal | — | crash | restart, except SIGTERM, SIGHUP, SIGINT and `0xC000013A`, which mean stop |

- **Licensing:**
  - Ship only libmpv built with `-Dgpl=false -Dcplayer=false` against FFmpeg configured **without** `--enable-gpl`, `--enable-version3` or `--enable-nonfree`.
  - The libmpv source tarball goes on the same GitHub Release as the installers. That, plus the swappable DLL, meets LGPL-2.1 §6 (b) and (d).
  - A GPL libmpv (such as the one used for check C1) is for local experiments only.
- **Pins** live in `packaging/pins.env`, the single place to bump them:
  - cargo-packager 0.11.8
  - mpv v0.41.0, plus backport `c5d391adba7bd024954d0df1e0405f5749f4d4ca`
  - FFmpeg 9.0.2
  - libplacebo ≥ 6.338.2
  - pdfium: one `bblanchon/pdfium-binaries` tag, with each archive's sha256
- **Minimum OS:**
  - Windows 10.
  - macOS 12: `minimumSystemVersion = "12.0"`, `MACOSX_DEPLOYMENT_TARGET=12.0`.
  - AppImage: built on ubuntu-22.04 (glibc).
  - `.deb`: Debian 12 / Ubuntu 24.04+. It depends on `libmpv2`. Ubuntu 22.04's `libmpv1` is client API 1.x, which `Mpv::api_version` rejects.
- **Product questions for Jake.** Build with these defaults until he answers. Each sits behind one constant or one dialog tile, so changing it is small.
  1. Accept PDF files as decks? Default: yes; the Slides tab file filter adds `.pdf`.
  2. Add a Settings choice "Import PowerPoint with: Automatic / PowerPoint / LibreOffice"? Default: no setting; Automatic.
  3. Offer one-click "Install LibreOffice" (winget) on Windows? Default: no; show the install hint text with a **Copy** button and **Check again**.
  4. Support Intel Macs? Default: no. The Mac build is **arm64 only** (Apple silicon). An x86_64 or universal job can be added to `libmpv.yml` and `package.yml` later.
  5. Audio output device: there is no device picker (sound follows the system default output). v1 §6 asks for one. Default: no picker; record what happens on unplug/replug in the Task 17 checklist.

## Review Focus

1. **A text or zero-byte file named `.pptx`** (C5 gotcha 2). Expected: `sniff` returns `NotPresentation` and the user sees "This doesn't look like a PowerPoint file." LibreOffice is never started. If sniffing is bypassed, an A4/Letter-ratio PDF with no known slide size is rejected with "LibreOffice couldn't read this presentation." Tests in Task 3 and Task 4.
2. **Password-protected presentation.** Expected: `Encrypted` with the password message. No PowerPoint password prompt ever appears. Tests in Task 3 and Task 6.
3. **Cancel or timeout mid-conversion.** Expected: the running job card has a Cancel (×) button. The whole tree dies (`soffice.com` → `soffice.exe` → `soffice.bin`, or `oosplash` → `soffice.bin`), the work folder is removed, no deck is added, and the job shows the timeout message or disappears on cancel. Tests in Task 2 (tree kill), Task 5, Tasks 6a and 6c.
4. **Two imports at once.** Expected: LibreOffice runs are serialised on the one private profile (warm-up included), and PowerPoint runs are serialised on the one POWERPNT instance. The second job's label shows "Waiting for LibreOffice…" or "Waiting for PowerPoint…" and then goes back to "Importing “…”" and converts. Neither fails, and each deck is added to the service exactly once. Tests in Task 5, Task 6b and Task 6c.
5. **Profile path with spaces or non-ASCII** (`C:\Users\Jake Smith\…`, `é`), and `\\?\` prefixes. Expected: a percent-encoded `file:///` URL, and conversion works. Tests in Task 1 and Task 5.
6. **PowerPoint already open with the user's own presentation, or opened by the user during the import.** Expected: import never closes or kills the user's PowerPoint or any presentation other than ours, even on cancel or timeout. Nothing kills PowerPoint by image name. `.pptm` macros never run. Task 6b string tests, plus a **manual check on Windows with PowerPoint**.
7. **PowerPoint installed but fails on a file.** Expected: LibreOffice is used if present, and the finished job card keeps the note "PowerPoint couldn't open it; used LibreOffice." until dismissed. Test in Task 6c.
8. **Corrupt or unsupported media file.** Expected: the probe fails, the user sees "This file can't be played.", nothing is added to the library, and the live room is never slowed (no hwdec, at most 2 probes). Tests in Task 7 and Task 8.
9. **Old `data.json` with `Converting`/`Unconverted` media** (written by v0.2). Expected: it loads, those entries become `Ready`, and they are re-probed in the background. Test in Task 8.
10. **App crash while a video plays.** Expected: the supervisor relaunches with `--restore` within 2 s. Room and stage reopen on their monitors, the committed frame is back, the video resumes within 5 s of where it was, and the operator sees "RustyPresenter restarted after a problem." No extra start-up backup is made on `--restore`. A LibreOffice left over from the dead app is killed before the next import. Tests in Tasks 5, 9–11, plus a **manual check per OS**.
11. **A saved state that itself crashes the app.** Expected:
    - Attempt 2 restores the frame without video or audio, renames the file to `live-state.crashed.json`, and shows "RustyPresenter restarted after a problem. Video/audio stopped — click it again to replay."
    - A third crash within 60 s shows a native error dialog with the log path and stops.
    - There is never an endless loop.

    Tests in Task 9 and Task 10.
12. **Normal quit, logoff or shutdown.** Expected: no relaunch, and the next normal launch does not restore. Tests in Task 10 and Task 11.
13. **Second launch while running.** Expected: the running operator window comes forward and the new supervisor exits 0 within about 1 s. Test in Task 11.
14. **Port held by a different program.** Expected: one clear message and no restart. Test in Task 11.
15. **`--restore` racing the dying process for the port.** Expected: the bind is retried for up to 1.5 s before deciding "already running". Test in Task 11.
16. **`live-state.json` rename blocked** (antivirus, indexer, OneDrive). Expected: retried 10 × 30 ms, then logged. The writer never panics and never blocks the UI. Test in Task 9.
17. **Panic in a critical background thread** (mpv events, live-state writer). Expected: exit 101 and a restart. A panic inside a tokio HTTP task is only logged. Test in Task 10.
18. **A GPL libmpv or FFmpeg slipping into a package.** Expected: the libmpv workflow fails on any of its LGPL checks. Test in Task 16.
19. **Installed app looks in the wrong folder for libmpv or pdfium.** Expected: the resolver finds them for NSIS, `.app`, `.deb` and AppImage layouts. When a file really is missing, About says so and video items show the missing-player message. Tests in Task 1, plus **manual install checks** in Task 15.
20. **A Keynote package (a folder named `.key`)** handed over by the picker or a drop. Expected: "Save the Keynote file as a single file and import again." and no I/O error text. Test in Task 6c.
21. **pdfium hangs on one page.** Expected: that import fails after 5 s, the renderer is marked dead, and later PDF-route imports fail at once with "The slide converter stopped responding. Restart RustyPresenter and try again." instead of hanging. Test in Task 4.
22. **SongSelect file for a song already in the library** (same CCLI #). Expected: a Confirm "“T” is already in the library." with **Update existing** / **Keep both**. Update keeps the song id, so service items still point at it. Tests in Task 8a.
23. **LibreOffice installed while the app runs.** Expected: the next import finds it without a restart, and **Check again** in Add → Slides finds it. Tests in Tasks 1 and 6c.

---

### Task 1: Resource folder and tool discovery (`platform::resources`, `platform::tools`)

**Files:**
- Create: `src/platform/resources.rs`, `src/platform/tools.rs`
- Modify: `src/platform/mod.rs`, `src/video/ffi.rs` (`Mpv::candidates` uses `resources`), `Cargo.toml` (`url = "2.5"`; `windows-sys` feature `Win32_System_Registry`)

**Interfaces:**
- `resources.rs`:
  - `pub fn resource_dir() -> PathBuf`.
  - `pub fn resource_dir_from(exe: &Path, var: &dyn Fn(&str) -> Option<OsString>) -> PathBuf` (pure). It tries, in order:
    1. `RUSTYPRESENTER_RESOURCES`
    2. on macOS, an exe under `*.app/Contents/MacOS/` → `../Resources`
    3. with `APPDIR` set → `$APPDIR/usr/lib/rustypresenter`
    4. an exe in `/usr/bin` → `/usr/lib/rustypresenter`
    5. otherwise the exe's directory
  - `pub fn frameworks_dir_from(exe: &Path) -> Option<PathBuf>` (`.app/Contents/Frameworks`).
  - `pub fn native_candidates(names: &[&str], env_key: &str, exe: &Path, data_dir: &Path, deps_sub: &str, var: &dyn Fn(&str) -> Option<OsString>) -> Vec<PathBuf>`. It yields, in order:
    1. the env path
    2. the resource dir
    3. the Frameworks dir
    4. the exe dir
    5. `<data>/deps/<deps_sub>/`
    6. the bare system names
- `tools.rs`:
  - `pub enum Os { Windows, MacOs, Linux }`, `pub const OS: Os`.
  - `pub struct ToolEnv { pub os: Os, pub var: Box<dyn Fn(&str) -> Option<OsString>>, pub home: Option<PathBuf>, pub path_dirs: Vec<PathBuf>, pub registry: Vec<PathBuf> }`.
  - `pub fn soffice_candidates(env: &ToolEnv) -> Vec<PathBuf>` (pure). It lists, in order:
    1. `RP_SOFFICE`
    2. Windows: registry `HKLM`/`HKLM\WOW6432Node`/`HKCU` `SOFTWARE\LibreOffice\UNO\InstallPath` + `soffice.com`; `%ProgramFiles%` and `%ProgramFiles(x86)%` `\LibreOffice\program\soffice.com`; Scoop `%USERPROFILE%\scoop\apps\libreoffice\current\LibreOffice\program\soffice.com`; PATH. `soffice.com` comes before `soffice.exe`.
    3. macOS: `/Applications/LibreOffice.app/Contents/MacOS/soffice`, `~/Applications/…`, PATH.
    4. Linux: PATH `soffice`, `libreoffice`; `/usr/lib/libreoffice/program/soffice`, `/usr/lib64/…`; `/opt/libreoffice*/program/soffice`; `/snap/bin/libreoffice`.
  - `pub struct Soffice { pub path: PathBuf, pub snap: bool }`.
  - `pub fn find_soffice() -> Option<Soffice>`: first file that exists. Flatpak is not supported; snap is flagged. A found path is cached; **a negative result is never cached**, so every call while nothing is found searches again (a volunteer may install LibreOffice while the app runs).
  - `pub fn install_hint(os: Os) -> &'static str`:
    - Windows: "Install LibreOffice (free) from libreoffice.org, or run: winget install TheDocumentFoundation.LibreOffice"
    - macOS: "Install LibreOffice (free) from libreoffice.org, or run: brew install --cask libreoffice"
    - Linux: "Install LibreOffice Impress, for example: sudo apt install libreoffice-impress"
  - `pub fn powerpoint_installed() -> bool`: on Windows, registry key `HKEY_CLASSES_ROOT\PowerPoint.Application\CLSID` exists; elsewhere `false`.
  - `pub fn pdfium_candidates(data_dir: &Path) -> Vec<PathBuf>`: `native_candidates(&["pdfium.dll" | "libpdfium.dylib" | "libpdfium.so"], "RP_PDFIUM", …, "pdfium")`.
  - `pub fn file_url(path: &Path, dir: bool) -> Result<String, String>`: strips `\\?\` (and `\\?\UNC\` → `\\`), then `url::Url::from_directory_path` / `from_file_path`.

- [ ] **Step 1: Write the failing tests:**
  - `resources`:
    - `env_override_wins`
    - `mac_bundle_uses_contents_resources`
    - `appimage_uses_appdir_usr_lib`
    - `deb_install_uses_usr_lib_name`
    - `nsis_and_dev_use_exe_dir`
    - `native_candidates_order` (env, resource, frameworks, exe, data/deps, system)
  - `tools`:
    - `windows_candidates_prefer_registry_then_program_files_soffice_com`
    - `mac_candidates_include_user_applications`
    - `linux_candidates_cover_usr_lib_opt_and_snap`
    - `rp_soffice_override_first`
    - `snap_path_is_flagged`
    - `file_url_escapes_spaces_and_accents` (`C:\Users\Jake Smith\é` → `file:///C:/Users/Jake%20Smith/%C3%A9/`)
    - `file_url_strips_verbatim_prefix`
    - `install_hint_per_os`
    - `negative_soffice_result_is_not_cached` (a `find_soffice_in(&ToolEnv)` variant over a temp dir: absent → `None`; create the file → found on the next call)
  - `video::ffi`: the existing `candidates_*` tests are updated to the new order.
- [ ] **Step 2:** `cargo test platform::` → FAIL.
- [ ] **Step 3:** Implement. Windows registry reads use `RegGetValueW`. `find_soffice` keeps a found path in a `Mutex<Option<Soffice>>`, re-checks when the cached path no longer exists, and never stores `None`. Also add `pub fn install_command(os: Os) -> Option<&'static str>` (`winget install TheDocumentFoundation.LibreOffice`, `brew install --cask libreoffice`, `sudo apt install libreoffice-impress`) for the Copy button in Task 6c.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(platform): resource folder and tool discovery for LibreOffice, PowerPoint, pdfium`.

### Task 2: Process trees that die together (`platform::proc`)

**Files:**
- Create: `src/platform/proc.rs`, `tests/proc_tree.rs`
- Modify: `src/platform/mod.rs`, `Cargo.toml` (`tokio-util = "0.7"`; `windows-sys` features `Win32_System_JobObjects`, `Win32_System_Threading`, `Win32_Security`; `[target.'cfg(unix)'.dependencies] libc = "0.2"`)

**Interfaces:**
- `pub struct Tree { child: tokio::process::Child, #[cfg(windows)] job: OwnedJob, #[cfg(unix)] pgid: i32 }`.
- `pub fn spawn_tree(cmd: tokio::process::Command) -> std::io::Result<Tree>`:
  - Windows: adds `CREATE_NO_WINDOW`, creates the job, and assigns the child right after spawn. Children started before the assignment are a known microsecond race; LibreOffice and PowerShell take milliseconds to fork.
  - Unix: `process_group(0)`.
- `pub enum Stop { Timeout, Cancelled, Io(String) }`.
- `impl Tree`:
  - `pub async fn wait(&mut self, timeout: Duration, cancel: &CancellationToken) -> Result<std::process::ExitStatus, Stop>`: `select!` over `child.wait()`, `sleep(timeout)` and `cancel.cancelled()`, and kills the tree on Timeout or Cancelled.
  - `pub fn kill(&mut self)`: `TerminateJobObject` / `kill(-pgid, SIGKILL)`.
  - `pub fn stdout(&mut self) -> Option<tokio::process::ChildStdout>`.
- `Drop` kills the tree.

- `pub(crate) fn creation_flags() -> u32` (Windows), used by `spawn_tree`.
- **Self-invocation helper (shared by Tasks 2, 5, 6c and 10).** A test re-runs its own test binary as a helper process: `Command::new(std::env::current_exe()?)` with args `["--exact", "<helper_test_name>", "--nocapture"]` and an env var that selects the role. The helper is an ordinary `#[test] fn <helper_test_name>()` that returns at once unless its env var is set, so it passes as a no-op in normal runs. Here: `#[test] fn proc_helper()`, active when `RP_PROC_HELPER=parent|grandchild`.

- [ ] **Step 1: Write the failing tests** in `tests/proc_tree.rs`, using the helper above. The parent starts a grandchild (the same binary, `RP_PROC_HELPER=grandchild`) that writes its pid and then sleeps 60 s.
  - `drop_kills_grandchild`: after drop, the grandchild pid is gone within 2 s.
  - `timeout_kills_tree_and_reports_timeout`
  - `cancel_kills_tree_and_reports_cancelled`
  - `normal_exit_returns_status`
  - In `src/platform/proc.rs` (a unit test, because an integration test cannot call a `pub(crate)` fn): `no_console_window_flag_set_on_windows` asserts `creation_flags() & CREATE_NO_WINDOW != 0`.
- [ ] **Step 2:** `cargo test --test proc_tree` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes on Windows. CI covers macOS and Linux.
- [ ] **Step 5:** Commit `feat(platform): spawn converter process trees that are killed together`.

### Task 3: What kind of file is this? (`import::sniff`)

**Files:**
- Create: `src/import/mod.rs`, `src/import/sniff.rs`
- Modify: `src/lib.rs`, `Cargo.toml` (`zip`, `cfb = "0.10"`, `quick-xml = "0.37"`)

**Interfaces:**
- `import/mod.rs`:
  - `pub struct WorkDir(PathBuf)` with `WorkDir::new(tmp_root: &Path) -> io::Result<WorkDir>` (`ppt-<uuid>`), `path()`, and a `Drop` that does `remove_dir_all`.
  - `pub enum Stage { Prepare, Convert, Render, Publish }` and `pub fn percent(stage: Stage, frac: f32) -> u8`. The ranges are 0–5, 5–70, 70–95, 95–100.
- `sniff.rs`:
  - `#[derive(Debug, PartialEq)] pub enum Kind`:
    - `Pdf`
    - `Ooxml { variant: Variant, slide_size: Option<(u64, u64)>, slides: Option<usize>, hidden: usize }`
    - `Ppt`
    - `Odp`
    - `Keynote`
    - `Encrypted`
    - `NotPresentation`
  - `pub enum Variant { Pptx, Ppsx, Potx, Pptm }`.
  - `pub fn sniff(path: &Path) -> std::io::Result<Kind>` (callers check `path.is_dir()` first; see the Keynote package rule in Task 6c) and `pub fn sniff_reader<R: Read + Seek>(r: R) -> std::io::Result<Kind>`.
  - `pub fn staged_ext(kind: &Kind) -> Option<&'static str>`: `pdf`/`pptx`/`ppsx`/`potx`/`pptm`/`ppt`/`odp`/`key`, and `None` for the rejected kinds.
  - `pub const ENCRYPTED: &str = "This presentation has a password. Open it in PowerPoint, remove the password (File → Info → Protect), save, and import again.";`
  - `pub const NOT_PRESENTATION: &str = "This doesn't look like a PowerPoint file.";`
- Rules:
  - `%PDF-` → `Pdf`.
  - `PK\x03\x04`:
    - `mimetype` entry = `application/vnd.oasis.opendocument.presentation` → `Odp`, or `Encrypted` when `META-INF/manifest.xml` has a `manifest:encryption-data` element.
    - `[Content_Types].xml` Override for `/ppt/presentation.xml` → `Ooxml` with variant from its content type.
    - `Index/Document.iwa` plus an entry starting `Index/Slide` or `Index/MasterSlide` → `Keynote`. Pages and Numbers have `Index/Document.iwa` too, and LibreOffice would convert them.
    - Anything else → `NotPresentation`.
  - `D0CF11E0A1B11AE1`:
    - `EncryptionInfo` + `EncryptedPackage` streams → `Encrypted`.
    - `PowerPoint Document` stream → `Ppt`, or `Encrypted` when there is an `EncryptedSummary` stream or the `Current User` header token (bytes 12..16) is `0xF3D1C4DF` (MS-PPT). Word and Excel files encrypted the same way as OOXML also give `Encrypted`; they can't be told apart without the password.
    - Anything else → `NotPresentation`.
  - Anything else, including empty files and `PK\x05\x06`, → `NotPresentation`.
  - For OOXML, read `ppt/presentation.xml` `<p:sldSz cx cy>` and count the `<p:sldIdLst>` entries. A slide is hidden when its root is `<p:sld show="0">`. Part names are matched without case. Each slide is read once, only its first 64 KB, and all slides together at most 50 MB; past that the hidden count is 0.
  - Refuse to parse an entry over 50 MB uncompressed (zip-bomb guard). Fall back to `slide_size: None`.

- [ ] **Step 1: Write the failing tests.** Build the fixtures in-test with the `zip` and `cfb` writers; there are no binary files in git.
  - `pdf_magic`
  - `minimal_pptx_with_16x9_size_and_3_slides`
  - `ppsx_and_pptm_variants`
  - `hidden_slides_counted`
  - `odp_mimetype`
  - `keynote_index`
  - `ppt_cfb`
  - `encrypted_cfb`
  - `docx_is_not_presentation`
  - `xlsx_is_not_presentation`
  - `plain_text_named_pptx_is_not_presentation`
  - `empty_file_is_not_presentation`
  - `empty_zip_is_not_presentation`
  - `oversized_presentation_xml_gives_no_size`
  - `percent_ranges`
  - `workdir_removed_on_drop`
- [ ] **Step 2:** `cargo test import::sniff` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(import): sniff presentations by content, not by name`.

### Task 4: PDF pages to slide images (`import::pdf`)

**Files:**
- Create: `src/import/pdf.rs`
- Modify: `src/import/mod.rs`, `Cargo.toml` (`pdfium-render = "0.8.37"`)

**Interfaces:**
- `pub fn output_size(ratio: f32) -> (u32, u32)`: the global rule.
- `pub fn check_pages(sizes: &[(f32, f32)], expected_ratio: Option<f32>) -> Result<f32, String>`. It returns the ratio, and errors on:
  - no pages: "This presentation has no slides."
  - pages differing by more than 1%: "This presentation mixes slide sizes, which RustyPresenter can't show."
  - ratio more than 1% from the expected ratio: "LibreOffice couldn't read this presentation."
  - with no expected ratio: portrait pages, or A4/Letter-like ratios (0.707, 0.773, 1.414, 1.294 ± 1%): "LibreOffice couldn't read this presentation."
- `pub struct PdfRenderer { tx: std::sync::mpsc::Sender<Job> }`:
  - `PdfRenderer::start(data_dir: &Path) -> Result<PdfRenderer, String>` spawns the thread `pdfium` and binds with `Pdfium::bind_to_library` over `tools::pdfium_candidates`, then `bind_to_system_library()`. Failure message: "The slide converter (pdfium) is missing. Reinstall RustyPresenter."
  - `pub async fn render(&self, pdf: PathBuf, out_dir: PathBuf, expected_ratio: Option<f32>, progress: Box<dyn Fn(usize, usize) + Send>, cancel: CancellationToken) -> Result<Vec<PathBuf>, String>`.
  - Each page renders with `PdfRenderConfig::new().set_target_width(w).set_maximum_height(h).render_form_data(true)`, white background and no LCD text. Pages are written as `slide-001.png`… with `PngEncoder::new_with_quality(CompressionType::Fast, FilterType::Adaptive)`. Each page has a 5 s timeout, and cancel is checked between pages.
  - **A page that times out.** A pdfium call cannot be interrupted, so after a timeout the `pdfium` thread may stay stuck for good. The renderer then sets `dead: AtomicBool`; that import fails with "The slide converter stopped responding. Restart RustyPresenter and try again.", and every later `render` returns the same message at once instead of queueing behind the stuck thread. Nothing tries to recover the thread; a restart fixes it.
  - For tests, `PdfRenderer::with_backend(f: Box<dyn FnMut(&Path, &Path, …) -> Result<…> + Send>)` runs a fake page renderer on the same thread and timeout logic.
- `pub fn check_pdf(path: &Path) -> Result<(), String>`: the file exists and starts with `%PDF-`.

- [ ] **Step 1: Write the failing tests:**
  - Not gated:
    - `output_size_16x9_is_1920x1080`
    - `output_size_4x3_is_1920x1440`
    - `output_size_tall_caps_height_and_scales_width`
    - `output_size_heights_are_even`
    - `mixed_page_sizes_rejected`
    - `a4_without_expected_ratio_rejected`
    - `portrait_rejected`
    - `ratio_must_match_expected`
    - `check_pdf_rejects_non_pdf`
    - `stuck_page_marks_renderer_dead` (a `with_backend` fake that sleeps 60 s on page 1, with the per-page timeout shortened to 200 ms through a test constructor: the first render fails with the "stopped responding" message, and a second render returns the same message in under 100 ms)
  - Gated on `RP_PDFIUM` (path to the library), skipped otherwise: `renders_three_coloured_pages`. The test writes a minimal 3-page PDF in-test (a hand-written PDF string with three filled rectangles #ff0000/#00ff00/#0000ff) and checks 3 PNGs of 1920×1080 using the C5 verify rule (dominant colour of a 160×160 corner block).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes; with `RP_PDFIUM=…\pdfium.dll` the gated test passes on Windows.
- [ ] **Step 5:** Commit `feat(import): render PDF pages to slide images with pdfium`.

### Task 5: LibreOffice to PDF (`import::libreoffice`)

**Files:**
- Create: `src/import/libreoffice.rs`

**Interfaces:**
- `pub struct LibreOffice { soffice: Soffice, profile: PathBuf, gate: tokio::sync::Semaphore /* 1 permit */, warmed: AtomicBool }`.
- `impl LibreOffice`:
  - `pub fn new(soffice: Soffice, data_dir: &Path) -> Arc<Self>`. The profile is `<data>/deps/libreoffice-profile/`.
  - `pub fn args(&self, input: &Path, out_dir: &Path) -> Result<Vec<OsString>, String>`. The arguments are:
    - `--headless --invisible --norestore --nologo --nodefault --nolockcheck --nofirststartwizard`
    - `-env:UserInstallation=<file_url(profile)>`
    - one argv element `pdf:impress_pdf_Export:{"ExportHiddenSlides":{"type":"boolean","value":"false"},"ExportNotesPages":{"type":"boolean","value":"false"},"ReduceImageResolution":{"type":"boolean","value":"false"},"UseLosslessCompression":{"type":"boolean","value":"true"},"Quality":{"type":"long","value":"100"}}`
    - `--outdir <out_dir> <input>`, with `\\?\` stripped from both paths
  - `pub fn timeout_for(bytes: u64) -> Duration`.
  - `pub async fn to_pdf(&self, input: &Path, work: &Path, waiting: &(dyn Fn(bool) + Sync), cancel: &CancellationToken) -> Result<PathBuf, String>`:
    - If the permit is not free at once, it calls `waiting(true)` before blocking on the gate and `waiting(false)` once it holds the permit. `deck::start` passes a closure that calls `app.job_label(id, "Waiting for LibreOffice…")` and then restores "Importing “{display}”" (Task 6a). Cancel while waiting returns `Cancelled` without starting LibreOffice.
    - With the permit held, it first runs `kill_leftover(&profile_parent)` (below), then removes a stale `<profile>/.lock`.
    - The environment drops `PYTHONHOME`/`PYTHONPATH`; on Linux it sets `SAL_USE_VCLPLUGIN=svp`. stdout/stderr go to `work/soffice.log`.
    - On Unix, right after spawn it writes the child's pgid and the current time to `<data>/deps/libreoffice.pgid`, and removes the file when the tree has exited or been killed.
    - Success is judged by `check_pdf(work/pdf/input.pdf)`, not by the exit code.
    - If the log contains "user installation could not be completed", it deletes the profile and retries once (still holding the permit).
    - When `soffice.snap` is true it returns the Snap message at once and never starts LibreOffice. `route` (Task 6c) already turns a Snap install into that message, so this is a second guard.
  - `pub async fn warm_up(self: Arc<Self>)`: `--terminate_after_init`, run once in the background after start-up. It **takes the same gate** for its whole run, so an import started during warm-up shows "Waiting for LibreOffice…" and never shares or deletes the profile under it.
  - `pub fn kill_leftover(deps_dir: &Path)` (Unix; no-op on Windows, where the Job Object already died with the old app): reads `libreoffice.pgid`; if that group still exists and its leader is a LibreOffice process (`/proc/<pid>/cmdline` on Linux, `ps -o comm= -p <pid>` on macOS contains `soffice` or `oosplash`), it sends `kill(-pgid, SIGKILL)`, waits up to 2 s for it to go, and removes the file. It is called by `to_pdf` and once at start-up by `import::init`, before `warm_up`. `PR_SET_PDEATHSIG` is **not** used: it only reaches the direct child (the `soffice` shell script or `oosplash`), not `soffice.bin`.
  - Start-up keeps `store.clean_tmp()` on every start, `--restore` included, so a killed import's work folder is removed.
- Messages:
  - "LibreOffice took too long and was stopped. Try again; the first import after starting the computer is slower."
  - "LibreOffice couldn't read this presentation."
  - For snap: "LibreOffice from Snap can't read RustyPresenter's files. Install LibreOffice from your distribution instead (sudo apt install libreoffice-impress)."
- **Fakes in tests.** Tests build `LibreOffice::new(Soffice { path: current_exe(), snap: false }, tmp)` directly, never through `RP_SOFFICE` or `find_soffice` (which caches, and `set_var` is `unsafe` in edition 2024 and races between tests). The fake soffice is the test binary itself through the Task 2 self-invocation helper: `#[test] fn fake_soffice()`, active when `RP_FAKE_SOFFICE=sleep:<ms>|pdf:<fixture>|fail|hang` is set on the **child's** `Command` env (not the test process). To pass that env and the helper args, `LibreOffice` has a `#[cfg(test)] extra_env`/`prefix_args` hook. A `.cmd` script is not used: cmd.exe mangles the JSON filter argument.

- [ ] **Step 1: Write the failing tests:**
  - `args_use_private_profile_url_and_pdf_filter_as_one_argument`
  - `args_strip_verbatim_prefix`
  - `timeout_scales_with_size_and_caps_at_600s`
  - `stale_lock_removed_before_run`
  - `second_conversion_waits_for_first`: the fake soffice (`sleep:1000` then a PDF); it asserts the start times don't overlap and the second call's `waiting` saw `true` then `false`.
  - `import_waits_behind_warm_up`: start `warm_up` with a fake that sleeps 1 s, then `to_pdf`; `waiting(true)` is called and the conversion starts after warm-up ends.
  - `missing_pdf_is_an_error_even_with_exit_0`
  - `snap_soffice_returns_snap_message_without_spawning` (the fake would write a marker file if started; none appears)
  - Unix: `leftover_group_is_killed_before_lock_removed` (spawn a fake `hang` soffice in its own group, write its pgid file, drop the handle without killing, then `kill_leftover`: the process is gone and the file removed); `pgid_file_with_unrelated_process_is_ignored` (the pgid of the test's own `sleep` child, which is not soffice, is not killed)
  - Gated `RP_TEST_SOFFICE=1`: `converts_sample_pptx` (`experiments/media/sample.pptx` → PDF with 3 pages of ratio 1.7777).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test` passes; the gated test passes on Windows with LibreOffice 26.8.
- [ ] **Step 5:** Commit `feat(import): LibreOffice conversion with a private profile, one at a time`.

### Task 6a: Job cancel, labels, notes and results (`App` jobs, Plan 3 job card)

**Files:**
- Modify:
  - `src/app.rs`: job fields, cancel, `job/cancel` action
  - Plan 3's `src/ui/operator/overlays.rs` and `ui/operator/overlays.slint` (job card), and its test file

**Interfaces:**
- `Job` (`app.rs`, today `#[derive(Serialize)] struct Job { id, label, percent, error }`) gains, each with `#[serde(skip_serializing_if = …)]` so the web operator's JSON does not change while the Tauri app still ships:
  - `note: Option<String>` — shown like `error`, but not red.
  - `cancellable: bool` (`skip_serializing_if = "std::ops::Not::not"`).
  - `result: Option<Uuid>` — the id of what the job made (the new deck).
- `app.rs`:
  - `cancels: Mutex<HashMap<Uuid, CancellationToken>>`.
  - `pub(crate) fn job_start_cancellable(&self, label: &str) -> (Uuid, CancellationToken)` sets `cancellable: true`.
  - `pub fn job_cancel(&self, id: Uuid)`: cancels the token. The pipeline then ends the job with `job_end_cancelled(id)`, which removes it with no error and no note.
  - `pub(crate) fn job_label(&self, id: Uuid, label: &str)`: changes the label and broadcasts `Update::Data`. Used for "Waiting for LibreOffice…" / "Waiting for PowerPoint…" and to restore "Importing “…”".
  - `pub(crate) fn job_note(&self, id: Uuid, note: String)`: sets the note (the last one wins, except that a fallback note and a count note are joined with a space).
  - `pub(crate) fn job_result(&self, id: Uuid, made: Uuid)`.
  - `job_end(id, None)` changes: a job **with a note** stays in the list with `percent = 100`, `cancellable = false`, and is removed only by `job/dismiss {id}`. A job without a note is removed as today. The cancel token is dropped on every end.
  - Action `"job/cancel" {id}` → `{}`. Unknown ids are ignored.
- Plan 3 job card (`JobRow { id, label, percent, error }` in `overlays.slint`/`overlays.rs`) gains `note: string`, `cancellable: bool`, `done: bool`:
  - A running cancellable job shows a Cancel (×) button (element id `job-cancel-<n>`, tooltip "Cancel") → `job/cancel {id}`.
  - A finished job with a note shows the note in `Theme.muted` (not red) and a close (×) button → `job/dismiss {id}`, the same as an error card.
  - Client jobs ("Uploading `<name>`") are not cancellable.

- [ ] **Step 1: Write the failing tests:**
  - `app.rs`: `note_survives_job_end_until_dismissed` (`job_start`, `job_note`, `job_end(id, None)` → the job is still in `jobs` with the note and percent 100; `job/dismiss` removes it), `job_without_note_is_removed_on_end`, `cancel_token_fires_and_job_disappears`, `job_cancel_action_is_wired`, `job_label_broadcasts`, `job_json_unchanged_without_new_fields` (`serde_json::to_value(job)` has exactly `id, label, percent, error`).
  - Testing backend (Plan 3's overlay test file): `running_cancellable_job_shows_cancel_button` (click `job-cancel-0` → the token is cancelled), `finished_job_note_shows_and_dismisses`, `client_upload_job_has_no_cancel`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(app): cancellable jobs with notes, labels and results`.

### Task 6b: PowerPoint export (`import::powerpoint`)

**Files:**
- Create: `src/import/powerpoint.rs`
- Modify: `src/media.rs` (`PPT_SCRIPT` and `export_powerpoint` move out; Task 6c deletes them once the server path uses `deck::start`)

**Interfaces:**
- `pub const SCRIPT: &str`. It changes `PPT_SCRIPT` as follows:
  - sets `$app.AutomationSecurity = 3` (msoAutomationSecurityForceDisable) **before** `Open`, so a `.pptm` never runs macros
  - opens read-only, windowless, and with `"$Path::x::"` as the password, so a password-protected file fails instead of prompting
  - prints `STARTED_BY_US` when `$hadOpen -eq 0`
  - prints `COUNT <n>` and `SIZE <w> <h>` (points)
  - computes the output size with the global rule
  - prints `SLIDE <i> <n>` after each `Slide.Export(file,'PNG',w,h)`
  - skips hidden slides as today
  - closes only its own presentation (`$pres.Close()`), then calls `$app.Quit()` only when `STARTED_BY_US` was printed **and** `$app.Presentations.Count -eq 0` at that moment. A presentation the user opened during the import lives in the same single PowerPoint instance, so this re-check is what keeps it alive.
- `pub const CLEANUP_SCRIPT: &str`, run after a timeout or cancel (the export PowerShell tree has already been killed):
  - attaches with `[Runtime.InteropServices.Marshal]::GetActiveObject('PowerPoint.Application')`; if none is running, exits 0
  - closes only presentations whose `FullName` equals our staged `input.*` path (passed as an argument)
  - calls `Quit()` only when the export had printed `STARTED_BY_US` (passed as `-StartedByUs`) and `Presentations.Count -eq 0`
  - has its own 15 s timeout through `spawn_tree`
- Nothing ever runs `taskkill /IM POWERPNT.EXE`. If the cleanup script fails or times out, the hidden PowerPoint is left running and the failure is logged; it never risks the user's work.
- `#[derive(Debug, PartialEq)] pub enum Line { Count(usize), Size(f32, f32), Slide(usize, usize), StartedByUs }` and `pub fn parse_line(s: &str) -> Option<Line>`.
- `pub struct PowerPoint { gate: tokio::sync::Semaphore /* 1 permit */ }`, held in `Converters`. Only one import uses PowerPoint at a time.
- `pub async fn export(&self, input: &Path, out: &Path, waiting: &(dyn Fn(bool) + Sync), progress: &dyn Fn(usize, usize), cancel: &CancellationToken) -> Result<Vec<PathBuf>, String>` (`#[cfg(windows)]`):
  - waits on the gate like `LibreOffice::to_pdf` (`waiting(true)` → label "Waiting for PowerPoint…", `waiting(false)` → back to "Importing “…”"); cancel while waiting returns at once
  - runs PowerShell through `spawn_tree` and reads stdout lines; after `COUNT` arrives, the timeout becomes 60 s + 10 s per slide
  - on timeout or cancel it kills the PowerShell tree and then runs `CLEANUP_SCRIPT`, still holding the permit
  - exported PNG sizes must equal `output_size(w / h)`

- [ ] **Step 1: Write the failing tests:**
  - `parse_line_reads_count_size_slide_and_marker`
  - `script_never_quits_powerpoint_when_presentations_were_open`: string checks that `Quit()` appears only inside a condition containing both `$hadOpen -eq 0` (or the started-by-us flag) and `Presentations.Count -eq 0`, and that `AutomationSecurity = 3` comes before `.Open(`
  - `cleanup_script_closes_only_our_presentation`: string checks that it uses `GetActiveObject`, compares `FullName` to the argument, guards `Quit()` with the started flag and `Presentations.Count -eq 0`, and that neither script contains `taskkill` or `Stop-Process`
  - Gated `RP_TEST_PPT=1` (Windows with PowerPoint):
    - `powerpoint_exports_sample_with_progress` (3 PNGs at 1920×1080 and 3 `Slide` lines)
    - `two_exports_run_one_at_a_time` (two `export` calls at once: start times don't overlap, the second saw `waiting(true)`, both succeed, and PowerPoint is not left running when it was not running before)
    - `pptm_macro_does_not_run` (a `.pptm` fixture built in-test whose `Auto_Open` macro would write a marker file; no marker appears). If building a signed-off macro fixture in-test is not practical, this becomes a manual check in Step 3.
- [ ] **Step 2:** FAIL → **Step 3:** implement. **Manual check on Windows with PowerPoint**, recorded in `2026-native-checks.md`:
  - Open a presentation of your own in PowerPoint, import a deck, then cancel one import mid-way. Your PowerPoint stays open and untouched.
  - Start an import with PowerPoint closed, and **open one of your own presentations while it is exporting**. When the import ends (and again after cancelling a second one), your presentation stays open.
  - Import a password-protected `.pptx`. You get the password message and no prompt.
  - Import a `.pptm` with an `Auto_Open` macro: no macro runs and no security bar appears.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(import): PowerPoint export one at a time that never touches the user's presentations`.

### Task 6c: The deck pipeline (`import::deck`, `import::init`, `import_staged`)

**Files:**
- Create: `src/import/deck.rs`
- Modify:
  - `src/import/mod.rs`: `pub fn init(app: &Arc<App>)`
  - `src/app.rs`: `converters`/`probe_mpv` `OnceLock`s and accessors; `import_staged` body for `ImportPurpose::PowerPoint`
  - `src/media.rs`: delete `PPT_SCRIPT` and `export_powerpoint`
  - `src/main.rs` (Tauri binary) and `src/bin/native.rs`: call `import::init(&app)` after `App::new`
  - Plan 3's `src/ui/operator/files.rs` (`route`, `pick` filters, `watch_deck_job`), `ui/operator/overlays.slint` (drop hint), `ui/dialogs/add-media.slint` + `src/ui/dialogs/add_media.rs` (Slides tab)

**Interfaces:**
- `deck.rs`:
  - `pub const DECK_EXT: &[&str] = &["pptx", "ppt", "pps", "ppsx", "pot", "potx", "pptm", "odp", "key", "pdf"];`
  - `pub const KEYNOTE_PACKAGE: &str = "Save the Keynote file as a single file and import again.";`
  - `pub enum Lo { None, Snap, Ok }` and `pub enum Route { Pdf, PowerPoint { fallback: bool }, LibreOffice, Missing }`.
  - `pub fn route(kind: &Kind, ppt: bool, lo: Lo) -> Result<Route, String>`:
    - `Encrypted` → `ENCRYPTED`; `NotPresentation` → `NOT_PRESENTATION`.
    - `Pdf` → `Pdf`.
    - `Keynote` → `LibreOffice`; with `Lo::Snap` → the Snap message; with `Lo::None` → `Missing`.
    - `Ooxml`/`Ppt`/`Odp` on Windows with PowerPoint → `PowerPoint { fallback: lo == Lo::Ok }`.
    - Otherwise `LibreOffice` with `Lo::Ok`, the Snap message with `Lo::Snap`, and `Missing` with `Lo::None`.
  - `pub struct Converters { pub libreoffice: Mutex<Option<Arc<LibreOffice>>>, pub pdf: Option<Arc<PdfRenderer>>, pub powerpoint: Option<PowerPoint>, data_dir: PathBuf }`:
    - `pdf: None` (pdfium failed to bind) makes the `Pdf`/`LibreOffice` routes fail with "The slide converter (pdfium) is missing. Reinstall RustyPresenter." This keeps `Converters` buildable in ordinary tests.
    - `pub fn libreoffice(&self) -> Option<Arc<LibreOffice>>`: when the field is `None`, it calls its finder again (never cached while missing, Task 1) and stores a new `LibreOffice` if one is found. Called at the start of every import and by **Check again**. The finder is a field `find: Box<dyn Fn() -> Option<Soffice> + Send + Sync>`, `find_soffice` in `import::init`; tests pass a closure over a temp dir (`find_soffice_in(&ToolEnv)`), never the real machine, so a PC with LibreOffice installed cannot change the result.
    - `pub fn status(&self) -> ConverterStatus { powerpoint: bool, libreoffice: Lo, pdfium: bool }` for the Slides tab and About.
  - `pub fn start(app: Arc<App>, file: PathBuf, display: String) -> Uuid` (job id). It reads `app.converters()`, calls `job_start_cancellable("Importing “{display}”")` and spawns the pipeline:
    1. if `file.is_dir()` (a Keynote package from the picker or a drop): end the job with `KEYNOTE_PACKAGE`
    2. sniff
    3. `WorkDir`, copy to `input.<staged_ext>`
    4. convert: Route → PNGs, with progress via `percent`. Both `to_pdf` and `export` get a `waiting` closure that calls `app.job_label(id, "Waiting for LibreOffice…")` / `"Waiting for PowerPoint…"` and then `app.job_label(id, "Importing “{display}”")`.
    5. `store.import_file` for each PNG
    6. one `App::edit` that pushes a `Deck`; `app.job_result(id, deck_id)`
    7. `job_end` (a job with a note stays as a card until dismissed, Task 6a)
  - Notes (through `job_note`):
    - LibreOffice fallback after a PowerPoint failure: "PowerPoint couldn't open it; used LibreOffice."
    - Any import done with LibreOffice: "Imported with LibreOffice — check the slides."
    - When the PDF page count differs from the expected visible slide count: "N slides were expected but M were imported."
  - `Missing` error: "To import presentations, install LibreOffice. " + `install_hint(OS)`.
- `import::init(app: &Arc<App>)`: builds `Converters` (`find_soffice`, `PdfRenderer::start` → `Option`, `powerpoint_installed()`), loads the probe `Mpv` (`Mpv::load()`, `None` on failure), sets both `OnceLock`s, runs `kill_leftover` and then `warm_up` in the background. **Both binaries call it**, so the Tauri web operator keeps importing PowerPoint (and, after Task 8, media) until Task 12. In the Tauri binary a missing libmpv makes media uploads fail with `ffi::MISSING`; that is acceptable because development machines have libmpv and Tauri is removed in Task 12.
- `App`: `pub fn converters(&self) -> Option<Arc<Converters>>`, `pub fn probe_mpv(&self) -> Option<Arc<Mpv>>`.
- `import_staged(tmp, name, sha, ImportPurpose::PowerPoint)` calls `deck::start` and returns `Imported::Job(job_id)`. `server::upload` is unchanged (it already calls `import_staged`), and its JSON reply `{job}` is unchanged.
- Plan 3 changes:
  - `files::route` uses `import::deck::DECK_EXT` for `Route::Deck` (and `.txt` → Song, anything else → Media). The PowerPoint `pick` filter is `DECK_EXT`.
  - `watch_deck_job(job)` adds **only** `job.result` when that job ends (driven by `Update::Data`), never "every new deck". A job that ends without a result adds nothing.
  - Add → Slides tab: the button **Import PowerPoint** becomes **Import presentation**. The Google Slides note becomes "Google Slides: in Google Slides choose File → Download → Microsoft PowerPoint (.pptx) or PDF, then import that file here. Keynote and PDF files work too." When `ConverterStatus` has no PowerPoint and `Lo::None`, the tab shows, above the list: "To import PowerPoint or Keynote files, install LibreOffice (free). PDF files work without it.", the `install_command(OS)` text in a copy box with **Copy** (`arboard`, toast "Copied"; same pattern as the Settings → Video Copy in Plan 3), and **Check again** (calls `converters.libreoffice()`, toasts "LibreOffice found!" or "Not found yet — finish installing, then click Check again."). With `Lo::Snap` it shows the Snap message instead.
  - The drop hint becomes "Videos, pictures, music, presentations (PowerPoint, Keynote, PDF), SongSelect lyric files".

- [ ] **Step 1: Write the failing tests** (all with `Converters` built directly: `powerpoint: None`, `pdf: None` or a real renderer when gated, LibreOffice from the Task 5 fake):
  - `route_table`: every `Kind` × PowerPoint yes/no × `Lo::{None, Snap, Ok}`
  - `not_presentation_adds_no_deck_and_ends_job_with_message`: `test_app()`, a text file named `.pptx`; it asserts `data.decks` is unchanged, `jobs[0].error == NOT_PRESENTATION`, and `tmp/` is empty
  - `keynote_package_directory_gives_plain_message` (a temp folder named `Talk.key` → `KEYNOTE_PACKAGE`, no I/O error text)
  - `cancel_removes_job_and_work_dir`: fake soffice `sleep:30000`, cancel after 200 ms; the job is gone, `tmp/` is empty and the fake process is gone
  - `waiting_label_shows_then_restores`: two imports with the fake (`sleep:1000`); while the second waits its label is "Waiting for LibreOffice…", then "Importing “…”"
  - `pdf_route_without_pdfium_gives_reinstall_message`
  - `libreoffice_found_after_install_without_restart` (`Converters` with `libreoffice: None` and a `ToolEnv` over a temp dir; create the fake soffice; the next import uses it)
  - Gated on `RP_PDFIUM`:
    - `libreoffice_import_adds_note` (fake soffice copying a fixture PDF; after the job ends it is still listed with "Imported with LibreOffice — check the slides.")
    - `overlapping_imports_set_their_own_results` (two imports at once; each job's `result` is its own deck, and the decks differ)
  - Plan 3 files (testing backend / pure): `route_by_extension` updated for `pot potx pptm key pdf`; `deck_filter_matches_router` (every `DECK_EXT` routes to `Deck` and the filter lists the same set); `watch_deck_job_adds_only_its_result` (two jobs end in either order → the service gets each deck exactly once); `slides_tab_shows_install_hint_copy_and_check_again` (status with no PowerPoint and `Lo::None`); `slides_tab_button_says_import_presentation`.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement. Delete `media::export_powerpoint` and `PPT_SCRIPT`.
- [ ] **Step 4:** `cargo test` passes; gated tests pass on Windows with PowerPoint and LibreOffice.
- [ ] **Step 5: Manual responsiveness check on Windows,** recorded in `2026-native-checks.md` (v1 §14.2 Responsiveness, global constraint "no blocking work"):
  - Import a 60-slide deck through PowerPoint while a 1080p video plays live.
  - Import a deck through LibreOffice while a 1080p video plays live. (The same check with 5 videos probing at once is in Task 8.)
  - In both: Next and Clear All take effect in under 200 ms, and the room keeps the source frame rate (`video_surface --measure` method).
- [ ] **Step 6:** Commit `feat(import): cross-platform deck import via PowerPoint, LibreOffice or PDF`.

### Task 7: mpv nodes, check C6, probing and thumbnails (`video::ffi`, `import::probe`)

**Files:**
- Create: `src/import/probe.rs`, `examples/probe.rs`
- Modify: `src/video/ffi.rs`, `docs/superpowers/checks/2026-native-checks.md`

**Interfaces:**
- `ffi.rs`: **extend Plan 3 Task 7's bindings**, which already add `mpv_command_ret`, `mpv_free_node_contents`, `mpv_node`, `mpv_node_list`, `mpv_byte_array` and the `NODE_MAP`/`BYTE_ARRAY` constants. Do not add them again. Add:
  - `pub const FORMAT_NODE_ARRAY: mpv_format = 7;` (neither plan defines it; `ffi.rs` stops at `FORMAT_NODE = 6`), and any of `NODE_MAP = 8` / `BYTE_ARRAY = 9` that Plan 3 did not name exactly.
  - An owned `#[derive(Debug, Clone, PartialEq)] pub enum Node { None, Str(String), Flag(bool), Int(i64), Double(f64), Array(Vec<Node>), Map(Vec<(String, Node)>), Bytes(Vec<u8>) }` with `Node::get(&self, key) -> Option<&Node>`, `as_f64`, `as_str`, and `unsafe fn Node::from_raw(*const mpv_node) -> Node`.
  - `pub unsafe fn command_node(&self, h, args: &[&str]) -> Result<Node, String>` (wraps Plan 3's `command_ret` and copies into `Node`) and `pub unsafe fn get_node(&self, h, name: &str) -> Option<Node>` (`mpv_get_property` with `FORMAT_NODE`). Both copy, then call `mpv_free_node_contents`. If Plan 3's `FrameGrabber` has its own node walker, switch it to `Node::from_raw` so there is one.
  - `pub unsafe fn request_log_messages(&self, h, level: &str)`.
- `probe.rs`:
  - `pub struct Probe { pub kind: MediaKind, pub duration: Option<f64>, pub size: Option<(u32, u32)>, pub thumb: Option<image::RgbImage> }`.
  - `pub fn classify(track_list: &Node) -> Option<MediaKind>`: video if any non-albumart video track, audio if audio tracks (album art allowed), otherwise `None`.
  - `pub fn thumb_time(duration: Option<f64>) -> f64`: `min(1.0, d * 0.1)`, or 0.
  - `pub fn bgr0_to_rgb(w: u32, h: u32, stride: usize, data: &[u8]) -> image::RgbImage`.
  - `pub fn probe_blocking(mpv: &Arc<Mpv>, path: &Path, deadline: Duration, cancel: &CancellationToken) -> Result<Probe, String>`:
    - One core per probe. Options: `vo=null ao=null sid=no pause=yes idle=yes config=no load-scripts=no ytdl=no osc=no input-default-bindings=no input-vo-keyboard=no terminal=no hwdec=no cache=no demuxer-readahead-secs=1 hr-seek=yes vd-lavc-threads=2 msg-level=all=error`.
    - `osc`, `ytdl` and `load-scripts` only exist in builds with Lua, and Task 16 builds without Lua. Setting them ignores `MPV_ERROR_OPTION_NOT_FOUND` (-5), as `player.rs` already does for `osc`. Any other option error fails the probe.
    - Flow: `loadfile` → `FILE_LOADED` (or `END_FILE` error → "This file can't be played.") → `duration` + `track-list` → `seek <t> absolute+exact` → `PLAYBACK_RESTART` → `screenshot-raw video` → `terminate_destroy`. A watchdog calls `mpv_wakeup` on the deadline **or** when `cancel` fires; the loop then returns "This file took too long to open." or `Cancelled`.
  - `pub fn probe_image(path: &Path) -> Result<Probe, String>`: `image::ImageReader::open(..)?.with_guessed_format()`, dimensions plus a decoded thumbnail. A file the enabled `image` features cannot decode gives "This picture can't be opened. Save it as PNG or JPEG and try again."
  - `pub async fn probe(mpv: Option<Arc<Mpv>>, path: PathBuf, cancel: CancellationToken) -> Result<Probe, String>`: images go through `probe_image`; everything else through `spawn_blocking(probe_blocking)` behind a 2-permit semaphore (waiting on the semaphore is also cancellable). With `None` it returns `ffi::MISSING`.
  - `pub fn thumb_jpeg(img: &image::RgbImage) -> Vec<u8>`: 320 px wide, Triangle filter, JPEG quality 85.

- [ ] **Step 1: Check C6 probe (needs libmpv).** First read Plan 3's row **"C6 live preview grab"**, which already records whether `screenshot-raw` works with `vo=null`. Do not repeat that part. `examples/probe.rs <file>` prints duration, tracks and the size of the `screenshot-raw` frame, and saves `probe-thumb.png`. Run it on a phone video with rotation metadata and a lavfi `testsrc2` clip, to check the **rotation** and the **seek-then-grab thumbnail** with the vo Plan 3 found working.
  - PASS: the rotation is applied and the thumbnail is non-uniform.
  - FAIL (or Plan 3's row says `vo=null` does not work): switch `probe_blocking` to `vo=libmpv` with an `MPV_RENDER_API_TYPE_SW` context sized to the thumbnail (the proven C1 software path), rendering one frame after `PLAYBACK_RESTART`.
  - Record the result as a new row **"C6 probe"** in the checks doc.
- [ ] **Step 2: Write the failing tests:**
  - Not gated:
    - `node_from_nested_map_and_bytes` (build an `mpv_node` tree by hand, with a `FORMAT_NODE_ARRAY` inside a map, and convert it)
    - `classify_video_audio_albumart_and_empty`
    - `thumb_time_rules`
    - `bgr0_to_rgb_respects_stride`
    - `probe_image_reads_png_dimensions`
    - `probe_image_refuses_undecodable_picture` (an `.avif` or truncated PNG → the picture message)
    - `missing_libmpv_gives_missing_message`
    - `thumb_jpeg_is_320_wide`
  - Gated `RP_VIDEO_TESTS=1`:
    - `probes_lavfi_video_duration_and_nonuniform_thumb` (`av://lavfi:testsrc2=size=640x360:rate=30:duration=3`)
    - `probes_sine_as_audio_without_thumb`
    - `garbage_file_fails_readably`
    - `probe_times_out`: a source that never delivers a frame — on Unix a FIFO made with `mkfifo` and never written; on Windows a named pipe `\\.\pipe\rp-probe-<uuid>` created by the test and never written — with a 1 s deadline. It returns the "took too long" message within 2 s. (A lavfi source with `pause=yes` loads in well under 1 s, so it cannot time out.)
    - `probe_cancel_returns_quickly` (the same never-written pipe, cancel after 200 ms)
    - `probe_works_on_lgpl_build_without_lua` (run with the Task 16 libmpv when `RP_LIBMPV` points at it; the Lua-only options don't fail the probe)
- [ ] **Step 3:** FAIL → **Step 4:** implement → `cargo test` passes; the gated tests pass on Windows.
- [ ] **Step 5:** Commit `feat(import): probe media and grab thumbnails with headless mpv (C6 probe)`.

### Task 8: Media import without FFmpeg, and video thumbnails everywhere (`import::media`, model, Plan 3 UI)

**Files:**
- Create: `src/import/media.rs`
- Modify:
  - `src/model.rs`: `MediaFile` gains `#[serde(default)] duration: Option<f64>, thumb: Option<AssetId>, size: Option<(u32, u32)>`. `referenced_assets` includes `thumb`. Doc comments on the legacy `MediaStatus::Converting`/`Unconverted` mark them as read-only.
  - `src/media.rs`: the extension lists move to `import::media::{VIDEO, AUDIO, IMAGE}`; `IMAGE` drops `avif` (the `image` crate here has no AVIF decoder, and adding one needs a system dav1d).
  - `src/app.rs`: the body of `import_staged` for `ImportPurpose::Media` and `ImportPurpose::Asset` (see the purpose mapping in the Global Constraints); action `media/reprobe {id}`; `upgrade_media` call site.
  - `src/bin/native.rs`: calls `upgrade_media` inside one `App::edit`, then `reprobe`.
  - Plan 3's UI: `src/ui/thumbs.rs`, `src/ui/dialogs/background.rs` + `ui/dialogs/background-picker.slint`, `src/ui/dialogs/add_media.rs` + `ui/dialogs/add-media.slint`, `src/ui/operator/files.rs` (`add_files`, filters).

**Interfaces:**
- `pub async fn import_media(app: &Arc<App>, file: &Path, display: String) -> Result<MediaFile, String>`. It reads `app.probe_mpv()` and runs as a job:
  1. `job_start_cancellable("Adding “{display}”")`
  2. `store.import_file` (hash + publish) for the source
  3. `probe(mpv, path, token)`; while waiting for a probe permit the label is "Waiting for another file…", then back. On failure or cancel, nothing is added, the published source is left for the normal unreferenced-asset cleanup, and the error is returned ("This file can't be played.", or `import::CANCELLED` = "Cancelled.").
  4. the thumbnail is published as a `.jpg` asset
  5. one `App::edit` pushes `MediaFile { status: Ready, duration, size, thumb, .. }`
  6. `job_end(id, None)` on success, `job_end_cancelled` on cancel, and `job_end(id, Some(err))` otherwise, so a failure shows on the card as well as in the returned error.

  The kind comes from the probe, not from the extension. An unprobed image extension goes to `probe_image` first.
- `ImportPurpose::Asset` (logo, Make slides from pictures, background upload): runs `probe_image` first and refuses files it cannot decode with "This picture can't be opened. Save it as PNG or JPEG and try again.", before `store.publish`.
- **How Plan 3's `add_files` waits:** `Ops::import(path, Media, done)` still awaits `App::import_path`, which returns only when `import_media` has finished, so `add_files` gets `Imported::Media(m)` (or the error) as before and then adds the item. The client "Uploading `<name>`" card covers the copy into `tmp/` and ends when the copy ends; the server job "Adding “…”" then shows the probe with a Cancel button. An `Err` equal to `import::CANCELLED` shows no toast.
- `pub fn upgrade_media(data: &mut Data) -> Vec<Uuid>`: maps `Converting`/`Unconverted` → `Ready` and returns every media id with `thumb == None` or a legacy status. The native binary calls it inside one `App::edit` right after `App::new`, so the change is saved and `data.json` no longer holds legacy statuses.
- `pub fn reprobe(app: Arc<App>, ids: Vec<Uuid>)`: runs in the background, one at a time, and fills in duration, size and thumb. A failure sets `Failed { reason: "This file can't be played." }`. It ends with one `App::edit`. Action `media/reprobe {id}` runs it for one id (the **Try again** button in Task 13).
- **Thumbnails in Plan 3's UI** (Plan 3 left placeholders for these):
  - `ui::thumbs`: a video slide (video item) and a **video background** (item or `settings.background`) use the media's `thumb` asset, decoded at the cell size, instead of a black frame. Audio stays black with the overlay.
  - `BackgroundPicker`: video choices show the `thumb` as their image.
  - Add → Media rows: every row with a `thumb` shows it. Picture rows use the `thumb` too, instead of decoding the full picture at 96 px; rows without a `thumb` (old entries before `reprobe` finishes) keep the Plan 3 behaviour.
- Plan 3's `pick` media filter and `files::all_images` read `import::media::{VIDEO, AUDIO, IMAGE}`.

- [ ] **Step 1: Write the failing tests:**
  - `old_statuses_become_ready_and_are_listed_for_reprobe`: also reloads `data.json` from disk through `store.load()` and checks no `Converting`/`Unconverted` is left
  - `old_data_json_without_new_fields_loads`
  - `thumb_is_a_referenced_asset_and_survives_backup` (extends the store backup test)
  - `unplayable_file_adds_nothing` (`probe_mpv` unset → `MISSING` error, `data.media` unchanged, and the job shows the error)
  - `image_import_sets_size_and_thumb`
  - `asset_import_refuses_undecodable_picture` (`ImportPurpose::Asset` with a truncated PNG → the picture message, nothing published)
  - `media_import_is_a_cancellable_job` (a job "Adding “x”" with `cancellable` appears while importing; with a never-written pipe file and `RP_VIDEO_TESTS=1`, `job/cancel` makes `import_path` return `CANCELLED` quickly)
  - `image_filter_has_no_avif`
  - Plan 3 UI (testing backend / pure):
    - `video_slide_thumb_uses_media_thumb` (`thumbs` for a video item with a `thumb` returns a non-black buffer)
    - `video_background_thumb_uses_media_thumb`
    - `background_picker_video_choice_has_thumb_image`
    - `media_row_shows_thumb_when_set` (Add → Media row image is non-empty for a media file with a `thumb`)
    - `cancelled_media_import_shows_no_toast`
  - Gated `RP_VIDEO_TESTS=1`: `video_import_sets_duration_size_thumb_ready`
- [ ] **Step 2:** FAIL → **Step 3:** implement. No conversion is started for new uploads; `spawn_conversion` is now only reachable from the legacy `media/convert` action, which Task 13 deletes.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Manual responsiveness check on Windows** (v1 §14.2 Responsiveness), recorded in `2026-native-checks.md`: while a 1080p video plays live, add 5 videos at once and import a deck through LibreOffice. Next and Clear All take effect in under 200 ms and the room keeps the source frame rate.
- [ ] **Step 6:** Commit `feat(import): media import probed by mpv as a job; video thumbnails in the operator`.

### Task 8a: SongSelect duplicate prompt ("update existing or keep both", v1 §9.2)

**Files:**
- Modify: `src/app.rs` (`song/import` action, `.txt` path of `import_staged`), Plan 3's `src/ui/operator/files.rs` and its tests

**Interfaces:**
- New action `song/import {text: String, mode: "ask" | "update" | "keep_both"}` → `{id, title, duplicate_of}`:
  - `"ask"`: parse; when a song with the same non-empty CCLI # exists, save **nothing** and return `{id: null, title, duplicate_of: <existing id>}`. Otherwise save and return as today.
  - `"update"`: replace the existing song's title, writers, CCLI #, copyright and lyrics; **keep its id** (and its `order`; order names that no longer match a section are dropped by the core as today), so service items still point at it. Returns `{id: <existing id>, …}`.
  - `"keep_both"`: today's behaviour (push a new song).
  - The raw-body `song/import-txt` action keeps its keep-both behaviour for the web operator until Task 13 removes the web operator, then it is deleted.
- `import_staged(.., ImportPurpose::Media)` for a `.txt` uses `"ask"`. `Imported::Song { id: Option<Uuid>, title, duplicate_of }`; `id: None` means "not saved, ask the user".
- Plan 3's `add_files` for a song with `id: None`: show **Confirm** "“T” is already in the library." with **Update existing** / **Keep both**. Either button reads the file again off the UI thread and calls `song/import` with that mode, then adds the song to the service and toasts "Updated “T”" or "Imported “T”". Esc/closing the confirm adds nothing. The old toast "“T” was already in the library — added a second copy." is no longer used.

- [ ] **Step 1: Write the failing tests:**
  - `app.rs`: `import_ask_with_duplicate_saves_nothing`, `import_update_keeps_song_id_and_service_items` (a service item for the old song shows the new lyrics through `slides::song_slides`), `import_keep_both_adds_second_song`, `import_ask_without_duplicate_saves`.
  - Testing backend: `duplicate_song_file_asks_update_or_keep_both` (Update → one song, same id, new lyrics; Keep both → two songs), `closing_duplicate_confirm_adds_nothing`.
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat: SongSelect duplicate prompt — update existing or keep both`.

### Task 9: Saving the committed frame (`recovery::state`)

**Files:**
- Create: `src/recovery/mod.rs`, `src/recovery/state.rs`
- Modify:
  - `src/lib.rs`
  - `src/live.rs`: add `Deserialize` to `Cursor` and `LiveItem` (today both are `#[derive(Serialize, Clone, Debug, PartialEq)]` only; their field types `model::Slide` and `model::Background` already derive `Deserialize`, and `Frame` already does, so nothing else needs it); add `Live::committed()` and `Live::restore()`
  - `src/app.rs`: state writer hook and resume positions
  - `src/ui/room.rs`: prepared players start at `app.resume_position(inst)`

**Interfaces:**
- `state.rs`:
  - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] pub struct LiveState { pub v: u32 /* 1 */, pub seq: u64, pub saved_at: String, pub frame: Frame, pub cursor: Option<Cursor>, pub item: Option<LiveItem>, pub positions: Vec<(u64, f64)> /* inst → seconds */, pub windows: Vec<String> /* open roles: "room", "stage" */ }`.
  - `pub fn path(root: &Path) -> PathBuf` (`<data>/live-state.json`).
  - `pub fn load(root: &Path) -> Option<LiveState>`: a missing file, a parse error, or `v != 1` all mean `None`.
  - `pub fn quarantine(root: &Path)`: renames to `live-state.crashed.json`.
  - `pub fn clear(root: &Path)`.
  - `pub fn write_with_retry(path: &Path, bytes: &[u8]) -> io::Result<()>`: `store::write_atomic`, retried 10 × 30 ms on `PermissionDenied` and sharing violations (raw OS error 32 on Windows).
  - `pub struct StateWriter`, backed by `Arc<(Mutex<Option<LiveState>>, Condvar)>` and the thread `live-state`.
    - `StateWriter::start(root: PathBuf) -> StateWriter`.
    - `pub fn submit(&self, s: LiveState)`: latest value wins; never blocks beyond the mutex.
    - `pub fn flush_and_stop(self)`.
  - `pub fn snapshot(live: &Live, windows: Vec<String>, seq: u64) -> Option<LiveState>`: built from the **committed** snapshot. Positions come from `live.media_status`/`audio_status` for instances in that frame.
- `live.rs`:
  - `pub fn committed(&self) -> Option<(Frame, Option<Cursor>, Option<LiveItem>)>`: reads the private `history` entry whose `version == committed_version` (the same lookup as `committed_frame()`), taking its `frame`, `cursor` and `item`. It never reads `self.frame`/`self.cursor`/`self.item`, which hold the *requested* state. `None` when nothing has been committed yet.
  - `pub fn restore(&mut self, s: &LiveState, with_media: bool)`:
    - sets frame, cursor and item, then bumps the version (so the room commits it);
    - `next_inst` = max saved inst + 1;
    - when `with_media` is false, drops `frame.media` (video), `frame.audio` and a video `frame.background`, and keeps colour and image backgrounds, stills and slides.
- `app.rs`:
  - `state_writer: OnceLock<StateWriter>`, `pub fn set_state_writer(&self, w: StateWriter)`.
  - After `room_event` advances `committed_version`, it submits `snapshot(..)`.
  - `pub fn save_live_state(&self)`: called every 5 s by the native binary while media plays.
  - `resume: Mutex<HashMap<u64, f64>>`, `pub fn set_resume(&self, positions: &[(u64, f64)])`, `pub fn resume_position(&self, inst: u64) -> Option<f64>` (consumed once).
  - `pub fn set_notice(&self, text: &str)`: sets `live.notice` and broadcasts.

- [ ] **Step 1: Write the failing tests:**
  - `snapshot_uses_committed_frame_not_requested`
  - `load_missing_garbage_and_wrong_version_are_none`
  - `restore_reissues_instances_above_saved`
  - `restore_without_media_keeps_slide_and_colour_background`
  - `writer_keeps_only_latest` (submit 100 states while the thread is paused by a test hook; the file holds `seq` 100)
  - `rename_retries_when_target_is_locked` (Windows: hold the target open without `FILE_SHARE_DELETE` for 100 ms)
  - `app_writes_state_after_commit` (`test_app()`, `command(show)`, `room_event(Committed)`, then the file has the frame)
  - `resume_position_is_consumed_once`
  - `quarantine_renames`
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(recovery): save the committed frame and media positions atomically`.

### Task 10: Supervisor, exit codes and panic hook (`recovery::supervisor`, `recovery::panic`)

**Files:**
- Create: `src/recovery/supervisor.rs`, `src/recovery/panic.rs`, `src/recovery/exitcode.rs`
- Modify:
  - `Cargo.toml`:
    - `windows-sys` feature `Win32_System_Diagnostics_Debug`.
    - Unix `signal-hook = "0.3"`.
    - `[profile.release]`: `lto = "thin"`, `debug = "line-tables-only"`, `strip = "debuginfo"`. This replaces `strip = true`, so release backtraces keep their names. **No `split-debuginfo`:** the local Windows toolchain is `x86_64-pc-windows-gnu` (MinGW from `.tools\mingw64`, see `dev-build.cmd`), where rustc rejects `-Csplit-debuginfo=packed` as unstable, and `cargo test --locked --release` must keep working until Task 12.
  - Plan 2's panic logging in `src/bin/native.rs` moves into `recovery::panic`.

**Interfaces:**
- `exitcode.rs`: `pub const OK: i32 = 0; CONFIG = 2; ALREADY_RUNNING = 3; PORT_TAKEN = 4; GAVE_UP = 5; PANIC = 101;`. `GAVE_UP` is only ever the supervisor's own exit code.
- `supervisor.rs`:
  - `#[derive(Debug, PartialEq)] pub enum Exit { Stop(i32), Crash(String /* "code=0xC0000005" | "signal=11" */) }`.
  - `pub fn classify(st: std::process::ExitStatus) -> Exit`: 0/2/3/4 → Stop; `0xC000013A` and SIGTERM/SIGHUP/SIGINT → Stop; everything else → Crash.
  - `pub struct Guard { crashes: VecDeque<Instant> }` with `pub fn record(&mut self, now: Instant) -> Next`, where `pub enum Next { Restart { attempt: u32 }, GiveUp }`. Three crashes inside 60 s give `GiveUp`.
  - `pub fn app_args(attempt: u32, passthrough: &[OsString]) -> Vec<OsString>`: attempt 0 → `passthrough`; 1 → `["--restore"]` + passthrough; 2 → `["--restore", "--attempt", "2"]` + passthrough. A `--restore` already in `passthrough` is not repeated (so a supervisor started with `--restore` restores on attempt 0 too; the recovery tests use this).
  - `pub fn run(exe: &Path, log: &Path, passthrough: &[OsString]) -> i32`, the loop:
    - spawns `exe --app …` with env `RUSTYPRESENTER_SUPERVISED=1`, stdin null, stdout/stderr appended to the log
    - after each exit, logs `"{time} app exited: {classify}"`
    - on a crash, sleeps 250 ms and relaunches
    - on `GiveUp`, logs `"{time} gave up after 3 crashes"`, calls `give_up_dialog` and returns `GAVE_UP`
    - checks `shutting_down()` before every relaunch
  - `pub fn shutting_down() -> bool`: Windows `GetSystemMetrics(SM_SHUTTINGDOWN) != 0`; Unix a `signal-hook` flag, set together with forwarding SIGTERM to the child.
  - `pub fn quiet_crash_dialogs()`: Windows `SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX)`; a no-op elsewhere. It is called in the supervisor and again at the top of `--app`.
  - `pub fn give_up_dialog(log: &Path)`: `rfd::MessageDialog` with "RustyPresenter stopped after crashing 3 times in a minute. The log is at {path}." It is skipped when `RUSTYPRESENTER_HEADLESS=1`.
- `panic.rs`:
  - `pub fn install(log: PathBuf)`: `Backtrace::force_capture`; appends and `sync_all`s the log; calls the default hook; when on a critical thread and not inside `Catching`, `process::exit(PANIC)`.
  - `pub fn mark_critical()`, called on the UI/main thread, the `live-state` thread and each mpv event thread.
  - `pub struct Catching` (RAII counter for `catch_unwind` users).
  - `pub fn test_crash_point(name: &str)`: when `RUSTYPRESENTER_TEST_CRASH` equals `panic`, `abort` or `segv` and `RUSTYPRESENTER_TEST_CRASH_AT == name`, it crashes that way. Always compiled in; it does nothing unless set.
- Native binary arguments: `pub struct Args { pub app: bool, pub restore: bool, pub attempt: u32, pub no_browser: bool, pub rest: Vec<OsString> }` and `pub fn parse_args(it: impl Iterator<Item = OsString>) -> Args`. With no `--app`, `main` runs `supervisor::run` and exits with its code. The supervisor uses `server::data_dir()` for the log path only, and never reads `data.json`.

- [ ] **Step 1: Write the failing tests:**
  - `classify_normal_codes_stop` (Windows: `ExitStatusExt::from_raw(0/2/3/4)`; Unix: `from_raw(code << 8)`)
  - `classify_access_violation_and_fastfail_crash` (Windows `0xC0000005`, `0xC0000409`)
  - `classify_ctrl_c_stops` (Windows `0xC000013A`)
  - `classify_signals` (Unix SIGSEGV/SIGABRT/SIGKILL crash; SIGTERM/SIGHUP/SIGINT stop)
  - `guard_gives_up_on_third_crash_within_60s`
  - `guard_forgets_crashes_older_than_60s`
  - `app_args_escalate`
  - `parse_args_cases`
  - `critical_thread_panic_exits_101` (the Task 2 self-invocation helper: `current_exe()` with `["--exact", "panic_helper", "--nocapture"]` and `RP_PANIC_HELPER=<log path>`; `#[test] fn panic_helper()` does nothing unless that env is set, otherwise installs the hook, marks its thread critical and panics. Assert exit code 101 and that the log contains `PANIC thread=`)
  - `caught_panic_does_not_exit`
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `feat(recovery): supervisor process, exit codes and crash logging`.

### Task 11a: Headless room for recovery tests (`recovery::headless`)

**Files:**
- Create: `src/recovery/headless.rs`
- Modify: `src/recovery/mod.rs`

**Interfaces:**
- `pub fn run(app: Arc<App>, log: impl Write + Send + 'static) -> JoinHandle<()>`: a thread that follows `app.subscribe()` with no Slint and no mpv. On every `Update::Live` whose version is newer than `committed_version`, it calls `app.room_event(RoomEvent::Committed { version })`. After a restore (the first committed version after start-up with `--restore`) it logs `restored version={v} slide={json}`, where `slide` is the committed cursor's slide text, and `media={true|false}` for whether the frame has video or audio.
- The `restore-media` crash point: `Live::restore(.., with_media: true)` calls `panic::test_crash_point("restore-media")` before it applies the media layers, so the crash happens only on a restore **with** media.
- Used only when `RUSTYPRESENTER_HEADLESS=1` (tests). It is compiled in all builds; it is a few dozen lines and has no dependencies.

- [ ] **Step 1: Write the failing tests:** `headless_commits_each_version` (`test_app()`, two `show` commands → `committed_version` follows), `headless_logs_restored_version_and_slide` (restore a `LiveState`, then the log line has the version and slide text), `restore_media_crash_point_only_with_media` (with the crash env set through the Task 2 self-invocation helper: `with_media: false` returns normally; `with_media: true` exits with 101).
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit `test(recovery): headless room for supervisor tests`.

### Task 11: Restore, single instance and the recovery test (`platform::instance`, native binary)

**Files:**
- Create: `src/platform/instance.rs`, `tests/recovery.rs`
- Modify: `src/bin/native.rs`, `src/ui/screens.rs` (reopen restored roles; `open("operator")` focuses the operator window)

**Interfaces:**
- `instance.rs`:
  - `pub enum Bind { Ok(std::net::TcpListener), Running, Taken }`.
  - `pub fn bind(port: u16, restore: bool) -> Bind`: on `--restore`, retries 15 × 100 ms. `Running` means a loopback `POST /api/do/display/open {"role":"operator"}` succeeded; `Taken` means it failed.
  - `pub fn focus_running(port: u16) -> Result<(), String>`: `show_running` moved from `src/main.rs`.
- `--app` flow in `native.rs`:
  1. `quiet_crash_dialogs()`, `panic::install`, `mark_critical()`.
  2. `bind`:
     - `Running` → exit `ALREADY_RUNNING`.
     - `Taken` → the dialog "Port {port} is in use by another program, so RustyPresenter cannot start." and exit `PORT_TAKEN`.
     - `fail()` exits `CONFIG`.
  3. Write `<data>/app.pid`. `Store::open`, then `store.clean_tmp()` on **every** start, `--restore` included (a killed import's work folder must go). `import::init` runs `kill_leftover` before `warm_up` (Task 5), so a LibreOffice left by the dead app is gone before anything touches its profile.
  4. The start-up backup (today `main.rs`: "A complete backup at every start, then daily") runs only **without** `--restore`. On `--restore` the daily timer still starts, but its first run waits the full 24 h. Each crash-restart would otherwise make another copy of the same data and push older good backups out of the 10 that are kept.
  5. On `--restore`, `state::load`:
     - attempt ≥ 2 → `quarantine`, `restore(with_media: false)`, and `set_notice("RustyPresenter restarted after a problem. Video/audio stopped — click it again to replay.")` (the second sentence is today's room-restart wording, `app.rs` "The room screen restarted. Video/audio stopped — click it again to replay.");
     - otherwise `restore(with_media: true)`, `set_resume(positions)` and `set_notice("RustyPresenter restarted after a problem.")`;
     - open the saved `windows` roles on their saved monitors (Plan 2 `pick` rules).
  6. Without `--restore`: `state::clear`.
  7. Start the `StateWriter` and a 5 s tokio interval calling `save_live_state` while media or audio is live.
  8. On normal quit (operator closed): `flush_and_stop`, `state::clear`, exit `OK`. Closing the operator window ends the `--app` process with 0, so the supervisor exits too.
- `RUSTYPRESENTER_HEADLESS=1` (tests only): no Slint. The core, the server and the headless room from Task 11a run instead.

- [ ] **Step 1: Write the failing tests** in `tests/recovery.rs`. They use `env!("CARGO_BIN_EXE_rustypresenter-native")` (`rustypresenter` after Task 12), a temp `RUSTYPRESENTER_DATA`, a random `RUSTYPRESENTER_PORT` and `RUSTYPRESENTER_HEADLESS=1`. Commands go through loopback `POST /api/do/service/add` and `/api/do/command` with `x-rp: 1`.
  - `killed_app_is_restored_with_its_frame`: start the supervisor, add a text item and show it, read `app.pid`, kill that pid (`taskkill /F /PID` / `libc::kill(SIGKILL)`). Within 2 s a new pid appears, and the log has `--restore` and `restored version=` with the same slide text.
  - `three_crashes_in_a_minute_stop_the_supervisor`: `RUSTYPRESENTER_TEST_CRASH=panic`, `_AT=startup`. The supervisor exits with `GAVE_UP` (5) within 5 s and the log has "gave up".
  - `second_attempt_restores_without_media`: write a saved state with a text slide and an audio layer to `live-state.json`, and start the supervisor **with `--restore` in its arguments** (passed through to attempt 0, see `app_args`), plus `RUSTYPRESENTER_TEST_CRASH=panic`, `_AT=restore-media`. Attempts 0 and 1 restore with media and crash at that point; attempt 2 restores without media and never reaches it. The log has `restored … media=false` with the slide text, `live-state.crashed.json` exists, and `/api/do/…` state shows the notice "RustyPresenter restarted after a problem. Video/audio stopped — click it again to replay."
  - `restore_skips_startup_backup`: after `killed_app_is_restored_with_its_frame`-style kill and restore, `backups/` holds exactly the one backup from the first normal start.
  - `killed_import_publishes_nothing` (v1 §14.2 "Interrupted import publishes nothing"). Setup: the app binary itself acts as the fake soffice — at the very top of `main`, before argument parsing, `import::libreoffice::fake_soffice_main()` runs and exits only when `RP_FAKE_SOFFICE` is set **and** the first argument is `--headless` (always compiled in, a no-op otherwise, like `test_crash_point`). The supervisor and the `--app` process inherit `RP_FAKE_SOFFICE` too, so the `--headless` check is what keeps them from acting as the fake. The test sets `RP_SOFFICE=<CARGO_BIN_EXE path>` and `RP_FAKE_SOFFICE=hang` on the supervisor's env only (no `set_var` in the test process). The input is a minimal Keynote zip built in-test (`Index/Document.iwa` and `Index/Slide.iwa`), because `route` sends Keynote to LibreOffice even on a Windows PC with PowerPoint installed; a `.pptx` would go to PowerPoint there. It starts a deck import through the loopback action `import/path {path, purpose: "powerpoint"}` (new in this task: it calls `App::import_path`; it is operator-only and never on the phone allow-list), kills the app pid mid-import, and waits for the restore. Then: no deck was added, `tmp/` is empty, and no fake soffice process is left (Unix: its pgid is gone; Windows: the Job Object killed it). Gated on `RP_PDFIUM`: restart with `RP_FAKE_SOFFICE=pdf:<fixture>`, and a second import succeeds.
  - `second_launch_exits_already_running`: a second supervisor exits 0 within 3 s, and the first app's pid is unchanged.
  - `port_taken_by_other_program_does_not_restart`: bind the port in the test first. The supervisor exits 4 once.
  - `normal_start_clears_old_state`
- [ ] **Step 2:** FAIL → **Step 3:** implement → **Step 4:** `cargo test` passes on Windows (CI: all three).
- [ ] **Step 5: Manual checks.** Record the results in the checks doc.
  - **Windows:** End task on `rustypresenter-native.exe --app` in Task Manager's Details tab while a video plays on the room screen. It is back within 2 s, the video continues near where it was, and the notice shows.
  - **macOS (needs a Mac):** Force Quit. Check for a second Dock icon or a bouncing supervisor icon. If one appears, set the supervisor's activation policy to Prohibited (`objc2-app-kit`).
  - **Linux (needs a Linux PC):** `kill -9`.
  - **All three:** log out while the app runs; it must not relaunch.
- [ ] **Step 6:** Commit `feat: crash restore and single instance in the native app`.

### Task 12: Remove Tauri and WebView2; the native binary becomes `rustypresenter`

**Files:**
- Delete: `src/displays.rs`, `tauri.conf.json`, `gen/`, `src/bin/native.rs` (its content moves to `src/main.rs`)
- Create: `src/platform/dialog.rs` (`pub fn error(msg: &str)` through `rfd::MessageDialog`; replaces `displays::error_box`)
- Modify:
  - `Cargo.toml`: remove `tauri` and `tauri-build`; one `[[bin]] name = "rustypresenter"`; `[target.'cfg(windows)'.build-dependencies] winresource = "0.1"`
  - `build.rs`: keep `slint_build`; on Windows embed `icons/icon.ico` and the existing Common Controls manifest with `winresource`
  - `src/main.rs`: **keep** its existing `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` (line 2 today) and replace the Tauri `main` with the native one
  - `tests/recovery.rs`: `env!("CARGO_BIN_EXE_rustypresenter-native")` becomes `env!("CARGO_BIN_EXE_rustypresenter")`; the same for any other test that names the native binary
  - `src/lib.rs`, `.github/workflows/ci.yml` (drop the webkit/gtk packages; drop the `--release` workaround; add the Slint Linux packages Plan 2 Task 9 found necessary), `dev-build.cmd`, `RustyPresenter.cmd`
  - `.cargo/config.toml` only if it exists by then (it does not today)

**Interfaces:**
- No new APIs. `App::Screens` has one implementation, `NativeScreens`.
- `room_connected`/`stage_connected` must reflect the native windows. If they are still only bumped by the SSE `ConnGuard`, compute them from `Screens::open_roles` in `state_json` instead.

- [ ] **Step 1: Write the failing check:** a test `no_tauri_or_webview_left` in `tests/cleanup.rs`. It scans `src/`, `build.rs` and `Cargo.toml` and asserts that none contains `tauri`, `WebView2` or `webview`, case-insensitive.
- [ ] **Step 2:** FAIL → **Step 3:** remove and move.
- [ ] **Step 4:** `cargo test --locked` (debug) passes on Windows. `cargo build --release` produces `rustypresenter.exe` with the icon and no console window.
- [ ] **Step 5: Manual check on Windows:** the double-clicked release exe starts the supervisor and the operator window. The taskbar shows one icon.
- [ ] **Step 6:** Commit `refactor!: remove Tauri and WebView2; the native app is the only binary`.

### Task 13: Remove the web screens, `render.js` and FFmpeg

**Files:**
- Delete: `web/operator.html`, `web/operator.css`, `web/operator.js`, `web/output.html`, `web/output.js`, `web/render.js`, `web/stage.html`
- Modify:
  - `src/server.rs`:
    - Keep `/remote*`, plus `GET /static/remote.js` only.
    - Keep `POST /api/do/{*action}`, loopback-only with `x-rp`, used by single-instance focus, tests and local automation.
    - Remove `/`, `/output`, `/stage`, `/assets/{name}`, `/api/state`, `/api/upload`, `/events`, `/api/room-event`, and `ConnGuard` for room/stage.
  - `src/media.rs`: delete `find_tool`, `ffmpeg`, `ffprobe`, `Plan`, `plan_video`, `plan_audio`, `convert` and `cmd`. The extension lists already live in `import::media` (Task 8); delete the file.
  - `src/app.rs`: delete `spawn_conversion`, the `media/convert` action, `Update::Convert`, the `"ffmpeg"` key, the raw-body `song/import-txt` action (Task 8a), and `room_recovered` if nothing native calls it.
  - Plan 3 UI — every FFmpeg leftover Plan 3 ported, each removed or replaced:
    1. `OperatorSnapshot.ffmpeg` (`src/ui/operator/view.rs`): removed.
    2. `Update::Convert{id, percent}` handling in `follow` (`src/ui/operator/window.rs`): removed. It stops compiling once `Update::Convert` is deleted, so this goes in the same commit.
    3. `subtitle` strings "Video · converting N%", "Video · may not play — needs converter" and "Video · conversion failed" (`view.rs`): removed. `Failed { reason }` (now only set by `reprobe`) reads "Video · can't be played". The legacy `Converting`/`Unconverted` statuses no longer occur after `upgrade_media`; if one is seen it shows like `Ready`.
    4. The toast "`<name>` added — converting so it plays smoothly…" (`files.rs`): removed; media always toasts "`<name>` added".
    5. The Add → Media hint "Any video format works. Unusual ones are converted once, automatically." becomes "Any video format works." (`add-media.slint`).
    6. The Converting and Needs converter chips and the **Convert** button (`add-media.slint`, `add_media.rs`): removed. A `Failed` row keeps the Failed chip with tooltip "This file can't be played." and gets **Try again** → `media/reprobe {id}` → toast "Checking the file again…". Plan 3's test `convert_button_only_for_unconverted_or_failed` becomes `try_again_button_only_for_failed` and `media_rows_newest_first_with_status_chips` drops the Converting/Needs converter cases.
    7. The Settings → Video tab (`settings.slint`, `settings.rs`): the FFmpeg panel, the `winget install --id Gyan.FFmpeg -e` Copy box and its **Check again** are removed. The tab stays, showing player status only: "Video player: libmpv `<version>` (LGPL build)" or the missing-player message, and "Video drawing: GPU (OpenGL)" / "Software", from the same source as About.
    8. `dialog_refresh()` no longer re-runs on Convert updates.
  - `README.md` Video section.

**Interfaces:**
- `MediaStatus::Converting` and `Unconverted` stay in the enum for reading old `data.json`. `upgrade_media` (Task 8) removes them on load.

- [ ] **Step 1: Write the failing tests:**
  - `cleanup.rs::no_ffmpeg_left`: forbidden uses only — `find_tool`, `Command::new("ffmpeg"` / `Command::new("ffprobe"`, `media::ffmpeg`, `plan_video`, `plan_audio`, `spawn_conversion`, `Update::Convert`, `Gyan.FFmpeg` — appear nowhere in `src/` or `ui/`. The word "FFmpeg" itself is allowed: About reads mpv's `ffmpeg-version` and shows "FFmpeg libraries under the LGPLv2.1".
  - `cleanup.rs::web_folder_has_only_remote`.
  - `server` tests: `operator_page_is_gone_404`, `remote_page_still_served`, `api_do_rejects_non_loopback`.
  - Plan 3 tests: `try_again_button_only_for_failed`, `failed_video_subtitle_says_cant_be_played`, `video_tab_shows_player_status_not_ffmpeg`.
  - Update the existing server tests that read web pages.
- [ ] **Step 2:** FAIL → **Step 3:** delete and fix → **Step 4:** `cargo test --locked` passes.
- [ ] **Step 5: Manual check:** pair a real phone and drive Next/Clear All against the native room. **Needs a phone on the church Wi-Fi.**
- [ ] **Step 6:** Commit `refactor!: remove web operator, room and stage pages, render.js and FFmpeg`.

### Task 14: Licences and About — extend Plan 3 Task 16

Plan 3 Task 16 already created `about.toml`, `about.hbs`, the committed root `THIRD-PARTY-LICENSES.txt` (embedded with `include_str!`), `packaging/licenses/native.txt`, `src/ui/dialogs/about.rs` with `AboutInfo { version, data_folder, video, mpv }` and `about_info(snap, mode, mpv)`, `ui/dialogs/settings-about.slint`, and the CI job that regenerates the file and fails on a diff. This task **modifies** those; it creates no second copy.

**One licence source.** The committed root `THIRD-PARTY-LICENSES.txt` is the only licence text. About shows the embedded copy (so development builds always have it), and the installers ship the same committed file (Task 15 `resources`). Nothing reads a licence file at runtime, and there is no "Licence file not found." case.

**Files:**
- Create:
  - `packaging/licenses/` additions, committed: `NotoSans-OFL.txt` (copied from `assets/fonts/OFL.txt`), `pdfium-LICENSE.txt`, `LGPL-2.1.txt`, `mpv-Copyright.txt`, `FFmpeg-COPYING.LGPLv2.1.txt` (copied once from the libmpv artifact, then committed) and `NOTICE-LGPL.txt` (the source link). Plan 3's `native.txt` stays as the short summary at the top.
  - `scripts/licenses.sh`: `cargo about generate about.hbs -o THIRD-PARTY-LICENSES.txt`, then appends `packaging/licenses/native.txt` and every other file in `packaging/licenses/`, in a fixed order.
- Modify:
  - `about.toml` (Plan 3): make sure the accepted list also has `Apache-2.0 WITH LLVM-exception`; Slint is accepted only under `LicenseRef-Slint-Royalty-free-2.0`, never `GPL-3.0-only`.
  - `src/ui/dialogs/about.rs` and `ui/dialogs/settings-about.slint` (Plan 3).
  - Plan 3's CI licence job in `.github/workflows/ci.yml`: it runs `scripts/licenses.sh` and then `git diff --exit-code THIRD-PARTY-LICENSES.txt`. Task 16 adds no second licences job.
  - The `include_str!` in `about.rs` drops the separate `native.txt` include, because the root file now contains it.

**Interfaces:**
- Plan 3's `AboutInfo { version, data_folder, video, mpv }` **gains** `ffmpeg: String` (mpv `ffmpeg-version`), `lgpl: bool` (`mpv-configuration` contains `-Dgpl=false`), `pdfium: String` ("found" / "missing"), `libreoffice: String` (path, "Snap — not usable", or "not installed"), `powerpoint: bool`. The existing fields and their wording stay; `mpv` shows "libmpv `<version>` (LGPL build)" or "libmpv `<version>` (not an LGPL build)".
- `about_info(snap, mode, mpv)` becomes `about_info(snap, mode, mpv, conv: Option<&Converters>)`; Plan 3's call site passes `app.converters()`.
- The About tab adds the new rows under Plan 3's. The LGPL line and **Show licenses** stay as Plan 3 built them.

- [ ] **Step 1: Write the failing tests** (extending Plan 3's `about.rs` tests):
  - `about_info_reports_missing_pdfium_and_libreoffice`
  - `about_info_flags_gpl_libmpv` (a stub `mpv-configuration` string without `-Dgpl=false` gives "(not an LGPL build)")
  - `licenses_text_has_lgpl_and_pdfium_texts` (the embedded text contains "GNU LESSER GENERAL PUBLIC LICENSE" and the pdfium licence)
  - Plan 3's `about_shows_aboutslint` and `licenses_text_mentions_slint_and_libmpv` still pass.
- [ ] **Step 2:** FAIL → **Step 3:** implement. Run `scripts/licenses.sh` locally and commit the regenerated `THIRD-PARTY-LICENSES.txt`; fix any rejected licence by adding it to `about.toml` only when it really is compatible.
- [ ] **Step 4:** `cargo test` passes, `scripts/licenses.sh` exits 0, and `git diff --exit-code THIRD-PARTY-LICENSES.txt` is clean afterwards.
- [ ] **Step 5:** Commit `feat: full third-party licence texts and converter status in About`.

### Task 15: Installers (`Packager.toml`, staging)

**Files:**
- Create:
  - `Packager.toml`. It is camelCase only, and lives in its own file so packaging changes never touch `Cargo.toml`.
  - `packaging/pins.env`.
  - `packaging/stage.sh`, a bash script that also runs in Git Bash on Windows.
  - `packaging/icons/`: 32, 128, 256 and 512 PNGs plus `icon.ico`, made from `icons/icon.ico`. `icon.icns` is generated on macOS by `stage.sh` with `iconutil`.
  - `packaging/README.md`, covering the staging layout.
- Modify: `.gitignore` (`/packaging/native/`, `/packaging/out/`, `/dist/`)

**Interfaces:**
- `Packager.toml`:

  ```toml
  productName = "RustyPresenter"
  identifier = "church.gracefirst.rustypresenter"
  version = "0.2.0"                    # keep equal to Cargo.toml; Task 16 CI checks it
  publisher = "RustyPresenter contributors"
  outDir = "dist"
  icons = ["packaging/icons/32x32.png", "packaging/icons/128x128.png", "packaging/icons/256x256.png", "packaging/icons/icon.icns", "packaging/icons/icon.ico"]
  binaries = [{ path = "rustypresenter", main = true }]
  beforePackagingCommand = "cargo build --release --locked"
  resources = [
    { src = "packaging/native/*", target = "." },
    { src = "THIRD-PARTY-LICENSES.txt", target = "licenses/THIRD-PARTY-LICENSES.txt" },
  ]
  [nsis]
  installMode = "currentUser"
  compression = "lzma"
  [macos]
  minimumSystemVersion = "12.0"
  signingIdentity = "-"
  frameworks = ["packaging/native-macos/libmpv.2.dylib", "packaging/native-macos/libpdfium.dylib"]
  [appimage]
  libs = ["libmpv.so.2"]
  [deb]
  depends = ["libmpv2", "libfontconfig1", "libxkbcommon0"]
  ```

  Check the nested-table form against cargo-packager's `schema.json` first. If top-level `[nsis]` is rejected, use `packager.json`.
- `stage.sh <os>`:
  - unpacks the pinned libmpv artifact and the pinned pdfium archive, checking both sha256 values from `pins.env`;
  - copies the libraries to `packaging/native/` (macOS: `packaging/native-macos/`, after `dylibbundler -od -b -x libmpv.2.dylib -d packaging/native-macos -p @loader_path/`, then checked with `otool -L`);
  - on macOS also writes `packaging/native/NATIVE-LIBS.txt` ("The video and PDF libraries are in Contents/Frameworks."), so the `packaging/native/*` resource glob always matches at least one file. A glob that matches nothing may be rejected by cargo-packager; Step 1's dry run and the Task 16 macOS job check this. Dylibs are not put in `Resources`, because code there breaks `codesign --verify --strict`;
  - does **not** regenerate the licences: it checks that the committed root `THIRD-PARTY-LICENSES.txt` exists and contains the LGPL text (Task 14). The CI licence job keeps that file current.
  - On Linux the `.deb` uses the distro `libmpv2`. For the AppImage, our LGPL `libmpv.so.2` is installed to `/usr/local/lib` on the runner, with `ldconfig`, so linuxdeploy can bundle it.
- Runtime: `Mpv::candidates` and `pdfium_candidates` (Task 1) find these files in every layout without a feature flag.

- [ ] **Step 1:** Windows locally: `cargo install cargo-packager --locked --version 0.11.8`, then `packaging/stage.sh windows` and `cargo packager --release -f nsis`. Also do a dry run of the resource list with `packaging/native/` holding only `NATIVE-LIBS.txt` (the macOS layout) to confirm the glob is accepted.
- [ ] **Step 2: Manual check on Windows:**
  - Install for the current user with no admin prompt.
  - The SmartScreen warning appears; click More info → Run anyway.
  - About shows libmpv (LGPL build), pdfium and the licences.
  - Play a video with sound and import a deck through LibreOffice.
  - The program installs to `%LOCALAPPDATA%\Programs\RustyPresenter` (`packaging/windows/installer.nsi`), which differs from the data folder shown in Settings → About.
  - The install's `licenses\` folder holds `THIRD-PARTY-LICENSES.txt` and the `libmpv-*` and `pdfium-*` notices.
  - Uninstall removes the program folder and keeps the data folder.
- [ ] **Step 3: Manual check on macOS (needs a Mac):**
  - `cargo packager --release -f app,dmg` and `codesign --verify --deep --strict -v RustyPresenter.app`.
  - If ad-hoc signing through cargo-packager fails: package with `-f app`, run `codesign --force --deep -s - RustyPresenter.app`, then `hdiutil create -volname RustyPresenter -srcfolder RustyPresenter.app -ov -format UDZO RustyPresenter.dmg`. Put that in `stage.sh mac-finish`.
  - Install from the dmg, then System Settings → Privacy & Security → Open Anyway.
  - About shows libmpv (LGPL build) and pdfium found (the hardened runtime needs `packaging/macos/entitlements.plist` to load them), video plays, and the room shows on a second display.
- [ ] **Step 4: Manual check on Linux (needs a Linux PC; Ubuntu 24.04):**
  - The AppImage runs after `chmod +x`. Check with `--appimage-extract` that `usr/lib` holds `libmpv.so.2` and its dependencies.
  - `sudo apt install ./rustypresenter_*.deb` pulls in `libmpv2`.
  - Video plays in both.
- [ ] **Step 5:** Record the results in `2026-native-checks.md` under a new row "P1 installers".
- [ ] **Step 6:** Commit `build: cargo-packager installers for Windows, macOS and Linux`.

### Task 16: CI — LGPL libmpv, packages, releases

**Files:**
- Create:
  - `.github/workflows/libmpv.yml`:
    - Runs on `workflow_dispatch` and on pushes that change `packaging/libmpv/**`.
    - Jobs: `windows-2025` (MSYS2 UCRT64), `macos-15` (arm64 only; Intel Macs are not supported, see product question 4), `ubuntu-22.04`.
    - Cache key: hash of the build scripts + `pins.env`.
  - `packaging/libmpv/build-windows.sh`, `build-macos.sh`, `build-linux.sh`:
    - Build static FFmpeg 9.0.2 with no `--enable-gpl`/`version3`/`nonfree`; Windows uses SChannel; macOS uses `--disable-network` unless SecureTransport is detected.
    - Build static dav1d, freetype, harfbuzz, fribidi, libass and libplacebo.
    - Build mpv v0.41.0 with `-Dgpl=false -Dcplayer=false -Dlibmpv=true` and disable lua, javascript, libarchive, libbluray, dvdnav, cdda, rubberband, vapoursynth, libavdevice and docs.
    - On macOS also: `-Dcocoa=disabled -Dswift-build=disabled -Dmacos-cocoa-cb=disabled -Dvideotoolbox-gl=enabled`, then `install_name_tool -id @rpath/libmpv.2.dylib`.
    - `build-macos.sh` exports `MACOSX_DEPLOYMENT_TARGET=12.0` at the top, before FFmpeg, every dependency and mpv are built (and passes `-mmacosx-version-min=12.0` where a build system ignores the variable). Without it, a libmpv built on macos-15 does not load on macOS 12–14. A failing check: `otool -l libmpv.2.dylib` shows `minos 12.0` for the library and every bundled dylib.
    - Never install MSYS2's or Homebrew's `ffmpeg`/`mpv` packages.
  - `.github/workflows/package.yml`:
    - Runs on tags `v*` and `workflow_dispatch`.
    - Matrix: `windows-latest` (`nsis`), `macos-14` (`app,dmg`, arm64, `MACOSX_DEPLOYMENT_TARGET=12.0`), `ubuntu-22.04` (`appimage,deb`).
    - Steps: download the pinned libmpv release, `stage.sh`, `cargo packager --release -f …`, and on macOS `codesign --verify --deep --strict`.
    - Uploads `dist/*` and `THIRD-PARTY-LICENSES.txt` to the GitHub Release, together with `libmpv-sources-r<N>.tar.xz` (LGPL §6(d): same place as the download).
- Modify: `.github/workflows/ci.yml`:
  - no new licences job: Plan 3's licence job (changed in Task 14 to run `scripts/licenses.sh`) already covers it
  - a `version-match` step (`Packager.toml` version == `Cargo.toml` version)
  - a Windows job that downloads the libmpv artifact and runs the gated tests with `RP_VIDEO_TESTS=1`

**Interfaces:**
- Artifacts:
  - `libmpv-<os>-<arch>-mpv0.41.0-ff9.0.2-r<N>.zip`, containing the library, `COPYING.LGPL-2.1`, mpv `Copyright`, FFmpeg `COPYING.LGPLv2.1`, the other dependency licences, and `BUILDINFO.txt` (configure/meson lines, versions, commits, patches, `pacman -Q` on Windows).
  - `libmpv-sources-r<N>.tar.xz`.
  - `pins.env` records the release tag and sha256 values.
- **Checks that fail the job:**
  - FFmpeg configure prints `License: LGPL version 2.1 or later`.
  - `config.h` has `#define CONFIG_GPL 0`.
  - mpv's generated `config.h` has its GPL define at 0. Find the exact name on the first run and record it in the script.
  - `strings -a libmpv* | grep -c -- '--enable-gpl'` is 0.
  - `otool -L` (macOS) or `ldd` (Windows/Linux) shows only system libraries, plus any listed bundled DLLs and their licences.

- [ ] **Step 1:** Write the workflows and scripts.
- [ ] **Step 2: Manual:** trigger `libmpv.yml`; iterate until all three jobs are green. The macOS recipe has never run, so expect fixes. Publish release `libmpv-r1`; update `pins.env`.
- [ ] **Step 3:** Push a test tag `v0.2.0-rc1`; `package.yml` produces all five files. Download each and repeat the Task 15 manual checks on the CI-built packages.
- [ ] **Step 4:** Commit `ci: LGPL libmpv build, installer packaging and releases`.

### Task 17: Records and per-platform manual checklist

**Files:**
- Modify:
  - `README.md`:
    - install and first launch per OS for unsigned builds:
      - Windows SmartScreen: More info → Run anyway. Smart App Control blocks it outright, so say so.
      - macOS 15+: Privacy & Security → Open Anyway, or `xattr -dr com.apple.quarantine`.
      - Linux: `chmod +x`, `libfuse2`.
      - Windows Firewall: on the first run of the installed exe, Windows asks whether RustyPresenter may use the network. Tick **Private networks** and click Allow, or phone remotes cannot connect. Do this before pairing a phone.
    - LibreOffice install hints.
    - the LGPL notice, plus how to replace libmpv: `RP_LIBMPV`, or swap the file next to the exe / in Frameworks.
    - the Slint attribution badge (spec §10b).
  - `PROJECT_PLAN.md`: status line, NATIVE-P4 done, handoff, open product questions 1–5.
  - `docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md`: in the header **Status**, add "Platform sections superseded by the native app (2026-10-29-native-slint-design.md); v0.2 Tauri app removed November 2026", followed by a list naming each one, so the v1 acceptance matrix cannot be read as covered:
    - §4.2–4.3 recovery: replaced by native §8.3 (supervisor restart; media resumes).
    - §6.4 FFmpeg conversion and the Ready/Converting/Needs converter library states: replaced by mpv playback and probing; no conversion.
    - §9.4 PowerPoint-only export: replaced by PowerPoint, LibreOffice or pdfium (native §9).
    - §16 items "LibreOffice conversion", "macOS and Linux" and "ffmpeg auto-transcode": the first two are done, the third is dropped.
    - §9.3 EasyWorship and §9.5 Google Slides: **still not built**.
    In §19, add a dated note that architecture changes 1 and 2 (Tauri windows, FFmpeg conversion) are replaced by the native app and mpv playback. Keep the rest as history.
  - `docs/superpowers/specs/2026-10-29-native-slint-design.md`: Status "built".
  - `docs/superpowers/checks/2026-native-checks.md`: a per-platform table from spec §13.

- [ ] **Step 1:** Write the records.
- [ ] **Step 2: Manual checklist**, one column each for Windows, macOS and Linux. **Each needs a person on that OS; Windows also needs the church laptop with a projector.**
  - Install from the release.
  - First-launch warning steps match the README.
  - Windows: the Firewall "allow on private networks" prompt appears on the first run of the installed exe, before phone pairing, and the README step works.
  - Second launch while running focuses the running operator window (AppImage and `.deb` especially).
  - Closing the operator window ends the app **and** the supervisor (no process left; it does not restart).
  - Room on a second monitor; unplug and replug.
  - Video formats with sound: H.264, HEVC, VP9, ProRes, WMV, MP3, AAC.
  - v1 §6.4 compatibility folder: phone HEVC portrait/VFR, AV1, 4K, a video with no audio, and a corrupt file. Each plays, loops and stops cleanly; the corrupt file gives an operator error at import and is never black on air.
  - v1 §14.2 Audio row: sound to HDMI and to a USB audio interface; unplug and replug each while audio plays. There is no device picker (product question 5); record what actually happens.
  - v1 §14.2 Assets/backups row: restore a backup into a clean data folder and play its media; delete a library entry while it is live (output stays valid).
  - Phone pairing from a real phone.
  - PowerPoint through PowerPoint (Windows).
  - PowerPoint through LibreOffice (all three).
  - Keynote and PDF decks.
  - Software video forced with `RP_FORCE_SOFTWARE=1`.
  - Crash restore by killing the app.
  - Three crashes in a minute: the give-up dialog with the log path appears. On macOS and Linux it is only printed to stderr until `rfd` (Plan 3 Task 15) is in the build; then switch `recovery::supervisor::give_up_dialog` to `rfd::MessageDialog` there and check it.
  - Logout with the app open.
  - Uninstall keeps the data folder.
  - About shows the LGPL libmpv and opens the licences.
- [ ] **Step 3:** Commit `docs: Plan 4 results, install guide, v1 spec status`.

---

**After this plan:** the native app is the product. The next steps are code signing (Apple Developer ID plus notarization through cargo-packager's `signingIdentity`; Windows Artifact Signing through `windows.signCommand`), and a hang watchdog (heartbeat from the app, killed by the supervisor after about 15 s), which §8.3 does not cover.
