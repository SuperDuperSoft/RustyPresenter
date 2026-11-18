//! Free disk space, so a download can stop before it fills the disk (Bible library spec §6).
use std::path::Path;

/// Bytes this user may still write on the disk that holds `path` (an existing folder). `None` when the system
/// won't say, and then the caller goes ahead: a full disk still fails the write, and the job says so.
pub fn free_space(path: &Path) -> Option<u64> { imp::free_space(path) }

#[cfg(unix)]
mod imp {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, path::Path};

    pub fn free_space(path: &Path) -> Option<u64> {
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is a valid C string and `s` a writable statvfs for the call's length.
        if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 { return None; }
        #[allow(clippy::unnecessary_cast)] // The field types differ between Linux and macOS.
        Some((s.f_bavail as u64).saturating_mul(s.f_frsize as u64))
    }
}

#[cfg(windows)]
mod imp {
    use std::{os::windows::ffi::OsStrExt, path::Path};
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    pub fn free_space(path: &Path) -> Option<u64> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut free: u64 = 0;
        // SAFETY: `wide` ends in a NUL; the two totals we don't need may be null.
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
        (ok != 0).then_some(free)
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn free_space(_: &std::path::Path) -> Option<u64> { None }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_temp_folder_has_some_space_and_a_missing_one_says_nothing() {
        let free = super::free_space(&std::env::temp_dir());
        assert!(free.is_some_and(|f| f > 0), "{free:?}");
        assert_eq!(super::free_space(std::path::Path::new("/no/such/folder/here")), None);
    }
}
