//! Slides to pictures through the installed PowerPoint (Plan 4 Task 6b), one export at a time.
//! PowerPoint is one program shared with the operator, so these scripts close only what they opened and
//! nothing here ever kills it by name. A hidden PowerPoint left behind is safe; a closed sermon is not.
use super::{CANCELLED, pdf::output_size, sniff, strings};
use crate::platform::{proc::{Stop, Tree, spawn_tree}, tools::strip_verbatim};
#[cfg(test)]
use std::ffi::OsString;
use std::{path::{Path, PathBuf}, process::{ExitStatus, Stdio}, time::Duration};
use tokio::{io::{AsyncBufReadExt, BufReader}, process::{ChildStdout, Command}, sync::{Semaphore, SemaphorePermit}, time::Instant};
use tokio_util::sync::CancellationToken;

/// Must stay ASCII: Windows PowerShell reads a script without a BOM in the machine's old code page.
pub const SCRIPT: &str = r#"
param([string]$Path, [string]$Out)
$ErrorActionPreference = 'Stop'
$invariant = [Globalization.CultureInfo]::InvariantCulture
function Say([string]$line) { [Console]::Out.WriteLine($line) }
function Even([double]$n) { 2 * [long][math]::Round($n / 2, [MidpointRounding]::AwayFromZero) }
$wasRunning = [bool](Get-Process -Name POWERPNT -ErrorAction SilentlyContinue)
$app = New-Object -ComObject PowerPoint.Application
$hadOpen = $app.Presentations.Count
$startedByUs = (-not $wasRunning) -and ($hadOpen -eq 0)
if ($startedByUs) { Say 'STARTED_BY_US' }
# The setting belongs to the whole program, and the operator may have it open, so it goes back afterwards.
$security = $app.AutomationSecurity
# msoAutomationSecurityForceDisable: a .pptm never runs its macros.
$app.AutomationSecurity = 3
$pres = $null
try {
  # ReadOnly, Untitled = false, WithWindow = false. The password makes a protected file fail instead of asking.
  $pres = $app.Presentations.Open(($Path + '::x::'), -1, 0, 0)
  $visible = @()
  foreach ($s in $pres.Slides) { if (-not $s.SlideShowTransition.Hidden) { $visible += $s } }
  $w = [double]$pres.PageSetup.SlideWidth
  $h = [double]$pres.PageSetup.SlideHeight
  Say ('COUNT ' + $visible.Count)
  Say ('SIZE ' + $w.ToString($invariant) + ' ' + $h.ToString($invariant))
  $outW = 1920
  $outH = Even (1920 * $h / $w)
  if ($outH -gt 2160) { $outH = 2160; $outW = Even (2160 * $w / $h) }
  $i = 0
  foreach ($s in $visible) {
    $i++
    $s.Export((Join-Path $Out ('slide-{0:D3}.png' -f $i)), 'PNG', [int]$outW, [int]$outH)
    Say ('SLIDE ' + $i + ' ' + $visible.Count)
  }
} finally {
  try {
    if ($pres) { $pres.Close() }
  } finally {
    $app.AutomationSecurity = $security
    # The operator may have opened a presentation of their own in this same PowerPoint while we worked.
    if ($startedByUs -and $hadOpen -eq 0 -and $app.Presentations.Count -eq 0) { $app.Quit() }
  }
}
"#;

/// Runs after a timeout or a cancel, when the export has been killed and could not tidy up. Same ASCII rule.
pub const CLEANUP_SCRIPT: &str = r#"
param([string]$Path, [switch]$StartedByUs)
$ErrorActionPreference = 'Stop'
try {
  $app = [Runtime.InteropServices.Marshal]::GetActiveObject('PowerPoint.Application')
} catch {
  exit 0
}
for ($i = $app.Presentations.Count; $i -ge 1; $i--) {
  $pres = $app.Presentations.Item($i)
  if ($pres.FullName -eq $Path) { $pres.Close() }
}
# What PowerPoint starts with; the killed export never got to put its own value back.
$app.AutomationSecurity = 1
if ($StartedByUs -and $app.Presentations.Count -eq 0) { $app.Quit() }
"#;

const POWERSHELL: &str = "powershell.exe";
const EXPORT_FILE: &str = "ppt-export.ps1";
const CLEANUP_FILE: &str = "ppt-cleanup.ps1";
const LOG_FILE: &str = "ppt-export.log";
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(15);
/// Rounding is done twice, here and in the script, on different number types.
const SIZE_SLACK: u32 = 2;

/// One line the export script prints.
#[derive(Debug, PartialEq)]
pub enum Line {
    Count(usize),
    Size(f32, f32),
    Slide(usize, usize),
    StartedByUs,
}

pub fn parse_line(s: &str) -> Option<Line> {
    let words: Vec<&str> = s.split_whitespace().collect();
    match words[..] {
        ["COUNT", n] => n.parse().ok().map(Line::Count),
        ["SIZE", w, h] => {
            let size = (w.parse::<f32>().ok()?, h.parse::<f32>().ok()?);
            (size.0.is_finite() && size.1.is_finite()).then_some(Line::Size(size.0, size.1))
        }
        ["SLIDE", i, n] => Some(Line::Slide(i.parse().ok()?, n.parse().ok()?)),
        ["STARTED_BY_US"] => Some(Line::StartedByUs),
        _ => None,
    }
}

/// What the export said before it ended or was stopped.
#[derive(Default)]
struct Heard {
    started_by_us: bool,
    count: Option<usize>,
    size: Option<(f32, f32)>,
}

fn slide_file(i: usize) -> String {
    format!("slide-{i:03}.png")
}

pub struct PowerPoint {
    /// One permit: PowerPoint is a single program, and two exports would share it.
    gate: Semaphore,
    program: PathBuf,
    #[cfg(test)]
    extra_env: Vec<(String, String)>,
    #[cfg(test)]
    prefix_args: Vec<OsString>,
    /// Replaces `timeout_for` in tests, so a hang stops in milliseconds: this long for each slide, and once more.
    #[cfg(test)]
    test_timeout: Option<Duration>,
}

impl Default for PowerPoint {
    fn default() -> PowerPoint {
        PowerPoint::new()
    }
}

impl PowerPoint {
    pub fn new() -> PowerPoint {
        PowerPoint {
            gate: Semaphore::new(1),
            program: POWERSHELL.into(),
            #[cfg(test)]
            extra_env: Vec::new(),
            #[cfg(test)]
            prefix_args: Vec::new(),
            #[cfg(test)]
            test_timeout: None,
        }
    }

    /// How long an export may run in all: 60 s until the slide count is known, then 10 s per slide on top.
    pub fn timeout_for(slides: usize) -> Duration {
        Duration::from_secs(60u64.saturating_add((slides as u64).saturating_mul(10)).min(600))
    }

    fn timeout(&self, slides: usize) -> Duration {
        #[cfg(test)]
        if let Some(t) = self.test_timeout {
            return t.saturating_mul(1 + slides as u32);
        }
        Self::timeout_for(slides)
    }

    /// Each visible slide of `input` as a PNG in `out`, in order. Never closes or quits anything the operator has open.
    /// `waiting(true)` is called if another export holds PowerPoint, and `waiting(false)` when this one gets it.
    /// `progress(i, n)` is called after slide `i` of `n`.
    pub async fn export(&self, input: &Path, out: &Path, waiting: &(dyn Fn(bool) + Sync), progress: &(dyn Fn(usize, usize) + Sync), cancel: &CancellationToken) -> Result<Vec<PathBuf>, String> {
        let _permit = self.enter(waiting, cancel).await?;
        let (input, out) = (plain(input), plain(out));
        let log = out.join(LOG_FILE);
        std::fs::create_dir_all(&out).map_err(not_started)?;
        let script = out.join(EXPORT_FILE);
        std::fs::write(&script, SCRIPT).map_err(not_started)?;
        let mut cmd = self.powershell(&script);
        cmd.arg("-Path").arg(&input).arg("-Out").arg(&out);
        cmd.stdout(Stdio::piped()).stderr(std::fs::File::create(&log).map_err(not_started)?);
        let mut tree = spawn_tree(cmd).map_err(not_started)?;
        let stdout = tree.stdout().ok_or_else(|| not_started("no output from PowerShell"))?;
        let mut heard = Heard::default();
        let ended = self.follow(&mut tree, stdout, &mut heard, progress, cancel).await;
        let status = match ended {
            Ok(status) => status,
            Err(stop) => {
                // The tree must be gone before the cleanup looks at PowerPoint. `wait` kills it on the way out.
                let _ = tree.wait(Duration::ZERO, cancel).await;
                drop(tree);
                self.clean_up(&input, &out, heard.started_by_us).await;
                return Err(match stop {
                    Stop::Timeout => strings::PPT_TIMEOUT.to_string(),
                    Stop::Cancelled => CANCELLED.to_string(),
                    Stop::Io(e) => {
                        eprintln!("PowerPoint export: {e}");
                        strings::PPT_CANNOT_OPEN.to_string()
                    }
                });
            }
        };
        drop(tree);
        if !status.success() {
            return Err(refused(&String::from_utf8_lossy(&std::fs::read(&log).unwrap_or_default())));
        }
        finished(&heard, &out)
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

    fn powershell(&self, script: &Path) -> Command {
        let mut cmd = Command::new(&self.program);
        #[cfg(test)]
        {
            cmd.args(&self.prefix_args).envs(self.extra_env.iter().map(|(k, v)| (k, v)));
        }
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(script).stdin(Stdio::null());
        cmd
    }

    /// Reads what the script says until it ends. The time limit is measured from the start and grows once the
    /// slide count is known.
    async fn follow(&self, tree: &mut Tree, stdout: ChildStdout, heard: &mut Heard, progress: &(dyn Fn(usize, usize) + Sync), cancel: &CancellationToken) -> Result<ExitStatus, Stop> {
        let start = Instant::now();
        let mut deadline = start + self.timeout(0);
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(Stop::Cancelled),
                _ = tokio::time::sleep_until(deadline) => return Err(Stop::Timeout),
                read = reader.read_until(b'\n', &mut line) => read,
            };
            match read {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => return Err(Stop::Io(e.to_string())),
            }
            match parse_line(&String::from_utf8_lossy(&line)) {
                Some(Line::StartedByUs) => heard.started_by_us = true,
                Some(Line::Count(n)) => {
                    heard.count = Some(n);
                    deadline = start + self.timeout(n);
                }
                Some(Line::Size(w, h)) => heard.size = Some((w, h)),
                Some(Line::Slide(i, n)) => progress(i, n),
                None => {}
            }
        }
        tree.wait(deadline.saturating_duration_since(Instant::now()), cancel).await
    }

    /// Closes the presentation a killed export left open. Still under the gate, so the next export finds PowerPoint
    /// tidy. If this fails, the hidden PowerPoint stays: that is only a log line, never a risk to the operator's work.
    async fn clean_up(&self, input: &Path, out: &Path, started_by_us: bool) {
        let script = out.join(CLEANUP_FILE);
        if let Err(e) = std::fs::write(&script, CLEANUP_SCRIPT) {
            eprintln!("PowerPoint cleanup: {e}");
            return;
        }
        let mut cmd = self.powershell(&script);
        cmd.arg("-Path").arg(input).stdout(Stdio::null()).stderr(Stdio::null());
        if started_by_us {
            cmd.arg("-StartedByUs");
        }
        let ended = match spawn_tree(cmd) {
            Ok(mut tree) => tree.wait(CLEANUP_TIMEOUT, &CancellationToken::new()).await.map_err(|stop| format!("{stop:?}")),
            Err(e) => Err(e.to_string()),
        };
        match ended {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("PowerPoint cleanup: {status}"),
            Err(e) => eprintln!("PowerPoint cleanup: {e}"),
        }
    }
}

/// A path as PowerPoint can read it: `\\?\` confuses it, and the cleanup compares this text with `FullName`.
fn plain(path: &Path) -> PathBuf {
    path.to_str().map_or_else(|| path.to_path_buf(), |p| strip_verbatim(p).into())
}

/// The raw error goes to the log, never to the operator.
fn not_started(e: impl std::fmt::Display) -> String {
    eprintln!("PowerPoint could not start: {e}");
    strings::PPT_NOT_STARTED.to_string()
}

/// What to say about a script that failed. Its error text goes to the log.
fn refused(said: &str) -> String {
    eprintln!("PowerPoint export failed: {}", said.lines().find(|l| !l.trim().is_empty()).unwrap_or("(nothing on stderr)"));
    let low = said.to_lowercase();
    if low.contains("80040154") || low.contains("comobject") {
        strings::PPT_NOT_STARTED.to_string()
    } else if low.contains("password") {
        sniff::ENCRYPTED.to_string()
    } else {
        strings::PPT_CANNOT_OPEN.to_string()
    }
}

/// The script ended well: every slide it promised must be there, at the size every path uses.
fn finished(heard: &Heard, out: &Path) -> Result<Vec<PathBuf>, String> {
    let failed = |why: String| {
        eprintln!("PowerPoint export: {why}");
        strings::PPT_CANNOT_OPEN.to_string()
    };
    let (Some(count), Some((w, h))) = (heard.count, heard.size) else {
        return Err(failed("the script did not report the slides".into()));
    };
    if count == 0 {
        return Err(strings::PDF_NO_SLIDES.to_string());
    }
    if w <= 0.0 || h <= 0.0 {
        return Err(failed(format!("slide size {w} x {h}")));
    }
    let want = output_size(w / h);
    let files: Vec<PathBuf> = (1..=count).map(|i| out.join(slide_file(i))).collect();
    for file in &files {
        let got = image::image_dimensions(file).map_err(|e| failed(format!("{}: {e}", file.display())))?;
        if got.0.abs_diff(want.0) > SIZE_SLACK || got.1.abs_diff(want.1) > SIZE_SLACK {
            return Err(failed(format!("{}: {got:?}, expected {want:?}", file.display())));
        }
    }
    Ok(files)
}

#[cfg(test)]
#[path = "powerpoint_tests.rs"]
pub(crate) mod tests;
