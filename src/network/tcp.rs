//! N31: a plain-text TCP control for other gear (a lighting desk, a Stream Deck, a script). One command a line,
//! one answer a line: `OK …` or `ERR …`. Off by default; the port and an optional password are in Settings →
//! Network. The protocol is written up in `docs/tcp-control.md`.
use super::{password, strings as s};
use crate::{app::App, live::{Command, Envelope}};
use std::sync::{Arc, Weak, atomic::{AtomicUsize, Ordering}};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The longest command line, and the most connections at once.
pub const MAX_LINE: usize = 1024;
pub const MAX_CONNECTIONS: usize = 8;
/// Wrong passwords before the connection is closed.
const MAX_WRONG: u32 = 3;

pub const HELP: &str = "OK commands: next, prev, clear [all|slide|media|audio], logo, go <item> [<slide>], status, ping, password <password>, quit";

/// The listener while it runs; dropping it stops the listener and closes every connection.
pub struct Running { pub port: u16, stop: CancellationToken }

impl Drop for Running {
    fn drop(&mut self) { self.stop.cancel() }
}

/// The port the TCP control listens on now, if it does.
pub fn running_port(app: &App) -> Option<u16> { app.net.tcp.lock().unwrap().as_ref().map(|r| r.port) }

/// Start, stop or move the listener to match the settings. Needs a tokio runtime. An error (the port is in use)
/// leaves it stopped.
pub fn sync(app: &Arc<App>) -> Result<(), String> {
    let n = app.data.lock().unwrap().settings.network.clone();
    let want = n.tcp.then_some(n.tcp_port);
    let mut cur = app.net.tcp.lock().unwrap();
    if cur.as_ref().map(|r| r.port) == want { return Ok(()) }
    *cur = None;
    let Some(port) = want else { return Ok(()) };
    let fail = |e: std::io::Error| s::tcp_failed(port, &e.to_string());
    let std = std::net::TcpListener::bind(("0.0.0.0", port)).map_err(fail)?;
    std.set_nonblocking(true).map_err(fail)?;
    let listener = tokio::net::TcpListener::from_std(std).map_err(fail)?;
    let stop = CancellationToken::new();
    tokio::spawn(accept(Arc::downgrade(app), listener, stop.clone()));
    *cur = Some(Running { port, stop });
    Ok(())
}

async fn accept(app: Weak<App>, listener: tokio::net::TcpListener, stop: CancellationToken) {
    let open = Arc::new(AtomicUsize::new(0));
    loop {
        let conn = tokio::select! { _ = stop.cancelled() => return, c = listener.accept() => c };
        let Ok((stream, _)) = conn else { continue };
        let Some(app) = app.upgrade() else { return };
        if open.load(Ordering::Relaxed) >= MAX_CONNECTIONS { drop(stream); continue }
        open.fetch_add(1, Ordering::Relaxed);
        let (open, stop) = (open.clone(), stop.clone());
        tokio::spawn(async move {
            tokio::select! { _ = stop.cancelled() => {}, _ = serve(Arc::downgrade(&app), stream) => {} }
            open.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

async fn serve(app: Weak<App>, stream: tokio::net::TcpStream) {
    let (r, mut w) = stream.into_split();
    let mut r = BufReader::new(r);
    let mut session = Session::default();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let Ok(n) = (&mut r).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf).await else { return };
        if n == 0 { return }
        let reply = if buf.len() > MAX_LINE && !buf.ends_with(b"\n") {
            Reply { text: "ERR line too long".into(), close: true }
        } else {
            let Some(app) = app.upgrade() else { return };
            answer(&app, String::from_utf8_lossy(&buf).trim(), &mut session)
        };
        if reply.text.starts_with("ERR wrong password") { tokio::time::sleep(std::time::Duration::from_secs(1)).await }
        if w.write_all(format!("{}\r\n", reply.text).as_bytes()).await.is_err() || reply.close { return }
    }
}

/// One connection's state: whether its password was given, and how many wrong ones.
#[derive(Default, Debug)]
pub struct Session { authed: bool, wrong: u32 }

#[derive(Debug, PartialEq)]
pub struct Reply { pub text: String, pub close: bool }

fn reply(text: impl Into<String>) -> Reply { Reply { text: text.into(), close: false } }

/// The answer to one line. Words are not case-sensitive; an empty line gets no command run.
pub fn answer(app: &App, line: &str, session: &mut Session) -> Reply {
    let mut words = line.split_whitespace();
    let verb = words.next().unwrap_or("").to_ascii_lowercase();
    let args: Vec<&str> = words.collect();
    let needed = app.data.lock().unwrap().settings.network.tcp_password.clone();
    match verb.as_str() {
        "" => return reply("ERR empty line"),
        "help" | "?" => return reply(HELP),
        "ping" => return reply("OK pong"),
        "quit" | "exit" | "bye" => return Reply { text: "OK bye".into(), close: true },
        "password" | "pass" => {
            let typed = line.split_once(char::is_whitespace).map_or("", |(_, p)| p.trim());
            return match &needed {
                None => reply("OK no password needed"),
                Some(h) if password::verify(typed, h) => { session.authed = true; reply("OK") }
                Some(_) => {
                    session.wrong += 1;
                    Reply { text: "ERR wrong password".into(), close: session.wrong >= MAX_WRONG }
                }
            };
        }
        _ => {}
    }
    if needed.is_some() && !session.authed { return reply("ERR password needed: send password <password> first") }
    let cmd = match (verb.as_str(), args.first().map(|a| a.to_ascii_lowercase()).as_deref()) {
        ("next", _) => Command::Next,
        ("prev" | "previous" | "back", _) => Command::Prev,
        ("clear", None | Some("all")) => Command::ClearAll,
        ("clear", Some("slide" | "text")) => Command::ClearSlide,
        ("clear", Some("media" | "video")) => Command::ClearMedia,
        ("clear", Some("audio")) => Command::ClearAudio,
        ("clear", Some(_)) => return reply("ERR clear what? all, slide, media or audio"),
        ("logo", _) => Command::Logo,
        ("go" | "goto", _) => match go(app, &args) {
            Ok(cmd) => cmd,
            Err(e) => return reply(format!("ERR {e}")),
        },
        ("status", _) => return reply(status(app)),
        _ => return reply(format!("ERR unknown command {verb:?}; send help")),
    };
    // Gear acts on whatever is on screen now: the newest version is the base.
    let base = Some(app.live.lock().unwrap().version);
    let out = app.command(Envelope { id: format!("tcp-{}", Uuid::new_v4().simple()), base, cmd });
    if out.ok { reply("OK") } else { reply(format!("ERR {}", out.error.unwrap_or_default())) }
}

/// `go <item> [<slide>]`: the item's place in the current service and the slide in it, both from 1.
fn go(app: &App, args: &[&str]) -> Result<Command, String> {
    let num = |i: usize| args.get(i).map(|a| a.parse::<usize>().ok().filter(|n| *n >= 1).ok_or_else(|| format!("{a:?} is not a number from 1")));
    let item = num(0).ok_or("go needs an item number")??;
    let slide = num(1).transpose()?.unwrap_or(1);
    let data = app.data.lock().unwrap();
    let items = data.service().map(|s| s.items.as_slice()).unwrap_or_default();
    let it = items.get(item - 1).ok_or_else(|| format!("there is no item {item} (the service has {})", items.len()))?;
    let count = crate::slides::build_item(&data, it).2.len();
    if slide > count { return Err(format!("item {item} has {count} slides")) }
    Ok(Command::Show { item_id: it.id, slide: slide - 1 })
}

/// `OK live <item> <slide> <slides> <title>`, or `OK nothing live`.
fn status(app: &App) -> String {
    let v = app.live_view();
    let (Some(c), Some(item)) = (&v.cursor, &v.item) else { return "OK nothing live".into() };
    let data = app.data.lock().unwrap();
    let place = data.service().and_then(|s| s.items.iter().position(|i| i.id == c.item_id)).map_or(0, |p| p + 1);
    format!("OK live {place} {} {} {}", c.slide + 1, item.count, item.title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::test_app, model::{ItemKind, ServiceItem}};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    fn with_items(app: &App) {
        let mut d = app.data.lock().unwrap();
        for (t, body) in [("Welcome", "Hello\n\nStand"), ("Notices", "Coffee")] {
            d.services[0].items.push(ServiceItem { id: Uuid::new_v4(), kind: ItemKind::Text { title: t.into(), body: body.into() }, background: None, notes: String::new(), theme: None });
        }
    }

    #[test]
    fn lines_run_commands_and_answer_ok_or_err() {
        let app = test_app();
        with_items(&app);
        let mut s = Session::default();
        let mut say = |l: &str| answer(&app, l, &mut s).text;
        assert_eq!(say("status"), "OK nothing live");
        assert_eq!(say("NEXT"), "OK");
        assert_eq!(say("status"), "OK live 1 1 2 Welcome");
        assert_eq!(say("go 2"), "OK");
        assert_eq!(say("status"), "OK live 2 1 1 Notices");
        assert_eq!(say("go 1 2"), "OK");
        assert_eq!(say("status"), "OK live 1 2 2 Welcome");
        assert_eq!(say("prev"), "OK");
        assert_eq!(say("go 3"), "ERR there is no item 3 (the service has 2)");
        assert_eq!(say("go 1 5"), "ERR item 1 has 2 slides");
        assert_eq!(say("go x"), "ERR \"x\" is not a number from 1");
        assert_eq!(say("go"), "ERR go needs an item number");
        assert_eq!(say("clear slide"), "OK");
        assert!(app.live.lock().unwrap().frame.slide.is_none());
        assert_eq!(say("clear sideways"), "ERR clear what? all, slide, media or audio");
        assert_eq!(say("clear"), "OK");
        assert_eq!(say("dance"), "ERR unknown command \"dance\"; send help");
        assert_eq!(say("help"), HELP);
        assert_eq!(say("ping"), "OK pong");
        assert_eq!(say("password x"), "OK no password needed");
        assert_eq!(answer(&app, "quit", &mut Session::default()), Reply { text: "OK bye".into(), close: true });
    }

    #[test]
    fn a_password_must_come_first_and_three_wrong_ones_close_the_line() {
        let app = test_app();
        with_items(&app);
        app.data.lock().unwrap().settings.network.tcp_password = Some(password::hash("lights"));
        let mut s = Session::default();
        assert_eq!(answer(&app, "next", &mut s).text, "ERR password needed: send password <password> first");
        assert_eq!(answer(&app, "ping", &mut s).text, "OK pong");
        assert_eq!(answer(&app, "password nope", &mut s), Reply { text: "ERR wrong password".into(), close: false });
        assert_eq!(answer(&app, "password lights", &mut s).text, "OK");
        assert_eq!(answer(&app, "next", &mut s).text, "OK");
        let mut s = Session::default();
        answer(&app, "password a", &mut s);
        answer(&app, "password b", &mut s);
        assert!(answer(&app, "password c", &mut s).close);
    }

    #[tokio::test]
    async fn off_by_default_and_listens_only_when_turned_on() {
        let app = test_app();
        assert_eq!(sync(&app), Ok(()));
        assert_eq!(running_port(&app), None, "off by default");
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        { let mut d = app.data.lock().unwrap(); d.settings.network.tcp = true; d.settings.network.tcp_port = port; }
        sync(&app).unwrap();
        assert_eq!(running_port(&app), Some(port));
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (r, mut w) = stream.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();
        w.write_all(b"ping\r\n").await.unwrap();
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("OK pong"));
        w.write_all(&vec![b'a'; MAX_LINE + 10]).await.unwrap();
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("ERR line too long"));
        assert_eq!(lines.next_line().await.unwrap(), None, "closed");
        // Turned off: the port is let go.
        app.data.lock().unwrap().settings.network.tcp = false;
        sync(&app).unwrap();
        assert_eq!(running_port(&app), None);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(std::net::TcpListener::bind(("0.0.0.0", port)).is_ok(), "the port is free again");
    }
}
