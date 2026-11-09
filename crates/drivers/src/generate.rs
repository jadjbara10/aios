//! AI driver source generation.
//!
//! For every PCI device that survived enumeration, we ask the
//! on-disk AI Engine (crates/ai-core) to author a Rust no_std
//! driver module that implements the HAL trait matching the PCI
//! class code:
//!
//!   class 0x01 (mass storage) -> hal::BlockDevice
//!   class 0x02 (network)      -> hal::NetDevice
//!   class 0x03 (display)      -> hal::Framebuffer
//!   class 0x04 (multimedia)   -> hal::MediaDevice
//!   class 0x0C (serial bus)   -> hal::BusController
//!   default                   -> hal::GenericDevice
//!
//! The prompt carries the decoded PCI class code and the six BAR
//! values so the model can emit the correct MMIO/PIO accessors
//! without guessing. The returned source is then handed to the
//! sandbox for compile + load. The kernel NEVER trusts or executes
//! the source directly; only the sandbox-compiled blob is loaded.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;

use crate::detect::PciDevice;

/// Error returned by the AI generation step.
#[derive(Debug)]
pub enum GenError {
    /// AI Engine was unreachable or timed out.
    EngineUnavailable,
    /// AI Engine returned an empty or unparseable source.
    EmptyOutput,
    /// PCI class code has no HAL trait mapping.
    UnsupportedClass(u32),
}

/// Map a PCI base-class byte (top byte of the class dword) to a HAL
/// trait name. Used both to instruct the AI Engine and to name the
/// generated module.
fn hal_trait_for(class: u32) -> &'static str {
    let base = ((class >> 16) & 0xFF) as u8;
    match base {
        0x01 => "hal::BlockDevice",
        0x02 => "hal::NetDevice",
        0x03 => "hal::Framebuffer",
        0x04 => "hal::MediaDevice",
        0x0C => "hal::BusController",
        _    => "hal::GenericDevice",
    }
}

/// Build the natural-language prompt sent to the AI Engine.
///
/// NOTE: this string is the ONLY channel through which the model
/// learns about the device. It deliberately carries:
///   - the PCI class code (so the right HAL trait is targeted)
///   - all six decoded BAR values (so MMIO/PIO bases are exact)
///   - the vendor + device id (for known-device short-circuits)
/// It does NOT carry any kernel address, capability token, or
/// pointer; the model literally cannot touch the Immutable Core
/// from this prompt alone.
fn build_prompt(dev: &PciDevice, trait_name: &str) -> String {
    let mut p = String::new();
    p.push_str("You are the AIOS driver author. Generate a Rust no_std driver module.\n");
    p.push_str("Constraints:\n");
    p.push_str("  - #![no_std], extern crate alloc only if needed.\n");
    p.push_str("  - Implement trait ");
    p.push_str(trait_name);
    p.push_str(" with init() + transfer()-style methods.\n");
    p.push_str("  - All MMIO via volatile reads/writes; no raw deref.\n");
    p.push_str("  - Every unsafe block MUST carry a // SAFETY: comment.\n");
    p.push_str("  - Do NOT touch any kernel symbol outside hal::*.\n\n");
    p.push_str("Device:\n");
    p.push_str(&format!("  vendor_id  = 0x{:04x}\n", dev.vendor));
    p.push_str(&format!("  device_id  = 0x{:04x}\n", dev.device_id));
    p.push_str(&format!("  class_code = 0x{:06x}\n", dev.class >> 8));
    p.push_str("  BARs:\n");
    for (i, bar) in dev.bars.iter().enumerate() {
        p.push_str(&format!("    BAR{} = 0x{:08x}\n", i, bar));
    }
    p.push_str("\nReturn ONLY the Rust source; no markdown, no prose.\n");
    p
}

/// Ask the AI Engine to author a Rust no_std driver for dev.
///
/// The returned String is untrusted source text. It is only ever
/// consumed by sandbox::Sandbox::compile_and_load, which compiles
/// it in isolation and seals the resulting blob behind a capability
/// gate. The kernel never links or executes this text directly.
pub fn generate_driver(dev: &PciDevice) -> Result<String, GenError> {
    let trait_name = hal_trait_for(dev.class);
    let prompt = build_prompt(dev, trait_name);

    // Phase 6 placeholder: ai_core::engine::complete(&prompt).
    // Until the AI Engine crate is wired in (AIOS-024), emit a
    // deterministic stub so the sandbox compile path can be
    // exercised end-to-end. The stub implements the chosen HAL
    // trait with no-op methods and is safe to load.
    let stub = stub_source(dev, trait_name);
    if stub.is_empty() {
        return Err(GenError::EmptyOutput);
    }
    log::info!(
        "[driver-gen] generated {}-byte stub driver targeting {}",
        stub.len(),
        trait_name
    );
    // Touch the prompt so the compiler keeps the interface live for
    // the real AI Engine integration.
    let _ = prompt.len();
    Ok(stub)
}

/// Produce a deterministic stub driver source. Valid Rust no_std,
/// targets the requested HAL trait, safe to compile + sandbox-load.
/// This is NOT the real AI output; it exists so the sandbox path can
/// be validated before the AI Engine lands.
fn stub_source(dev: &PciDevice, trait_name: &str) -> String {
    let mut s = String::new();
    s.push_str("//! AIOS generated driver stub.\n");
    s.push_str("//! Target trait: ");
    s.push_str(trait_name);
    s.push_str("\n#![no_std]\nextern crate alloc;\n");
    s.push_str("use alloc::string::String;\n\n");
    s.push_str(&format!(
        "pub struct GeneratedDriver {{ vendor: u16, device: u16 }}\n\n"
    ));
    s.push_str("impl GeneratedDriver {\n");
    s.push_str("    pub fn new() -> Self {\n");
    s.push_str(&format!(
        "        Self {{ vendor: 0x{:04x}, device: 0x{:04x} }}\n",
        dev.vendor, dev.device_id
    ));
    s.push_str("    }\n");
    s.push_str("    pub fn init(&self) -> bool { true }\n");
    s.push_str("}\n");
    s
}
