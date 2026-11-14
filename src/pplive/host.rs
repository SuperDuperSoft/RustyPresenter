//! The real `SlideShowHost`: one PowerShell per show, running `SCRIPT`, which drives PowerPoint through COM the way
//! `import::powerpoint` does (one PowerPoint per user, our own read-only copy, nothing else of the user's touched).
//! Rust writes one command line and reads one answer line. Compiled everywhere; only Windows ever starts it.
use super::{HostError, Pos, Rect, ShowState, SlideShowHost};
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Opens `-Path` and runs its slide show on the rectangle `-Left -Top -Width -Height` (physical pixels), in front of
/// every window, then answers commands on stdin until `CLOSE` or the end of stdin (see `Reply`). PowerShell 5.1
/// reads a script without a byte order mark as ANSI, so it stays ASCII.
pub const SCRIPT: &str = r#"
param([string]$Path, [int]$Left, [int]$Top, [int]$Width, [int]$Height)
$ErrorActionPreference = 'Stop'
function Say([string]$s) { [Console]::Out.WriteLine($s); [Console]::Out.Flush() }
Add-Type -Namespace RustyPresenter -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
'@
# Per-monitor aware, so the room rectangle is in real pixels as RustyPresenter measured it. Older Windows: no matter.
try { [void][RustyPresenter.Win]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) } catch {}
# As in the picture import: we started PowerPoint only if it was not running and had nothing open.
$session = (Get-Process -Id $PID).SessionId
$wasRunning = [bool](Get-Process POWERPNT -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq $session })
$app = New-Object -ComObject PowerPoint.Application
Say 'ATTACHED'
$hadOpen = $app.Presentations.Count
$startedByUs = (-not $wasRunning) -and ($hadOpen -eq 0)
$security = $app.AutomationSecurity
# msoAutomationSecurityForceDisable: a .pptm run from here never runs its macros.
$app.AutomationSecurity = 3
$pres = $null
$win = $null
function Pos {
  $v = $win.View
  $state = [int]$v.State
  $n = $shown.Count
  # ppSlideShowDone: the black end screen.
  if ($state -eq 5) { return ('POS ' + $n + ' ' + $n + ' 0 0 5') }
  $index = $v.Slide.SlideIndex
  $at = @($shown | Where-Object { $_ -le $index }).Count
  if ($at -lt 1) { $at = 1 }
  $click = 0
  $clicks = 0
  try { $click = [int]$v.GetClickIndex(); $clicks = [int]$v.GetClickCount() } catch {}
  return ('POS ' + $at + ' ' + $n + ' ' + $click + ' ' + $clicks + ' ' + $state)
}
try {
  # ReadOnly, Untitled, WithWindow = false. The "::x::" password makes a protected file fail instead of asking.
  $pres = $app.Presentations.Open($Path + '::x::', -1, 0, 0)
  # The show skips hidden slides, and so do the pictures: slide n here is picture n.
  $shown = @($pres.Slides | Where-Object { -not $_.SlideShowTransition.Hidden } | ForEach-Object { $_.SlideIndex })
  if ($shown.Count -eq 0) { Say 'NOSLIDES'; return }
  $set = $pres.SlideShowSettings
  # ppShowTypeSpeaker: full screen. Every slide, with animations and narration, PowerPoint's own timings, no loop.
  $set.ShowType = 1
  $set.RangeType = 1
  $set.ShowWithAnimation = -1
  $set.ShowWithNarration = -1
  $set.LoopUntilStopped = 0
  $set.AdvanceMode = 2
  # Presenter View would take the operator's screen.
  try { $set.ShowPresenterView = 0 } catch {}
  $before = [RustyPresenter.Win]::GetForegroundWindow()
  $win = $set.Run()
  $hwnd = [IntPtr]::new([int64]$win.HWND)
  # HWND_TOPMOST with SWP_NOACTIVATE | SWP_SHOWWINDOW: on the room monitor, in front of the room window.
  [void][RustyPresenter.Win]::SetWindowPos($hwnd, [IntPtr]::new(-1), $Left, $Top, $Width, $Height, 0x50)
  # The operator's keys stay with RustyPresenter.
  if ($before -ne [IntPtr]::Zero) { [void][RustyPresenter.Win]::SetForegroundWindow($before) }
  Say ('OPEN ' + $shown.Count)
  while ($true) {
    $line = [Console]::In.ReadLine()
    if ($line -eq $null) { break }
    $words = $line.Trim().Split(' ')
    if ($words[0] -eq 'CLOSE') { break }
    try {
      switch ($words[0]) {
        'NEXT' { $win.View.Next() }
        'PREV' { $win.View.Previous() }
        'GOTO' { $win.View.GotoSlide($shown[[int]$words[1] - 1], -1) }
        'BLACK' { if ($words[1] -eq '1') { $win.View.State = 3 } else { $win.View.State = 1 } }
        # Behind every window (HWND_BOTTOM; a topmost window loses that there), or topmost again. Neither moves,
        # sizes nor activates it (SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE).
        'COVER' { if ($words[1] -eq '1') { [void][RustyPresenter.Win]::SetWindowPos($hwnd, [IntPtr]::new(1), 0, 0, 0, 0, 0x13) } else { [void][RustyPresenter.Win]::SetWindowPos($hwnd, [IntPtr]::new(-1), 0, 0, 0, 0, 0x13) } }
        'POS' { }
        default { throw ('unknown command ' + $line) }
      }
      Say (Pos)
    } catch {
      # The show window is gone when it was ended in PowerPoint, or PowerPoint closed.
      $alive = $true
      try { $null = $win.View.State } catch { $alive = $false }
      if (-not $alive) { Say 'ENDED'; break }
      Say ('ERR ' + ($_.Exception.Message -replace '[\r\n]+', ' '))
    }
  }
} finally {
  if ($win -ne $null) { try { $win.View.Exit() } catch {} }
  if ($pres -ne $null) { try { $pres.Close() } catch {} }
  try { $app.AutomationSecurity = $security } catch {}
  try {
    if ($startedByUs -and $app.Presentations.Count -eq 0 -and $app.ProtectedViewWindows.Count -eq 0 -and -not $app.Visible) { $app.Quit() }
  } catch {}
}
"#;

/// One line the script prints.
#[derive(Debug, PartialEq)]
pub enum Reply {
    /// PowerPoint answered: a script that ends before this could not start PowerPoint.
    Attached,
    /// The show runs, with this many slides.
    Open(usize),
    NoSlides,
    Pos(Pos),
    Ended,
    Err(String),
}

/// Reads one line of the script's output. Anything else PowerShell prints is `None`.
pub fn parse_reply(s: &str) -> Option<Reply> {
    let s = s.trim();
    if let Some(why) = s.strip_prefix("ERR ") { return Some(Reply::Err(why.trim().to_string())) }
    let mut words = s.split_whitespace();
    let reply = match words.next()? {
        "ATTACHED" => Reply::Attached,
        "NOSLIDES" => Reply::NoSlides,
        "ENDED" => Reply::Ended,
        "OPEN" => Reply::Open(words.next()?.parse().ok().filter(|n| *n > 0)?),
        "POS" => {
            let mut n = || words.next()?.parse::<usize>().ok();
            let (slide, slides, click, clicks, state) = (n()?, n()?, n()?, n()?, n()?);
            if slide == 0 || slide > slides { return None }
            Reply::Pos(Pos { slide, slides, click, clicks, state: ShowState::from_code(state as u32)? })
        }
        _ => return None,
    };
    words.next().is_none().then_some(reply)
}

/// The program that runs the script: PowerShell, or a stand-in in tests.
#[derive(Clone)]
pub struct Launcher {
    pub exe: PathBuf,
    /// Before the script's own arguments.
    pub prefix: Vec<OsString>,
    pub env: Vec<(String, String)>,
}

impl Launcher {
    /// Windows PowerShell by full path, so a `powershell.exe` elsewhere on PATH is never the one started.
    pub fn powershell() -> Launcher {
        let system = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        Launcher { exe: system.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"), prefix: vec![], env: vec![] }
    }
}

/// Opening waits this long: a cold PowerPoint and a big file.
const OPEN_TIMEOUT: Duration = Duration::from_secs(120);
/// Each command waits this long.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(15);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(15);

struct Running { child: Child, stdin: ChildStdin, lines: Receiver<String> }

pub struct PsHost {
    launcher: Launcher,
    /// Where the script and PowerShell's log go.
    work: PathBuf,
    run: Option<Running>,
    open_timeout: Duration,
    answer_timeout: Duration,
}

impl PsHost {
    pub fn new(launcher: Launcher, work: PathBuf) -> PsHost { PsHost { launcher, work, run: None, open_timeout: OPEN_TIMEOUT, answer_timeout: ANSWER_TIMEOUT } }

    fn log(&self) -> PathBuf { self.work.join("powerpoint-show.log") }

    /// The script's arguments after `-File`, with `\\?\` taken off (PowerPoint cannot open it).
    fn args(&self, script: &Path, file: &Path, r: Rect) -> Vec<OsString> {
        let mut args: Vec<OsString> = self.launcher.prefix.clone();
        args.extend(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"].map(OsString::from));
        args.push(plain(script));
        args.push("-Path".into());
        args.push(plain(file));
        for (name, n) in [("-Left", r.x), ("-Top", r.y), ("-Width", r.width), ("-Height", r.height)] {
            args.push(name.into());
            args.push(n.to_string().into());
        }
        args
    }

    fn start(&mut self, file: &Path, screen: Rect) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.work)?;
        let script = self.work.join("powerpoint-show.ps1");
        std::fs::write(&script, SCRIPT)?;
        let mut cmd = Command::new(&self.launcher.exe);
        cmd.args(self.args(&script, file, screen)).envs(self.launcher.env.iter().map(|(k, v)| (k, v)));
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(std::fs::File::create(self.log())?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(crate::platform::proc::creation_flags());
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| std::io::Error::other("no input pipe"))?;
        let stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("no output pipe"))?;
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() { return }
            }
        });
        self.run = Some(Running { child, stdin, lines });
        Ok(())
    }

    /// The next line the script understands, or `None` when it ended or `until` passed.
    fn read(&mut self, until: Instant) -> Option<Reply> {
        let run = self.run.as_mut()?;
        loop {
            match run.lines.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(line) => if let Some(reply) = parse_reply(&line) { return Some(reply) },
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    /// What PowerShell said on its error stream, for the log.
    fn said(&self) -> String {
        let text = std::fs::read(self.log()).unwrap_or_default();
        String::from_utf8_lossy(&text).lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string()
    }

    fn ask(&mut self, command: &str) -> Result<Pos, HostError> {
        let Some(run) = self.run.as_mut() else { return Err(HostError::Ended) };
        if writeln!(run.stdin, "{command}").and_then(|()| run.stdin.flush()).is_err() {
            self.stop();
            return Err(HostError::Ended);
        }
        let until = Instant::now() + self.answer_timeout;
        loop {
            match self.read(until) {
                Some(Reply::Pos(p)) => return Ok(p),
                Some(Reply::Err(why)) => {
                    eprintln!("PowerPoint show: {command} failed: {why}");
                    return Err(HostError::Failed(why));
                }
                Some(Reply::Ended) | None => {
                    self.stop();
                    return Err(HostError::Ended);
                }
                Some(_) => {}
            }
        }
    }

    /// Ends the script: `CLOSE` lets it close our copy; one that does not end in time is killed (PowerPoint then
    /// keeps our copy open, never anything worse).
    fn stop(&mut self) {
        let Some(mut run) = self.run.take() else { return };
        let _ = writeln!(run.stdin, "CLOSE").and_then(|()| run.stdin.flush());
        drop(run.stdin);
        let began = Instant::now();
        loop {
            match run.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if began.elapsed() < CLOSE_TIMEOUT => std::thread::sleep(Duration::from_millis(50)),
                Ok(None) | Err(_) => {
                    let _ = run.child.kill();
                    let _ = run.child.wait();
                    return eprintln!("The PowerPoint show did not close in time; PowerPoint may still have it open.");
                }
            }
        }
    }
}

impl SlideShowHost for PsHost {
    fn open(&mut self, file: &Path, screen: Rect) -> Result<usize, HostError> {
        use crate::import::strings;
        self.stop();
        if let Err(e) = self.start(file, screen) {
            eprintln!("PowerPoint show could not start: {e}");
            return Err(HostError::Failed(strings::ppt_not_started()));
        }
        let until = Instant::now() + self.open_timeout;
        let mut attached = false;
        let failed = loop {
            match self.read(until) {
                Some(Reply::Attached) => attached = true,
                Some(Reply::Open(n)) => return Ok(n),
                Some(Reply::NoSlides) => break strings::pdf_no_slides(),
                Some(Reply::Err(why)) => break why,
                Some(Reply::Pos(_) | Reply::Ended) => {}
                None if attached => break strings::ppt_cannot_read(),
                None => break strings::ppt_not_started(),
            }
        };
        eprintln!("PowerPoint show failed: {failed} {}", self.said());
        self.stop();
        Err(HostError::Failed(failed))
    }
    fn next(&mut self) -> Result<Pos, HostError> { self.ask("NEXT") }
    fn prev(&mut self) -> Result<Pos, HostError> { self.ask("PREV") }
    fn goto(&mut self, slide: usize) -> Result<Pos, HostError> { self.ask(&format!("GOTO {slide}")) }
    fn current(&mut self) -> Result<Pos, HostError> { self.ask("POS") }
    fn black(&mut self, on: bool) -> Result<Pos, HostError> { self.ask(if on { "BLACK 1" } else { "BLACK 0" }) }
    fn cover(&mut self, on: bool) -> Result<Pos, HostError> { self.ask(if on { "COVER 1" } else { "COVER 0" }) }
    fn close(&mut self) { self.stop() }
}

impl Drop for PsHost {
    fn drop(&mut self) { self.stop() }
}

/// A path as PowerPoint can open it: `\\?\` confuses it.
fn plain(path: &Path) -> OsString {
    path.to_str().map_or_else(|| path.into(), |p| crate::platform::tools::strip_verbatim(p).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_are_read_strictly() {
        assert_eq!(parse_reply("ATTACHED"), Some(Reply::Attached));
        assert_eq!(parse_reply("OPEN 12\r"), Some(Reply::Open(12)));
        assert_eq!(parse_reply("NOSLIDES"), Some(Reply::NoSlides));
        assert_eq!(parse_reply("ENDED"), Some(Reply::Ended));
        assert_eq!(parse_reply("ERR Slide index out of range"), Some(Reply::Err("Slide index out of range".into())));
        assert_eq!(parse_reply("POS 2 5 1 3 1"), Some(Reply::Pos(Pos { slide: 2, slides: 5, click: 1, clicks: 3, state: ShowState::Running })));
        assert_eq!(parse_reply("POS 5 5 0 0 5"), Some(Reply::Pos(Pos { slide: 5, slides: 5, click: 0, clicks: 0, state: ShowState::Done })));
        for junk in ["", "OPEN", "OPEN 0", "OPEN x", "POS 1 2 0 0", "POS 0 2 0 0 1", "POS 3 2 0 0 1", "POS 1 2 0 0 9", "POS 1 2 0 0 1 7", "open 3", "WARNING: x", "ENDED now"] {
            assert_eq!(parse_reply(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn script_drives_the_show_with_the_documented_calls() {
        let s = SCRIPT;
        // Read-only, windowless, password-proof, macros off first: as the picture import opens it.
        assert!(s.find("$app.AutomationSecurity = 3").unwrap() < s.find(".Open(").unwrap());
        assert!(s.contains("$app.Presentations.Open($Path + '::x::', -1, 0, 0)"));
        assert!(s.find("Say 'ATTACHED'").unwrap() < s.find(".Open(").unwrap());
        for call in ["$set.ShowType = 1", "$set.ShowWithAnimation = -1", "$set.LoopUntilStopped = 0", "$set.AdvanceMode = 2", "$set.ShowPresenterView = 0",
            "$win = $set.Run()", "$win.HWND", "SetWindowPos($hwnd, [IntPtr]::new(-1), $Left, $Top, $Width, $Height, 0x50)", "$win.View.Next()", "$win.View.Previous()",
            "$win.View.GotoSlide($shown[[int]$words[1] - 1], -1)", "$win.View.State = 3", "$win.View.State = 1",
            "SetWindowPos($hwnd, [IntPtr]::new(1), 0, 0, 0, 0, 0x13)", "SetWindowPos($hwnd, [IntPtr]::new(-1), 0, 0, 0, 0, 0x13)", "$v.GetClickIndex()", "$v.GetClickCount()",
            "$win.View.Exit()", "$pres.Close()", "SlideShowTransition.Hidden"] {
            assert!(s.contains(call), "{call}");
        }
        // `break` inside a PowerShell `switch` leaves only the switch: CLOSE is checked before it.
        let close = s.find("if ($words[0] -eq 'CLOSE') { break }").unwrap();
        assert!(close < s.find("switch ($words[0])").unwrap());
        assert!(!s[s.find("switch (").unwrap()..].contains("'CLOSE'"));
        // Quits PowerPoint only as the import does: started by us, nothing of anyone's open, not visible.
        let quits: Vec<&str> = s.lines().filter(|l| l.contains("Quit()")).collect();
        assert_eq!(quits.len(), 1);
        assert!(quits[0].trim().starts_with("if ($startedByUs -and $app.Presentations.Count -eq 0 -and $app.ProtectedViewWindows.Count -eq 0 -and -not $app.Visible)"));
        assert_eq!(s.matches(".Close()").count(), 1);
        let lower = s.to_lowercase();
        for banned in ["taskkill", "stop-process", "kill", "getactiveobject("] { assert!(!lower.contains(banned), "{banned}") }
        for line in s.lines().filter(|l| l.contains("POWERPNT")) {
            assert!(line.contains("Get-Process POWERPNT -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq $session }"), "{line}");
        }
        // The here-string's end must start its line.
        assert!(s.contains("\n'@\n"));
        assert!(s.is_ascii());
    }

    const NAME: &str = "pplive::host::tests::fake_show_powershell";
    const FAKE: &str = "RP_FAKE_SHOW";
    const MARK: &str = "RP_FAKE_SHOW_MARK";

    /// Not a real test: the script's stand-in, this test binary run again. `RP_FAKE_SHOW` is comma separated:
    /// `clicks:<a>:<b>…` (the clicks on each slide; the show has that many slides), `comfail` (fails before
    /// `ATTACHED`), `openfail`, `noslides`, `endafter:<n>` (the show ends after n commands), `mute` (never
    /// answers commands). Each command and the arguments go to the file in `RP_FAKE_SHOW_MARK`.
    #[test]
    fn fake_show_powershell() {
        use std::io::Write;
        let Ok(steps) = std::env::var(FAKE) else { return };
        let mark = std::env::var(MARK).unwrap();
        let log = |line: String| writeln!(std::fs::OpenOptions::new().create(true).append(true).open(&mark).unwrap(), "{line}").unwrap();
        let args: Vec<String> = std::env::args().collect();
        let at = args.iter().position(|a| a == "-File").unwrap();
        log(format!("args {}", args[at + 2..].join(" ")));
        // The test harness may print its own words first, without a line end.
        println!();
        let mut clicks = vec![0usize];
        let mut end_after = usize::MAX;
        let mut mute = false;
        for step in steps.split(',') {
            let parts: Vec<&str> = step.split(':').collect();
            match parts[0] {
                "comfail" => { eprintln!("New-Object : Retrieving the COM class factory failed"); std::process::exit(1) }
                "openfail" => { println!("ATTACHED"); eprintln!("Exception calling \"Open\""); std::process::exit(1) }
                "noslides" => { println!("ATTACHED"); println!("NOSLIDES"); std::process::exit(0) }
                "clicks" => clicks = parts[1..].iter().map(|n| n.parse().unwrap()).collect(),
                "endafter" => end_after = parts[1].parse().unwrap(),
                "mute" => mute = true,
                other => panic!("unknown step {other}"),
            }
        }
        println!("ATTACHED");
        println!("OPEN {}", clicks.len());
        std::io::stdout().flush().unwrap();
        let (mut slide, mut click, mut black, mut n) = (1usize, 0usize, false, 0usize);
        for line in std::io::stdin().lines() {
            let line = line.unwrap();
            log(format!("cmd {line}"));
            let words: Vec<&str> = line.split_whitespace().collect();
            if words[0] == "CLOSE" { break }
            if mute { continue }
            n += 1;
            if n > end_after { println!("ENDED"); break }
            match words[0] {
                "NEXT" => if click < clicks[slide - 1] { click += 1 } else if slide < clicks.len() { slide += 1; click = 0 },
                "PREV" => if click > 0 { click -= 1 } else if slide > 1 { slide -= 1; click = clicks[slide - 1] },
                "GOTO" => match words[1].parse::<usize>() {
                    Ok(s) if (1..=clicks.len()).contains(&s) => { slide = s; click = 0 }
                    _ => { println!("ERR Slide index out of range"); continue }
                },
                "BLACK" => black = words[1] == "1",
                _ => {}
            }
            println!("POS {slide} {} {click} {} {}", clicks.len(), clicks[slide - 1], if black { 3 } else { 1 });
            std::io::stdout().flush().unwrap();
        }
        log("closed".into());
        std::process::exit(0);
    }

    struct Fx { dir: PathBuf, host: PsHost, mark: PathBuf }

    impl Drop for Fx {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); }
    }

    fn fixture(steps: &str) -> Fx {
        let dir = std::env::temp_dir().join(format!("rp-show-{}", uuid::Uuid::new_v4()));
        let mark = dir.join("mark");
        let launcher = Launcher {
            exe: std::env::current_exe().unwrap(),
            prefix: ["--exact", NAME, "--nocapture", "--"].map(OsString::from).to_vec(),
            env: vec![(FAKE.into(), steps.into()), (MARK.into(), mark.display().to_string())],
        };
        Fx { host: PsHost::new(launcher, dir.join("work")), dir, mark }
    }

    fn marks(mark: &Path, word: &str) -> Vec<String> {
        let text = std::fs::read_to_string(mark).unwrap_or_default();
        text.lines().filter_map(|l| Some(l.strip_prefix(word)?.trim().to_string())).collect()
    }

    const ROOM: Rect = Rect { x: -1280, y: 0, width: 1280, height: 720 };

    #[test]
    fn a_show_opens_answers_each_command_and_closes() {
        let mut fx = fixture("clicks:1:0");
        let file = fx.dir.join("talk.pptx");
        assert_eq!(fx.host.open(&file, ROOM), Ok(2));
        assert_eq!(marks(&fx.mark, "args"), [format!("-Path {} -Left -1280 -Top 0 -Width 1280 -Height 720", file.display())]);
        assert!(fx.dir.join("work").join("powerpoint-show.ps1").is_file());
        let p = fx.host.current().unwrap();
        assert_eq!((p.slide, p.click, p.clicks), (1, 0, 1));
        assert_eq!(fx.host.next().unwrap().click, 1);
        assert_eq!(fx.host.next().unwrap().slide, 2);
        assert_eq!(fx.host.prev().unwrap(), Pos { slide: 1, slides: 2, click: 1, clicks: 1, state: ShowState::Running });
        assert_eq!(fx.host.black(true).unwrap().state, ShowState::Black);
        assert_eq!(fx.host.goto(2).unwrap().slide, 2);
        assert_eq!(fx.host.goto(9), Err(HostError::Failed("Slide index out of range".into())));
        fx.host.close();
        assert_eq!(marks(&fx.mark, "cmd"), ["POS", "NEXT", "NEXT", "PREV", "BLACK 1", "GOTO 2", "GOTO 9", "CLOSE"]);
        assert_eq!(marks(&fx.mark, "closed").len(), 1, "the script ended by itself");
        assert_eq!(fx.host.current(), Err(HostError::Ended));
    }

    #[test]
    fn a_show_ended_in_powerpoint_says_ended() {
        let mut fx = fixture("clicks:0:0,endafter:1");
        fx.host.open(&fx.dir.join("talk.pptx"), ROOM).unwrap();
        fx.host.next().unwrap();
        assert_eq!(fx.host.next(), Err(HostError::Ended));
        assert!(fx.host.run.is_none());
    }

    #[test]
    fn a_show_that_cannot_start_says_why() {
        use crate::import::strings;
        for (steps, said) in [("comfail", strings::ppt_not_started()), ("openfail", strings::ppt_cannot_read()), ("noslides", strings::pdf_no_slides())] {
            let mut fx = fixture(steps);
            assert_eq!(fx.host.open(&fx.dir.join("talk.pptx"), ROOM), Err(HostError::Failed(said)), "{steps}");
            assert!(fx.host.run.is_none());
        }
        let mut fx = fixture("");
        fx.host.launcher.exe = fx.dir.join("nope");
        assert_eq!(fx.host.open(&fx.dir.join("talk.pptx"), ROOM), Err(HostError::Failed(strings::ppt_not_started())));
    }

    #[test]
    fn a_script_that_stops_answering_is_ended() {
        let mut fx = fixture("clicks:0,mute");
        fx.host.answer_timeout = Duration::from_millis(300);
        fx.host.open(&fx.dir.join("talk.pptx"), ROOM).unwrap();
        let began = Instant::now();
        assert_eq!(fx.host.current(), Err(HostError::Ended));
        assert!(began.elapsed() < Duration::from_secs(10));
        // It was told to close, and did.
        assert_eq!(marks(&fx.mark, "cmd"), ["POS", "CLOSE"]);
        assert_eq!(marks(&fx.mark, "closed").len(), 1);
    }
}
