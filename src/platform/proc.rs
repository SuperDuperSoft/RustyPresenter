//! Converter process trees that die together (Plan 4 Task 2). LibreOffice starts `soffice.bin` from a launcher,
//! so killing the launcher alone leaves a process holding the file and the profile (check C5).
//! Windows: a Job Object that kills its members when its handle closes, so the tree dies with the app for any reason.
//! Unix: a process group, which does not outlive a crashed app on its own (Plan 4 Task 5 records the group id).
use std::process::ExitStatus;
use std::time::Duration;
use tokio::process::{Child, ChildStdout, Command};
use tokio_util::sync::CancellationToken;

/// How long `wait` lets a killed child take to be collected.
const REAP_GRACE: Duration = Duration::from_secs(2);

/// Why `Tree::wait` gave up. The tree is already dead for `Timeout` and `Cancelled`.
#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    Timeout,
    Cancelled,
    Io(String),
}

/// A running program and everything it starts. Dropping it kills them all.
pub struct Tree {
    child: Child,
    #[cfg(windows)]
    job: win::OwnedJob,
    /// Cleared by `kill`: the id may belong to a new group afterwards. A normal exit does not clear it, so `Drop`
    /// still kills survivors such as a `soffice.bin` that outlived its launcher. A group id is not reused while any
    /// member lives; the accepted risk is a group that fully ended and whose id was taken by a new group before the
    /// drop, which needs the whole pid space to wrap in that short window.
    #[cfg(unix)]
    pgid: Option<i32>,
}

#[cfg(unix)]
pub fn spawn_tree(mut cmd: Command) -> std::io::Result<Tree> {
    cmd.process_group(0);
    let child = cmd.spawn()?;
    let pgid = child.id().and_then(|id| i32::try_from(id).ok()).filter(|id| *id > 1);
    Ok(Tree { child, pgid })
}

#[cfg(windows)]
pub fn spawn_tree(mut cmd: Command) -> std::io::Result<Tree> {
    cmd.creation_flags(creation_flags());
    // kill_on_drop covers the moment before the job holds the child.
    cmd.kill_on_drop(true);
    let job = win::OwnedJob::new()?;
    let mut child = cmd.spawn()?;
    let handle = child.raw_handle().ok_or_else(|| std::io::Error::other("the program ended at once"))?;
    if let Err(e) = job.assign(handle) {
        // Windows refuses a process that has already exited; that is a finished program, not a failed start.
        if !matches!(child.try_wait(), Ok(Some(_))) {
            return Err(e);
        }
    }
    Ok(Tree { child, job })
}

/// Windows children never open a console window.
#[cfg(windows)]
pub(crate) fn creation_flags() -> u32 {
    windows_sys::Win32::System::Threading::CREATE_NO_WINDOW
}

impl Tree {
    /// Waits for the program to exit. On a timeout or a cancel it kills the tree first.
    pub async fn wait(&mut self, timeout: Duration, cancel: &CancellationToken) -> Result<ExitStatus, Stop> {
        let stop = tokio::select! {
            status = self.child.wait() => return status.map_err(|e| Stop::Io(e.to_string())),
            _ = tokio::time::sleep(timeout) => Stop::Timeout,
            _ = cancel.cancelled() => Stop::Cancelled,
        };
        self.kill();
        let _ = tokio::time::timeout(REAP_GRACE, self.child.wait()).await;
        Err(stop)
    }

    /// Kills the whole tree. Safe to call again.
    pub fn kill(&mut self) {
        #[cfg(windows)]
        self.job.terminate();
        #[cfg(unix)]
        if let Some(pgid) = self.pgid.take() {
            // SAFETY: `kill` takes no pointers. `pgid` is the group `spawn_tree` created and is above 1, so this
            // can never mean "my own group" (0) or "everything" (-1).
            unsafe { libc::kill(-pgid, libc::SIGKILL) };
        }
    }

    /// The child's pid, or `None` once it has been collected.
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// The process group `spawn_tree` created (the child's pid), until `kill` clears it. Plan 4 Task 5 records it.
    #[cfg(unix)]
    pub fn pgid(&self) -> Option<i32> {
        self.pgid
    }

    pub fn stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(windows)]
mod win {
    use std::io::{Error, Result};
    use std::mem::size_of;
    use std::os::windows::io::RawHandle;
    use std::ptr::null;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject};

    /// A Job Object that kills its members when the last handle closes, however the app ended.
    pub struct OwnedJob(HANDLE);

    // SAFETY: a job handle is a kernel handle, usable from any thread.
    unsafe impl Send for OwnedJob {}
    // SAFETY: as above; the job calls used here are thread-safe.
    unsafe impl Sync for OwnedJob {}

    impl OwnedJob {
        pub fn new() -> Result<OwnedJob> {
            // SAFETY: both arguments may be null (default security, unnamed job).
            let handle = unsafe { CreateJobObjectW(null(), null()) };
            if handle.is_null() {
                return Err(Error::last_os_error());
            }
            let job = OwnedJob(handle);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
            // SAFETY: `info` is a live value of the type the class asks for, and `size` is its size.
            let ok = unsafe { SetInformationJobObject(job.0, JobObjectExtendedLimitInformation, (&raw const info).cast(), size) };
            if ok == 0 { Err(Error::last_os_error()) } else { Ok(job) }
        }

        pub fn assign(&self, process: RawHandle) -> Result<()> {
            // SAFETY: both handles are open; `process` belongs to a child we still hold.
            let ok = unsafe { AssignProcessToJobObject(self.0, process as HANDLE) };
            if ok == 0 { Err(Error::last_os_error()) } else { Ok(()) }
        }

        pub fn terminate(&self) {
            // SAFETY: the handle is open until `drop`.
            unsafe { TerminateJobObject(self.0, 1) };
        }
    }

    impl Drop for OwnedJob {
        fn drop(&mut self) {
            // SAFETY: we own the handle and close it once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn no_console_window_flag_set_on_windows() {
        assert_ne!(creation_flags() & windows_sys::Win32::System::Threading::CREATE_NO_WINDOW, 0);
    }
}
