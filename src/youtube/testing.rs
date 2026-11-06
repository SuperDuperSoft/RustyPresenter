//! Test helpers: a stand-in for the release pages on this computer (no internet), and zips made in memory.
use super::fetch::Sources;
use crate::sha256::digest_hex;
use axum::{http::{StatusCode, Uri, header::LOCATION}, response::{IntoResponse, Response}};
use std::{collections::HashMap, io::Write, sync::{Arc, Mutex}};

#[derive(Clone)]
enum Reply {
    Body(Vec<u8>),
    Redirect(String),
}

#[derive(Default)]
struct Pages {
    replies: HashMap<String, Reply>,
    hits: usize,
    /// Answer every request with this status (offline-ish).
    fail: Option<u16>,
}

/// GitHub's release pages and ffmpeg.martin-riedl.de, as far as `fetch` uses them, served from 127.0.0.1.
pub(crate) struct FakeReleases {
    base: String,
    pages: Arc<Mutex<Pages>>,
}

impl FakeReleases {
    pub(crate) async fn start() -> FakeReleases {
        let pages: Arc<Mutex<Pages>> = Arc::default();
        let p = pages.clone();
        let app = axum::Router::new().fallback(move |uri: Uri| {
            let p = p.clone();
            async move {
                let mut pages = p.lock().unwrap();
                pages.hits += 1;
                if let Some(code) = pages.fail { return StatusCode::from_u16(code).unwrap().into_response(); }
                match pages.replies.get(uri.path()).cloned() {
                    Some(Reply::Body(b)) => b.into_response(),
                    Some(Reply::Redirect(to)) => Response::builder().status(StatusCode::FOUND).header(LOCATION, to).body(axum::body::Body::empty()).unwrap(),
                    None => StatusCode::NOT_FOUND.into_response(),
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { let _ = axum::serve(listener, app).await; });
        FakeReleases { base, pages }
    }

    pub(crate) fn sources(&self) -> Sources {
        let b = &self.base;
        Sources {
            yt_dlp: format!("{b}/yt-dlp/releases"),
            yt_dlp_nightly: format!("{b}/yt-dlp-nightly/releases"),
            deno: format!("{b}/deno/releases"),
            ffmpeg_builds: format!("{b}/ffmpeg-builds/releases"),
            ffmpeg_static: format!("{b}/riedl"),
        }
    }

    pub(crate) fn put(&self, path: &str, body: Vec<u8>) {
        self.pages.lock().unwrap().replies.insert(path.into(), Reply::Body(body));
    }

    fn redirect(&self, path: &str, to: &str) {
        self.pages.lock().unwrap().replies.insert(path.into(), Reply::Redirect(to.into()));
    }

    pub(crate) fn hits(&self) -> usize { self.pages.lock().unwrap().hits }

    pub(crate) fn fail_all(&self, status: u16) { self.pages.lock().unwrap().fail = Some(status); }

    /// A yt-dlp release whose Windows zip holds `yt-dlp.exe` (with `exe` as its bytes) and an `_internal` folder.
    pub(crate) fn yt_dlp_release(&self, tag: &str, exe: &[u8]) { self.yt_dlp_at("yt-dlp", tag, exe) }

    pub(crate) fn yt_dlp_nightly(&self, tag: &str, exe: &[u8]) { self.yt_dlp_at("yt-dlp-nightly", tag, exe) }

    fn yt_dlp_at(&self, repo: &str, tag: &str, exe: &[u8]) {
        let zip = zip_of(&[("yt-dlp.exe", exe), ("_internal/python314.dll", b"py")]);
        let sums = format!("{}  yt-dlp.exe\n{}  yt-dlp_win.zip\n", digest_hex(b"one-file"), digest_hex(&zip));
        self.put(&format!("/{repo}/releases/download/{tag}/yt-dlp_win.zip"), zip);
        self.put(&format!("/{repo}/releases/download/{tag}/SHA2-256SUMS"), sums.into_bytes());
        self.redirect(&format!("/{repo}/releases/latest/download/SHA2-256SUMS"), &format!("/{repo}/releases/download/{tag}/SHA2-256SUMS"));
    }

    /// A Deno release; `powershell` writes its fingerprint the way Deno's Windows builds do.
    pub(crate) fn deno_release(&self, tag: &str, asset: &str, zip: &[u8], powershell: bool) {
        let sha = digest_hex(zip);
        let sums = if powershell { format!("\nAlgorithm : SHA256\nHash      : {}\nPath      : C:\\a\\{asset}\n", sha.to_uppercase()) } else { format!("{sha}  {asset}\n") };
        self.put(&format!("/deno/releases/download/{tag}/{asset}"), zip.to_vec());
        self.put(&format!("/deno/releases/download/{tag}/{asset}.sha256sum"), sums.into_bytes());
        self.redirect(&format!("/deno/releases/latest/download/{asset}.sha256sum"), &format!("/deno/releases/download/{tag}/{asset}.sha256sum"));
    }

    /// yt-dlp/FFmpeg-Builds: one rolling release named `latest`.
    #[cfg_attr(not(windows), allow(dead_code))] // only the Windows-only layout test uses it
    pub(crate) fn ffmpeg_builds(&self, asset: &str, zip: &[u8]) {
        self.put(&format!("/ffmpeg-builds/releases/download/latest/{asset}"), zip.to_vec());
        self.put("/ffmpeg-builds/releases/download/latest/checksums.sha256", format!("{}  {asset}\n", digest_hex(zip)).into_bytes());
        self.redirect("/ffmpeg-builds/releases/latest/download/checksums.sha256", "/ffmpeg-builds/releases/download/latest/checksums.sha256");
    }

    /// ffmpeg.martin-riedl.de: `redirect/latest/…` points at the build, with `<file>.sha256` beside it.
    pub(crate) fn ffmpeg_static(&self, os_arch: &str, tag: &str, ffmpeg: &[u8], ffprobe: &[u8], wrong_sum: bool) {
        for (name, zip) in [("ffmpeg", ffmpeg), ("ffprobe", ffprobe)] {
            let at = format!("/riedl/download/{os_arch}/{tag}/{name}.zip");
            let sha = if wrong_sum { digest_hex(b"something else") } else { digest_hex(zip) };
            self.put(&at, zip.to_vec());
            self.put(&format!("{at}.sha256"), format!("{sha}  {name}.zip\n").into_bytes());
            self.redirect(&format!("/riedl/redirect/latest/{os_arch}/release/{name}.zip"), &at);
        }
    }
}

/// A zip with these files, deflated.
pub(crate) fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in files {
        w.start_file(*name, opts).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}
