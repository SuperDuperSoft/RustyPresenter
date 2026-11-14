//! Where stream keys are kept (Jake, November 14, 2026: Windows' own password store, not `data.json`). A key is
//! a password, so it is never written with the church's data: not to `data.json`, a backup or a service bundle
//! (`Destination::key` is not saved). On Windows it is a "generic credential" of the signed-in user in
//! Credential Manager, named after the destination's id; Windows encrypts it for that user. Each computer has
//! its keys typed in once.
//!
//! macOS and Linux have no store here yet (`system` gives one that forgets): keys last until the app closes.
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// A place that keeps stream keys by destination id.
pub trait KeyStore: Send + Sync {
    fn get(&self, id: Uuid) -> Option<String>;
    /// Keeps `key` for `id`; an empty key forgets it.
    fn set(&self, id: Uuid, key: &str) -> Result<(), String>;
    fn remove(&self, id: Uuid);
}

/// Keys in memory only: tests, and computers with no store yet.
#[derive(Default)]
pub struct Memory(Mutex<HashMap<Uuid, String>>);

impl KeyStore for Memory {
    fn get(&self, id: Uuid) -> Option<String> { self.0.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned() }
    fn set(&self, id: Uuid, key: &str) -> Result<(), String> {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if key.trim().is_empty() { m.remove(&id); } else { m.insert(id, key.trim().to_string()); }
        Ok(())
    }
    fn remove(&self, id: Uuid) { self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(&id); }
}

/// The name a destination's key has in the store.
pub fn target(id: Uuid) -> String { format!("RustyPresenter stream key {id}") }

/// This computer's store: Credential Manager on Windows, else one that forgets when the app closes.
pub fn system() -> Box<dyn KeyStore> {
    #[cfg(windows)]
    return Box::new(windows::Credentials);
    #[cfg(not(windows))]
    Box::new(Memory::default())
}

/// Each destination's key from the store, put where `Destination::address` reads it.
pub fn fill(store: &dyn KeyStore, all: &mut [super::Destination]) {
    for d in all { d.key = store.get(d.id).unwrap_or_default() }
}

#[cfg(windows)]
mod windows {
    use super::{KeyStore, target};
    use uuid::Uuid;
    use windows_sys::Win32::Security::Credentials::{CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW};

    /// Credential Manager's generic credentials of the signed-in user.
    pub struct Credentials;

    fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() }

    impl KeyStore for Credentials {
        fn get(&self, id: Uuid) -> Option<String> {
            let name = wide(&target(id));
            let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
            // SAFETY: `name` ends in a zero and outlives the call; `cred` is set by Windows on success and
            // freed with `CredFree` below, after its bytes are copied out.
            unsafe {
                if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) == 0 || cred.is_null() { return None }
                let c = &*cred;
                let bytes = if c.CredentialBlob.is_null() { &[][..] } else { std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize) };
                let key = String::from_utf8(bytes.to_vec()).ok();
                CredFree(cred as *const core::ffi::c_void);
                key.filter(|k| !k.is_empty())
            }
        }

        fn set(&self, id: Uuid, key: &str) -> Result<(), String> {
            let key = key.trim();
            if key.is_empty() { self.remove(id); return Ok(()) }
            let (mut name, mut user) = (wide(&target(id)), wide("RustyPresenter"));
            let mut blob = key.as_bytes().to_vec();
            // SAFETY: every pointer is to a buffer that outlives the call; Windows copies what it keeps. The
            // other fields of the zeroed struct are "none".
            let ok = unsafe {
                let mut c: CREDENTIALW = std::mem::zeroed();
                c.Type = CRED_TYPE_GENERIC;
                c.TargetName = name.as_mut_ptr();
                c.UserName = user.as_mut_ptr();
                c.CredentialBlob = blob.as_mut_ptr();
                c.CredentialBlobSize = blob.len() as u32;
                c.Persist = CRED_PERSIST_LOCAL_MACHINE;
                CredWriteW(&c, 0)
            };
            // The copy of the key made for the call does not stay in memory.
            blob.iter_mut().for_each(|b| *b = 0);
            if ok == 0 { Err(std::io::Error::last_os_error().to_string()) } else { Ok(()) }
        }

        fn remove(&self, id: Uuid) {
            let name = wide(&target(id));
            // SAFETY: `name` ends in a zero and outlives the call. A credential that is not there is no error here.
            unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{Destination, Service};

    #[test]
    fn a_store_keeps_changes_and_forgets_keys() {
        let store = Memory::default();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(store.get(a), None);
        store.set(a, " key-a ").unwrap();
        store.set(b, "key-b").unwrap();
        assert_eq!((store.get(a).as_deref(), store.get(b).as_deref()), (Some("key-a"), Some("key-b")));
        store.set(a, "  ").unwrap();
        store.remove(b);
        assert_eq!((store.get(a), store.get(b)), (None, None));
    }

    #[test]
    fn keys_come_from_the_store_and_are_never_saved_with_the_data() {
        let store = Memory::default();
        let mut all = vec![Destination { id: Uuid::new_v4(), service: Service::YouTube, on: true, ..Destination::default() },
            Destination { id: Uuid::new_v4(), service: Service::Twitch, on: true, ..Destination::default() }];
        store.set(all[0].id, "SECRET-1").unwrap();
        fill(&store, &mut all);
        assert_eq!(all[0].address().as_deref(), Ok("rtmp://a.rtmp.youtube.com/live2/SECRET-1"));
        assert_eq!(all[1].key, "", "no key kept for it");
        // What is written with the church's data has no key in it, and reading it back gives none.
        let json = serde_json::to_string(&all).unwrap();
        assert!(!json.contains("SECRET") && !json.contains("\"key\""), "{json}");
        let back: Vec<Destination> = serde_json::from_str(&json).unwrap();
        assert_eq!((back[0].id, back[0].key.as_str()), (all[0].id, ""));
        // An old or hand-made file with a key in it: the key is not taken from there.
        let d: Destination = serde_json::from_str(r#"{"service":"twitch","key":"FROM-FILE"}"#).unwrap();
        assert_eq!(d.key, "");
    }

    /// The real store on this computer: a key goes in, comes back, and is taken out again.
    #[cfg(windows)]
    #[test]
    fn windows_credential_manager_keeps_a_key() {
        let store = system();
        let id = Uuid::new_v4();
        assert_eq!(store.get(id), None);
        store.set(id, "test-key-ünï").unwrap();
        assert_eq!(store.get(id).as_deref(), Some("test-key-ünï"));
        store.set(id, "changed").unwrap();
        assert_eq!(store.get(id).as_deref(), Some("changed"));
        store.remove(id);
        assert_eq!(store.get(id), None, "nothing of the test is left in Credential Manager");
        assert!(target(id).starts_with("RustyPresenter stream key "));
    }
}
