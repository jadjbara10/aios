; crates/bootloader/src/entry.asm
; AIOS UEFI Bootloader - post-ExitBootServices handoff trampoline.
; Author: Jad Jbara <jad@aios.dev>
;
; Context when this runs:
;   * CPU is in 64-bit long mode (UEFI brought us here).
;   * Paging is ON (UEFI's identity-mapped tables; CR3 unchanged unless
;     the 'cr3' arg is non-zero).
;   * Interrupts may still be enabled - we disable them here.
;   * Boot services are dead; UEFI's stack is NOT safe to keep using.
;
; Called from Rust as (SysV AMD64):
;   extern "C" fn handoff_to_kernel(
;       entry:  u64,   ; rdi - kernel _start virtual address
;       bi:     u64,   ; rsi - &BootInfo
;       cr3:    u64,   ; rdx - new CR3, or 0 to keep UEFI's
;       stack:  u64,   ; rcx - kernel stack top
;   ) -> !;
;
; Result: control transfers to kernel _start with:
;   rdi = &BootInfo   (kernel's first arg, per the boot contract)
;   rsp = stack top
;   interrupts disabled

[BITS 64]
section .text
global handoff_to_kernel

handoff_to_kernel:
    ; (1) Install the fresh kernel stack.
    mov     rsp, rcx
    mov     rbp, 0

    ; (2) Optional CR3 swap - switch to AIOS-owned page tables.
    ;     Phase 1 passes 0 (reuse UEFI's identity map). Phase 2 (AIOS-005)
    ;     passes our PML4 physical address; loading CR3 flushes the TLB.
    test    rdx, rdx
    jz      .no_cr3
    mov     cr3, rdx
.no_cr3:

    ; (3) Reorder args for the kernel: kernel wants &BootInfo in RDI.
    ;     'entry' was in RDI; move it to RAX (the indirect jmp target).
    mov     rax, rdi        ; rax = kernel _start
    mov     rdi, rsi        ; rdi = &BootInfo  (kernel arg #0)

    ; (4) Clear direction flag + disable interrupts for the handoff.
    cld
    cli

    ; (5) Jump to the kernel. We use a 64-bit indirect jmp; the kernel
    ;     runs in the same code segment UEFI set up (CS = long-mode 64-bit
    ;     selector). _start is #[no_mangle] extern "C" and diverges.
    jmp     rax

    ; The kernel must not return. If it ever does, halt forever.
.hang:
    hlt
    jmp     .hang
