//! crates/kernel/src/memory/mod.rs
//! AIOS v0.1 Alpha — Memory Subsystem root.
//!
//! Orchestrates the Phase 2 bring-up of the kernel's own memory substrate.
//! After the UEFI bootloader hands control to the kernel entry point, the
//! firmware-owned memory map and identity-mapped PML4 are only valid until
//! this module runs. 'init' performs the strict bring-up order:
//!
//!   1. frame_allocator::init — pin a bitmap over EfiConventionalMemory.
//!   2. heap::init              — bring the bump kernel heap online so the
//!                                rest of the kernel can alloc / Box.
//!   3. paging::activate        — swap CR3 from the UEFI identity map to
//!                                the AIOS-owned PML4 the bootloader built.
//!
//! Every later allocation in the kernel is capability-scoped: a Frame can
//! only be obtained through the frame allocator, and a heap region can
//! only be obtained through the #[global_allocator]. AI-generated sandbox
//! code never gets a raw Frame handle — it goes through the Phase 3
//! capability broker. This is the Immutable Core Principle applied to
//! memory.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![allow(dead_code)]

use crate::bootinfo::BootInfo;

pub mod frame_allocator;
pub mod heap;
pub mod paging;

/// Number of 4 KiB frames the bitmap can describe. Sized for a 64 GiB
/// physical address space; the bitmap itself costs 2 MiB of BSS.
pub const FRAME_BITMAP_BYTES: usize = 2 * 1024 * 1024;

/// Small-page size in bytes. AIOS uses 4 KiB pages at the kernel level;
/// 2 MiB huge pages are a Phase 3 optimisation on top of this base.
pub const PAGE_SIZE: usize = 4096;

/// Initialise the whole memory substrate in the strict bring-up order.
///
/// # Panics
/// Panics if any sub-stage fails: a failed frame-allocator init means we
/// cannot manage physical memory, a failed heap init means we cannot box,
/// and a failed paging activate means we are still on the UEFI identity
/// map (a security invariant we refuse to violate).
pub fn init(boot_info: &BootInfo) {
    // 1. Physical frame allocator — must precede heap and paging so both
    //    can request backing frames from us when they grow.
    frame_allocator::init(boot_info);
    log::info!("[memory] frame allocator online");

    // 2. Kernel heap — bump allocator over a static backing array. After
    //    this returns, alloc::vec::Vec, alloc::boxed::Box, etc. work.
    heap::init();
    log::info!("[memory] kernel heap online");

    // 3. Paging — swap CR3 to the AIOS-owned PML4. Until this point we
    //    are running on the bootloader's identity map, which is fine for
    //    boot but violates the security base (the kernel must own its
    //    own address space).
    paging::activate();
    log::info!("[memory] AIOS CR3 active — UEFI identity map off");
}

/// Convenience accessor used by higher subsystems (scheduler, agent
/// loader) to ask the kernel how many free 4 KiB frames remain.
pub fn free_frame_count() -> u64 {
    frame_allocator::free_frame_count()
}
