//! Plan 4 Task 2: a converter and everything it started die together.
//! Each test re-runs this test binary as a helper: `proc_helper` is a no-op unless `RP_PROC_HELPER` is set.
//! The helpers share the test's stdout pipe, so the pipe reaches end-of-file only when both are gone.
//! That also works when a killed process stays a zombie, where a pid check would not.
use rustypresenter::platform::proc::{Stop, Tree, spawn_tree};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::{ChildStdout, Command};
use tokio_util::sync::CancellationToken;

const ROLE: &str = "RP_PROC_HELPER";
const MARK: &str = "RP_PROC_MARK";
const LONG: Duration = Duration::from_secs(60);

fn helper(role: &str, mark: &Path) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "proc_helper", "--nocapture"]).env(ROLE, role).env(MARK, mark).stdout(Stdio::piped()).stderr(Stdio::null());
    c
}

/// Not a real test: the helper process. `parent` starts `grandchild`, which writes the mark file.
#[test]
fn proc_helper() {
    let Some(role) = std::env::var_os(ROLE) else { return };
    match role.to_str() {
        Some("parent") => {
            let mut c = std::process::Command::new(std::env::current_exe().unwrap());
            c.args(["--exact", "proc_helper", "--nocapture"]).env(ROLE, "grandchild").stderr(Stdio::null());
            let _ = c.spawn().unwrap().wait();
        }
        Some("grandchild") => std::fs::write(std::env::var_os(MARK).unwrap(), "up").unwrap(),
        Some("exit3") => std::process::exit(3),
        _ => panic!("unknown role"),
    }
    std::thread::sleep(LONG);
}

fn mark_file() -> PathBuf {
    std::env::temp_dir().join(format!("rp-proc-{}", uuid::Uuid::new_v4()))
}

async fn start_family() -> (Tree, ChildStdout, PathBuf) {
    let mark = mark_file();
    let mut tree = spawn_tree(helper("parent", &mark)).unwrap();
    let out = tree.stdout().unwrap();
    let started = Instant::now();
    while !mark.exists() {
        assert!(started.elapsed() < Duration::from_secs(30), "the grandchild never started");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    (tree, out, mark)
}

/// Every helper is gone when nobody holds the pipe any more.
async fn assert_all_gone(mut out: ChildStdout, mark: &Path) {
    let mut sink = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(2), out.read_to_end(&mut sink)).await;
    let _ = std::fs::remove_file(mark);
    assert!(read.is_ok(), "something in the tree is still alive");
}

#[tokio::test]
async fn drop_kills_grandchild() {
    let (tree, out, mark) = start_family().await;
    drop(tree);
    assert_all_gone(out, &mark).await;
}

#[tokio::test]
async fn timeout_kills_tree_and_reports_timeout() {
    let (mut tree, out, mark) = start_family().await;
    let got = tree.wait(Duration::from_millis(50), &CancellationToken::new()).await;
    assert_eq!(got, Err(Stop::Timeout));
    assert_all_gone(out, &mark).await;
}

#[tokio::test]
async fn cancel_kills_tree_and_reports_cancelled() {
    let (mut tree, out, mark) = start_family().await;
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        trigger.cancel();
    });
    assert_eq!(tree.wait(LONG, &cancel).await, Err(Stop::Cancelled));
    assert_all_gone(out, &mark).await;
}

#[tokio::test]
async fn normal_exit_returns_status() {
    let mut tree = spawn_tree(helper("exit3", &mark_file())).unwrap();
    let status = tree.wait(LONG, &CancellationToken::new()).await.unwrap();
    assert_eq!(status.code(), Some(3));
}

#[tokio::test]
async fn kill_kills_the_tree_and_can_be_repeated() {
    let (mut tree, out, mark) = start_family().await;
    tree.kill();
    assert_all_gone(out, &mark).await;
    // A second kill must not signal anything else: on Unix a cleared group id would mean this very process's group.
    tree.kill();
    drop(tree);
}

#[tokio::test]
async fn a_program_that_cannot_start_is_an_error() {
    let c = Command::new(std::env::temp_dir().join("rp-no-such-program"));
    assert!(spawn_tree(c).is_err());
}
