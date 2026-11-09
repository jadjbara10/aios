//! AIOS Knowledge System - Phase 4 crate root.
//!
//! In AIOS, every File, Note, Memory, and Tool is a Knowledge Object (KO):
//! a content-addressed blob carrying a vector embedding and capability
//! tags. This crate owns the KO model, the vector index, and the
//! short/long-term memory tiers.
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

pub mod embedding;
pub mod vector;
pub mod memory;

pub use embedding::{Embedder, normalize, LocalEmbedder};
pub use memory::{ShortTermMemory, LongTermMemory};
pub use vector::VectorIndex;

/// Identifier for a Knowledge Object. Stable across reboots (content hash).
pub type KoId = [u8; 16];

/// Kind discriminant. Caps on a KO constrain which agents may touch it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KoKind {
    File,
    Note,
    Memory,
    Tool,
}

impl fmt::Display for KoKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            KoKind::File => "file",
            KoKind::Note => "note",
            KoKind::Memory => "memory",
            KoKind::Tool => "tool",
        };
        f.write_str(s)
    }
}

/// Capability tag. Tags gate which subsystems may read/write a KO.
///
/// Tags are stable short strings (e.g. "agent:planner", "vfs:read",
/// "mem:long"). They travel with the object so policy enforcement is
/// local to the KO, not the caller.
pub type Cap = String;

/// A Knowledge Object: content + embedding + capability tags.
///
/// The embedding is a unit-norm f32 vector produced by the canonical
/// Embedder (the AI Engine). The content_hash is what makes the KoId
/// stable: identical content -> identical id.
#[derive(Debug, Clone)]
pub struct KnowledgeObject {
    pub id: KoId,
    pub kind: KoKind,
    pub content_hash: [u8; 32],
    pub embedding: Vec<f32>,
    pub tags: Vec<Cap>,
}

impl KnowledgeObject {
    /// Construct a KO with an empty tag set; callers add caps as policy.
    pub fn new(id: KoId, kind: KoKind, content_hash: [u8; 32], embedding: Vec<f32>) -> Self {
        KnowledgeObject {
            id,
            kind,
            content_hash,
            embedding,
            tags: Vec::new(),
        }
    }

    /// Attach a capability tag. Idempotent: adding twice is a no-op.
    pub fn tag(&mut self, cap: &str) {
        if !self.tags.iter().any(|t| t.as_str() == cap) {
            self.tags.push(String::from(cap));
        }
    }

    /// True iff the KO grants the requested capability tag.
    pub fn has_cap(&self, cap: &str) -> bool {
        self.tags.iter().any(|t| t.as_str() == cap)
    }
}

/// In-memory KnowledgeStore. Phase 4.1 will swap the backing Vec for a
/// persistent object log on the immutable core; the trait surface stays.
pub struct KnowledgeStore {
    objects: Vec<KnowledgeObject>,
}

impl KnowledgeStore {
    pub fn new() -> Self {
        KnowledgeStore { objects: Vec::new() }
    }

    /// Insert or replace a KO by id. Returns the previous object, if any.
    pub fn put(&mut self, ko: KnowledgeObject) -> Option<KnowledgeObject> {
        let slot = self.objects.iter_mut().find(|o| o.id == ko.id);
        slot.map(|s| core::mem::replace(s, ko))
    }

    /// Fetch a KO by id.
    pub fn get(&self, id: &KoId) -> Option<&KnowledgeObject> {
        self.objects.iter().find(|o| &o.id == id)
    }

    /// Brute-force semantic query: top-k KOs by cosine similarity to the
    /// probe vector. The VectorIndex is the canonical path; this is a
    /// convenience for callers that already hold an embedding.
    pub fn query(&self, probe: &[f32], k: usize) -> Vec<(&KnowledgeObject, f32)> {
        let mut scored: Vec<(&KnowledgeObject, f32)> = self
            .objects
            .iter()
            .map(|o| (o, vector::cosine(probe, &o.embedding)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }
}
