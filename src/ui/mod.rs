//! Native screens: the `.slint` files under `ui/` (compiled by `build.rs` from `ui/app.slint`) and their Rust glue.
slint::include_modules!();

pub mod stage;
pub mod room;
pub mod screens;
pub mod operator;
pub mod ops;
pub mod thumbs;

/// Runs a future on this (plain) thread, e.g. a `Renderer` reply, which needs no runtime.
pub(crate) fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::{sync::Arc, task::{Context, Poll, Wake, Waker}};
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark { fn wake(self: Arc<Self>) { self.0.unpark() } }
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) { return v }
        std::thread::park();
    }
}

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

    #[test]
    fn tooltip_does_not_swallow_the_click() {
        // An operator rests on a button until its tooltip shows, then clicks without moving: the click must land.
        use i_slint_backend_testing::{mock_elapsed_time, ElementHandle};
        use slint::platform::{PointerEventButton, WindowEvent};
        use slint::{ComponentHandle, LogicalPosition};
        use std::time::Duration;
        ui_test_init();
        let w = crate::ui::WidgetGallery::new().unwrap();
        w.show().unwrap();
        let btn = ElementHandle::find_by_element_id(&w, "WidgetGallery::tip-btn").next().expect("tooltip button");
        let (p, s) = (btn.absolute_position(), btn.size());
        let at = LogicalPosition::new(p.x + s.width / 2.0, p.y + s.height / 2.0);
        let tip_shown = || ElementHandle::find_by_accessible_label(&w, "A tooltip").next().is_some();
        w.window().dispatch_event(WindowEvent::PointerMoved { position: at });
        mock_elapsed_time(Duration::from_millis(700));
        assert!(tip_shown(), "the tooltip shows after resting on the button");
        w.window().dispatch_event(WindowEvent::PointerPressed { position: at, button: PointerEventButton::Left });
        w.window().dispatch_event(WindowEvent::PointerReleased { position: at, button: PointerEventButton::Left });
        assert_eq!(w.get_tip_clicks(), 1, "the click reached the button");
        // A second rest-and-click works too (the tooltip closed on the press and reopens after the delay).
        w.window().dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(at.x + 1.0, at.y) });
        mock_elapsed_time(Duration::from_millis(700));
        w.window().dispatch_event(WindowEvent::PointerPressed { position: at, button: PointerEventButton::Left });
        w.window().dispatch_event(WindowEvent::PointerReleased { position: at, button: PointerEventButton::Left });
        assert_eq!(w.get_tip_clicks(), 2);
        w.hide().unwrap();
    }

    #[test]
    fn narrow_top_button_keeps_its_side_padding() {
        // `operator.css:180` hides the label of Screens/Phone but keeps their 8×10 padding; `.icon-only` is 8 all round.
        use i_slint_backend_testing::ElementHandle;
        use slint::ComponentHandle;
        ui_test_init();
        let w = crate::ui::WidgetGallery::new().unwrap();
        w.show().unwrap();
        let width = |id: &str| ElementHandle::find_by_element_id(&w, id).next().expect(id).size().width;
        assert_eq!(width("WidgetGallery::narrow-btn"), 18.0 + 2.0 * 10.0);
        assert_eq!(width("WidgetGallery::icon-btn"), 18.0 + 2.0 * 8.0);
        w.hide().unwrap();
    }

    #[test]
    fn copy_text_is_not_a_typing_field() {
        // The web `.copy` is a `<span>`: clicking it must not make the operator keys think a text field has focus.
        use i_slint_backend_testing::ElementHandle;
        use slint::platform::PointerEventButton;
        use slint::ComponentHandle;
        ui_test_init();
        let w = crate::ui::WidgetGallery::new().unwrap();
        w.show().unwrap();
        let copy = ElementHandle::find_by_element_id(&w, "WidgetGallery::copy").next().expect("copy text");
        copy.mock_single_click(PointerEventButton::Left);
        assert!(!w.global::<crate::ui::OperatorState>().get_focus_in_text());
        w.hide().unwrap();
    }
}
