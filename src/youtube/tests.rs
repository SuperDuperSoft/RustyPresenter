use super::*;
use crate::{app::test_app, model::MediaKind, youtube::testing::FakeReleases};
use std::io::Write;
use testing_fake::Fake;

/// Not a real test: yt-dlp's stand-in, this test binary run again (`Hub::run_in` under `cfg(test)`). Steps come
/// from `RP_FAKE_INFO` for a `-J` run and `RP_FAKE_GET` otherwise, comma separated: `out:<file>` (prints it),
/// `nth:<prefix>` (prints `<prefix><n>` on the run numbered n from 0),
/// `err:<file>` (prints it as error output), `exit:<code>`, `hang`, `child` (starts a grandchild that writes to
/// `<log>.beat` until killed), `make:<name>` (saves a file where `-o` says and reports it), and
/// `brokenunless:<text>` (fails as an out-of-date yt-dlp unless the program it stands for contains the text).
/// Each run adds `<program>\t<args joined by \x1f>` to `RP_FAKE_LOG`.
#[test]
fn fake_yt_dlp() {
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|a| a == "--") else { return };
    let Ok(log) = std::env::var("RP_FAKE_LOG") else { return };
    let ours = &args[at + 1..];
    let program = std::env::var("RP_FAKE_AS").unwrap_or_default();
    if let Ok(beat) = std::env::var("RP_FAKE_BEAT") {
        for _ in 0..1200 {
            let _ = std::fs::OpenOptions::new().create(true).append(true).open(&beat).and_then(|mut f| f.write_all(b"."));
            std::thread::sleep(Duration::from_millis(50));
        }
        std::process::exit(0);
    }
    let run = std::fs::read_to_string(&log).map_or(0, |t| t.lines().count());
    writeln!(std::fs::OpenOptions::new().create(true).append(true).open(&log).unwrap(), "{program}\t{}", ours.join("\x1f")).unwrap();
    let steps = std::env::var(if ours.iter().any(|a| a == "-J") { "RP_FAKE_INFO" } else { "RP_FAKE_GET" }).unwrap_or_default();
    // A fresh line after the test harness's own output.
    println!();
    for step in steps.split(',').filter(|s| !s.is_empty()) {
        let (kind, value) = step.split_once(':').unwrap_or((step, ""));
        match kind {
            "out" => {
                for l in std::fs::read_to_string(value).unwrap().lines() { println!("{l}") }
                std::io::stdout().flush().unwrap();
            }
            "nth" => println!("{}", std::fs::read_to_string(format!("{value}{run}")).unwrap().trim_end()),
            "err" => eprint!("{}", std::fs::read_to_string(value).unwrap()),
            "exit" => std::process::exit(value.parse().unwrap()),
            "hang" => std::thread::sleep(Duration::from_secs(60)),
            "child" => {
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(&args[1..at + 1]).env("RP_FAKE_BEAT", format!("{log}.beat"))
                    .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
            }
            "make" => {
                let o = &ours[ours.iter().position(|a| a == "-o").unwrap() + 1];
                let file = Path::new(o).parent().unwrap().join(value);
                std::fs::write(&file, crate::import::probe::FAKE_VIDEO).unwrap();
                println!("RPF {}", file.display());
            }
            "brokenunless" => if !program.contains(value) {
                eprintln!("ERROR: [youtube] aqz-KE-bpKQ: Unable to extract nsig function code; please report this issue on  https://github.com/yt-dlp/yt-dlp/issues");
                std::process::exit(1);
            },
            _ => panic!("unknown step {kind}"),
        }
    }
    std::process::exit(0);
}

/// A folder for one test's files.
fn temp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("rp-yt-{}", Uuid::new_v4().simple()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn file(dir: &Path, name: &str, text: &str) -> String {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p.display().to_string()
}

/// Each yt-dlp run: the program it stood for, and its arguments.
fn runs(log: &Path) -> Vec<(String, Vec<String>)> {
    std::fs::read_to_string(log).unwrap_or_default().lines().map(|l| {
        let (program, args) = l.split_once('\t').unwrap();
        (program.to_string(), args.split('\x1f').map(str::to_string).collect())
    }).collect()
}

const VIDEO_JSON: &str = r#"{"_type":"video","id":"aqz-KE-bpKQ","title":"Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film","channel":"Blender","duration":634,"thumbnail":null,"live_status":"not_live","availability":"public","webpage_url":"https://www.youtube.com/watch?v=aqz-KE-bpKQ","requested_formats":[{"format_id":"137","filesize":9000},{"format_id":"140","filesize":1000}]}"#;

fn quiet(_: Setup) {}

#[test]
fn links_keep_only_the_video() {
    let v = |id: &str| Some(Link { url: format!("https://www.youtube.com/watch?v={id}"), playlist: false });
    assert_eq!(link("https://www.youtube.com/watch?v=aqz-KE-bpKQ"), v("aqz-KE-bpKQ"));
    assert_eq!(link("  youtube.com/watch?v=aqz-KE-bpKQ&list=PL123&t=42s \n"), v("aqz-KE-bpKQ"), "this video only, no playlist");
    assert_eq!(link("https://youtu.be/aqz-KE-bpKQ?si=abc"), v("aqz-KE-bpKQ"));
    assert_eq!(link("https://m.youtube.com/watch?feature=share&v=aqz-KE-bpKQ"), v("aqz-KE-bpKQ"));
    assert_eq!(link("https://music.youtube.com/watch?v=aqz-KE-bpKQ"), v("aqz-KE-bpKQ"));
    assert_eq!(link("https://www.youtube.com/shorts/aqz-KE-bpKQ"), v("aqz-KE-bpKQ"));
    assert_eq!(link("https://www.youtube.com/live/aqz-KE-bpKQ?feature=share"), v("aqz-KE-bpKQ"));
    assert_eq!(link("https://www.youtube-nocookie.com/embed/aqz-KE-bpKQ"), v("aqz-KE-bpKQ"));
    assert_eq!(link("https://www.youtube.com/playlist?list=PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG"),
        Some(Link { url: "https://www.youtube.com/playlist?list=PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG".into(), playlist: true }));
    for no in ["", "hello", "https://vimeo.com/123", "https://www.youtube.com/", "https://www.youtube.com/watch?v=short", "https://www.youtube.com/@Blender",
        "ftp://youtube.com/watch?v=aqz-KE-bpKQ", "https://notyoutube.com/watch?v=aqz-KE-bpKQ", "https://www.youtube.com/playlist?list=a b"] {
        assert_eq!(link(no), None, "{no}");
    }
}

#[test]
fn quality_picks_h264_mp4_up_to_1080p() {
    assert_eq!(format_args(Quality::Best, true), [
        "-f", "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b", "-S", "vcodec:h264,res:1080,fps:30,acodec:aac",
        "--merge-output-format", "mp4", "--remux-video", "mp4",
    ]);
    assert_eq!(format_args(Quality::Smaller, true)[1], "bv*[height<=720]+ba/b[height<=720]/bv*+ba/b");
    assert_eq!(format_args(Quality::Smallest, false), ["-f", "b[ext=mp4][height<=480]/b[ext=mp4]/b", "-S", "vcodec:h264,res:480,acodec:aac"]);
    assert_eq!(format_args(Quality::Sound, true), ["-f", "ba[ext=m4a]/ba[acodec^=mp4a]/ba"]);
    assert_eq!(format_args(Quality::Best, false)[1], "b[ext=mp4][height<=1080]/b[ext=mp4]/b", "no joiner: one file with both");
    assert_eq!((Quality::from_index(0), Quality::from_index(3), Quality::from_index(9)), (Quality::Best, Quality::Sound, Quality::Best));
    assert_eq!(tr::qualities().len(), 4);
}

#[test]
fn errors_in_plain_words() {
    let cases = [
        ("ERROR: [youtube] abc: Private video. Sign in if you've been granted access to this video", YtError::Private),
        ("ERROR: [youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users.", YtError::SignIn),
        ("ERROR: [youtube] abc: Sign in to confirm you’re not a bot. Use --cookies-from-browser", YtError::BotCheck),
        ("ERROR: [youtube] abc: Join this channel to get access to members-only content like this video", YtError::Members),
        ("ERROR: [youtube] abc: This live event will begin in 3 hours.", YtError::Upcoming),
        ("ERROR: [youtube] abc: Video unavailable. This video has been removed by the uploader", YtError::Unavailable),
        ("ERROR: Unsupported URL: https://example.com/", YtError::NotAVideo),
        ("ERROR: [youtube] abc: Unable to download API page: <urlopen error [Errno 11001] getaddrinfo failed> (caused by TransportError)", YtError::Offline),
        ("ERROR: unable to write data: [Errno 28] No space left on device", YtError::DiskFull),
        ("ERROR: Unable to rename file: [WinError 112] There is not enough space on the disk", YtError::DiskFull),
        ("ERROR: [youtube] abc: Requested format is not available. Use --list-formats", YtError::Broken),
        ("WARNING: [youtube] nsig extraction failed: Some formats may be missing\nERROR: [youtube] abc: Unable to extract yt initial data", YtError::Broken),
        ("ERROR: [youtube] aqz-KE-bpKQ: Something odd happened", YtError::Other("Something odd happened".into())),
        ("", YtError::Other("yt-dlp stopped".into())),
    ];
    for (stderr, want) in cases { assert_eq!(classify(stderr), want, "{stderr}") }
    // Every error has its own words, never yt-dlp's jargon.
    assert_eq!(YtError::Offline.message(), "Can't reach YouTube. Check the internet connection and try again.");
    assert_eq!(YtError::Setup(FetchError::Checksum).message(), tr::setup_checksum());
    // Setup failures keep the plain first line and add what exactly failed under it.
    let http = YtError::Setup(FetchError::Http(404, "https://github.com/yt-dlp/yt-dlp/releases/download/x/yt-dlp_win.zip".into()));
    assert_eq!(http.message(), format!("{}\nHTTP 404 for https://github.com/yt-dlp/yt-dlp/releases/download/x/yt-dlp_win.zip", tr::setup_offline()));
    assert_eq!(YtError::Setup(FetchError::Offline("timed out".into())).message(), format!("{}\ntimed out", tr::setup_offline()));
    assert_eq!(YtError::Other("x".into()).message(), "The download didn't work: x");
}

#[test]
fn details_from_yt_dlp_json() {
    let Ok(Info::Video(v)) = parse_info(VIDEO_JSON) else { panic!() };
    assert_eq!((v.id.as_str(), v.channel.as_str(), v.duration), ("aqz-KE-bpKQ", "Blender", Some(634.0)));
    assert_eq!(v.parts, [("137".to_string(), 9000), ("140".to_string(), 1000)]);
    assert_eq!(v.url, "https://www.youtube.com/watch?v=aqz-KE-bpKQ");
    let single = r#"{"id":"x","title":"T","uploader":"U","format_id":"18","filesize_approx":5000.0}"#;
    let Ok(Info::Video(v)) = parse_info(single) else { panic!() };
    assert_eq!((v.channel.as_str(), v.parts.clone()), ("U", vec![("18".to_string(), 5000)]));
    let live = |status: &str| parse_info(&format!(r#"{{"id":"x","title":"T","live_status":"{status}"}}"#)).err();
    assert_eq!((live("is_live"), live("is_upcoming"), live("post_live"), live("was_live")), (Some(YtError::Live), Some(YtError::Upcoming), Some(YtError::Processing), None));
    let avail = |a: &str| parse_info(&format!(r#"{{"id":"x","title":"T","availability":"{a}"}}"#)).err();
    assert_eq!((avail("private"), avail("needs_auth"), avail("subscriber_only"), avail("unlisted")), (Some(YtError::Private), Some(YtError::SignIn), Some(YtError::Members), None));
    let list = r#"{"_type":"playlist","title":"Shorts","entries":[{"id":"aqz-KE-bpKQ","url":"https://www.youtube.com/watch?v=aqz-KE-bpKQ"},{"id":"bbbbbbbbbbb"}]}"#;
    assert_eq!(parse_info(list), Ok(Info::Playlist { title: "Shorts".into(), count: 2, first: Some("https://www.youtube.com/watch?v=aqz-KE-bpKQ".into()) }));
    assert_eq!(parse_info(r#"{"_type":"playlist","title":"E","entries":[]}"#), Ok(Info::Playlist { title: "E".into(), count: 0, first: None }));
    assert!(matches!(parse_info("not json"), Err(YtError::Other(_))));
}

#[test]
fn progress_is_one_smooth_bar() {
    let t = |f: &str, d: u64, total: u64| parse_tick(&format!(r#"RPJ {f} {{"status":"downloading","downloaded_bytes":{d},"total_bytes":{total},"eta":3,"speed":1.5}}"#)).unwrap();
    assert_eq!(t("137", 50, 100), Tick { format: "137".into(), downloaded: 50, total: Some(100), finished: false });
    let est = parse_tick(r#"RPJ 140 {"status":"downloading","downloaded_bytes":10,"total_bytes_estimate":40.5,"total_bytes":null}"#).unwrap();
    assert_eq!(est.total, Some(40));
    let done = parse_tick(r#"RPJ 140 {"status":"finished","total_bytes":40}"#).unwrap();
    assert!(done.finished && done.downloaded == 40);
    assert_eq!(parse_tick("[download]  50.0% of 10MiB"), None, "yt-dlp's own text is never parsed");
    assert_eq!(parse_tick("RPJ 137 {broken"), None);

    // Sizes known: the picture (9000 bytes) is 90% of the bar's download share.
    let parts = [("137".to_string(), 9000), ("140".to_string(), 1000)];
    let mut m = Meter::new(&parts);
    assert_eq!(m.tick(&t("137", 4500, 9000)), 43);
    assert_eq!(m.tick(&t("137", 9000, 9000)), 86);
    assert_eq!(m.tick(&t("140", 500, 1000)), 91);
    assert_eq!(m.tick(&t("137", 0, 9000)), 91, "never goes back");
    // Sizes unknown: each file counts the same.
    let mut m = Meter::new(&[("137".to_string(), 0), ("140".to_string(), 0)]);
    assert_eq!(m.tick(&t("137", 1, 2)), 24);
    // Formats yt-dlp picked differently from the lookup: counted as they come.
    let mut m = Meter::new(&parts);
    assert_eq!(m.tick(&t("18", 1, 2)), 24);
}

#[test]
fn titles_become_safe_library_names() {
    assert_eq!(library_name("Psalm 23 (feat. Choir) / Live"), "Psalm 23 (feat. Choir) - Live");
    assert_eq!(library_name("  \n "), "YouTube video");
    assert_eq!(library_name(&"x".repeat(400)).len(), 150);
}

/// An app whose yt-dlp is the fake, with these steps.
fn faked(info: &str, get: &str) -> (Arc<App>, PathBuf, PathBuf) {
    let app = test_app();
    let dir = temp();
    let log = dir.join("log");
    *app.youtube.fake.lock().unwrap() = Some(Fake::new(info, get, &log));
    (app, dir, log)
}

#[tokio::test]
async fn look_up_reads_details_with_the_download_s_format() {
    let dir = temp();
    let (app, _, log) = faked(&format!("out:{}", file(&dir, "info.json", VIDEO_JSON)), "");
    let found = app.youtube.look_up(&link("youtu.be/aqz-KE-bpKQ").unwrap(), Quality::Best, &quiet, &CancellationToken::new()).await.unwrap();
    assert_eq!(found.video.title, "Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film");
    assert_eq!(found.video.playlist, None);
    assert!(found.picture.is_none());
    let runs = runs(&log);
    assert_eq!(runs.len(), 1);
    let args = &runs[0].1;
    for want in ["--ignore-config", "--no-update", "--no-playlist", "-J", "--flat-playlist", "vcodec:h264,res:1080,fps:30,acodec:aac", "https://www.youtube.com/watch?v=aqz-KE-bpKQ"] {
        assert!(args.iter().any(|a| a == want), "{want} in {args:?}");
    }
    let at = args.iter().position(|a| a == "--ffmpeg-location").unwrap();
    assert_eq!(args[at + 1], "ffmpeg-dir");
}

#[tokio::test]
async fn a_playlist_link_looks_up_its_first_video_only() {
    let dir = temp();
    let list = r#"{"_type":"playlist","title":"Blender films","entries":[{"id":"aqz-KE-bpKQ","url":"https://www.youtube.com/watch?v=aqz-KE-bpKQ"},{"id":"bbbbbbbbbbb"},{"id":"ccccccccccc"}]}"#;
    // The first -J run gets the playlist, the second the video.
    file(&dir, "answer0", list);
    file(&dir, "answer1", VIDEO_JSON);
    let (app, _, log) = faked(&format!("nth:{}", dir.join("answer").display()), "");
    let found = app.youtube.look_up(&link("https://www.youtube.com/playlist?list=PLabc").unwrap(), Quality::Best, &quiet, &CancellationToken::new()).await.unwrap();
    assert_eq!(found.video.playlist, Some(("Blender films".to_string(), 3)));
    let runs = runs(&log);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].1.last().map(String::as_str), Some("https://www.youtube.com/playlist?list=PLabc"));
    assert_eq!(runs[1].1.last().map(String::as_str), Some("https://www.youtube.com/watch?v=aqz-KE-bpKQ"));
}

#[tokio::test]
async fn look_up_errors_are_plain() {
    let dir = temp();
    let err = file(&dir, "err.txt", "ERROR: [youtube] aqz-KE-bpKQ: Private video. Sign in if you've been granted access to this video\n");
    let (app, _, log) = faked(&format!("err:{err},exit:1"), "");
    let r = app.youtube.look_up(&link("youtu.be/aqz-KE-bpKQ").unwrap(), Quality::Best, &quiet, &CancellationToken::new()).await;
    assert_eq!(r.err(), Some(YtError::Private));
    assert_eq!(runs(&log).len(), 1, "not an out-of-date yt-dlp: no retry");
    let live = file(&dir, "live.json", r#"{"id":"aqz-KE-bpKQ","title":"Sunday","live_status":"is_live"}"#);
    *app.youtube.fake.lock().unwrap() = Some(Fake::new(&format!("out:{live}"), "", &log));
    let r = app.youtube.look_up(&link("youtu.be/aqz-KE-bpKQ").unwrap(), Quality::Best, &quiet, &CancellationToken::new()).await;
    assert_eq!(r.err().map(|e| e.message()), Some("This livestream hasn't finished yet. Try again after it ends.".into()));
}

/// Waits for the job to leave the list or finish; returns it as it last was (`None` once it is gone).
async fn job_end(app: &App, id: Uuid) -> Option<crate::app::Job> {
    for _ in 0..600 {
        let job = app.jobs.lock().unwrap().iter().find(|j| j.id == id).cloned();
        match job {
            None => return None,
            Some(j) if j.done || j.error.is_some() => return Some(j),
            _ => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
    panic!("the job never ended");
}

#[tokio::test]
async fn download_lands_in_the_library_and_the_service() {
    let dir = temp();
    let progress = file(&dir, "progress.txt", concat!(
        "RPJ 137 {\"status\":\"downloading\",\"downloaded_bytes\":4500,\"total_bytes\":9000}\n",
        "RPJ 137 {\"status\":\"finished\",\"downloaded_bytes\":9000,\"total_bytes\":9000}\n",
        "RPJ 140 {\"status\":\"finished\",\"downloaded_bytes\":1000,\"total_bytes\":1000}\n",
        "RPP Merger\n",
    ));
    let (app, _, log) = faked("", &format!("out:{progress},make:aqz-KE-bpKQ.mp4"));
    let Ok(Info::Video(video)) = parse_info(VIDEO_JSON) else { panic!() };
    let service = app.data.lock().unwrap().services[0].id;
    let mut updates = app.subscribe();
    let job = start_download(&app, video, Quality::Best, Some(service));
    let card = job_end(&app, job).await.expect("a finished download keeps its card for its note");
    assert_eq!(card.error, None);
    assert_eq!(card.label, "Downloaded “Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film”");
    assert_eq!(card.note.as_deref(), Some("It's in Video & media and in the service."));
    let (media, items) = { let d = app.data.lock().unwrap(); (d.media.clone(), d.services[0].items.clone()) };
    assert_eq!(media.len(), 1);
    assert_eq!((media[0].name.as_str(), media[0].kind), ("Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film", MediaKind::Video));
    assert!(app.store.asset_path(&media[0].asset).is_file());
    assert_eq!(card.result, Some(media[0].id));
    assert!(matches!(items[..], [crate::model::ServiceItem { kind: ItemKind::Media { media_id, autoplay: true, .. }, .. }] if media_id == media[0].id));
    // The bar moved smoothly through the download and on.
    let mut seen = Vec::new();
    while let Ok(u) = updates.try_recv() { if let crate::app::Update::Job { id, percent } = u && id == job { seen.push(percent) } }
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    assert!(seen.contains(&43) && seen.contains(&96) && seen.contains(&98), "{seen:?}");
    // The download ran with the progress and file report switches, into the data folder's tmp, which is now clear.
    let args = &runs(&log)[0].1;
    assert!(args.iter().any(|a| a == "download:RPJ %(info.format_id)s %(progress)j"));
    let out = &args[args.iter().position(|a| a == "-o").unwrap() + 1];
    assert!(out.starts_with(&app.store.tmp().display().to_string()) && out.ends_with("%(id)s.%(ext)s"), "{out}");
    let left: Vec<_> = std::fs::read_dir(app.store.tmp()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "{left:?}");
}

#[tokio::test]
async fn without_a_joiner_it_saves_360p_and_says_so() {
    let (app, _, log) = faked("", "make:x.mp4");
    if let Some(f) = app.youtube.fake.lock().unwrap().as_mut() { f.tools.as_mut().unwrap().ffmpeg = None }
    let Ok(Info::Video(video)) = parse_info(VIDEO_JSON) else { panic!() };
    let job = start_download(&app, video, Quality::Best, None);
    let card = job_end(&app, job).await.unwrap();
    assert_eq!(card.note.as_deref(), Some("It's in Video & media. Saved at lower quality (360p) because the video joiner couldn't be set up."));
    let args = &runs(&log)[0].1;
    assert!(args.iter().any(|a| a == "b[ext=mp4][height<=1080]/b[ext=mp4]/b"));
    assert!(!args.iter().any(|a| a == "--ffmpeg-location"));
    assert!(app.data.lock().unwrap().services[0].items.is_empty(), "not asked to add it");
}

#[tokio::test]
async fn a_failed_download_shows_its_error_on_the_card() {
    let dir = temp();
    let err = file(&dir, "err.txt", "ERROR: unable to write data: [Errno 28] No space left on device\n");
    let (app, _, _) = faked("", &format!("err:{err},exit:1"));
    let Ok(Info::Video(video)) = parse_info(VIDEO_JSON) else { panic!() };
    let job = start_download(&app, video, Quality::Best, None);
    let card = job_end(&app, job).await.unwrap();
    assert_eq!(card.error.as_deref(), Some("The computer's disk is full. Free up some space and try again."));
    assert!(app.data.lock().unwrap().media.is_empty());
}

#[tokio::test]
async fn cancel_kills_yt_dlp_and_what_it_started() {
    let (app, _, log) = faked("", "child,hang");
    let Ok(Info::Video(video)) = parse_info(VIDEO_JSON) else { panic!() };
    let job = start_download(&app, video, Quality::Best, None);
    let beat = PathBuf::from(format!("{}.beat", log.display()));
    let size = || std::fs::metadata(&beat).map(|m| m.len()).unwrap_or(0);
    for _ in 0..300 { if size() > 2 { break } tokio::time::sleep(Duration::from_millis(20)).await }
    assert!(size() > 2, "the grandchild runs");
    app.job_cancel(job);
    assert_eq!(job_end(&app, job).await.map(|j| j.label), None, "a cancelled job leaves no card");
    tokio::time::sleep(Duration::from_millis(400)).await;
    let after = size();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(size(), after, "the grandchild is gone too");
    assert!(app.data.lock().unwrap().media.is_empty());
    let left: Vec<_> = std::fs::read_dir(app.store.tmp()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "{left:?}");
}

/// An app whose yt-dlp comes from a local release page (installed first), run as the fake.
async fn released(info: &str, server: &FakeReleases) -> (Arc<App>, PathBuf) {
    let app = test_app();
    let log = temp().join("log");
    app.youtube.fetch.set_sources(server.sources());
    app.youtube.fetch.set_builds(fetch::Builds { yt_dlp: Some(fetch::YtBuild { asset: "yt-dlp_win.zip", exe: "yt-dlp.exe", zip: true }), deno: None, ffmpeg: None });
    let mut fake = Fake::new(info, "", &log);
    fake.tools = None;
    *app.youtube.fake.lock().unwrap() = Some(fake);
    app.youtube.fetch.ensure(Tool::YtDlp, false, &|_| {}, &CancellationToken::new()).await.unwrap();
    (app, log)
}

#[tokio::test]
async fn an_out_of_date_yt_dlp_is_updated_and_tried_once_more() {
    let dir = temp();
    let server = FakeReleases::start().await;
    server.yt_dlp_release("2026.10.26", b"old");
    let (app, log) = released(&format!("brokenunless:2026.11.01,out:{}", file(&dir, "i.json", VIDEO_JSON)), &server).await;
    server.yt_dlp_release("2026.11.01", b"new");
    let found = app.youtube.look_up(&link("youtu.be/aqz-KE-bpKQ").unwrap(), Quality::Best, &quiet, &CancellationToken::new()).await.unwrap();
    assert_eq!(found.video.id, "aqz-KE-bpKQ");
    let runs = runs(&log);
    assert_eq!(runs.len(), 2, "failed, updated, worked");
    assert!(runs[0].0.contains("2026.10.26") && runs[1].0.contains("2026.11.01"), "{runs:?}");
    assert_eq!(app.youtube.fetch.installed(Tool::YtDlp).unwrap().state.version, "2026.11.01");
}

#[tokio::test]
async fn it_is_tried_once_more_only() {
    let dir = temp();
    let server = FakeReleases::start().await;
    server.yt_dlp_release("2026.10.26", b"old");
    let (app, log) = released(&format!("brokenunless:never,out:{}", file(&dir, "i.json", VIDEO_JSON)), &server).await;
    // Nothing newer anywhere (nightly is the same build): no second run.
    server.yt_dlp_nightly("2026.10.26", b"old");
    let r = app.youtube.look_up(&link("youtu.be/aqz-KE-bpKQ").unwrap(), Quality::Best, &quiet, &CancellationToken::new()).await;
    assert_eq!(r.err(), Some(YtError::Broken));
    assert_eq!(runs(&log).len(), 1);
    // A newer one that still cannot read it: exactly two runs, then the plain message.
    server.yt_dlp_release("2026.11.01", b"newer");
    let before = runs(&log).len();
    let r = app.youtube.look_up(&link("youtu.be/aqz-KE-bpKQ").unwrap(), Quality::Best, &quiet, &CancellationToken::new()).await;
    assert_eq!(r.err().map(|e| e.message()), Some(tr::broken()));
    assert_eq!(runs(&log).len() - before, 2);
}

#[tokio::test]
async fn first_use_fetches_the_helpers_and_reports_it() {
    let server = FakeReleases::start().await;
    server.yt_dlp_release("2026.10.26", b"exe");
    server.deno_release("v2.9.8", "deno-x86_64-pc-windows-msvc.zip", &crate::youtube::testing::zip_of(&[(fetch_exe("deno").as_str(), b"deno")]), true);
    let app = test_app();
    app.youtube.fetch.set_sources(server.sources());
    app.youtube.fetch.set_builds(fetch::Builds {
        yt_dlp: Some(fetch::YtBuild { asset: "yt-dlp_win.zip", exe: "yt-dlp.exe", zip: true }),
        deno: Some("deno-x86_64-pc-windows-msvc.zip"), ffmpeg: Some(fetch::FfBuild::Builds("missing.zip")),
    });
    let seen = std::sync::Mutex::new(Vec::new());
    let tools = app.youtube.tools(true, &|s| seen.lock().unwrap().push(s), &CancellationToken::new()).await.unwrap();
    assert!(tools.yt_dlp.ends_with("yt-dlp.exe") && tools.deno.as_ref().is_some_and(|d| d.ends_with(fetch_exe("deno"))));
    assert_eq!(tools.ffmpeg, None, "no joiner to be had: downloads fall back, they do not fail");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.first().map(|s| (s.tool, s.first)), Some((Tool::YtDlp, true)));
    assert!(seen.iter().any(|s| s.tool == Tool::Deno && s.percent == 100));
    assert_eq!(seen[0].text(), "Getting YouTube downloads ready — first time only (0%)…");
    // The download's arguments name Deno and our cache.
    let args: Vec<String> = app.youtube.common_args(&tools).iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let at = args.iter().position(|a| a == "--js-runtimes").unwrap();
    assert!(args[at + 1].starts_with("deno:") && args[at + 1].ends_with(&fetch_exe("deno")), "{args:?}");
    assert!(args.iter().any(|a| a.ends_with("cache")));
}

fn fetch_exe(base: &str) -> String { if cfg!(windows) { format!("{base}.exe") } else { base.into() } }

/// `RP_NET_TESTS=1`: fetches the real yt-dlp, Deno and FFmpeg into a temporary data folder, downloads a short
/// Creative Commons video (the Blender Foundation's 2½-minute "Caminandes 2: Gran Dillama", CC BY) and probes it.
/// `RP_NET_VIDEO` picks another.
#[tokio::test]
async fn net_downloads_a_real_video() {
    if std::env::var("RP_NET_TESTS").as_deref() != Ok("1") { return }
    let app = test_app();
    app.youtube.fetch.set_sources(fetch::Sources::default());
    let url = std::env::var("RP_NET_VIDEO").unwrap_or_else(|_| "https://www.youtube.com/watch?v=Z4C82eyhwgU".into());
    let began = Instant::now();
    let found = app.youtube.look_up(&link(&url).unwrap(), Quality::Best, &|s| eprintln!("{}", s.text()), &CancellationToken::new()).await.unwrap();
    eprintln!("looked up “{}” ({:?} s, {} by {}) in {:?}; picture: {}", found.video.title, found.video.duration, found.video.parts.len(), found.video.channel, began.elapsed(), found.picture.is_some());
    for t in [Tool::YtDlp, Tool::Deno, Tool::Ffmpeg] { eprintln!("{t:?}: {:?}", app.youtube.fetch.installed(t).map(|i| i.state.version)) }
    let job = start_download(&app, found.video, Quality::Best, None);
    let card = loop {
        let j = app.jobs.lock().unwrap().iter().find(|j| j.id == job).cloned();
        match j { Some(j) if j.done || j.error.is_some() => break j, Some(_) => tokio::time::sleep(Duration::from_millis(200)).await, None => panic!("cancelled?") }
    };
    assert_eq!(card.error, None, "{}", card.label);
    let m = app.data.lock().unwrap().media[0].clone();
    let path = app.store.asset_path(&m.asset);
    eprintln!("downloaded “{}” ({} bytes, {:?}) in {:?}", m.name, std::fs::metadata(&path).unwrap().len(), m.status, began.elapsed());
    assert_eq!(m.kind, MediaKind::Video);
    assert_eq!(m.status, crate::model::MediaStatus::Ready, "H.264 + AAC in MP4 plays as it is");
    // The app's own probe (ffprobe from the fetched FFmpeg) sees picture and sound.
    let ff = app.youtube.fetch.installed(Tool::Ffmpeg).map(|i| i.exe.with_file_name(fetch_exe("ffprobe")));
    if let Some(ffprobe) = ff.filter(|p| p.is_file()) {
        let out = std::process::Command::new(ffprobe).args(["-v", "error", "-show_entries", "stream=codec_name,height", "-of", "csv=p=0"]).arg(&path).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        eprintln!("streams: {text}");
        assert!(text.contains("h264") && text.contains("aac"), "{text}");
    }
    let _ = std::fs::remove_dir_all(&app.store.root);
}
