//! A value behind a lock that counts its changes. `App::data` is one: everything that draws from the data (the
//! operator window, each phone's view, the auto-follow worker) remembers the revision it last read and does its
//! work again only when the revision has moved on.
use std::ops::{Deref, DerefMut};
use std::sync::{LockResult, Mutex, MutexGuard, PoisonError, TryLockError, TryLockResult, atomic::{AtomicU64, Ordering}};

/// A `Mutex` whose revision moves on each time a holder of the lock reached for the value mutably. It cannot miss
/// a change, whoever makes it and however: there is no "remember to bump" for an edit to forget. A holder that
/// only read leaves the revision alone; one that took `&mut` and changed nothing moves it on, which costs the
/// readers one needless look.
pub struct Tracked<T> { value: Mutex<T>, rev: AtomicU64 }

/// The lock on a `Tracked`, held. Reads like the value; the first mutable reach marks it changed, and the revision
/// moves on when the lock is let go (before the next holder can see the value).
pub struct TrackedGuard<'a, T> { guard: MutexGuard<'a, T>, rev: &'a AtomicU64, touched: bool }

impl<T> Tracked<T> {
    pub fn new(value: T) -> Self { Self { value: Mutex::new(value), rev: AtomicU64::new(0) } }

    fn held<'a>(&'a self, guard: MutexGuard<'a, T>) -> TrackedGuard<'a, T> { TrackedGuard { guard, rev: &self.rev, touched: false } }

    /// As `Mutex::lock`.
    pub fn lock(&self) -> LockResult<TrackedGuard<'_, T>> {
        match self.value.lock() {
            Ok(guard) => Ok(self.held(guard)),
            Err(poisoned) => Err(PoisonError::new(self.held(poisoned.into_inner()))),
        }
    }

    /// As `Mutex::try_lock`.
    pub fn try_lock(&self) -> TryLockResult<TrackedGuard<'_, T>> {
        match self.value.try_lock() {
            Ok(guard) => Ok(self.held(guard)),
            Err(TryLockError::Poisoned(poisoned)) => Err(TryLockError::Poisoned(PoisonError::new(self.held(poisoned.into_inner())))),
            Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
        }
    }

    /// The revision: a number that is different after every change. Read while the lock is held, it is the
    /// revision of exactly what the lock shows.
    pub fn rev(&self) -> u64 { self.rev.load(Ordering::Acquire) }
}

impl<T> Deref for TrackedGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { &self.guard }
}

impl<T> DerefMut for TrackedGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.touched = true;
        &mut self.guard
    }
}

impl<T> Drop for TrackedGuard<'_, T> {
    fn drop(&mut self) {
        if self.touched { self.rev.fetch_add(1, Ordering::AcqRel); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_revision_moves_only_when_the_value_was_reached_for_mutably() {
        let t = Tracked::new(vec![1, 2, 3]);
        assert_eq!(t.rev(), 0);
        assert_eq!(t.lock().unwrap().len(), 3);
        { let read = t.lock().unwrap(); assert_eq!(read[0], 1); }
        assert_eq!(t.rev(), 0, "reading changes nothing");
        t.lock().unwrap().push(4);
        assert_eq!(t.rev(), 1);
        {
            let mut held = t.lock().unwrap();
            held.push(5);
            held.push(6);
            assert_eq!(t.rev(), 1, "one hold is one change, counted when the lock is let go");
        }
        assert_eq!((t.rev(), t.lock().unwrap().len()), (2, 6));
        // A `mut` binding that only reads is still only a read.
        { let mut held = t.lock().unwrap(); assert_eq!(held.len(), 6); let _ = &mut held; }
        assert_eq!(t.rev(), 2);
        *t.try_lock().unwrap() = vec![];
        assert_eq!(t.rev(), 3);
    }

    #[test]
    fn a_poisoned_lock_still_hands_out_the_value() {
        let t = std::sync::Arc::new(Tracked::new(1));
        let other = t.clone();
        let _ = std::thread::spawn(move || { let mut held = other.lock().unwrap(); *held = 2; panic!("while holding the lock") }).join();
        let held = t.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(*held, 2);
    }
}
