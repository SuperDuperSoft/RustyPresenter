//! Windows Firewall and the phones (Settings → Remote and network → Phone camera and microphone). The first time the
//! app listens on the network, Windows asks whether to let it in, with only "Private networks" ticked: on a
//! "Public" network (church Wi-Fi often is one) it then keeps every phone out, the remote, camera and microphone
//! alike, and says nothing. `check` asks Windows whether a phone may reach the app's port now; `let_phones_in`
//! adds the rule for private and public networks (Windows asks for an administrator's OK).

/// What Windows Firewall does with phones that come to the app's port on the network this computer is on now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Firewall {
    /// Phones may come in (or the firewall is off).
    Open,
    /// Phones are kept out; `public`: this computer is on a network Windows calls public.
    Blocked { public: bool },
    /// Not known (not Windows, or Windows did not say).
    Unknown,
}

/// The rule's name in Windows Firewall, as the installer adds it too.
pub const RULE: &str = "RustyPresenter";

/// What Windows Firewall does with phones coming to TCP `port` of this program.
pub fn check(port: u16) -> Firewall {
    #[cfg(windows)]
    { let exe = std::env::current_exe().ok(); exe.map_or(Firewall::Unknown, |e| win::check(&e, port)) }
    #[cfg(not(windows))]
    { let _ = port; Firewall::Unknown }
}

/// What `check` gives for program `exe` (tests ask about other programs).
#[cfg(windows)]
pub fn check_program(exe: &std::path::Path, port: u16) -> Firewall { win::check(exe, port) }

/// The command that lets phones in: the program's own rules go (a "keep out" rule Windows made when its question
/// was cancelled wins over any "let in" rule), then it is let in on private and public networks, TCP and UDP.
/// Run by `cmd /c` as an administrator.
pub fn command(exe: &str) -> String {
    let add = |protocol: &str| format!("netsh advfirewall firewall add rule name=\"{RULE}\" dir=in action=allow program=\"{exe}\" enable=yes profile=private,public protocol={protocol}");
    format!("\"netsh advfirewall firewall delete rule name=all program=\"{exe}\" dir=in & {} & {}\"", add("TCP"), add("UDP"))
}

/// Lets phones in through Windows Firewall: Windows asks for an administrator's OK. Waits for it (up to a minute).
/// Err: the words to show (said no, or it did not work).
pub fn let_phones_in() -> Result<(), String> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        win::run_as_admin("cmd.exe", &format!("/c {}", command(&exe.to_string_lossy())))
    }
    #[cfg(not(windows))]
    { Err(String::new()) }
}

#[cfg(windows)]
mod win {
    use super::Firewall;
    use ::windows::Win32::NetworkManagement::WindowsFirewall::{INetFwMgr, INetFwPolicy2, NET_FW_IP_PROTOCOL_TCP, NET_FW_IP_VERSION_V4, NET_FW_PROFILE2_PUBLIC, NetFwMgr, NetFwPolicy2};
    use ::windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use ::windows::Win32::System::Variant::VARIANT;
    use ::windows::core::BSTR;

    pub fn check(exe: &std::path::Path, port: u16) -> Firewall {
        // SAFETY: COM for this thread (a second call on the same thread is harmless), then plain COM calls.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let Ok(mgr) = CoCreateInstance::<_, INetFwMgr>(&NetFwMgr, None, CLSCTX_INPROC_SERVER) else { return Firewall::Unknown };
            let (mut allowed, mut restricted) = (VARIANT::default(), VARIANT::default());
            let asked = mgr.IsPortAllowed(&BSTR::from(exe.to_string_lossy().as_ref()), NET_FW_IP_VERSION_V4, i32::from(port), &BSTR::new(), NET_FW_IP_PROTOCOL_TCP, &mut allowed, &mut restricted);
            if asked.is_err() { return Firewall::Unknown }
            match bool::try_from(&allowed) {
                Ok(true) => Firewall::Open,
                Ok(false) => {
                    let public = CoCreateInstance::<_, INetFwPolicy2>(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER).ok()
                        .and_then(|p| p.CurrentProfileTypes().ok()).is_some_and(|t| t & NET_FW_PROFILE2_PUBLIC.0 != 0);
                    Firewall::Blocked { public }
                }
                Err(_) => Firewall::Unknown,
            }
        }
    }

    /// Runs `file params` as an administrator, hidden, and waits for it (up to a minute).
    pub fn run_as_admin(file: &str, params: &str) -> Result<(), String> {
        use windows_sys::Win32::{Foundation::{CloseHandle, GetLastError}, System::Threading::{GetExitCodeProcess, WaitForSingleObject},
            UI::Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW}};
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (verb, file, params) = (wide("runas"), wide(file), wide(params));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: verb.as_ptr(), lpFile: file.as_ptr(), lpParameters: params.as_ptr(),
            nShow: 0,
            ..Default::default()
        };
        // SAFETY: the strings live until the call returns; the process handle is closed below.
        unsafe {
            if ShellExecuteExW(&mut info) == 0 {
                // 1223: the person said no to Windows' question.
                return Err(if GetLastError() == 1223 { super::tr::said_no() } else { super::tr::did_not_work() });
            }
            if info.hProcess.is_null() { return Ok(()) }
            WaitForSingleObject(info.hProcess, 60_000);
            let mut code = 1u32;
            GetExitCodeProcess(info.hProcess, &mut code);
            CloseHandle(info.hProcess);
            if code == 0 { Ok(()) } else { Err(super::tr::did_not_work()) }
        }
    }
}

pub mod tr {
    use crate::tr;
    pub fn said_no() -> String { tr!("Windows was not given the OK, so nothing changed.") }
    pub fn did_not_work() -> String { tr!("Windows Firewall could not be changed. Ask whoever looks after this computer to let RustyPresenter in for private and public networks.") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_removes_the_programs_rules_and_lets_it_in_everywhere_but_work_networks() {
        let c = command(r"C:\Users\Church\AppData\Local\Programs\RustyPresenter\rustypresenter.exe");
        assert!(c.starts_with("\"netsh advfirewall firewall delete rule name=all program=\"C:\\Users\\Church") && c.ends_with("protocol=UDP\""), "{c}");
        assert_eq!(c.matches("add rule name=\"RustyPresenter\" dir=in action=allow").count(), 2);
        assert!(c.contains("profile=private,public protocol=TCP"));
    }

    /// Asks this PC's own Windows Firewall: a program it has never seen is kept out (when the firewall is on), and
    /// the answer never panics.
    #[cfg(windows)]
    #[test]
    fn windows_answers_for_a_program_it_never_saw() {
        let never = std::env::temp_dir().join(format!("rp-never-seen-{}.exe", std::process::id()));
        let answer = check_program(&never, 4320);
        eprintln!("firewall for an unknown program: {answer:?}; for this test: {:?}", check(4320));
        assert!(matches!(answer, Firewall::Blocked { .. } | Firewall::Open | Firewall::Unknown));
    }
}
