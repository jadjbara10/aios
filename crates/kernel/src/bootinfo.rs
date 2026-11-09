//! Kernel-side mirror of the bootloader's BootInfo.
//!
//! Received via RDI at '_start'. Phase 2 (AIOS-005) consumes the memory
//! map and framebuffer for the HAL + frame allocator; Phase 1 only
//! validates the version then proceeds to serial logging.
//!
//! MUST stay layout-identical to crates/bootloader/src/bootinfo.rs.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Framebuffer {
    pub addr: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32, // pixels per row
    pub bpp: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BootInfo {
    /// Layout version. Kernel rejects mismatches at _start.
    pub version: u64,
    pub framebuffer: Framebuffer,
    /// ACPI RSDP physical address (0 if not found).
    pub rsdp: u64,
    /// Pointer to the UEFI memory map buffer (phys == virt under UEFI).
    pub memory_map_ptr: u64,
    pub memory_map_len: u64,
    /// Top of the private 64 KiB stack the bootloader carved out.
    pub stack_top: u64,
}

impl BootInfo {
    pub const EXPECTED_VERSION: u64 = 1;
}
