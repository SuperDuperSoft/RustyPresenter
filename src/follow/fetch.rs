//! The ear's speech models, downloaded from Hugging Face the first time they are wanted (never in the installer)
//! into `<data>/deps/follow-models/<model>/`. Each comes from a pinned commit, the weights are checked against
//! their SHA-256 before they are used, and the model's licence (Apache-2.0) is kept beside it. A download cut off
//! goes on from where it stopped next time.
use super::ear::Choice;
use crate::sha256::{Digest, Sha256, hex};
use crate::youtube::fetch::{FetchError, client, get_bytes, io_at, ok_or_status};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

/// One downloadable model.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// The Hugging Face repository.
    pub repo: &'static str,
    /// Its commit: what is downloaded never changes.
    pub rev: &'static str,
    /// The weights file and its SHA-256 and size.
    pub weights: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    /// The folder under `follow-models`.
    pub folder: &'static str,
}

/// `facebook/wav2vec2-base-960h` (Apache-2.0): English letters, 95 M weights.
pub const FAST: Model = Model {
    repo: "facebook/wav2vec2-base-960h",
    rev: "22aad52d435eb6dbaf354bdad9b0da84ce7d6156",
    weights: "model.safetensors",
    sha256: "8aa76ab2243c81747a1f832954586bc566090c83a0ac167df6f31f0fa917d74a",
    size: 377_607_901,
    folder: "wav2vec2-base-960h",
};

/// `facebook/wav2vec2-xlsr-53-espeak-cv-ft` (Apache-2.0): phonemes of any language, 315 M weights. It has no
/// safetensors file, so the PyTorch one is read (Candle reads it without Python).
pub const BEST: Model = Model {
    repo: "facebook/wav2vec2-xlsr-53-espeak-cv-ft",
    rev: "2c733782da5604684829819a5eb744c193fe9398",
    weights: "pytorch_model.bin",
    sha256: "04366b6c8d24099ef313cf02f0e58d26f5dddfda16edbfc8eb2c713d94a9f551",
    size: 1_263_535_127,
    folder: "wav2vec2-xlsr-53-espeak-cv-ft",
};

/// The small files beside the weights (fixed by the commit).
pub const SMALL_FILES: [&str; 2] = ["config.json", "vocab.json"];
pub const LICENCE_FILE: &str = "LICENSE.txt";
pub const HUGGING_FACE: &str = "https://huggingface.co";

impl Choice {
    pub fn model(self) -> Option<&'static Model> {
        match self { Choice::Best => Some(&BEST), Choice::Fast => Some(&FAST), Choice::Off => None }
    }
}

/// `<data>/deps`, where the app's downloaded helpers live.
pub fn deps_dir() -> PathBuf { crate::server::data_dir().unwrap_or_default().join("deps") }

pub fn models_dir(deps: &Path) -> PathBuf { deps.join("follow-models") }

/// The model's folder, when it is fully downloaded and checked.
pub fn installed(deps: &Path, model: &Model) -> Option<PathBuf> {
    let dir = models_dir(deps).join(model.folder);
    let all = [model.weights, LICENCE_FILE].into_iter().chain(SMALL_FILES).all(|f| dir.join(f).is_file());
    all.then_some(dir)
}

/// The licence kept beside a model.
fn licence(model: &Model) -> String {
    format!("{} (https://huggingface.co/{}) is licensed under the Apache License, Version 2.0:\n\n{}", model.repo, model.repo,
        include_str!("../../LICENSE-APACHE"))
}

/// Downloads `model` from `base` (Hugging Face, or a test server) unless it is already there; `progress` hears the
/// bytes done of the total. Returns its folder.
pub async fn ensure(deps: &Path, model: &Model, base: &str, progress: &(dyn Fn(u64, u64) + Send + Sync), cancel: &CancellationToken)
    -> Result<PathBuf, FetchError> {
    if let Some(dir) = installed(deps, model) { return Ok(dir) }
    let dir = models_dir(deps).join(model.folder);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| io_at(&dir, e))?;
    let url = |file: &str| format!("{base}/{}/resolve/{}/{file}", model.repo, model.rev);
    for f in SMALL_FILES {
        let bytes = get_bytes(&url(f), 1024 * 1024).await?;
        let to = dir.join(f);
        crate::store::write_atomic(&to, &bytes).map_err(|e| io_at(&to, e))?;
    }
    let to = dir.join(LICENCE_FILE);
    crate::store::write_atomic(&to, licence(model).as_bytes()).map_err(|e| io_at(&to, e))?;
    download(&url(model.weights), &dir.join(model.weights), model.sha256, model.size, progress, cancel).await?;
    Ok(dir)
}

/// Streams `url` into `<to>.part` (going on from what is already there), checks its SHA-256 and size, then
/// renames it to `to`. A file that doesn't match is thrown away.
async fn download(url: &str, to: &Path, sha256: &str, size: u64, progress: &(dyn Fn(u64, u64) + Send + Sync), cancel: &CancellationToken)
    -> Result<(), FetchError> {
    let part = to.with_extension(format!("{}.part", to.extension().and_then(|e| e.to_str()).unwrap_or("")));
    let mut have = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    if have > size { let _ = tokio::fs::remove_file(&part).await; have = 0; }
    if have < size {
        let mut req = client()?.get(url);
        if have > 0 { req = req.header(reqwest::header::RANGE, format!("bytes={have}-")) }
        let mut resp = tokio::select! {
            _ = cancel.cancelled() => return Err(FetchError::Cancelled),
            r = req.send() => ok_or_status(r?, url)?,
        };
        // The server may ignore the range and send it all again.
        let append = have > 0 && resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        if !append { have = 0 }
        let mut out = tokio::fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(&part).await
            .map_err(|e| io_at(&part, e))?;
        progress(have, size);
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => { let _ = out.flush().await; return Err(FetchError::Cancelled) }
                c = resp.chunk() => c?,
            };
            let Some(chunk) = chunk else { break };
            have += chunk.len() as u64;
            if have > size {
                drop(out);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(FetchError::Checksum);
            }
            out.write_all(&chunk).await.map_err(|e| io_at(&part, e))?;
            progress(have, size);
        }
        out.flush().await.map_err(|e| io_at(&part, e))?;
    }
    let file = part.clone();
    let got = tokio::task::spawn_blocking(move || -> std::io::Result<(String, u64)> {
        use std::io::Read;
        let mut f = std::fs::File::open(&file)?;
        let (mut h, mut n, mut buf) = (Sha256::new(), 0u64, vec![0u8; 1 << 20]);
        loop {
            let k = f.read(&mut buf)?;
            if k == 0 { break }
            h.update(&buf[..k]);
            n += k as u64;
        }
        Ok((hex(&h.finalize()), n))
    }).await.map_err(|e| FetchError::Other(e.to_string()))?.map_err(|e| io_at(&part, e))?;
    if got.1 < size { return Err(FetchError::Offline(crate::tr!("{address} stopped part way", address = url))) }
    if got.0 != sha256 || got.1 != size {
        eprintln!("{url} did not match its fingerprint: got {}, want {sha256}", got.0);
        let _ = tokio::fs::remove_file(&part).await;
        return Err(FetchError::Checksum);
    }
    tokio::fs::rename(&part, to).await.map_err(|e| io_at(to, e))?;
    Ok(())
}

#[cfg(test)]
#[path = "fetch_tests.rs"]
mod tests;
