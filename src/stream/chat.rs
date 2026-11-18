//! Live chat (design §4 part 8): what viewers write on YouTube, Facebook and Twitch while the church is live, in
//! one list in the app. YouTube's chat and the Facebook live video's comments are asked for every few seconds
//! (`next`); Twitch's chat comes as it is written, over its chat connection (`twitch_reader`).
use super::accounts::{Ends, Ids, Live};
use super::Service;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// One message: who wrote it, where, and what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub from: Service,
    pub name: String,
    pub text: String,
}

/// The most messages kept; older ones go.
pub const KEEP: usize = 200;

/// The messages so far (newest last, the newest `KEEP`), and how many ever came.
#[derive(Debug, Default)]
pub struct Chat {
    pub list: Vec<Message>,
    pub total: u64,
}

/// Shared with the readers.
pub type Board = Arc<Mutex<Chat>>;

/// Adds `new` to `board`, keeping the newest `KEEP`.
pub fn add(board: &Board, new: Vec<Message>) {
    if new.is_empty() { return }
    let mut b = board.lock().unwrap_or_else(|e| e.into_inner());
    b.total += new.len() as u64;
    b.list.extend(new);
    let over = b.list.len().saturating_sub(KEEP);
    b.list.drain(..over);
}

/// Where a broadcast's chat was read up to.
#[derive(Debug, Default)]
pub struct Cursor {
    /// YouTube: the page to ask for next.
    page: String,
    /// Facebook: the comments already seen.
    seen: HashSet<String>,
}

/// What was written on `live` since `cursor`. Twitch's comes by its own reader: nothing here.
pub async fn next(client: &reqwest::Client, ends: &Ends, ids: &Ids, live: &Live, cursor: &mut Cursor) -> Result<Vec<Message>, String> {
    let access = live.access_now(client, ends, ids).await?;
    match live.service {
        Service::YouTube => {
            if live.chat.is_empty() { return Ok(vec![]) }
            let mut q = vec![("liveChatId", live.chat.as_str()), ("part", "snippet,authorDetails")];
            if !cursor.page.is_empty() { q.push(("pageToken", cursor.page.as_str())) }
            let v = get(client.get(format!("{}/liveChat/messages", ends.youtube)).query(&q).bearer_auth(&access)).await?;
            if let Some(p) = v["nextPageToken"].as_str() { cursor.page = p.to_string() }
            Ok(v["items"].as_array().into_iter().flatten().filter_map(|m| Some(Message {
                from: Service::YouTube,
                name: m.pointer("/authorDetails/displayName")?.as_str()?.to_string(),
                text: m.pointer("/snippet/displayMessage")?.as_str()?.to_string(),
            })).collect())
        }
        Service::Facebook => {
            let v = get(client.get(format!("{}/{}/comments", ends.graph, live.id))
                .query(&[("fields", "id,from{name},message"), ("order", "reverse_chronological"), ("limit", "50"), ("access_token", access.as_str())])).await?;
            let mut new: Vec<Message> = vec![];
            for c in v["data"].as_array().into_iter().flatten() {
                let Some(id) = c["id"].as_str() else { continue };
                if !cursor.seen.insert(id.to_string()) { continue }
                let name = c.pointer("/from/name").and_then(Value::as_str).unwrap_or("Facebook").to_string();
                if let Some(text) = c["message"].as_str() { new.push(Message { from: Service::Facebook, name, text: text.to_string() }) }
            }
            // Newest first from Facebook: oldest first here.
            new.reverse();
            Ok(new)
        }
        _ => Ok(vec![]),
    }
}

async fn get(r: reqwest::RequestBuilder) -> Result<Value, String> {
    let r = r.send().await.map_err(|e| e.to_string())?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or_default();
    if status.is_success() { Ok(v) } else { Err(v.pointer("/error/message").and_then(Value::as_str).map_or_else(|| status.to_string(), str::to_string)) }
}

/// One line of Twitch chat (IRC): who wrote it and what, when it is a message.
pub fn twitch_line(line: &str) -> Option<Message> {
    // "@display-name=Grace;… :grace!grace@grace.tmi.twitch.tv PRIVMSG #church :Amen!"
    let (tags, rest) = if let Some(r) = line.strip_prefix('@') { r.split_once(' ').map(|(t, r)| (t, r))? } else { ("", line) };
    let rest = rest.strip_prefix(':')?;
    let (prefix, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix("PRIVMSG ")?;
    let (_, text) = rest.split_once(" :")?;
    let login = prefix.split('!').next()?;
    let shown = tags.split(';').find_map(|t| t.strip_prefix("display-name=")).filter(|n| !n.is_empty()).unwrap_or(login);
    Some(Message { from: Service::Twitch, name: shown.to_string(), text: text.trim_end_matches(['\r', '\n']).to_string() })
}

/// Reads the Twitch channel `login`'s chat at `server` ("ircs://host:port", or "irc://" in a test) into `board`
/// until `stop`, signing in with what `access` gives at each connection. Started again after a lost connection.
pub fn twitch_reader(server: String, login: String, access: Arc<dyn Fn() -> Option<String> + Send + Sync>, board: Board, stop: Arc<std::sync::atomic::AtomicBool>) {
    let _ = std::thread::Builder::new().name("twitch-chat".into()).spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        rt.block_on(async {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                match access() {
                    Some(a) => if let Err(e) = read_twitch(&server, &login, &a, &board, &stop).await { eprintln!("twitch chat: {e}") },
                    None => eprintln!("twitch chat: no sign-in"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
    });
}

async fn read_twitch(server: &str, login: &str, access: &str, board: &Board, stop: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (tls, host_port) = if let Some(h) = server.strip_prefix("ircs://") { (true, h) } else { (false, server.strip_prefix("irc://").unwrap_or(server)) };
    let tcp = tokio::net::TcpStream::connect(host_port).await.map_err(|e| e.to_string())?;
    let stream: Box<dyn Stream> = if tls {
        let host = host_port.split(':').next().unwrap_or_default().to_string();
        let _ = rustls::crypto::ring::default_provider().install_default();
        let config = <rustls::ClientConfig as rustls_platform_verifier::ConfigVerifierExt>::with_platform_verifier().map_err(|e| e.to_string())?;
        let name = rustls::pki_types::ServerName::try_from(host).map_err(|e| e.to_string())?;
        Box::new(tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await.map_err(|e| e.to_string())?)
    } else { Box::new(tcp) };
    let (r, mut w) = tokio::io::split(stream);
    let channel = login.to_ascii_lowercase();
    let hello = format!("CAP REQ :twitch.tv/tags\r\nPASS oauth:{access}\r\nNICK {channel}\r\nJOIN #{channel}\r\n");
    w.write_all(hello.as_bytes()).await.map_err(|e| e.to_string())?;
    let mut lines = BufReader::new(r).lines();
    loop {
        if stop.load(std::sync::atomic::Ordering::Relaxed) { return Ok(()) }
        let line = match tokio::time::timeout(std::time::Duration::from_secs(1), lines.next_line()).await {
            Err(_) => continue,
            Ok(Ok(Some(l))) => l,
            Ok(Ok(None)) => return Err("the chat connection closed".into()),
            Ok(Err(e)) => return Err(e.to_string()),
        };
        if let Some(rest) = line.strip_prefix("PING") { w.write_all(format!("PONG{rest}\r\n").as_bytes()).await.map_err(|e| e.to_string())?; continue }
        if let Some(m) = twitch_line(&line) { add(board, vec![m]) }
    }
}

trait Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Stream for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_twitch_chat_line_is_read_with_the_writers_shown_name() {
        let m = twitch_line("@badge-info=;display-name=GraceMember;emotes= :gracemember!gracemember@gracemember.tmi.twitch.tv PRIVMSG #gracechurch :Amen! Praying with you\r");
        assert_eq!(m, Some(Message { from: Service::Twitch, name: "GraceMember".into(), text: "Amen! Praying with you".into() }));
        let m = twitch_line(":bob!bob@bob.tmi.twitch.tv PRIVMSG #gracechurch :hello");
        assert_eq!(m.map(|m| m.name), Some("bob".into()), "no tags: the login");
        assert_eq!(twitch_line(":tmi.twitch.tv 001 gracechurch :Welcome, GLHF!"), None);
        assert_eq!(twitch_line("PING :tmi.twitch.tv"), None);
    }

    #[test]
    fn the_board_keeps_the_newest() {
        let board: Board = Arc::default();
        let m = |i: usize| Message { from: Service::YouTube, name: format!("n{i}"), text: "t".into() };
        add(&board, (0..KEEP + 5).map(m).collect());
        let b = board.lock().unwrap();
        assert_eq!((b.list.len(), b.total), (KEEP, KEEP as u64 + 5));
        assert_eq!(b.list[0].name, "n5");
    }

    #[tokio::test]
    async fn twitch_chat_is_read_as_it_is_written() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let said = Arc::new(Mutex::new(Vec::<String>::new()));
        let s2 = said.clone();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let (r, mut w) = tokio::io::split(sock);
            let mut lines = BufReader::new(r).lines();
            for _ in 0..4 { if let Ok(Some(l)) = lines.next_line().await { s2.lock().unwrap().push(l) } }
            w.write_all(b"PING :tmi.twitch.tv\r\n@display-name=Ann :ann!ann@ann.tmi.twitch.tv PRIVMSG #gracechurch :Good morning!\r\n").await.unwrap();
            if let Ok(Some(l)) = lines.next_line().await { s2.lock().unwrap().push(l) }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        });
        let board: Board = Arc::default();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        twitch_reader(format!("irc://{addr}"), "GraceChurch".into(), Arc::new(|| Some("tok".to_string())), board.clone(), stop.clone());
        let t = std::time::Instant::now();
        while board.lock().unwrap().list.is_empty() && t.elapsed() < std::time::Duration::from_secs(5) { tokio::time::sleep(std::time::Duration::from_millis(50)).await }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(board.lock().unwrap().list.clone(), [Message { from: Service::Twitch, name: "Ann".into(), text: "Good morning!".into() }]);
        let said = said.lock().unwrap().clone();
        assert_eq!(said[..4], ["CAP REQ :twitch.tv/tags", "PASS oauth:tok", "NICK gracechurch", "JOIN #gracechurch"]);
        assert_eq!(said.get(4).map(String::as_str), Some("PONG :tmi.twitch.tv"), "a ping is answered");
    }
}
