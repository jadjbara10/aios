//! Rust-side bindings for the NASM handoff trampoline (entry.asm).
//!
//! The 'handoff_to_kernel' symbol is assembled by build.rs and linked into
//! the final .efi. Declared here so efi_main can call it with type safety.

extern "C" {
    /// See 'entry.asm'. Never returns.
    ///
    /// # Safety
    ///
    /// Caller must have:
    ///   - exited UEFI boot services
    ///   - loaded the kernel ELF at 'entry' with a valid virtual address
    ///   - prepared a valid BootInfo at 'boot_info'
    ///   - prepared a valid, page-aligned 'stack_top'
    ///
    /// This function diverges; any code after the call site is unreachable.
    pub fn handoff_to_kernel(
        entry: u64,
        boot_info: u64,
        cr3: u64,
        stack_top: u64,
    ) -> !;
}
