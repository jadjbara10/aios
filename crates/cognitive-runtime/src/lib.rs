//! # AIOS Cognitive Runtime
//!
//! The think-loop that sits above the AI Engine and below the Agent
//! Federation. For each goal it cycles **plan -> reason -> decide**:
//!
//! 1. Planner decomposes the goal into a task DAG (delegation to the
//!    AI Engine for the actual sub-task synthesis).
//! 2. Reasoner weighs evidence into per-task Beliefs with a confidence
//!    score, stubbing chain-of-thought into the local LLM.
//! 3. Decision Engine picks a concrete Action under a Policy, gated by
//!    the Immutable Core (no kernel-mutating action ever leaves).
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub mod planner;
pub mod reasoner;
pub mod decision;

pub use planner::{Capability, Goal, Planner, Task, TaskGraph, TaskId};
pub use reasoner::{Belief, Context, Evidence, Reasoner};
pub use decision::{Action, AgentId, DecisionEngine, Policy, SafetyGate};

/// Boundary to the lower-layer ai-engine crate.
///
/// In production this is "use ai_engine::AIEngine;" — the real local
/// LLM. Here it is a tiny stub so the cognitive runtime compiles
/// standalone under no_std. The two methods the runtime actually calls
/// are decompose (goal -> tasks) and chain_of_thought (prompt ->
/// rationale).
pub struct AIEngine {
    calls: u32,
}

impl AIEngine {
    pub const fn new() -> Self {
        Self { calls: 0 }
    }

    /// Decompose a goal into a task list. The real implementation lives
    /// in the ai-engine crate; this stub returns a single userspace
    /// task echoing the goal, which is enough to drive the think-loop.
    pub fn decompose(&mut self, goal: &str) -> Vec<Task> {
        self.calls = self.calls.saturating_add(1);
        let task = Task {
            id: 0,
            description: String::from(goal),
            deps: Vec::new(),
            capability: Capability::Userspace,
        };
        let mut tasks = Vec::new();
        tasks.push(task);
        tasks
    }

    /// Produce a chain-of-thought rationale for a prompt. Stubbed.
    pub fn chain_of_thought(&mut self, prompt: &str) -> String {
        self.calls = self.calls.saturating_add(1);
        format!("[cot] {}", prompt)
    }

    /// Total number of engine calls made through this handle.
    pub fn calls(&self) -> u32 {
        self.calls
    }
}

/// The Cognitive Runtime. Owns the planner, reasoner, decision engine,
/// and the AIEngine boundary handle.
pub struct CognitiveRuntime {
    pub planner: Planner,
    pub reasoner: Reasoner,
    pub decision: DecisionEngine,
    pub engine: AIEngine,
}

impl CognitiveRuntime {
    pub const fn new() -> Self {
        Self {
            planner: Planner::new(),
            reasoner: Reasoner::new(),
            decision: DecisionEngine::new(),
            engine: AIEngine::new(),
        }
    }

    /// Run one think-loop cycle for a goal: plan -> reason -> decide.
    ///
    /// Returns the concrete Action the runtime proposes to take next.
    /// The Immutable Core gate inside the Decision Engine guarantees
    /// this never returns a kernel-mutating action.
    pub fn run_goal(&mut self, goal: &str) -> Action {
        // 1. plan: goal -> task DAG (engine fills the decomposition).
        let graph = self.planner.plan(goal, &mut self.engine);

        // 2. reason: weigh evidence into beliefs. Empty context here —
        //    the Agent Federation layer populates real evidence before
        //    invoking run_goal in production.
        let ctx = Context::empty();
        let mut beliefs: Vec<Belief> = Vec::with_capacity(graph.tasks.len());
        for task in &graph.tasks {
            let belief = self.reasoner.evaluate(task, &ctx, &mut self.engine);
            beliefs.push(belief);
        }

        // 3. decide: policy-constrained action selection + Core gate.
        let policy = Policy::default_cognitive();
        self.decision.decide(&beliefs, &policy)
    }
}
