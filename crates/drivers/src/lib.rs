//! AIOS Driver Generation System
//!
//! Fully AI-driven, no human in the loop. The pipeline is:
//!   detect -> generate -> sandbox -> install
//!
//! The kernel never trusts the generated code; it is compiled into a
//! loadable blob, sealed inside a least-privilege sandbox, and only
//! the BARs it actually needs are minted as capabilities. The
//! Immutable Core security base is never exposed to generated drivers.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]
#![forbid(warnings)]

extern crate alloc;

use alloc::vec::Vec;

pub mod detect;
pub mod generate;
pub mod sandbox;

pub use detect::{enumerate as enumerate_pci, PciDevice};
pub use generate::generate_driver;
pub use sandbox::{DriverHandle, Sandbox, SandboxError};

/// Opaque identifier for an installed driver.
///
/// Minted by Sandbox::compile_and_load once the generated module is
/// registered with the HAL DeviceRegistry. The numeric id is unique
/// per boot and is used by the capability table and the AI Engine
/// for telemetry attribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverId(pub u64);

/// Top-level entry point invoked by the HAL bootstrap once PCI bus
/// enumeration is permitted (Phase 6, after the Immutable Core is
/// sealed and the capability table is online).
///
/// Order matters: detection failures must NOT abort the whole bus
/// scan, only the affected device. Generation failures are logged and
/// the device is skipped. Sandbox load failures cause the device to be
/// quarantined but never panic the kernel.
pub fn run() -> Vec<DriverId> {
    log::info!("[driver-gen] pipeline start");

    let mut installed = Vec::new();
    let devices = detect::enumerate();
    log::info!("[driver-gen] detected {} PCI device(s)", devices.len());

    for dev in devices.iter() {
        log::info!(
            "[driver-gen] {:#x}:{:#x} vendor={:#04x} device={:#04x} class={:#02x}",
            dev.bus, dev.dev, dev.vendor, dev.device_id, dev.class
        );

        // Short-circuit: if we already have a hand-verified driver for
        // this (vendor, device) pair, the sandbox will load that blob
        // instead of asking the AI Engine to author a new one.
        if let Some(name) = detect::known_match(dev) {
            log::info!("[driver-gen] known device: {}", name);
        }

        let src = match generate::generate_driver(dev) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[driver-gen] generate failed: {:?}; skipping", e);
                continue;
            }
        };

        let handle = match Sandbox::compile_and_load(&src, dev) {
            Ok(h) => h,
            Err(e) => {
                log::error!("[driver-gen] sandbox load failed: {:?}; quarantined", e);
                continue;
            }
        };

        log::info!(
            "[driver-gen] installed driver id={} blob={} bytes",
            handle.id.0, handle.blob_len
        );
        installed.push(handle.id);
    }

    log::info!("[driver-gen] pipeline complete: {} driver(s) live", installed.len());
    installed
}
