//! AIOS Kernel - Binary Entry Point
//!
//! The UEFI bootloader transfers control here in 64-bit long mode with
//! paging enabled. We do NOT return; '_start' diverges.
//!
//! Phase 1 milestone: validate BootInfo handoff, emit "AIOS BOOT SUCCESS"
//! over COM1 serial.

#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(abi_x86_interrupt)]

mod bootinfo;
mod serial;
mod logger;
mod panic;

// Phase 2 subsystem modules (authored by the Wave-3 agent fan-out).
// memory: frame allocator + kernel heap + paging activation.
// interrupts: GDT/TSS + IDT + exception handlers + 8259 PIC.
// sched: task model + round-robin + IPC channels.
mod memory;
mod interrupts;
mod sched;

/// Kernel entry symbol.
///
/// # Safety
///
/// Must be invoked by the bootloader with:
///   - CPU in 64-bit long mode
///   - Paging enabled (UEFI identity map or AIOS PML4)
///   - Interrupts disabled
///   - RDI = pointer to a BootInfo structure (SysV AMD64 arg #0)
///
/// 'extern "C"' + '#[no_mangle]' preserve the exact symbol '_start'
/// so the linker script / bootloader can find it.
#[no_mangle]
pub extern "C" fn _start(boot_info: *const bootinfo::BootInfo) -> ! {
    // 1. Bring up the serial console FIRST - nothing else can be
    //    debugged without it.
    serial::init();
    logger::init();

    log::info!("==========================================");
    log::info!(" AIOS v0.1-alpha - Cognitive Kernel");
    log::info!(" Copyright (c) 2025 Jad Jbara");
    log::info!("==========================================");

    // 2. Validate the bootloader handoff contract before anything else.
    //
    // SAFETY: the bootloader (crates/bootloader) passes &BootInfo in RDI
    // per the SysV AMD64 ABI. A null pointer means a bare-metal test boot
    // (e.g. QEMU -kernel direct load) where no bootloader ran.
    if !boot_info.is_null() {
        let bi = unsafe { &*boot_info };
        if bi.version != bootinfo::BootInfo::EXPECTED_VERSION {
            log::error!(
                "[boot] BootInfo version mismatch: got {}, expected {}",
                bi.version,
                bootinfo::BootInfo::EXPECTED_VERSION
            );
        } else {
            log::info!("[boot] BootInfo v{} accepted", bi.version);
            log::info!(
                "[boot] framebuffer {}x{} @ {:#x} ({}bpp, stride {})",
                bi.framebuffer.width,
                bi.framebuffer.height,
                bi.framebuffer.addr,
                bi.framebuffer.bpp,
                bi.framebuffer.stride
            );
            log::info!("[boot] RSDP @ {:#x}", bi.rsdp);
        }
    } else {
        log::warn!("[boot] BootInfo pointer is null (direct-load test boot)");
    }

    log::info!("[boot] serial console online @ COM1 (115200 8N1)");
    log::info!("[boot] AIOS BOOT SUCCESS");

    // 3. Halt - Phase 2 installs GDT/IDT + scheduler here.
    halt_loop();
}

/// Halt the CPU indefinitely with interrupts disabled.
fn halt_loop() -> ! {
    loop {
        // SAFETY: 'hlt' halts the core until the next interrupt. With
        // interrupts disabled (the bootloader default) this is a hard
        // stop. No memory is touched.
        unsafe {
            core::arch::asm!("hlt", options(nostack, nomem, preserves_flags))
        };
    }
}

#[allow(dead_code)]
pub fn hcf() -> ! {
    halt_loop()
}
