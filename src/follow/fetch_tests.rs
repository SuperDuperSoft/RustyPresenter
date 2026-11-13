use super::*;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A stand-in Hugging Face: files by path; ranges honoured; the ranges asked for are kept.
struct Hub { base: String, files: Arc<Mutex<HashMap<String, Vec<u8>>>>, ranges: Arc<Mutex<Vec<String>>> }

async fn hub() -> Hub {
    let files: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
    let ranges: Arc<Mutex<Vec<String>>> = Arc::default();
    let (f, r) = (files.clone(), ranges.clone());
    let app = axum::Router::new().fallback(move |uri: Uri, headers: HeaderMap| {
        let (f, r) = (f.clone(), r.clone());
        async move {
            let Some(body) = f.lock().unwrap().get(uri.path()).cloned() else { return StatusCode::NOT_FOUND.into_response() };
            let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).map(String::from);
            if let Some(range) = range {
                r.lock().unwrap().push(range.clone());
                let from: usize = range.trim_start_matches("bytes=").trim_end_matches('-').parse().unwrap();
                return Response::builder().status(StatusCode::PARTIAL_CONTENT).body(axum::body::Body::from(body[from..].to_vec())).unwrap();
            }
            body.into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { let _ = axum::serve(listener, app).await; });
    Hub { base, files, ranges }
}

const TEST_MODEL: Model = Model { repo: "facebook/tiny", rev: "abc123", weights: "model.safetensors", sha256: "", size: 0, folder: "tiny" };

fn temp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("rp-follow-fetch-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
async fn fetch_checks_sha256() {
    let hub = hub().await;
    let weights: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let sha = crate::sha256::digest_hex(&weights);
    for (f, body) in [("config.json", b"{}".to_vec()), ("vocab.json", b"{\"<pad>\": 0}".to_vec()), ("model.safetensors", weights.clone())] {
        hub.files.lock().unwrap().insert(format!("/facebook/tiny/resolve/abc123/{f}"), body);
    }
    let sha: &'static str = Box::leak(sha.into_boxed_str());
    let good = Model { sha256: sha, size: weights.len() as u64, ..TEST_MODEL };
    let cancel = CancellationToken::new();

    // A wrong fingerprint: thrown away, nothing installed.
    let deps = temp();
    let bad = Model { sha256: "00", ..good.clone() };
    assert_eq!(ensure(&deps, &bad, &hub.base, &|_, _| {}, &cancel).await, Err(FetchError::Checksum));
    let dir = models_dir(&deps).join("tiny");
    assert!(!dir.join("model.safetensors").exists() && !dir.join("model.safetensors.part").exists());
    assert_eq!(installed(&deps, &bad), None);

    // The right one: in place with its licence; progress reaches the end.
    let seen = Mutex::new((0, 0));
    let got = ensure(&deps, &good, &hub.base, &|d, t| *seen.lock().unwrap() = (d, t), &cancel).await.unwrap();
    assert_eq!(got, dir);
    assert_eq!(std::fs::read(dir.join("model.safetensors")).unwrap(), weights);
    assert_eq!(std::fs::read_to_string(dir.join("vocab.json")).unwrap(), "{\"<pad>\": 0}");
    let lic = std::fs::read_to_string(dir.join("LICENSE.txt")).unwrap();
    assert!(lic.starts_with("facebook/tiny (https://huggingface.co/facebook/tiny) is licensed under the Apache License"), "{lic}");
    assert!(lic.contains("TERMS AND CONDITIONS FOR USE"));
    assert_eq!(*seen.lock().unwrap(), (weights.len() as u64, weights.len() as u64));
    assert_eq!(installed(&deps, &good), Some(dir.clone()));
    assert!(hub.ranges.lock().unwrap().is_empty());

    // Cut off half way last time: goes on from there.
    let deps = temp();
    let dir = models_dir(&deps).join("tiny");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("model.safetensors.part"), &weights[..120_000]).unwrap();
    ensure(&deps, &good, &hub.base, &|_, _| {}, &cancel).await.unwrap();
    assert_eq!(std::fs::read(dir.join("model.safetensors")).unwrap(), weights);
    assert_eq!(*hub.ranges.lock().unwrap(), ["bytes=120000-"]);
    assert!(!dir.join("model.safetensors.part").exists());

    // The real models are pinned.
    for m in [&FAST, &BEST] {
        assert_eq!(m.sha256.len(), 64);
        assert_eq!(m.rev.len(), 40);
        assert!(m.size > 300_000_000);
    }
}
