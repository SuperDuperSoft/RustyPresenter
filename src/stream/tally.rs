//! Tally lights on phones: a camera operator opens `/stage-view/tally?camera=1` on a phone fixed to the camera, and
//! it goes red while that camera is in the stream, green while it is the camera that the stream switches to when a
//! camera scene comes (so the operator can get the shot ready), and dark otherwise. Served with the stage view, so
//! it is on when the stage view is on and asks for the same password.
//!
//! The operator window keeps `App::tally` current while it streams (`Operator::show_scene`); the page asks twice a
//! second.

/// What the tallies show now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// A livestream is going out.
    pub live: bool,
    /// The camera in the stream's picture now (0 is the first), if any.
    pub on_air: Option<usize>,
    /// The camera the stream switches to when a camera scene comes, while none is in the picture.
    pub next: Option<usize>,
}

/// One camera's light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Light { OnAir, Next, Off }

impl Light {
    pub fn code(self) -> &'static str { match self { Light::OnAir => "on_air", Light::Next => "next", Light::Off => "off" } }
}

impl Tally {
    /// While streaming: `camera` (0 is the first) is in the picture when the scene going out shows a camera, and
    /// is next otherwise. `None`: no camera chosen.
    pub fn of(live: bool, camera: Option<usize>, shows_camera: bool) -> Tally {
        if !live { return Tally::default() }
        Tally { live, on_air: camera.filter(|_| shows_camera), next: camera.filter(|_| !shows_camera) }
    }

    /// Camera `camera`'s light (0 is the first).
    pub fn light(&self, camera: usize) -> Light {
        if !self.live { Light::Off } else if self.on_air == Some(camera) { Light::OnAir } else if self.next == Some(camera) { Light::Next } else { Light::Off }
    }
}

/// What the tally page says.
pub mod tr {
    use crate::tr;
    pub fn on_air(camera: usize) -> String { tr!("Camera {camera}: on air", camera = camera) }
    pub fn next(camera: usize) -> String { tr!("Camera {camera}: next", camera = camera) }
    pub fn off(camera: usize) -> String { tr!("Camera {camera}", camera = camera) }
    pub fn not_live() -> String { tr!("Not streaming") }
    pub fn needs_stage_view() -> String {
        tr!("Turn on the stage view in Settings → Network. If it has a password, open the stage view on this phone once and sign in.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_in_the_picture_is_red_and_the_chosen_one_waiting_is_green() {
        let t = Tally::of(true, Some(1), true);
        assert_eq!([t.light(0), t.light(1)], [Light::Off, Light::OnAir]);
        let t = Tally::of(true, Some(0), false);
        assert_eq!([t.light(0), t.light(1)], [Light::Next, Light::Off]);
        // Not streaming, or no camera: every light is off.
        for t in [Tally::of(false, Some(0), true), Tally::of(true, None, true), Tally::default()] {
            assert_eq!([t.light(0), t.light(1)], [Light::Off, Light::Off], "{t:?}");
        }
        assert_eq!([Light::OnAir.code(), Light::Next.code(), Light::Off.code()], ["on_air", "next", "off"]);
    }
}
