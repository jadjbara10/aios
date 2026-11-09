//! crates/ai-core/src/lib.rs
//! AIOS AI Engine - local LLM runner (no cloud, no network).
//!
//! Orchestrates a parsed GGUF model, a BPE tokenizer, and a fixed-window
//! context manager. Inference runs entirely on-device: the forward pass is
//! a deterministic stub in Phase 3.0; real matmul/SIMD kernels land in
//! Phase 3.1 (AIOS-014).
//!
//! Author: Jad Jbara <jad@aios.dev>

#![no_std]
#![forbid(missing_docs)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

pub mod context;
pub mod gguf;
pub mod tokenizer;

use crate::context::ContextManager;
use crate::gguf::{GgufModel, GgufParser};
use crate::tokenizer::Tokenizer;

/// Errors surfaced by the AI engine. Cheap to propagate across the
/// kernel/userland boundary without string allocation in the hot path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiError {
    /// GGUF magic mismatch or truncated header.
    BadGgufMagic,
    /// Unsupported GGUF version (only v3 is accepted today).
    UnsupportedGgufVersion(u32),
    /// Tensor metadata references an unknown dtype.
    UnknownDtype(u32),
    /// Token id out of vocab range.
    TokenOutOfRange(u32),
    /// Context window exhausted even after truncation.
    ContextExhausted,
    /// Buffer too small to hold the requested slice.
    Truncated,
}

/// Top-level engine handle. Never sends data off-chip.
pub struct AIEngine {
    /// Parsed GGUF weights and metadata.
    pub model: GgufModel,
    /// BPE tokenizer (vocab + merges).
    pub tokenizer: Tokenizer,
    /// Sliding-window context and KV cache ring.
    pub ctx: ContextManager,
}

impl AIEngine {
    /// Load a model from an in-memory GGUF blob. The caller is expected to
    /// have already mapped the file read-only from the immutable core FS.
    pub fn load_model(bytes: &[u8]) -> Result<Self, AiError> {
        let model = GgufParser::parse(bytes)?;
        let vocab_size = model.header.tensor_count as usize;
        let tokenizer = Tokenizer::stub(vocab_size);
        let ctx = ContextManager::new(2048);
        Ok(Self { model, tokenizer, ctx })
    }

    /// Run one inference round: encode -> forward (stub) -> decode.
    ///
    /// The forward pass is a placeholder that reflects the prompt back
    /// through the context manager. Real Q8_0/Q4_K matmul plus fused
    /// attention is delivered in Phase 3.1 under AIOS-014.
    pub fn infer(&mut self, prompt: &str) -> String {
        let ids = self.tokenizer.encode(prompt);
        for id in &ids {
            self.ctx.append_token(*id);
        }
        // Forward pass stub. Phase 3.1 replaces this with the real loop:
        //   for layer in 0..n_layers { matmul_q8_0(...); attention(...); }
        let echoed: Vec<u32> = self.ctx.tokens.clone();
        self.tokenizer.decode(&echoed)
    }

    /// Reset the live context window (keeps the loaded model).
    pub fn reset(&mut self) {
        self.ctx.clear();
    }
}

/// Human-readable label for an AiError. Lives here (not in logger.rs) so
/// the AI crate stays self-describing.
pub fn describe_error(err: AiError) -> &'static str {
    match err {
        AiError::BadGgufMagic => "gguf: bad magic (expected GGUF)",
        AiError::UnsupportedGgufVersion(_) => "gguf: unsupported version",
        AiError::UnknownDtype(_) => "gguf: unknown dtype",
        AiError::TokenOutOfRange(_) => "tokenizer: token id out of range",
        AiError::ContextExhausted => "context: window exhausted",
        AiError::Truncated => "buffer: truncated",
    }
}
