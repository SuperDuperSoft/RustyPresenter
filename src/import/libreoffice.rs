//! LibreOffice to PDF (Plan 4 Task 5). One private profile, so runs take turns: a second soffice on the same profile
//! would hand its job to the first and exit at once (check C5). Success is a real PDF, not an exit code.
use super::strings as s;
use super::{CANCELLED, pdf};
use crate::platform::proc::{Stop, Tree, spawn_tree};
use crate::platform::tools::{self, Soffice};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio_util::sync::CancellationToken;

/// Every filter option is one argv element, or LibreOffice reads it as a file to open.
const PDF_FILTER: &str = r#"pdf:impress_pdf_Export:{"ExportHiddenSlides":{"type":"boolean","value":"false"},"ExportNotesPages":{"type":"boolean","value":"false"},"ReduceImageResolution":{"type":"boolean","value":"false"},"UseLosslessCompression":{"type":"boolean","value":"true"},"Quality":{"type":"long","value":"100"}}"#;
const FLAGS: [&str; 7] = ["--headless", "--invisible", "--norestore", "--nologo", "--nodefault", "--nolockcheck", "--nofirststartwizard"];
const BROKEN_PROFILE: &str = "user installation could not be completed";
const PGID_FILE: &str = "libreoffice.pgid";
const WARM_UP_TIMEOUT: Duration = Duration::from_secs(120);
const BASE_SECS: u64 = 120;
const SECS_PER_STEP: u64 = 30;
const STEP_BYTES: u64 = 50 * 1024 * 1024;
const MAX_SECS: u64 = 600;
#[cfg(unix)]
const LEFTOVER_GRACE: Duration = Duration::from_secs(2);

pub struct LibreOffice {
    soffice: Soffice,
    profile: PathBuf,
    /// One permit: warm-up and conversions share the profile.
    gate: Semaphore,
    warmed: AtomicBool,
    #[cfg(test)]
    extra_env: std::sync::Mutex<Vec<(String, String)>>,
    #[cfg(test)]
    test_timeout: std::sync::Mutex<Option<Duration>>,
}

impl LibreOffice {
    pub fn new(soffice: Soffice, data_dir: &Path) -> Arc<Self> {
        Arc::new(LibreOffice {
            soffice,
            profile: data_dir.join("deps").join("libreoffice-profile"),
            gate: Semaphore::new(1),
            warmed: AtomicBool::new(false),
            #[cfg(test)]
            extra_env: Default::default(),
            #[cfg(test)]
            test_timeout: Default::default(),
        })
    }

    fn deps_dir(&self) -> PathBuf {
        self.profile.parent().map_or_else(|| self.profile.clone(), Path::to_path_buf)
    }

    fn common_args(&self) -> Result<Vec<OsString>, String> {
        let mut args: Vec<OsString> = FLAGS.iter().map(OsString::from).collect();
        args.push(format!("-env:UserInstallation={}", tools::file_url(&self.profile, true)?).into());
        Ok(args)
    }

    pub fn args(&self, input: &Path, out_dir: &Path) -> Result<Vec<OsString>, String> {
        let mut args = self.common_args()?;
        args.extend([OsString::from("--convert-to"), PDF_FILTER.into(), "--outdir".into()]);
        args.extend([tools::plain_path(out_dir).into_os_string(), tools::plain_path(input).into_os_string()]);
        Ok(args)
    }

    pub fn timeout_for(bytes: u64) -> Duration {
        Duration::from_secs((BASE_SECS + SECS_PER_STEP * (bytes / STEP_BYTES)).min(MAX_SECS))
    }

    fn timeout(&self, input: &Path) -> Duration {
        #[cfg(test)]
        if let Some(timeout) = *self.test_timeout.lock().unwrap() {
            return timeout;
        }
        Self::timeout_for(std::fs::metadata(input).map_or(0, |m| m.len()))
    }

    fn command(&self, args: Vec<OsString>) -> Command {
        let mut cmd = Command::new(&self.soffice.path);
        // A leaked Python setting makes LibreOffice's own Python complain (check C5, gotcha 4).
        cmd.env_remove("PYTHONHOME").env_remove("PYTHONPATH").stdin(Stdio::null());
        if cfg!(target_os = "linux") {
            cmd.env("SAL_USE_VCLPLUGIN", "svp");
        }
        #[cfg(test)]
        {
            let env = self.extra_env.lock().unwrap();
            if !env.is_empty() {
                // The fake is this test binary, whose test harness would refuse LibreOffice's arguments.
                let joined = args.join(OsStr::new("\u{1f}"));
                cmd.args(["--exact", "import::libreoffice::tests::fake_soffice", "--nocapture"]).env("RP_FAKE_ARGS", joined);
                cmd.envs(env.iter().cloned());
                return cmd;
            }
        }
        cmd.args(args);
        cmd
    }

    /// Takes the one permit. `waiting` hears `true` only if another run holds it.
    async fn wait_turn(&self, waiting: &(dyn Fn(bool) + Sync), cancel: &CancellationToken) -> Result<SemaphorePermit<'_>, String> {
        if let Ok(permit) = self.gate.try_acquire() {
            return Ok(permit);
        }
        waiting(true);
        let permit = tokio::select! {
            permit = self.gate.acquire() => permit.map_err(|_| s::LO_NOT_STARTED.to_string())?,
            _ = cancel.cancelled() => return Err(CANCELLED.into()),
        };
        waiting(false);
        Ok(permit)
    }

    /// Runs one LibreOffice to its end, or kills it. Needs the permit.
    async fn run(&self, args: Vec<OsString>, log: &Path, timeout: Duration, cancel: &CancellationToken) -> Result<(), String> {
        if cancel.is_cancelled() {
            return Err(CANCELLED.into());
        }
        let not_started = |_| s::LO_NOT_STARTED.to_string();
        // A dead LibreOffice leaves this behind, and the next one would stop to ask about it.
        let _ = std::fs::remove_file(self.profile.join(".lock"));
        let deps = self.deps_dir();
        std::fs::create_dir_all(&deps).map_err(not_started)?;
        let stdout = std::fs::File::create(log).map_err(not_started)?;
        let stderr = stdout.try_clone().map_err(not_started)?;
        let mut cmd = self.command(args);
        cmd.stdout(stdout).stderr(stderr);
        let mut tree = spawn_tree(cmd).map_err(not_started)?;
        record_group(&deps, &tree);
        let result = tree.wait(timeout, cancel).await;
        // The launcher can end before everything it started. Nothing may keep the file or the profile.
        tree.kill();
        let _ = std::fs::remove_file(deps.join(PGID_FILE));
        match result {
            Ok(_) => Ok(()),
            Err(Stop::Timeout) => Err(s::LO_TOO_LONG.into()),
            Err(Stop::Cancelled) => Err(CANCELLED.into()),
            Err(Stop::Io(_)) => Err(s::LO_UNREADABLE.into()),
        }
    }

    /// Converts `input`, leaving `work/pdf/<name>.pdf` and `work/soffice.log`. `waiting` hears `true` before waiting
    /// behind another run and `false` once this run may start.
    pub async fn to_pdf(&self, input: &Path, work: &Path, waiting: &(dyn Fn(bool) + Sync), cancel: &CancellationToken) -> Result<PathBuf, String> {
        if self.soffice.snap {
            return Err(s::LO_SNAP.into());
        }
        let _permit = self.wait_turn(waiting, cancel).await?;
        let deps = self.deps_dir();
        let _ = tokio::task::spawn_blocking(move || kill_leftover(&deps)).await;
        let out_dir = work.join("pdf");
        std::fs::create_dir_all(&out_dir).map_err(|_| s::LO_NOT_STARTED.to_string())?;
        let pdf_path = out_dir.join(input.file_stem().unwrap_or(OsStr::new("input"))).with_extension("pdf");
        let log = work.join("soffice.log");
        let timeout = self.timeout(input);
        for retry in [true, false] {
            self.run(self.args(input, &out_dir)?, &log, timeout, cancel).await?;
            if pdf::check_pdf(&pdf_path).is_ok() {
                return Ok(pdf_path);
            }
            if !(retry && broken_profile(&log)) {
                break;
            }
            let _ = std::fs::remove_dir_all(&self.profile);
        }
        Err(s::LO_UNREADABLE.into())
    }

    /// Starts LibreOffice once in the background, so the first import is not the slow one. It holds the same permit
    /// as a conversion, so an import shows "Waiting for LibreOffice…" and never meets a half-made profile.
    pub async fn warm_up(self: Arc<Self>) {
        if self.soffice.snap || self.warmed.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(_permit) = self.gate.acquire().await else { return };
        let Ok(mut args) = self.common_args() else { return };
        args.push("--terminate_after_init".into());
        let log = self.deps_dir().join("libreoffice-warm-up.log");
        if let Err(e) = self.run(args, &log, WARM_UP_TIMEOUT, &CancellationToken::new()).await {
            eprintln!("LibreOffice warm-up: {e}");
        }
    }
}

fn broken_profile(log: &Path) -> bool {
    std::fs::read(log).is_ok_and(|text| String::from_utf8_lossy(&text).to_lowercase().contains(BROKEN_PROFILE))
}

/// A crashed app cannot take its process group with it on Unix, so the group is written down for the next start.
#[cfg(unix)]
fn record_group(deps: &Path, tree: &Tree) {
    if let Some(pgid) = tree.pgid() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let _ = std::fs::write(deps.join(PGID_FILE), format!("{pgid} {now}\n"));
    }
}

/// Windows' Job Object already died with the old app.
#[cfg(not(unix))]
fn record_group(_: &Path, _: &Tree) {}

/// Ends a LibreOffice left by a crashed app, if the recorded group still is one: its id may have been reused.
#[cfg(unix)]
pub fn kill_leftover(deps_dir: &Path) {
    let file = deps_dir.join(PGID_FILE);
    let Ok(text) = std::fs::read_to_string(&file) else { return };
    let pgid = text.split_whitespace().next().and_then(|id| id.parse::<i32>().ok()).filter(|id| *id > 1);
    if let Some(pgid) = pgid.filter(|id| group_alive(*id) && is_libreoffice(*id)) {
        // SAFETY: `kill` takes no pointers; `pgid` is above 1, so this names exactly one group.
        unsafe { libc::kill(-pgid, libc::SIGKILL) };
        let start = std::time::Instant::now();
        while group_alive(pgid) && start.elapsed() < LEFTOVER_GRACE {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let _ = std::fs::remove_file(&file);
}

#[cfg(not(unix))]
pub fn kill_leftover(_: &Path) {}

#[cfg(unix)]
fn group_alive(pgid: i32) -> bool {
    // SAFETY: signal 0 only asks whether the group exists.
    let ok = unsafe { libc::kill(-pgid, 0) } == 0;
    ok || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(unix)]
fn is_libreoffice(pid: i32) -> bool {
    let name = process_name(pid);
    name.contains("soffice") || name.contains("oosplash")
}

#[cfg(target_os = "linux")]
fn process_name(pid: i32) -> String {
    std::fs::read(format!("/proc/{pid}/cmdline")).map_or_else(|_| String::new(), |b| String::from_utf8_lossy(&b).into_owned())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_name(pid: i32) -> String {
    let out = std::process::Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]).output();
    out.map_or_else(|_| String::new(), |o| String::from_utf8_lossy(&o.stdout).into_owned())
}

#[cfg(test)]
mod tests;
