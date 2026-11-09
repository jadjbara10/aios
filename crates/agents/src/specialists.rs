// crates/agents/src/specialists.rs
//! Specialist agents — Network, Hardware, Coding.
//!
//! Each specialist implements [crate::Agent], holds a minimal capability
//! set per least-privilege, and translates incoming [crate::Payload::Goal]s
//! into subsystem ABI calls. None carries [crate::Capability::Spawn].
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

#[macro_use]
extern crate alloc;

use alloc::format;

use crate::{Agent, AgentId, Capability, Effect, Message, Payload};

// Kernel ABI — symbols provided by the kernel syscall shim.
// SAFETY: the kernel guarantees these symbols exist at the link addresses
// declared here, follow the C ABI, and accept the documented argument
// types. The extern block is marked unsafe to assert that soundness.
unsafe extern "C" {
    /// Push bytes into the netstack socket bound to a NetworkAgent.
    fn netstack_send(handle: u64, ptr: *const u8, len: usize) -> usize;
    /// Read a 32-bit MMIO word through the HAL bus for a HardwareAgent.
    fn hal_read32(handle: u64, offset: u32) -> u32;
    /// Submit a source blob to a fresh sandbox for a CodingAgent.
    fn sandbox_submit(handle: u64, sid: u64, ptr: *const u8, len: usize) -> i32;
}

/// NetworkAgent — wraps the netstack crate. Holds only [Capability::Net].
pub struct NetworkAgent {
    id: AgentId,
    /// Kernel netstack handle (opaque token).
    netstack: u64,
}

impl NetworkAgent {
    /// Construct a NetworkAgent bound to a netstack handle.
    pub fn new(id: AgentId, netstack: u64) -> Self {
        Self { id, netstack }
    }

    /// Push a payload into the netstack; returns bytes written.
    fn net_write(&self, bytes: &[u8]) -> usize {
        // SAFETY: netstack is a kernel-issued handle valid for the lifetime
        // of this agent; the underlying socket is mapped and writable. We
        // pass a valid byte slice that outlives the call and never retain
        // the handle past it.
        unsafe { netstack_send(self.netstack, bytes.as_ptr(), bytes.len()) }
    }
}

impl Agent for NetworkAgent {
    fn id(&self) -> AgentId {
        self.id
    }
    fn capabilities(&self) -> &[Capability] {
        const NET_CAPS: [Capability; 1] = [Capability::Net];
        &NET_CAPS
    }
    fn handle(&mut self, msg: Message) -> Effect {
        if let Payload::Goal(text) = &msg.payload {
            let stripped = text.strip_prefix("net:").unwrap_or(text.as_str());
            let _ = self.net_write(stripped.as_bytes());
            return Effect::Send(Message {
                from: self.id,
                to: AgentId::ROOT,
                payload: Payload::Final(format!("net-ok:{}", stripped.len())),
            });
        }
        Effect::Continue
    }
}

/// HardwareAgent — wraps the HAL + driver-gen. Holds only [Capability::Hardware].
pub struct HardwareAgent {
    id: AgentId,
    /// HAL bus handle (opaque token).
    hal: u64,
}

impl HardwareAgent {
    /// Construct a HardwareAgent.
    pub fn new(id: AgentId, hal: u64) -> Self {
        Self { id, hal }
    }

    /// Issue a HAL read at a given MMIO offset.
    fn hal_read(&self, offset: u32) -> u32 {
        // SAFETY: hal is a kernel-issued HAL bus handle valid for the
        // lifetime of this agent. The kernel validates offset against the
        // device's MMIO region; we read one 32-bit word and never retain it.
        unsafe { hal_read32(self.hal, offset) }
    }
}

impl Agent for HardwareAgent {
    fn id(&self) -> AgentId {
        self.id
    }
    fn capabilities(&self) -> &[Capability] {
        const HW_CAPS: [Capability; 1] = [Capability::Hardware];
        &HW_CAPS
    }
    fn handle(&mut self, msg: Message) -> Effect {
        if let Payload::Goal(text) = &msg.payload {
            let stripped = text.strip_prefix("hw:").unwrap_or(text.as_str());
            let _probe = self.hal_read(0); // probe the control register
            return Effect::Send(Message {
                from: self.id,
                to: AgentId::ROOT,
                payload: Payload::Final(format!("hw-ok:{}", stripped.len())),
            });
        }
        Effect::Continue
    }
}

/// CodingAgent — generates Rust source and submits to the sandbox.
/// Holds only [Capability::Code].
pub struct CodingAgent {
    id: AgentId,
    /// Sandbox submission handle (opaque token).
    sandbox: u64,
    /// Monotonic counter of submissions.
    submissions: u64,
}

impl CodingAgent {
    /// Construct a CodingAgent.
    pub fn new(id: AgentId, sandbox: u64) -> Self {
        Self { id, sandbox, submissions: 0 }
    }

    /// Submit a source blob to the sandbox. Returns the submission id.
    fn submit(&mut self, src: &str) -> u64 {
        let sid = self.submissions;
        self.submissions = self.submissions.wrapping_add(1);
        // SAFETY: sandbox is a kernel-issued handle; the sandbox is fresh,
        // filesystem-isolated, and the supplied pointer+len is a valid byte
        // range. We pass src.as_ptr() and its length; the sandbox copies the
        // bytes and never retains the pointer past the call.
        unsafe {
            sandbox_submit(self.sandbox, sid, src.as_ptr(), src.len());
        }
        sid
    }
}

impl Agent for CodingAgent {
    fn id(&self) -> AgentId {
        self.id
    }
    fn capabilities(&self) -> &[Capability] {
        const CODE_CAPS: [Capability; 1] = [Capability::Code];
        &CODE_CAPS
    }
    fn handle(&mut self, msg: Message) -> Effect {
        if let Payload::Goal(text) = &msg.payload {
            let stripped = text.strip_prefix("code:").unwrap_or(text.as_str());
            // Minimal generated source — real impl asks the Cognitive
            // Runtime to author it; here we wrap the goal as a comment.
            let src = format!(
                "// generated by ag#{}\n// goal: {}\nfn main() {{}}\n",
                self.id.0,
                stripped
            );
            let sid = self.submit(&src);
            return Effect::Send(Message {
                from: self.id,
                to: AgentId::ROOT,
                payload: Payload::Final(format!("code-ok:sub#{}", sid)),
            });
        }
        Effect::Continue
    }
}
