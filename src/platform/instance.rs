//! One RustyPresenter at a time: the port is the lock. A second launch asks the running one to bring
//! its operator window forward, and a port held by some other program is said plainly.
use crate::recovery::strings;
use std::{io::{Read, Write}, net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream}, time::Duration};

/// A restored app may start while the crashed one still lets go of the port: about 1.5 s in all.
const RESTORE_TRIES: u32 = 15;
const RESTORE_WAIT: Duration = Duration::from_millis(100);
/// How long a running RustyPresenter has to answer. A program that never answers is not one.
const CONNECT_WAIT: Duration = Duration::from_secs(1);
const ANSWER_WAIT: Duration = Duration::from_secs(2);

pub enum Bind {
    Ok(TcpListener),
    /// RustyPresenter is already running, and its operator window has been brought forward.
    Running,
    /// Another program holds the port.
    Taken,
}

/// Takes the port on every interface (phones connect over the network). On `--restore` it is retried for
/// a while, as the crashed app may not have let go of it yet.
pub fn bind(port: u16, restore: bool) -> Bind {
    let tries = if restore { RESTORE_TRIES } else { 1 };
    for i in 0..tries {
        if let Ok(l) = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)) { return Bind::Ok(l) }
        if i + 1 < tries { std::thread::sleep(RESTORE_WAIT) }
    }
    match focus_running(port) {
        Ok(()) => Bind::Running,
        Err(_) => Bind::Taken,
    }
}

/// RustyPresenter is already running on `port`: ask it to bring its operator window forward. Only a
/// success answer counts; anything else means another program holds the port.
pub fn focus_running(port: u16) -> Result<(), String> {
    let body = r#"{"role":"operator","monitor":null}"#;
    let req = format!(
        "POST /api/do/display/open HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nx-rp: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let answer = || -> std::io::Result<String> {
        let mut s = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), CONNECT_WAIT)?;
        s.set_read_timeout(Some(ANSWER_WAIT))?;
        s.set_write_timeout(Some(ANSWER_WAIT))?;
        s.write_all(req.as_bytes())?;
        // The status line is enough.
        let (mut seen, mut buf) = (Vec::new(), [0u8; 256]);
        while !seen.contains(&b'\n') {
            let n = s.read(&mut buf)?;
            if n == 0 { break }
            seen.extend_from_slice(&buf[..n]);
        }
        Ok(String::from_utf8_lossy(&seen).into_owned())
    };
    match answer() {
        Ok(line) if line.starts_with("HTTP/1.1 2") || line.starts_with("HTTP/1.0 2") => Ok(()),
        _ => Err(strings::port_taken(port)),
    }
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
