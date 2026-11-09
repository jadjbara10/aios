//! PCI bus enumeration via legacy config-space I/O ports.
//!
//! Two 32-bit I/O ports are involved:
//!   0xCF8 - CONFIG_ADDRESS - write the encoded address dword
//!   0xCFC - CONFIG_DATA    - read the 32-bit word at that offset
//!
//! The address dword layout (PCI Local Spec r3.0 sec 2.2.2.2):
//!   bit  31      enable bit (must be 1)
//!   bits 23..16  bus number
//!   bits 15..11  device number
//!   bits 10..8   function number
//!   bits  7..2   dword-aligned register offset
//!   bits  1..0   zero
//!
//! All port I/O is unsafe inline asm. Every unsafe block has a SAFETY
//! comment. detect.rs never writes to 0xCFC.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::arch::asm;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;
const ENABLE_BIT: u32 = 1u32 << 31;

/// A minimally-decoded PCI device on bus/dev/func.
#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub vendor: u16,
    pub device_id: u16,
    /// Full 24-bit class code shifted left 8 (base/sub/prog-if).
    pub class: u32,
    /// Decoded BARs (BAR0..BAR5); zero if unimplemented.
    pub bars: [u32; 6],
}

/// Known-device table. A match short-circuits AI generation: the
/// pipeline can hand the sandbox a hand-verified blob instead of
/// asking the AI Engine to author one from scratch.
pub const KNOWN: &[(u16, u16, &str)] = &[
    (0x8086, 0x100E, "Intel 82540EM Gigabit (e1000)"),
    (0x8086, 0x1237, "Intel 440FX PCI/ISA bridge (piix)"),
    (0x1AF4, 0x1000, "VirtIO Network (net)"),
    (0x1AF4, 0x1001, "VirtIO Block (blk)"),
    (0x1234, 0x1111, "QEMU/Bochs VBE framebuffer"),
    (0x1B36, 0x0001, "Red Hat QEMU PCI test device"),
];

/// Returns a static description if the device matches the known table.
pub fn known_match(dev: &PciDevice) -> Option<&'static str> {
    KNOWN.iter()
        .find(|(v, d, _)| *v == dev.vendor && *d == dev.device_id)
        .map(|(_, _, name)| *name)
}

/// Encode a (bus, dev, func, reg_off) into a CONFIG_ADDRESS dword.
fn addr(bus: u8, dev: u8, func: u8, reg: u8) -> u32 {
    let b = (bus as u32) << 16;
    let d = ((dev & 0x1F) as u32) << 11;
    let f = ((func & 0x07) as u32) << 8;
    let r = (reg & 0xFC) as u32;
    ENABLE_BIT | b | d | f | r
}

/// Write a dword to an I/O port.
///
/// SAFETY: outl touches the CPU I/O bus. On x86 the port range
/// 0xCF8/0xCFC is reserved by the PCI host bridge; no memory
/// aliasing occurs. The caller guarantees the port is a 32-bit
/// PCI config-space port.
unsafe fn outl(port: u16, val: u32) {
    asm!("out dx, eax", in("dx") port, in("eax") val, options(nostack, preserves_flags));
}

/// Read a dword from an I/O port.
///
/// SAFETY: inl issues a CPU I/O read. The port must be a valid
/// 32-bit PCI config-space port; the host bridge serializes the
/// access. No memory is mutated.
unsafe fn inl(port: u16) -> u32 {
    let v: u32;
    asm!("in eax, dx", out("eax") v, in("dx") port, options(nostack, preserves_flags));
    v
}

/// Read one 32-bit config dword at the given byte offset.
fn read_dword(bus: u8, dev: u8, func: u8, reg: u8) -> u32 {
    let a = addr(bus, dev, func, reg);
    // SAFETY: outl writes the encoded address to 0xCF8 (no data
    // effect); inl reads the latched 32-bit dword from 0xCFC.
    // Both ports are 32-bit PCI config-space ports and the host
    // bridge serializes them.
    unsafe {
        outl(CONFIG_ADDRESS, a);
        inl(CONFIG_DATA)
    }
}

/// Returns true if a function is present (vendor != 0xFFFF).
fn present(bus: u8, dev: u8, func: u8) -> bool {
    read_dword(bus, dev, func, 0x00) != 0xFFFFFFFF
}

/// Decode a fully populated PciDevice from its config header.
fn decode(bus: u8, dev: u8, func: u8) -> PciDevice {
    let id = read_dword(bus, dev, func, 0x00);
    let cls = read_dword(bus, dev, func, 0x08);
    let mut bars = [0u32; 6];
    for (i, slot) in bars.iter_mut().enumerate() {
        let reg = (0x10 + (i as u8) * 4) & 0xFC;
        *slot = read_dword(bus, dev, func, reg);
    }
    PciDevice {
        bus,
        dev,
        func,
        vendor: (id & 0xFFFF) as u16,
        device_id: ((id >> 16) & 0xFFFF) as u16,
        class: cls & 0xFFFFFF00,
        bars,
    }
}

/// Walk the whole PCI bus tree (bus 0..255, dev 0..31, func 0..7).
///
/// Multi-function devices are detected via the header-type bitfield
/// at offset 0x0E. Functions that read 0xFFFFFFFF are absent.
pub fn enumerate() -> Vec<PciDevice> {
    let mut out = Vec::new();
    for bus in 0u8..=255u8 {
        for dev in 0u8..32u8 {
            if !present(bus, dev, 0) {
                continue;
            }
            let ht = (read_dword(bus, dev, 0, 0x0C) >> 16) & 0xFF;
            let funcs: u8 = if ht & 0x80 != 0 { 8 } else { 1 };
            for func in 0u8..funcs {
                if present(bus, dev, func) {
                    out.push(decode(bus, dev, func));
                }
            }
        }
    }
    out
}
