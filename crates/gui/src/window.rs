//! Agent-centric window manager.
//!
//! Each window is owned by an 'AgentId' — a cognitive agent in the
//! AIOS runtime. The window manager keeps a z-ordered Vec of windows
//! and a 'focused' window id. There is no overlap negotiation: agents
//! own their surface geometry; the compositor paints whatever they ask.
//!
//! Author: Jad Jbara <jad@aios.dev>
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;

/// Identifier of the agent that owns a window. Mirrors
/// 'crates/agents::AgentId' to keep the GUI crate decoupled from the
/// agents crate's exact type identity.
pub type AgentId = u64;

/// A rectangular agent surface. 'surface' is owned xrgb8888 pixels,
/// 'w * h' of u32, row-major. The compositor blits it verbatim.
#[derive(Clone)]
pub struct Window {
    pub id: u64,
    pub agent: AgentId,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub z: u32,
    pub title: String,
    pub surface: Vec<u32>,
}

impl Window {
    /// Allocate a window at (x,y) with width w, height h, owned by
    /// agent. Surface starts black so the first composite is not
    /// garbage.
    pub fn new(id: u64, agent: AgentId, x: i32, y: i32, w: u32, h: u32, title: &str) -> Self {
        let len = (w as usize).saturating_mul(h as usize);
        Self {
            id,
            agent,
            x,
            y,
            w,
            h,
            z: 0,
            title: String::from(title),
            surface: vec![0; len],
        }
    }

    /// Paint one pixel on this surface in local coordinates. The
    /// compositor blits the surface verbatim, so this is the only way
    /// an agent updates its visible pixels.
    pub fn put(&mut self, lx: u32, ly: u32, color: u32) {
        if lx >= self.w || ly >= self.h { return; }
        let i = (ly as usize) * (self.w as usize) + (lx as usize);
        if let Some(slot) = self.surface.get_mut(i) {
            *slot = color;
        }
    }

    /// Fill the whole surface with a solid color.
    pub fn clear(&mut self, color: u32) {
        for px in self.surface.iter_mut() {
            *px = color;
        }
    }
}

/// Z-ordered collection of windows + a focused window id.
pub struct WindowManager {
    windows: Vec<Window>,
    next_id: u64,
    focused: Option<u64>,
    hover: (i32, i32),
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            next_id: 1,
            focused: None,
            hover: (0, 0),
        }
    }

    /// Spawn a window for an agent. Returns its window id. New windows
    /// land on top (highest z) and steal focus.
    pub fn spawn(&mut self, agent: AgentId, x: i32, y: i32, w: u32, h: u32, title: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let z = self.windows.iter().map(|w| w.z).max().unwrap_or(0) + 1;
        let mut win = Window::new(id, agent, x, y, w, h, title);
        win.z = z;
        self.focused = Some(id);
        self.windows.push(win);
        id
    }

    /// Close the window with this id. If it was focused, focus falls
    /// back to the highest-z remaining window.
    pub fn close(&mut self, id: u64) {
        if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            self.windows.remove(pos);
        }
        if self.focused == Some(id) {
            self.focused = self.windows.iter().max_by_key(|w| w.z).map(|w| w.id);
        }
    }

    /// Bring window id to the top of the z-stack and focus it.
    pub fn focus(&mut self, id: u64) {
        let top = self.windows.iter().map(|w| w.z).max().unwrap_or(0) + 1;
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.z = top;
            self.focused = Some(id);
        }
    }

    /// Move a window to a new top-left. Bounds are advisory — the
    /// compositor clips on draw.
    pub fn move_window(&mut self, id: u64, x: i32, y: i32) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.x = x;
            w.y = y;
        }
    }

    /// Return the agent that owns the currently focused window. This
    /// is the route target for keyboard events.
    pub fn focused_agent(&self) -> Option<AgentId> {
        let id = self.focused?;
        self.windows.iter().find(|w| w.id == id).map(|w| w.agent)
    }

    pub fn focused(&self) -> Option<u64> { self.focused }
    pub fn set_hover(&mut self, x: i32, y: i32) { self.hover = (x, y); }
    pub fn len(&self) -> usize { self.windows.len() }
    pub fn is_empty(&self) -> bool { self.windows.is_empty() }

    /// Hit-test a mouse press top-first (highest z) and focus the hit.
    pub fn click_focus(&mut self, x: i32, y: i32) {
        let mut order: Vec<&Window> = self.windows.iter().collect();
        order.sort_by_key(|w| core::cmp::Reverse(w.z));
        for w in order {
            if x >= w.x && x < w.x + w.w as i32
                && y >= w.y && y < w.y + w.h as i32
            {
                self.focus(w.id);
                return;
            }
        }
    }

    /// Iterate windows back-to-front (lowest z first) so the compositor
    /// paints the topmost window last.
    pub fn iter_z_back_to_front(&self) -> impl Iterator<Item = &Window> {
        let mut v: Vec<&Window> = self.windows.iter().collect();
        v.sort_by_key(|w| w.z);
        v.into_iter()
    }
}

/// Deliver a keyboard scancode to the agent that owns the focused
/// window. The agent runtime registers a listener per 'AgentId'; the
/// GUI does not interpret keys — it just routes them.
pub fn dispatch_key(_agent: AgentId, _scancode: u8) {
    // Bridge into crates/agents once AIOS-014 lands. For now the call
    // site is wired so the GUI crate compiles standalone.
}
