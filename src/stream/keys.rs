//! Where stream keys are kept (Jake, November 14, 2026: Windows' own password store, not `data.json`). A key is
//! a password, so it is never written with the church's data: not to `data.json`, a backup or a service bundle
//! (`Destination::key` is not saved). On Windows it is a "generic credential" of the signed-in user in
//! Credential Manager, named after the destination's id; Windows encrypts it for that user. Each computer has
//! its keys typed in once.
//!
//! macOS: the login Keychain, as a generic password of service "RustyPresenter stream key <id>". Linux: the desktop's
//! password store (GNOME Keyring, KWallet, KeePassXC: whatever offers the Secret Service over D-Bus), unlocked
//! with the user's sign-in. A computer with no such store keeps keys in memory: they last until the app closes.
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

/// Keys in memory only: tests, and computers with no store.
#[derive(Default)]
pub struct Memory(Mutex<HashMap<Uuid, String>>);

/// A store shared by several who use it (the one in memory that lasts until the app closes).
impl<T: KeyStore + ?Sized> KeyStore for std::sync::Arc<T> {
    fn get(&self, id: Uuid) -> Option<String> { (**self).get(id) }
    fn set(&self, id: Uuid, key: &str) -> Result<(), String> { (**self).set(id, key) }
    fn remove(&self, id: Uuid) { (**self).remove(id) }
}

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

/// This computer's store: Credential Manager on Windows, the Keychain on macOS, the desktop's password store on
/// Linux; else one that forgets when the app closes.
pub fn system() -> Box<dyn KeyStore> { platform_store() }

#[cfg(windows)]
fn platform_store() -> Box<dyn KeyStore> { Box::new(windows::Credentials) }

#[cfg(target_os = "macos")]
fn platform_store() -> Box<dyn KeyStore> { Box::new(keychain::Keychain) }

#[cfg(target_os = "linux")]
fn platform_store() -> Box<dyn KeyStore> {
    // Opened once: each use would otherwise open a connection of its own.
    static OPENED: std::sync::OnceLock<Option<secret_service::SecretService>> = std::sync::OnceLock::new();
    let opened = OPENED.get_or_init(|| secret_service::SecretService::open()
        .map_err(|e| eprintln!("stream keys: no password store on this computer, so keys last until the app closes: {e}")).ok());
    match opened {
        Some(s) => Box::new(s.clone()),
        None => Box::new(forgetful()),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn platform_store() -> Box<dyn KeyStore> { Box::new(forgetful()) }

/// The store that keeps keys until the app closes, the same one each time.
#[allow(dead_code)]
fn forgetful() -> std::sync::Arc<Memory> {
    static MEMORY: std::sync::OnceLock<std::sync::Arc<Memory>> = std::sync::OnceLock::new();
    MEMORY.get_or_init(Default::default).clone()
}

/// The attributes a key is found by in the Secret Service (Linux): the app, and the destination's id.
pub fn attributes(id: Uuid) -> [(&'static str, String); 2] { [("application", "RustyPresenter".into()), ("stream-key", id.to_string())] }

/// The store the app uses: this computer's (`system`); in a test, one of the test's own that forgets, so no
/// test ever reads or writes the real one by way of the screens.
pub fn app_store() -> std::sync::Arc<dyn KeyStore> {
    #[cfg(test)]
    return TEST_STORE.with(|s| s.clone());
    #[cfg(not(test))]
    std::sync::Arc::from(system())
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::sync::Arc<Memory> = std::sync::Arc::new(Memory::default());
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

#[cfg(target_os = "macos")]
mod keychain {
    use super::{KeyStore, target};
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
    use uuid::Uuid;

    /// The login Keychain's generic passwords.
    pub struct Keychain;
    const ACCOUNT: &str = "RustyPresenter";

    impl KeyStore for Keychain {
        fn get(&self, id: Uuid) -> Option<String> {
            get_generic_password(&target(id), ACCOUNT).ok().and_then(|b| String::from_utf8(b).ok()).filter(|k| !k.is_empty())
        }
        fn set(&self, id: Uuid, key: &str) -> Result<(), String> {
            let key = key.trim();
            if key.is_empty() { self.remove(id); return Ok(()) }
            set_generic_password(&target(id), ACCOUNT, key.as_bytes()).map_err(|e| e.to_string())
        }
        fn remove(&self, id: Uuid) { let _ = delete_generic_password(&target(id), ACCOUNT); }
    }
}

/// Linux: the desktop's password store, over D-Bus (the freedesktop.org Secret Service). The key goes between the
/// app and the store unencrypted ("plain"), over the user's own session bus, which only that user can reach.
#[cfg(target_os = "linux")]
mod secret_service {
    use super::{KeyStore, attributes, target};
    use std::collections::HashMap;
    use uuid::Uuid;
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    const SERVICE: &str = "org.freedesktop.secrets";
    /// A secret as the store hands it over: (session, parameters, the key, its type).
    type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

    #[derive(Clone)]
    pub struct SecretService { conn: Connection, session: OwnedObjectPath }

    fn found_by(id: Uuid) -> HashMap<String, String> { attributes(id).into_iter().map(|(k, v)| (k.to_string(), v)).collect() }

    impl SecretService {
        /// Connects to the store, if this computer has one.
        pub fn open() -> zbus::Result<SecretService> {
            let conn = Connection::session()?;
            let service = Proxy::new(&conn, SERVICE, "/org/freedesktop/secrets", "org.freedesktop.Secret.Service")?;
            let (_, session): (OwnedValue, OwnedObjectPath) = service.call("OpenSession", &("plain", Value::from("")))?;
            Ok(SecretService { conn, session })
        }

        fn proxy(&self, path: &str, interface: &'static str) -> zbus::Result<Proxy<'static>> {
            Proxy::new(&self.conn, SERVICE, path.to_string(), interface)
        }

        fn service(&self) -> zbus::Result<Proxy<'static>> { self.proxy("/org/freedesktop/secrets", "org.freedesktop.Secret.Service") }

        /// The store's items for `id`: (unlocked, locked).
        fn search(&self, id: Uuid) -> zbus::Result<(Vec<OwnedObjectPath>, Vec<OwnedObjectPath>)> {
            self.service()?.call("SearchItems", &(found_by(id),))
        }

        /// Lets the store ask the user (to unlock it), and waits for the answer. True: done.
        fn prompt(&self, prompt: &OwnedObjectPath) -> zbus::Result<bool> {
            if prompt.as_str() == "/" { return Ok(true) }
            let p = self.proxy(prompt.as_str(), "org.freedesktop.Secret.Prompt")?;
            let mut completed = p.receive_signal("Completed")?;
            let _: () = p.call("Prompt", &("",))?;
            match completed.next() {
                Some(m) => { let (dismissed, _): (bool, OwnedValue) = m.body().deserialize()?; Ok(!dismissed) }
                None => Ok(false),
            }
        }

        fn unlock(&self, items: &[OwnedObjectPath]) -> zbus::Result<bool> {
            let (_, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) = self.service()?.call("Unlock", &(items.to_vec(),))?;
            self.prompt(&prompt)
        }

        fn create(&self, collection: &str, id: Uuid, key: &str) -> zbus::Result<bool> {
            let mut props: HashMap<&str, Value> = HashMap::new();
            props.insert("org.freedesktop.Secret.Item.Label", Value::from(target(id)));
            props.insert("org.freedesktop.Secret.Item.Attributes", Value::from(found_by(id)));
            let secret: Secret = (self.session.clone(), vec![], key.as_bytes().to_vec(), "text/plain".into());
            let (_, prompt): (OwnedObjectPath, OwnedObjectPath) =
                self.proxy(collection, "org.freedesktop.Secret.Collection")?.call("CreateItem", &(props, secret, true))?;
            self.prompt(&prompt)
        }
    }

    impl KeyStore for SecretService {
        fn get(&self, id: Uuid) -> Option<String> {
            let (mut unlocked, locked) = self.search(id).ok()?;
            if unlocked.is_empty() && !locked.is_empty() && self.unlock(&locked).ok()? { unlocked = locked }
            let item = self.proxy(unlocked.first()?.as_str(), "org.freedesktop.Secret.Item").ok()?;
            let secret: Secret = item.call("GetSecret", &(self.session.clone(),)).ok()?;
            String::from_utf8(secret.2).ok().filter(|k| !k.is_empty())
        }

        fn set(&self, id: Uuid, key: &str) -> Result<(), String> {
            let key = key.trim();
            if key.is_empty() { self.remove(id); return Ok(()) }
            // The default collection (normally "login"), else "login" itself.
            let made = self.create("/org/freedesktop/secrets/aliases/default", id, key)
                .or_else(|_| self.create("/org/freedesktop/secrets/collection/login", id, key)).map_err(|e| e.to_string())?;
            if made { Ok(()) } else { Err("the password store was not unlocked".into()) }
        }

        fn remove(&self, id: Uuid) {
            let Ok((unlocked, locked)) = self.search(id) else { return };
            for item in unlocked.iter().chain(&locked) {
                let Ok(p) = self.proxy(item.as_str(), "org.freedesktop.Secret.Item") else { continue };
                if let Ok(prompt) = p.call::<_, _, OwnedObjectPath>("Delete", &()) { let _ = self.prompt(&prompt); }
            }
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

    #[test]
    fn the_memory_store_is_shared_and_linux_finds_keys_by_app_and_id() {
        let id = Uuid::new_v4();
        forgetful().set(id, "kept").unwrap();
        let b: Box<dyn KeyStore> = Box::new(forgetful());
        assert_eq!(b.get(id).as_deref(), Some("kept"), "the same store each time, so a key lasts until the app closes");
        b.remove(id);
        let [(k1, v1), (k2, v2)] = attributes(id);
        assert_eq!((k1, v1.as_str(), k2), ("application", "RustyPresenter", "stream-key"));
        assert_eq!(v2, id.to_string());
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
