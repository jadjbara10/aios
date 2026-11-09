//! Driver sandbox: compile, seal, install.
//!
//! The sandbox is the trust boundary between AI-generated driver
//! source and the Immutable Core. The pipeline is:
//!
//!   1. compile:         src  -> loadable blob   (crates/compiler-host)
//!   2. capability mint: only the BARs the device declared
//!   3. load:            blob + caps -> isolated address space
//!   4. install:         register in the HAL DeviceRegistry
//!
//! The sandbox CANNOT mint capabilities for resources outside its
//! own BAR set; the capability table (crates/hal::caps) rejects any
//! out-of-scope request. The kernel security base is therefore
//! never reachable from generated code.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use crate::detect::PciDevice;
use crate::DriverId;

/// Opaque handle to a live, sandboxed driver instance.
#[derive(Clone, Copy, Debug)]
pub struct DriverHandle {
    pub id: DriverId,
    /// Length in bytes of the compiled loadable blob.
    pub blob_len: usize,
    /// Number of capabilities minted for this driver.
    pub cap_count: u8,
}

/// Errors that can occur during compile + load.
#[derive(Debug)]
pub enum SandboxError {
    /// Generated source failed to compile in the isolated toolchain.
    CompileFailed,
    /// The blob loaded but capability minting was refused by the
    /// kernel capability table (e.g. out-of-scope BAR).
    CapabilityRefused,
    /// HAL DeviceRegistry rejected the registration (duplicate id,
    /// table full, etc.).
    RegistryFull,
}

/// The sandbox. Stateless on purpose; all per-driver state lives in
/// the loaded blob's private address space.
pub struct Sandbox;

impl Sandbox {
    /// Compile src into a loadable blob, mint the least-privilege
    /// capability set (only the BARs the device declared), load it
    /// into an isolated address space, and register it in the HAL
    /// DeviceRegistry.
    ///
    /// Returns a DriverHandle on success. The handle's id is the
    /// capability-scoped identifier the rest of the kernel uses to
    /// address the driver.
    pub fn compile_and_load(src: &str, dev: &PciDevice) -> Result<DriverHandle, SandboxError> {
        // 1. Compile the generated source into a loadable blob.
        let blob = Self::compile(src)?;
        log::info!(
            "[sandbox] compiled {} bytes of source -> {} bytes of blob",
            src.len(),
            blob.len()
        );

        // 2. Mint capabilities ONLY for the device's own BARs. The
        //    capability table enforces this in kernel space.
        let caps = Self::mint_caps(dev)?;
        log::info!("[sandbox] minted {} capabilities", caps.len());

        // 3. Load the blob + caps into an isolated address space.
        //    The loader returns a stable id from the capability table.
        let id = Self::load(&blob, &caps)?;

        // 4. Register with the HAL DeviceRegistry so other kernel
        //    subsystems (VFS, netstack, ...) can discover the driver.
        Self::install(id, dev)?;

        Ok(DriverHandle {
            id,
            blob_len: blob.len(),
            cap_count: caps.len() as u8,
        })
    }

    /// Compile the source via the isolated toolchain
    /// (crates/compiler-host). In Phase 6 alpha this is a stub that
    /// validates the source is non-empty and well-formed enough to
    /// wrap in a 1-byte header.
    fn compile(src: &str) -> Result<Vec<u8>, SandboxError> {
        if src.is_empty() {
            return Err(SandboxError::CompileFailed);
        }
        // Real implementation: invoke compiler_host::build(src).
        // Stub: prefix the bytes with a 0xA1 tag so the loader can
        // distinguish a sandboxed blob from a raw one.
        let mut blob = Vec::with_capacity(src.len() + 1);
        blob.push(0xA1);
        blob.extend_from_slice(src.as_bytes());
        Ok(blob)
    }

    /// Mint capabilities for the device's own BARs only. Zero-valued
    /// BARs are skipped (the device does not use them).
    fn mint_caps(dev: &PciDevice) -> Result<Vec<u32>, SandboxError> {
        let mut caps = Vec::new();
        for bar in dev.bars.iter() {
            if *bar != 0 {
                caps.push(*bar);
            }
        }
        // Real implementation: hal::caps::mint(bar) per BAR, returning
        // Err(CapabilityRefused) on out-of-scope requests.
        Ok(caps)
    }

    /// Load the blob + caps into an isolated address space. Returns
    /// the capability-scoped driver id.
    fn load(blob: &[u8], caps: &[u32]) -> Result<DriverId, SandboxError> {
        if blob.is_empty() || caps.is_empty() {
            return Err(SandboxError::CapabilityRefused);
        }
        // Real implementation: scheduler::spawn_sandbox(blob, caps).
        // Stub: derive a stable id from the blob length + cap count.
        let id_val = (blob.len() as u64) << 8 | (caps.len() as u64 & 0xFF);
        Ok(DriverId(id_val))
    }

    /// Register the loaded driver in the HAL DeviceRegistry.
    fn install(id: DriverId, dev: &PciDevice) -> Result<(), SandboxError> {
        // Real implementation: hal::registry::register(id, dev).
        log::info!(
            "[sandbox] installed driver id={} vendor=0x{:04x} device=0x{:04x}",
            id.0, dev.vendor, dev.device_id
        );
        Ok(())
    }
}
