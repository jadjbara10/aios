//! AIOS GUI desktop — agent-centric framebuffer compositor.
//!
//! Owns the UEFI GOP framebuffer left behind by the bootloader and
//! renders an agent-centric desktop: each window is the surface of an
//! AIOS agent, drawn back-to-front by 'Desktop::compositor'.
//!
//! Author: Jad Jbara <jad@aios.dev>
#![no_std]

extern crate alloc;

use crate::framebuffer::{draw_text, Framebuffer};
use crate::input::{InputEvent, InputQueue};
use crate::window::WindowManager;

pub mod framebuffer;
pub mod input;
pub mod window;

pub use framebuffer::{Color, Framebuffer};
pub use input::{InputEvent, InputQueue};
pub use window::{AgentId, Window, WindowManager};

/// The desktop is the root of the GUI. It owns the framebuffer, the
/// z-ordered window manager, and the input queue that the kernel's IRQ
/// layer pushes events into.
pub struct Desktop {
    fb: Framebuffer,
    windows: WindowManager,
    input: InputQueue,
    running: bool,
}

impl Desktop {
    /// Bind a desktop to the GOP framebuffer the bootloader handed us.
    /// 'fb_addr' is the physical address; the bootloader identity-maps
    /// the first 1 GiB so we can write through it directly.
    pub fn new(fb_addr: usize, width: u32, height: u32, stride: u32, bpp: u8) -> Self {
        let fb = Framebuffer::new(fb_addr, width, height, stride, bpp);
        // Wipe to a calm deep-violet background so the first composite
        // is not stale firmware pixels.
        fb.fill_rect(0, 0, width, height, 0x0a0a12);
        Self {
            fb,
            windows: WindowManager::new(),
            input: InputQueue::new(),
            running: false,
        }
    }

    pub fn windows(&self) -> &WindowManager { &self.windows }
    pub fn windows_mut(&mut self) -> &mut WindowManager { &mut self.windows }
    pub fn input(&self) -> &InputQueue { &self.input }

    /// One frame of the compositor loop:
    ///   1. drain input -> route to focused window
    ///   2. layout (windows own their geometry — no-op for v0.1)
    ///   3. composite back-to-front into the framebuffer
    pub fn tick(&mut self) -> bool {
        while let Some(ev) = self.input.poll() {
            self.route(ev);
        }
        self.compositor();
        self.running
    }

    /// Dispatch a single input event to the focused agent window.
    /// Mouse hits walk z-order top-first so a click lands on the
    /// visually topmost surface.
    fn route(&mut self, ev: InputEvent) {
        match ev {
            InputEvent::KeyDown(sc) => {
                if let Some(agent) = self.windows.focused_agent() {
                    window::dispatch_key(agent, sc);
                }
            }
            InputEvent::KeyUp(_) => {}
            InputEvent::MouseMove(x, y) => self.windows.set_hover(x, y),
            InputEvent::MouseDown(x, y) => self.windows.click_focus(x, y),
            InputEvent::MouseUp(_, _) => {}
        }
    }

    /// Draw all windows back-to-front, then the desktop chrome on top.
    /// Every pixel store goes through 'Framebuffer::put_pixel', which
    /// bounds-checks the coordinate before the volatile write — so the
    /// compositor itself has no 'unsafe'.
    fn compositor(&self) {
        for win in self.windows.iter_z_back_to_front() {
            self.fb.blit(&win.surface, win.w, win.h, win.x, win.y);
            self.fb.draw_rect(win.x, win.y, win.w, win.h, 0x8b5cf6);
            draw_text(&self.fb, win.x + 4, win.y - 14, &win.title, 0xe5e7eb);
        }
    }

    /// Ask the loop to exit after the next frame. Used by the kernel
    /// signal path; the GUI itself never initiates shutdown.
    pub fn request_stop(&mut self) { self.running = false; }
}

/// Boot-time entry. Called from the kernel main thread once the
/// framebuffer BootInfo field is non-null. Never returns.
pub fn run(desktop: &mut Desktop) -> ! {
    desktop.running = true;
    while desktop.tick() {
        // The kernel scheduler yields here; we have no real vsync yet,
        // so the loop bounds on the input queue going idle.
        core::hint::spin_loop();
    }
    unreachable!("desktop loop exited without panic");
}
