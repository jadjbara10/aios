// crates/kernel/src/sched/task.rs
// Task Control Block + saved CPU context.
//
// Every AIOS task (OS thread or AI agent) owns a Task and a 16 KiB private
// stack. The context-switch trampoline saves/restores the callee-saved
// subset of the SysV AMD64 ABI register file.
//
// Author: Jad Jbara <jad@aios.dev>

#![no_std]

extern crate alloc;

use alloc::alloc::{alloc_zeroed, Layout};
use core::ptr::NonNull;

/// Newtype around the monotonic task id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskId(pub u64);

impl TaskId {
    pub const fn new(v: u64) -> Self { Self(v) }
}

/// Lifecycle of a Task. The scheduler transitions tasks between these
/// states on every tick; only Ready tasks are eligible for pick_next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Terminated,
}

/// Saved callee-saved register file per SysV AMD64 ABI. rip and rsp are
/// also saved so the trampoline can ret into a fresh task on first run.
/// Field order is load-bearing -- the asm offsets in round_robin.rs are
/// computed against this exact layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CpuContext {
    pub rbx: u64,  // offset 0x00
    pub rbp: u64,  // offset 0x08
    pub r12: u64,  // offset 0x10
    pub r13: u64,  // offset 0x18
    pub r14: u64,  // offset 0x20
    pub r15: u64,  // offset 0x28
    pub rip: u64,  // offset 0x30
    pub rsp: u64,  // offset 0x38
}

/// Stack size for every user-spawned task -- 16 KiB per the AIOS spec.
pub const STACK_SIZE: usize = 16 * 1024;

/// Task Control Block.
pub struct Task {
    pub id: TaskId,
    pub state: TaskState,
    /// Pointer to the base of the allocated stack (lowest address). The
    /// allocation itself is owned by Task via stack_layout; dealloc runs
    /// in Drop.
    pub stack: *mut u8,
    pub stack_layout: Layout,
    pub ctx: CpuContext,
    /// Human-readable label for diagnostics; AI agents use their handle.
    pub label: &'static str,
}

impl Task {
    /// Construct the bootstrap kernel task -- it has no separate stack
    /// because the bootloader stack is reused until the first switch.
    pub fn new_kernel(id: TaskId, label: &'static str) -> Self {
        Self {
            id,
            state: TaskState::Running,
            stack: core::ptr::null_mut(),
            stack_layout: Layout::from_size_align(1, 1).unwrap(),
            ctx: CpuContext::default(),
            label,
        }
    }

    /// Allocate a 16 KiB stack and lay out a return-to-trampoline frame
    /// so the first context_switch into this task transfers control to
    /// entry(arg) with a clean ABI-conformant stack.
    pub fn new_user(id: TaskId, entry: extern "C" fn(usize) -> !, arg: usize) -> Self {
        let layout = Layout::from_size_align(STACK_SIZE, 16).unwrap();
        // SAFETY: layout.size() > 0 and layout.align() is a power of two
        // (16) <= the allocator maximum alignment on x86_64. The
        // returned pointer is valid for STACK_SIZE bytes and
        // zero-initialized, which lets stack-guard diagnostics detect
        // uninitialized frames.
        let base = unsafe { alloc_zeroed(layout) };
        let base = NonNull::new(base).expect("OOM: cannot allocate 16 KiB task stack");

        // Top of stack (highest address). SysV AMD64 requires the stack
        // pointer to be 16-byte aligned at the call site; the trampoline
        // performs the equivalent of a call (pushing the return
        // address), so we align top to 16 here and let the implicit
        // push land rsp on an 8-mod-16 boundary as required.
        let mut top = base.as_ptr() as usize + STACK_SIZE;
        top &= !0xFusize;

        // Lay out the initial frame. From low to high (top of stack
        // pushed last) we store the argument in rdi at the very top so
        // the trampoline can read it after restoring callee-saved regs
        // but before ret-ing into entry. The entry point lives in
        // ctx.rip; the trampoline pushes it and ret-s into it.
        let top_ptr = top as *mut u64;
        // SAFETY: top is 16-byte aligned and points inside the freshly
        // allocated 16 KiB block, with at least 16 bytes of headroom
        // below the top. Writing one u64 cannot overflow.
        unsafe {
            *top_ptr.sub(1) = arg as u64;
        }

        let ctx = CpuContext {
            rbx: 0, rbp: 0, r12: 0, r13: 0, r14: 0, r15: 0,
            rip: entry as usize as u64,
            // rsp points just below the arg slot so the trampoline can
            // pop the arg into rdi before ret-ing into entry.
            rsp: (top as usize - 8) as u64,
        };

        Task {
            id,
            state: TaskState::Ready,
            stack: base.as_ptr(),
            stack_layout: layout,
            ctx,
            label: "user",
        }
    }

    /// Mark the task terminated; the scheduler reaps it on a later tick.
    pub fn terminate(&mut self) {
        self.state = TaskState::Terminated;
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        if self.stack.is_null() { return; }
        // SAFETY: self.stack was produced by alloc_zeroed(layout) in
        // new_user and the layout is unchanged. The Task is being
        // dropped, so no other reference to this allocation exists.
        unsafe {
            alloc::alloc::dealloc(self.stack, self.stack_layout);
        }
    }
}
