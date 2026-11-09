//! Keyboard / mouse event queue routed to the focused agent window.
//!
//! The kernel's interrupt layer pushes raw scancodes and mouse
//! positions into an 'InputQueue'. The desktop polls the queue once
//! per frame and routes each event to the focused window via
//! 'WindowManager'.
//!
//! Author: Jad Jbara <jad@aios.dev>
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use spin::Mutex;

/// A single input event. Scancodes are raw PS/2 set-2 bytes for now;
/// USB HID lands in AIOS-019. Mouse coordinates are framebuffer-space
/// absolute positions (not deltas) so the GUI can hit-test directly.
#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    /// PS/2 set-2 scancode byte (post-decode).
    KeyDown(u8),
    /// Key release; carries the same scancode for symmetry.
    KeyUp(u8),
    /// Absolute mouse move to (x, y) in framebuffer pixels.
    MouseMove(i32, i32),
    /// Left-button press at (x, y).
    MouseDown(i32, i32),
    /// Left-button release at (x, y).
    MouseUp(i32, i32),
}

/// Ring capacity. 256 events is enough for ~4 frames of full-keyboard
/// rollover at 60 Hz; we drop oldest under flood rather than block IRQs.
const QUEUE_CAP: usize = 256;

/// Lock-free would be ideal; for v0.1 we use a 'Mutex'-guarded ring
/// because the kernel is single-core at this stage and the IRQ handler
/// is the only producer. 'spin::Mutex' is the only spinlock in the
/// workspace — see 'Cargo.toml' feature 'spin_mutex'.
pub struct InputQueue {
    buf: Mutex<Vec<InputEvent>>,
}

impl InputQueue {
    pub fn new() -> Self {
        Self { buf: Mutex::new(Vec::with_capacity(QUEUE_CAP)) }
    }

    /// Push an event from interrupt context. Drops the oldest event
    /// if the ring is full (input is lossy under flood — preferred to
    /// blocking the IRQ handler).
    pub fn push(&self, ev: InputEvent) {
        let mut g = self.buf.lock();
        if g.len() >= QUEUE_CAP {
            g.remove(0);
        }
        g.push(ev);
    }

    /// Drain one event from the head. Called by the desktop once per
    /// frame until the queue is empty.
    pub fn poll(&self) -> Option<InputEvent> {
        let mut g = self.buf.lock();
        if g.is_empty() {
            None
        } else {
            Some(g.remove(0))
        }
    }

    /// Current queued depth. Diagnostic only.
    pub fn len(&self) -> usize {
        self.buf.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.lock().is_empty()
    }

    /// Drop every queued event. Used on shutdown.
    pub fn flush(&self) {
        self.buf.lock().clear();
    }
}

impl Default for InputQueue {
    fn default() -> Self {
        Self::new()
    }
}
