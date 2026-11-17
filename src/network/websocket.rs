//! A small WebSocket (RFC 6455) for the phone cameras: the handshake on hyper's upgrade, and the frames read and
//! written. Only what a browser sends is needed: text and binary messages (also in pieces), ping, close. The server
//! sends text only. Written here because the WebSocket crates are not in the build.
use axum::{
    body::Body,
    http::{HeaderMap, Response, StatusCode, header},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The key every WebSocket handshake mixes in.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// The `Sec-WebSocket-Accept` answer to the browser's `Sec-WebSocket-Key`.
pub fn accept_key(key: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, format!("{}{GUID}", key.trim()).as_bytes());
    base64(digest.as_ref())
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() { out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char) } else { out.push('=') }
        }
    }
    out
}

/// Whether the request asks for a WebSocket; its key when it does.
pub fn wants_websocket(headers: &HeaderMap) -> Option<String> {
    let has = |name: header::HeaderName, word: &str| headers.get_all(name).iter().filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(',')).any(|v| v.trim().eq_ignore_ascii_case(word));
    if !has(header::UPGRADE, "websocket") || !has(header::CONNECTION, "upgrade") { return None }
    headers.get(header::SEC_WEBSOCKET_KEY).and_then(|k| k.to_str().ok()).map(str::to_string)
}

/// The "switching to WebSocket" answer for key `key`.
pub fn switching(key: &str) -> Response<Body> {
    Response::builder().status(StatusCode::SWITCHING_PROTOCOLS)
        .header(header::UPGRADE, "websocket").header(header::CONNECTION, "Upgrade")
        .header(header::SEC_WEBSOCKET_ACCEPT, accept_key(key)).body(Body::empty()).unwrap_or_default()
}

/// A message from the browser.
#[derive(Debug, PartialEq)]
pub enum Message { Text(String), Binary(Vec<u8>) }

/// One frame as written: `fin`, the kind (`op`), and the payload, masked with `mask` when given (a browser masks,
/// a server does not).
pub fn frame(op: u8, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
    let mut out = vec![0x80 | (op & 0x0F)];
    let m = if mask.is_some() { 0x80 } else { 0 };
    match payload.len() {
        n if n < 126 => out.push(m | n as u8),
        n if n <= 0xFFFF => { out.push(m | 126); out.extend((n as u16).to_be_bytes()) }
        n => { out.push(m | 127); out.extend((n as u64).to_be_bytes()) }
    }
    match mask {
        Some(k) => { out.extend(k); out.extend(payload.iter().enumerate().map(|(i, b)| b ^ k[i % 4])) }
        None => out.extend(payload),
    }
    out
}

const TEXT: u8 = 1;
const BINARY: u8 = 2;
const CLOSE: u8 = 8;
const PING: u8 = 9;
const PONG: u8 = 10;

/// A WebSocket connection. `max`: the biggest message taken; a bigger one ends the connection.
pub struct Socket<S> { io: S, max: usize }

impl<S: AsyncRead + AsyncWrite + Unpin> Socket<S> {
    pub fn new(io: S, max: usize) -> Socket<S> { Socket { io, max } }

    /// The next message; `None` when the browser closed the connection or it broke. Pings are answered here.
    pub async fn recv(&mut self) -> Option<Message> {
        let mut message: Option<(u8, Vec<u8>)> = None;
        loop {
            let mut head = [0u8; 2];
            self.io.read_exact(&mut head).await.ok()?;
            let (fin, op, masked) = (head[0] & 0x80 != 0, head[0] & 0x0F, head[1] & 0x80 != 0);
            let len = match head[1] & 0x7F {
                126 => { let mut b = [0u8; 2]; self.io.read_exact(&mut b).await.ok()?; u64::from(u16::from_be_bytes(b)) }
                127 => { let mut b = [0u8; 8]; self.io.read_exact(&mut b).await.ok()?; u64::from_be_bytes(b) }
                n => u64::from(n),
            };
            let so_far = message.as_ref().map_or(0, |(_, m)| m.len());
            if len > (self.max - so_far.min(self.max)) as u64 { let _ = self.close().await; return None }
            let mut key = [0u8; 4];
            if masked { self.io.read_exact(&mut key).await.ok()?; }
            let mut payload = vec![0u8; len as usize];
            self.io.read_exact(&mut payload).await.ok()?;
            if masked { payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= key[i % 4]) }
            match op {
                CLOSE => { let _ = self.close().await; return None }
                PING => { self.io.write_all(&frame(PONG, &payload, None)).await.ok()?; continue }
                PONG => continue,
                TEXT | BINARY if message.is_none() => message = Some((op, payload)),
                0 if message.is_some() => if let Some((_, m)) = message.as_mut() { m.extend(payload) },
                _ => { let _ = self.close().await; return None }
            }
            if fin {
                let (op, m) = message.take()?;
                return Some(if op == TEXT { Message::Text(String::from_utf8_lossy(&m).into_owned()) } else { Message::Binary(m) });
            }
        }
    }

    /// Sends a text message.
    pub async fn send(&mut self, text: &str) -> std::io::Result<()> { self.io.write_all(&frame(TEXT, text.as_bytes(), None)).await }

    /// Says goodbye.
    pub async fn close(&mut self) -> std::io::Result<()> {
        self.io.write_all(&frame(CLOSE, &1000u16.to_be_bytes(), None)).await?;
        self.io.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handshake_answer_is_the_rfcs() {
        // RFC 6455 §1.3.
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        let mut h = HeaderMap::new();
        h.insert(header::UPGRADE, "websocket".parse().unwrap());
        h.insert(header::CONNECTION, "keep-alive, Upgrade".parse().unwrap());
        assert_eq!(wants_websocket(&h), None, "no key");
        h.insert(header::SEC_WEBSOCKET_KEY, "abc".parse().unwrap());
        assert_eq!(wants_websocket(&h).as_deref(), Some("abc"));
    }

    #[tokio::test]
    async fn messages_are_read_in_pieces_and_pings_answered() {
        let (mut browser, app) = tokio::io::duplex(1 << 20);
        let mut s = Socket::new(app, 100_000);
        let k = Some([1, 2, 3, 4]);
        let mut sent = frame(PING, b"hi", k);
        // "Hello" in two pieces, then a big binary one.
        sent.extend([0x01, 0x80 | 3, 1, 2, 3, 4]);
        sent.extend(b"Hel".iter().enumerate().map(|(i, b)| b ^ [1, 2, 3, 4][i % 4]));
        sent.extend(frame(0, b"lo", k));
        let big = vec![7u8; 70_000];
        sent.extend(frame(BINARY, &big, k));
        browser.write_all(&sent).await.unwrap();
        assert_eq!(s.recv().await, Some(Message::Text("Hello".into())));
        assert_eq!(s.recv().await, Some(Message::Binary(big)));
        let mut pong = [0u8; 4];
        browser.read_exact(&mut pong).await.unwrap();
        assert_eq!(pong, [0x80 | PONG, 2, b'h', b'i']);
        s.send("ok").await.unwrap();
        let mut got = [0u8; 4];
        browser.read_exact(&mut got).await.unwrap();
        assert_eq!(got, [0x81, 2, b'o', b'k']);
        // Too big: the connection ends.
        browser.write_all(&frame(BINARY, &vec![0u8; 100_001], k)).await.unwrap();
        assert_eq!(s.recv().await, None);
    }
}
