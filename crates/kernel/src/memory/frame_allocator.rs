//! crates/kernel/src/memory/frame_allocator.rs
//! AIOS v0.1 Alpha — Bitmap frame allocator over the UEFI memory map.
//!
//! The UEFI firmware hands us a memory map of EfiMemoryDescriptor entries
//! just before ExitBootServices. We walk it once, mark every
//! EfiConventionalMemory region's 4 KiB frames as free in a static
//! bitmap, and hand out frames from there. Allocation is a linear scan
//! for the first clear bit; deallocation clears the bit. This is
//! intentionally simple — Phase 3 will replace it with a buddy allocator
//! plus per-CPU freelists.
//!
//! The bitmap lives in BSS (2 MiB) and is indexed by frame number, where
//! frame number = physical_address / PAGE_SIZE. The bitmap therefore
//! covers the first 64 GiB of physical RAM, which is enough for the
//! Alpha target hardware. Frames above 64 GiB are ignored (logged once).
//!
//! Author: Jad Jbara <jad@aios.dev>

use crate::bootinfo::{BootInfo, EfiMemoryDescriptor, EFI_MEMORY_CONVENTIONAL};
use crate::memory::PAGE_SIZE;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// 2 MiB backing bitmap (one bit per 4 KiB frame -> 64 GiB addressable).
static mut BITMAP: [u8; super::FRAME_BITMAP_BYTES] = [0u8; super::FRAME_BITMAP_BYTES];

/// Set once init runs; guards against double-init and use-before-init.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Total free frames remaining — bumped down on alloc, up on free.
static FREE_FRAMES: AtomicU64 = AtomicU64::new(0);

/// Highest frame index that has ever been marked free. The allocator
/// scans from 0 up to this watermark on every alloc, which keeps the
/// scan cheap early in boot when only low memory is in use.
static HIGH_WATERMARK: AtomicU64 = AtomicU64::new(0);

/// Initialise the bitmap from the UEFI memory map.
///
/// # Safety
/// Caller guarantees boot_info.memory_map_ptr points to a contiguous
/// array of boot_info.memory_map_len EfiMemoryDescriptor entries that
/// remain valid for the lifetime of this call (they do — we are post
/// ExitBootServices and the LOADER_DATA region is still identity-mapped).
pub fn init(boot_info: &BootInfo) {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        panic!("[memory] frame_allocator::init called twice");
    }

    let descriptors = unsafe {
        // SAFETY: BootInfo contract — memory_map_ptr is a valid
        // LOADER_DATA pointer, memory_map_len is the descriptor count,
        // and the descriptor size matches sizeof(EfiMemoryDescriptor).
        // The bootloader validated this in exit_boot_services before
        // the entry.asm handoff to _start.
        core::slice::from_raw_parts(
            boot_info.memory_map_ptr as *const EfiMemoryDescriptor,
            boot_info.memory_map_len,
        )
    };

    let mut marked: u64 = 0;
    for desc in descriptors {
        if desc.memory_type != EFI_MEMORY_CONVENTIONAL {
            continue;
        }
        let start = desc.physical_start / PAGE_SIZE as u64;
        let count = desc.number_of_pages;
        for i in 0..count {
            let frame = start + i;
            if set_free(frame) {
                marked += 1;
                if frame > HIGH_WATERMARK.load(Ordering::Relaxed) {
                    HIGH_WATERMARK.store(frame, Ordering::Relaxed);
                }
            }
        }
    }

    FREE_FRAMES.store(marked, Ordering::SeqCst);
    log::info!(
        "[memory] frame allocator: {} frames free across bitmap",
        marked
    );
}

/// Allocate one 4 KiB frame. Returns the physical address, or None if
/// the bitmap is exhausted (or before init has run).
pub fn alloc_frame() -> Option<u64> {
    if !INITIALIZED.load(Ordering::SeqCst) {
        return None;
    }
    let limit = HIGH_WATERMARK.load(Ordering::Acquire);
    let mut idx: u64 = 0;
    while idx <= limit {
        if let Some(frame) = try_claim(idx) {
            FREE_FRAMES.fetch_sub(1, Ordering::Relaxed);
            return Some(frame * PAGE_SIZE as u64);
        }
        idx += 1;
    }
    None
}

/// Return a frame to the pool. Double-free is logged but tolerated.
pub fn free_frame(frame: u64) {
    let idx = frame / PAGE_SIZE as u64;
    if set_free(idx) {
        FREE_FRAMES.fetch_add(1, Ordering::Relaxed);
    } else {
        log::warn!("[memory] double free of frame {:#x}", frame);
    }
}

/// Snapshot of how many 4 KiB frames are currently free.
pub fn free_frame_count() -> u64 {
    FREE_FRAMES.load(Ordering::Acquire)
}

/// Try to atomically claim frame idx. Returns Some(idx) on success.
fn try_claim(idx: u64) -> Option<u64> {
    let byte = idx / 8;
    let bit = (idx % 8) as u8;
    if byte as usize >= super::FRAME_BITMAP_BYTES {
        return None;
    }
    let cell = unsafe {
        // SAFETY: BITMAP is 'static, byte index is bounds-checked above.
        // Multiple CPUs cannot race here yet — Phase 2 is uniprocessor.
        &mut BITMAP[byte as usize]
    };
    let mask = 1u8 << bit;
    if *cell & mask != 0 {
        // Free — claim it.
        *cell &= !mask;
        Some(idx)
    } else {
        None
    }
}

/// Mark frame idx as free. Returns true if it was previously allocated.
fn set_free(idx: u64) -> bool {
    let byte = idx / 8;
    let bit = (idx % 8) as u8;
    if byte as usize >= super::FRAME_BITMAP_BYTES {
        return false;
    }
    let cell = unsafe {
        // SAFETY: BITMAP is 'static, byte index is bounds-checked above.
        &mut BITMAP[byte as usize]
    };
    let mask = 1u8 << bit;
    let was = *cell & mask == 0;
    *cell |= mask;
    was
}
