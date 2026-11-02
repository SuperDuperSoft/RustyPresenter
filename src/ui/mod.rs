//! Native screens: the `.slint` files under `ui/` (compiled by `build.rs` from `ui/app.slint`) and their Rust glue.
slint::include_modules!();

pub mod stage;
pub mod room;
pub mod screens;
pub mod operator;

/// Installs the testing backend on this thread once. Slint's platform slot is thread-local and cargo runs each test
/// on its own thread, so the guard is per thread too (a process-wide `Once` would leave later threads without one).
#[cfg(test)]
pub(crate) fn ui_test_init() {
    thread_local!(static INIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    INIT.with(|done| {
        if !done.replace(true) {
            i_slint_backend_testing::init_no_event_loop();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_matches_operator_css() {
        ui_test_init();
        let w = crate::ui::OperatorWindow::new().unwrap();
        let t = w.global::<crate::ui::Theme>();
        assert_eq!(t.get_bg(), slint::Color::from_rgb_u8(0x0d, 0x0f, 0x13));
        assert_eq!(t.get_accent(), slint::Color::from_rgb_u8(0xf5, 0xb9, 0x4a));
        assert_eq!(t.get_live(), slint::Color::from_rgb_u8(0xff, 0x4f, 0x55));
    }

    #[test]
    fn widget_gallery_builds() {
        // Every shared widget and icon is instantiated once (the `widgets` example shows this window).
        ui_test_init();
        let w = crate::ui::WidgetGallery::new().unwrap();
        w.show().unwrap();
        assert_eq!(slint::Model::row_count(&w.get_icons()), 24);
        w.hide().unwrap();
    }
}
