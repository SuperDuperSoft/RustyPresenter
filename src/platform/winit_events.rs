//! One window event filter shared by every user of a window. Slint keeps a single `on_winit_window_event` filter per
//! window (a second call replaces the first), so the operator window installs one filter that calls a `Dispatcher`.
//! Generic over the event so the fan-out is tested without a window.
use std::{cell::RefCell, rc::Rc};

/// The events a Slint window's winit filter sees.
pub type WindowEvent = slint::winit_030::winit::event::WindowEvent;

type Handler<E> = Box<dyn FnMut(&E)>;

pub struct Dispatcher<E> { handlers: Rc<RefCell<Vec<Handler<E>>>> }

impl<E> Clone for Dispatcher<E> {
    fn clone(&self) -> Self { Self { handlers: self.handlers.clone() } }
}

impl<E> Default for Dispatcher<E> {
    fn default() -> Self { Self { handlers: Rc::default() } }
}

impl<E> Dispatcher<E> {
    pub fn add(&self, f: impl FnMut(&E) + 'static) { self.handlers.borrow_mut().push(Box::new(f)); }

    /// A handler may call `add` (it starts with the next event) or `dispatch` (that event goes nowhere): the list is
    /// taken out while it runs, so neither can hit the `RefCell`.
    pub fn dispatch(&self, event: &E) {
        let mut running = std::mem::take(&mut *self.handlers.borrow_mut());
        for handler in &mut running { handler(event); }
        let added = std::mem::replace(&mut *self.handlers.borrow_mut(), running);
        self.handlers.borrow_mut().extend(added);
    }
}

/// Makes `events` the window's one winit event filter. Every event still reaches Slint afterwards. On a backend
/// without winit (the testing backend) nothing is installed.
pub fn attach(window: &slint::Window, events: Dispatcher<WindowEvent>) {
    use slint::winit_030::{EventResult, WinitWindowAccessor};
    window.on_winit_window_event(move |_, e| {
        events.dispatch(e);
        EventResult::Propagate
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn dispatcher_calls_every_handler() {
        let d = Dispatcher::<u32>::default();
        let seen = Rc::new(RefCell::new(Vec::new()));
        for tag in ["a", "b"] {
            let seen = seen.clone();
            d.add(move |e| seen.borrow_mut().push((tag, *e)));
        }
        d.dispatch(&7);
        d.dispatch(&8);
        assert_eq!(*seen.borrow(), [("a", 7), ("b", 7), ("a", 8), ("b", 8)]);
    }

    #[test]
    fn clones_share_their_handlers() {
        let d = Dispatcher::<u32>::default();
        let hits = Rc::new(Cell::new(0));
        let h = hits.clone();
        d.clone().add(move |_| h.set(h.get() + 1));
        d.dispatch(&1);
        assert_eq!(hits.get(), 1);
    }

    #[test]
    fn no_handlers_is_fine() { Dispatcher::<u32>::default().dispatch(&1); }

    #[test]
    fn a_handler_may_add_a_handler() {
        let d = Dispatcher::<u32>::default();
        let hits = Rc::new(Cell::new(0));
        let (outer, h) = (d.clone(), hits.clone());
        let mut added = false;
        d.add(move |_| {
            if !added {
                added = true;
                let h = h.clone();
                outer.add(move |_| h.set(h.get() + 1));
            }
        });
        d.dispatch(&1);
        assert_eq!(hits.get(), 0, "a new handler waits for the next event");
        d.dispatch(&2);
        assert_eq!(hits.get(), 1);
    }

    #[test]
    fn a_handler_may_dispatch() {
        let d = Dispatcher::<u32>::default();
        let hits = Rc::new(Cell::new(0));
        let (outer, h) = (d.clone(), hits.clone());
        d.add(move |e| {
            h.set(h.get() + 1);
            if *e == 1 { outer.dispatch(&2); }
        });
        d.dispatch(&1);
        d.dispatch(&3);
        assert_eq!(hits.get(), 2, "the nested event is dropped and the handler survives");
    }
}
