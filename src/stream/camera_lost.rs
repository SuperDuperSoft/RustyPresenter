//! What the stream shows in a camera's place while that camera sends nothing (unplugged, a phone out of Wi-Fi, a
//! network camera restarting): Settings → Media → Livestream → "When a camera sends nothing". The room screen (as
//! before), the church's logo, or black. The camera's own picture comes back by itself as soon as it sends again
//! (`camera_feed`); the lost-camera light still says the camera is not sending.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The church's choice.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CameraLost {
    /// The room screen fills the camera's place (as before this choice existed).
    #[default]
    Room,
    /// The church's logo (Settings → Look → Logo), on black; black when there is none.
    Logo,
    Black,
}

impl CameraLost {
    /// In Settings' order.
    pub const ALL: [CameraLost; 3] = [CameraLost::Room, CameraLost::Logo, CameraLost::Black];

    pub fn index(self) -> i32 { CameraLost::ALL.iter().position(|c| *c == self).unwrap_or(0) as i32 }
    pub fn from_index(i: i32) -> CameraLost { usize::try_from(i).ok().and_then(|i| CameraLost::ALL.get(i).copied()).unwrap_or_default() }
    pub fn is_default(&self) -> bool { *self == CameraLost::Room }
}

/// What the recorder's writer needs to know: the choice, and the logo's file when the church has one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lost {
    pub choice: CameraLost,
    pub logo: Option<PathBuf>,
}

/// The picture to put in a lost camera's place, RGBA of `size`; `None` for the room screen. Reads the logo's file:
/// done once when the stream starts, not for each picture.
pub fn prepare(lost: &Lost, size: (u32, u32)) -> Option<Arc<Vec<u8>>> {
    let mut out = [0u8, 0, 0, 255].repeat(super::camera_feed::frame_bytes(size) / 4);
    match lost.choice {
        CameraLost::Room => return None,
        CameraLost::Black => {}
        CameraLost::Logo => if let Some(p) = lost.logo.as_deref().and_then(logo) {
            let (w, h) = p.dimensions();
            // The logo at most two thirds of the picture, in the middle, its see-through parts on black.
            let (bw, bh) = (size.0 * 2 / 3, size.1 * 2 / 3);
            let (x0, y0, fw, fh) = crate::record::pump::fit_rect(w, h, bw.max(1), bh.max(1));
            let small = image::imageops::resize(&p, fw.max(1), fh.max(1), image::imageops::FilterType::Triangle);
            let (ox, oy) = ((size.0 - bw) / 2 + x0, (size.1 - bh) / 2 + y0);
            for (x, y, px) in small.enumerate_pixels() {
                let i = (((oy + y) * size.0 + ox + x) * 4) as usize;
                let a = u32::from(px[3]);
                for c in 0..3 { out[i + c] = (u32::from(px[c]) * a / 255) as u8 }
            }
        },
    }
    Some(Arc::new(out))
}

fn logo(path: &Path) -> Option<image::RgbaImage> {
    image::open(path).map_err(|e| eprintln!("stream: the logo for a lost camera could not be read: {e}")).ok().map(|i| i.to_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_camera_shows_the_room_the_logo_or_black() {
        let size = (64, 36);
        assert_eq!(prepare(&Lost::default(), size), None, "the room screen, as before");
        let black = prepare(&Lost { choice: CameraLost::Black, logo: None }, size).unwrap();
        assert!(black.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
        // A white logo: white in the middle, black at the edges.
        let dir = crate::store::test_dir();
        let file = dir.join("logo.png");
        image::RgbaImage::from_pixel(20, 20, image::Rgba([255, 255, 255, 255])).save(&file).unwrap();
        let p = prepare(&Lost { choice: CameraLost::Logo, logo: Some(file) }, size).unwrap();
        let mid = (18 * 64 + 32) * 4;
        assert_eq!(&p[mid..mid + 4], &[255, 255, 255, 255]);
        assert_eq!(&p[..4], &[0, 0, 0, 255]);
        // No logo, or one that cannot be read: black.
        let p = prepare(&Lost { choice: CameraLost::Logo, logo: Some(dir.join("missing.png")) }, size).unwrap();
        assert_eq!(&p[mid..mid + 4], &[0, 0, 0, 255]);
        assert_eq!(CameraLost::from_index(CameraLost::Black.index()), CameraLost::Black);
        assert_eq!(CameraLost::from_index(9), CameraLost::Room);
    }
}
