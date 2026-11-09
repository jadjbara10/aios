//! Vector index for Knowledge Objects.
//!
//! Brute-force cosine similarity search over a flat slab of
//! (KoId, embedding) pairs. This is intentionally simple and correct
//! for Phase 4; HNSW (hierarchical navigable small-world graphs) lands
//! in Phase 4.1 once the heap allocator and object log are stable.
//!
//! Author: Jad Jbara <jad@aios.dev>

use crate::KoId;
use alloc::vec::Vec;

/// Flat vector index. One entry per Knowledge Object embedding.
///
/// Invariants:
///   - every stored vector has identical length (the embedding dim D)
///   - every stored vector is unit-norm (see embedding::normalize)
///
/// Unit-norm lets cosine reduce to a dot product, so search is O(N*D)
/// with one multiply-add per element.
pub struct VectorIndex {
    dim: usize,
    vectors: Vec<(KoId, Vec<f32>)>,
}

impl VectorIndex {
    /// Construct an empty index expecting vectors of length dim.
    pub fn new(dim: usize) -> Self {
        VectorIndex { dim, vectors: Vec::new() }
    }

    /// Expected embedding dimension.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Number of indexed vectors.
    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    /// True when the index holds no vectors.
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// Insert (or replace by KoId) a unit-norm embedding.
    ///
    /// Returns the previous embedding for that KoId, if any. Vectors of
    /// the wrong dimension are rejected without insertion.
    pub fn insert(&mut self, id: KoId, v: Vec<f32>) -> Option<Vec<f32>> {
        if v.len() != self.dim {
            return None;
        }
        if let Some(slot) = self.vectors.iter_mut().find(|(k, _)| *k == id) {
            let prev = core::mem::replace(&mut slot.1, v);
            return Some(prev);
        }
        self.vectors.push((id, v));
        None
    }

    /// Remove a KoId from the index. Returns the removed embedding.
    pub fn remove(&mut self, id: &KoId) -> Option<Vec<f32>> {
        let pos = self.vectors.iter().position(|(k, _)| k == id)?;
        Some(self.vectors.swap_remove(pos).1)
    }

    /// Cosine similarity between two equal-length vectors.
    ///
    /// Both inputs are assumed unit-norm; if either is zero the result
    /// is 0.0 (no similarity) rather than NaN.
    pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let mut dot = 0.0f32;
        let mut na = 0.0f32;
        let mut nb = 0.0f32;
        for i in 0..a.len() {
            dot += a[i] * b[i];
            na += a[i] * a[i];
            nb += b[i] * b[i];
        }
        let denom = (na * nb).sqrt();
        if denom == 0.0 {
            0.0
        } else {
            dot / denom
        }
    }

    /// Top-k nearest neighbors by cosine similarity.
    ///
    /// Brute-force scan + partial sort. Returns (KoId, score) pairs in
    /// descending similarity order, truncated to k. The query vector
    /// need not be unit-norm; cosine handles it.
    ///
    /// NOTE: HNSW (Phase 4.1) will replace this scan with sub-linear
    /// search. The trait surface (search signature) will not change.
    pub fn search(&self, query: &[f32], k: usize) -> Vec<(KoId, f32)> {
        if k == 0 || self.vectors.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(KoId, f32)> = self
            .vectors
            .iter()
            .map(|(id, v)| (*id, Self::cosine(query, v)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }
}

/// Free function alias for VectorIndex::cosine (used by KnowledgeStore).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    VectorIndex::cosine(a, b)
}
