use super::*;
use crate::{
    app::{ImportPurpose, Imported, Update, test_app},
    model::{Data, MediaKind},
    video::ffi,
};
use std::time::{Duration, Instant};

fn entry(kind: MediaKind, asset: &str, status: MediaStatus) -> MediaFile {
    MediaFile { id: Uuid::new_v4(), name: "x".into(), kind, asset: asset.into(), status, original_asset: None, duration: None, thumb: None, size: None }
}

fn temp(name: &str) -> PathBuf {
    let dir = crate::store::test_dir();
    dir.join(name)
}

fn png(name: &str, w: u32, h: u32) -> PathBuf {
    let p = temp(name);
    image::RgbImage::new(w, h).save_with_format(&p, image::ImageFormat::Png).unwrap();
    p
}

/// A file in the store's tmp, as `import_path` leaves one.
fn staged(app: &Arc<App>, bytes: &[u8]) -> PathBuf {
    let p = app.store.tmp().join(format!("{}.part", Uuid::new_v4()));
    std::fs::write(&p, bytes).unwrap();
    p
}

fn files_in(dir: PathBuf) -> usize {
    std::fs::read_dir(dir).map_or(0, |d| d.count())
}

fn video_probe() -> Probe {
    Probe { kind: MediaKind::Video, duration: Some(12.5), size: Some((640, 360)), thumb: Some(image::RgbImage::new(64, 36)), tracks: ffi::Node::None }
}

fn job_error(app: &App) -> Option<String> {
    app.jobs.lock().unwrap().first().and_then(|j| j.error.clone())
}

#[test]
fn old_statuses_become_ready_and_are_listed_for_reprobe() {
    let mut data = Data::default();
    let converting = entry(MediaKind::Video, "a.avi", MediaStatus::Converting { percent: 30 });
    let unconverted = entry(MediaKind::Audio, "b.wma", MediaStatus::Unconverted { reason: "no FFmpeg".into() });
    let plain = entry(MediaKind::Video, "c.mp4", MediaStatus::Ready);
    let failed = entry(MediaKind::Video, "d.mp4", MediaStatus::Failed { reason: "old".into() });
    let ids: Vec<Uuid> = [&converting, &unconverted, &plain, &failed].iter().map(|m| m.id).collect();
    data.media = vec![converting, unconverted, plain, failed];
    assert_eq!(upgrade_media(&mut data), ids);
    assert_eq!(data.media[0].status, MediaStatus::Ready);
    assert_eq!(data.media[1].status, MediaStatus::Ready);
    assert_eq!(data.media[3].status, MediaStatus::Failed { reason: "old".into() });
}

#[test]
fn finished_entries_are_left_alone() {
    let mut data = Data::default();
    let mut video = entry(MediaKind::Video, "a.mp4", MediaStatus::Ready);
    video.thumb = Some("t.jpg".into());
    let mut audio = entry(MediaKind::Audio, "b.mp3", MediaStatus::Ready);
    audio.duration = Some(10.0);
    data.media = vec![video, audio];
    assert!(upgrade_media(&mut data).is_empty(), "audio never gets a thumbnail, so it must not be probed at every start");
}

#[tokio::test]
async fn the_upgrade_is_saved_at_start() {
    let app = test_app();
    app.data.lock().unwrap().media.push(entry(MediaKind::Video, "gone.avi", MediaStatus::Converting { percent: 1 }));
    at_start(&app, None, &tokio::runtime::Handle::current());
    let text = std::fs::read_to_string(app.store.data_path()).unwrap();
    assert!(!text.contains("converting") && !text.contains("unconverted"), "{text}");
    assert_eq!(app.store.load().unwrap().media[0].status, MediaStatus::Ready);
}

#[test]
fn old_data_json_without_new_fields_loads() {
    let old = r#"{"id":"00000000-0000-0000-0000-000000000000","name":"Old","kind":"video","asset":"a.avi","status":{"state":"unconverted","reason":"r"}}"#;
    let m: MediaFile = serde_json::from_str(old).unwrap();
    assert_eq!((m.duration, m.thumb, m.size), (None, None, None));
}

#[test]
fn thumb_is_a_referenced_asset_and_survives_backup() {
    let app = test_app();
    let thumb = app.store.import_file(&png("t.jpg", 2, 2), "jpg").unwrap();
    let mut m = entry(MediaKind::Video, "a.mp4", MediaStatus::Ready);
    std::fs::write(app.store.asset_path("a.mp4"), b"v").unwrap();
    m.thumb = Some(thumb.clone());
    let mut data = Data::default();
    data.media.push(m);
    assert!(data.referenced_assets().contains(&thumb));
    let name = app.store.backup(&data).unwrap();
    std::fs::remove_file(app.store.asset_path(&thumb)).unwrap();
    app.store.restore(&name).unwrap();
    assert!(app.store.asset_path(&thumb).exists());
}

#[tokio::test]
async fn unplayable_file_adds_nothing() {
    let app = test_app();
    let tmp = staged(&app, b"not a video");
    let e = import_media(&app, &tmp, "abc", "mp4", "Clip".into()).await.unwrap_err();
    assert_eq!(e, ffi::MISSING);
    assert!(app.data.lock().unwrap().media.is_empty());
    assert_eq!(job_error(&app).as_deref(), Some(ffi::MISSING), "the card shows it too");
    assert_eq!(files_in(app.store.assets()), 0);
    assert_eq!(files_in(app.store.tmp()), 0, "the copy in tmp goes too");
}

#[tokio::test]
async fn image_import_sets_size_and_thumb() {
    let app = test_app();
    let Imported::Media(m) = app.import_path(&png("Sunrise.png", 640, 320), ImportPurpose::Media, |_| {}).await.unwrap() else { panic!() };
    assert_eq!((m.kind, m.status.clone(), m.size, m.duration, m.name.as_str()), (MediaKind::Image, MediaStatus::Ready, Some((640, 320)), None, "Sunrise"));
    let thumb = m.thumb.unwrap();
    assert!(thumb.ends_with(".jpg"), "{thumb}");
    let small = image::open(app.store.asset_path(&thumb)).unwrap();
    assert_eq!((small.width(), small.height()), (320, 160));
    assert!(app.jobs.lock().unwrap().is_empty(), "the job ended");
    assert_eq!(files_in(app.store.tmp()), 0);
}

#[tokio::test]
async fn a_probed_video_is_published_with_what_the_probe_found() {
    let app = test_app();
    let file = staged(&app, b"video");
    let m = add(&app, &file, "abc", "mkv", "Clip".into(), Ok(video_probe()), &CancellationToken::new()).await.unwrap();
    assert_eq!(m.asset, "abc.mkv");
    assert_eq!(std::fs::read(app.store.asset_path(&m.asset)).unwrap(), b"video");
    assert_eq!((m.kind, m.duration, m.size), (MediaKind::Video, Some(12.5), Some((640, 360))));
    assert!(app.store.asset_path(m.thumb.as_ref().unwrap()).exists());
    assert_eq!(files_in(app.store.tmp()), 0);
    assert_eq!(app.store.load().unwrap().media[0].id, m.id, "saved");
}

#[tokio::test]
async fn the_kind_comes_from_the_probe_not_the_extension() {
    let app = test_app();
    let file = staged(&app, b"sound");
    let probe = Probe { kind: MediaKind::Audio, duration: Some(3.0), size: None, thumb: None, tracks: ffi::Node::None };
    let m = add(&app, &file, "abc", "webm", "Choir".into(), Ok(probe), &CancellationToken::new()).await.unwrap();
    assert_eq!((m.kind, m.thumb), (MediaKind::Audio, None));
}

#[tokio::test]
async fn a_cancel_after_the_probe_adds_nothing() {
    let app = test_app();
    let file = staged(&app, b"video");
    let cancel = CancellationToken::new();
    cancel.cancel();
    let e = add(&app, &file, "abc", "mp4", "Clip".into(), Ok(video_probe()), &cancel).await.unwrap_err();
    assert_eq!(e, crate::import::CANCELLED);
    assert!(app.data.lock().unwrap().media.is_empty());
    assert_eq!(files_in(app.store.assets()), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn media_import_is_a_cancellable_job() {
    let app = test_app();
    // Big enough that the decode outlasts the look at the job list.
    let file = png("Big.png", 6000, 6000);
    let (a, f) = (app.clone(), file.clone());
    let import = tokio::spawn(async move { a.import_path(&f, ImportPurpose::Media, |_| {}).await });
    let until = Instant::now() + Duration::from_secs(20);
    let job = loop {
        let found = app.jobs.lock().unwrap().iter().find(|j| j.label == "Adding “Big”").map(|j| (j.id, j.cancellable));
        if let Some(found) = found { break found }
        assert!(Instant::now() < until, "no job");
        tokio::time::sleep(Duration::from_millis(1)).await;
    };
    assert!(job.1, "it has a Cancel button");
    let body = serde_json::to_vec(&serde_json::json!({ "id": job.0 })).unwrap();
    app.do_action("job/cancel", &body).await.unwrap();
    let e = import.await.unwrap().unwrap_err();
    assert_eq!(e.message, crate::import::CANCELLED);
    assert!(app.data.lock().unwrap().media.is_empty());
    assert!(app.jobs.lock().unwrap().is_empty(), "a cancelled job leaves no card");
    assert_eq!(files_in(app.store.tmp()), 0);
}

#[tokio::test]
async fn asset_import_refuses_undecodable_picture() {
    let app = test_app();
    let good = png("whole.png", 40, 40);
    let bytes = std::fs::read(good).unwrap();
    let p = temp("logo.png");
    std::fs::write(&p, &bytes[..bytes.len() / 2]).unwrap();
    let e = app.import_path(&p, ImportPurpose::Asset, |_| {}).await.unwrap_err();
    assert_eq!(e.message, BAD_PICTURE);
    assert_eq!(files_in(app.store.assets()), 0);
    assert_eq!(files_in(app.store.tmp()), 0);
}

#[tokio::test]
async fn a_lyrics_file_on_the_media_tab_is_a_song() {
    let app = test_app();
    let p = temp("song.txt");
    std::fs::write(&p, "Amazing Grace\n\nVerse 1\nAmazing grace how sweet the sound\n\nJohn Newton\nCCLI Song # 22025\n").unwrap();
    let got = app.import_path(&p, ImportPurpose::Media, |_| {}).await.unwrap();
    assert!(matches!(got, Imported::Song { id: Some(_), .. }), "{got:?}");
}

#[test]
fn image_filter_has_no_avif() {
    assert!(!IMAGE.contains(&"avif"));
    assert!(crate::ui::operator::file_input::Filter::Pictures.extensions().iter().all(|e| *e != "avif"));
}

/// An entry for `bytes`, in the store.
fn stored(app: &Arc<App>, kind: MediaKind, name: &str, bytes: &[u8], status: MediaStatus) -> Uuid {
    std::fs::write(app.store.asset_path(name), bytes).unwrap();
    let m = entry(kind, name, status);
    let id = m.id;
    app.data.lock().unwrap().media.push(m);
    id
}

fn media(app: &App, id: Uuid) -> MediaFile {
    app.data.lock().unwrap().media.iter().find(|m| m.id == id).unwrap().clone()
}

#[tokio::test]
async fn reprobe_fills_in_a_picture() {
    let app = test_app();
    let bytes = std::fs::read(png("p.png", 100, 50)).unwrap();
    let id = stored(&app, MediaKind::Image, "p.png", &bytes, MediaStatus::Ready);
    reprobe_all(app.clone(), vec![id]).await;
    let m = media(&app, id);
    assert_eq!((m.size, m.status), (Some((100, 50)), MediaStatus::Ready));
    assert!(app.store.asset_path(&m.thumb.unwrap()).exists());
}

#[tokio::test]
async fn reprobe_failure_is_recorded_and_try_again_clears_it() {
    let app = test_app();
    let id = stored(&app, MediaKind::Image, "p.png", b"not a picture", MediaStatus::Ready);
    reprobe_all(app.clone(), vec![id]).await;
    assert_eq!(media(&app, id).status, MediaStatus::Failed { reason: CANT_PLAY.into() });
    let good = std::fs::read(png("q.png", 4, 4)).unwrap();
    std::fs::write(app.store.asset_path("p.png"), good).unwrap();
    app.do_action("media/reprobe", serde_json::to_vec(&serde_json::json!({ "id": id })).unwrap().as_slice()).await.unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while media(&app, id).status != MediaStatus::Ready {
        assert!(Instant::now() < until, "still {:?}", media(&app, id).status);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn reprobe_without_a_player_leaves_videos_as_they_are() {
    let app = test_app();
    let id = stored(&app, MediaKind::Video, "v.mp4", b"video", MediaStatus::Ready);
    reprobe_all(app.clone(), vec![id]).await;
    assert_eq!(media(&app, id).status, MediaStatus::Ready, "a missing player says nothing about the file");
}

#[tokio::test]
async fn reprobe_ends_with_one_edit_and_skips_what_was_deleted() {
    let app = test_app();
    let good = std::fs::read(png("p.png", 4, 4)).unwrap();
    let a = stored(&app, MediaKind::Image, "a.png", &good, MediaStatus::Ready);
    let b = stored(&app, MediaKind::Image, "b.png", b"bad", MediaStatus::Ready);
    let mut updates = app.tx.subscribe();
    reprobe_all(app.clone(), vec![a, Uuid::new_v4(), b]).await;
    let mut edits = 0;
    while let Ok(u) = updates.try_recv() {
        if matches!(u, Update::Data) { edits += 1 }
    }
    assert_eq!(edits, 1);
    assert!(media(&app, a).thumb.is_some());
}

#[tokio::test]
async fn reprobe_keeps_a_thumbnail_it_could_not_replace() {
    let app = test_app();
    let id = stored(&app, MediaKind::Audio, "s.png", b"", MediaStatus::Ready);
    app.data.lock().unwrap().media[0].thumb = Some("kept.jpg".into());
    let good = std::fs::read(png("p.png", 4, 4)).unwrap();
    std::fs::write(app.store.asset_path("s.png"), good).unwrap();
    // The folder for the thumbnail's tmp file is gone, so it can't be written.
    std::fs::remove_dir_all(app.store.tmp()).unwrap();
    reprobe_all(app.clone(), vec![id]).await;
    assert_eq!(media(&app, id).thumb.as_deref(), Some("kept.jpg"));
}

/// `RP_VIDEO_TESTS=1`, a real libmpv, and `RP_TEST_VIDEO` naming a short video file.
#[tokio::test]
async fn video_import_sets_duration_size_thumb_ready() {
    let Some(sample) = std::env::var_os("RP_TEST_VIDEO").filter(|_| std::env::var_os("RP_VIDEO_TESTS").is_some()) else { return };
    let app = test_app();
    app.set_probe_mpv(ffi::Mpv::load().ok());
    let Imported::Media(m) = app.import_path(Path::new(&sample), ImportPurpose::Media, |_| {}).await.unwrap() else { panic!() };
    assert_eq!((m.kind, m.status), (MediaKind::Video, MediaStatus::Ready));
    assert!(m.duration.unwrap() > 0.0 && m.size.is_some() && m.thumb.is_some());
}
