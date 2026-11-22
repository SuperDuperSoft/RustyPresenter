//! Small files the Bible lists are made from (`secrets.json`, `bibles/online.json`), kept in memory once read. The
//! operator window asks for those lists after every change, and each list used to read and parse both files again:
//! eleven reads for one look. A file is read again only when its size or time is not what it was, or when the app
//! itself has just written it (`forget`).
use std::{collections::HashMap, path::{Path, PathBuf}, sync::Mutex, time::SystemTime};

/// A file's size and time; `None` when there is no such file.
type Stamp = Option<(u64, Option<SystemTime>)>;

/// One kind of file, parsed, by its path.
pub(crate) struct Kept<T> { read: Mutex<Option<HashMap<PathBuf, (Stamp, T)>>> }

impl<T: Clone> Kept<T> {
    pub(crate) const fn new() -> Self { Self { read: Mutex::new(None) } }

    /// What `parse` makes of the file's bytes (`None`: no file), from memory when the file is as it was.
    pub(crate) fn get(&self, path: &Path, parse: impl FnOnce(Option<Vec<u8>>) -> T) -> T {
        let stamp: Stamp = std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()));
        let mut read = self.read.lock().unwrap_or_else(|e| e.into_inner());
        let read = read.get_or_insert_with(HashMap::new);
        if let Some((at, value)) = read.get(path) && *at == stamp { return value.clone() }
        let value = parse(std::fs::read(path).ok());
        read.insert(path.to_path_buf(), (stamp, value.clone()));
        value
    }

    /// The app wrote the file: the next `get` reads it again, whatever its size and time say (two writes can land
    /// within one tick of a coarse file clock).
    pub(crate) fn forget(&self, path: &Path) {
        if let Some(read) = self.read.lock().unwrap_or_else(|e| e.into_inner()).as_mut() { read.remove(path); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_parsed_once_and_again_when_it_changes() {
        let dir = std::env::temp_dir().join(format!("rp-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("list.json");
        static KEPT: Kept<String> = Kept::new();
        let parsed = std::cell::Cell::new(0);
        let get = || KEPT.get(&path, |bytes| { parsed.set(parsed.get() + 1); bytes.map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default() });
        assert_eq!((get(), get(), parsed.get()), (String::new(), String::new(), 1), "no file: asked once");
        std::fs::write(&path, "one").unwrap();
        assert_eq!((get(), get(), get(), parsed.get()), ("one".into(), "one".into(), "one".into(), 2));
        std::fs::write(&path, "three").unwrap();
        assert_eq!((get(), parsed.get()), ("three".into(), 3), "another size: read again");
        // Written by the app itself: read again even if the size and time look the same.
        KEPT.forget(&path);
        assert_eq!((get(), parsed.get()), ("three".into(), 4));
        std::fs::remove_file(&path).unwrap();
        assert_eq!((get(), parsed.get()), (String::new(), 5), "gone");
    }
}
