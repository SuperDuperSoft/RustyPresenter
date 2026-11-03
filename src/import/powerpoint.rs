//! Slides to pictures through the installed PowerPoint (Plan 4 Task 6b), on Windows only, one import at a time.
//! PowerPoint runs as one instance per user, so the import shares it with whatever the operator has open: it opens
//! its own copy read-only and without a window, and closes only that. Nothing here ever kills PowerPoint itself:
//! if the clean-up after a stopped import fails, a hidden PowerPoint is left running rather than risk the user's work.
use std::time::Duration;
use tokio::sync::Semaphore;
#[cfg(windows)]
use super::{CANCELLED, pdf::output_size, strings};
#[cfg(windows)]
use crate::platform::{proc::{Stop, spawn_tree}, tools};
#[cfg(windows)]
use std::{ffi::OsString, path::{Path, PathBuf}, process::{ExitStatus, Stdio}};
#[cfg(windows)]
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, process::Command, sync::SemaphorePermit, time::Instant};
#[cfg(windows)]
use tokio_util::sync::CancellationToken;

/// Exports the visible slides of `-Path` to `-Out\slide-001.png`…, printing what it does (see `Line`). After
/// `SIZE` it reads the picture size `W H` from stdin: `export` answers with `output_size`, so one rule decides it.
/// PowerShell 5.1 reads it as ANSI, so it stays ASCII.
pub const SCRIPT: &str = r#"
param([string]$Path, [string]$Out)
$ErrorActionPreference = 'Stop'
function Say([string]$s) { [Console]::Out.WriteLine($s); [Console]::Out.Flush() }
# PowerPoint is one instance per user, and New-Object attaches to it when it runs. We started it only if it was
# not running and had nothing open. (A PowerPoint started by automation is not in the running object table, so
# the process list is asked, read only.)
$session = (Get-Process -Id $PID).SessionId
$wasRunning = [bool](Get-Process POWERPNT -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq $session })
$app = New-Object -ComObject PowerPoint.Application
# Anything that fails before this line means PowerPoint is missing or broken, not that the file is bad.
Say 'ATTACHED'
$hadOpen = $app.Presentations.Count
$startedByUs = (-not $wasRunning) -and ($hadOpen -eq 0)
if ($startedByUs) { Say 'STARTED_BY_US' }
$security = $app.AutomationSecurity
# msoAutomationSecurityForceDisable: a .pptm opened from here never runs its macros.
$app.AutomationSecurity = 3
$pres = $null
try {
  # ReadOnly, Untitled, WithWindow = false. The "::x::" password makes a protected file fail instead of asking.
  $pres = $app.Presentations.Open($Path + '::x::', -1, 0, 0)
  Say ('COUNT ' + $pres.Slides.Count)
  $sw = [double]$pres.PageSetup.SlideWidth
  $sh = [double]$pres.PageSetup.SlideHeight
  $inv = [Globalization.CultureInfo]::InvariantCulture
  Say ('SIZE ' + $sw.ToString($inv) + ' ' + $sh.ToString($inv))
  # The picture size from import::pdf::output_size. Rounding it here as well would disagree at half-way points.
  $reply = [Console]::In.ReadLine()
  if ($reply -eq $null) { throw 'no picture size' }
  $wh = $reply.Trim().Split(' ')
  if ($wh.Count -ne 2) { throw ('bad picture size ' + $reply) }
  $outW = [int]$wh[0]
  $outH = [int]$wh[1]
  if ($outW -le 0 -or $outH -le 0) { throw ('bad picture size ' + $reply) }
  $shown = @($pres.Slides | Where-Object { -not $_.SlideShowTransition.Hidden })
  $i = 0
  foreach ($s in $shown) {
    $i++
    $file = Join-Path $Out ('slide-{0:D3}.png' -f $i)
    $s.Export($file, 'PNG', $outW, $outH)
    Say ('SLIDE ' + $i + ' ' + $shown.Count)
  }
} finally {
  if ($pres -ne $null) { $pres.Close() }
  try { $app.AutomationSecurity = $security } catch {}
  # A presentation the user opened meanwhile lives in this same PowerPoint, so count again. A file opened in
  # Protected View (an e-mail attachment) is not among Presentations, and a PowerPoint the user brought up is visible.
  if ($startedByUs -and $app.Presentations.Count -eq 0 -and $app.ProtectedViewWindows.Count -eq 0 -and -not $app.Visible) { $app.Quit() }
}
"#;

/// Run after the export was stopped (its PowerShell is already dead, but PowerPoint runs outside that tree and
/// still has our copy open). Closes only the presentation opened from `-Path`.
pub const CLEANUP_SCRIPT: &str = r#"
param([string]$Path, [switch]$StartedByUs)
$ErrorActionPreference = 'Stop'
# GetActiveObject cannot find a PowerPoint started by automation (it is not in the running object table), so
# attach with New-Object, which joins the one running instance, but only when one runs: otherwise it would start one.
$session = (Get-Process -Id $PID).SessionId
if (-not (Get-Process POWERPNT -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq $session })) { exit 0 }
$app = New-Object -ComObject PowerPoint.Application
foreach ($p in @($app.Presentations)) {
  if ($p.FullName -eq $Path) { $p.Close() }
}
if ($StartedByUs -and $app.Presentations.Count -eq 0 -and $app.ProtectedViewWindows.Count -eq 0 -and -not $app.Visible) { $app.Quit() }
"#;

#[cfg(windows)]
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(15);

/// One line the export script prints.
#[derive(Debug, PartialEq)]
pub enum Line {
    /// PowerPoint answered: an export that ends before this could not start PowerPoint.
    Attached,
    /// Slides in the file, hidden ones included. Sets the time allowed.
    Count(usize),
    /// Slide width and height in points.
    Size(f32, f32),
    /// Visible slide `i` of `n` is saved.
    Slide(usize, usize),
    /// PowerPoint was not running before, so the import may quit it when nothing else is open.
    StartedByUs,
}

/// Reads one line of the export script's output. Anything else PowerShell prints is `None`.
pub fn parse_line(s: &str) -> Option<Line> {
    let mut words = s.split_whitespace();
    let line = match words.next()? {
        "ATTACHED" => Line::Attached,
        "STARTED_BY_US" => Line::StartedByUs,
        "COUNT" => Line::Count(words.next()?.parse().ok()?),
        "SIZE" => {
            let w: f32 = words.next()?.parse().ok()?;
            let h: f32 = words.next()?.parse().ok()?;
            let good = |n: f32| n.is_finite() && n > 0.0;
            if !good(w) || !good(h) {
                return None;
            }
            Line::Size(w, h)
        }
        "SLIDE" => {
            let i: usize = words.next()?.parse().ok()?;
            let n: usize = words.next()?.parse().ok()?;
            if i == 0 || i > n {
                return None;
            }
            Line::Slide(i, n)
        }
        _ => return None,
    };
    words.next().is_none().then_some(line)
}

pub struct PowerPoint {
    /// One permit: every import shares the user's single PowerPoint.
    #[cfg_attr(not(windows), allow(dead_code))]
    gate: Semaphore,
    #[cfg(all(test, windows))]
    fake: Option<Fake>,
}

/// PowerShell's stand-in in tests: the test binary run again (`tests::fake_powershell`).
#[cfg(all(test, windows))]
#[derive(Clone)]
struct Fake {
    exe: std::path::PathBuf,
    prefix_args: Vec<std::ffi::OsString>,
    env: Vec<(String, String)>,
    /// Replaces `timeout_for`: a base plus a time per slide, so a hang stops in milliseconds.
    timeout: Option<(Duration, Duration)>,
}

impl PowerPoint {
    pub fn new() -> PowerPoint {
        PowerPoint {
            gate: Semaphore::new(1),
            #[cfg(all(test, windows))]
            fake: None,
        }
    }

    /// 60 s plus 10 s per slide, at most 600 s. Before `COUNT` arrives the slide count is 0.
    pub fn timeout_for(slides: usize) -> Duration {
        Duration::from_secs((60 + 10 * slides as u128).min(600) as u64)
    }
}

#[cfg(windows)]
impl PowerPoint {
    fn timeout(&self, slides: usize) -> Duration {
        #[cfg(test)]
        if let Some((base, per)) = self.fake.as_ref().and_then(|f| f.timeout) {
            return base + per * slides as u32;
        }
        Self::timeout_for(slides)
    }

    /// Saves each visible slide of `input` as `out/slide-001.png`…, sized by `output_size`. The scripts and
    /// PowerShell's log go in `out`'s parent, the import's work folder. `waiting(true)` is called if another
    /// import holds PowerPoint, and `waiting(false)` when this one gets it. `progress(i, n)` follows each slide.
    /// Stop it through `cancel`: a dropped future still closes our copy, but without waiting for that (`Unfinished`).
    pub async fn export(&self, input: &Path, out: &Path, waiting: &(dyn Fn(bool) + Sync), progress: &(dyn Fn(usize, usize) + Sync), cancel: &CancellationToken) -> Result<Vec<PathBuf>, String> {
        let _permit = self.enter(waiting, cancel).await?;
        let work = out.parent().unwrap_or(out);
        std::fs::create_dir_all(out).map_err(not_started)?;
        let script = work.join("powerpoint-export.ps1");
        std::fs::write(&script, SCRIPT).map_err(not_started)?;
        let log = work.join("powerpoint.log");
        let mut cmd = self.command();
        cmd.args(self.args(&script, &[("-Path", input), ("-Out", out)]));
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(std::fs::File::create(&log).map_err(not_started)?);
        let began = Instant::now();
        // Declared before the tree, so a dropped export kills PowerShell first and then closes our copy.
        let mut unfinished = Unfinished { ppt: self, work, input, started: false, armed: false };
        let mut tree = spawn_tree(Command::from(cmd)).map_err(not_started)?;
        unfinished.armed = true;
        let stdout = tree.stdout().ok_or_else(|| not_started("no output pipe"))?;
        let mut stdin = tree.stdin();
        let mut lines = BufReader::new(stdout).lines();
        let mut seen = Seen::default();
        let mut deadline = began + self.timeout(0);
        let stopped = loop {
            let line = tokio::select! {
                biased;
                _ = cancel.cancelled() => break Some(Stop::Cancelled),
                _ = tokio::time::sleep_until(deadline) => break Some(Stop::Timeout),
                line = lines.next_line() => line,
            };
            let Ok(Some(line)) = line else { break None };
            match parse_line(&line) {
                Some(Line::Attached) => seen.attached = true,
                Some(Line::StartedByUs) => {
                    seen.started = true;
                    unfinished.started = true;
                }
                Some(Line::Count(n)) => {
                    seen.count = Some(n);
                    deadline = began + self.timeout(n);
                }
                Some(Line::Size(w, h)) => {
                    let size = output_size(w / h);
                    seen.size = Some(size);
                    // The script waits for this answer. If it cannot be sent, the closed pipe makes the script fail.
                    if let Some(mut pipe) = stdin.take() {
                        let _ = pipe.write_all(format!("{} {}\n", size.0, size.1).as_bytes()).await;
                        let _ = pipe.flush().await;
                    }
                }
                Some(Line::Slide(i, n)) => {
                    seen.slides = (i, n);
                    progress(i, n);
                }
                // A SIZE line that cannot be read gets no answer: closing stdin ends the script's wait at once.
                None if line.trim_start().starts_with("SIZE") => drop(stdin.take()),
                None => {}
            }
        };
        let ended = match stopped {
            None => tree.wait(deadline.saturating_duration_since(Instant::now()), cancel).await,
            Some(stop) => Err(stop),
        };
        drop(tree);
        match ended {
            Ok(status) => {
                unfinished.armed = false;
                finish(out, status, &seen, &log)
            }
            Err(stop) => {
                // Still holding the turn, so the next import cannot open its copy while ours is being closed.
                self.cleanup(work, input, seen.started).await;
                unfinished.armed = false;
                Err(match stop {
                    Stop::Timeout => strings::PPT_TIMEOUT,
                    Stop::Cancelled => CANCELLED,
                    Stop::Io(_) => strings::PPT_CANNOT_READ,
                }
                .to_string())
            }
        }
    }

    async fn enter(&self, waiting: &(dyn Fn(bool) + Sync), cancel: &CancellationToken) -> Result<SemaphorePermit<'_>, String> {
        if cancel.is_cancelled() {
            return Err(CANCELLED.to_string());
        }
        if let Ok(permit) = self.gate.try_acquire() {
            return Ok(permit);
        }
        waiting(true);
        let permit = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(CANCELLED.to_string()),
            permit = self.gate.acquire() => permit.map_err(not_started)?,
        };
        waiting(false);
        Ok(permit)
    }

    /// Closes our copy after a stopped export, and quits PowerPoint only if the export started it and nothing
    /// else is open. A failure is logged and PowerPoint is left alone.
    async fn cleanup(&self, work: &Path, input: &Path, started_by_us: bool) {
        let Some(cmd) = self.cleanup_command(work, input, started_by_us) else { return };
        let ended = match spawn_tree(Command::from(cmd)) {
            Ok(mut tree) => tree.wait(CLEANUP_TIMEOUT, &CancellationToken::new()).await.map_err(|stop| format!("{stop:?}")),
            Err(e) => Err(e.to_string()),
        };
        match ended {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("PowerPoint clean-up ended with {status}; PowerPoint may still be running hidden."),
            Err(e) => eprintln!("PowerPoint clean-up failed ({e}); PowerPoint may still be running hidden."),
        }
    }

    /// The clean-up's PowerShell, or `None` (logged) if its script cannot be written.
    fn cleanup_command(&self, work: &Path, input: &Path, started_by_us: bool) -> Option<std::process::Command> {
        let script = work.join("powerpoint-cleanup.ps1");
        if let Err(e) = std::fs::write(&script, CLEANUP_SCRIPT) {
            eprintln!("PowerPoint clean-up could not be written: {e}");
            return None;
        }
        let mut cmd = self.command();
        cmd.args(self.args(&script, &[("-Path", input)]));
        if started_by_us {
            cmd.arg("-StartedByUs");
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        Some(cmd)
    }

    /// A std command, so `Unfinished` can start one outside the runtime; `Command::from` makes it async.
    fn command(&self) -> std::process::Command {
        use std::process::Command;
        #[cfg(test)]
        if let Some(fake) = &self.fake {
            let mut cmd = Command::new(&fake.exe);
            cmd.args(&fake.prefix_args).envs(fake.env.iter().map(|(k, v)| (k, v)));
            return cmd;
        }
        // By full path, so a `powershell.exe` elsewhere on PATH is never the one started.
        let system = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        Command::new(system.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
    }

    /// PowerShell's arguments: the script, then each `-Name path` with `\\?\` stripped (PowerPoint cannot open it).
    fn args(&self, script: &Path, params: &[(&str, &Path)]) -> Vec<OsString> {
        let mut args: Vec<OsString> = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"].map(OsString::from).to_vec();
        args.push(plain(script));
        for (name, path) in params {
            args.push(name.into());
            args.push(plain(path));
        }
        args
    }
}

impl Default for PowerPoint {
    fn default() -> Self {
        Self::new()
    }
}

/// An export whose PowerShell was started and not yet waited for or cleaned up. If the export future is dropped
/// (runtime shut down, or a caller that drops instead of cancelling), PowerShell dies with its tree but PowerPoint
/// still has our copy open, so this starts the clean-up on its own thread, with the same 15 s limit. The turn is
/// already given back by then, which is why `export` should be stopped through its token.
#[cfg(windows)]
struct Unfinished<'a> {
    ppt: &'a PowerPoint,
    work: &'a Path,
    input: &'a Path,
    started: bool,
    armed: bool,
}

#[cfg(windows)]
impl Drop for Unfinished<'_> {
    fn drop(&mut self) {
        use std::os::windows::process::CommandExt;
        if !self.armed {
            return;
        }
        let Some(mut cmd) = self.ppt.cleanup_command(self.work, self.input, self.started) else { return };
        cmd.creation_flags(crate::platform::proc::creation_flags());
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => return eprintln!("PowerPoint clean-up failed ({e}); PowerPoint may still be running hidden."),
        };
        std::thread::spawn(move || {
            let began = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) if status.success() => return,
                    Ok(Some(status)) => return eprintln!("PowerPoint clean-up ended with {status}; PowerPoint may still be running hidden."),
                    Ok(None) if began.elapsed() < CLEANUP_TIMEOUT => std::thread::sleep(Duration::from_millis(50)),
                    Ok(None) | Err(_) => {
                        let _ = child.kill();
                        return eprintln!("PowerPoint clean-up did not finish; PowerPoint may still be running hidden.");
                    }
                }
            }
        });
    }
}

/// What the export script said before it ended.
#[cfg(windows)]
#[derive(Default)]
struct Seen {
    attached: bool,
    started: bool,
    count: Option<usize>,
    /// The picture size sent back after `SIZE`.
    size: Option<(u32, u32)>,
    /// The last `SLIDE i n`.
    slides: (usize, usize),
}

/// The pictures of an export that ended by itself, checked against what the script said.
#[cfg(windows)]
fn finish(out: &Path, status: ExitStatus, seen: &Seen, log: &Path) -> Result<Vec<PathBuf>, String> {
    let failed = |why: &str| {
        let said = std::fs::read(log).unwrap_or_default();
        let said = String::from_utf8_lossy(&said);
        eprintln!("PowerPoint export failed ({why}): {}", said.lines().find(|l| !l.trim().is_empty()).unwrap_or(""));
        Err(strings::PPT_CANNOT_READ.to_string())
    };
    if !seen.attached {
        let said = std::fs::read(log).unwrap_or_default();
        return Err(not_started(String::from_utf8_lossy(&said).lines().find(|l| !l.trim().is_empty()).unwrap_or("PowerShell ended at once")));
    }
    let (Some(_), Some(size)) = (seen.count, seen.size) else { return failed("no slide count or size") };
    if !status.success() {
        return failed(&status.to_string());
    }
    let (done, total) = seen.slides;
    if total == 0 {
        return Err(strings::PDF_NO_SLIDES.to_string());
    }
    if done != total {
        return failed("not every slide was saved");
    }
    let pngs: Vec<PathBuf> = (1..=total).map(|i| out.join(format!("slide-{i:03}.png"))).collect();
    for png in &pngs {
        match image::image_dimensions(png) {
            Ok(got) if got == size => {}
            got => return failed(&format!("{} is {got:?}, not {size:?}", png.display())),
        }
    }
    Ok(pngs)
}

#[cfg(windows)]
fn not_started(e: impl std::fmt::Display) -> String {
    eprintln!("PowerPoint could not start: {e}");
    strings::PPT_NOT_STARTED.to_string()
}

/// A path as PowerPoint can open it: `\\?\` confuses it.
#[cfg(windows)]
fn plain(path: &Path) -> OsString {
    path.to_str().map_or_else(|| path.into(), |p| tools::strip_verbatim(p).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_line_reads_count_size_slide_and_marker() {
        assert_eq!(parse_line("COUNT 12"), Some(Line::Count(12)));
        assert_eq!(parse_line("SIZE 960 540"), Some(Line::Size(960.0, 540.0)));
        assert_eq!(parse_line("SIZE 720.5 540.25\r"), Some(Line::Size(720.5, 540.25)));
        assert_eq!(parse_line("SLIDE 2 3"), Some(Line::Slide(2, 3)));
        assert_eq!(parse_line("STARTED_BY_US"), Some(Line::StartedByUs));
        assert_eq!(parse_line("ATTACHED"), Some(Line::Attached));
        assert_eq!(parse_line("  STARTED_BY_US  "), Some(Line::StartedByUs));
        for junk in ["", "COUNT", "COUNT x", "SIZE 960", "SIZE 960,5 540", "SIZE 0 540", "SIZE 960 -1", "SLIDE 1", "SLIDE 4 3", "SLIDE 0 3", "slide 1 3", "WARNING: something", "COUNT 3 4", "ATTACHED 1"] {
            assert_eq!(parse_line(junk), None, "{junk:?}");
        }
    }

    /// Each `if` line of the script that quits PowerPoint.
    fn quit_guards(script: &str) -> Vec<&str> {
        script.lines().filter(|l| l.contains("Quit()")).collect()
    }

    #[test]
    fn script_never_quits_powerpoint_when_presentations_were_open() {
        let guards = quit_guards(SCRIPT);
        assert_eq!(guards.len(), 1, "{guards:?}");
        for line in guards {
            let line = line.trim();
            assert!(line.starts_with("if ("), "{line}");
            let cond = &line[..line.find('{').unwrap()];
            assert!(cond.contains("$startedByUs") && cond.contains("Presentations.Count -eq 0"), "{line}");
            // Protected View windows are not presentations, and a visible PowerPoint is the user's.
            assert!(cond.contains("ProtectedViewWindows.Count -eq 0") && cond.contains("-not $app.Visible"), "{line}");
        }
        // PowerPoint answered before anything about the file is tried.
        let attached = SCRIPT.find("Say 'ATTACHED'").unwrap();
        assert!(SCRIPT.find("New-Object -ComObject PowerPoint.Application").unwrap() < attached && attached < SCRIPT.find(".Open(").unwrap());
        // The picture size is read back after SIZE, never rounded here (import::pdf::output_size decides it).
        let size = SCRIPT.find("Say ('SIZE '").unwrap();
        let reply = SCRIPT.find("[Console]::In.ReadLine()").unwrap();
        assert!(size < reply && reply < SCRIPT.find(".Export(").unwrap());
        assert!(!SCRIPT.contains("Round("));
        // The flag means nothing of the user's was open when the import started.
        let flag = SCRIPT.lines().find(|l| l.trim_start().starts_with("$startedByUs =")).unwrap();
        assert!(flag.contains("$hadOpen -eq 0"), "{flag}");
        assert!(SCRIPT.contains("if ($startedByUs) { Say 'STARTED_BY_US' }"));
        // Macros are off before the file is opened, and it is opened read-only, windowless, with a password.
        let security = SCRIPT.find("$app.AutomationSecurity = 3").unwrap();
        let open = SCRIPT.find(".Open(").unwrap();
        assert!(security < open);
        assert!(SCRIPT.contains(".Open($Path + '::x::', -1, 0, 0)"));
        // Only our own presentation is closed.
        assert_eq!(SCRIPT.matches(".Close()").count(), 1);
        assert!(SCRIPT.contains("$pres.Close()"));
        assert!(SCRIPT.contains("'slide-{0:D3}.png'") && SCRIPT.contains("$s.Export($file, 'PNG', $outW, $outH)"));
        assert!(SCRIPT.contains("SlideShowTransition.Hidden"));
    }

    #[test]
    fn cleanup_script_closes_only_our_presentation() {
        // No PowerPoint in this session: nothing to close, and attaching would start one.
        let gate = CLEANUP_SCRIPT.lines().find(|l| l.contains("exit 0")).unwrap();
        assert!(gate.contains("if (-not (Get-Process POWERPNT -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq $session }))"), "{gate}");
        assert!(CLEANUP_SCRIPT.find("exit 0").unwrap() < CLEANUP_SCRIPT.find("New-Object -ComObject PowerPoint.Application").unwrap());
        let close = CLEANUP_SCRIPT.lines().find(|l| l.contains(".Close()")).unwrap();
        assert!(close.contains("if ($p.FullName -eq $Path)"), "{close}");
        assert_eq!(CLEANUP_SCRIPT.matches(".Close()").count(), 1);
        let guards = quit_guards(CLEANUP_SCRIPT);
        assert_eq!(guards.len(), 1, "{guards:?}");
        assert!(guards[0].trim().starts_with("if ($StartedByUs -and $app.Presentations.Count -eq 0 -and $app.ProtectedViewWindows.Count -eq 0 -and -not $app.Visible)"), "{}", guards[0]);
        for script in [SCRIPT, CLEANUP_SCRIPT] {
            let lower = script.to_lowercase();
            for banned in ["taskkill", "stop-process", "kill", "terminate"] {
                assert!(!lower.contains(banned), "{banned}");
            }
            // PowerPoint's process is only ever looked at, in this session.
            for line in script.lines().filter(|l| l.contains("POWERPNT")) {
                assert!(line.contains("Get-Process POWERPNT -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq $session }"), "{line}");
            }
            // GetActiveObject cannot see a PowerPoint started by automation (MK_E_UNAVAILABLE), so neither relies on it.
            assert!(!script.contains("GetActiveObject("));
            // Windows PowerShell 5.1 reads a script without a byte order mark as ANSI.
            assert!(script.is_ascii());
        }
    }

    #[test]
    fn timeout_scales_with_slides_and_caps_at_600s() {
        assert_eq!(PowerPoint::timeout_for(0), Duration::from_secs(60));
        assert_eq!(PowerPoint::timeout_for(3), Duration::from_secs(90));
        assert_eq!(PowerPoint::timeout_for(54), Duration::from_secs(600));
        assert_eq!(PowerPoint::timeout_for(1000), Duration::from_secs(600));
        assert_eq!(PowerPoint::timeout_for(usize::MAX), Duration::from_secs(600));
    }

    #[cfg(windows)]
    fn now_ms() -> u128 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
    }

    #[cfg(windows)]
    const NAME: &str = "import::powerpoint::tests::fake_powershell";
    #[cfg(windows)]
    const FAKE: &str = "RP_FAKE_PPT";
    #[cfg(windows)]
    const MARK: &str = "RP_FAKE_PPT_MARK";

    /// Not a real test: PowerShell's stand-in, this test binary run again. The export run follows the steps in
    /// `RP_FAKE_PPT`, comma separated: `started`, `count:<n>`, `size:<w>:<h>` (points; then reads the picture
    /// size from stdin, as the script does), `slides:<n>[:<w>:<h>]` (writes n pictures of w×h pixels, by default
    /// the size it was sent, each followed by its `SLIDE` line), `sleep:<ms>`, `hang`, `fail`. It prints
    /// `ATTACHED` first unless the first step is `comfail`, which fails as `New-Object` does without PowerPoint.
    /// The export run adds `export <its arguments after -File>`, `start <ms>`, `picture <w> <h>` and `end <ms>` lines
    /// to the file in `RP_FAKE_PPT_MARK`; the clean-up run adds `cleanup <its arguments after -File>`.
    #[cfg(windows)]
    #[test]
    fn fake_powershell() {
        use std::io::Write;
        let Ok(steps) = std::env::var(FAKE) else { return };
        let mark = std::env::var(MARK).unwrap();
        let log = |line: String| writeln!(std::fs::OpenOptions::new().create(true).append(true).open(&mark).unwrap(), "{line}").unwrap();
        let args: Vec<String> = std::env::args().collect();
        let at = args.iter().position(|a| a == "-File").unwrap();
        if args[at + 1].ends_with("powerpoint-cleanup.ps1") {
            log(format!("cleanup {}", args[at + 2..].join(" ")));
            std::process::exit(0);
        }
        log(format!("export {}", args[at + 2..].join(" ")));
        let out = PathBuf::from(&args[args.iter().position(|a| a == "-Out").unwrap() + 1]);
        log(format!("start {}", now_ms()));
        if steps.starts_with("comfail") {
            eprintln!("New-Object : Retrieving the COM class factory for component with CLSID {{}} failed: 80040154");
            std::process::exit(1);
        }
        println!("ATTACHED");
        let mut picture = (0u32, 0u32);
        for step in steps.split(',').filter(|s| !s.is_empty()) {
            let parts: Vec<&str> = step.split(':').collect();
            match parts[0] {
                "started" => println!("STARTED_BY_US"),
                "count" => println!("COUNT {}", parts[1]),
                "size" => {
                    println!("SIZE {} {}", parts[1], parts[2]);
                    std::io::stdout().flush().unwrap();
                    let mut reply = String::new();
                    std::io::stdin().read_line(&mut reply).unwrap();
                    let wh: Vec<u32> = reply.split_whitespace().map(|n| n.parse().unwrap()).collect();
                    picture = (wh[0], wh[1]);
                    log(format!("picture {} {}", wh[0], wh[1]));
                }
                "slides" => {
                    let n: usize = parts[1].parse().unwrap();
                    let (w, h) = if parts.len() > 2 { (parts[2].parse().unwrap(), parts[3].parse().unwrap()) } else { picture };
                    for i in 1..=n {
                        image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 30])).save(out.join(format!("slide-{i:03}.png"))).unwrap();
                        println!("SLIDE {i} {n}");
                    }
                }
                "sleep" => std::thread::sleep(Duration::from_millis(parts[1].parse().unwrap())),
                "hang" => std::thread::sleep(Duration::from_secs(60)),
                "fail" => {
                    eprintln!("Exception calling \"Open\" with \"4\" argument(s)");
                    std::process::exit(1);
                }
                _ => panic!("unknown step {step}"),
            }
        }
        log(format!("end {}", now_ms()));
        std::process::exit(0);
    }

    #[cfg(windows)]
    struct Fx {
        dir: PathBuf,
        ppt: PowerPoint,
        mark: PathBuf,
        input: PathBuf,
        out: PathBuf,
    }

    #[cfg(windows)]
    impl Drop for Fx {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[cfg(windows)]
    fn fixture(steps: &str) -> Fx {
        let dir = std::env::temp_dir().join(format!("rp-ppt-{}", uuid::Uuid::new_v4()));
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let input = work.join("input.pptx");
        std::fs::write(&input, "deck").unwrap();
        let mark = dir.join("mark");
        let mut ppt = PowerPoint::new();
        ppt.fake = Some(Fake {
            exe: std::env::current_exe().unwrap(),
            prefix_args: ["--exact", NAME, "--nocapture", "--"].map(std::ffi::OsString::from).to_vec(),
            env: vec![(FAKE.into(), steps.into()), (MARK.into(), mark.display().to_string())],
            timeout: None,
        });
        Fx { out: work.join("slides"), dir, ppt, mark, input }
    }

    #[cfg(windows)]
    fn marks(mark: &Path, word: &str) -> Vec<String> {
        let text = std::fs::read_to_string(mark).unwrap_or_default();
        text.lines().filter_map(|l| Some(l.strip_prefix(word)?.trim().to_string())).collect()
    }

    #[cfg(windows)]
    fn times(mark: &Path, word: &str) -> Vec<u128> {
        marks(mark, word).iter().map(|t| t.parse().unwrap()).collect()
    }

    #[cfg(windows)]
    fn quiet(_: bool) {}
    #[cfg(windows)]
    fn no_progress(_: usize, _: usize) {}

    #[cfg(windows)]
    #[tokio::test]
    async fn export_reports_progress_and_returns_slides_in_order() {
        let fx = fixture("started,count:3,size:960:540,slides:3:1920:1080");
        let seen = std::sync::Mutex::new(Vec::new());
        let progress = |i: usize, n: usize| seen.lock().unwrap().push((i, n));
        let pngs = fx.ppt.export(&fx.input, &fx.out, &quiet, &progress, &CancellationToken::new()).await.unwrap();
        assert_eq!(pngs, ["slide-001.png", "slide-002.png", "slide-003.png"].map(|n| fx.out.join(n)));
        assert_eq!(*seen.lock().unwrap(), vec![(1, 3), (2, 3), (3, 3)]);
        assert!(marks(&fx.mark, "cleanup").is_empty(), "a finished export needs no clean-up");
        // The scripts live in the work folder, not among the pictures.
        assert!(fx.out.parent().unwrap().join("powerpoint-export.ps1").is_file());
        assert_eq!(std::fs::read_dir(&fx.out).unwrap().count(), 3);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn export_passes_plain_paths_to_powershell() {
        let fx = fixture("fail");
        let verbatim = PathBuf::from(format!(r"\\?\{}", fx.input.display()));
        let _ = fx.ppt.export(&verbatim, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await;
        let log = std::fs::read_to_string(fx.out.parent().unwrap().join("powerpoint.log")).unwrap();
        assert!(log.contains("Exception calling"), "{log}");
        // What the export run was really given.
        assert_eq!(marks(&fx.mark, "export"), vec![format!("-Path {} -Out {}", fx.input.display(), fx.out.display())]);
        let args = fx.ppt.args(&fx.out.parent().unwrap().join("x.ps1"), &[("-Path", verbatim.as_path())]);
        let text: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        for s in ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"] {
            assert!(text.iter().any(|a| a == s), "{s}");
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn second_export_waits_for_first() {
        let fx = fixture("count:1,size:960:540,sleep:800,slides:1:1920:1080");
        let cancel = CancellationToken::new();
        let seen = std::sync::Mutex::new(Vec::new());
        let note = |w: bool| seen.lock().unwrap().push(w);
        let out2 = fx.dir.join("work2").join("slides");
        let first = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &cancel);
        let second = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            fx.ppt.export(&fx.input, &out2, &note, &no_progress, &cancel).await
        };
        let (a, b) = tokio::join!(first, second);
        assert_eq!(a.unwrap().len(), 1);
        assert_eq!(b.unwrap().len(), 1);
        let (starts, ends) = (times(&fx.mark, "start"), times(&fx.mark, "end"));
        assert_eq!((starts.len(), ends.len()), (2, 2));
        assert!(starts[1] >= ends[0], "the runs overlapped: {starts:?} {ends:?}");
        assert_eq!(*seen.lock().unwrap(), vec![true, false]);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn cancel_while_waiting_never_starts_powershell() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let fx = fixture("count:1,size:960:540,sleep:1500,slides:1:1920:1080");
        let free = CancellationToken::new();
        let cancel = CancellationToken::new();
        let out2 = fx.dir.join("work2").join("slides");
        let (is_waiting, got_turn) = (AtomicBool::new(false), AtomicBool::new(false));
        let note = |w: bool| if w { is_waiting.store(true, Ordering::SeqCst) } else { got_turn.store(true, Ordering::SeqCst) };
        let first = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &free);
        let second = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let r = fx.ppt.export(&fx.input, &out2, &note, &no_progress, &cancel).await;
            (r, now_ms())
        };
        // Cancelled only once the second export is really waiting for the turn.
        let canceller = async {
            while !is_waiting.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            cancel.cancel();
        };
        let (a, (b, b_ended), ()) = tokio::join!(first, second, canceller);
        a.unwrap();
        assert_eq!(b, Err(crate::import::CANCELLED.to_string()));
        assert!(!got_turn.load(Ordering::SeqCst), "waiting(false) after a cancel");
        // It returned while the first export still held the turn, and never started PowerShell.
        assert!(b_ended < times(&fx.mark, "end")[0]);
        assert_eq!(times(&fx.mark, "start").len(), 1);
    }

    /// Deck shapes that round differently in f32 and f64 (1920 / (384 / 305) is about 1525): the picture size is
    /// the one `output_size` gives, sent to the script, so the export is accepted.
    #[cfg(windows)]
    #[tokio::test]
    async fn the_picture_size_comes_from_output_size() {
        for (w, h) in [(384.0f32, 305.0f32), (436.535_43, 634.960_6), (960.0, 540.0)] {
            let fx = fixture(&format!("count:2,size:{w}:{h},slides:2"));
            let pngs = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await.unwrap();
            let (pw, ph) = crate::import::pdf::output_size(w / h);
            assert_eq!(marks(&fx.mark, "picture"), vec![format!("{pw} {ph}")]);
            for png in &pngs {
                assert_eq!(image::image_dimensions(png).unwrap(), (pw, ph));
            }
        }
    }

    /// Without PowerPoint, `New-Object` fails before `ATTACHED`: that is PowerPoint missing, not a bad file.
    #[cfg(windows)]
    #[tokio::test]
    async fn no_powerpoint_says_it_could_not_be_started() {
        let fx = fixture("comfail");
        let r = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await;
        assert_eq!(r, Err("PowerPoint could not be started.".to_string()));
        assert!(marks(&fx.mark, "cleanup").is_empty());
    }

    /// A dropped export (not cancelled through its token) still closes our copy, after PowerShell is gone.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_dropped_export_still_cleans_up() {
        let fx = fixture("started,count:3,size:960:540,hang");
        let never = CancellationToken::new();
        let export = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &never);
        // `picture` is logged once the size was answered, so STARTED_BY_US has been read by then.
        let answered = async {
            while marks(&fx.mark, "picture").is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        tokio::select! {
            r = export => panic!("the export ended: {r:?}"),
            () = answered => {}
        }
        let began = Instant::now();
        while marks(&fx.mark, "cleanup").is_empty() && began.elapsed() < Duration::from_secs(20) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(marks(&fx.mark, "cleanup"), vec![format!("-Path {} -StartedByUs", fx.input.display())]);
        assert!(times(&fx.mark, "end").is_empty(), "the export ran on");
        assert_eq!(fx.ppt.gate.available_permits(), 1);
    }

    /// Review Focus 6: a stopped export leaves our copy open in PowerPoint, so the clean-up closes it by name and
    /// may quit PowerPoint only if the export said it started it.
    #[cfg(windows)]
    #[tokio::test]
    async fn cancel_kills_export_and_cleans_up_our_presentation() {
        let fx = fixture("started,count:3,size:960:540,hang");
        let cancel = CancellationToken::new();
        let later = cancel.clone();
        let mark = fx.mark.clone();
        tokio::spawn(async move {
            while times(&mark, "start").is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            later.cancel();
        });
        let began = Instant::now();
        let r = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &cancel).await;
        assert_eq!(r, Err(crate::import::CANCELLED.to_string()));
        assert!(began.elapsed() < Duration::from_secs(20));
        assert!(times(&fx.mark, "end").is_empty(), "the export ran on");
        let cleanup = marks(&fx.mark, "cleanup");
        assert_eq!(cleanup, vec![format!("-Path {} -StartedByUs", fx.input.display())]);
        // The turn is free again.
        assert_eq!(fx.ppt.gate.available_permits(), 1);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn timeout_cleans_up_without_quitting_a_powerpoint_we_did_not_start() {
        let mut fx = fixture("count:3,size:960:540,hang");
        fx.ppt.fake.as_mut().unwrap().timeout = Some((Duration::from_millis(300), Duration::ZERO));
        let r = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await;
        assert_eq!(r, Err("PowerPoint took too long and was stopped. Try again.".to_string()));
        assert_eq!(marks(&fx.mark, "cleanup"), vec![format!("-Path {}", fx.input.display())]);
    }

    /// The first budget only covers opening the file; once `COUNT` arrives each slide adds time.
    #[cfg(windows)]
    #[tokio::test]
    async fn count_extends_the_time_allowed() {
        let mut fx = fixture("count:2,size:960:540,sleep:900,slides:2:1920:1080");
        fx.ppt.fake.as_mut().unwrap().timeout = Some((Duration::from_millis(400), Duration::from_millis(500)));
        let pngs = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await.unwrap();
        assert_eq!(pngs.len(), 2);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn pictures_must_have_the_slide_size() {
        let fx = fixture("count:1,size:960:540,slides:1:1920:1200");
        let r = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await;
        assert_eq!(r, Err("PowerPoint couldn't open this presentation.".to_string()));
        // 4:3 slides give 1920×1440.
        let fx = fixture("count:1,size:720:540,slides:1:1920:1440");
        fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await.unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn every_slide_hidden_means_no_slides() {
        let fx = fixture("count:2,size:960:540");
        let r = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await;
        assert_eq!(r, Err("This presentation has no slides.".to_string()));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_failing_script_says_powerpoint_could_not_open_it_and_needs_no_clean_up() {
        let fx = fixture("started,fail");
        let r = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await;
        assert_eq!(r, Err("PowerPoint couldn't open this presentation.".to_string()));
        assert!(marks(&fx.mark, "cleanup").is_empty());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn missing_lines_mean_the_export_failed() {
        // Exit 0 but no COUNT/SIZE: something else answered.
        let fx = fixture("slides:1:1920:1080");
        let r = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await;
        assert_eq!(r, Err("PowerPoint couldn't open this presentation.".to_string()));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_powershell_that_cannot_start_says_so_plainly() {
        let mut fx = fixture("");
        fx.ppt.fake.as_mut().unwrap().exe = fx.dir.join("nope.exe");
        let r = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &CancellationToken::new()).await;
        assert_eq!(r, Err("PowerPoint could not be started.".to_string()));
    }

    /// Whether PowerPoint is running at all, by its process list (read only; nothing here ever stops it).
    #[cfg(windows)]
    fn powerpoint_running() -> bool {
        let out = std::process::Command::new("tasklist").args(["/FI", "IMAGENAME eq POWERPNT.EXE", "/NH"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).to_uppercase().contains("POWERPNT.EXE")
    }

    /// Real PowerPoint (`RP_TEST_PPT=1`). Skipped while PowerPoint is already running, so a test never shares
    /// PowerPoint with presentations someone has open.
    #[cfg(windows)]
    fn real_powerpoint() -> bool {
        if std::env::var_os("RP_TEST_PPT").is_none() {
            return false;
        }
        if powerpoint_running() {
            eprintln!("PowerPoint is already running; the real PowerPoint test is skipped so it never touches open presentations.");
            return false;
        }
        true
    }

    #[cfg(windows)]
    fn sample_in(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let input = dir.join("input.pptx");
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments").join("media").join("sample.pptx"), &input).unwrap();
        input
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn powerpoint_exports_sample_with_progress() {
        if !real_powerpoint() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rp ppt é {}", uuid::Uuid::new_v4()));
        let input = sample_in(&dir.join("work"));
        let seen = std::sync::Mutex::new(Vec::new());
        let progress = |i: usize, n: usize| seen.lock().unwrap().push((i, n));
        let ppt = PowerPoint::new();
        let pngs = ppt.export(&input, &dir.join("work").join("slides"), &quiet, &progress, &CancellationToken::new()).await.unwrap();
        assert_eq!(pngs.len(), 3);
        for png in &pngs {
            assert_eq!(image::image_dimensions(png).unwrap(), (1920, 1080), "{png:?}");
        }
        assert_eq!(*seen.lock().unwrap(), vec![(1, 3), (2, 3), (3, 3)]);
        if let Some(keep) = std::env::var_os("RP_KEEP_SLIDE") {
            std::fs::copy(&pngs[1], keep).unwrap();
        }
        wait_for_powerpoint_to_quit(Duration::from_secs(15));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// What the running PowerPoint has open (attaching only when it runs, so this never starts one).
    #[cfg(windows)]
    fn open_presentations() -> Vec<String> {
        const LIST: &str = "$s = (Get-Process -Id $PID).SessionId; if (Get-Process POWERPNT -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq $s }) { foreach ($p in @((New-Object -ComObject PowerPoint.Application).Presentations)) { $p.FullName } }";
        let out = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", LIST]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
    }

    /// PowerPoint leaves a moment after `Quit()`. After a cancel it waits for COM to notice that the killed
    /// PowerShell's references are gone: about 6 minutes after the kill on the test PC.
    #[cfg(windows)]
    fn wait_for_powerpoint_to_quit(budget: Duration) {
        let began = Instant::now();
        while powerpoint_running() && began.elapsed() < budget {
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(!powerpoint_running(), "PowerPoint was left running");
    }

    /// Cancelled after the first slide: the clean-up closes our copy, and PowerPoint, which this test started, quits.
    #[cfg(windows)]
    #[tokio::test]
    async fn cancelled_export_closes_our_copy_and_quits_the_powerpoint_it_started() {
        if !real_powerpoint() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rp-ppt-{}", uuid::Uuid::new_v4()));
        let input = sample_in(&dir.join("work"));
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let progress = move |i: usize, _: usize| if i == 1 { stop.cancel() };
        let ppt = PowerPoint::new();
        let r = ppt.export(&input, &dir.join("work").join("slides"), &quiet, &progress, &cancel).await;
        assert_eq!(r, Err(crate::import::CANCELLED.to_string()));
        let open = open_presentations();
        assert!(!open.iter().any(|p| p.eq_ignore_ascii_case(&input.display().to_string())), "{open:?}");
        wait_for_powerpoint_to_quit(Duration::from_secs(900));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn two_exports_run_one_at_a_time() {
        if !real_powerpoint() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rp-ppt-{}", uuid::Uuid::new_v4()));
        let (a_in, b_in) = (sample_in(&dir.join("a")), sample_in(&dir.join("b")));
        let ppt = PowerPoint::new();
        let cancel = CancellationToken::new();
        let seen = std::sync::Mutex::new(Vec::new());
        let note = |w: bool| seen.lock().unwrap().push(w);
        // When the second export first reports a slide, the first must have finished.
        let firsts = std::sync::Mutex::new(Vec::<(char, Instant)>::new());
        let ends = std::sync::Mutex::new(Vec::<(char, Instant)>::new());
        let pa = |i: usize, _: usize| if i == 1 { firsts.lock().unwrap().push(('a', Instant::now())) };
        let pb = |i: usize, _: usize| if i == 1 { firsts.lock().unwrap().push(('b', Instant::now())) };
        let a = async {
            let r = ppt.export(&a_in, &dir.join("a").join("slides"), &quiet, &pa, &cancel).await;
            ends.lock().unwrap().push(('a', Instant::now()));
            r
        };
        let b = async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let r = ppt.export(&b_in, &dir.join("b").join("slides"), &note, &pb, &cancel).await;
            ends.lock().unwrap().push(('b', Instant::now()));
            r
        };
        let (ra, rb) = tokio::join!(a, b);
        assert_eq!(ra.unwrap().len(), 3);
        assert_eq!(rb.unwrap().len(), 3);
        assert_eq!(*seen.lock().unwrap(), vec![true, false]);
        let firsts = firsts.lock().unwrap();
        let ends = ends.lock().unwrap();
        let a_end = ends.iter().find(|e| e.0 == 'a').unwrap().1;
        let b_first = firsts.iter().find(|e| e.0 == 'b').unwrap().1;
        assert!(b_first >= a_end, "the exports overlapped");
        wait_for_powerpoint_to_quit(Duration::from_secs(15));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Runs PowerShell on the running PowerPoint (attaching only when it runs) and returns what it printed.
    #[cfg(windows)]
    fn on_powerpoint(body: &str) -> String {
        let cmd = format!("$s = (Get-Process -Id $PID).SessionId; if (Get-Process POWERPNT -ErrorAction SilentlyContinue | Where-Object {{ $_.SessionId -eq $s }}) {{ $app = New-Object -ComObject PowerPoint.Application; {body} }}");
        let out = std::process::Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &cmd]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A deck the user opens in Protected View (an e-mail attachment) while the export runs is not among
    /// `Presentations`, so the export must not quit the PowerPoint it started.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_deck_opened_in_protected_view_during_the_export_stays_open() {
        if !real_powerpoint() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rp-ppt-{}", uuid::Uuid::new_v4()));
        let input = sample_in(&dir.join("work"));
        let theirs = sample_in(&dir.join("theirs"));
        let open = format!("$null = $app.ProtectedViewWindows.Open('{}')", theirs.display());
        let progress = |i: usize, _: usize| {
            if i == 1 {
                on_powerpoint(&open);
            }
        };
        let ppt = PowerPoint::new();
        let r = ppt.export(&input, &dir.join("work").join("slides"), &quiet, &progress, &CancellationToken::new()).await;
        let still = on_powerpoint("$app.ProtectedViewWindows.Count");
        // Close what the test opened, then let the PowerPoint it started go.
        on_powerpoint("foreach ($w in @($app.ProtectedViewWindows)) { $w.Close() }; if ($app.Presentations.Count -eq 0) { $app.Quit() }");
        assert_eq!(r.unwrap().len(), 3);
        assert_eq!(still, "1", "the export quit PowerPoint or closed the user's window");
        wait_for_powerpoint_to_quit(Duration::from_secs(30));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
