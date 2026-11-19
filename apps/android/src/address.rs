//! The computer's address as a volunteer types it, or as the QR code on the computer carries it
//! (`http://192.168.1.20:4318/remote#pair=<one-time secret>`, made by the desktop's `pair_link`).

/// The phone server's usual port (`startup::port`).
pub const PORT: u16 = 4318;

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// `192.168.1.20:4318`.
    pub host: String,
    /// The QR code's one-time secret, which pairs without typing the code.
    pub secret: Option<String>,
}

/// `192.168.1.20`, `192.168.1.20:4318`, `http://church-pc:4318/remote` or the QR code's link → where to connect.
/// The secure pages' port (4320, https) is turned back into the plain one beside it.
pub fn parse(input: &str) -> Option<Target> {
    let s = input.trim();
    let (https, rest) = if let Some(r) = strip_ci(s, "https://") { (true, r) } else { (false, strip_ci(s, "http://").unwrap_or(s)) };
    let (before, fragment) = rest.split_once('#').map_or((rest, ""), |(a, b)| (a, b));
    let hostport = before.split(['/', '?']).next().unwrap_or("");
    let secret = fragment.strip_prefix("pair=").filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).map(str::to_string);
    let (name, port) = split_port(hostport)?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']')) { return None }
    let port = match port { Some(p) if https && p == PORT + 2 => PORT, Some(p) => p, None => PORT };
    Some(Target { host: format!("{name}:{port}"), secret })
}

fn strip_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    (s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)).then(|| &s[prefix.len()..])
}

/// `name:port` → (name, port); `[fe80::1]:4318` keeps its brackets.
fn split_port(s: &str) -> Option<(&str, Option<u16>)> {
    if s.starts_with('[') {
        let end = s.find(']')?;
        let rest = &s[end + 1..];
        return Some((&s[..=end], if rest.is_empty() { None } else { Some(rest.strip_prefix(':')?.parse().ok()?) }));
    }
    match s.rsplit_once(':') {
        Some((n, p)) => Some((n, Some(p.parse().ok()?))),
        None => Some((s, None)),
    }
}

/// The address to show a person: the port left out when it is the usual one.
pub fn shown(host: &str) -> String { host.strip_suffix(&format!(":{PORT}")).unwrap_or(host).to_string() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_addresses() {
        assert_eq!(parse("192.168.1.20"), Some(Target { host: "192.168.1.20:4318".into(), secret: None }));
        assert_eq!(parse(" 192.168.1.20:5000 ").unwrap().host, "192.168.1.20:5000");
        assert_eq!(parse("http://192.168.1.20:4318/remote").unwrap().host, "192.168.1.20:4318");
        assert_eq!(parse("HTTP://church-pc/remote").unwrap().host, "church-pc:4318");
        assert_eq!(parse("https://192.168.1.20:4320/camera").unwrap().host, "192.168.1.20:4318", "the secure page's port");
        assert_eq!(parse("[fe80::1]:4318").unwrap().host, "[fe80::1]:4318");
        assert_eq!(parse(""), None);
        assert_eq!(parse("192.168.1.20:port"), None);
        assert_eq!(parse("a b"), None);
    }

    #[test]
    fn the_qr_codes_link_carries_the_secret() {
        let t = parse("http://192.168.1.20:4318/remote#pair=0f3a9c2b7d1e4f5a6b7c8d9e0f1a2b3c").unwrap();
        assert_eq!(t, Target { host: "192.168.1.20:4318".into(), secret: Some("0f3a9c2b7d1e4f5a6b7c8d9e0f1a2b3c".into()) });
        assert_eq!(parse("http://192.168.1.20:4318/remote#pair=").unwrap().secret, None);
        assert_eq!(parse("http://192.168.1.20:4318/remote#pair=<script>").unwrap().secret, None);
    }

    #[test]
    fn the_usual_port_is_not_shown() {
        assert_eq!(shown("192.168.1.20:4318"), "192.168.1.20");
        assert_eq!(shown("192.168.1.20:5000"), "192.168.1.20:5000");
    }
}
