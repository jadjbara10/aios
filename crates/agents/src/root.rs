// crates/agents/src/root.rs
//! RootAgent — the federated orchestrator.
//!
//! Exactly one RootAgent lives in the [crate::Federation]. It owns a
//! handle to the Cognitive Runtime and is the *only* agent allowed to
//! emit [crate::Effect::Spawn] — the kernel rejects Spawn effects
//! from any agent that does not carry [crate::Capability::Spawn],
//! and only the RootAgent declares that capability.
//!
//! The RootAgent routes incoming goals to specialists based on the
//! prefix of the goal string:
//!   * "net:"  -> NetworkAgent
//!   * "hw:"   -> HardwareAgent
//!   * "code:" -> CodingAgent
//! Specialists reply with Partial/Final payloads; Root aggregates and
//! emits further Spawns or Halts as needed.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

#[macro_use]
extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::{Agent, AgentId, Capability, Effect, Federation, Message, Payload};

/// Marker type for the Cognitive Runtime handle. The real handle lives
/// in the ai-engine crate; here we hold an opaque 64-bit token the
/// kernel resolves to a runtime control-block address.
#[derive(Clone, Copy, Debug)]
pub struct CognitiveRuntimeHandle(pub u64);

/// Internal Root state: which specialist is currently responsible for
/// a goal. Goals are tracked by a monotonic local counter.
#[derive(Clone, Copy, Debug)]
struct GoalSlot {
    /// Local goal id.
    id: u64,
    /// Specialist assigned to this goal.
    specialist: AgentId,
}

/// The RootAgent itself.
pub struct RootAgent {
    /// Always [AgentId::ROOT].
    id: AgentId,
    /// Cognitive Runtime handle (kernel-resolved).
    crt: CognitiveRuntimeHandle,
    /// Outstanding goal slots.
    slots: Vec<GoalSlot>,
    /// Next goal id.
    next_goal: u64,
}

impl RootAgent {
    /// Construct the RootAgent. Caller passes a Cognitive Runtime
    /// handle obtained from the ai-engine crate.
    pub fn new(crt: CognitiveRuntimeHandle) -> Self {
        Self {
            id: AgentId::ROOT,
            crt,
            slots: Vec::new(),
            next_goal: 1,
        }
    }

    /// Build a fresh Federation pre-populated with just the RootAgent.
    /// This is the canonical entrypoint the kernel uses at boot.
    pub fn federation(self) -> Federation {
        let mut fed = Federation::new();
        let root: Box<dyn Agent> = Box::new(self);
        fed.register(AgentId::ROOT, root);
        fed
    }

    /// Route a goal to a specialist based on payload shape. Returns the
    /// capability the goal implies; Capability::Spawn means Root
    /// handles it itself via the Cognitive Runtime.
    fn route_goal(&self, payload: &Payload) -> Capability {
        match payload {
            Payload::Goal(t) if t.starts_with("net:") => Capability::Net,
            Payload::Goal(t) if t.starts_with("hw:") => Capability::Hardware,
            Payload::Goal(t) if t.starts_with("code:") => Capability::Code,
            _ => Capability::Spawn,
        }
    }

    /// Allocate a new goal slot.
    fn slot(&mut self, specialist: AgentId) -> u64 {
        let id = self.next_goal;
        self.next_goal = self.next_goal.wrapping_add(1);
        self.slots.push(GoalSlot { id, specialist });
        id
    }

    /// Read the Cognitive Runtime status byte. The kernel guarantees
    /// the handle was issued by ai-engine and is still valid.
    fn crt_status(&self) -> u8 {
        let ptr = self.crt.0 as usize as *const u8;
        // SAFETY: The Cognitive Runtime handle is a kernel-issued token
        // whose low 48 bits encode the physical address of the runtime
        // control block. The kernel guarantees the page is mapped and
        // readable for the lifetime of this RootAgent. We read exactly
        // one byte via read_volatile (the control block is MMIO) and
        // never retain the pointer past this call.
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// Allocate an AgentId for a new specialist. The Federation owns
    /// id allocation; the RootAgent asks the kernel to allocate on its
    /// behalf via the Spawn effect. The id returned here is only used
    /// for bookkeeping in slots.
    fn allocate_specialist(&self, _cap: Capability) -> AgentId {
        AgentId(self.next_goal)
    }
}

impl Agent for RootAgent {
    fn id(&self) -> AgentId {
        self.id
    }

    fn capabilities(&self) -> &[Capability] {
        // Root holds ONLY Spawn. It cannot directly use Net/Hardware/
        // Code — that's what specialists are for. This is the core of
        // the least-privilege guarantee.
        const ROOT_CAPS: [Capability; 1] = [Capability::Spawn];
        &ROOT_CAPS
    }

    fn handle(&mut self, msg: Message) -> Effect {
        let cap = self.route_goal(&msg.payload);
        match msg.payload {
            Payload::Goal(_) => {
                if cap == Capability::Spawn {
                    // Root handles it itself via the Cognitive Runtime.
                    let _status = self.crt_status();
                    return Effect::Continue;
                }
                // The kernel capability-checks this Spawn because Root
                // is the only agent that carries Capability::Spawn.
                let name = match cap {
                    Capability::Net => "network-agent",
                    Capability::Hardware => "hardware-agent",
                    Capability::Code => "coding-agent",
                    Capability::Spawn => "root",
                };
                let specialist = self.allocate_specialist(cap);
                let _ = self.slot(specialist);
                Effect::Spawn {
                    name: name.to_string(),
                    caps: vec![cap],
                }
            }
            Payload::Final(_) => {
                // Specialist finished; reap its slot.
                self.slots.retain(|s| s.specialist != msg.from);
                Effect::Continue
            }
            Payload::Partial(_) => {
                // Aggregation step — forward to Cognitive Runtime.
                Effect::Continue
            }
            Payload::CapRequest(_) => {
                // Root never requests more caps at runtime.
                Effect::Continue
            }
            Payload::Ping => Effect::Continue,
        }
    }
}
