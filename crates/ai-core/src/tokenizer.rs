//! crates/ai-core/src/tokenizer.rs
//! Byte-Pair-Encoding tokenizer for the local AI engine.
//!
//! Phase 3.0 ships a deterministic encode/decode loop over a vocab vector
//! and a merge rank table. The vocab is loaded from the GGUF metadata KV in
//! Phase 3.1 (AIOS-015); for now a stub vocab is constructed in-memory by
//! the engine so the round-trip contract is testable.
//!
//! Tiny hardcoded vocab example (comment only - real vocab is GGUF-driven):
//!   0 -> "<unk>"
//!   1 -> "<s>"
//!   2 -> "</s>"
//!   3 -> "hello"
//!   4 -> " world"
//!   5 -> "aios"
//!
//! Author: Jad Jbara <jad@aios.dev>

use alloc::string::String;
use alloc::vec::Vec;

use crate::AiError;

/// One BPE merge rule: left + right -> merged_id, ranked by 'rank'.
#[derive(Debug, Clone)]
pub struct BpeMerge {
    /// Left token id.
    pub left: u32,
    /// Right token id.
    pub right: u32,
    /// Resulting merged token id.
    pub merged: u32,
    /// Merge priority (lower = applied first).
    pub rank: u32,
}

/// BPE tokenizer.
pub struct Tokenizer {
    /// Vocabulary indexed by token id.
    pub vocab: Vec<String>,
    /// Merge rules ordered by rank.
    pub merges: Vec<BpeMerge>,
}

impl Tokenizer {
    /// Build a stub tokenizer with 'vocab_size' placeholder ids. The real
    /// vocab + merges are loaded from GGUF metadata in Phase 3.1.
    pub fn stub(vocab_size: usize) -> Self {
        let mut vocab: Vec<String> = Vec::with_capacity(vocab_size);
        for i in 0..vocab_size {
            let b = ((i as u8).max(b' ')).min(0x7e);
            // SAFETY: b is in [0x20, 0x7e] - a printable ASCII byte. ASCII
            // bytes below 0x80 are always valid UTF-8, so the 1-byte slice
            // is a valid UTF-8 scalar and from_utf8_unchecked is sound.
            let s = unsafe { String::from_utf8_unchecked(alloc::vec![b]) };
            vocab.push(s);
        }
        Self { vocab, merges: Vec::new() }
    }

    /// Encode a text prompt into a sequence of token ids.
    ///
    /// Phase 3.0 path: each byte of the input maps to a token id modulo
    /// the vocab size. Phase 3.1 (AIOS-015) replaces this with the real
    /// BPE merge loop driven by 'self.merges'.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let n = self.vocab.len().max(1) as u32;
        text.bytes().map(|b| (b as u32) % n).collect()
    }

    /// Decode a sequence of token ids back into a String. Out-of-range ids
    /// map to "<unk>" so the decoder never panics.
    pub fn decode(&self, ids: &[u32]) -> String {
        let mut out = String::new();
        for id in ids {
            match self.vocab.get(*id as usize) {
                Some(s) => out.push_str(s),
                None => out.push_str("<unk>"),
            }
        }
        out
    }

    /// Validate that every id in 'ids' is in range. Returns the first
    /// offending id as an AiError, or Ok(()) if all are in range.
    pub fn validate(&self, ids: &[u32]) -> Result<(), AiError> {
        for id in ids {
            if *id as usize >= self.vocab.len() {
                return Err(AiError::TokenOutOfRange(*id));
            }
        }
        Ok(())
    }

    /// Current vocab size.
    pub fn vocab_len(&self) -> usize {
        self.vocab.len()
    }
}

/// Apply one BPE merge pass to a working sequence of token ids. Phase 3.0
/// returns the sequence unchanged; Phase 3.1 implements the real greedy
/// merge by lowest rank.
pub fn apply_merge_pass(seq: &mut Vec<u32>, _merges: &[BpeMerge]) {
    // Placeholder: real loop scans adjacent (left, right) pairs, looks up
    // the lowest-rank merge rule, and replaces matches left-to-right.
    let _ = seq;
}
