//! AIOS Cognitive Runtime — Reasoner.
//!
//! Weighs evidence into per-task Beliefs with a confidence score, and
//! asks the local LLM (via the AIEngine boundary) for a chain-of-thought
//! rationale. The Reasoner never decides an action — that is the
//! Decision Engine's job; it only judges how confident we should be.
//!
//! Author: Jad Jbara <jad@aios.dev>

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::planner::{Capability, Task, TaskId};
use crate::AIEngine;

/// A single piece of evidence bearing on a task. supports == true
/// raises confidence; false lowers it. weight is 0..=100.
pub struct Evidence {
    pub source: String,
    pub weight: u8,
    pub supports: bool,
}

/// The context in which a task is evaluated: a set of evidence plus
/// (eventually) budget/namespace fields. Kept as a struct so the
/// evaluate signature stays stable as the runtime grows.
pub struct Context {
    pub evidence: Vec<Evidence>,
}

impl Context {
    pub const fn empty() -> Self {
        Self { evidence: Vec::new() }
    }
}

/// A Reasoner's judgement of one task. confidence is -100..=100:
/// negative means the evidence argues against executing the task.
/// The capability is mirrored from the task so the Decision Engine
/// can gate on it without re-touching the graph.
pub struct Belief {
    pub task_id: TaskId,
    pub confidence: i8,
    pub capability: Capability,
    pub rationale: String,
}

/// The Reasoner. Stateless — all LLM state lives in the AIEngine.
pub struct Reasoner;

impl Reasoner {
    pub const fn new() -> Self {
        Self
    }

    /// Evaluate a task under a context, producing a Belief.
    ///
    /// Evidence is weighed additively (supports adds weight, against
    /// subtracts), then normalized to -100..=100. The chain-of-thought
    /// rationale is a stub call into the AIEngine — in production this
    /// hits the local LLM in the ai-engine crate.
    pub fn evaluate(
        &mut self,
        task: &Task,
        ctx: &Context,
        engine: &mut AIEngine,
    ) -> Belief {
        let mut score: i32 = 0;
        let mut total: i32 = 0;
        for ev in &ctx.evidence {
            total = total.saturating_add(ev.weight as i32);
            if ev.supports {
                score = score.saturating_add(ev.weight as i32);
            } else {
                score = score.saturating_sub(ev.weight as i32);
            }
        }
        let confidence: i8 = if total == 0 {
            0
        } else {
            let raw = (score * 100) / total;
            raw.clamp(-100, 100) as i8
        };
        let rationale =
            engine.chain_of_thought(&format!("evaluate: {}", task.description));
        Belief {
            task_id: task.id,
            confidence,
            capability: task.capability,
            rationale,
        }
    }
}
