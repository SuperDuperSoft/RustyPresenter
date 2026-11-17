//! Tally lights on phones: a camera operator opens `/stage-view/tally?camera=1` on a phone fixed to the camera, and
//! it goes red while that camera is in the stream, green while it is the camera chosen to go out next (so the camera
//! operator can get the shot ready), and dark otherwise. Served with the stage view, so
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
    /// The camera chosen to go out next (with one camera: that camera, while the scene shows none).
    pub next: Option<usize>,
}

/// One camera's light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Light { OnAir, Next, Off }

impl Light {
    pub fn code(self) -> &'static str { match self { Light::OnAir => "on_air", Light::Next => "next", Light::Off => "off" } }
}

impl Tally {
    /// While streaming: `camera` (0 is the first) is in the picture when the scene going out shows a camera;
    /// `chosen_next` is the camera chosen to go out next (`None` with one camera, when `camera` is next while the
    /// scene shows none). `None` camera: no camera.
    pub fn of(live: bool, camera: Option<usize>, chosen_next: Option<usize>, shows_camera: bool) -> Tally {
        if !live || camera.is_none() { return Tally::default() }
        let on_air = camera.filter(|_| shows_camera);
        let next = chosen_next.or(camera.filter(|_| !shows_camera)).filter(|n| Some(*n) != on_air);
        Tally { live, on_air, next }
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
        tr!("Turn on the stage screen on phones (Settings → Remote and network). If it has a password, open the stage screen on this phone once and sign in.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_in_the_picture_is_red_and_the_chosen_one_waiting_is_green() {
        // One camera: red in the picture, green while the scene shows none.
        let t = Tally::of(true, Some(1), None, true);
        assert_eq!([t.light(0), t.light(1)], [Light::Off, Light::OnAir]);
        let t = Tally::of(true, Some(0), None, false);
        assert_eq!([t.light(0), t.light(1)], [Light::Next, Light::Off]);
        // Not streaming, or no camera: every light is off.
        for t in [Tally::of(false, Some(0), Some(1), true), Tally::of(true, None, Some(1), true), Tally::default()] {
            assert_eq!([t.light(0), t.light(1)], [Light::Off, Light::Off], "{t:?}");
        }
        assert_eq!([Light::OnAir.code(), Light::Next.code(), Light::Off.code()], ["on_air", "next", "off"]);
    }

    #[test]
    fn green_is_the_camera_chosen_to_go_out_next() {
        let t = Tally::of(true, Some(0), Some(2), true);
        assert_eq!([t.light(0), t.light(1), t.light(2), t.light(3)], [Light::OnAir, Light::Off, Light::Next, Light::Off]);
        // A scene without a camera: the camera out stays dark, the one chosen next green.
        let t = Tally::of(true, Some(0), Some(1), false);
        assert_eq!([t.light(0), t.light(1)], [Light::Off, Light::Next]);
    }
}
