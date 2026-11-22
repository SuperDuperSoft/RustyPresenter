//! How much memory the computer has, so the picture caches take a fair share of a small one: 256 MB of drawn
//! slides is nothing on a 16 GB desktop and a great deal on a 4 GB laptop that also plays the video.
use std::sync::OnceLock;

/// The computer's memory in bytes. `None` when the system won't say, and then the caches keep their full size.
pub fn total() -> Option<u64> {
    static TOTAL: OnceLock<Option<u64>> = OnceLock::new();
    *TOTAL.get_or_init(imp::total)
}

const GIB: u64 = 1 << 30;

/// What a cache of `full` bytes may hold on a computer with `total` bytes of memory: all of it with 8 GB or more,
/// half with 4 GB, a quarter below that. A computer sold with 8 GB reports a little less (the graphics chip keeps
/// some), so the steps sit under the round numbers.
pub fn share_of(full: usize, total: Option<u64>) -> usize {
    match total {
        Some(t) if t < 3 * GIB => full / 4,
        Some(t) if t < 7 * GIB => full / 2,
        _ => full,
    }
}

/// `share_of` for this computer.
pub fn share(full: usize) -> usize { share_of(full, total()) }

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    pub fn total() -> Option<u64> {
        let mut s: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        s.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: `s` is a writable MEMORYSTATUSEX with its length set, as the call asks.
        (unsafe { GlobalMemoryStatusEx(&mut s) } != 0 && s.ullTotalPhys > 0).then_some(s.ullTotalPhys)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    pub fn total() -> Option<u64> {
        let mut bytes: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        // SAFETY: the name is a C string, and `bytes` is writable for `len` bytes.
        let r = unsafe { libc::sysctlbyname(c"hw.memsize".as_ptr(), (&mut bytes as *mut u64).cast(), &mut len, std::ptr::null_mut(), 0) };
        (r == 0 && bytes > 0).then_some(bytes)
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    pub fn total() -> Option<u64> { super::meminfo_total(&std::fs::read_to_string("/proc/meminfo").ok()?) }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn total() -> Option<u64> { None }
}

/// `MemTotal` of Linux's `/proc/meminfo` (given in kB), in bytes.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn meminfo_total(text: &str) -> Option<u64> {
    let line = text.lines().find_map(|l| l.strip_prefix("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().next()?.parse().ok()?;
    (kb > 0).then(|| kb.saturating_mul(1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_computer_keeps_less() {
        let full = 256 << 20;
        assert_eq!(share_of(full, None), full, "not known: as before");
        assert_eq!(share_of(full, Some(16 * GIB)), full);
        assert_eq!(share_of(full, Some(7 * GIB + GIB / 2)), full, "an 8 GB laptop reports a little under 8");
        assert_eq!(share_of(full, Some(4 * GIB - GIB / 8)), full / 2, "a 4 GB laptop");
        assert_eq!(share_of(full, Some(2 * GIB)), full / 4);
    }

    #[test]
    fn this_computer_says_how_much_it_has() {
        if cfg!(any(windows, target_os = "linux", target_os = "macos")) {
            assert!(total().is_some_and(|t| t > GIB / 4), "{:?}", total());
        }
    }

    #[test]
    fn linux_meminfo() {
        assert_eq!(meminfo_total("MemTotal:        3980204 kB\nMemFree:  1 kB\n"), Some(3980204 * 1024));
        assert_eq!(meminfo_total("MemFree: 1 kB\n"), None);
    }
}
