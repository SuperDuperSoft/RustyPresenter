//! N31: passwords are kept only as salted PBKDF2-HMAC-SHA256 hashes: `pbkdf2-sha256$<rounds>$<salt hex>$<hash hex>`.
use crate::sha256::{Digest, Sha256, hex};

/// Rounds for a new hash: slow enough to make guessing a stolen hash costly, quick enough for one check.
pub const ROUNDS: u32 = 60_000;

/// A new hash of `password` with a random salt.
pub fn hash(password: &str) -> String {
    let salt = uuid::Uuid::new_v4().into_bytes();
    format!("pbkdf2-sha256${ROUNDS}${}${}", hex(&salt), hex(&pbkdf2(password.as_bytes(), &salt, ROUNDS)))
}

/// Whether `password` matches `stored` (a hash from `hash`). A hash that doesn't read never matches.
pub fn verify(password: &str, stored: &str) -> bool {
    let mut parts = stored.split('$');
    let (Some("pbkdf2-sha256"), Some(rounds), Some(salt), Some(want), None) = (parts.next(), parts.next(), parts.next(), parts.next(), parts.next()) else { return false };
    let (Ok(rounds), Some(salt)) = (rounds.parse::<u32>(), unhex(salt)) else { return false };
    if rounds == 0 || rounds > 10_000_000 { return false }
    crate::app::ct_eq(&hex(&pbkdf2(password.as_bytes(), &salt, rounds)), want)
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) { return None }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// HMAC-SHA256 with the key's inner and outer states made once.
struct Hmac { inner: Sha256, outer: Sha256 }

impl Hmac {
    fn new(key: &[u8]) -> Self {
        let mut k = [0u8; 64];
        if key.len() > 64 { k[..32].copy_from_slice(&Sha256::digest(key)) } else { k[..key.len()].copy_from_slice(key) }
        let (mut inner, mut outer) = (Sha256::new(), Sha256::new());
        inner.update(k.map(|b| b ^ 0x36));
        outer.update(k.map(|b| b ^ 0x5c));
        Hmac { inner, outer }
    }

    fn mac(&self, data: &[u8]) -> [u8; 32] {
        let mut i = self.inner.clone();
        i.update(data);
        let mut o = self.outer.clone();
        o.update(i.finalize());
        o.finalize().into()
    }
}

/// PBKDF2-HMAC-SHA256, one 32-byte block.
fn pbkdf2(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 32] {
    let mac = Hmac::new(password);
    let mut u = mac.mac(&[salt, &1u32.to_be_bytes()].concat());
    let mut out = u;
    for _ in 1..rounds {
        u = mac.mac(&u);
        for (o, b) in out.iter_mut().zip(u) { *o ^= b }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_matches_the_rfc_7914_vector() {
        // RFC 7914 §11: P = "passwd", S = "salt", c = 1 (first 32 bytes).
        assert_eq!(hex(&pbkdf2(b"passwd", b"salt", 1)), "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc");
        // RFC 7914 §11: P = "Password", S = "NaCl", c = 80000 (first 32 bytes).
        assert_eq!(hex(&pbkdf2(b"Password", b"NaCl", 80000)), "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56");
    }

    #[test]
    fn a_hash_verifies_only_its_password_and_is_salted() {
        let h = hash("sing loud");
        assert!(h.starts_with("pbkdf2-sha256$60000$") && !h.contains("sing"), "{h}");
        assert!(verify("sing loud", &h));
        assert!(!verify("sing quiet", &h));
        assert!(!verify("", &h));
        assert_ne!(hash("sing loud"), h, "a fresh salt each time");
        for bad in ["", "sing loud", "pbkdf2-sha256$x$00$00", "pbkdf2-sha256$0$00$00", "pbkdf2-sha256$1$0$00", "md5$1$00$00"] { assert!(!verify("sing loud", bad), "{bad}") }
    }
}
