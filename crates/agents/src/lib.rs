// crates/agents/src/lib.rs
//! AIOS agents crate — multi-agent federation core.
//!
//! Every application in AIOS is an Agent. An Agent owns an [AgentId],
//! receives [Message]s via the [AgentMessageBus], and returns an [Effect]
//! (Spawn / Send / Halt) describing side-effects it wishes the kernel to
//! perform on its behalf. The [Federation] registry owns the live Agent
//! set and is the single source of truth for capability-gated dispatch.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

#[macro_use]
extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// Message bus implementation (pub/sub + per-agent mailboxes).
pub mod bus;
/// RootAgent — sole spawner, owns the Cognitive Runtime.
pub mod root;
/// Specialist agents (Network, Hardware, Coding).
pub mod specialists;

pub use bus::AgentMessageBus;
pub use root::RootAgent;

/// Capability tokens carried by an Agent. The kernel enforces
/// least-privilege by refusing to dispatch Effects whose issuer does
/// not hold the named capability.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    /// May spawn new specialist agents (Root only).
    Spawn,
    /// May use the network stack.
    Net,
    /// May touch hardware via the HAL / driver-gen.
    Hardware,
    /// May emit and submit generated code to the sandbox.
    Code,
}

/// Stable 64-bit agent identifier. 0 is reserved for the RootAgent.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentId(pub u64);

impl AgentId {
    /// Sentinel id for the RootAgent.
    pub const ROOT: AgentId = AgentId(0);

    /// Render an id as "ag#NNN" for logs.
    pub fn label(self) -> String {
        format!("ag#{}", self.0)
    }
}

impl fmt::Debug for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AgentId({})", self.0)
    }
}

/// Payload carried by a Message.
#[derive(Clone, Debug)]
pub enum Payload {
    /// Goal decomposition request from Root to a specialist.
    Goal(String),
    /// Partial result returned by a specialist to Root.
    Partial(String),
    /// Final result returned by a specialist to Root.
    Final(String),
    /// Capability request — refused by the kernel if the holder does
    /// not carry the named capability.
    CapRequest(Capability),
    /// No-op heartbeat / liveness ping.
    Ping,
}

/// One message in flight on the bus.
#[derive(Clone, Debug)]
pub struct Message {
    /// Sender agent id.
    pub from: AgentId,
    /// Recipient agent id (broadcast handled at the bus layer).
    pub to: AgentId,
    /// Opaque payload.
    pub payload: Payload,
}

/// Side-effect an Agent asks the kernel to perform on its behalf.
#[derive(Debug)]
pub enum Effect {
    /// Do nothing this turn.
    Continue,
    /// Spawn a new specialist (name + caps). Only agents holding
    /// [Capability::Spawn] may emit this; the kernel rejects otherwise.
    Spawn { name: String, caps: Vec<Capability> },
    /// Send a follow-up Message through the bus.
    Send(Message),
    /// The agent wishes to halt itself; the Federation reaps it.
    Halt,
}

/// The Agent contract. Implementations must be deterministic and
/// side-effect free apart from the [Effect] they return — actual
/// I/O happens in the kernel.
pub trait Agent: Send {
    /// Stable identifier of this agent.
    fn id(&self) -> AgentId;
    /// Capabilities this agent holds. Checked on every Effect.
    fn capabilities(&self) -> &[Capability];
    /// Process one Message, returning the next Effect to perform.
    fn handle(&mut self, msg: Message) -> Effect;
}

/// Federation registry — owns the live Agent set keyed by [AgentId].
///
/// The RootAgent holds the only write handle; specialists receive a
/// read-only snapshot. Dispatch is single-threaded per CPU.
pub struct Federation {
    agents: Vec<(AgentId, Box<dyn Agent>)>,
    next_id: u64,
}

impl Federation {
    /// Construct an empty federation. The RootAgent is inserted by
    /// [RootAgent::federation]; that path lives in root.rs.
    pub fn new() -> Self {
        Self { agents: Vec::new(), next_id: 1 }
    }

    /// Allocate the next AgentId. Monotonic; never reuses 0 (Root).
    pub fn allocate_id(&mut self) -> AgentId {
        let id = AgentId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// Insert a fully-constructed agent. Duplicate ids are ignored.
    pub fn register(&mut self, id: AgentId, agent: Box<dyn Agent>) {
        if self.agents.iter().any(|(aid, _)| *aid == id) {
            return;
        }
        self.agents.push((id, agent));
    }

    /// Dispatch a message to its recipient. Returns [Effect::Halt]
    /// for unknown recipients so callers reap cleanly.
    pub fn dispatch(&mut self, msg: Message) -> Effect {
        for (id, agent) in self.agents.iter_mut() {
            if *id == msg.to {
                return agent.handle(msg);
            }
        }
        Effect::Halt
    }

    /// Current population count (including Root).
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// True if the federation has no agents.
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

impl Default for Federation {
    fn default() -> Self {
        Self::new()
    }
}
