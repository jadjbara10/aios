//! Interrupt Descriptor Table for AIOS.
//!
//! A flat 256-entry IDT, each entry a 16-byte interrupt-gate
//! descriptor. Idt::new() zero-initializes the table;
//! set_handler(idx, handler) installs an extern "x86-interrupt"
//! handler; load() issues lidt to activate the table. Handlers live
//! in handlers.rs.
//!
//! NOTE: requires nightly + #![feature(abi_x86_interrupt)] at the
//! crate root for the extern "x86-interrupt" ABI used by handlers.
//! Also requires the bitflags 2.x crate for the const union below.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

use core::arch::asm;
use bitflags::bitflags;

/// Number of IDT entries (CPU-defined: vectors 0..=255).
pub const IDT_ENTRIES: usize = 256;

/// A single 16-byte IDT gate descriptor (interrupt / trap / task gate).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    /// Bits [0:15] of the handler RIP.
    offset_low: u16,
    /// GDT selector for the handler's code segment.
    selector: u16,
    /// Interrupt Stack Table index (0 = no IST switch).
    ist: u8,
    /// Gate type + DPL + present bit (see GateFlags).
    flags: u8,
    /// Bits [16:31] of the handler RIP.
    offset_middle: u16,
    /// Bits [32:63] of the handler RIP.
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            flags: 0,
            offset_middle: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    /// Patch this entry to point at handler with the given IST slot.
    fn set_handler(&mut self, handler: usize, ist: u8, flags: GateFlags) {
        self.offset_low = (handler & 0xFFFF) as u16;
        self.offset_middle = ((handler >> 16) & 0xFFFF) as u16;
        self.offset_high = ((handler >> 32) & 0xFFFF_FFFF) as u32;
        self.selector = super::gdt::KERNEL_CODE_SELECTOR;
        self.ist = ist & 0x07;
        self.flags = flags.bits();
    }
}

bitflags! {
    /// IDT gate-type flags packed into the descriptor's flags byte.
    pub struct GateFlags: u8 {
        /// 64-bit interrupt gate -- the canonical long-mode setting.
        const INTERRUPT_64 = 0x0E;
        /// 64-bit trap gate (used by debug/BP).
        const TRAP_64 = 0x0F;
        /// Descriptor Privilege Level = 3 (callable from user space).
        const DPL_3 = 0x60;
        /// Present bit -- must be set for a live gate.
        const PRESENT = 0x80;
    }
}

/// A present ring-0 64-bit interrupt gate.
pub const INTERRUPT_GATE: GateFlags = GateFlags::INTERRUPT_64.union(GateFlags::PRESENT);

/// The 256-entry IDT, 16-byte aligned as required by the CPU.
#[repr(C, align(16))]
pub struct Idt {
    entries: [IdtEntry; IDT_ENTRIES],
}

impl Idt {
    /// Build a zero-initialized IDT -- every gate is "missing" until
    /// set_handler() patches it.
    pub const fn new() -> Self {
        Self {
            entries: [IdtEntry::missing(); IDT_ENTRIES],
        }
    }

    /// Install handler at vector idx with IST=0 and a present ring-0
    /// 64-bit interrupt-gate flags byte.
    pub fn set_handler(&mut self, idx: usize, handler: usize) {
        self.set_handler_with(idx, handler, 0, INTERRUPT_GATE);
    }

    /// Install handler at vector idx with an explicit IST slot and
    /// gate flags. Used for #DF (IST=1) and user-callable gates.
    pub fn set_handler_with(&mut self, idx: usize, handler: usize, ist: u8, flags: GateFlags) {
        assert!(idx < IDT_ENTRIES);
        self.entries[idx].set_handler(handler, ist, flags);
    }

    /// Activate this IDT via lidt. The table must outlive every
    /// interrupt taken after this call -- typically placed in a static.
    ///
    /// # Safety
    ///
    /// lidt is a privileged ring-0 instruction. Caller must own the
    /// Idt for the lifetime of the system (place it in a static).
    pub unsafe fn load(&self) {
        let desc = Idtr {
            limit: (core::mem::size_of::<Self>() - 1) as u16,
            base: self as *const Self as u64,
        };
        // SAFETY: ring-0; desc lives on the stack for the duration of
        // the asm block (its address escapes only as far as lidt).
        let desc_ptr = &desc;
        asm!(
            "lidt [{}]",
            in(reg) desc_ptr,
            options(nostack, preserves_flags),
        );
    }
}

/// The 10-byte IDT register pseudo-descriptor used by lidt/sidt.
#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

/// Convenience: a single global IDT placed in a static. The kernel's
/// init() fn patches entries, then calls IDT.load() on every CPU.
pub static mut IDT: Idt = Idt::new();
