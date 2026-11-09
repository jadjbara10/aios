//! AIOS UEFI Bootloader - application entry & kernel handoff.
//!
//! Runs in the UEFI boot services environment (64-bit long mode, paging on,
//! identity-mapped). We:
//!   1. Bring up the UEFI logger (UEFI text output + COM1).
//!   2. Probe the Graphics Output Protocol for the framebuffer.
//!   3. Locate the RSDP via UEFI configuration tables.
//!   4. Snapshot the UEFI memory map.
//!   5. Load + relocate the embedded kernel ELF into LOADER_DATA pages.
//!   6. Allocate a private 64 KiB kernel stack.
//!   7. ExitBootServices - we now own the machine.
//!   8. Call the assembly handoff (entry.asm) which installs the fresh
//!      stack, sets RDI = &BootInfo, disables interrupts, jumps to _start.
//!
//! Targets uefi-rs 0.28. Minor API field names may shift across versions;
//! the structure is stable.

#![no_std]
#![no_main]
#![feature(abi_efiapi)]

extern crate alloc;

use log::info;
use uefi::prelude::*;
use uefi::table::boot::{AllocateType, MemoryType};
use uefi::table::cfg::{ACPI2_GUID, ACPI_GUID};

mod bootinfo;
mod elf;
mod entry; // NASM handoff (linked as a static symbol via build.rs)
mod page_tables;

/// Kernel ELF baked in at build time.
///
/// The Makefile 'boot-image' target compiles crates/kernel for the
/// x86_64-aios target and copies the ELF to build/aios-kernel.elf. We
/// embed it so the bootloader is a single self-contained .efi.
const KERNEL_ELF: &[u8] = include_bytes!("../../../build/aios-kernel.elf");

/// UEFI application entry point.
#[entry]
fn efi_main(image: Handle, mut st: SystemTable<Boot>) -> Status {
    // (1) Logger - uefi-services installs a global logger + panic handler.
    uefi_services::init(&mut st).expect("uefi logger init");

    info!("==========================================");
    info!(" AIOS v0.1-alpha - UEFI Bootloader");
    info!("==========================================");
    info!("[boot] UEFI services online");

    let bs = st.boot_services();

    // (2) Graphics Output Protocol - framebuffer for the AI GUI (Phase 7).
    let fb = framebuffer(bs);

    // (3) RSDP - needed later for ACPI / HAL interrupt routing.
    let rsdp = st
        .config_tables()
        .iter()
        .find(|t| t.guid == ACPI2_GUID || t.guid == ACPI_GUID)
        .map(|t| t.address as u64)
        .unwrap_or(0);
    info!("[boot] RSDP @ {:#x}", rsdp);

    // (4) Memory map - snapshot the buffer now; finalize after ExitBootServices.
    let map_size = bs.memory_map_size();
    let mut map_buf =
        vec![0u8; map_size.map_size + map_size.entry_size * 2];

    // (5) Load + relocate the kernel ELF into LOADER_DATA pages.
    let entry_va = elf::load(KERNEL_ELF, bs);
    info!("[boot] kernel entry VA @ {:#x}", entry_va);

    // (6) Allocate a private 64 KiB stack for the kernel.
    let stack_pages: usize = 16; // 16 * 4 KiB
    let stack_base = bs
        .allocate_pages(
            AllocateType::AnyPages,
            MemoryType::LOADER_DATA,
            stack_pages,
        )
        .expect("kernel stack alloc");
    let stack_top = stack_base + (stack_pages * 4096) as u64;

    // (7) Build the BootInfo the kernel consumes via RDI.
    let boot_info = bootinfo::BootInfo {
        version: bootinfo::BootInfo::EXPECTED_VERSION,
        framebuffer: fb,
        rsdp,
        memory_map_ptr: map_buf.as_ptr() as u64,
        memory_map_len: map_buf.len() as u64,
        stack_top,
    };
    let bi_addr = &boot_info as *const _ as u64;

    // (8) ExitBootServices. After this, only runtime services + our own
    //     code may run; boot services calls are forbidden.
    info!("[boot] exiting boot services...");
    let _rt = st
        .exit_boot_services(image)
        .expect("exit boot services");

    // (9) Hand off to the kernel via the assembly trampoline.
    //
    // SAFETY: boot services are exited; the kernel ELF is loaded and its
    // entry VA is valid under UEFI's identity mapping. The trampoline
    // installs a fresh stack, sets RDI = &BootInfo, disables interrupts,
    // and jumps to _start. This call never returns.
    //
    // cr3 = 0 keeps UEFI's identity-mapped paging (Phase 1). Phase 2
    // (AIOS-005) will pass page_tables::PageTables::cr3() to switch to
    // AIOS-owned tables.
    unsafe {
        entry::handoff_to_kernel(entry_va, bi_addr, 0, stack_top);
    }
}

/// Probe GOP and return a framebuffer descriptor.
fn framebuffer(bs: &BootServices) -> bootinfo::Framebuffer {
    let gop_handle = bs
        .get_handle_for_protocol::<uefi::proto::console::gop::GraphicsOutput>()
        .expect("GOP handle");
    let gop = bs
        .open_protocol::<uefi::proto::console::gop::GraphicsOutput>(gop_handle)
        .expect("open GOP");

    let mode = gop.current_mode_info();
    let (w, h) = mode.resolution();
    let fb_ptr = mode.fb().as_mut_ptr() as u64;
    bootinfo::Framebuffer {
        addr: fb_ptr,
        width: w,
        height: h,
        stride: mode.stride(),
        bpp: 32,
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("BOOTLOADER PANIC: {}", info);
    loop {
        // SAFETY: halt with interrupts disabled. No memory touched.
        unsafe {
            core::arch::asm!("cli; hlt", options(nostack, nomem))
        };
    }
}
