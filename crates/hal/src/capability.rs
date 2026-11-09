//! crates/hal/src/capability.rs
//! Capability bus - typed permission tokens for device access.
//!
//! Every device operation mediated by the HAL requires a 'Capability<T>'
//! held by the calling task. The kernel is the sole entity permitted to
//! mint, grant, or revoke capabilities; agent code receives them via IPC
//! and can never forge a 'CapabilityToken' without 'unsafe'.
//!
//! This file does NOT link against the scheduler crate; 'TaskId' is a
//! generic newtype so the HAL stays scheduler-agnostic.
//!
//! Author: Jad Jbara <jad@aios.dev>

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

/// Lightweight task identifier. The HAL treats this as opaque; the kernel
/// supplies its own 'TaskId' type via a 'From' impl at the syscall layer.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(pub u64);

/// Opaque permission nonce. 'Copy' so capabilities pass by value through
/// IPC envelopes, but forging one requires 'unsafe' because the inner u64
/// is private and only 'CapabilityToken::mint' is public.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapabilityToken(u64);

impl CapabilityToken {
    /// Mint a brand-new token. Marked 'unsafe' because callers must
    /// guarantee the resulting token is never observable outside the
    /// kernel - forging a capability would bypass the entire capability
    /// bus invariant.
    ///
    /// # Safety
    /// The caller (kernel capability allocator) must assert 'nonce' is
    /// drawn from a CSPRNG and is not exposed to user tasks.
    pub unsafe fn mint(nonce: u64) -> Self {
        Self(nonce)
    }

    /// The raw u64. Public for serialization across IPC envelopes; not
    /// forgeable because 'Self' is not constructible without 'mint'.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

/// Typed capability handle bound to a device interface 'T'. Dropping the
/// handle does NOT revoke the underlying token; the kernel must explicitly
/// call 'CapabilityBus::revoke'.
pub struct Capability<T: ?Sized> {
    /// Permission nonce presented to 'CapabilityBus::check' on each use.
    pub token: CapabilityToken,
    /// Invariant: 'target' is a valid registered device id (see DEVICES).
    pub target: u32,
    /// Compile-time association with the device interface 'T'.
    _marker: core::marker::PhantomData<T>,
}

impl<T: ?Sized> Capability<T> {
    /// Construct a capability. 'unsafe' because the caller must hold the
    /// capability-bus lock and confirm 'target' is a registered device.
    ///
    /// # Safety
    /// The caller (kernel 'CapabilityBus') must hold the bus lock and
    /// must have verified 'target' exists in 'DEVICES'.
    pub unsafe fn new(token: CapabilityToken, target: u32) -> Self {
        Self {
            token,
            target,
            _marker: core::marker::PhantomData,
        }
    }

    /// The device id this capability addresses. Used by driver dispatch.
    pub fn target(&self) -> u32 {
        self.target
    }
}

/// Per-task capability table. The kernel owns one instance per task.
pub struct CapabilityBus {
    grants: BTreeMap<TaskId, Vec<(CapabilityToken, u32)>>,
}

impl CapabilityBus {
    /// Empty capability bus.
    pub const fn new() -> Self {
        Self { grants: BTreeMap::new() }
    }

    /// Grant a capability to 'task'. Only callable by the kernel.
    pub fn grant(&mut self, task: TaskId, token: CapabilityToken, target: u32) {
        self.grants.entry(task).or_default().push((token, target));
    }

    /// Revoke every capability held by 'task' whose token matches 'token'.
    /// Returns the number of capabilities revoked.
    pub fn revoke(&mut self, task: TaskId, token: CapabilityToken) -> usize {
        if let Some(v) = self.grants.get_mut(&task) {
            let before = v.len();
            v.retain(|(t, _)| *t != token);
            before - v.len()
        } else {
            0
        }
    }

    /// Revoke ALL capabilities held by 'task'. Used on task exit.
    pub fn revoke_all(&mut self, task: TaskId) -> usize {
        self.grants.remove(&task).map(|v| v.len()).unwrap_or(0)
    }

    /// Check whether 'task' holds 'token' against 'target'.
    pub fn check(&self, task: TaskId, token: CapabilityToken, target: u32) -> bool {
        self.grants
            .get(&task)
            .map(|v| v.iter().any(|(t, d)| *t == token && *d == target))
            .unwrap_or(false)
    }

    /// Count of capabilities held by 'task'. Used by the audit log.
    pub fn count(&self, task: TaskId) -> usize {
        self.grants.get(&task).map(|v| v.len()).unwrap_or(0)
    }
}

impl Default for CapabilityBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Global capability bus instance. Held under a spin lock; the kernel
/// acquires it before any grant/revoke/check.
pub static CAPABILITY_BUS: Mutex<CapabilityBus> = Mutex::new(CapabilityBus::new());

// -----------------------------------------------------------------------------
// End of crates/hal/src/capability.rs
// -----------------------------------------------------------------------------
