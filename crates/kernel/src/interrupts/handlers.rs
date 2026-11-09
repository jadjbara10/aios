//! x86-64 exception handlers for AIOS.
//!
//! Each handler is extern "x86-interrupt" fn(...) so the compiler
//! emits the correct prologue/epilogue (preserving caller-saved GPRs
//! and using iretq). On entry we log via log::error! with the saved
//! frame, then cli; hlt to halt the core. None of the fatal handlers
//! return; Phase 3 will route #PF to a real page-fault dispatcher.
//!
//! NOTE: requires nightly + #![feature(abi_x86_interrupt)] at the
//! crate root. The feature was stabilized for x86_64 in recent
//! nightlies; rust-toolchain.toml pins the known-good toolchain.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

use core::arch::asm;

/// The CPU-pushed exception stack frame (long-mode int, always 5
/// words: RIP, CS, RFLAGS, RSP, SS).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ExceptionStackFrame {
    pub instruction_pointer: u64,
    pub code_segment: u64,
    pub cpu_flags: u64,
    pub stack_pointer: u64,
    pub stack_segment: u64,
}

/// Read the page-fault linear address from CR2.
///
/// # Safety
///
/// mov from CR2 is privileged but always safe in ring 0.
unsafe fn read_cr2() -> u64 {
    let value: u64;
    // SAFETY: ring-0 read of CR2; no memory side effects, no flags
    // clobbered.
    asm!(
        "mov {}, cr2",
        out(reg) value,
        options(nostack, preserves_flags),
    );
    value
}

/// Disable interrupts and halt the core forever. We never return from
/// a fatal exception; cli blocks any pending IRQ from racing the
/// halt, and hlt parks the CPU until the next non-maskable event.
fn halt_forever() -> ! {
    // SAFETY: cli; hlt is safe in ring 0 on a halted core with no
    // further state to preserve.
    unsafe {
        asm!(
            "cli",
            "hlt",
            options(nostack, preserves_flags),
        );
    }
    loop {
        // SAFETY: defensive -- keep the core parked even if woken by
        // an NMI; interrupts are already disabled above.
        unsafe {
            asm!("hlt", options(nostack, preserves_flags));
        }
    }
}

/// #DE -- Divide Error (vector 0). Raised by DIV/IDIV with a zero
/// divisor or an overflow. No error code is pushed.
#[no_mangle]
pub extern "x86-interrupt" fn divide_by_zero(stack_frame: &mut ExceptionStackFrame) {
    log::error!(
        "[#DE] divide-by-zero at rip=0x{:016x}",
        stack_frame.instruction_pointer
    );
    halt_forever();
}

/// #PF -- Page Fault (vector 14). CR2 holds the faulting linear
/// address; the error code encodes the access kind and cause bits.
#[no_mangle]
pub extern "x86-interrupt" fn page_fault(
    stack_frame: &mut ExceptionStackFrame,
    error_code: u64,
) {
    // SAFETY: ring-0 read of CR2.
    let cr2 = unsafe { read_cr2() };
    log::error!(
        "[#PF] page fault: cr2=0x{:016x} rip=0x{:016x} err=0x{:x}",
        cr2,
        stack_frame.instruction_pointer,
        error_code
    );
    halt_forever();
}

/// #DF -- Double Fault (vector 8). Routed to IST[0] (set by
/// gdt::load_gdt) so a corrupt kernel stack cannot tear down this
/// handler. The error code is always zero. Never returns.
#[no_mangle]
pub extern "x86-interrupt" fn double_fault(
    stack_frame: &mut ExceptionStackFrame,
    error_code: u64,
) -> ! {
    log::error!(
        "[#DF] double fault: rip=0x{:016x} err=0x{:x} -- kernel halted",
        stack_frame.instruction_pointer,
        error_code
    );
    halt_forever();
}

/// #GP -- General Protection Fault (vector 13). Raised by any
/// non-page-fault protection violation in protected/long mode.
#[no_mangle]
pub extern "x86-interrupt" fn general_protection_fault(
    stack_frame: &mut ExceptionStackFrame,
    error_code: u64,
) {
    log::error!(
        "[#GP] general protection fault: rip=0x{:016x} err=0x{:x}",
        stack_frame.instruction_pointer,
        error_code
    );
    halt_forever();
}
