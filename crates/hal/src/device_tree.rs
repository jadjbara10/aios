//! crates/hal/src/device_tree.rs
//! Runtime registry of discovered devices.
//!
//! The HAL does not probe hardware itself - that is the job of the
//! driver-gen pipeline. Instead, the HAL provides a stable 'DeviceRegistry'
//! behind a 'spin::Mutex' into which driver crates insert 'DeviceDescriptor's
//! as they bring devices online. The kernel then enumerates the registry to
//! build its device table and to route capability grants.
//!
//! Author: Jad Jbara <jad@aios.dev>

extern crate alloc;

use alloc::vec::Vec;
use spin::Mutex;

/// Coarse classification of a discovered device.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    /// Block-addressable storage (NVMe, AHCI, virtio-blk).
    Block,
    /// Network interface (virtio-net, e1000).
    Net,
    /// GPU / framebuffer controller.
    Gpu,
    /// Human-input device (keyboard, mouse, touch).
    Input,
    /// Serial console (16550 UART).
    Serial,
    /// Monotonic timer (HPET, TSC, local APIC timer).
    Timer,
}

/// Stable descriptor for a discovered device.
#[derive(Copy, Clone, Debug)]
pub struct DeviceDescriptor {
    /// Opaque id assigned by the registering driver. For PCI devices this
    /// is the packed segment:bus:device:function address.
    pub id: u32,
    /// Coarse device kind. Drives the HAL trait the driver must implement.
    pub kind: DeviceKind,
    /// PCI vendor id (0xFFFF for non-PCI devices).
    pub vendor_id: u16,
    /// PCI device id (0xFFFF for non-PCI devices).
    pub device_id: u16,
    /// Capability bitmask granted to the device at registration time.
    pub caps: u64,
}

/// PCI segment:bus:device:function packed into a u32 for compact lookup.
pub type PciAddr = u32;

/// Pack a BDF into a 'PciAddr'.
pub const fn pack_bdf(seg: u16, bus: u8, dev: u8, func: u8) -> PciAddr {
    ((seg as u32) << 16) | ((bus as u32) << 8) | ((dev as u32) << 3) | (func as u32)
}

/// Runtime device registry. Protected by a spin lock because drivers and
/// the kernel may register/enumerate from different CPU contexts.
pub struct DeviceRegistry {
    entries: Vec<DeviceDescriptor>,
}

impl DeviceRegistry {
    /// Construct an empty registry.
    pub const fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Append a descriptor. Returns the assigned index (used as a handle).
    pub fn register(&mut self, d: DeviceDescriptor) -> usize {
        let idx = self.entries.len();
        self.entries.push(d);
        idx
    }

    /// Borrow every registered descriptor in registration order.
    pub fn enumerate(&self) -> &[DeviceDescriptor] {
        &self.entries
    }

    /// Find the first descriptor whose 'id' field matches 'addr'.
    /// For PCI devices 'addr' is the packed BDF; for non-PCI devices it
    /// is the driver-assigned opaque id.
    pub fn probe(&self, addr: PciAddr) -> Option<&DeviceDescriptor> {
        // Linear scan is fine for v0.1 alpha (single-digit device counts).
        self.entries.iter().find(|d| d.id as PciAddr == addr)
    }

    /// Number of currently-registered devices.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True iff the registry holds no descriptors.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global registry instance. Drivers insert here during init; the kernel
/// enumerates here when building the device table. The spin lock is
/// acquired IRQ-disabled by the kernel before invoking driver init.
pub static DEVICES: Mutex<DeviceRegistry> = Mutex::new(DeviceRegistry::new());

/// Register a descriptor against the global 'DEVICES' registry.
/// Returns the assigned handle index.
pub fn global_register(d: DeviceDescriptor) -> usize {
    DEVICES.lock().register(d)
}

/// Count of currently-registered devices. Used by the boot log banner.
pub fn global_count() -> usize {
    DEVICES.lock().len()
}

// -----------------------------------------------------------------------------
// End of crates/hal/src/device_tree.rs
// -----------------------------------------------------------------------------
