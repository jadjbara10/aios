//! crates/ai-core/src/context.rs
//! Fixed-window context manager with a KV-cache ring.
//!
//! The ContextManager owns the live token id sequence plus a per-layer KV
//! cache ring. Phase 3.0 implements the sliding window (append + truncate);
//! Phase 3.1 (AIOS-015) wires the KV cache into the real attention kernel.
//!
//! Author: Jad Jbara <jad@aios.dev>

use alloc::vec::Vec;

use crate::AiError;

/// One KV-cache entry (stub). Phase 3.1 replaces the f32 stubs with
/// quantized K/V tensors per layer.
#[derive(Debug, Clone, Copy)]
pub struct KvEntry {
    /// Layer index.
    pub layer: u32,
    /// Key head sum (debug-only placeholder).
    pub k: f32,
    /// Value head sum (debug-only placeholder).
    pub v: f32,
}

impl KvEntry {
    /// Construct a zeroed KV entry at the given layer.
    pub fn zero(layer: u32) -> Self {
        Self { layer, k: 0.0, v: 0.0 }
    }
}

/// Sliding-window context manager.
pub struct ContextManager {
    /// Maximum number of tokens the engine will hold live.
    pub n_ctx: usize,
    /// Live token id sequence (most-recent-last).
    pub tokens: Vec<u32>,
    /// KV cache ring (one slot per live token, per layer in Phase 3.1).
    pub kv_cache: Vec<KvEntry>,
}

impl ContextManager {
    /// Construct a new context manager with capacity 'n_ctx'.
    pub fn new(n_ctx: usize) -> Self {
        Self {
            n_ctx,
            tokens: Vec::with_capacity(n_ctx),
            kv_cache: Vec::with_capacity(n_ctx),
        }
    }

    /// Current live token count.
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// True if no tokens are live.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Append a token id. When the window is full, the oldest token is
    /// evicted and the KV cache ring advances by one slot.
    pub fn append_token(&mut self, id: u32) {
        if self.tokens.len() >= self.n_ctx {
            // Sliding window: drop the oldest token + its KV slot.
            self.tokens.remove(0);
            if !self.kv_cache.is_empty() {
                self.kv_cache.remove(0);
            }
        }
        self.tokens.push(id);
        self.kv_cache.push(KvEntry::zero(0));
    }

    /// Truncate the live window to 'len' tokens, keeping the most recent
    /// 'len' ids. If 'len' exceeds the current length, this is a no-op.
    pub fn truncate(&mut self, len: usize) {
        if len >= self.tokens.len() {
            return;
        }
        let drop = self.tokens.len() - len;
        let new_len = self.tokens.len() - drop;
        // SAFETY: drop is bounded by tokens.len() (checked above), so
        // new_len = tokens.len() - drop is in [0, tokens.len()). The source
        // range [drop .. drop+new_len] = [drop .. tokens.len()] is in-bounds
        // for the allocation (capacity >= tokens.len()). ptr::copy is
        // memmove-equivalent so the overlapping shift is well-defined.
        // After the copy, all elements in [0 .. new_len] are initialized;
        // u32 is Copy (no Drop), so set_len(new_len) does not leak.
        unsafe {
            let p = self.tokens.as_mut_ptr();
            core::ptr::copy(p.add(drop), p, new_len);
            self.tokens.set_len(new_len);
        }
        // Mirror the truncation on the KV cache ring.
        if self.kv_cache.len() >= drop {
            self.kv_cache.truncate(self.kv_cache.len() - drop);
        }
    }

    /// Reset the context and KV cache to empty (keeps capacity).
    pub fn clear(&mut self) {
        self.tokens.clear();
        self.kv_cache.clear();
    }

    /// Reserve 'additional' slots in both the token vec and KV ring.
    pub fn reserve(&mut self, additional: usize) -> Result<(), AiError> {
        self.tokens
            .try_reserve(additional)
            .map_err(|_| AiError::ContextExhausted)?;
        self.kv_cache
            .try_reserve(additional)
            .map_err(|_| AiError::ContextExhausted)?;
        Ok(())
    }
}
