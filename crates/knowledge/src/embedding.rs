//! Embedding trait + utilities.
//!
//! The Embedder trait is the single abstraction the knowledge crate
//! uses to turn text into vectors. The canonical implementor is the
//! AI Engine (crates/ai-core): it owns the model, the tokenizer, and
//! the embedding dimension. Anything else (including the LocalEmbedder
//! stub below) is a deterministic fallback for host tests and early
//! boot before the AI Engine is online.
//!
//! Author: Jad Jbara <jad@aios.dev>

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Convert text into a fixed-dimension f32 vector.
///
/// Implementors MUST:
///   - return vectors of length exactly dim()
///   - be deterministic for a given input (caching is encouraged)
///   - be cheap to call from no_std (no host I/O)
///
/// They NEED NOT return unit-norm vectors; callers normalize via
/// normalize() before inserting into a VectorIndex.
pub trait Embedder {
    /// Embed a text fragment into a vector of length dim().
    fn embed(&self, text: &str) -> Vec<f32>;

    /// The fixed dimension of every vector this embedder produces.
    fn dim(&self) -> usize;
}

/// L2-normalize a vector in place. A zero vector is left as zero
/// (rather than NaN) so cosine similarity degrades to 0 gracefully.
pub fn normalize(v: &mut [f32]) {
    let mut sum_sq = 0.0f32;
    for x in v.iter() {
        sum_sq += (*x) * (*x);
    }
    let norm = sum_sq.sqrt();
    if norm > 0.0 {
        let inv = 1.0 / norm;
        for x in v.iter_mut() {
            *x *= inv;
        }
    }
}

/// Dot product of two equal-length vectors. Returns 0.0 on length
/// mismatch (defensive; callers should pre-check).
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut s = 0.0f32;
    for i in 0..a.len() {
        s += a[i] * b[i];
    }
    s
}

/// Deterministic stub embedder for host tests and early boot.
///
/// Hashes the input text with FNV-1a into a fixed-dim vector, then
/// spreads the hash across the dimensions. This is NOT a semantic
/// embedding - two near-identical strings will have unrelated vectors.
/// Real embeddings come from the AI Engine; this exists so the vector
/// index and memory tiers are testable without it.
///
/// When the AI Engine is online, swap LocalEmbedder for the engine's
/// embedder at the dependency-injection boundary. The trait surface is
/// identical, so no caller changes.
pub struct LocalEmbedder {
    dim: usize,
}

impl LocalEmbedder {
    /// Construct a stub embedder producing dim-length vectors.
    /// dim must be non-zero.
    pub fn new(dim: usize) -> Self {
        assert!(dim > 0, "LocalEmbedder dim must be non-zero");
        LocalEmbedder { dim }
    }
}

impl Embedder for LocalEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        // FNV-1a 32-bit over the bytes, folded into each dimension with
        // a per-dimension salt. Cheap, deterministic, non-semantic.
        let mut out = vec![0.0f32; self.dim];
        if text.is_empty() {
            return out;
        }
        let mut h: u32 = 0x811c9dc5;
        for b in text.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        // Spread: each dimension mixes the running hash with its index
        // so adjacent dims differ even for a single-block hash.
        for i in 0..self.dim {
            let salt = (i as u32).wrapping_mul(0x9e37_79b1);
            let mixed = h ^ salt;
            let f = (mixed as i32 as f32) / (i32::MAX as f32);
            out[i] = f;
        }
        // Normalize so the stub is drop-in compatible with AI Engine
        // output (which is also unit-norm).
        normalize(&mut out);
        out
    }

    fn dim(&self) -> usize {
        self.dim
    }
}

/// Helper: format a byte slice as lowercase hex into the given String.
/// Used by callers that need a textual key for an embedder (e.g.
/// embedding a KO by its content hash). Allocates.
pub fn hex_into(bytes: &[u8], out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
}
