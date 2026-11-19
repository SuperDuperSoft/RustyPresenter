//! Settings → About (native spec §7, Plan 3 Task 21): Slint's own `AboutSlint` panel, the version and data folder,
//! how video is drawn and which libmpv loaded, what can convert slides here, the LGPL notice for libmpv/FFmpeg, and
//! the third-party licences (`THIRD-PARTY-LICENSES.txt`, made by `scripts/licenses.sh`).
use super::settings::strings;
use crate::{
    import::deck::{ConverterInfo, Lo},
    ui::{SettingsDlg, operator::{view::OperatorSnapshot, window::{Operator, VideoInfo}}},
    video::DrawMode,
};
use slint::ComponentHandle;
use std::rc::Rc;

/// Every crate's licence, then the run-time libraries' notices and texts: the only licence text there is, and the
/// same file the installers ship. Shown by **Show licenses**.
pub const THIRD_PARTY: &str = include_str!("../../../THIRD-PARTY-LICENSES.txt");

/// The embedded licence text.
pub fn licences() -> String { THIRD_PARTY.into() }

/// What the About tab says. `pdfium` and `libreoffice` are empty until the converters have been looked at.
#[derive(Clone, Debug, PartialEq)]
pub struct AboutInfo {
    pub version: String,
    pub data_folder: String,
    pub video: String,
    pub mpv: String,
    pub ffmpeg: String,
    pub lgpl: bool,
    pub pdfium: String,
    pub libreoffice: String,
    pub powerpoint: bool,
}

/// The libmpv was built without GPL parts. Only a build that says so counts: mpv's default is GPL.
pub fn is_lgpl(configuration: &str) -> bool { configuration.split_whitespace().any(|o| o == "-Dgpl=false") }

pub fn about_info(snap: &OperatorSnapshot, info: &VideoInfo, conv: Option<&ConverterInfo>) -> AboutInfo {
    let video = match (&info.mpv, info.draw_mode) {
        (None, _) => strings::video_missing(),
        (Some(_), DrawMode::OpenGl) => strings::video_gpu(),
        (Some(_), DrawMode::Software) => strings::video_software(),
    };
    let lgpl = info.mpv.as_ref().is_some_and(|m| is_lgpl(&m.configuration));
    AboutInfo {
        version: strings::about_version(&snap.version),
        data_folder: about_folder(&snap.data_folder, crate::platform::data_folder::portable_dir().is_some()),
        video: video.into(),
        mpv: info.mpv.as_ref().map(|m| strings::mpv_line(&m.mpv_version, lgpl, m.api, &m.path)).unwrap_or_default(),
        ffmpeg: info.mpv.as_ref().map(|m| m.ffmpeg_version.clone()).unwrap_or_default(),
        lgpl,
        pdfium: conv.map(|c| if c.status.pdfium { strings::pdfium_found() } else { strings::pdfium_missing() }.to_string()).unwrap_or_default(),
        libreoffice: conv.map(libreoffice).unwrap_or_default(),
        powerpoint: conv.is_some_and(|c| c.status.powerpoint),
    }
}

/// The data folder as About shows it; a portable copy says its data is kept beside the app.
pub fn about_folder(folder: &str, portable: bool) -> String {
    if portable { crate::tr!("{folder} (a portable copy: the data is kept beside the app)", folder = folder) } else { folder.into() }
}

fn libreoffice(c: &ConverterInfo) -> String {
    match (c.status.libreoffice, &c.soffice) {
        (Lo::Snap, _) => strings::libreoffice_snap().into(),
        (Lo::Ok, Some(path)) => path.display().to_string(),
        _ => strings::libreoffice_none().into(),
    }
}

/// Fills the About tab from the snapshot and the window's video info. The licence text is set only when shown.
pub(super) fn fill(op: &Rc<Operator>, snap: &OperatorSnapshot) {
    let info = op.screens().video_info();
    let a = about_info(snap, &info, None);
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    g.set_about_version(a.version.into());
    g.set_about_data_folder(a.data_folder.into());
    g.set_about_video(a.video.into());
    g.set_about_mpv(a.mpv.into());
    g.set_about_ffmpeg(strings::ffmpeg_line(&a.ffmpeg).into());
    let (player, drawing) = video_lines(&op.screens().video_info());
    g.set_video_player(player.into());
    g.set_video_drawing(drawing.into());
    g.set_about_licence(strings::media_licence().into());
    let (me, snap) = (Rc::downgrade(op), snap.clone());
    op.ops().converter_info(move |conv| {
        let Some(op) = me.upgrade() else { return };
        let Some(conv) = conv else { return };
        let a = about_info(&snap, &info, Some(&conv));
        let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
        g.set_about_pdfium(strings::pdfium_line(&a.pdfium).into());
        g.set_about_libreoffice(strings::libreoffice_line(&a.libreoffice).into());
        g.set_about_powerpoint(strings::powerpoint_line(a.powerpoint).into());
    });
}

/// The Video tab's two lines: which player, and how video is drawn.
pub fn video_lines(info: &VideoInfo) -> (String, String) {
    let Some(mpv) = &info.mpv else { return (super::add_strings::player_missing().into(), String::new()) };
    let drawing = match info.draw_mode { DrawMode::OpenGl => strings::drawing_gpu(), DrawMode::Software => strings::drawing_software() };
    (strings::video_player(&mpv.mpv_version), drawing.into())
}

/// Show licenses / Back: the licence text replaces the tab's body while shown.
pub(super) fn show_licences(op: &Operator, show: bool) {
    let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
    g.set_licences(if show { licences().into() } else { Default::default() });
    g.set_licences_open(show);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::test_app,
        ui::{
            dialogs::{SettingsTab, settings},
            operator::window::{MpvInfo, testing::{open, refresh}},
            dialogs::settings::testing::find_any as find,
        },
    };
    use crate::import::deck::ConverterStatus;
    use i_slint_backend_testing::ElementHandle;
    use slint::platform::PointerEventButton;

    fn snapshot() -> OperatorSnapshot { OperatorSnapshot::read(&test_app()) }

    #[test]
    fn about_shows_aboutslint() {
        let app = test_app();
        let op = open(&app);
        op.window().set_size(slint::LogicalSize::new(1520.0, 900.0));
        slint::platform::update_timers_and_animations();
        refresh(&op);
        settings::open(&op, SettingsTab::About);
        slint::platform::update_timers_and_animations();
        assert_eq!(ElementHandle::find_by_element_type_name(crate::ui::dialogs::settings_window::win(&op), "AboutSlint").count(), 1);
        let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
        assert_eq!(g.get_tab(), settings::tab_index(SettingsTab::About));
        assert_eq!(g.get_about_version(), format!("RustyPresenter {}", env!("CARGO_PKG_VERSION")).as_str());
        assert_eq!(g.get_about_data_folder(), app.store.root.display().to_string().as_str());
        assert_eq!(g.get_about_video(), "Video: software drawing", "the fake screens draw in software");
        assert!(g.get_about_licence().starts_with("Video playback uses libmpv (LGPL-2.1+) and FFmpeg libraries under the LGPLv2.1 — source: https://"));
        find(&op, "SettingsAbout::show-licences").unwrap().mock_single_click(PointerEventButton::Left);
        slint::platform::update_timers_and_animations();
        assert!(g.get_licences_open());
        assert!(find(&op, "SettingsAbout::licence-text").is_some());
        find(&op, "SettingsAbout::hide-licences").unwrap().mock_single_click(PointerEventButton::Left);
        assert!(!g.get_licences_open());
        assert_eq!(g.get_licences(), "", "the text is dropped once hidden");
    }

    #[test]
    fn about_info_reports_software_mode() {
        let snap = snapshot();
        let mpv = MpvInfo { api: (2, 5), path: r"C:\deps\libmpv-2.dll".into(), mpv_version: "mpv 0.39.0".into(), ffmpeg_version: "n7.1".into(), configuration: "-Dgpl=false -Dlibmpv=true".into() };
        let info = VideoInfo { draw_mode: DrawMode::Software, mpv: Some(mpv.clone()) };
        let a = about_info(&snap, &info, None);
        assert_eq!(a.video, "Video: software drawing");
        assert_eq!(a.mpv, r"libmpv 0.39.0 (LGPL build) · client API 2.5 — C:\deps\libmpv-2.dll");
        assert_eq!(a.ffmpeg, "n7.1");
        assert!(a.lgpl);
        let unread = VideoInfo { draw_mode: DrawMode::Software, mpv: Some(MpvInfo { mpv_version: String::new(), ffmpeg_version: String::new(), configuration: String::new(), ..mpv }) };
        assert_eq!(about_info(&snap, &unread, None).mpv, r"libmpv (client API 2.5) — C:\deps\libmpv-2.dll", "versions not read");
        assert_eq!(a.version, format!("RustyPresenter {}", snap.version));
        assert_eq!(a.data_folder, snap.data_folder);
        let gpu = about_info(&snap, &VideoInfo { draw_mode: DrawMode::OpenGl, ..info }, None);
        assert_eq!(gpu.video, "Video: GPU (OpenGL)");
    }

    #[test]
    fn about_says_when_the_data_is_beside_the_app() {
        assert_eq!(about_folder("C:/Data", false), "C:/Data");
        let p = about_folder("E:/RustyPresenter/RustyPresenter Data", true);
        assert!(p.starts_with("E:/RustyPresenter/RustyPresenter Data") && p.contains("portable"), "{p}");
    }

    #[test]
    fn about_info_reports_missing_player() {
        let a = about_info(&snapshot(), &VideoInfo { draw_mode: DrawMode::OpenGl, mpv: None }, None);
        assert_eq!(a.video, "Video player missing");
        assert_eq!(a.mpv, "");
    }

    fn conv(pdfium: bool, libreoffice: Lo, powerpoint: bool, soffice: Option<&str>) -> ConverterInfo {
        ConverterInfo { status: ConverterStatus { powerpoint, libreoffice, pdfium }, soffice: soffice.map(Into::into) }
    }

    fn no_video() -> VideoInfo { VideoInfo { draw_mode: DrawMode::Software, mpv: None } }

    #[test]
    fn about_info_reports_missing_pdfium_and_libreoffice() {
        let a = about_info(&snapshot(), &no_video(), Some(&conv(false, Lo::None, false, None)));
        assert_eq!(a.pdfium, "missing");
        assert_eq!(a.libreoffice, "not installed");
        assert!(!a.powerpoint);
    }

    #[test]
    fn about_info_reports_the_converters_found() {
        let a = about_info(&snapshot(), &no_video(), Some(&conv(true, Lo::Ok, true, Some("/opt/lo/soffice"))));
        assert_eq!(a.pdfium, "found");
        assert_eq!(a.libreoffice, "/opt/lo/soffice");
        assert!(a.powerpoint);
        let snap = about_info(&snapshot(), &no_video(), Some(&conv(true, Lo::Snap, false, Some("/snap/bin/libreoffice"))));
        assert_eq!(snap.libreoffice, "Snap — not usable");
    }

    #[test]
    fn about_info_says_nothing_of_converters_until_they_are_looked_at() {
        let a = about_info(&snapshot(), &no_video(), None);
        assert_eq!((a.pdfium.as_str(), a.libreoffice.as_str(), a.powerpoint), ("", "", false));
    }

    #[test]
    fn about_info_flags_gpl_libmpv() {
        let mpv = |configuration: &str| MpvInfo { api: (2, 5), path: "libmpv-2.dll".into(), mpv_version: "mpv 0.41.0".into(), ffmpeg_version: String::new(), configuration: configuration.into() };
        let line = |configuration| {
            let a = about_info(&snapshot(), &VideoInfo { draw_mode: DrawMode::Software, mpv: Some(mpv(configuration)) }, None);
            (a.mpv, a.lgpl)
        };
        assert_eq!(line("-Dlibmpv=true"), ("libmpv 0.41.0 (not an LGPL build) · client API 2.5 — libmpv-2.dll".to_string(), false));
        assert_eq!(line("-Dgpl=false -Dlibmpv=true"), ("libmpv 0.41.0 (LGPL build) · client API 2.5 — libmpv-2.dll".to_string(), true));
        assert!(!is_lgpl("-Dgpl=true"));
        assert!(!is_lgpl("-Dgpl=falsey"), "the whole option, not a piece of one");
    }

    #[test]
    fn about_tab_shows_the_converters_it_finds() {
        let app = test_app();
        let dir = app.store.root.clone();
        let soffice = dir.join("soffice");
        std::fs::write(&soffice, "").unwrap();
        let path = soffice.display().to_string();
        let find = Box::new(move || Some(crate::platform::tools::Soffice::new(soffice.clone())));
        app.set_converters(std::sync::Arc::new(crate::import::deck::Converters::new(find, None, None, dir)));
        let op = open(&app);
        refresh(&op);
        settings::open(&op, SettingsTab::About);
        let g = crate::ui::dialogs::settings_window::win(&op).global::<SettingsDlg>();
        assert_eq!(g.get_about_pdfium(), "pdfium: missing");
        assert_eq!(g.get_about_libreoffice(), format!("LibreOffice: {path}").as_str());
        assert_eq!(g.get_about_powerpoint(), "PowerPoint: not found");
    }

    #[test]
    fn the_rows_show_once_the_converters_are_looked_at() {
        assert_eq!(strings::pdfium_line("missing"), "pdfium: missing");
        assert_eq!(strings::libreoffice_line("not installed"), "LibreOffice: not installed");
        assert_eq!(strings::powerpoint_line(true), "PowerPoint: found");
        assert_eq!(strings::powerpoint_line(false), "PowerPoint: not found");
        assert_eq!(strings::ffmpeg_line(""), "", "no row for a version that could not be read");
        assert_eq!(strings::ffmpeg_line("n7.1"), "FFmpeg n7.1");
    }

    /// Marks a licence file still waiting for its real text; `scripts/licenses.sh` leaves it out.
    const PLACEHOLDER: &str = "RUSTYPRESENTER-LICENCE-PLACEHOLDER";

    /// `scripts/licenses.sh` appended every file in `packaging/licenses/` but placeholders. Fails when one is added
    /// or changed and the script was not run; the CI licence job checks the crates half.
    #[test]
    fn every_licence_file_is_in_the_text() {
        let text = licences().replace("\r\n", "\n");
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/licenses")).unwrap() {
            let path = entry.unwrap().path();
            let file = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
            if file.contains(PLACEHOLDER) {
                continue;
            }
            assert!(text.contains(file.trim()), "{} is not in THIRD-PARTY-LICENSES.txt: run scripts/licenses.sh", path.display());
        }
    }

    #[test]
    fn no_placeholder_reaches_the_text() {
        assert!(!licences().contains(PLACEHOLDER), "a placeholder licence file was appended: rerun scripts/licenses.sh");
    }

    #[test]
    fn licenses_text_has_the_ffmpeg_text() {
        assert!(licences().contains("=== FFmpeg-COPYING.LGPLv2.1.txt ==="));
    }

    #[test]
    fn licenses_text_has_the_lgpl_text() {
        let text = licences();
        assert!(text.contains("GNU LESSER GENERAL PUBLIC LICENSE"));
        assert!(text.contains("Version 2.1, February 1999"), "the licence itself, not a mention of it");
        assert!(text.contains("SIL OPEN FONT LICENSE"), "the font's licence");
    }

    /// Needs `packaging/licenses/pdfium-LICENSE.txt`, copied from the pdfium archive. No copy could be reached from
    /// the sandbox that wrote this, and it must be the real file. Remove the `ignore` once it is there.
    #[test]
    #[ignore = "pdfium-LICENSE.txt is not in the tree yet"]
    fn licenses_text_has_the_pdfium_text() {
        assert!(licences().contains("=== pdfium-LICENSE.txt ==="));
    }

    #[test]
    fn licenses_text_mentions_slint_and_libmpv() {
        let text = licences();
        assert!(text.contains("Slint"), "the Slint crates' licence");
        assert!(text.contains("libmpv"), "the run-time libraries' notice");
        assert!(text.contains("LGPL"));
        assert!(text.find("Slint").unwrap() < text.find("libmpv (mpv)").unwrap(), "crates first, then native.txt");
        // The programs Download from YouTube fetches on first use are named with their licences.
        for name in ["yt-dlp — The Unlicense", "Deno — MIT License", "FFmpeg (ffmpeg, ffprobe) — GNU General Public License"] {
            assert!(text.contains(name), "{name}");
        }
    }
}
