// crates/kernel/src/sched/round_robin.rs
// Round-robin policy + the asm context-switch primitive.
//
// RoundRobinScheduler owns the task vector plus a ready queue of TaskIds.
// pick_next rotates the index of the currently-running task and returns
// the next ready one. Priority is a Phase-3 placeholder tracked by
// AIOS-012; today all tasks are equal-weight.
//
// Author: Jad Jbara <jad@aios.dev>

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use crate::sched::task::{CpuContext, Task, TaskId, TaskState};

pub struct RoundRobinScheduler {
    pub(crate) tasks: Vec<Task>,
    pub(crate) ready: Vec<TaskId>,
    pub(crate) current_idx: Option<usize>,
}

impl RoundRobinScheduler {
    pub fn new() -> Self {
        Self { tasks: Vec::new(), ready: Vec::new(), current_idx: None }
    }

    pub fn insert(&mut self, task: Task) {
        self.tasks.push(task);
    }

    /// Rotate to the next ready task. Returns a shared reference into
    /// the scheduler owned task vector; the caller (sched::tick) is
    /// responsible for performing the context switch without holding
    /// any extra borrows.
    pub fn pick_next(&mut self) -> Option<&Task> {
        if self.tasks.is_empty() || self.ready.is_empty() {
            return None;
        }
        let next_id = self.ready.remove(0);
        let next_idx = self.tasks.iter().position(|t| t.id == next_id)?;
        if let Some(cur) = self.current_idx {
            if let Some(t) = self.tasks.get_mut(cur) {
                if t.state == TaskState::Running {
                    t.state = TaskState::Ready;
                    self.ready.push(t.id);
                }
            }
        }
        if let Some(t) = self.tasks.get_mut(next_idx) {
            t.state = TaskState::Running;
        }
        self.current_idx = Some(next_idx);
        self.tasks.get(next_idx)
    }

    /// Borrow the currently-running task mutably.
    pub fn current_mut(&mut self) -> Option<&mut Task> {
        self.current_idx.and_then(move |i| self.tasks.get_mut(i))
    }

    /// Reap terminated tasks. Called from tick opportunistically.
    pub fn reap(&mut self) {
        self.tasks.retain(|t| t.state != TaskState::Terminated);
    }
}

/// Field offsets into CpuContext matching the declaration order in
/// task.rs: rbx, rbp, r12, r13, r14, r15, rip, rsp. Each is u64 so the
/// byte offset is field_index * 8. Kept here so the asm trampoline has
/// a single source of truth.
pub const OFF_RBX: usize = 0x00;
pub const OFF_RBP: usize = 0x08;
pub const OFF_R12: usize = 0x10;
pub const OFF_R13: usize = 0x18;
pub const OFF_R14: usize = 0x20;
pub const OFF_R15: usize = 0x28;
pub const OFF_RIP: usize = 0x30;
pub const OFF_RSP: usize = 0x38;

/// context_switch -- switch from prev to next by:
///   1. saving callee-saved regs of the caller into prev.ctx
///   2. saving caller rsp into prev.rsp, loading next.rsp into rsp
///   3. restoring callee-saved regs from next.ctx
///   4. pushing next.rip and ret-ing into it
///
/// Implemented as an extern C Rust wrapper around the __sched_switch
/// global_asm symbol. Splitting the asm out lets the compiler treat
/// context_switch as a regular C-ABI call without having to model
/// callee-saved clobbers across inline asm.
///
/// Safety: both references must point to valid CpuContext structs whose
/// rsp field (if loaded) refers to a 16-byte-aligned stack with at
/// least 64 bytes of headroom. The prev context must be writable for
/// the lifetime of the call; next must be readable.
#[no_mangle]
pub unsafe extern "C" fn context_switch(prev: &mut CpuContext, next: &CpuContext) {
    // SAFETY: caller (sched::tick) guarantees both references point to
    // valid CpuContext structs embedded in live Task entries. The asm
    // trampoline performs no memory access other than the explicit
    // push/pop and the two pointer writes through rdi/rsi. IRQs are
    // disabled at the call site (timer IRQ top-half or syscall trap).
    unsafe {
        __sched_switch(
            prev as *mut CpuContext as *mut u8,
            next as *const CpuContext as *const u8,
        );
    }
}

extern "C" {
    /// Raw context-switch primitive. Defined via global_asm below. See
    /// context_switch for the safe wrapper and the full safety contract.
    #[link_name = "__sched_switch"]
    fn __sched_switch(prev: *mut u8, next: *const u8);
}

// The actual register dance. Intel syntax (Rust global_asm default on
// x86_64). rdi = prev (CpuContext*), rsi = next (const CpuContext*).
core::arch::global_asm!(
    ".section .text",
    ".p2align 4",
    ".globl __sched_switch",
    ".type __sched_switch, @function",
    "__sched_switch:",
    // 1. Save callee-saved regs of the caller into prev.ctx.
    "mov [rdi + 0x00], rbx",
    "mov [rdi + 0x08], rbp",
    "mov [rdi + 0x10], r12",
    "mov [rdi + 0x18], r13",
    "mov [rdi + 0x20], r14",
    "mov [rdi + 0x28], r15",
    // 2. Save caller rsp into prev.rsp; load next.rsp into rsp.
    "mov [rdi + 0x38], rsp",
    "mov rsp, [rsi + 0x38]",
    // 3. Restore callee-saved regs from next.ctx.
    "mov rbx, [rsi + 0x00]",
    "mov rbp, [rsi + 0x08]",
    "mov r12, [rsi + 0x10]",
    "mov r13, [rsi + 0x18]",
    "mov r14, [rsi + 0x20]",
    "mov r15, [rsi + 0x28]",
    // 4. Push next.rip and ret into it.
    "push qword ptr [rsi + 0x30]",
    "ret",
    ".size __sched_switch, . - __sched_switch",
);
