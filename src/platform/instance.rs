//! One RustyPresenter at a time: the port is the lock. A second launch asks the running one to bring
//! its operator window forward, and a port held by some other program is said plainly.
use crate::recovery::strings;
use std::{io::{Read, Write}, net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream}, time::{Duration, Instant}};

/// A restored app may start while the crashed one still lets go of the port: about 1.5 s in all.
const RESTORE_TRIES: u32 = 15;
const RESTORE_WAIT: Duration = Duration::from_millis(100);
/// How long a running RustyPresenter has to answer. A program that never answers is not one.
const CONNECT_WAIT: Duration = Duration::from_secs(1);
const ANSWER_WAIT: Duration = Duration::from_secs(2);
/// Whether it is RustyPresenter at all: the server answers this at once, whatever its windows are doing.
const STATUS_WAIT: Duration = Duration::from_secs(1);
/// A RustyPresenter still starting, busy or quitting cannot bring its operator forward yet: how long a second
/// launch waits for it to start, or to let go of the port.
const BUSY_WAIT: Duration = Duration::from_secs(3);
const BUSY_RETRY: Duration = Duration::from_millis(250);

pub enum Bind {
    Ok(TcpListener),
    /// RustyPresenter is already running, and has been asked to bring its operator window forward.
    Running,
    /// Another program holds the port.
    Taken,
}

/// What answered on the port.
#[derive(PartialEq)]
enum Answer {
    /// RustyPresenter, and its operator window came forward.
    Focused,
    /// RustyPresenter, but it could not bring its operator forward (still starting, busy or quitting).
    Ours,
    /// Some other program, or nothing.
    Other,
}

/// Takes the port on every interface (phones connect over the network). On `--restore` it is retried for
/// a while, as the crashed app may not have let go of it yet.
pub fn bind(port: u16, restore: bool) -> Bind {
    let tries = if restore { RESTORE_TRIES } else { 1 };
    for i in 0..tries {
        if let Ok(l) = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)) { return Bind::Ok(l) }
        if i + 1 < tries { std::thread::sleep(RESTORE_WAIT) }
    }
    let end = Instant::now() + BUSY_WAIT;
    loop {
        let answer = ask(port);
        // A RustyPresenter that was quitting may have let go of the port meanwhile: this one starts instead.
        if let Ok(l) = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)) { return Bind::Ok(l) }
        match answer {
            Answer::Focused => return Bind::Running,
            Answer::Other => return Bind::Taken,
            Answer::Ours if Instant::now() >= end => return Bind::Running,
            Answer::Ours => std::thread::sleep(BUSY_RETRY),
        }
    }
}

/// RustyPresenter is already running on `port`: ask it to bring its operator window forward. Any answer from
/// RustyPresenter counts, even one that could not do it yet; anything else means another program holds the port.
pub fn focus_running(port: u16) -> Result<(), String> {
    match ask(port) {
        Answer::Focused | Answer::Ours => Ok(()),
        Answer::Other => Err(strings::port_taken(port)),
    }
}

fn ask(port: u16) -> Answer {
    match request(port, "POST", "/api/do/display/open", r#"{"role":"operator","monitor":null}"#, ANSWER_WAIT) {
        Some((status, _)) if (200..300).contains(&status) => Answer::Focused,
        // The phone page's status is served by every RustyPresenter, starting or not.
        _ => match request(port, "GET", "/remote/status", "", STATUS_WAIT) {
            Some((200, body)) if body.contains(r#""paired""#) => Answer::Ours,
            _ => Answer::Other,
        },
    }
}

/// One loopback request: the status code and the body, or `None` when nothing (HTTP) answered in time.
fn request(port: u16, method: &str, path: &str, body: &str, wait: Duration) -> Option<(u16, String)> {
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nx-rp: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut s = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), CONNECT_WAIT).ok()?;
    s.set_read_timeout(Some(wait)).ok()?;
    s.set_write_timeout(Some(wait)).ok()?;
    s.write_all(req.as_bytes()).ok()?;
    // Small answers, and `Connection: close` ends them; a body cut short by the timeout is still looked at.
    let (mut seen, mut buf, end) = (Vec::new(), [0u8; 1024], Instant::now() + wait);
    while seen.len() < 64 * 1024 && Instant::now() < end {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => seen.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&seen);
    let rest = text.strip_prefix("HTTP/1.1 ").or_else(|| text.strip_prefix("HTTP/1.0 "))?;
    let status = rest.get(..3)?.parse().ok()?;
    Some((status, text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// A port nothing listens on, held on all interfaces as the app holds it.
    fn held() -> (TcpListener, u16) {
        let l = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    /// A running RustyPresenter: answers one request with `status` and records it.
    fn answering(l: TcpListener, status: &'static str) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut req = vec![0u8; 4096];
            let n = s.read(&mut req).unwrap_or(0);
            let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}");
            String::from_utf8_lossy(&req[..n]).into_owned()
        })
    }

    #[test]
    fn free_port_is_bound() {
        let port = { let (_l, p) = held(); p };
        let Bind::Ok(l) = bind(port, false) else { panic!("not bound") };
        assert_eq!(l.local_addr().unwrap().port(), port);
    }

    #[test]
    fn running_app_is_asked_to_bring_its_operator_forward() {
        let (l, port) = held();
        let seen = answering(l.try_clone().unwrap(), "200 OK");
        assert!(matches!(bind(port, false), Bind::Running));
        let req = seen.join().unwrap();
        assert!(req.starts_with("POST /api/do/display/open HTTP/1.1\r\n"), "{req}");
        assert!(req.contains("x-rp: 1") && req.ends_with(r#"{"role":"operator","monitor":null}"#), "{req}");
    }

    #[test]
    fn a_program_that_never_answers_holds_the_port() {
        let (_l, port) = held();
        let began = Instant::now();
        assert!(matches!(bind(port, false), Bind::Taken));
        assert!(began.elapsed() < Duration::from_secs(4), "{:?}", began.elapsed());
        assert_eq!(focus_running(port), Err(format!("Port {port} is in use by another program, so RustyPresenter cannot start.")));
    }

    #[test]
    fn a_program_that_refuses_holds_the_port() {
        let (l, port) = held();
        let _seen = answering(l.try_clone().unwrap(), "404 Not Found");
        assert!(matches!(bind(port, false), Bind::Taken));
    }

    /// A RustyPresenter that cannot bring its operator forward (still starting, busy or quitting): it answers
    /// `display/open` with its error and the phone page's status as usual. Serves `n` requests, then lets go.
    fn starting(l: TcpListener, n: usize) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            for _ in 0..n {
                let Ok((mut s, _)) = l.accept() else { return };
                s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut req = vec![0u8; 4096];
                let n = s.read(&mut req).unwrap_or(0);
                let req = String::from_utf8_lossy(&req[..n]).into_owned();
                let (status, body) = if req.starts_with("GET /remote/status ") { ("200 OK", r#"{"paired":false}"#) }
                    else { ("400 Bad Request", r#"{"error":"RustyPresenter is still starting. Try again in a moment."}"#) };
                let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        })
    }

    #[test]
    fn an_app_still_starting_is_running_not_another_program() {
        // A volunteer double-clicks the icon twice: the second launch must not say another program has the port.
        let (l, port) = held();
        let _server = starting(l, usize::MAX);
        let began = Instant::now();
        assert!(matches!(bind(port, false), Bind::Running));
        assert!(began.elapsed() < Duration::from_secs(8), "{:?}", began.elapsed());
        assert_eq!(focus_running(port), Ok(()));
    }

    #[test]
    fn an_app_quitting_meanwhile_lets_the_new_one_start() {
        // Relaunched just after closing: the old app answers once more, then lets go of the port.
        let (l, port) = held();
        let server = starting(l, 2);
        let Bind::Ok(l) = bind(port, false) else { panic!("not bound after the old app let go") };
        assert_eq!(l.local_addr().unwrap().port(), port);
        server.join().unwrap();
    }

    #[test]
    fn restore_waits_for_the_dying_app_to_let_go() {
        let (l, port) = held();
        let release = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(500)); drop(l) });
        let began = Instant::now();
        let Bind::Ok(_) = bind(port, true) else { panic!("not bound after the old app let go") };
        assert!(began.elapsed() >= Duration::from_millis(400), "{:?}", began.elapsed());
        release.join().unwrap();
    }

    #[test]
    fn restore_gives_up_waiting_after_a_second_and_a_half() {
        let (l, port) = held();
        let _seen = answering(l.try_clone().unwrap(), "200 OK");
        let began = Instant::now();
        assert!(matches!(bind(port, true), Bind::Running));
        let waited = began.elapsed();
        assert!(waited >= Duration::from_millis(1300) && waited < Duration::from_secs(4), "{waited:?}");
    }
}
