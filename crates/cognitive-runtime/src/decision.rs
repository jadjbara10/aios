//! AIOS Cognitive Runtime — Decision Engine.
//!
//! Picks the next concrete Action from a set of Beliefs, under a
//! Policy. The Immutable Core gate sits at the exit: no action that
//! would mutate the kernel security base (page tables, code signing,
//! the capability table) may ever leave this module. The gate is
//! structural, not policy-configurable.
//!
//! Author: Jad Jbara <jad@aios.dev>

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::planner::{Capability, TaskId};
use crate::reasoner::Belief;

/// Federation-level agent identifier. The Agent Federation layer above
/// resolves these to concrete agent handles.
pub type AgentId = u32;

/// The four classes of action the cognitive runtime may emit.
#[derive(Clone, Debug)]
pub enum Action {
    /// Execute the task locally on the AIOS kernel.
    Run { task_id: TaskId, capability: Capability },
    /// Hand the task to a federated agent.
    Delegate { agent: AgentId, task_id: TaskId, capability: Capability },
    /// Ask the originating principal for clarification.
    Ask { query: String },
    /// Stop the think-loop. Always returned when the Immutable Core
    /// gate trips, or when no belief is actionable.
    Halt { reason: String },
}

impl Action {
    /// True if this action, once executed, would mutate the kernel
    /// security base. Such actions are forbidden by the Immutable Core.
    pub fn is_kernel_mutation(&self) -> bool {
        matches!(
            self,
            Action::Run { capability: Capability::KernelMutation, .. }
                | Action::Delegate { capability: Capability::KernelMutation, .. }
        )
    }
}

/// A Policy tunes the decision threshold. It can never weaken the
/// Immutable Core gate — the gate is not a policy field.
pub struct Policy {
    /// Minimum belief confidence to act without asking.
    pub min_confidence: i8,
    /// If true, low-confidence tasks may be delegated instead of halted.
    pub allow_delegation: bool,
    /// Confidence below which delegation is preferred over a local Run.
    pub delegate_below: i8,
}

impl Policy {
    /// A permissive default used by the runtime's think-loop.
    pub const fn default_cognitive() -> Self {
        Self {
            min_confidence: 40,
            allow_delegation: true,
            delegate_below: 50,
        }
    }
}

/// The structural Immutable Core gate. Hard-coded, not policy-driven.
pub struct SafetyGate;

impl SafetyGate {
    /// Returns Err with a static reason if the action would mutate the
    /// kernel security base. The Decision Engine converts any Err into
    /// an Action::Halt — the action is dropped, never emitted.
    pub fn check(action: &Action) -> Result<(), &'static str> {
        if action.is_kernel_mutation() {
            return Err("Immutable Core: kernel mutation forbidden");
        }
        Ok(())
    }
}

/// The Decision Engine. Stateless — policy is passed per call.
pub struct DecisionEngine;

impl DecisionEngine {
    pub const fn new() -> Self {
        Self
    }

    /// Select the next action from a belief set under a policy.
    pub fn decide(&self, beliefs: &[Belief], policy: &Policy) -> Action {
        if beliefs.is_empty() {
            return Action::Halt {
                reason: String::from("no beliefs to act on"),
            };
        }
        // Highest-confidence belief wins. Ties broken by task id for
        // determinism (lower id first).
        let best = beliefs
            .iter()
            .max_by(|a, b| a.confidence.cmp(&b.confidence).then(a.task_id.cmp(&b.task_id)))
            .expect("beliefs is non-empty");
        let proposed = Self::propose(best, policy);
        // Immutable Core gate — never bypassed, never policy-configurable.
        match SafetyGate::check(&proposed) {
            Ok(()) => proposed,
            Err(reason) => Action::Halt { reason: String::from(reason) },
        }
    }

    /// Map a winning belief to a proposed action under the policy.
    /// The proposal still has to clear the SafetyGate before it leaves.
    fn propose(best: &Belief, policy: &Policy) -> Action {
        if best.confidence < policy.min_confidence {
            return Action::Ask {
                query: String::from("insufficient confidence to proceed"),
            };
        }
        if policy.allow_delegation && best.confidence < policy.delegate_below {
            return Action::Delegate {
                agent: 0,
                task_id: best.task_id,
                capability: best.capability,
            };
        }
        Action::Run {
            task_id: best.task_id,
            capability: best.capability,
        }
    }
}
