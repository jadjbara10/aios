//! Memory tiers: short-term ring and long-term vector store.
//!
//! Short-term memory (STM) is a fixed-capacity ring of recent KoIds -
//! fast O(1) insert, bounded footprint, evicts oldest first. Long-term
//! memory (LTM) is a VectorIndex-backed store: slower insert, but
//! semantic recall via cosine search.
//!
//! consolidate() promotes high-recall STM entries to LTM. "High recall"
//! is approximated by a hit counter: every STM probe that matches an
//! entry bumps that entry's score; entries crossing a threshold are
//! promoted to LTM where they become available to semantic search.
//!
//! Author: Jad Jbara <jad@aios.dev>

use crate::embedding::normalize;
use crate::vector::VectorIndex;
use crate::{KoId, KnowledgeObject};
use alloc::vec::Vec;

/// Default STM ring capacity (number of recent KOs held).
pub const STM_WINDOW: usize = 32;

/// Hit threshold above which an STM entry is promoted to LTM.
pub const STM_PROMOTE_THRESHOLD: u32 = 3;

/// One slot in the STM ring.
#[derive(Debug, Clone, Copy)]
struct StmSlot {
    id: KoId,
    hits: u32,
}

/// Short-term memory: a bounded ring of recent KoIds with hit counters.
///
/// The ring is *lossy* by design: when full, the oldest slot is
/// overwritten. Eviction does NOT consult hits; only consolidate() does.
pub struct ShortTermMemory {
    window: usize,
    slots: Vec<Option<StmSlot>>,
    head: usize,
    filled: usize,
}

impl ShortTermMemory {
    /// Construct an STM ring with the given window size.
    pub fn new(window: usize) -> Self {
        let cap = if window == 0 { STM_WINDOW } else { window };
        let mut slots = Vec::with_capacity(cap);
        for _ in 0..cap {
            slots.push(None);
        }
        ShortTermMemory { window: cap, slots, head: 0, filled: 0 }
    }

    /// Default-capacity ring (STM_WINDOW entries).
    pub fn with_default_window() -> Self {
        Self::new(STM_WINDOW)
    }

    /// Push a KoId into the ring. Evicts the oldest if full.
    pub fn push(&mut self, id: KoId) {
        let was_empty = self.slots[self.head].is_none();
        self.slots[self.head] = Some(StmSlot { id, hits: 0 });
        self.head = (self.head + 1) % self.window;
        if was_empty {
            self.filled += 1;
        }
    }

    /// Bump the hit counter of a resident KoId. Returns the new count,
    /// or 0 if the id is not currently in the ring.
    pub fn bump(&mut self, id: &KoId) -> u32 {
        for slot in self.slots.iter_mut().flatten() {
            if &slot.id == id {
                slot.hits = slot.hits.saturating_add(1);
                return slot.hits;
            }
        }
        0
    }

    /// Drain entries whose hit count crosses the promotion threshold,
    /// returning their KoIds. All slots are cleared after the drain.
    pub fn drain_promotable(&mut self, threshold: u32) -> Vec<KoId> {
        let mut out = Vec::new();
        for slot in self.slots.iter_mut() {
            if let Some(s) = slot.take() {
                if s.hits >= threshold {
                    out.push(s.id);
                }
            }
        }
        self.head = 0;
        self.filled = 0;
        out
    }

    /// Current occupancy (0..=window).
    pub fn len(&self) -> usize {
        self.filled
    }

    /// True when no KoIds are resident.
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }
}

/// Long-term memory: a VectorIndex keyed by KoId with semantic recall.
pub struct LongTermMemory {
    index: VectorIndex,
}

impl LongTermMemory {
    /// Construct an LTM store expecting embeddings of length dim.
    pub fn new(dim: usize) -> Self {
        LongTermMemory { index: VectorIndex::new(dim) }
    }

    /// Store a KO's (normalized) embedding in the LTM index. Returns the
    /// previous embedding for that KoId, if any.
    pub fn remember(&mut self, ko: &KnowledgeObject) -> Option<Vec<f32>> {
        let mut v = ko.embedding.clone();
        normalize(&mut v);
        self.index.insert(ko.id, v)
    }

    /// Top-k semantic recall over LTM.
    pub fn recall(&self, probe: &[f32], k: usize) -> Vec<(KoId, f32)> {
        self.index.search(probe, k)
    }

    /// Number of stored long-term KOs.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// True when the LTM index holds no embeddings.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}

/// Promote high-recall STM entries to LTM.
///
/// Drains promotable KoIds from STM, resolves each against the
/// KnowledgeStore, and inserts (normalized) into LTM. Returns the
/// number of KOs actually promoted (entries that could not be resolved
/// in the store are silently skipped; STM is still drained).
pub fn consolidate(
    stm: &mut ShortTermMemory,
    ltm: &mut LongTermMemory,
    store: &crate::KnowledgeStore,
) -> usize {
    let ids = stm.drain_promotable(STM_PROMOTE_THRESHOLD);
    let mut promoted = 0usize;
    for id in ids {
        if let Some(ko) = store.get(&id) {
            ltm.remember(ko);
            promoted += 1;
        }
    }
    promoted
}
