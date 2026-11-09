//! Legacy 8259 Programmable Interrupt Controller driver for AIOS.
//!
//! Phase 2 keeps the legacy 8259 PIC alive for early boot and as a
//! fallback before the local APIC is initialized (APIC lands in
//! Phase 3 / AIOS-010). remap() steers master IRQs to vectors
//! 0x20..0x27 and slave IRQs to 0x28..0x2F, avoiding collision with
//! CPU exceptions (vectors 0..0x1F). end_of_interrupt(irq) issues
//! the OCW2 non-specific EOI.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

use core::arch::asm;

/// 8259 master PIC I/O ports.
const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
/// 8259 slave PIC I/O ports.
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

/// ICW1: expect ICW4, cascade mode, interval=8.
const ICW1_ICW4: u8 = 0x10;
const ICW1_INIT: u8 = 0x01;
/// ICW4: 8086 mode, auto-EOI off, not buffered.
const ICW4_8086: u8 = 0x01;
/// OCW2 non-specific end-of-interrupt.
const OCW2_EOI: u8 = 0x20;

/// The pair of 8259 PICs (master + slave) as a single driver.
pub struct Pic;

impl Pic {
    pub const fn new() -> Self {
        Self
    }

    /// Remap master IRQs to offset_master..offset_master+8 and slave
    /// IRQs to offset_slave..offset_slave+8. Must run with
    /// interrupts disabled. Standard choice: master=0x20, slave=0x28.
    ///
    /// # Safety
    ///
    /// Issues raw out I/O port writes; safe only in ring 0.
    pub unsafe fn remap(&self, offset_master: u8, offset_slave: u8) {
        // SAFETY: ring-0 I/O writes; the sequence below is the
        // canonical IBM-PC/AT 8259 init dance (ICW1..ICW4).
        let saved_m = inb(PIC1_DATA);
        let saved_s = inb(PIC2_DATA);

        // SAFETY: ICW1 starts the init sequence on both PICs.
        outb(PIC1_CMD, ICW1_INIT | ICW1_ICW4);
        io_wait();
        outb(PIC2_CMD, ICW1_INIT | ICW1_ICW4);
        io_wait();

        // SAFETY: ICW2 sets the vector-offset base for each PIC.
        outb(PIC1_DATA, offset_master);
        io_wait();
        outb(PIC2_DATA, offset_slave);
        io_wait();

        // SAFETY: ICW3 encodes the cascade wiring -- master sees
        // slave on IRQ2 (bit 2), slave responds on cascade input 2.
        outb(PIC1_DATA, 0x04);
        io_wait();
        outb(PIC2_DATA, 0x02);
        io_wait();

        // SAFETY: ICW4 selects 8086 mode, auto-EOI off.
        outb(PIC1_DATA, ICW4_8086);
        io_wait();
        outb(PIC2_DATA, ICW4_8086);
        io_wait();

        // SAFETY: restore the saved IMR; mask everything by default
        // (callers can unmask per-IRQ later).
        outb(PIC1_DATA, saved_m);
        outb(PIC2_DATA, saved_s);
    }

    /// Mask every IRQ on both PICs.
    ///
    /// # Safety
    ///
    /// Raw I/O port writes; ring 0 only.
    pub unsafe fn mask_all(&self) {
        // SAFETY: writing 0xFF to the IMR masks all 8 IRQs per PIC.
        outb(PIC1_DATA, 0xFF);
        outb(PIC2_DATA, 0xFF);
    }

    /// Issue non-specific EOI for irq (0..15). Must be called at the
    /// end of every legacy-IRQ handler before iretq.
    ///
    /// # Safety
    ///
    /// Raw I/O port writes; ring 0 only.
    pub unsafe fn end_of_interrupt(&self, irq: u8) {
        // SAFETY: if the IRQ came from the slave (8..15), EOI the
        // slave first, then EOI the master -- the canonical 8259
        // cascade end-of-interrupt sequence.
        if irq >= 8 {
            outb(PIC2_CMD, OCW2_EOI);
        }
        outb(PIC1_CMD, OCW2_EOI);
    }
}

/// Write value to port via inline asm "out dx, al".
///
/// # Safety
///
/// Raw I/O port write; caller validates the port is safe.
#[inline]
pub unsafe fn outb(port: u16, value: u8) {
    // SAFETY: ring-0 "out dx, al" to a fixed ISA port.
    asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags),
    );
}

/// Read a byte from port via inline asm "in al, dx".
///
/// # Safety
///
/// Raw I/O port read; caller validates the port is safe.
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: ring-0 "in al, dx" from a fixed ISA port.
    asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags),
    );
    value
}

/// Burn ~1us by writing to the ISA POST port 0x80. Gives slow ISA
/// devices time between successive PIC I/O writes.
///
/// # Safety
///
/// Raw I/O port write; port 0x80 is the conventional debug/POST
/// port with no attached side effect.
#[inline]
unsafe fn io_wait() {
    // SAFETY: write of a don't-care byte to the POST port.
    outb(0x80, 0);
}
