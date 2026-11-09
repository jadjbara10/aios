//! AIOS page-table builder.
//!
//! Phase 1 keeps UEFI's identity-mapped paging (CR3 = 0 at handoff).
//! This module prepares the Phase 2 (AIOS-005) structure: a fresh PML4
//! that identity-maps the first 1 GiB with 2 MiB huge pages - enough for
//! MMIO + low memory until the frame allocator rebuilds a full map.
//!
//! Activated by passing a non-zero 'cr3' to handoff_to_kernel.

use uefi::table::boot::{AllocateType, BootServices, MemoryType};

const PAGE_SIZE: usize = 4096;
const ENTRIES_PER_TABLE: usize = 512;

#[repr(C, align(4096))]
pub struct PageTables {
    pml4: [u64; ENTRIES_PER_TABLE],
    pdpt: [u64; ENTRIES_PER_TABLE],
    pd: [u64; ENTRIES_PER_TABLE],
}

const PRESENT: u64 = 1 << 0;
const WRITABLE: u64 = 1 << 1;
const HUGE: u64 = 1 << 7;

impl PageTables {
    /// Allocate a 4 KiB-aligned, zeroed PageTables in LOADER_DATA.
    pub fn alloc(bs: &BootServices) -> &'static mut PageTables {
        let pages = core::mem::size_of::<PageTables>() / PAGE_SIZE;
        let base = bs
            .allocate_pages(
                AllocateType::AnyPages,
                MemoryType::LOADER_DATA,
                pages,
            )
            .expect("page table alloc") as *mut PageTables;
        // SAFETY: freshly allocated LOADER_DATA pages, owned solely by us.
        // Zero manually so the layout is portable across UEFI impls.
        unsafe {
            core::ptr::write_bytes(
                base as *mut u8,
                0,
                core::mem::size_of::<PageTables>(),
            )
        };
        // SAFETY: base is non-null, 4 KiB-aligned, and zeroed above.
        unsafe { &mut *base }
    }

    /// Identity-map the first 1 GiB using 512 * 2 MiB huge pages.
    ///
    /// PML4[0] -> PDPT, PDPT[0] -> PD, PD[i] -> 2 MiB huge block at phys
    /// i * 2 MiB, flags present|writable|huge.
    pub fn build_identity(&mut self) {
        let pdpt_pa = &self.pdpt as *const _ as u64;
        let pd_pa = &self.pd as *const _ as u64;

        self.pml4[0] = pdpt_pa | PRESENT | WRITABLE;
        self.pdpt[0] = pd_pa | PRESENT | WRITABLE;

        for (i, entry) in self.pd.iter_mut().enumerate() {
            let phys = (i as u64) << 21; // 2 MiB granularity
            *entry = phys | PRESENT | WRITABLE | HUGE;
        }
    }

    /// Physical address to load into CR3 (passed to handoff_to_kernel).
    pub fn cr3(&self) -> u64 {
        &self.pml4 as *const _ as u64
    }
}
