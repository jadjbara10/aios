// crates/kernel/src/sched/mod.rs
// AIOS scheduler root -- preemptive task switching driven by the timer IRQ.
//
// Each AIOS task is either an OS thread or a first-class AI agent context;
// both share the same Task abstraction and the same round-robin policy.
//
// Author: Jad Jbara <jad@aios.dev>

#![no_std]

extern crate alloc;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use alloc::vec::Vec;
use spin::Mutex;

pub mod ipc;
pub mod round_robin;
pub mod task;

pub use ipc::{Channel, ChannelCap, Receiver, Sender};
pub use round_robin::context_switch;
pub use task::{CpuContext, Task, TaskId, TaskState};

/// Ready-queue container -- the live state of the scheduler. Held inside a
/// single static spin::Mutex; the timer IRQ top-half briefly takes this
/// lock to advance the quantum.
///
/// The lock is lock-free-ready: every per-operation critical section is
/// O(1) (push/pop from a Vec), so contention is bounded to a few hundred
/// cycles per tick. AIOS-009 tracks the per-CPU CAS run-queue successor.
pub struct TaskList {
    pub(crate) tasks: Vec<Task>,
    pub(crate) ready: Vec<TaskId>,
    pub(crate) current: Option<TaskId>,
}

impl TaskList {
    pub fn push_ready(&mut self, id: TaskId) { self.ready.push(id); }
}

/// Global scheduler state. Wrapped in Option so init() can install the
/// bootstrap task on first use; subsequent callers go through the
/// initialized fast path.
static SCHED: Mutex<Option<TaskList>> = Mutex::new(None);

/// Flips to true once init() has installed the bootstrap task.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Monotonic task id generator -- never reuses ids from terminated tasks.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Scheduler facade -- every public operation goes through these free
/// functions so the global lock is acquired in exactly one place per call.
pub struct Scheduler;

/// Initialize the global scheduler with a single bootstrap task wrapping
/// the currently-executing kernel context. Must be called exactly once
/// from _start before any spawn / yield_now.
///
/// Panics if called twice.
pub fn init() {
    let mut guard = SCHED.lock();
    if guard.is_some() {
        panic!("sched::init called twice");
    }
    let mut tasks = Vec::new();
    let mut ready = Vec::new();
    let bs = Task::new_kernel(TaskId::new(0), "kinit");
    ready.push(bs.id);
    tasks.push(bs);
    *guard = Some(TaskList { tasks, ready, current: Some(TaskId::new(0)) });
    INITIALIZED.store(true, Ordering::Release);
}

/// Spawn a new task whose entry point is entry(arg). The task is added to
/// the ready queue and will run on the next pick_next that selects it.
/// Returns the assigned TaskId.
pub fn spawn(entry: extern "C" fn(usize) -> !, arg: usize) -> TaskId {
    assert!(INITIALIZED.load(Ordering::Acquire), "sched not initialized");
    let id = TaskId::new(NEXT_ID.fetch_add(1, Ordering::Relaxed));
    let task = Task::new_user(id, entry, arg);
    let mut guard = SCHED.lock();
    let list = guard.as_mut().expect("sched not initialized");
    list.tasks.push(task);
    list.ready.push(id);
    id
}

/// Cooperative yield -- invokes the syscall trap (int 0x80) so the
/// in-kernel syscall handler can call tick() on our behalf. We do not
/// directly enter context_switch from here because doing so would bypass
/// the IRQ-safe path and risk re-entering the scheduler lock.
///
/// The timer IRQ (PIT/APIC, Phase 2 / AIOS-008) eventually drives the
/// same tick() entry point, giving us preemption on top of cooperation.
/// Control resumes at the instruction following int 0x80 once the
/// scheduler switches back to this task.
#[inline(always)]
pub fn yield_now() {
    assert!(INITIALIZED.load(Ordering::Acquire), "sched not initialized");
    // SAFETY: int 0x80 is the legacy IA-32 syscall trap. On AIOS it is
    // wired by the IDT (crates/kernel/src/trap/idt.rs) to dispatch into
    // sched::tick after saving the caller CpuContext. The trap frame is
    // restored atomically on iretq, so no caller-saved register is
    // observed in an inconsistent state by the resumed task. clobber_abi
    // tells the compiler the trap path may touch any caller-saved reg.
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("rax") SYS_YIELD,
            clobber_abi(C),
            options(preserves_flags),
        );
    }
}

/// Syscall numbers consumed by the trap handler. Kept here (rather than
/// in trap/) so the scheduler owns its own ABI surface.
pub const SYS_YIELD: u64 = 0x01;
pub const SYS_SEND:  u64 = 0x02;
pub const SYS_RECV:  u64 = 0x03;

/// Advance one scheduling quantum. Called either from the timer IRQ
/// top-half or from the int 0x80 syscall path. Rotates the ready queue
/// and performs a context switch if a different task was picked.
pub fn tick() {
    if !INITIALIZED.load(Ordering::Acquire) { return; }

    let (prev_ctx, next_ctx): (*mut CpuContext, *const CpuContext) = {
        let mut guard = SCHED.lock();
        let list = match guard.as_mut() { Some(s) => s, None => return };
        if list.ready.is_empty() { return; }
        let next_id = list.ready.remove(0);
        let prev_id = match list.current {
            Some(c) => c,
            None => { list.current = Some(next_id); return; }
        };
        if prev_id == next_id { return; }
        let prev_idx = match list.tasks.iter().position(|t| t.id == prev_id) {
            Some(i) => i, None => return,
        };
        let next_idx = match list.tasks.iter().position(|t| t.id == next_id) {
            Some(i) => i, None => return,
        };
        list.tasks[prev_idx].state = TaskState::Ready;
        list.ready.push(prev_id);
        list.tasks[next_idx].state = TaskState::Running;
        list.current = Some(next_id);
        let p: *mut CpuContext = &mut list.tasks[prev_idx].ctx;
        let n: *const CpuContext = &list.tasks[next_idx].ctx;
        (p, n)
        // guard dropped at block end
    };

    // SAFETY: IRQs are disabled (we are in the timer IRQ top-half or the
    // int 0x80 syscall trap) and the kernel is uniprocessor in Phase 2,
    // so no other code can mutate SCHED between the lock release above
    // and the context switch below. The two pointers were valid when
    // extracted and remain valid because the task vector cannot be
    // reallocated without the lock. Both contexts are #[repr(C)] and
    // their rsp fields point to 16-byte-aligned stacks with headroom.
    unsafe {
        context_switch(prev_ctx, next_ctx);
    }
}
