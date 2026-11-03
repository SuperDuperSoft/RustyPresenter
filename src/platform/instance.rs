//! One copy at a time: the port is the lock, and a second launch asks the first to come forward.
use std::{io::{Read, Write}, net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream}, time::Duration};

pub enum Bind { Ok(TcpListener), Running, Taken }

/// After a crash the dead app's port can take a moment to come free.
const RESTORE_TRIES: u32 = 15;
const RESTORE_WAIT: Duration = Duration::from_millis(100);
const ANSWER_TIME: Duration = Duration::from_secs(5);
/// A copy that is still starting, or whose windows are busy, cannot come forward, but it is a copy.
const OURS_NOT_READY: [&str; 3] = ["RustyPresenter is still starting", "The screens are not running", "The screens did not answer"];

pub fn taken_message(port: u16) -> String { format!("Port {port} is in use by another program, so RustyPresenter cannot start.") }

/// Takes the port. When it is held, asks whoever holds it to show the operator window: `Running` if
/// that is RustyPresenter, `Taken` if not. The listener is non-blocking, ready for tokio.
pub fn bind(port: u16, restore: bool) -> Bind {
    let tries = if restore { RESTORE_TRIES } else { 1 };
    for attempt in 0..tries {
        if attempt > 0 { std::thread::sleep(RESTORE_WAIT); }
        if let Ok(listener) = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)) {
            let _ = listener.set_nonblocking(true);
            return Bind::Ok(listener);
        }
    }
    if focus_running(port).is_ok() { Bind::Running } else { Bind::Taken }
}

/// RustyPresenter is already running on `port`: asks it to bring its operator window forward.
pub fn focus_running(port: u16) -> Result<(), String> {
    let body = r#"{"role":"operator","monitor":null}"#;
    let req = format!(
        "POST /api/do/display/open HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nx-rp: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let answer = ask(port, &req).map_err(|_| taken_message(port))?;
    let ok = answer.starts_with("HTTP/1.1 2") || OURS_NOT_READY.iter().any(|t| answer.contains(t));
    if ok { Ok(()) } else { Err(taken_message(port)) }
}

/// What the port answers. A program that accepts and never answers runs out the read timeout.
fn ask(port: u16, req: &str) -> std::io::Result<String> {
    let mut s = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), ANSWER_TIME)?;
    s.set_read_timeout(Some(ANSWER_TIME))?;
    s.write_all(req.as_bytes())?;
    let mut answer = Vec::new();
    s.take(64 * 1024).read_to_end(&mut answer)?;
    Ok(String::from_utf8_lossy(&answer).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Instant};

    fn free_port() -> u16 { TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap().local_addr().unwrap().port() }

    /// Takes the port and answers one request with `reply`.
    fn answers(reply: &'static str) -> u16 {
        let l = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut s, _): (TcpStream, _) = l.accept().unwrap();
            let _ = s.read(&mut [0; 2048]);
            let _ = s.write_all(reply.as_bytes());
        });
        port
    }

    #[test]
    fn free_port_binds() { assert!(matches!(bind(free_port(), false), Bind::Ok(_))); }

    #[test]
    fn our_answer_means_running() {
        let port = answers("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}");
        assert!(matches!(bind(port, false), Bind::Running));
    }

    /// The first copy has the port before its windows exist. Double-clicking twice must not say "another program".
    #[test]
    fn a_copy_still_starting_is_running() {
        let port = answers("HTTP/1.1 400 Bad Request\r\n\r\n{\"error\":\"RustyPresenter is still starting. Try again in a moment.\"}");
        assert!(matches!(bind(port, false), Bind::Running));
    }

    #[test]
    fn another_web_server_is_taken() {
        let port = answers("HTTP/1.1 404 Not Found\r\n\r\nnope");
        assert!(matches!(bind(port, false), Bind::Taken));
    }

    #[test]
    fn a_program_that_never_answers_is_taken() {
        let l = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        assert!(matches!(bind(l.local_addr().unwrap().port(), false), Bind::Taken));
        drop(l);
    }

    #[test]
    fn restore_waits_for_the_dead_app_to_let_go() {
        let l = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        thread::spawn(move || { thread::sleep(Duration::from_millis(300)); drop(l); });
        assert!(matches!(bind(port, true), Bind::Ok(_)));
    }

    #[test]
    fn a_normal_start_does_not_wait_for_the_port() {
        let port = answers("HTTP/1.1 200 OK\r\n\r\n{}");
        let t = Instant::now();
        bind(port, false);
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn bound_listener_is_nonblocking() {
        let Bind::Ok(l) = bind(free_port(), false) else { panic!("not bound") };
        assert_eq!(l.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    }
}
