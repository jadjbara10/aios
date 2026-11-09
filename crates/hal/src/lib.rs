//! crates/hal/src/lib.rs
//! AIOS Hardware Abstraction Layer - trait surface.
//!
//! The HAL defines the *uniform* device interfaces that every driver -
//! whether handwritten or AI-generated - must implement. By keeping the
//! trait set intentionally small (block, IRQ, timer, serial), the kernel
//! can talk to any device through a stable vtable without naming the
//! concrete driver crate.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]
#![allow(dead_code)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use alloc::string::String;

pub mod capability;
pub mod device_tree;

pub use capability::{Capability, CapabilityBus, CapabilityToken, TaskId};
pub use device_tree::{DeviceDescriptor, DeviceKind, DeviceRegistry, DEVICES};

/// A block-addressable storage device (disk, SSD, virtual volume).
///
/// Implementations MUST fill 'buf' exactly; short reads are a contract
/// violation. The kernel does not retry on partial buffers.
pub trait BlockDevice {
    /// Read the block at 'lba' into 'buf'. buf.len() must equal block_size().
    fn read_block(&self, lba: u64, buf: &mut [u8]);
    /// Write 'buf' to the block at 'lba'. buf.len() must equal block_size().
    fn write_block(&self, lba: u64, buf: &[u8]);
    /// Constant block size in bytes (typically 512 or 4096).
    fn block_size(&self) -> u64;
}

/// Programmable interrupt controller (legacy PIC, APIC, or GIC).
pub trait InterruptController {
    /// Unmask and route 'irq' to the kernel IRQ stub.
    fn enable(&self, irq: u8);
    /// Mask 'irq'. Pending IRQs are dropped.
    fn disable(&self, irq: u8);
    /// Signal end-of-interrupt for 'irq' to the controller.
    fn eoi(&self, irq: u8);
}

/// Monotonic timer source (TSC, HPET, or architecture timer).
pub trait Timer {
    /// Current tick count in nanoseconds since an unspecified epoch.
    fn now_ns(&self) -> u64;
    /// Reprogram the timer tick frequency in Hertz.
    fn set_frequency(&self, hz: u32);
}

/// Byte-oriented serial console (16550 UART, E9 port, etc.).
pub trait SerialConsole {
    /// Write 'buf' to the serial line. Blocking until all bytes are flushed.
    fn write_bytes(&self, buf: &[u8]);
}

/// Convenience: write an ASCII str via any SerialConsole.
pub fn serial_write(console: &dyn SerialConsole, msg: &str) {
    console.write_bytes(msg.as_bytes());
}

/// Raw x86_64 port I/O helpers used by low-level drivers to implement the
/// HAL traits above. Feature-gated to x86_64 targets.
#[cfg(feature = "x86_64_io")]
pub mod port {
    use core::arch::asm;

    /// Write one byte to an I/O port.
    pub unsafe fn outb(port: u16, val: u8) {
        // SAFETY: 'out dx, al' is a single-byte I/O write. The caller
        // asserts 'port' is a valid hardware I/O address on x86_64 and
        // that no aliased mutable state is observed across the write.
        unsafe {
            asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
        }
    }

    /// Read one byte from an I/O port.
    pub unsafe fn inb(port: u16) -> u8 {
        let val: u8;
        // SAFETY: 'in al, dx' is a single-byte I/O read. The caller
        // asserts 'port' is a valid hardware I/O address on x86_64.
        unsafe {
            asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack, preserves_flags));
        }
        val
    }

    /// Stall the CPU for ~1us via 'out 0x80, al' (legacy diagnostic port).
    pub unsafe fn io_wait() {
        // SAFETY: Writing a discarded byte to diagnostic port 0x80 is the
        // canonical x86 I/O delay. The port is reserved on all PC-AT systems
        // and has no side effects beyond the timing delay.
        unsafe {
            asm!("out 0x80, al", in("al") 0u8, options(nomem, nostack, preserves_flags));
        }
    }
}

/// Human-readable name for a 'DeviceKind', used by the boot log.
pub fn kind_name(kind: DeviceKind) -> &'static str {
    use DeviceKind::*;
    match kind {
        Block => "block",
        Net => "net",
        Gpu => "gpu",
        Input => "input",
        Serial => "serial",
        Timer => "timer",
    }
}

/// Build a stable device id string of the form 'kind:vendor:device'.
pub fn device_id_str(d: &DeviceDescriptor) -> String {
    use alloc::format;
    format!("{}:{:04x}:{:04x}", kind_name(d.kind), d.vendor_id, d.device_id)
}

// -----------------------------------------------------------------------------
// End of crates/hal/src/lib.rs
// -----------------------------------------------------------------------------
