//! Handoff structure passed from bootloader to kernel via RDI.
//!
//! MUST stay layout-identical to crates/kernel/src/bootinfo.rs.
//! Versioned so the kernel can reject incompatible layouts at _start.

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
    /// Layout version. Kernel checks this == EXPECTED_VERSION at _start.
    pub version: u64,
    pub framebuffer: Framebuffer,
    /// ACPI RSDP physical address (0 if not found in config tables).
    pub rsdp: u64,
    /// Pointer to the UEFI memory map buffer (phys == virt under UEFI).
    pub memory_map_ptr: u64,
    pub memory_map_len: u64,
    /// Top of the private 64 KiB stack allocated for the kernel.
    pub stack_top: u64,
}

impl BootInfo {
    pub const EXPECTED_VERSION: u64 = 1;
}
