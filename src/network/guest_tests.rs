use super::*;
use crate::app::test_app;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn the_relay_gives_a_new_reader_the_header_then_the_pieces() {
    let mut r = Relay::default();
    assert!(r.reader().is_none(), "no guest yet");
    r.piece(7, 1, Bytes::from_static(b"stray"));
    assert!(r.reader().is_none(), "a piece before a session's start is dropped");
    r.piece(7, 0, Bytes::from_static(b"HEAD"));
    let (session, head, mut rx) = r.reader().unwrap();
    assert_eq!((session, &head[..]), (7, &b"HEAD"[..]));
    r.piece(7, 1, Bytes::from_static(b"one"));
    r.piece(6, 2, Bytes::from_static(b"old"));
    assert_eq!(rx.try_recv().unwrap(), (7, Bytes::from_static(b"one")));
    assert!(rx.try_recv().is_err(), "an old session's piece is not passed on");
    r.end();
    assert!(r.reader().is_none());
    assert_eq!(rx.try_recv().unwrap().0, u64::MAX, "readers are told it ended");
}

#[test]
fn the_guest_camera_is_known_by_its_address() {
    let a = camera_address(4318);
    assert_eq!(a, "http://127.0.0.1:4318/guest/video.webm");
    assert!(is_guest_camera(&a) && crate::record::args::is_network_camera(&a), "read as a network camera");
    assert!(!is_guest_camera("rtsp://192.168.1.50/stream1"));
    assert_eq!(crate::record::args::camera_label(&a), "Remote guest");
    assert_eq!(host_address(4318), "http://127.0.0.1:4318/guest/host");
}

async fn served() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let svc = crate::server::router(test_app()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, svc).await });
    addr
}

/// A request as a browser or FFmpeg would make it: the status code and the body read within `wait`.
async fn ask(addr: SocketAddr, req: &str, wait: Duration) -> (u16, Vec<u8>) {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(req.as_bytes()).await.unwrap();
    let mut got = vec![];
    let mut b = [0u8; 4096];
    let until = tokio::time::Instant::now() + wait;
    while let Ok(Ok(n)) = tokio::time::timeout_at(until, s.read(&mut b)).await {
        if n == 0 { break }
        got.extend_from_slice(&b[..n]);
    }
    let code = String::from_utf8_lossy(&got[9..12]).parse().unwrap_or(0);
    (code, got)
}

fn post(addr: SocketAddr, path: &str, body: &[u8], page: bool) -> String {
    let extra = if page { "x-rp-guest: 1\r\n" } else { "" };
    format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{}", addr.port(), body.len(), String::from_utf8_lossy(body))
}

/// The host page's pieces come out of `/guest/video.webm` as one file; a page elsewhere (no `x-rp-guest`, the
/// browser would have asked first) and another computer's name for this one are refused.
#[tokio::test]
async fn the_guests_picture_goes_from_the_host_page_to_the_camera() {
    let addr = served().await;
    *relay().lock().unwrap() = Relay::default();
    let get = |path: &str| format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", addr.port());
    assert_eq!(ask(addr, &get("/guest/video.webm"), Duration::from_secs(2)).await.0, 503, "no guest yet");
    assert_eq!(ask(addr, &post(addr, "/guest/video?session=1&n=0", b"HEAD", false), Duration::from_secs(2)).await.0, 403);
    let other = format!("GET /guest/host HTTP/1.1\r\nHost: church-pc:{}\r\nConnection: close\r\n\r\n", addr.port());
    assert_eq!(ask(addr, &other, Duration::from_secs(2)).await.0, 403, "only as 127.0.0.1");
    let (code, page) = ask(addr, &get("/guest/host"), Duration::from_secs(2)).await;
    assert_eq!(code, 200);
    assert!(String::from_utf8_lossy(&page).contains("RTCPeerConnection"));
    assert_eq!(ask(addr, &post(addr, "/guest/video?session=1&n=0", b"HEAD", true), Duration::from_secs(2)).await.0, 204);
    // A reader (the stream's FFmpeg) gets the header, then each piece as it comes.
    let reader = tokio::spawn(async move { ask(addr, &format!("GET /guest/video.webm HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", addr.port()), Duration::from_secs(3)).await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(ask(addr, &post(addr, "/guest/video?session=1&n=1", b"PIECE-ONE", true), Duration::from_secs(2)).await.0, 204);
    assert_eq!(ask(addr, &post(addr, "/guest/state?state=failed", b"", true), Duration::from_secs(2)).await.0, 204);
    let (code, body) = reader.await.unwrap();
    let body = String::from_utf8_lossy(&body).into_owned();
    assert_eq!(code, 200);
    assert!(body.contains("video/webm") && body.contains("HEAD") && body.contains("PIECE-ONE"), "{body}");
    assert_eq!(relay().lock().unwrap().state, "failed");
    // The guest's own page, to save and send.
    let (code, page) = ask(addr, &get("/guest/join?save=1"), Duration::from_secs(2)).await;
    assert_eq!(code, 200);
    let page = String::from_utf8_lossy(&page);
    assert!(page.contains("attachment") && page.contains("getUserMedia"));
}
