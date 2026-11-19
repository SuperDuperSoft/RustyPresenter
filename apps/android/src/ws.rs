//! A small WebSocket client (RFC 6455) for the phone camera, as the browser's camera page uses: the phone sends each
//! JPEG picture as a binary message, the computer answers each with the camera's tally light as text. The other
//! side is the desktop app's own `network::websocket`.
use std::{
    io::{self, BufReader, Read, Write},
    net::TcpStream,
    time::Duration,
};

pub struct Socket {
    read: BufReader<TcpStream>,
    write: TcpStream,
    seed: u64,
}

/// A message from the computer.
#[derive(Debug, PartialEq)]
pub enum Message { Text(String), Binary(Vec<u8>), Closed }

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

/// A changing number for the masks and the key (a mask only keeps proxies from reading the stream as HTTP: it
/// needs no secrecy).
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    h.finish() | 1
}

/// One frame as a client writes it: whole (`fin`), masked.
pub fn frame(op: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | (op & 0x0F));
    match payload.len() {
        n if n < 126 => out.push(0x80 | n as u8),
        n if n <= 0xFFFF => { out.push(0x80 | 126); out.extend((n as u16).to_be_bytes()) }
        n => { out.push(0x80 | 127); out.extend((n as u64).to_be_bytes()) }
    }
    out.extend(mask);
    out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    out
}

impl Socket {
    /// Opens `path` on `host` as a WebSocket, with `cookie` (`rp_session=…`) for the phone's sign-in. `Err` names
    /// the status when the computer says no (401: pair first).
    pub fn connect(host: &str, path: &str, cookie: Option<&str>, timeout: Duration) -> io::Result<Socket> {
        let s = crate::http::connect(host, timeout)?;
        s.set_read_timeout(Some(timeout))?;
        s.set_write_timeout(Some(timeout))?;
        let mut seed = seed();
        let key: Vec<u8> = (0..2).flat_map(|_| next(&mut seed).to_le_bytes()).collect();
        let mut head = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {}\r\n", base64(&key));
        if let Some(c) = cookie { head += &format!("Cookie: {c}\r\n") }
        head += "\r\n";
        let mut write = s.try_clone()?;
        write.write_all(head.as_bytes())?;
        let mut read = BufReader::new(s);
        let (status, _) = crate::http::read_head(&mut read)?;
        if status != 101 { return Err(io::Error::other(format!("status {status}"))) }
        Ok(Socket { read, write, seed })
    }

    pub fn send_binary(&mut self, data: &[u8]) -> io::Result<()> {
        let mask = (next(&mut self.seed) as u32).to_le_bytes();
        self.write.write_all(&frame(2, data, mask))
    }

    pub fn close(&mut self) {
        let mask = (next(&mut self.seed) as u32).to_le_bytes();
        let _ = self.write.write_all(&frame(8, &[], mask));
    }

    /// The next message (pings are answered on the way).
    pub fn recv(&mut self) -> io::Result<Message> {
        let mut whole = Vec::new();
        let mut kind = 0u8;
        loop {
            let mut h = [0u8; 2];
            self.read.read_exact(&mut h)?;
            let (fin, op) = (h[0] & 0x80 != 0, h[0] & 0x0F);
            let masked = h[1] & 0x80 != 0;
            let mut len = u64::from(h[1] & 0x7F);
            if len == 126 { let mut b = [0u8; 2]; self.read.read_exact(&mut b)?; len = u64::from(u16::from_be_bytes(b)) }
            else if len == 127 { let mut b = [0u8; 8]; self.read.read_exact(&mut b)?; len = u64::from_be_bytes(b) }
            if len > 16 << 20 { return Err(io::Error::new(io::ErrorKind::InvalidData, "message too big")) }
            let mut mask = [0u8; 4];
            if masked { self.read.read_exact(&mut mask)? }
            let mut payload = vec![0u8; len as usize];
            self.read.read_exact(&mut payload)?;
            if masked { for (i, b) in payload.iter_mut().enumerate() { *b ^= mask[i % 4] } }
            match op {
                8 => return Ok(Message::Closed),
                9 => { let m = (next(&mut self.seed) as u32).to_le_bytes(); self.write.write_all(&frame(10, &payload, m))?; continue }
                10 => continue,
                0 => whole.extend(payload),
                _ => { kind = op; whole = payload }
            }
            if fin {
                return Ok(if kind == 1 { Message::Text(String::from_utf8_lossy(&whole).into_owned()) } else { Message::Binary(whole) });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::BufRead, net::TcpListener};

    #[test]
    fn base64_as_the_standard_says() {
        assert_eq!(base64(b"the sample nonce"), "dGhlIHNhbXBsZSBub25jZQ==");
        assert_eq!(base64(b"ab"), "YWI=");
    }

    /// A computer played by a test: it checks the handshake, unmasks the phone's picture and answers its light,
    /// as `network::phone_camera` does.
    #[test]
    fn a_picture_goes_out_and_the_light_comes_back() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = l.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut head = String::new();
            loop { let mut line = String::new(); r.read_line(&mut line).unwrap(); head += &line; if line == "\r\n" { break } }
            s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").unwrap();
            let mut h = [0u8; 2];
            r.read_exact(&mut h).unwrap();
            assert_eq!((h[0], h[1] & 0x80), (0x82, 0x80), "a whole binary frame, masked");
            let mut len = [0u8; 2];
            assert_eq!(h[1] & 0x7F, 126);
            r.read_exact(&mut len).unwrap();
            let mut mask = [0u8; 4];
            r.read_exact(&mut mask).unwrap();
            let mut p = vec![0u8; u16::from_be_bytes(len) as usize];
            r.read_exact(&mut p).unwrap();
            for (i, b) in p.iter_mut().enumerate() { *b ^= mask[i % 4] }
            let text = br#"{"light":"on_air","text":"On air"}"#;
            let mut answer = vec![0x81, text.len() as u8];
            answer.extend(text);
            s.write_all(&answer).unwrap();
            (head, p)
        });
        let mut sock = Socket::connect(&host, "/camera/ws?name=Balcony", Some("rp_session=tok"), Duration::from_secs(2)).unwrap();
        let picture: Vec<u8> = [0xFF, 0xD8].into_iter().chain((0..300u32).map(|i| i as u8)).collect();
        sock.send_binary(&picture).unwrap();
        assert_eq!(sock.recv().unwrap(), Message::Text(r#"{"light":"on_air","text":"On air"}"#.into()));
        let (head, got) = server.join().unwrap();
        assert_eq!(got, picture);
        assert!(head.starts_with("GET /camera/ws?name=Balcony HTTP/1.1\r\n") && head.contains("Cookie: rp_session=tok\r\n"), "{head}");
        assert!(head.contains("Sec-WebSocket-Version: 13") && head.contains("Upgrade: websocket"));
    }

    #[test]
    fn a_refusal_is_an_error() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 512];
            let _ = s.read(&mut b);
            s.write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\n\r\n").unwrap();
        });
        let e = Socket::connect(&host, "/camera/ws?name=x", None, Duration::from_secs(2)).err().unwrap();
        assert!(e.to_string().contains("401"));
    }
}
