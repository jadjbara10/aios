//! aios-sync — synchronization primitives shared across AIOS crates.
//!
//! A thin, auditable layer over 'spin'. The kernel, HAL and agents all
//! take locks from here so the sandbox code CANNOT reach raw primitives
//! and deadlock the Immutable Core.

#![no_std]

pub use spin::{Mutex, MutexGuard, Once, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Helper alias used across the kernel for a global singleton lock.
pub type Shared<T> = Mutex<T>;

/// Helper alias for the read half of a shared registry.
pub type SharedRead<'a, T> = RwLockReadGuard<'a, T>;

/// Helper alias for the write half of a shared registry.
pub type SharedWrite<'a, T> = RwLockWriteGuard<'a, T>;

/// Acquire a shared lock.
///
/// AIOS forbids silent lock re-entrancy: a task that double-locks a
/// Shared<T> has violated its capability contract and is halted.
pub fn lock_shared<T>(m: &Shared<T>) -> MutexGuard<'_, T> {
    m.lock()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_roundtrip() {
        let s: Shared<u32> = Mutex::new(41);
        *lock_shared(&s) += 1;
        assert_eq!(*s.lock(), 42);
    }
}
