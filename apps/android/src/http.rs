//! A small HTTP/1.1 client for the computer's phone server (plain http on the church's Wi-Fi, as the browser
//! remote uses): one request a connection, the sign-in cookies kept, and the live updates read as Server-Sent
//! Events. Written out here rather than taken from a crate: the server is ours, the requests are few and simple,
//! and the app stays small.
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    sync::Mutex,
    time::Duration,
};

/// An answer: its status, headers (names in small letters) and body.
#[derive(Debug, Clone, Default)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn ok(&self) -> bool { (200..300).contains(&self.status) }
    pub fn json(&self) -> serde_json::Value { serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null) }
    /// The `name=value` of each `Set-Cookie`.
    pub fn set_cookies(&self) -> Vec<(String, String)> {
        self.headers.iter().filter(|(n, _)| n == "set-cookie").filter_map(|(_, v)| parse_set_cookie(v)).collect()
    }
}

/// `rp_session=abc; HttpOnly; Path=/` → ("rp_session", "abc").
pub fn parse_set_cookie(v: &str) -> Option<(String, String)> {
    let first = v.split(';').next()?.trim();
    let (name, value) = first.split_once('=')?;
    let name = name.trim();
    (!name.is_empty()).then(|| (name.to_string(), value.trim().to_string()))
}

/// One computer: its address (`192.168.1.20:4318`) and the cookies it gave this phone.
pub struct Client {
    pub host: String,
    cookies: Mutex<Vec<(String, String)>>,
    pub timeout: Duration,
}

impl Client {
    pub fn new(host: &str) -> Client {
        Client { host: host.to_string(), cookies: Mutex::new(Vec::new()), timeout: Duration::from_secs(5) }
    }

    pub fn set_cookie(&self, name: &str, value: Option<&str>) {
        let mut c = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        c.retain(|(n, _)| n != name);
        if let Some(v) = value.filter(|v| !v.is_empty()) { c.push((name.to_string(), v.to_string())) }
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        self.cookies.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())
    }

    /// `Cookie: a=1; b=2`, or nothing.
    pub fn cookie_header(&self) -> Option<String> {
        let c = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        (!c.is_empty()).then(|| c.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; "))
    }

    pub fn get(&self, path: &str) -> io::Result<Response> { self.send("GET", path, None, &[]) }

    pub fn post_json(&self, path: &str, body: &serde_json::Value) -> io::Result<Response> {
        self.send("POST", path, Some("application/json"), body.to_string().as_bytes())
    }

    pub fn post_bytes(&self, path: &str, content_type: &str, body: &[u8]) -> io::Result<Response> {
        self.send("POST", path, Some(content_type), body)
    }

    /// One request; the cookies it sets are kept.
    pub fn send(&self, method: &str, path: &str, content_type: Option<&str>, body: &[u8]) -> io::Result<Response> {
        let mut s = connect(&self.host, self.timeout)?;
        s.set_read_timeout(Some(self.timeout))?;
        s.set_write_timeout(Some(self.timeout))?;
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: */*\r\n", self.host);
        if let Some(c) = self.cookie_header() { head += &format!("Cookie: {c}\r\n") }
        if let Some(t) = content_type { head += &format!("Content-Type: {t}\r\n") }
        if method != "GET" || !body.is_empty() { head += &format!("Content-Length: {}\r\n", body.len()) }
        head += "\r\n";
        s.write_all(head.as_bytes())?;
        s.write_all(body)?;
        let r = read_response(BufReader::new(s))?;
        for (n, v) in r.set_cookies() { self.set_cookie(&n, Some(&v)) }
        Ok(r)
    }

    /// Opens `path` as a stream of Server-Sent Events and calls `each` with every message's data until `go_on`
    /// says stop (it is asked at least twice a second), the stream ends, or nothing comes for `quiet` (the server
    /// sends a keep-alive every 10 s, so a longer silence is a lost connection). `Err` carries the status when the
    /// answer is not a stream (401: pair first).
    pub fn events(&self, path: &str, quiet: Duration, go_on: &dyn Fn() -> bool, mut each: impl FnMut(&str)) -> Result<(), StreamEnd> {
        let mut s = connect(&self.host, self.timeout).map_err(StreamEnd::Io)?;
        s.set_read_timeout(Some(self.timeout)).map_err(StreamEnd::Io)?;
        let mut head = format!("GET {path} HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\nCache-Control: no-store\r\n", self.host);
        if let Some(c) = self.cookie_header() { head += &format!("Cookie: {c}\r\n") }
        head += "\r\n";
        s.write_all(head.as_bytes()).map_err(StreamEnd::Io)?;
        let mut r = BufReader::new(s);
        let (status, headers) = read_head(&mut r).map_err(StreamEnd::Io)?;
        if status != 200 { return Err(StreamEnd::Status(status)) }
        let chunked = headers.iter().any(|(n, v)| n == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked"));
        // Short waits from here on, so `go_on` is asked often; the line read so far is kept across them.
        r.get_ref().set_read_timeout(Some(Duration::from_millis(500))).map_err(StreamEnd::Io)?;
        let mut body: Box<dyn BufRead> = if chunked { Box::new(BufReader::new(Chunked::new(r))) } else { Box::new(r) };
        let mut sse = Sse::default();
        let mut line = Vec::new();
        let mut heard = std::time::Instant::now();
        loop {
            if !go_on() { return Ok(()) }
            match body.read_until(b'\n', &mut line) {
                Ok(0) => return Err(StreamEnd::Closed),
                Ok(_) if line.ends_with(b"\n") => {
                    heard = std::time::Instant::now();
                    let text = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
                    line.clear();
                    if let Some(data) = sse.line(&text) { each(&data) }
                }
                Ok(_) => {}
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    if heard.elapsed() > quiet { return Err(StreamEnd::Closed) }
                }
                Err(e) => return Err(StreamEnd::Io(e)),
            }
        }
    }
}

/// Why a stream of events stopped.
#[derive(Debug)]
pub enum StreamEnd {
    Io(io::Error),
    Status(u16),
    Closed,
}

pub fn connect(host: &str, timeout: Duration) -> io::Result<TcpStream> {
    let addrs: Vec<SocketAddr> = host.to_socket_addrs()?.collect();
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no address");
    for a in addrs {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => { let _ = s.set_nodelay(true); return Ok(s) }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// The status line and headers.
pub fn read_head(r: &mut impl BufRead) -> io::Result<(u16, Vec<(String, String)>)> {
    let mut line = String::new();
    r.read_line(&mut line)?;
    let status = line.split_whitespace().nth(1).and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("not HTTP: {line:?}")))?;
    let mut headers = Vec::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 { break }
        let l = line.trim_end();
        if l.is_empty() { break }
        if let Some((n, v)) = l.split_once(':') { headers.push((n.trim().to_ascii_lowercase(), v.trim().to_string())) }
    }
    Ok((status, headers))
}

/// A whole answer: chunked, with a length, or up to the end.
pub fn read_response(mut r: impl BufRead) -> io::Result<Response> {
    let (status, headers) = read_head(&mut r)?;
    let mut body = Vec::new();
    let find = |n: &str| headers.iter().find(|(h, _)| h == n).map(|(_, v)| v.clone());
    if find("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
        Chunked::new(r).read_to_end(&mut body)?;
    } else if let Some(n) = find("content-length").and_then(|v| v.parse::<usize>().ok()) {
        body.resize(n, 0);
        r.read_exact(&mut body)?;
    } else {
        r.read_to_end(&mut body)?;
    }
    Ok(Response { status, headers, body })
}

/// A chunked body as plain bytes.
pub struct Chunked<R: BufRead> { inner: R, left: usize, done: bool }

impl<R: BufRead> Chunked<R> {
    pub fn new(inner: R) -> Self { Chunked { inner, left: 0, done: false } }
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done || buf.is_empty() { return Ok(0) }
        if self.left == 0 {
            let mut line = String::new();
            // The end of the chunk before (an empty line), then the next one's size.
            while line.trim().is_empty() {
                line.clear();
                if self.inner.read_line(&mut line)? == 0 { self.done = true; return Ok(0) }
            }
            let size = line.trim().split(';').next().unwrap_or("");
            self.left = usize::from_str_radix(size.trim(), 16).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk"))?;
            if self.left == 0 { self.done = true; return Ok(0) }
        }
        let n = buf.len().min(self.left);
        let got = self.inner.read(&mut buf[..n])?;
        if got == 0 { return Err(io::ErrorKind::UnexpectedEof.into()) }
        self.left -= got;
        Ok(got)
    }
}

/// Server-Sent Events, line by line: `data:` lines are joined, a blank line hands the message out.
#[derive(Default)]
pub struct Sse { data: Vec<String> }

impl Sse {
    pub fn line(&mut self, line: &str) -> Option<String> {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            if self.data.is_empty() { return None }
            return Some(std::mem::take(&mut self.data).join("\n"));
        }
        if let Some(d) = line.strip_prefix("data:") { self.data.push(d.strip_prefix(' ').unwrap_or(d).to_string()) }
        None
    }
}

/// `%`-encodes a query value.
pub fn encode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A server for one connection: reads the request, answers `reply`, and hands back what was asked.
    fn serve_once(reply: &'static [u8]) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = l.local_addr().unwrap().to_string();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut asked = String::new();
            let mut len = 0;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") { len = v.trim().parse().unwrap() }
                asked += &line;
                if line == "\r\n" { break }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            asked += &String::from_utf8_lossy(&body);
            s.write_all(reply).unwrap();
            asked
        });
        (host, h)
    }

    #[test]
    fn a_post_sends_json_and_keeps_the_cookie() {
        let (host, h) = serve_once(b"HTTP/1.1 200 OK\r\nSet-Cookie: rp_session=tok123; HttpOnly; SameSite=Strict; Path=/\r\nContent-Length: 11\r\n\r\n{\"ok\":true}");
        let c = Client::new(&host);
        let r = c.post_json("/remote/pair", &serde_json::json!({ "pin": "123456" })).unwrap();
        assert_eq!((r.status, r.json()["ok"].as_bool()), (200, Some(true)));
        assert_eq!(c.cookie("rp_session").as_deref(), Some("tok123"));
        let asked = h.join().unwrap();
        assert!(asked.starts_with("POST /remote/pair HTTP/1.1\r\n"), "{asked}");
        assert!(asked.contains("Content-Type: application/json") && asked.ends_with("{\"pin\":\"123456\"}"), "{asked}");
        assert!(!asked.contains("Origin:"), "no Origin: the server's same-page check lets an app through");
    }

    #[test]
    fn cookies_go_with_every_request() {
        let (host, h) = serve_once(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n");
        let c = Client::new(&host);
        c.set_cookie("rp_session", Some("a"));
        c.set_cookie("rp_stage", Some("b"));
        let r = c.get("/remote/status").unwrap();
        assert_eq!(r.body, b"hello world", "a chunked body");
        assert!(h.join().unwrap().contains("Cookie: rp_session=a; rp_stage=b\r\n"));
        c.set_cookie("rp_stage", None);
        assert_eq!(c.cookie_header().as_deref(), Some("rp_session=a"));
    }

    #[test]
    fn server_sent_events_come_one_message_at_a_time() {
        let (host, _h) = serve_once(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n12\r\ndata: {\"a\":1}\n\n:\n\n\r\nF\r\ndata: {\"a\":2}\n\n\r\n0\r\n\r\n");
        let c = Client::new(&host);
        let mut got = vec![];
        let end = c.events("/remote/events", Duration::from_secs(2), &|| true, |d| got.push(d.to_string()));
        assert_eq!(got, ["{\"a\":1}", "{\"a\":2}"], "the keep-alive comment is skipped");
        assert!(matches!(end, Err(StreamEnd::Closed)));
    }

    #[test]
    fn a_refused_stream_says_why() {
        let (host, _h) = serve_once(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 10\r\n\r\npair first");
        let end = Client::new(&host).events("/remote/events", Duration::from_secs(2), &|| true, |_| {});
        assert!(matches!(end, Err(StreamEnd::Status(401))));
    }

    /// A stream that goes quiet ends after `quiet`, and one the app is done with ends when asked.
    #[test]
    fn a_quiet_stream_ends_and_so_does_one_no_longer_wanted() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = l.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            for s in l.incoming().take(2) {
                let mut s = s.unwrap();
                std::thread::spawn(move || {
                    let mut b = [0u8; 1024];
                    let _ = s.read(&mut b);
                    s.write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n9\r\ndata: 1\n\n\r\n").unwrap();
                    std::thread::sleep(Duration::from_secs(3));
                });
            }
        });
        let c = Client::new(&host);
        let t = std::time::Instant::now();
        let mut n = 0;
        let end = c.events("/x", Duration::from_millis(800), &|| true, |_| n += 1);
        assert!(matches!(end, Err(StreamEnd::Closed)) && n == 1, "{end:?}");
        assert!(t.elapsed() < Duration::from_millis(2500));
        let t = std::time::Instant::now();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let end = c.events("/x", Duration::from_secs(10), &|| !stop.load(std::sync::atomic::Ordering::Relaxed), |_| stop.store(true, std::sync::atomic::Ordering::Relaxed));
        assert!(end.is_ok() && t.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn sse_joins_data_lines() {
        let mut s = Sse::default();
        assert_eq!(s.line("data: one"), None);
        assert_eq!(s.line("data:two"), None);
        assert_eq!(s.line("\r").as_deref(), Some("one\ntwo"));
        assert_eq!(s.line(""), None);
        assert_eq!(s.line(": keep-alive"), None);
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(encode("Pastor's phone"), "Pastor%27s%20phone");
        assert_eq!(encode("Balcony-2"), "Balcony-2");
    }
}
