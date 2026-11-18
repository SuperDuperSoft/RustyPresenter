//! The last committed frame on disk, so a crashed app can put it back. Saved on its own thread:
//! the room's reports never wait for the disk.
use crate::{live::{Cursor, Frame, Live, LiveItem}, store::{now_stamp, write_atomic}};
use serde::{Deserialize, Serialize};
use std::{fs, io, path::{Path, PathBuf}, sync::{Arc, Condvar, Mutex}, thread::JoinHandle, time::Duration};

const RETRIES: u32 = 10;
const RETRY_WAIT: Duration = Duration::from_millis(30);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LiveState {
    pub v: u32,
    pub seq: u64,
    pub saved_at: String,
    pub frame: Frame,
    pub cursor: Option<Cursor>,
    pub item: Option<LiveItem>,
    /// Playback instance → seconds.
    pub positions: Vec<(u64, f64)>,
    /// Open window roles: "room", "stage".
    pub windows: Vec<String>,
    /// `Live::clear_epoch`, so a restart never lowers it.
    #[serde(default)]
    pub clear_epoch: u64,
    /// `Live::next_inst`, so a restart never reuses an instance number.
    #[serde(default)]
    pub next_inst: u64,
}

const VERSION: u32 = 1;

pub fn path(root: &Path) -> PathBuf { root.join("live-state.json") }

/// The saved state, if there is a good one. A missing or damaged file just means nothing to restore.
pub fn load(root: &Path) -> Option<LiveState> {
    let s: LiveState = serde_json::from_slice(&fs::read(path(root)).ok()?).ok()?;
    (s.v == VERSION).then_some(s)
}

/// Moves aside a saved state that crashed the app, keeping it for the log reader.
pub fn quarantine(root: &Path) {
    if let Err(e) = fs::rename(path(root), root.join("live-state.crashed.json")) { eprintln!("The saved live state could not be moved aside: {e}"); }
}

pub fn clear(root: &Path) { let _ = fs::remove_file(path(root)); }

/// Antivirus, the indexer or OneDrive may hold the file for a moment.
fn locked(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied || (cfg!(windows) && e.raw_os_error() == Some(32))
}

fn retry(mut write: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    for _ in 0..RETRIES {
        match write() {
            Err(e) if locked(&e) => std::thread::sleep(RETRY_WAIT),
            done => return done,
        }
    }
    write()
}

pub fn write_with_retry(path: &Path, bytes: &[u8]) -> io::Result<()> { retry(|| write_atomic(path, bytes)) }

/// What is on air now, or `None` before anything has been committed. Positions come from the
/// players' reports, for the instances in that frame.
pub fn snapshot(live: &Live, windows: Vec<String>, seq: u64) -> Option<LiveState> {
    let (frame, cursor, item) = live.committed()?;
    let positions = [&live.media_status, &live.audio_status].into_iter().flatten()
        .filter(|p| frame.instances().any(|i| i == p.inst)).map(|p| (p.inst, p.position)).collect();
    Some(LiveState { v: VERSION, seq, saved_at: now_stamp(), frame, cursor, item, positions, windows, clear_epoch: live.clear_epoch, next_inst: live.next_inst() })
}

#[derive(Default)]
struct Queue { next: Option<LiveState>, stop: bool, #[cfg(test)] paused: bool }

/// Writes the newest submitted state on its own thread. Older ones waiting are replaced.
pub struct StateWriter { queue: Arc<(Mutex<Queue>, Condvar)>, thread: Option<JoinHandle<()>> }

impl StateWriter {
    pub fn start(root: PathBuf) -> StateWriter {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let shared = queue.clone();
        let thread = std::thread::Builder::new().name("live-state".into()).spawn(move || write_loop(&root, &shared))
            .map_err(|e| eprintln!("The live state will not be saved: {e}")).ok();
        StateWriter { queue, thread }
    }

    /// Never waits for the disk.
    pub fn submit(&self, s: LiveState) {
        self.queue.0.lock().unwrap().next = Some(s);
        self.queue.1.notify_one();
    }

    /// Writes what is waiting, then ends the thread.
    pub fn flush_and_stop(mut self) {
        self.queue.0.lock().unwrap().stop = true;
        self.queue.1.notify_one();
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }

    #[cfg(test)]
    fn pause(&self, on: bool) {
        self.queue.0.lock().unwrap().paused = on;
        self.queue.1.notify_one();
    }
}

fn write_loop(root: &Path, queue: &(Mutex<Queue>, Condvar)) {
    super::panic::mark_critical();
    let file = path(root);
    loop {
        let (state, stop) = {
            let mut q = queue.1.wait_while(queue.0.lock().unwrap(), |q| q.waiting()).unwrap();
            (q.next.take(), q.stop)
        };
        if let Some(s) = state {
            let written = serde_json::to_vec(&s).map_err(io::Error::other).and_then(|bytes| write_with_retry(&file, &bytes));
            if let Err(e) = written { eprintln!("The live state could not be saved: {e}"); }
        }
        if stop { return; }
    }
}

impl Queue {
    #[cfg(not(test))]
    fn waiting(&self) -> bool { self.next.is_none() && !self.stop }
    #[cfg(test)]
    fn waiting(&self) -> bool { self.paused || (self.next.is_none() && !self.stop) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn root() -> PathBuf {
        let p = std::env::temp_dir().join(format!("rp-state-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn state(seq: u64) -> LiveState {
        LiveState { v: 1, seq, saved_at: "now".into(), frame: Frame::default(), cursor: None, item: None, positions: vec![(3, 1.5)], windows: vec!["room".into()], clear_epoch: 0, next_inst: 0 }
    }

    #[test]
    fn load_missing_garbage_and_wrong_version_are_none() {
        let r = root();
        assert_eq!(load(&r), None, "missing");
        fs::write(path(&r), "{ not json").unwrap();
        assert_eq!(load(&r), None, "garbage");
        fs::write(path(&r), serde_json::to_vec(&LiveState { v: 2, ..state(1) }).unwrap()).unwrap();
        assert_eq!(load(&r), None, "wrong version");
        fs::write(path(&r), serde_json::to_vec(&state(4)).unwrap()).unwrap();
        assert_eq!(load(&r), Some(state(4)));
    }

    #[test]
    fn state_is_kept_in_the_data_folder() {
        assert_eq!(path(Path::new("data")), Path::new("data").join("live-state.json"));
    }

    #[test]
    fn quarantine_renames() {
        let r = root();
        fs::write(path(&r), "x").unwrap();
        quarantine(&r);
        assert!(!path(&r).exists());
        assert_eq!(fs::read_to_string(r.join("live-state.crashed.json")).unwrap(), "x");
    }

    #[test]
    fn quarantine_replaces_an_older_crashed_file() {
        let r = root();
        fs::write(r.join("live-state.crashed.json"), "old").unwrap();
        fs::write(path(&r), "new").unwrap();
        quarantine(&r);
        assert_eq!(fs::read_to_string(r.join("live-state.crashed.json")).unwrap(), "new");
    }

    #[test]
    fn clear_removes_and_does_not_mind_a_missing_file() {
        let r = root();
        fs::write(path(&r), "x").unwrap();
        clear(&r);
        assert!(!path(&r).exists());
        clear(&r);
    }

    #[test]
    fn writer_keeps_only_latest() {
        let r = root();
        let w = StateWriter::start(r.clone());
        w.pause(true);
        for seq in 1..=100 { w.submit(state(seq)); }
        assert_eq!(load(&r), None, "nothing is written while paused");
        w.pause(false);
        w.flush_and_stop();
        assert_eq!(load(&r).unwrap().seq, 100);
    }

    #[test]
    fn stopping_writes_what_is_waiting() {
        let r = root();
        let w = StateWriter::start(r.clone());
        w.pause(true);
        w.submit(state(7));
        w.pause(false);
        w.flush_and_stop();
        assert_eq!(load(&r).unwrap().seq, 7);
    }

    #[test]
    fn locked_target_is_retried_ten_times() {
        let tries = Cell::new(0);
        let r = retry(|| { tries.set(tries.get() + 1); if tries.get() < 11 { Err(io::ErrorKind::PermissionDenied.into()) } else { Ok(()) } });
        assert!(r.is_ok());
        assert_eq!(tries.get(), 11, "the first try and ten retries");
        tries.set(0);
        let r = retry(|| { tries.set(tries.get() + 1); Err(io::ErrorKind::PermissionDenied.into()) });
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(tries.get(), 11);
    }

    #[test]
    fn other_errors_are_not_retried() {
        let tries = Cell::new(0);
        let r = retry(|| { tries.set(tries.get() + 1); Err(io::ErrorKind::NotFound.into()) });
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(tries.get(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn sharing_violation_is_retried() {
        let tries = Cell::new(0);
        let _ = retry(|| { tries.set(tries.get() + 1); Err(io::Error::from_raw_os_error(32)) });
        assert_eq!(tries.get(), 11);
    }

    #[cfg(windows)]
    #[test]
    fn rename_retries_when_target_is_locked() {
        use std::os::windows::fs::OpenOptionsExt;
        let r = root();
        let target = path(&r);
        fs::write(&target, "old").unwrap();
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(&target).unwrap();
        let releaser = std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_millis(100)); drop(held); });
        write_with_retry(&target, b"new").unwrap();
        releaser.join().unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }
}
