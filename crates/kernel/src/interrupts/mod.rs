//! AIOS interrupt subsystem root.
//!
//! Brings up the GDT/TSS, loads the IDT, installs the exception handlers
//! and remaps the 8259 PIC. APIC/IO-APIC routing is Phase 3 work.

pub mod gdt;
pub mod handlers;
pub mod idt;
pub mod pic;

use log::info;

/// One-shot bring-up called from the kernel boot path, after serial
/// logging is online and before the scheduler starts.
pub fn init() {
    gdt::load_gdt();
    gdt::load_tss();
    idt::load();
    pic::remap(0x20, 0x28);
    pic::mask_all();
    info!("[interrupts] GDT/TSS/IDT online, PIC remapped to 0x20/0x28");
}
