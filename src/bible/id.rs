//! Translation ids (Bible library spec §5.1). An id is a stable string, separate from the abbreviation shown on a
//! chip. A bare id (`KJV`, `HCSB`, `NIV`) is an old-style abbreviation: any case means the same Bible. An id with a
//! source prefix (`ebible:spaRV1909`, `yv:111`, `ab:de4e12af7f28f599-02`) is opaque and case-sensitive.

/// Longest prefixed id, in bytes. Its file name, at three bytes each at worst, still fits in a Windows file name.
const MAX_LEN: usize = 80;

/// The one place an id is tidied: spaces go, and only a bare id is uppercased.
pub fn normalise_id(s: &str) -> String {
    let s = s.trim();
    if s.contains(':') { s.to_string() } else { s.to_uppercase() }
}

/// True when both name the same Bible.
pub fn same_id(a: &str, b: &str) -> bool { normalise_id(a) == normalise_id(b) }

/// A well-formed `source:rest` id: a lowercase source, then something for that source to read.
pub fn is_prefixed(id: &str) -> bool {
    id.split_once(':').is_some_and(|(source, rest)| {
        !source.is_empty() && source.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && !rest.is_empty() && !rest.contains(char::is_control) && rest.trim() == rest
    }) && id.len() <= MAX_LEN
}

/// The id as a file name, without the extension: `ebible:spaRV1909` is `ebible~spaRV1909`. Other characters outside
/// `[A-Za-z0-9._-]` become `%XX`, byte by byte, so no two ids share a name except by letter case, and nothing can
/// leave its folder. A bare abbreviation comes out as it went in, so existing files keep their names.
pub fn file_stem(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for b in id.bytes() {
        match b {
            b':' => out.push('~'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The id a file name stands for. `None` for a name `file_stem` would not have written.
pub fn from_file_stem(stem: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(stem.len());
    let mut it = stem.bytes();
    while let Some(b) = it.next() {
        bytes.push(match b {
            b'~' => b':',
            b'%' => {
                let hex = [it.next()?, it.next()?];
                u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?
            }
            b => b,
        });
    }
    let id = String::from_utf8(bytes).ok()?;
    (file_stem(&id) == stem).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_bare_id_is_uppercased() {
        assert_eq!(normalise_id(" hcsb "), "HCSB");
        assert_eq!(normalise_id(" ebible:spaRV1909 "), "ebible:spaRV1909");
        assert_eq!(normalise_id("ab:de4e12af7f28f599-02"), "ab:de4e12af7f28f599-02");
        assert_eq!(normalise_id("yv:111"), "yv:111");
    }

    #[test]
    fn prefixed_ids_differ_by_case_and_bare_ones_do_not() {
        assert!(same_id("kjv", "KJV"));
        assert!(same_id(" ebible:spaRV1909", "ebible:spaRV1909"));
        assert!(!same_id("ebible:spaRV1909", "ebible:sparv1909"));
    }

    #[test]
    fn recognises_a_prefixed_id() {
        for id in ["ebible:spaRV1909", "yv:111", "ab:de4e12af7f28f599-02", "door43:es-419_glt", "sword:KJV"] { assert!(is_prefixed(id), "{id}"); }
        for id in ["KJV", ":x", "ab:", "EBIBLE:x", "../x:y", "a b:c", "ab: x", "ab:x\n"] { assert!(!is_prefixed(id), "{id:?}"); }
        assert!(!is_prefixed(&format!("ab:{}", "x".repeat(MAX_LEN))));
    }

    #[test]
    fn file_names_follow_the_spec() {
        assert_eq!(file_stem("ebible:spaRV1909"), "ebible~spaRV1909");
        assert_eq!(file_stem("ab:de4e12af7f28f599-02"), "ab~de4e12af7f28f599-02");
        assert_eq!(file_stem("yv:111"), "yv~111");
        assert_eq!(file_stem("HCSB"), "HCSB", "existing files keep their names");
        assert_eq!(file_stem("ab:a/b\\c"), "ab~a%2Fb%5Cc");
        assert_eq!(file_stem("ab:ñ"), "ab~%C3%B1");
    }

    #[test]
    fn file_names_cannot_leave_the_folder_or_be_mistaken_for_each_other() {
        for id in ["../../etc/passwd:x", "ab:..\\..\\x", "ab:CON"] {
            assert!(!file_stem(id).contains(['/', '\\', ':']), "{id}");
        }
        assert_ne!(file_stem("ab:x~y"), file_stem("ab:x:y"));
        assert_ne!(file_stem("ab:x%2Fy"), file_stem("ab:x/y"));
    }

    #[test]
    fn file_names_round_trip() {
        for id in ["ebible:spaRV1909", "yv:111", "ab:de4e12af7f28f599-02", "HCSB", "ab:x~y%z", "ab:ñandú/2"] {
            assert_eq!(from_file_stem(&file_stem(id)).as_deref(), Some(id));
        }
    }

    #[test]
    fn names_it_would_not_have_written_are_refused() {
        for stem in ["ab~x%2", "ab~x%ZZ", "ab~%41", "ab~%2f", "ab~%FF", "a b"] { assert_eq!(from_file_stem(stem), None, "{stem}"); }
    }
}
