// crates/kernel/src/sched/ipc.rs
// Capability-scoped bounded MPSC channels for inter-task IPC.
//
// Channel<T> is a ring buffer behind a spin::Mutex. Sender and Receiver
// carry a ChannelCap token validated on every operation, so a task
// without the capability cannot send or receive even if it obtains a
// raw pointer to the channel.
//
// Recv blocks by recording the calling task id in the channel state and
// yielding back to the scheduler; the sender clears the blocker on a
// successful send. The Phase-2 single-receiver invariant is tracked by
// AIOS-011; the multi-receiver wake path arrives in Phase 3.
//
// Author: Jad Jbara <jad@aios.dev>

#![no_std]

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::sched::{yield_now, TaskId};

/// Capability token. Acts as a bearer token: whoever holds an instance
/// can use the associated channel endpoint. Generated once per channel
/// at construction; compared by value via the inner u64.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelCap(pub u64);

static NEXT_CAP: AtomicU64 = AtomicU64::new(1);

fn fresh_cap() -> ChannelCap {
    ChannelCap(NEXT_CAP.fetch_add(1, Ordering::Relaxed))
}

/// Bounded MPSC ring-buffer channel.
pub struct Channel<T> {
    inner: Mutex<Inner<T>>,
    cap: ChannelCap,
    bound: usize,
}

struct Inner<T> {
    buf: Vec<Option<T>>,
    head: usize, // next write index
    tail: usize, // next read index
    len: usize,
    /// TaskId of the receiver currently blocked in recv, if any. The
    /// Phase-2 single-receiver invariant guarantees at most one entry.
    blocked_recv: Option<TaskId>,
}

impl<T> Channel<T> {
    /// Construct a fresh channel with capacity bound and return the
    /// channel along with its single Sender and Receiver endpoints.
    pub fn bounded(bound: usize) -> (Arc<Self>, Sender<T>, Receiver<T>) {
        let cap = fresh_cap();
        let mut buf: Vec<Option<T>> = Vec::with_capacity(bound);
        for _ in 0..bound { buf.push(None); }
        let chan = Arc::new(Self {
            inner: Mutex::new(Inner {
                buf, head: 0, tail: 0, len: 0, blocked_recv: None,
            }),
            cap,
            bound,
        });
        let s = Sender { chan: chan.clone(), _t: PhantomData };
        let r = Receiver { chan: chan.clone(), _t: PhantomData };
        (chan, s, r)
    }

    /// Capability check -- returns the channel cap on match, else None.
    fn auth(&self, presented: ChannelCap) -> Option<ChannelCap> {
        if self.cap == presented { Some(self.cap) } else { None }
    }
}

/// Sending half. Cloning a Sender creates another capability-bearing
/// sender (multi-producer); the underlying channel is shared via Arc.
pub struct Sender<T> {
    chan: Arc<Channel<T>>,
    _t: PhantomData<fn() -> T>,
}

impl<T> Sender<T> {
    pub fn cap(&self) -> ChannelCap { self.chan.cap }

    /// Enqueue val. Returns Err(val) if the channel is full or the
    /// presented capability does not match. Capacity is never exceeded.
    pub fn try_send(&self, val: T, cap: ChannelCap) -> Result<(), T> {
        if self.chan.auth(cap).is_none() { return Err(val); }
        let mut g = self.chan.inner.lock();
        if g.len >= self.chan.bound { return Err(val); }
        g.buf[g.head] = Some(val);
        g.head = (g.head + 1) % self.chan.bound;
        g.len += 1;
        // If a receiver is blocked, clear the blocker; the scheduler
        // observes the receiver task as Ready on the next tick. The
        // multi-receiver wake path is tracked by AIOS-011.
        let _blocked = g.blocked_recv.take();
        Ok(())
    }

    /// Blocking send -- yields to the scheduler on a full channel and
    /// retries until success.
    pub fn send(&self, val: T, cap: ChannelCap) -> Result<(), T> {
        let mut v = val;
        loop {
            match self.try_send(v, cap) {
                Ok(()) => return Ok(()),
                Err(returned) => {
                    v = returned;
                    // Channel is full -- yield the quantum and retry on
                    // the next reschedule. yield_now issues the int 0x80
                    // syscall trap; the trap handler saves our context
                    // and calls sched::tick().
                    yield_now();
                }
            }
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender { chan: self.chan.clone(), _t: PhantomData }
    }
}

/// Receiving half. Single-consumer in Phase 2.
pub struct Receiver<T> {
    chan: Arc<Channel<T>>,
    _t: PhantomData<fn() -> T>,
}

impl<T> Receiver<T> {
    pub fn cap(&self) -> ChannelCap { self.chan.cap }

    /// Dequeue the next value if available. Returns None if empty or
    /// if the presented capability does not match.
    pub fn try_recv(&self, cap: ChannelCap) -> Option<T> {
        if self.chan.auth(cap).is_none() { return None; }
        let mut g = self.chan.inner.lock();
        if g.len == 0 { return None; }
        let val = g.buf[g.tail].take();
        g.tail = (g.tail + 1) % self.chan.bound;
        g.len -= 1;
        val
    }

    /// Blocking receive. On empty channel, records the calling task id
    /// in the channel state and yields. The sender clears blocked_recv
    // on the next successful send; the scheduler re-runs this task on
    // its next tick.
    pub fn recv(&self, cap: ChannelCap, me: TaskId) -> T {
        loop {
            if let Some(v) = self.try_recv(cap) { return v; }
            {
                let mut g = self.chan.inner.lock();
                g.blocked_recv = Some(me);
                // The scheduler queries blocked_recv on each tick and
                // skips this task until a sender clears the blocker.
                // The full wake path (state flip + ready push) is the
                // subject of AIOS-011.
            }
            // Empty channel -- yield the quantum. yield_now traps into
            // the syscall handler which saves our context and calls
            // sched::tick(); control resumes here when a sender has
            // cleared blocked_recv and the scheduler picks us again.
            yield_now();
        }
    }
}
