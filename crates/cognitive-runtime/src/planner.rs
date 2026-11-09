//! AIOS Cognitive Runtime — Planner.
//!
//! Turns a high-level goal into a task DAG. The actual decomposition
//! (goal -> sub-tasks) is delegated to the lower-layer AI Engine; the
//! planner's job is to wire the returned tasks into a directed acyclic
//! graph, validate acyclicity, and expose a topological execution
//! order. The Agent Federation layer above walks that order.
//!
//! Author: Jad Jbara <jad@aios.dev>

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::AIEngine;

/// Stable identifier for a task within a graph.
pub type TaskId = u32;

/// A capability tag carried by every task. The Decision Engine's
/// Immutable Core gate refuses to emit any action whose capability is
/// KernelMutation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    Userspace,
    Filesystem,
    Network,
    KernelMutation,
}

/// A goal is a single natural-language description supplied by the
/// Agent Federation layer above (or by run_goal on the runtime).
pub struct Goal<'a> {
    pub description: &'a str,
}

impl<'a> Goal<'a> {
    pub const fn new(description: &'a str) -> Self {
        Self { description }
    }
}

/// A unit of work. deps lists the tasks that must complete before this
/// task can run; together the deps form the task DAG.
pub struct Task {
    pub id: TaskId,
    pub description: String,
    pub deps: Vec<TaskId>,
    pub capability: Capability,
}

/// The full task graph for one goal. roots are the tasks with no
/// dependencies — the entry points for topological execution.
pub struct TaskGraph {
    pub tasks: Vec<Task>,
    pub roots: Vec<TaskId>,
    pub acyclic: bool,
}

impl TaskGraph {
    /// Build a graph from a flat task list, computing roots and
    /// validating acyclicity via Kahn's algorithm.
    pub fn new(tasks: Vec<Task>) -> Self {
        let mut roots: Vec<TaskId> = tasks
            .iter()
            .filter(|t| t.deps.is_empty())
            .map(|t| t.id)
            .collect();
        roots.sort_unstable();
        let mut graph = Self { tasks, roots, acyclic: false };
        let acyclic = graph.topo_order().is_some();
        graph.acyclic = acyclic;
        graph
    }

    /// Kahn's algorithm. Returns None if the graph contains a cycle
    /// (which would make a plan un-executable). The returned order is a
    /// valid topological order: every dep precedes its dependent.
    pub fn topo_order(&self) -> Option<Vec<TaskId>> {
        let n = self.tasks.len();
        let mut indeg: BTreeMap<TaskId, usize> = BTreeMap::new();
        for t in &self.tasks {
            indeg.insert(t.id, t.deps.len());
        }
        // start from roots (already sorted ascending for determinism)
        let mut queue: Vec<TaskId> = self.roots.clone();
        let mut order: Vec<TaskId> = Vec::with_capacity(n);
        while let Some(id) = queue.pop() {
            order.push(id);
            // for each task that depends on id, decrement indegree
            for t in &self.tasks {
                if t.deps.contains(&id) {
                    if let Some(remain) = indeg.get_mut(&t.id) {
                        *remain -= 1;
                        if *remain == 0 {
                            queue.push(t.id);
                        }
                    }
                }
            }
        }
        if order.len() == n {
            Some(order)
        } else {
            None
        }
    }

    /// Number of tasks in the graph.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// True if the graph holds no tasks.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}

/// The Planner. Stateless — the AIEngine owns all decomposition state.
pub struct Planner;

impl Planner {
    pub const fn new() -> Self {
        Self
    }

    /// Plan a goal into a task DAG.
    ///
    /// NOTE: the AIEngine fills the decomposition (goal -> sub-tasks,
    /// including ids and deps). The planner only wraps the result in a
    /// TaskGraph, validates acyclicity, and computes the topological
    /// order. This keeps the cognitive runtime independent of any
    /// specific LLM backend.
    pub fn plan(&mut self, goal: &str, engine: &mut AIEngine) -> TaskGraph {
        let tasks = engine.decompose(goal);
        TaskGraph::new(tasks)
    }
}
