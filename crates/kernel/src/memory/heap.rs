//! crates/kernel/src/memory/heap.rs
//! AIOS v0.1 Alpha — Bump kernel heap.
//!
//! Phase 2 uses a deliberately primitive bump allocator: a static 4 MiB
//! byte array in BSS with a monotonically rising cursor. alloc advances
//! the cursor; dealloc is a no-op that logs a warning. This is fine for
//! the Alpha because the kernel does almost no deallocation during boot
//! — every structure it allocates lives for the kernel's lifetime.
//!
//! Phase 3 will replace this with a buddy + slab allocator backed by
//! the frame allocator. The #[global_allocator] static stays the same
//! — only its inner implementation changes — so all kernel code keeps
//! using alloc::boxed::Box, alloc::vec::Vec, etc. unchanged.
//!
//! Author: Jad Jbara <jad@aios.dev>

use alloc::alloc::{AllocError, GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// 4 MiB backing store for the bump heap. Lives in BSS so it costs no
/// file size and is zero-initialised by the loader.
static mut HEAP: [u8; 4 * 1024 * 1024] = [0u8; 4 * 1024 * 1024];

/// Bump cursor — next free byte offset into HEAP.
static CURSOR: AtomicUsize = AtomicUsize::new(0);

/// Set true by init. We refuse allocations before init to catch
/// ordering bugs (e.g. a static constructor that tries to box).
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// The single kernel heap. Everything in alloc::* routes through here.
pub struct BumpHeap;

/// Bring the heap online. Must run after the frame allocator (so future
/// versions can grow HEAP from physical memory) and before paging
/// activation (so paging::activate can Box its log records).
pub fn init() {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        panic!("[memory] heap::init called twice");
    }
    CURSOR.store(0, Ordering::SeqCst);
    log::info!(
        "[memory] bump heap online: {} bytes ready",
        HEAP.len()
    );
}

unsafe impl GlobalAlloc for BumpHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !INITIALIZED.load(Ordering::Relaxed) {
            return core::ptr::null_mut();
        }
        let size = layout.size();
        let align = layout.align();

        // Align the cursor up to the requested alignment.
        let cur = CURSOR.load(Ordering::Acquire);
        let aligned = (cur + align - 1) & !(align - 1);
        let next = aligned.checked_add(size).unwrap_or(usize::MAX);

        if next > HEAP.len() {
            log::error!(
                "[memory] heap exhausted (need {:#x}, have {:#x})",
                size,
                HEAP.len() - cur
            );
            return core::ptr::null_mut();
        }

        // CAS the cursor. Phase 2 is uniprocessor so the loop almost
        // never spins, but we keep it lock-free for forward-compat with
        // SMP bring-up in Phase 4.
        let _ = CURSOR.compare_exchange(
            cur,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        // SAFETY: aligned..next is within HEAP (checked above) and is
        // not aliased because CURSOR is the sole owner of the bump
        // region for the duration of this call.
        unsafe { HEAP.as_mut_ptr().add(aligned) }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator — dealloc is a no-op. We log a warning at debug
        // level so the Phase 3 transition can spot hot deallocation
        // paths that will benefit from the buddy/slab.
        log::debug!(
            "[memory] bump-heap dealloc ignored (Phase 3 will handle this)"
        );
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: delegation to alloc returns either a valid pointer or
        // null; we then zero layout.size() bytes, which is safe because
        // the bump allocator just reserved exactly that many bytes.
        let ptr = unsafe { self.alloc(layout) };
        if !ptr.is_null() {
            unsafe { core::ptr::write_bytes(ptr, 0, layout.size()) };
        }
        ptr
    }
}

/// Declare BumpHeap as the kernel's #[global_allocator]. After this,
/// every use of alloc::* in the kernel routes through BumpHeap::alloc.
#[global_allocator]
static ALLOCATOR: BumpHeap = BumpHeap;

/// OOM handler — Phase 2 just halts. Phase 3 will swap to a recoverable
/// policy (shrink caches, evict agent sandboxes, then panic if still
/// out of memory).
#[alloc_error_handler]
fn oom(layout: Layout) -> ! {
    log::error!(
        "[memory] kernel heap OOM (layout size {:#x}, align {:#x})",
        layout.size(),
        layout.align()
    );
    panic!("[memory] kernel heap out of memory");
}

/// Suppress the unused-import warning for AllocError — it is re-exported
/// so Phase 3's slab can return it without touching the import list.
#[allow(dead_code)]
fn _alloc_error_anchor() -> AllocError {
    AllocError
}
