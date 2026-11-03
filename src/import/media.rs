//! Adding a video, song or picture to the library. No conversion: what libmpv can open is ready to play, so the
//! file goes in as it came, with what the probe found out about it. Nothing is published until the probe has
//! said yes, so a file that can't be played leaves nothing behind.
use super::{CANCELLED, probe::{self, BAD_PICTURE, CANT_PLAY, Probe}, strings};
use crate::{
    app::App,
    model::{AssetId, Data, MediaFile, MediaKind, MediaStatus},
    video::ffi::Mpv,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The only lists of extensions: file dialogs, drops and the operator's routing all read these.
pub const VIDEO: &[&str] = &["mp4", "m4v", "webm", "mov", "mkv", "avi", "wmv", "mpg", "mpeg", "m2v", "vob", "flv", "3gp", "3g2", "mts", "m2ts", "ts", "mxf", "ogv", "dv", "asf", "f4v", "divx", "qt"];
pub const AUDIO: &[&str] = &["mp3", "m4a", "aac", "wav", "ogg", "oga", "opus", "flac", "wma", "aiff", "aif", "alac", "ac3", "amr", "mka"];
/// No `avif`: the `image` crate here has no AVIF decoder, and one would need a system dav1d.
pub const IMAGE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

const DISK_FULL: &str = "Could not save the file (disk full?).";

/// Add `tmp`, a file in the store's tmp that this import owns, as a job with a Cancel button. `Err` is a message
/// for the operator, or [`CANCELLED`]. The job shows a failure too.
pub async fn import_media(app: &Arc<App>, tmp: &Path, sha: &str, ext: &str, display: String) -> Result<MediaFile, String> {
    let label = strings::adding(&display);
    let (job, token) = app.job_start_cancellable(&label);
    // The probe tells a picture by its extension, and `tmp` is a `.part`.
    let file = tmp.with_extension(ext);
    let probed = match std::fs::rename(tmp, &file) {
        Ok(()) => {
            let (shown, watcher) = (label, app.clone());
            let waiting = move |waiting: bool| watcher.job_label(job, if waiting { strings::WAITING_FOR_FILE } else { &shown });
            probe::probe_waiting(app.probe_mpv(), file.clone(), token.clone(), waiting).await
        }
        Err(_) => Err(DISK_FULL.into()),
    };
    let result = add(app, &file, sha, ext, display, probed, &token).await;
    match &result {
        Ok(_) => app.job_end(job, None),
        Err(e) => {
            // Whichever of the two names it still has.
            let _ = std::fs::remove_file(tmp);
            let _ = std::fs::remove_file(&file);
            if e == CANCELLED { app.job_end_cancelled(job) } else { app.job_end(job, Some(e.clone())) }
        }
    }
    result
}

/// The part after the probe: everything goes into the store, then one `App::edit` adds the entry.
async fn add(app: &Arc<App>, file: &Path, sha: &str, ext: &str, display: String, probed: Result<Probe, String>, cancel: &CancellationToken) -> Result<MediaFile, String> {
    let Probe { kind, duration, size, thumb, .. } = probed?;
    // A picture is not stopped by its token, and a cancel can come after the last look a probe has at it.
    if cancel.is_cancelled() { return Err(CANCELLED.into()) }
    let thumb = publish_thumb(app, thumb).await?;
    if cancel.is_cancelled() { return Err(CANCELLED.into()) }
    let asset = app.store.publish(file, sha, ext).map_err(|_| DISK_FULL.to_string())?;
    let media = MediaFile { id: Uuid::new_v4(), name: display, kind, asset, status: MediaStatus::Ready, original_asset: None, duration, thumb, size };
    app.edit(|d| { d.media.push(media.clone()); Ok(()) })?;
    Ok(media)
}

/// Scale and encode off the async threads, and store the thumbnail as a `.jpg` asset.
async fn publish_thumb(app: &Arc<App>, thumb: Option<image::RgbImage>) -> Result<Option<AssetId>, String> {
    let Some(img) = thumb else { return Ok(None) };
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let tmp = app.store.tmp().join(format!("{}.thumb", Uuid::new_v4()));
        let saved = std::fs::write(&tmp, probe::thumb_jpeg(&img)).and_then(|()| app.store.import_file(&tmp, "jpg"));
        let _ = std::fs::remove_file(&tmp);
        saved.map(Some).map_err(|_| DISK_FULL.to_string())
    }).await.map_err(|e| e.to_string())?
}

/// `ImportPurpose::Asset`: a picture is checked before it is published, so nothing that can't be drawn gets in.
pub async fn check_picture(file: &Path) -> Result<(), String> {
    let file = file.to_path_buf();
    tokio::task::spawn_blocking(move || probe::probe_image(&file).map(|_| ())).await.map_err(|_| BAD_PICTURE.to_string())?
}

/// Library entries from before this version. Maps `Converting`/`Unconverted` to `Ready`, as conversion is gone and
/// the player opens the file as it is, and returns the ids that [`reprobe`] should look at. An audio file has no
/// thumbnail to wait for, so it is finished once it has a duration.
pub fn upgrade_media(data: &mut Data) -> Vec<Uuid> {
    let mut ids = vec![];
    for m in &mut data.media {
        let legacy = matches!(m.status, MediaStatus::Converting { .. } | MediaStatus::Unconverted { .. });
        if legacy { m.status = MediaStatus::Ready }
        let unfinished = if m.kind == MediaKind::Audio { m.duration.is_none() } else { m.thumb.is_none() };
        if legacy || unfinished { ids.push(m.id) }
    }
    ids
}

/// What [`reprobe`] found out about one file.
enum Found {
    Fine { duration: Option<f64>, size: Option<(u32, u32)>, thumb: Option<AssetId> },
    CantPlay,
}

/// Look at these files again in the background. Action `media/reprobe` does it for one, the **Try again** button.
pub fn reprobe(app: Arc<App>, ids: Vec<Uuid>) {
    tokio::spawn(reprobe_all(app, ids));
}

/// One at a time, to stay out of the live room's way, and one `App::edit` at the end.
pub(crate) async fn reprobe_all(app: Arc<App>, ids: Vec<Uuid>) {
    let mpv = app.probe_mpv();
    let mut found = vec![];
    for id in ids {
        let asset = app.data.lock().unwrap().media.iter().find(|m| m.id == id).map(|m| m.asset.clone());
        let Some(path) = asset.map(|a| app.store.asset_path(&a)) else { continue };
        // Without a player nothing can be learned about the file, which is not the file's fault.
        if mpv.is_none() && !probe::is_picture(&path) { continue }
        found.push((id, look(&app, mpv.clone(), path).await));
    }
    if found.is_empty() { return }
    let saved = app.edit(|d| {
        for (id, found) in found {
            let Some(m) = d.media.iter_mut().find(|m| m.id == id) else { continue };
            match found {
                Found::Fine { duration, size, thumb } => {
                    (m.duration, m.size, m.status) = (duration, size, MediaStatus::Ready);
                    m.thumb = thumb.or(m.thumb.take());
                }
                Found::CantPlay => m.status = MediaStatus::Failed { reason: CANT_PLAY.into() },
            }
        }
        Ok(())
    });
    if let Err(e) = saved { eprintln!("Could not save what was found about the media files: {e}") }
}

async fn look(app: &Arc<App>, mpv: Option<Arc<Mpv>>, path: PathBuf) -> Found {
    match probe::probe(mpv, path, CancellationToken::new()).await {
        // A thumbnail that could not be saved leaves the next start to try again.
        Ok(p) => Found::Fine { duration: p.duration, size: p.size, thumb: publish_thumb(app, p.thumb).await.ok().flatten() },
        Err(_) => Found::CantPlay,
    }
}

/// At start-up of the native binary: the probe's player, then the upgrade of old entries, saved in one `App::edit`
/// before anything else can read them. The look at those files runs in the background.
pub fn at_start(app: &Arc<App>, mpv: Option<Arc<Mpv>>, rt: &tokio::runtime::Handle) {
    app.set_probe_mpv(mpv);
    match app.edit(|d| Ok(upgrade_media(d))) {
        Ok(ids) if !ids.is_empty() => { rt.spawn(reprobe_all(app.clone(), ids)); }
        Ok(_) => {}
        Err(e) => eprintln!("Could not update the media library: {e}"),
    }
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;
