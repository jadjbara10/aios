//! crates/kernel/src/memory/paging.rs
//! AIOS v0.1 Alpha — Paging activation and manual PTE manipulation.
//!
//! At handoff the CPU is running on the bootloader's identity-mapped
//! PML4. The bootloader (crates/bootloader/src/page_tables.rs) already
//! built an AIOS-owned PML4 (high-half kernel image + identity-mapped
//! first 1 GiB with 2 MiB huge pages) and exposed it via cr3().
//! activate swaps CR3 to that PML4, ending the UEFI identity-map era.
//!
//! After activation, kernel code can call map_page / unmap_page to edit
//! the live PML4 — used by the Phase 3 frame-allocator-backed mapper for
//! agent sandboxes and mmap'd agent heaps. All edits are unsynchronised
//! in Phase 2 (uniprocessor); Phase 4 will add a TLB-shootdown IPI.
//!
//! Author: Jad Jbara <jad@aios.dev>

use core::ptr::NonNull;

/// Page-table entry flags. Mirrors x86_64 bit assignments.
pub const FLAG_PRESENT: u64 = 1 << 0;
pub const FLAG_WRITABLE: u64 = 1 << 1;
pub const FLAG_USER: u64 = 1 << 2;
pub const FLAG_HUGE: u64 = 1 << 7;

/// 9 bits per level, 4 levels (PML4 / PDPT / PD / PT).
const PT_SHIFT: u32 = 12;
const PT_ENTRIES: usize = 512;

/// The AIOS-owned PML4 physical address. Captured by activate so later
/// map_page / unmap_page calls know which table tree to edit. Phase 2
/// is uniprocessor with interrupts off, so a plain static is safe.
static mut AIOS_CR3: u64 = 0;

/// Activate the AIOS-owned PML4 by loading CR3.
///
/// # Safety
/// Caller (the boot init sequence) guarantees that the bootloader's
/// cr3() returns the physical address of a valid PML4 whose identity
/// mappings cover the currently-executing code — otherwise we would
/// page-fault the instant after mov cr3, rax retires. We also require
/// that interrupts are off (they are — init runs with RFLAGS.IF=0 from
/// the entry.asm handoff).
pub fn activate() {
    let bootloader_cr3 = crate::bootinfo::bootloader_cr3();
    if bootloader_cr3 == 0 {
        panic!("[memory] paging::activate: bootloader did not supply a CR3");
    }
    // SAFETY: bootloader_cr3 is the physical address of a PML4 the
    // bootloader constructed and validated before ExitBootServices.
    // Reloading CR3 with it is a no-op for the running code (it is
    // already the live root) but it transitions ownership from
    // firmware-built, firmware-owned to AIOS-owned. We require
    // RFLAGS.IF=0 (true at this boot stage).
    unsafe {
        core::arch::asm!(
            "mov cr3, rax",
            in("rax") bootloader_cr3,
            options(nostack, preserves_flags),
        );
    }
    // SAFETY: AIOS_CR3 is a static u64; Phase 2 is uniprocessor with
    // interrupts off, so the store cannot race with another writer.
    unsafe { AIOS_CR3 = bootloader_cr3 };
    log::info!("[memory] CR3 <- {:#x} (AIOS PML4 active)", bootloader_cr3);
}

/// Map vaddr -> paddr with the given flags in the live AIOS PML4.
///
/// Allocates intermediate PDPT/PD/PT entries from the frame allocator
/// as needed. Returns the leaf PTE pointer for caller inspection, or
/// None if a needed intermediate could not be allocated.
pub fn map_page(vaddr: u64, paddr: u64, flags: u64) -> Option<NonNull<u64>> {
    let cr3 = unsafe {
        // SAFETY: AIOS_CR3 is set by activate() and never modified
        // after; we only read it here.
        AIOS_CR3
    };
    if cr3 == 0 {
        return None;
    }
    let pml4_index = ((vaddr >> 39) & 0x1FF) as usize;
    let pdpt_index = ((vaddr >> 30) & 0x1FF) as usize;
    let pd_index = ((vaddr >> 21) & 0x1FF) as usize;
    let pt_index = ((vaddr >> PT_SHIFT) & 0x1FF) as usize;

    let pml4 = unsafe {
        // SAFETY: cr3 is the physical address of a 4 KiB PML4. At Phase
        // 2 we are still identity-mapped over the first 1 GiB, so
        // phys == virt for the PML4 base. Phase 3 will add a recursive
        // PML4 mapping for high-half edits.
        NonNull::new(cr3 as *mut u64).unwrap_unchecked()
    };

    let pdpt = walk_or_alloc(pml4, pml4_index, flags)?;
    let pd = walk_or_alloc(pdpt, pdpt_index, flags)?;
    let pt = walk_or_alloc(pd, pd_index, flags)?;

    // SAFETY: pt points to a 512-entry PT in identity-mapped memory and
    // pt_index < PT_ENTRIES, so the resulting pointer is in-bounds.
    let pte_ptr = unsafe { pt.as_ptr().add(pt_index) };
    // SAFETY: pte_ptr is within the PT frame we just resolved. We are
    // uniprocessor with interrupts off — no aliasing race is possible.
    unsafe {
        pte_ptr.write(paddr | flags | FLAG_PRESENT);
    }
    // SAFETY: pte_ptr is non-null — we just dereferenced it above.
    Some(unsafe { NonNull::new_unchecked(pte_ptr) })
}

/// Unmap vaddr. Leaves intermediate tables in place (Phase 3 will
/// reclaim them). After this returns, accessing vaddr page-faults.
pub fn unmap_page(vaddr: u64) {
    let cr3 = unsafe {
        // SAFETY: AIOS_CR3 set by activate(); read-only here.
        AIOS_CR3
    };
    if cr3 == 0 {
        return;
    }
    let pml4_index = ((vaddr >> 39) & 0x1FF) as usize;
    let pdpt_index = ((vaddr >> 30) & 0x1FF) as usize;
    let pd_index = ((vaddr >> 21) & 0x1FF) as usize;
    let pt_index = ((vaddr >> PT_SHIFT) & 0x1FF) as usize;

    let pml4 = unsafe {
        // SAFETY: see map_page — identity-mapped PML4 base.
        NonNull::new(cr3 as *mut u64).unwrap_unchecked()
    };
    let pml4e = unsafe { pml4.as_ptr().add(pml4_index).read() };
    if pml4e & FLAG_PRESENT == 0 {
        return;
    }
    let pdpt = NonNull::new((pml4e & !0xFFFu64) as *mut u64).unwrap_unchecked();
    let pdpte = unsafe { pdpt.as_ptr().add(pdpt_index).read() };
    if pdpte & FLAG_PRESENT == 0 {
        return;
    }
    let pd = NonNull::new((pdpte & !0xFFFu64) as *mut u64).unwrap_unchecked();
    let pde = unsafe { pd.as_ptr().add(pd_index).read() };
    if pde & FLAG_PRESENT == 0 {
        return;
    }
    let pt = NonNull::new((pde & !0xFFFu64) as *mut u64).unwrap_unchecked();
    let pte_ptr = unsafe { pt.as_ptr().add(pt_index) };
    // SAFETY: pte_ptr points to a live PT entry; clearing it unmaps
    // vaddr. We then invalidate the local TLB entry for vaddr.
    unsafe {
        pte_ptr.write(0);
        core::arch::asm!(
            "invlpg [rax]",
            in("rax") vaddr,
            options(nostack, preserves_flags),
        );
    }
}

/// Walk one level of the page table: if the entry at index is present,
/// return its child frame; otherwise allocate a new zeroed frame from
/// the frame allocator, install it, and return it.
fn walk_or_alloc(table: NonNull<u64>, index: usize, flags: u64) -> Option<NonNull<u64>> {
    // SAFETY: index < PT_ENTRIES (caller masks with 0x1FF); the table
    // frame is identity-mapped so the computed pointer is valid.
    let entry_ptr = unsafe { table.as_ptr().add(index) };
    let entry = unsafe { entry_ptr.read() };
    if entry & FLAG_PRESENT != 0 {
        return NonNull::new((entry & !0xFFFu64) as *mut u64);
    }
    let frame = crate::memory::frame_allocator::alloc_frame()?;
    // SAFETY: alloc_frame returns a 4 KiB-aligned physical frame that
    // is identity-mapped. Zero it before installing so stale UEFI data
    // does not leak into our page tables.
    unsafe {
        core::ptr::write_bytes(frame as *mut u8, 0, 4096);
        entry_ptr.write(frame | flags | FLAG_PRESENT | FLAG_WRITABLE);
    }
    NonNull::new(frame as *mut u64)
}
