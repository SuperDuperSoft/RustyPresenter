use super::*;
use std::{sync::{Arc, Mutex}, time::SystemTime};

const NAME: &str = "import::powerpoint::tests::fake_powershell";
const FAKE: &str = "RP_FAKE_PS";
const MARK: &str = "RP_FAKE_PS_MARK";
const CLEAN: &str = "RP_FAKE_PS_CLEAN";

fn quiet(_: bool) {}
fn no_progress(_: usize, _: usize) {}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()
}

/// Not a real test: PowerShell's stand-in, this test binary run again. Steps come from the child's environment,
/// comma separated, for the export script: `say:<text>` (`+` is a space), `png:<i>:<w>:<h>`, `sleep:<ms>`, `hang`,
/// `fail:<text>` (says it on stderr and exits 1), `plain:<text>`. Cleanup runs do `RP_FAKE_PS_CLEAN`'s steps, default
/// none. Each run adds a line to the file in `RP_FAKE_PS_MARK`: `export <ms> <args>` or `cleanup <ms> <args>`, then `end <ms>`.
#[test]
fn fake_powershell() {
    use std::io::Write;
    let Ok(steps) = std::env::var(FAKE) else { return };
    let mark = std::env::var(MARK).unwrap();
    let log = |line: String| writeln!(std::fs::OpenOptions::new().create(true).append(true).open(&mark).unwrap(), "{line}").unwrap();
    let args: Vec<String> = std::env::args().collect();
    let tail = args.iter().position(|a| a == "--").map_or(&args[..], |i| &args[i + 1..]);
    let cleanup = tail.iter().any(|a| a.ends_with(CLEANUP_FILE));
    log(format!("{} {} {}", if cleanup { "cleanup" } else { "export" }, now_ms(), tail.join(" ")));
    let out = tail.iter().position(|a| a == "-Out").map(|i| PathBuf::from(&tail[i + 1]));
    let steps = if cleanup { std::env::var(CLEAN).unwrap_or_default() } else { steps };
    for step in steps.split(',').filter(|s| !s.is_empty()) {
        let (kind, value) = step.split_once(':').unwrap_or((step, ""));
        match kind {
            "say" => { println!("{}", value.replace('+', " ")); std::io::stdout().flush().unwrap(); }
            "sleep" => std::thread::sleep(Duration::from_millis(value.parse().unwrap())),
            "hang" => std::thread::sleep(Duration::from_secs(60)),
            "png" => {
                let v: Vec<u32> = value.split(':').map(|n| n.parse().unwrap()).collect();
                image::RgbImage::new(v[1], v[2]).save(out.as_ref().unwrap().join(slide_file(v[0] as usize))).unwrap();
            }
            "fail" => { eprintln!("{value}"); log(format!("end {}", now_ms())); std::process::exit(1); }
            other => panic!("unknown step {other}"),
        }
    }
    log(format!("end {}", now_ms()));
    std::process::exit(0);
}

struct Fx {
    dir: PathBuf,
    ppt: PowerPoint,
    input: PathBuf,
    out: PathBuf,
    mark: PathBuf,
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Fx {
    fn marks(&self) -> Vec<String> {
        std::fs::read_to_string(&self.mark).unwrap_or_default().lines().map(str::to_string).collect()
    }
    fn cleanup_line(&self) -> Option<String> {
        self.marks().into_iter().find(|l| l.starts_with("cleanup "))
    }
    async fn run(&self) -> Result<Vec<PathBuf>, String> {
        self.ppt.export(&self.input, &self.out, &quiet, &no_progress, &CancellationToken::new()).await
    }
}

fn fixture_with(steps: &str, clean: &str) -> Fx {
    let dir = std::env::temp_dir().join(format!("rp-ppt-{}", uuid::Uuid::new_v4()));
    let out = dir.join("slides");
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input.pptx");
    std::fs::write(&input, "deck").unwrap();
    let mark = dir.join("mark");
    let mut ppt = PowerPoint::new();
    ppt.program = std::env::current_exe().unwrap();
    ppt.extra_env = vec![(FAKE.into(), steps.into()), (MARK.into(), mark.display().to_string()), (CLEAN.into(), clean.into())];
    ppt.prefix_args = ["--exact", NAME, "--nocapture", "--"].map(OsString::from).to_vec();
    Fx { dir, ppt, input, out, mark }
}

fn fixture(steps: &str) -> Fx {
    fixture_with(steps, "")
}

/// A whole good run of `n` slides at 16:9.
fn good(n: usize) -> String {
    let mut s = format!("say:COUNT+{n},say:SIZE+960+540");
    for i in 1..=n {
        s += &format!(",png:{i}:1920:1080,say:SLIDE+{i}+{n}");
    }
    s
}

#[test]
fn parse_line_reads_count_size_slide_and_marker() {
    assert_eq!(parse_line("COUNT 12"), Some(Line::Count(12)));
    assert_eq!(parse_line("SIZE 960 540"), Some(Line::Size(960.0, 540.0)));
    assert_eq!(parse_line("SIZE 720.5 405.25\r"), Some(Line::Size(720.5, 405.25)));
    assert_eq!(parse_line("SLIDE 3 12"), Some(Line::Slide(3, 12)));
    assert_eq!(parse_line("STARTED_BY_US"), Some(Line::StartedByUs));
    for other in ["", "running 1 test", "COUNT", "COUNT x", "COUNT 1 2", "SIZE 960", "SIZE 960,5 540", "SIZE NaN 540", "SIZE inf 540", "SIZE 960 540 1", "SLIDE 3", "SLIDE 3 4 5", "STARTED_BY_US now"] {
        assert_eq!(parse_line(other), None, "{other:?}");
    }
}

/// The first position of `what`, or a failure that names it.
fn at(script: &str, what: &str) -> usize {
    script.find(what).unwrap_or_else(|| panic!("no {what}"))
}

#[test]
fn script_never_quits_powerpoint_when_presentations_were_open() {
    assert!(at(SCRIPT, "$app.AutomationSecurity = 3") < at(SCRIPT, ".Open("));
    assert!(at(SCRIPT, "if ($startedByUs) { Say 'STARTED_BY_US' }") < at(SCRIPT, ".Open("), "known before anything can hang");
    let quits: Vec<&str> = SCRIPT.lines().filter(|l| l.contains("Quit()")).collect();
    assert_eq!(quits.len(), 1);
    for need in ["$startedByUs", "$hadOpen -eq 0", "Presentations.Count -eq 0"] {
        assert!(quits[0].contains(need), "{need}");
    }
    assert!(at(SCRIPT, "$pres.Close()") < at(SCRIPT, "Quit()"));
    assert_eq!(SCRIPT.matches(".Close()").count(), 1, "only its own presentation");
    assert!(at(SCRIPT, "$app.AutomationSecurity = $security") > at(SCRIPT, "$pres.Close()"), "the setting goes back");
}

#[test]
fn script_prints_numbers_the_same_in_every_language() {
    assert!(SCRIPT.contains("$w.ToString($invariant)"));
    assert!(SCRIPT.contains("$h.ToString($invariant)"));
    assert!(SCRIPT.contains("[Console]::Out.WriteLine"), "written at once, not when the script ends");
    assert!(SCRIPT.contains("AwayFromZero"), "rounds like `output_size`, not to even");
    assert!(SCRIPT.contains("'slide-{0:D3}.png'"));
    assert_eq!(slide_file(7), "slide-007.png");
}

/// The script does this sum again in PowerShell. These are the sizes it was run against, under a stand-in for
/// PowerPoint, in a language with a decimal comma.
#[test]
fn script_sizes_are_the_global_rule() {
    for (w, h, want) in [(720.0, 540.0, (1920, 1440)), (720.5, 405.0, (1920, 1080)), (540.0, 720.0, (1620, 2160)), (612.0, 792.0, (1670, 2160)), (1000.0, 333.0, (1920, 640)), (1000.0, 500.5, (1920, 960)), (960.0, 541.0, (1920, 1082))] {
        assert_eq!(output_size(w / h), want, "{w} x {h}");
    }
}

#[test]
fn cleanup_script_closes_only_our_presentation() {
    let s = CLEANUP_SCRIPT;
    assert!(s.contains("GetActiveObject('PowerPoint.Application')"));
    assert!(s.contains("[switch]$StartedByUs"));
    let closes: Vec<&str> = s.lines().filter(|l| l.contains(".Close()")).collect();
    assert_eq!(closes.len(), 1);
    assert!(closes[0].contains("$pres.FullName -eq $Path"));
    let quits: Vec<&str> = s.lines().filter(|l| l.contains("Quit()")).collect();
    assert_eq!(quits.len(), 1);
    assert!(quits[0].contains("$StartedByUs") && quits[0].contains("Presentations.Count -eq 0"));
    for script in [SCRIPT, CLEANUP_SCRIPT] {
        let low = script.to_lowercase();
        assert!(!low.contains("taskkill") && !low.contains("stop-process") && !low.contains(".kill("));
    }
}

#[test]
fn scripts_are_ascii() {
    assert!(SCRIPT.is_ascii() && CLEANUP_SCRIPT.is_ascii());
}

#[test]
fn timeout_grows_with_slides_and_caps_at_600s() {
    assert_eq!(PowerPoint::timeout_for(0), Duration::from_secs(60));
    assert_eq!(PowerPoint::timeout_for(3), Duration::from_secs(90));
    assert_eq!(PowerPoint::timeout_for(54), Duration::from_secs(600));
    assert_eq!(PowerPoint::timeout_for(55), Duration::from_secs(600));
    assert_eq!(PowerPoint::timeout_for(usize::MAX), Duration::from_secs(600));
}

#[tokio::test]
async fn exports_every_slide_in_order_with_progress() {
    let fx = fixture(&good(3));
    let seen = Mutex::new(Vec::new());
    let files = fx.ppt.export(&fx.input, &fx.out, &quiet, &|i, n| seen.lock().unwrap().push((i, n)), &CancellationToken::new()).await.unwrap();
    assert_eq!(files, [1, 2, 3].map(|i| fx.out.join(format!("slide-00{i}.png"))));
    assert_eq!(*seen.lock().unwrap(), [(1, 3), (2, 3), (3, 3)]);
    assert!(fx.cleanup_line().is_none(), "a finished export tidied up after itself");
    let run = &fx.marks()[0];
    assert!(run.contains(EXPORT_FILE), "{run}");
    assert!(run.contains(&format!("-Path {}", fx.input.display())) && run.contains(&format!("-Out {}", fx.out.display())), "{run}");
    assert_eq!(std::fs::read_to_string(fx.out.join(EXPORT_FILE)).unwrap(), SCRIPT);
}

#[tokio::test]
async fn progress_line_arrives_while_powerpoint_works() {
    let fx = fixture("say:COUNT+1,say:SIZE+960+540,png:1:1920:1080,say:SLIDE+1+1,sleep:800");
    let at = Mutex::new(None);
    let start = Instant::now();
    fx.ppt.export(&fx.input, &fx.out, &quiet, &|_, _| *at.lock().unwrap() = Some(start.elapsed()), &CancellationToken::new()).await.unwrap();
    let ended = start.elapsed();
    let at = at.lock().unwrap().unwrap();
    assert!(ended - at >= Duration::from_millis(500), "progress came at {at:?}, end at {ended:?}");
}

#[tokio::test]
async fn no_visible_slides_is_the_plain_message() {
    let fx = fixture("say:COUNT+0,say:SIZE+960+540");
    assert_eq!(fx.run().await.unwrap_err(), strings::PDF_NO_SLIDES);
}

#[tokio::test]
async fn failed_run_never_shows_the_raw_error() {
    let fx = fixture("fail:Exception_0x80004005_at_C:\\secret");
    assert_eq!(fx.run().await.unwrap_err(), strings::PPT_CANNOT_OPEN);
    assert!(fx.cleanup_line().is_none());
}

#[tokio::test]
async fn powerpoint_missing_says_so() {
    let fx = fixture("fail:Retrieving_the_COM_class_factory_failed_80040154");
    assert_eq!(fx.run().await.unwrap_err(), strings::PPT_NOT_STARTED);
}

#[tokio::test]
async fn password_refused_is_the_password_message() {
    let fx = fixture("fail:The_password_is_incorrect");
    assert_eq!(fx.run().await.unwrap_err(), sniff::ENCRYPTED);
}

#[tokio::test]
async fn powershell_not_there_says_powerpoint_could_not_start() {
    let mut fx = fixture("");
    fx.ppt.program = fx.dir.join("nothing-here");
    assert_eq!(fx.run().await.unwrap_err(), strings::PPT_NOT_STARTED);
}

#[tokio::test]
async fn exit_zero_with_a_slide_missing_is_a_failure() {
    let fx = fixture("say:COUNT+2,say:SIZE+960+540,png:1:1920:1080,say:SLIDE+1+2,say:SLIDE+2+2");
    assert_eq!(fx.run().await.unwrap_err(), strings::PPT_CANNOT_OPEN);
}

#[tokio::test]
async fn exit_zero_without_a_count_is_a_failure() {
    let fx = fixture("");
    assert_eq!(fx.run().await.unwrap_err(), strings::PPT_CANNOT_OPEN);
}

#[tokio::test]
async fn pictures_of_the_wrong_size_are_refused() {
    let fx = fixture("say:COUNT+1,say:SIZE+960+540,png:1:1280:720,say:SLIDE+1+1");
    assert_eq!(fx.run().await.unwrap_err(), strings::PPT_CANNOT_OPEN);
}

#[tokio::test]
async fn four_by_three_uses_the_global_size() {
    let fx = fixture("say:COUNT+1,say:SIZE+720+540,png:1:1920:1440,say:SLIDE+1+1");
    assert_eq!(fx.run().await.unwrap().len(), 1);
}

#[tokio::test]
async fn cancel_while_exporting_runs_cleanup_for_our_file() {
    let fx = fixture("say:STARTED_BY_US,say:COUNT+5,say:SIZE+960+540,hang");
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        stop.cancel();
    });
    let start = Instant::now();
    let err = fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &cancel).await.unwrap_err();
    assert_eq!(err, CANCELLED);
    assert!(start.elapsed() < Duration::from_secs(10));
    let line = fx.cleanup_line().expect("cleanup ran");
    assert!(line.contains(CLEANUP_FILE), "{line}");
    assert!(line.contains(&format!("-Path {}", fx.input.display())), "{line}");
    assert!(line.ends_with("-StartedByUs"), "{line}");
}

#[tokio::test]
async fn cleanup_does_not_get_started_by_us_unless_the_export_said_so() {
    let mut fx = fixture("say:COUNT+1,say:SIZE+960+540,hang");
    fx.ppt.test_timeout = Some(Duration::from_millis(200));
    assert_eq!(fx.run().await.unwrap_err(), strings::PPT_TIMEOUT);
    let line = fx.cleanup_line().expect("cleanup ran");
    assert!(!line.contains("-StartedByUs"), "{line}");
}

#[tokio::test]
async fn timeout_applies_before_powerpoint_says_anything() {
    let mut fx = fixture("hang");
    fx.ppt.test_timeout = Some(Duration::from_millis(300));
    assert_eq!(fx.run().await.unwrap_err(), strings::PPT_TIMEOUT);
    assert!(fx.cleanup_line().is_some());
}

#[tokio::test]
async fn slide_count_extends_the_time_limit() {
    // 300 ms is not enough for this run; 300 ms for each of 9 slides, and once more, is.
    let mut fx = fixture(&format!("say:COUNT+9,sleep:800,say:SIZE+960+540,{}", (1..=9).map(|i| format!("png:{i}:1920:1080")).collect::<Vec<_>>().join(",")));
    fx.ppt.test_timeout = Some(Duration::from_millis(300));
    assert_eq!(fx.run().await.unwrap().len(), 9);
}

#[tokio::test]
async fn cleanup_that_fails_or_hangs_is_not_the_operators_problem() {
    let mut fx = fixture_with("hang", "fail:nope");
    fx.ppt.test_timeout = Some(Duration::from_millis(200));
    assert_eq!(fx.run().await.unwrap_err(), strings::PPT_TIMEOUT);
}

#[tokio::test]
async fn cancel_while_waiting_never_starts_powershell() {
    let fx = fixture(&good(1));
    let _held = fx.ppt.gate.acquire().await.unwrap();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let told = Mutex::new(Vec::new());
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        stop.cancel();
    });
    let err = fx.ppt.export(&fx.input, &fx.out, &|w| told.lock().unwrap().push(w), &no_progress, &cancel).await.unwrap_err();
    assert_eq!(err, CANCELLED);
    assert_eq!(*told.lock().unwrap(), [true]);
    assert!(fx.marks().is_empty());
}

#[tokio::test]
async fn already_cancelled_does_nothing() {
    let fx = fixture(&good(1));
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &cancel).await.unwrap_err(), CANCELLED);
    assert!(fx.marks().is_empty());
}

#[tokio::test]
async fn two_exports_run_one_at_a_time_and_cleanup_keeps_the_turn() {
    // The first is cancelled and its cleanup takes 500 ms; the second must not start until that is over.
    let fx = Arc::new(fixture_with(&format!("sleep:300,{}", good(1)), "sleep:500"));
    let second = fx.dir.join("slides2");
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let first = {
        let fx = fx.clone();
        tokio::spawn(async move { fx.ppt.export(&fx.input, &fx.out, &quiet, &no_progress, &cancel).await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    let told = Arc::new(Mutex::new(Vec::new()));
    let then = {
        let (fx, told) = (fx.clone(), told.clone());
        tokio::spawn(async move { fx.ppt.export(&fx.input, &second, &|w| told.lock().unwrap().push(w), &no_progress, &CancellationToken::new()).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    stop.cancel();
    assert_eq!(first.await.unwrap().unwrap_err(), CANCELLED);
    assert_eq!(then.await.unwrap().unwrap().len(), 1);
    assert_eq!(*told.lock().unwrap(), [true, false]);
    let times: Vec<(String, u128)> = fx.marks().iter().map(|l| {
        let mut w = l.split(' ');
        (w.next().unwrap().to_string(), w.next().unwrap().parse().unwrap())
    }).collect();
    let cleanup = times.iter().position(|t| t.0 == "cleanup").unwrap();
    let cleanup_end = times[cleanup + 1..].iter().find(|t| t.0 == "end").unwrap().1;
    let last_start = times.iter().rfind(|t| t.0 == "export").unwrap().1;
    assert!(last_start >= cleanup_end, "{times:?}");
}

#[test]
fn export_can_be_spawned() {
    fn sent<T: Send>(_: T) {}
    let ppt = PowerPoint::new();
    let cancel = CancellationToken::new();
    sent(ppt.export(Path::new("a"), Path::new("b"), &quiet, &no_progress, &cancel));
}

/// With `RP_TEST_PPT=1` on Windows with PowerPoint. PowerPoint makes the sample itself: a package good enough for
/// the real program is more than a test should build by hand.
fn real_powerpoint() -> Option<PowerPoint> {
    (std::env::var("RP_TEST_PPT").as_deref() == Ok("1")).then(PowerPoint::new)
}

fn sample(dir: &Path, name: &str) -> PathBuf {
    let file = dir.join(name);
    let script = dir.join("make-sample.ps1");
    std::fs::write(&script, r#"
param([string]$Path)
$app = New-Object -ComObject PowerPoint.Application
$pres = $app.Presentations.Add(0)
1..3 | ForEach-Object { [void]$pres.Slides.Add($_, 12) }
$pres.SaveAs($Path)
$pres.Close()
if ($app.Presentations.Count -eq 0) { $app.Quit() }
"#).unwrap();
    let run = std::process::Command::new(POWERSHELL).args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(&script).arg("-Path").arg(&file).status().unwrap();
    assert!(run.success());
    file
}

#[tokio::test]
async fn powerpoint_exports_sample_with_progress() {
    let Some(ppt) = real_powerpoint() else { return };
    let dir = crate::store::test_dir();
    let input = sample(&dir, "input.pptx");
    let seen = Mutex::new(Vec::new());
    let files = ppt.export(&input, &dir.join("out"), &quiet, &|i, n| seen.lock().unwrap().push((i, n)), &CancellationToken::new()).await.unwrap();
    assert_eq!(files.len(), 3);
    assert_eq!(*seen.lock().unwrap(), [(1, 3), (2, 3), (3, 3)]);
    for file in &files {
        assert_eq!(image::image_dimensions(file).unwrap(), (1920, 1080));
    }
}

#[tokio::test]
async fn two_exports_run_one_at_a_time() {
    let Some(ppt) = real_powerpoint() else { return };
    let dir = crate::store::test_dir();
    let (a, b) = (sample(&dir, "a.pptx"), sample(&dir, "b.pptx"));
    let told = Mutex::new(Vec::new());
    let cancel = CancellationToken::new();
    let (out_a, out_b) = (dir.join("a"), dir.join("b"));
    let second_waits = |w: bool| told.lock().unwrap().push(w);
    let (first, second) = tokio::join!(
        ppt.export(&a, &out_a, &quiet, &no_progress, &cancel),
        ppt.export(&b, &out_b, &second_waits, &no_progress, &cancel),
    );
    assert_eq!(first.unwrap().len(), 3);
    assert_eq!(second.unwrap().len(), 3);
    assert_eq!(*told.lock().unwrap(), [true, false]);
}
