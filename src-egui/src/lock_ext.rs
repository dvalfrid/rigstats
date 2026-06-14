//! Poison-tolerant `Mutex` locking.
//!
//! RigStats runs continuously on a secondary monitor and spawns several
//! background threads (poll loop, tray, heartbeat, updater). If any thread
//! panics while holding a shared [`std::sync::Mutex`], the lock becomes
//! *poisoned* and every subsequent `.lock().unwrap()` — including on the UI
//! thread — would panic too, cascading a single fault into a full-app crash.
//!
//! [`LockSafe::lock_safe`] recovers the guard from a poisoned lock. The data
//! behind the mutex is still structurally valid (a panic mid-update is rare and
//! these guards only wrap plain state), so recovering it keeps the app alive
//! instead of taking down the whole UI.

use std::sync::{Mutex, MutexGuard};

pub trait LockSafe<T> {
    /// Lock the mutex, recovering the guard even if the lock was poisoned by a
    /// panic in another thread.
    fn lock_safe(&self) -> MutexGuard<'_, T>;
}

impl<T> LockSafe<T> for Mutex<T> {
    fn lock_safe(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn lock_safe_recovers_after_poison() {
        let m = Arc::new(Mutex::new(7_i32));
        let m2 = m.clone();
        // Poison the mutex by panicking while holding the guard.
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison the lock");
        })
        .join();
        assert!(m.lock().is_err(), "lock should be poisoned");
        // lock_safe still returns a usable guard with the preserved value.
        assert_eq!(*m.lock_safe(), 7);
        *m.lock_safe() = 9;
        assert_eq!(*m.lock_safe(), 9);
    }
}
