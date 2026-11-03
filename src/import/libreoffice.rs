//! Presentations to PDF with LibreOffice (Plan 4 Task 5), one at a time, in a profile of our own.
use super::{CANCELLED, pdf::check_pdf, strings};
use crate::platform::{proc::{Stop, spawn_tree}, tools::{self, Soffice}};
use std::{ffi::OsString, path::{Path, PathBuf}, process::Stdio, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};
use tokio::{process::Command, sync::{Semaphore, SemaphorePermit}};
use tokio_util::sync::CancellationToken;

const FILTER: &str = r#"pdf:impress_pdf_Export:{"ExportHiddenSlides":{"type":"boolean","value":"false"},"ExportNotesPages":{"type":"boolean","value":"false"},"ReduceImageResolution":{"type":"boolean","value":"false"},"UseLosslessCompression":{"type":"boolean","value":"true"},"Quality":{"type":"long","value":"100"}}"#;
const SWITCHES: [&str; 7] = ["--headless", "--invisible", "--norestore", "--nologo", "--nodefault", "--nolockcheck", "--nofirststartwizard"];
const PROFILE_FAILED: &str = "user installation could not be completed";
const WARM_UP_TIMEOUT: Duration = Duration::from_secs(120);
#[cfg(unix)]
const PGID_FILE: &str = "libreoffice.pgid";
#[cfg(unix)]
const KILL_WAIT: Duration = Duration::from_secs(2);

pub struct LibreOffice {
    soffice: Soffice,
    profile: PathBuf,
    deps: PathBuf,
    /// One permit: two LibreOffices cannot share a profile.
    gate: Semaphore,
    warmed: AtomicBool,
    #[cfg(test)]
    extra_env: Vec<(String, String)>,
    #[cfg(test)]
    prefix_args: Vec<OsString>,
}

impl LibreOffice {
    pub fn new(soffice: Soffice, data_dir: &Path) -> Arc<Self> {
        let deps = data_dir.join("deps");
        Arc::new(LibreOffice {
            soffice,
            profile: deps.join("libreoffice-profile"),
            deps,
            gate: Semaphore::new(1),
            warmed: AtomicBool::new(false),
            #[cfg(test)]
            extra_env: Vec::new(),
            #[cfg(test)]
            prefix_args: Vec::new(),
        })
    }

    pub fn args(&self, input: &Path, out_dir: &Path) -> Result<Vec<OsString>, String> {
        let mut args = self.common_args()?;
        args.push("--convert-to".into());
        args.push(FILTER.into());
        args.push("--outdir".into());
        args.push(plain(out_dir));
        args.push(plain(input));
        Ok(args)
    }

    fn common_args(&self) -> Result<Vec<OsString>, String> {
        let mut args: Vec<OsString> = SWITCHES.map(OsString::from).to_vec();
        args.push(format!("-env:UserInstallation={}", tools::file_url(&self.profile, true)?).into());
        Ok(args)
    }

    /// 120 s plus 30 s per 50 MB, at most 600 s.
    pub fn timeout_for(bytes: u64) -> Duration {
        const STEP: u128 = 50 * 1024 * 1024;
        Duration::from_secs((120 + bytes as u128 * 30 / STEP).min(600) as u64)
    }

    /// Converts `input` to `work/pdf/<stem>.pdf`. Success means a PDF came out, whatever the exit code says.
    /// `waiting(true)` is called if another conversion holds LibreOffice, and `waiting(false)` when this one gets it.
    pub async fn to_pdf(&self, input: &Path, work: &Path, waiting: &(dyn Fn(bool) + Sync), cancel: &CancellationToken) -> Result<PathBuf, String> {
        if self.soffice.snap {
            return Err(strings::LO_SNAP.to_string());
        }
        let _permit = self.enter(waiting, cancel).await?;
        let out_dir = work.join("pdf");
        std::fs::create_dir_all(&out_dir).map_err(|e| strings::lo_could_not_start(&e.to_string()))?;
        let pdf = out_dir.join(input.file_stem().unwrap_or_default()).with_extension("pdf");
        let log = work.join("soffice.log");
        let args = self.args(input, &out_dir)?;
        let timeout = Self::timeout_for(std::fs::metadata(input).map_or(0, |m| m.len()));
        for _ in 0..2 {
            self.run(&args, Some(&log), timeout, cancel).await.map_err(|stop| match stop {
                Run::Timeout => strings::LO_TIMEOUT.to_string(),
                Run::Cancelled => CANCELLED.to_string(),
                Run::NotStarted(e) => strings::lo_could_not_start(&e),
                Run::Failed => strings::LO_CANNOT_READ.to_string(),
            })?;
            if check_pdf(&pdf).is_ok() {
                return Ok(pdf);
            }
            let said = std::fs::read(&log).unwrap_or_default();
            if !String::from_utf8_lossy(&said).to_lowercase().contains(PROFILE_FAILED) {
                break;
            }
            let _ = std::fs::remove_dir_all(&self.profile);
        }
        Err(strings::LO_CANNOT_READ.to_string())
    }

    /// Starts LibreOffice once in the background, so the first import is not the slow one.
    /// Holds the gate, so an import started meanwhile waits and never shares the profile.
    pub async fn warm_up(self: Arc<Self>) {
        if self.soffice.snap || self.warmed.swap(true, Ordering::Relaxed) {
            return;
        }
        let Ok(_permit) = self.gate.acquire().await else { return };
        let Ok(mut args) = self.common_args() else { return };
        args.push("--terminate_after_init".into());
        let _ = self.run(&args, None, WARM_UP_TIMEOUT, &CancellationToken::new()).await;
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
            permit = self.gate.acquire() => permit.map_err(|e| strings::lo_could_not_start(&e.to_string()))?,
        };
        waiting(false);
        Ok(permit)
    }

    /// One LibreOffice run. The caller holds the gate. Dropping the tree at the end kills a `soffice.bin` that
    /// outlived its launcher.
    async fn run(&self, args: &[OsString], log: Option<&Path>, timeout: Duration, cancel: &CancellationToken) -> Result<(), Run> {
        kill_leftover(&self.deps);
        let _ = std::fs::remove_file(self.profile.join(".lock"));
        std::fs::create_dir_all(&self.deps).map_err(|e| Run::NotStarted(e.to_string()))?;
        let mut cmd = Command::new(&self.soffice.path);
        #[cfg(test)]
        {
            cmd.args(&self.prefix_args).envs(self.extra_env.iter().map(|(k, v)| (k, v)));
        }
        cmd.args(args).env_remove("PYTHONHOME").env_remove("PYTHONPATH").stdin(Stdio::null());
        if cfg!(target_os = "linux") {
            cmd.env("SAL_USE_VCLPLUGIN", "svp");
        }
        match log {
            Some(path) => {
                let out = std::fs::File::create(path).map_err(|e| Run::NotStarted(e.to_string()))?;
                let err = out.try_clone().map_err(|e| Run::NotStarted(e.to_string()))?;
                cmd.stdout(out).stderr(err);
            }
            None => {
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        let mut tree = spawn_tree(cmd).map_err(|e| Run::NotStarted(e.to_string()))?;
        #[cfg(unix)]
        if let Some(pgid) = tree.pgid() {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let _ = std::fs::write(self.deps.join(PGID_FILE), format!("{pgid} {stamp}"));
        }
        let ended = tree.wait(timeout, cancel).await;
        drop(tree);
        #[cfg(unix)]
        let _ = std::fs::remove_file(self.deps.join(PGID_FILE));
        match ended {
            Ok(_) => Ok(()),
            Err(Stop::Timeout) => Err(Run::Timeout),
            Err(Stop::Cancelled) => Err(Run::Cancelled),
            Err(Stop::Io(_)) => Err(Run::Failed),
        }
    }
}

enum Run {
    Timeout,
    Cancelled,
    NotStarted(String),
    Failed,
}

/// A path as LibreOffice can read it: `\\?\` confuses it.
fn plain(path: &Path) -> OsString {
    path.to_str().map_or_else(|| path.into(), |p| tools::strip_verbatim(p).into())
}

/// Kills a LibreOffice left by an app that died on Unix. A process group does not die with its parent.
/// Windows has nothing to do: the Job Object died with the old app.
#[cfg(not(unix))]
pub fn kill_leftover(_deps_dir: &Path) {}

/// Kills a LibreOffice left by an app that died on Unix. A process group does not die with its parent.
/// The group id may be someone else's by now, so only a group led by LibreOffice is touched.
#[cfg(unix)]
pub fn kill_leftover(deps_dir: &Path) {
    let file = deps_dir.join(PGID_FILE);
    let Ok(text) = std::fs::read_to_string(&file) else { return };
    let pgid = text.split_whitespace().next().and_then(|p| p.parse::<i32>().ok()).filter(|p| *p > 1);
    if let Some(pgid) = pgid.filter(|p| group_exists(*p) && leads_libreoffice(*p)) {
        // SAFETY: `kill` takes no pointers; `pgid` is above 1, so this is one group, not "everything".
        unsafe { libc::kill(-pgid, libc::SIGKILL) };
        let began = std::time::Instant::now();
        while group_exists(pgid) && began.elapsed() < KILL_WAIT {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let _ = std::fs::remove_file(file);
}

#[cfg(unix)]
fn group_exists(pgid: i32) -> bool {
    // SAFETY: signal 0 only asks whether the group exists.
    unsafe { libc::kill(-pgid, 0) == 0 }
}

#[cfg(target_os = "linux")]
fn command_line(pid: i32) -> String {
    String::from_utf8_lossy(&std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default()).into_owned()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn command_line(pid: i32) -> String {
    let out = std::process::Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

#[cfg(unix)]
fn leads_libreoffice(pid: i32) -> bool {
    let line = command_line(pid);
    line.contains("soffice") || line.contains("oosplash")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    const NAME: &str = "import::libreoffice::tests::fake_soffice";
    const FAKE: &str = "RP_FAKE_SOFFICE";
    const MARK: &str = "RP_FAKE_MARK";

    fn now_ms() -> u128 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
    }

    /// Not a real test: LibreOffice's stand-in, this test binary run again. Steps come from the child's environment,
    /// comma separated: `sleep:<ms>`, `hang`, `pdf`, `fail`, `say:<text>`, `once:<text>` (says it and exits the first
    /// run only). Each run adds `start <ms>` and `end <ms>` lines to the file in `RP_FAKE_MARK`.
    #[test]
    fn fake_soffice() {
        use std::io::Write;
        let Ok(steps) = std::env::var(FAKE) else { return };
        let mark = std::env::var(MARK).ok();
        let log = |line: String| if let Some(m) = &mark {
            writeln!(std::fs::OpenOptions::new().create(true).append(true).open(m).unwrap(), "{line}").unwrap();
        };
        log(format!("start {}", now_ms()));
        let args: Vec<String> = std::env::args().collect();
        for step in steps.split(',') {
            let (kind, value) = step.split_once(':').unwrap_or((step, ""));
            match kind {
                "sleep" => std::thread::sleep(Duration::from_millis(value.parse().unwrap())),
                "hang" => std::thread::sleep(Duration::from_secs(60)),
                "say" => println!("{value}"),
                "once" => {
                    let flag = format!("{}.once", mark.as_deref().unwrap());
                    if !Path::new(&flag).exists() {
                        std::fs::write(flag, "").unwrap();
                        println!("{value}");
                        log(format!("end {}", now_ms()));
                        std::process::exit(0);
                    }
                }
                "pdf" => {
                    let at = args.iter().position(|a| a == "--outdir").unwrap();
                    let stem = Path::new(&args[at + 2]).file_stem().unwrap();
                    let out = Path::new(&args[at + 1]).join(stem).with_extension("pdf");
                    std::fs::write(out, b"%PDF-1.7\n").unwrap();
                }
                "fail" => std::process::exit(1),
                _ => panic!("unknown step"),
            }
        }
        log(format!("end {}", now_ms()));
        std::process::exit(0);
    }

    struct Fx {
        dir: PathBuf,
        lo: Arc<LibreOffice>,
        mark: PathBuf,
        input: PathBuf,
        work: PathBuf,
    }

    impl Drop for Fx {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture(steps: &str, snap: bool) -> Fx {
        let dir = std::env::temp_dir().join(format!("rp-lo-{}", uuid::Uuid::new_v4()));
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let input = work.join("input.pptx");
        std::fs::write(&input, "deck").unwrap();
        let mark = dir.join("mark");
        let mut lo = LibreOffice::new(Soffice { path: std::env::current_exe().unwrap(), snap }, &dir);
        let m = Arc::get_mut(&mut lo).unwrap();
        m.extra_env = vec![(FAKE.into(), steps.into()), (MARK.into(), mark.display().to_string())];
        m.prefix_args = ["--exact", NAME, "--nocapture", "--"].map(OsString::from).to_vec();
        Fx { dir, lo, mark, input, work }
    }

    fn times(mark: &Path, word: &str) -> Vec<u128> {
        let text = std::fs::read_to_string(mark).unwrap_or_default();
        text.lines().filter_map(|l| l.strip_prefix(word)?.trim().parse().ok()).collect()
    }

    fn quiet(_: bool) {}

    #[test]
    fn args_use_private_profile_url_and_pdf_filter_as_one_argument() {
        let fx = fixture("pdf", false);
        let args = fx.lo.args(&fx.input, &fx.work.join("pdf")).unwrap();
        let text: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        for s in SWITCHES {
            assert!(text.iter().any(|a| a == s), "{s}");
        }
        let url = tools::file_url(&fx.dir.join("deps").join("libreoffice-profile"), true).unwrap();
        assert!(text.contains(&format!("-env:UserInstallation={url}")), "{text:?}");
        let at = text.iter().position(|a| a == "--convert-to").unwrap();
        assert!(text[at + 1].starts_with("pdf:impress_pdf_Export:{\"ExportHiddenSlides\""));
        assert!(text[at + 1].contains("\"ExportNotesPages\":{\"type\":\"boolean\",\"value\":\"false\"}"));
        assert!(text[at + 1].contains("\"UseLosslessCompression\""));
        assert_eq!(text[text.len() - 3], "--outdir");
        assert_eq!(text[text.len() - 2], fx.work.join("pdf").display().to_string());
        assert_eq!(text[text.len() - 1], fx.input.display().to_string());
    }

    #[test]
    fn args_strip_verbatim_prefix() {
        let fx = fixture("pdf", false);
        let args = fx.lo.args(Path::new(r"\\?\C:\work\input.pptx"), Path::new(r"\\?\C:\work\pdf")).unwrap();
        let text: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(text[text.len() - 2], r"C:\work\pdf");
        assert_eq!(text[text.len() - 1], r"C:\work\input.pptx");
    }

    #[test]
    fn timeout_scales_with_size_and_caps_at_600s() {
        const MB: u64 = 1024 * 1024;
        assert_eq!(LibreOffice::timeout_for(0), Duration::from_secs(120));
        assert_eq!(LibreOffice::timeout_for(50 * MB), Duration::from_secs(150));
        assert_eq!(LibreOffice::timeout_for(100 * MB), Duration::from_secs(180));
        assert_eq!(LibreOffice::timeout_for(800 * MB), Duration::from_secs(600));
        assert_eq!(LibreOffice::timeout_for(u64::MAX), Duration::from_secs(600));
    }

    #[tokio::test]
    async fn converts_and_returns_the_pdf_beside_the_log() {
        let fx = fixture("say:hello,pdf", false);
        let pdf = fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &CancellationToken::new()).await.unwrap();
        assert_eq!(pdf, fx.work.join("pdf").join("input.pdf"));
        assert!(std::fs::read_to_string(fx.work.join("soffice.log")).unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn stale_lock_removed_before_run() {
        let fx = fixture("pdf", false);
        let lock = fx.dir.join("deps").join("libreoffice-profile").join(".lock");
        std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
        std::fs::write(&lock, "old").unwrap();
        fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &CancellationToken::new()).await.unwrap();
        assert!(!lock.exists());
    }

    #[tokio::test]
    async fn second_conversion_waits_for_first() {
        let fx = fixture("sleep:1000,pdf", false);
        let cancel = CancellationToken::new();
        let seen = std::sync::Mutex::new(Vec::new());
        let note = |w: bool| seen.lock().unwrap().push(w);
        let work2 = fx.dir.join("work2");
        std::fs::create_dir_all(&work2).unwrap();
        let first = fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &cancel);
        let second = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            fx.lo.to_pdf(&fx.input, &work2, &note, &cancel).await
        };
        let (a, b) = tokio::join!(first, second);
        a.unwrap();
        b.unwrap();
        let (starts, ends) = (times(&fx.mark, "start"), times(&fx.mark, "end"));
        assert_eq!((starts.len(), ends.len()), (2, 2));
        assert!(starts[1] >= ends[0], "the runs overlapped: {starts:?} {ends:?}");
        assert_eq!(*seen.lock().unwrap(), vec![true, false]);
    }

    #[tokio::test]
    async fn import_waits_behind_warm_up() {
        let fx = fixture("sleep:1000", false);
        let warm = tokio::spawn(fx.lo.clone().warm_up());
        while times(&fx.mark, "start").is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let seen = std::sync::Mutex::new(Vec::new());
        let note = |w: bool| seen.lock().unwrap().push(w);
        let r = fx.lo.to_pdf(&fx.input, &fx.work, &note, &CancellationToken::new()).await;
        warm.await.unwrap();
        assert_eq!(r, Err(strings::LO_CANNOT_READ.to_string()), "this fake writes no PDF");
        assert_eq!(*seen.lock().unwrap(), vec![true, false]);
        let (starts, ends) = (times(&fx.mark, "start"), times(&fx.mark, "end"));
        assert!(starts[1] >= ends[0], "{starts:?} {ends:?}");
    }

    #[tokio::test]
    async fn warm_up_runs_once() {
        let fx = fixture("sleep:1", false);
        fx.lo.clone().warm_up().await;
        fx.lo.clone().warm_up().await;
        assert_eq!(times(&fx.mark, "start").len(), 1);
    }

    #[tokio::test]
    async fn cancel_while_waiting_never_starts_libreoffice() {
        let fx = fixture("sleep:800,pdf", false);
        let free = CancellationToken::new();
        let cancel = CancellationToken::new();
        let later = cancel.clone();
        let work2 = fx.dir.join("work2");
        std::fs::create_dir_all(&work2).unwrap();
        let first = fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &free);
        let second = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            later.cancel();
            fx.lo.to_pdf(&fx.input, &work2, &quiet, &cancel).await
        };
        let (a, b) = tokio::join!(first, second);
        a.unwrap();
        assert_eq!(b, Err(CANCELLED.to_string()));
        assert_eq!(times(&fx.mark, "start").len(), 1);
    }

    #[tokio::test]
    async fn cancel_during_conversion_stops_it() {
        let fx = fixture("hang", false);
        let cancel = CancellationToken::new();
        let later = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            later.cancel();
        });
        let began = Instant::now();
        let r = fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &cancel).await;
        assert_eq!(r, Err(CANCELLED.to_string()));
        assert!(began.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn missing_pdf_is_an_error_even_with_exit_0() {
        let fx = fixture("say:nothing", false);
        let r = fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &CancellationToken::new()).await;
        assert_eq!(r, Err(strings::LO_CANNOT_READ.to_string()));
    }

    #[tokio::test]
    async fn a_pdf_counts_even_with_a_failing_exit() {
        let fx = fixture("pdf,fail", false);
        fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &CancellationToken::new()).await.unwrap();
    }

    #[tokio::test]
    async fn broken_profile_is_deleted_and_tried_once_more() {
        let fx = fixture("once:Fatal: user installation could not be completed,pdf", false);
        let keep = fx.dir.join("deps").join("libreoffice-profile").join("old");
        std::fs::create_dir_all(keep.parent().unwrap()).unwrap();
        std::fs::write(&keep, "x").unwrap();
        fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &CancellationToken::new()).await.unwrap();
        assert_eq!(times(&fx.mark, "start").len(), 2);
        assert!(!keep.exists());
    }

    /// LibreOffice's own text starts with a capital (`libmergedlo`: "User installation could not be completed.").
    #[tokio::test]
    async fn broken_profile_is_noticed_in_any_case() {
        let fx = fixture("once:User installation could not be completed.,pdf", false);
        fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &CancellationToken::new()).await.unwrap();
        assert_eq!(times(&fx.mark, "start").len(), 2);
    }

    #[tokio::test]
    async fn broken_profile_is_tried_only_twice() {
        let fx = fixture("say:user installation could not be completed", false);
        let r = fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &CancellationToken::new()).await;
        assert_eq!(r, Err(strings::LO_CANNOT_READ.to_string()));
        assert_eq!(times(&fx.mark, "start").len(), 2);
    }

    #[tokio::test]
    async fn snap_soffice_returns_snap_message_without_spawning() {
        let fx = fixture("pdf", true);
        let r = fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &CancellationToken::new()).await;
        assert_eq!(r, Err(strings::LO_SNAP.to_string()));
        assert!(times(&fx.mark, "start").is_empty());
    }

    #[tokio::test]
    async fn a_soffice_that_cannot_start_says_so() {
        let fx = fixture("pdf", false);
        let lo = LibreOffice::new(Soffice { path: fx.dir.join("nope"), snap: false }, &fx.dir);
        let r = lo.to_pdf(&fx.input, &fx.work, &quiet, &CancellationToken::new()).await;
        assert!(r.unwrap_err().starts_with("LibreOffice could not start: "));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pgid_file_is_gone_after_a_run() {
        let fx = fixture("pdf", false);
        fx.lo.to_pdf(&fx.input, &fx.work, &quiet, &CancellationToken::new()).await.unwrap();
        assert!(!fx.dir.join("deps").join("libreoffice.pgid").exists());
    }

    #[cfg(unix)]
    fn pgid_alive(pgid: i32) -> bool {
        // SAFETY: signal 0 only checks that the group exists.
        unsafe { libc::kill(-pgid, 0) == 0 }
    }

    #[cfg(unix)]
    #[test]
    fn leftover_group_is_killed_before_lock_removed() {
        use std::os::unix::process::CommandExt;
        let fx = fixture("hang", false);
        let mut child = std::process::Command::new(&fx.lo.soffice.path)
            .args(&fx.lo.prefix_args)
            .env(FAKE, "hang")
            .process_group(0)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let pgid = child.id() as i32;
        let deps = fx.dir.join("deps");
        std::fs::create_dir_all(&deps).unwrap();
        let file = deps.join("libreoffice.pgid");
        std::fs::write(&file, format!("{pgid} {}", now_ms())).unwrap();
        // The reaper is this test: `kill_leftover` waits for the group to go, and a zombie still counts as a member.
        let reap = std::thread::spawn(move || child.wait());
        kill_leftover(&deps);
        reap.join().unwrap().unwrap();
        assert!(!pgid_alive(pgid));
        assert!(!file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn pgid_file_with_unrelated_process_is_ignored() {
        use std::os::unix::process::CommandExt;
        let dir = std::env::temp_dir().join(format!("rp-lo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = std::process::Command::new("sleep").arg("30").process_group(0).spawn().unwrap();
        let file = dir.join("libreoffice.pgid");
        std::fs::write(&file, format!("{} 1", child.id())).unwrap();
        kill_leftover(&dir);
        assert!(child.try_wait().unwrap().is_none(), "sleep was killed");
        assert!(!file.exists());
        child.kill().unwrap();
        child.wait().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn nothing_recorded_is_fine() {
        let dir = std::env::temp_dir().join(format!("rp-lo-{}", uuid::Uuid::new_v4()));
        kill_leftover(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("libreoffice.pgid"), "1 0").unwrap();
        kill_leftover(&dir);
        std::fs::write(dir.join("libreoffice.pgid"), "rubbish").unwrap();
        kill_leftover(&dir);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Real LibreOffice and real pdfium: `RP_TEST_SOFFICE=1` and `RP_PDFIUM=<library>`.
    #[tokio::test]
    async fn sample_pptx_becomes_three_slide_images() {
        if std::env::var_os("RP_TEST_SOFFICE").is_none() || std::env::var_os("RP_PDFIUM").is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rp-lo-{}", uuid::Uuid::new_v4()));
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let input = work.join("input.pptx");
        std::fs::copy("experiments/media/sample.pptx", &input).unwrap();
        let lo = LibreOffice::new(tools::find_soffice().unwrap(), &dir);
        let pdf = lo.to_pdf(&input, &work, &quiet, &CancellationToken::new()).await.unwrap();
        let renderer = crate::import::pdf::PdfRenderer::start(&dir).unwrap();
        let pages = renderer.render(pdf, work.join("slides"), Some(16.0 / 9.0), Box::new(|_, _| ()), CancellationToken::new()).await.unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(image::open(&pages[0]).unwrap().to_rgb8().dimensions(), (1920, 1080));
        if let Some(keep) = std::env::var_os("RP_KEEP_SLIDE") {
            std::fs::copy(&pages[1], keep).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Real LibreOffice: `RP_TEST_SOFFICE=1`.
    #[tokio::test]
    async fn converts_sample_pptx() {
        if std::env::var_os("RP_TEST_SOFFICE").is_none() {
            return;
        }
        let Some(soffice) = tools::find_soffice() else { panic!("no LibreOffice") };
        let dir = std::env::temp_dir().join(format!("rp-lo-{}", uuid::Uuid::new_v4()));
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let input = work.join("input.pptx");
        std::fs::copy("experiments/media/sample.pptx", &input).unwrap();
        let lo = LibreOffice::new(soffice, &dir);
        lo.clone().warm_up().await;
        let pdf = lo.to_pdf(&input, &work, &quiet, &CancellationToken::new()).await.unwrap();
        let bytes = std::fs::read(pdf).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.matches("/Type/Page/").count() + text.matches("/Type/Page\n").count(), 3, "three pages");
        let media = text.split("/MediaBox[").nth(1).unwrap();
        let box_: Vec<f32> = media.split(']').next().unwrap().split_whitespace().map(|n| n.parse().unwrap()).collect();
        assert!((box_[2] / box_[3] - 16.0 / 9.0).abs() < 0.01, "{box_:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
