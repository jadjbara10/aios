//! Global Descriptor Table + Task State Segment for AIOS.
//!
//! Phase 2: flat 5-entry GDT (null, kernel code 0x9A, kernel data
//! 0x92, TSS low/high) + 64-bit TSS with IST[0] reserved for the
//! double-fault stack so a stack overflow can't tear down #DF.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

/// Selector offsets into the GDT (byte granularity).
pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
pub const TSS_SELECTOR: u16 = 0x18;

/// Bytes reserved for the double-fault IST stack.
pub const DOUBLE_FAULT_STACK_SIZE: usize = 4096 * 4;

/// The 64-bit Task State Segment. CPU ignores most fields in long
/// mode, but the IST and IOPB layout must be exact.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct TaskStateSegment {
    reserved0: u32,
    pub privilege_stack_table: [u64; 3],
    pub interrupt_stack_table: [u64; 7],
    reserved1: u64,
    reserved2: u16,
    pub iomap_base: u16,
}

impl TaskStateSegment {
    pub const fn new() -> Self {
        Self {
            reserved0: 0, privilege_stack_table: [0; 3],
            interrupt_stack_table: [0; 7], reserved1: 0, reserved2: 0,
            iomap_base: core::mem::size_of::<Self>() as u16,
        }
    }
}

/// One 8-byte GDT entry.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
}

impl GdtEntry {
    const fn null() -> Self {
        Self { limit_low: 0, base_low: 0, base_middle: 0, access: 0, granularity: 0, base_high: 0 }
    }

    /// Flat 64-bit ring-0 segment. access: 0x9A = code, 0x92 = data.
    const fn segment(access: u8, granularity: u8) -> Self {
        Self { limit_low: 0, base_low: 0, base_middle: 0, access, granularity, base_high: 0 }
    }

    /// Low half of a 64-bit TSS system-segment descriptor.
    const fn tss_low(base: u64, limit: u64) -> Self {
        Self {
            limit_low: (limit & 0xFFFF) as u16,
            base_low: (base & 0xFFFF) as u16,
            base_middle: ((base >> 16) & 0xFF) as u8,
            access: 0x89, // present | TSS available
            granularity: ((limit >> 16) & 0x0F) as u8,
            base_high: ((base >> 24) & 0xFF) as u8,
        }
    }
}

/// High half of the 64-bit TSS descriptor: bits [32:63] of base.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct GdtEntryHigh { base_high32: u32, reserved: u32 }

impl GdtEntryHigh {
    const fn tss_high(base: u64) -> Self {
        Self { base_high32: ((base >> 32) & 0xFFFF_FFFF) as u32, reserved: 0 }
    }
}

/// Pseudo-descriptor handed to lgdt.
#[repr(C, packed)]
struct GdtDescriptor { limit: u16, base: u64 }

/// 5-entry GDT layout: null, kernel code, kernel data, TSS low, TSS high.
#[repr(C, packed)]
struct GdtTable {
    null: GdtEntry,
    kernel_code: GdtEntry,
    kernel_data: GdtEntry,
    tss_low: GdtEntry,
    tss_high: GdtEntryHigh,
}

static mut GDT: GdtTable = GdtTable {
    null: GdtEntry::null(),
    kernel_code: GdtEntry::segment(0x9A, 0x20),
    kernel_data: GdtEntry::segment(0x92, 0x00),
    tss_low: GdtEntry::null(),
    tss_high: GdtEntryHigh { base_high32: 0, reserved: 0 },
};

static mut TSS: TaskStateSegment = TaskStateSegment::new();
static mut DOUBLE_FAULT_STACK: [u8; DOUBLE_FAULT_STACK_SIZE] = [0; DOUBLE_FAULT_STACK_SIZE];
static GDT_LOADED: AtomicBool = AtomicBool::new(false);

/// Install the GDT, populate TSS descriptors, reload segment registers.
///
/// # Safety
/// Caller must be on the boot CPU in ring 0 with interrupts disabled.
pub unsafe fn load_gdt() {
    // SAFETY: ring-0 early boot; we own the GDT/TSS statics
    // exclusively (pre-SMP, no other core observes them yet).
    let tss_addr = core::ptr::addr_of!(TSS) as u64;
    let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;

    // SAFETY: writing IST[0] before ltr is safe -- the field is
    // ignored by the CPU until ltr selects the descriptor.
    (*core::ptr::addr_of_mut!(TSS)).interrupt_stack_table[0] =
        DOUBLE_FAULT_STACK.as_ptr() as usize as u64 + DOUBLE_FAULT_STACK_SIZE as u64;

    // SAFETY: patching TSS descriptors with the runtime address of
    // the TSS static; no other core observes GDT yet.
    (*core::ptr::addr_of_mut!(GDT)).tss_low = GdtEntry::tss_low(tss_addr, tss_limit);
    (*core::ptr::addr_of_mut!(GDT)).tss_high = GdtEntryHigh::tss_high(tss_addr);

    let gd = GdtDescriptor {
        limit: (core::mem::size_of::<GdtTable>() - 1) as u16,
        base: core::ptr::addr_of!(GDT) as u64,
    };

    // SAFETY: lgdt is a privileged ring-0 instruction; the descriptor
    // points at our packed GDT in kernel memory. Interrupts are off.
    let gd_ptr = &gd;
    asm!("lgdt [{}]", in(reg) gd_ptr, options(nostack, preserves_flags));

    // SAFETY: reload data segments against the new GDT. The flat
    // ring-0 data selector (0x10) covers the entire address space.
    asm!(
        "mov ds, {ds}", "mov es, {ds}", "mov fs, {ds}",
        "mov gs, {ds}", "mov ss, {ds}",
        ds = in(reg) KERNEL_DATA_SELECTOR,
        options(preserves_flags),
    );

    // SAFETY: reload CS via a far return. We push the new CS, then
    // the continuation RIP, then retfq. 1: is the continuation label.
    asm!(
        "push {cs}",
        "lea {tmp}, [rip + 1f]",
        "push {tmp}",
        "retfq",
        "1:",
        cs = in(reg) KERNEL_CODE_SELECTOR,
        tmp = inout(reg) 0u64,
        options(preserves_flags),
    );

    GDT_LOADED.store(true, Ordering::SeqCst);
}

/// Load the TSS selector into TR. Must run after load_gdt().
///
/// # Safety
/// Issues ltr; caller must be ring 0 with the GDT installed.
pub unsafe fn load_tss() {
    // SAFETY: ltr is privileged; TSS_SELECTOR (0x18) indexes the
    // two TSS descriptor slots patched by load_gdt().
    asm!("ltr {sel}", sel = in(reg) TSS_SELECTOR, options(nostack, preserves_flags));
}
