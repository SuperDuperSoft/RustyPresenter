//! SHA-256 for asset names and phone tokens, from the `sha2` crate.
pub use sha2::{Digest, Sha256};

pub fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes { s.push(D[(b >> 4) as usize] as char); s.push(D[(b & 15) as usize] as char); }
    s
}

pub fn digest_hex(data: &[u8]) -> String { hex(&Sha256::digest(data)) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_vectors() {
        assert_eq!(digest_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(digest_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(digest_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
        let mut h = Sha256::new();
        for _ in 0..1000 { h.update([b'a'; 1000]); }
        assert_eq!(hex(&h.finalize()), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }
}
