use super::*;
use std::sync::Mutex;
use std::time::Instant;

const SEP: char = '\u{1f}';

/// The fake LibreOffice: this test binary, run with `RP_FAKE_SOFFICE`, a comma list of steps. Does nothing otherwise.
#[test]
fn fake_soffice() {
    let Ok(steps) = std::env::var("RP_FAKE_SOFFICE") else { return };
    let args: Vec<String> = std::env::var("RP_FAKE_ARGS").unwrap().split(SEP).map(String::from).collect();
    let mark = |word: &str| {
        use std::io::Write;
        let path = std::env::var("RP_FAKE_MARK").unwrap();
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        writeln!(file, "{word} {now}").unwrap();
    };
    let profile = || PathBuf::from(std::env::var("RP_FAKE_PROFILE").unwrap());
    let write_pdf = || {
        let at = args.iter().position(|a| a == "--outdir").unwrap();
        let input = Path::new(&args[at + 2]);
        std::fs::write(Path::new(&args[at + 1]).join(input.file_stem().unwrap()).with_extension("pdf"), "%PDF-1.7").unwrap();
    };
    mark("start");
    for step in steps.split(',') {
        match step.split_once(':') {
            Some(("sleep", ms)) => std::thread::sleep(Duration::from_millis(ms.parse().unwrap())),
            _ if step == "pdf" => write_pdf(),
            _ if step == "warm" && args.iter().any(|a| a == "--terminate_after_init") => mark("warm"),
            _ if step == "fail" => std::process::exit(1),
            _ if step == "hang" => std::thread::sleep(Duration::from_secs(120)),
            _ if step == "nolock" && profile().join(".lock").exists() => std::process::exit(1),
            _ if step == "badprofile" && profile().join("broken").exists() => {
                println!("[Java framework] The User Installation could not be completed.");
                std::process::exit(1);
            }
            _ => {}
        }
    }
    mark("end");
}

struct Run {
    dir: PathBuf,
    lo: Arc<LibreOffice>,
}

impl Run {
    fn new(steps: &str) -> Run {
        let dir = std::env::temp_dir().join(format!("rp-lo-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let path = std::env::current_exe().unwrap();
        let lo = LibreOffice::new(Soffice { path, snap: false }, &dir);
        let run = Run { dir, lo };
        run.set_steps(steps);
        run
    }

    fn set_steps(&self, steps: &str) {
        let env = [("RP_FAKE_SOFFICE", steps.to_string()), ("RP_FAKE_MARK", self.marks().display().to_string()), ("RP_FAKE_PROFILE", self.lo.profile.display().to_string())];
        *self.lo.extra_env.lock().unwrap() = env.map(|(k, v)| (k.to_string(), v)).into();
    }

    fn marks(&self) -> PathBuf {
        self.dir.join("marks.txt")
    }

    /// `(word, millis)` lines the fake wrote.
    fn read_marks(&self) -> Vec<(String, u128)> {
        let text = std::fs::read_to_string(self.marks()).unwrap_or_default();
        text.lines().map(|l| l.split_once(' ').unwrap()).map(|(w, t)| (w.to_string(), t.parse().unwrap())).collect()
    }

    fn input(&self) -> PathBuf {
        let path = self.dir.join("work").join("input.pptx");
        std::fs::write(&path, "x").unwrap();
        path
    }

    fn work(&self) -> PathBuf {
        self.dir.join("work")
    }

    async fn convert(&self) -> Result<PathBuf, String> {
        self.lo.to_pdf(&self.input(), &self.work(), &|_| (), &CancellationToken::new()).await
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn strs(args: &[OsString]) -> Vec<String> {
    args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

#[cfg(unix)]
#[test]
fn args_use_private_profile_url_and_pdf_filter_as_one_argument() {
    let lo = LibreOffice::new(Soffice { path: "soffice".into(), snap: false }, Path::new("/data/Jake Smith"));
    let args = strs(&lo.args(Path::new("/w/input.pptx"), Path::new("/w/pdf")).unwrap());
    assert!(args.contains(&"-env:UserInstallation=file:///data/Jake%20Smith/deps/libreoffice-profile/".to_string()), "{args:?}");
    let at = args.iter().position(|a| a == "--convert-to").unwrap();
    assert!(args[at + 1].starts_with("pdf:impress_pdf_Export:{"));
    assert!(args[at + 1].ends_with("}}"));
    assert!(args[at + 1].contains(r#""ExportHiddenSlides":{"type":"boolean","value":"false"}"#));
    assert_eq!(args[at + 2..], ["--outdir", "/w/pdf", "/w/input.pptx"]);
    for flag in FLAGS {
        assert!(args.iter().any(|a| a == flag), "{flag}");
    }
}

#[cfg(unix)]
#[test]
fn args_strip_verbatim_prefix() {
    let lo = LibreOffice::new(Soffice { path: "soffice".into(), snap: false }, Path::new("/data"));
    let args = strs(&lo.args(Path::new(r"\\?\C:\w\input.pptx"), Path::new(r"\\?\C:\w\pdf")).unwrap());
    assert_eq!(args[args.len() - 2..], [r"C:\w\pdf", r"C:\w\input.pptx"]);
}

#[test]
fn timeout_scales_with_size_and_caps_at_600s() {
    const MB: u64 = 1024 * 1024;
    assert_eq!(LibreOffice::timeout_for(0), Duration::from_secs(120));
    assert_eq!(LibreOffice::timeout_for(49 * MB), Duration::from_secs(120));
    assert_eq!(LibreOffice::timeout_for(50 * MB), Duration::from_secs(150));
    assert_eq!(LibreOffice::timeout_for(500 * MB), Duration::from_secs(420));
    assert_eq!(LibreOffice::timeout_for(800 * MB), Duration::from_secs(600));
    assert_eq!(LibreOffice::timeout_for(u64::MAX / 40), Duration::from_secs(600));
}

#[tokio::test]
async fn pdf_is_named_after_the_input_in_the_pdf_folder() {
    let run = Run::new("pdf");
    assert_eq!(run.convert().await.unwrap(), run.work().join("pdf").join("input.pdf"));
}

#[tokio::test]
async fn stale_lock_removed_before_run() {
    let run = Run::new("nolock,pdf");
    std::fs::create_dir_all(&run.lo.profile).unwrap();
    std::fs::write(run.lo.profile.join(".lock"), "old").unwrap();
    assert!(run.convert().await.is_ok());
}

#[tokio::test]
async fn second_conversion_waits_for_first() {
    let run = Run::new("sleep:600,pdf");
    let first = {
        let (lo, input, work) = (run.lo.clone(), run.input(), run.work());
        tokio::spawn(async move { lo.to_pdf(&input, &work, &|_| panic!("the first run does not wait"), &CancellationToken::new()).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let heard = Mutex::new(Vec::new());
    let work2 = run.dir.join("work2");
    std::fs::create_dir_all(&work2).unwrap();
    let second = run.lo.to_pdf(&run.input(), &work2, &|on| heard.lock().unwrap().push(on), &CancellationToken::new()).await;
    assert!(first.await.unwrap().is_ok());
    assert!(second.is_ok());
    assert_eq!(*heard.lock().unwrap(), [true, false]);
    let marks = run.read_marks();
    let words: Vec<_> = marks.iter().map(|(w, _)| w.as_str()).collect();
    assert_eq!(words, ["start", "end", "start", "end"]);
    assert!(marks[1].1 <= marks[2].1);
}

#[tokio::test]
async fn import_waits_behind_warm_up() {
    let run = Run::new("sleep:600");
    let warm = tokio::spawn(run.lo.clone().warm_up());
    tokio::time::sleep(Duration::from_millis(100)).await;
    run.set_steps("pdf");
    let heard = Mutex::new(Vec::new());
    let done = run.lo.to_pdf(&run.input(), &run.work(), &|on| heard.lock().unwrap().push(on), &CancellationToken::new()).await;
    warm.await.unwrap();
    assert!(done.is_ok());
    assert_eq!(*heard.lock().unwrap(), [true, false]);
    let words: Vec<_> = run.read_marks().into_iter().map(|(w, _)| w).collect();
    assert_eq!(words, ["start", "end", "start", "end"]);
}

#[tokio::test]
async fn warm_up_runs_once_with_terminate_after_init() {
    let run = Run::new("warm");
    run.lo.clone().warm_up().await;
    run.lo.clone().warm_up().await;
    let words: Vec<_> = run.read_marks().into_iter().map(|(w, _)| w).collect();
    assert_eq!(words, ["start", "warm", "end"]);
}

#[tokio::test]
async fn cancel_while_waiting_starts_nothing() {
    let run = Run::new("sleep:800");
    let warm = tokio::spawn(run.lo.clone().warm_up());
    tokio::time::sleep(Duration::from_millis(100)).await;
    let cancel = CancellationToken::new();
    let hit = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        hit.cancel();
    });
    let result = run.lo.to_pdf(&run.input(), &run.work(), &|_| (), &cancel).await;
    assert_eq!(result.unwrap_err(), CANCELLED);
    warm.await.unwrap();
    assert_eq!(run.read_marks().len(), 2, "only warm-up ran");
}

#[tokio::test]
async fn already_cancelled_starts_nothing() {
    let run = Run::new("pdf");
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(run.lo.to_pdf(&run.input(), &run.work(), &|_| (), &cancel).await.unwrap_err(), CANCELLED);
    assert!(run.read_marks().is_empty());
}

#[tokio::test]
async fn cancel_kills_the_run_and_frees_the_turn() {
    let run = Run::new("hang");
    let cancel = CancellationToken::new();
    let hit = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        hit.cancel();
    });
    let start = Instant::now();
    assert_eq!(run.lo.to_pdf(&run.input(), &run.work(), &|_| (), &cancel).await.unwrap_err(), CANCELLED);
    assert!(start.elapsed() < Duration::from_secs(5));
    run.set_steps("pdf");
    assert!(run.convert().await.is_ok());
}

#[tokio::test]
async fn timeout_stops_the_run_with_the_plain_message() {
    let run = Run::new("hang");
    *run.lo.test_timeout.lock().unwrap() = Some(Duration::from_millis(200));
    let e = run.convert().await.unwrap_err();
    assert_eq!(e, "LibreOffice took too long and was stopped. Try again; the first import after starting the computer is slower.");
}

#[tokio::test]
async fn missing_pdf_is_an_error_even_with_exit_0() {
    let run = Run::new("");
    assert_eq!(run.convert().await.unwrap_err(), "LibreOffice couldn't read this presentation.");
}

#[tokio::test]
async fn failed_run_reports_unreadable() {
    let run = Run::new("fail");
    assert_eq!(run.convert().await.unwrap_err(), "LibreOffice couldn't read this presentation.");
    assert_eq!(run.read_marks().len(), 1, "a plain failure is not retried");
}

#[tokio::test]
async fn broken_profile_is_deleted_and_retried_once() {
    let run = Run::new("badprofile,pdf");
    std::fs::create_dir_all(&run.lo.profile).unwrap();
    std::fs::write(run.lo.profile.join("broken"), "").unwrap();
    assert!(run.convert().await.is_ok());
    assert!(!run.lo.profile.join("broken").exists());
    let starts = run.read_marks().iter().filter(|(w, _)| w == "start").count();
    assert_eq!(starts, 2);
}

#[tokio::test]
async fn a_program_that_will_not_start_gets_a_plain_message() {
    let run = Run::new("pdf");
    let lo = LibreOffice::new(Soffice { path: run.dir.join("no-such-soffice"), snap: false }, &run.dir);
    let e = lo.to_pdf(&run.input(), &run.work(), &|_| (), &CancellationToken::new()).await.unwrap_err();
    assert_eq!(e, "LibreOffice could not be started. Reinstall it and try again.");
}

#[tokio::test]
async fn snap_soffice_returns_snap_message_without_spawning() {
    let run = Run::new("pdf");
    let lo = LibreOffice::new(Soffice { path: std::env::current_exe().unwrap(), snap: true }, &run.dir);
    *lo.extra_env.lock().unwrap() = run.lo.extra_env.lock().unwrap().clone();
    let e = lo.to_pdf(&run.input(), &run.work(), &|_| (), &CancellationToken::new()).await.unwrap_err();
    assert!(e.starts_with("LibreOffice from Snap can't read"));
    lo.clone().warm_up().await;
    assert!(run.read_marks().is_empty());
}

#[cfg(unix)]
mod leftovers {
    use super::*;
    use std::os::unix::process::CommandExt;

    fn fake_hang(run: &Run) -> std::process::Child {
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "import::libreoffice::tests::fake_soffice", "--nocapture"]);
        cmd.env("RP_FAKE_SOFFICE", "hang").env("RP_FAKE_ARGS", "").env("RP_FAKE_MARK", run.marks()).process_group(0);
        cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()
    }

    fn write_pgid(run: &Run, id: u32) -> PathBuf {
        let deps = run.lo.deps_dir();
        std::fs::create_dir_all(&deps).unwrap();
        std::fs::write(deps.join(PGID_FILE), format!("{id} 1\n")).unwrap();
        deps
    }

    #[test]
    fn leftover_group_is_killed_before_lock_removed() {
        let run = Run::new("pdf");
        let mut child = fake_hang(&run);
        let deps = write_pgid(&run, child.id());
        let start = Instant::now();
        while run.read_marks().is_empty() {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(10));
        }
        // A zombie still counts as alive, so something must collect it, as init does for a real leftover.
        let reaper = std::thread::spawn(move || child.wait().unwrap());
        kill_leftover(&deps);
        assert!(!reaper.join().unwrap().success());
        assert!(!deps.join(PGID_FILE).exists());
    }

    #[test]
    fn pgid_file_with_unrelated_process_is_ignored() {
        let run = Run::new("pdf");
        let mut sleeper = std::process::Command::new("sleep").arg("30").process_group(0).spawn().unwrap();
        let deps = write_pgid(&run, sleeper.id());
        kill_leftover(&deps);
        assert!(sleeper.try_wait().unwrap().is_none(), "still running");
        assert!(!deps.join(PGID_FILE).exists());
        sleeper.kill().unwrap();
        sleeper.wait().unwrap();
    }

    #[test]
    fn nonsense_pgid_file_is_removed() {
        for text in ["", "abc", "1 2", "-1 2", "0 0"] {
            let run = Run::new("pdf");
            let deps = write_pgid(&run, 0);
            std::fs::write(deps.join(PGID_FILE), text).unwrap();
            kill_leftover(&deps);
            assert!(!deps.join(PGID_FILE).exists(), "{text:?}");
        }
    }

    /// Real LibreOffice: the launcher and `soffice.bin` in one group, as a crashed app leaves them.
    #[test]
    fn real_leftover_group_is_killed() {
        if std::env::var_os("RP_TEST_SOFFICE").is_none() {
            return;
        }
        let run = Run::new("");
        let soffice = tools::find_soffice().unwrap();
        let mut cmd = std::process::Command::new(&soffice.path);
        cmd.args(run.lo.common_args().unwrap()).env("SAL_USE_VCLPLUGIN", "svp").process_group(0);
        let mut child = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let pgid = child.id() as i32;
        let deps = write_pgid(&run, child.id());
        std::thread::sleep(Duration::from_secs(3));
        assert!(group_alive(pgid) && is_libreoffice(pgid), "{}", process_name(pgid));
        let reaper = std::thread::spawn(move || child.wait().unwrap());
        kill_leftover(&deps);
        reaper.join().unwrap();
        assert!(!group_alive(pgid));
    }

    #[tokio::test]
    async fn conversion_records_the_group_and_clears_it() {
        let run = Run::new("sleep:400,pdf");
        let lo = run.lo.clone();
        let (input, work) = (run.input(), run.work());
        let job = tokio::spawn(async move { lo.to_pdf(&input, &work, &|_| (), &CancellationToken::new()).await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        let text = std::fs::read_to_string(run.lo.deps_dir().join(PGID_FILE)).unwrap();
        assert!(text.split_whitespace().next().unwrap().parse::<i32>().unwrap() > 1);
        assert_eq!(text.split_whitespace().count(), 2);
        assert!(job.await.unwrap().is_ok());
        assert!(!run.lo.deps_dir().join(PGID_FILE).exists());
    }
}

/// Real LibreOffice, in a data folder with a space and an accent (Review Focus 5).
#[tokio::test]
async fn converts_sample_pptx() {
    if std::env::var_os("RP_TEST_SOFFICE").is_none() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("rp lo é {}", uuid::Uuid::new_v4()));
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let input = work.join("input.pptx");
    std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("experiments/media/sample.pptx"), &input).unwrap();
    let lo = LibreOffice::new(tools::find_soffice().unwrap(), &dir);
    lo.clone().warm_up().await;
    let pdf_path = lo.to_pdf(&input, &work, &|_| (), &CancellationToken::new()).await.unwrap();
    pdf::check_pdf(&pdf_path).unwrap();
    assert!(!lo.deps_dir().join(PGID_FILE).exists());
    if std::env::var_os("RP_PDFIUM").is_some() {
        let renderer = pdf::PdfRenderer::start(&dir).unwrap();
        let files = renderer.render(pdf_path, work.join("slides"), Some(16.0 / 9.0), Box::new(|_, _| ()), CancellationToken::new()).await.unwrap();
        assert_eq!(files.len(), 3);
        for (file, colour) in files.iter().zip([[255, 0, 0], [0, 255, 0], [0, 0, 255]]) {
            let image = image::open(file).unwrap().to_rgb8();
            assert_eq!(image.dimensions(), (1920, 1080));
            assert_eq!(image.get_pixel(40, 40).0, colour);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
