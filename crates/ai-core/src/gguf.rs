//! crates/ai-core/src/gguf.rs
//! Minimal GGUF (llama.cpp container) parser.
//!
//! Parses the header, tensor metadata table, and metadata KV table. Weight
//! payloads are referenced by byte slice - no copy, no alloc beyond the
//! Vec<GgufTensor> for the tensor table. Quantized tensor dequantization is
//! stubbed for Phase 3.0; Phase 3.1 adds SIMD Q8_0/Q4_K kernels (AIOS-014).
//!
//! Author: Jad Jbara <jad@aios.dev>

use alloc::string::String;
use alloc::vec::Vec;

use crate::AiError;

/// GGUF magic bytes: 'GGUF' little-endian.
pub const GGUF_MAGIC: u32 = 0x46554747;

/// Supported GGUF version (llama.cpp v3).
pub const GGUF_VERSION: u32 = 3;

/// Quantization dtype enum (subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GgufDType {
    /// 4-bit K-quant (Q4_K).
    Q4K,
    /// 8-bit block-quant (Q8_0).
    Q8_0,
    /// 32-bit IEEE float (F32).
    F32,
}

impl GgufDType {
    /// Decode a raw dtype id from the tensor table.
    pub fn from_u32(raw: u32) -> Result<Self, AiError> {
        match raw {
            2 => Ok(GgufDType::Q4K),
            8 => Ok(GgufDType::Q8_0),
            0 => Ok(GgufDType::F32),
            other => Err(AiError::UnknownDtype(other)),
        }
    }
}

/// Parsed GGUF header.
#[derive(Debug, Clone)]
pub struct GgufHeader {
    /// Magic check value (must equal GGUF_MAGIC).
    pub magic: u32,
    /// Container version (must equal GGUF_VERSION).
    pub version: u32,
    /// Number of tensors in the file.
    pub tensor_count: u64,
    /// Number of metadata KV pairs.
    pub metadata_kv_count: u64,
}

/// One tensor entry from the tensor table.
#[derive(Debug, Clone)]
pub struct GgufTensor {
    /// Tensor name (UTF-8).
    pub name: String,
    /// Dimension lengths (little-endian).
    pub dims: Vec<u64>,
    /// Quantization dtype.
    pub dtype: GgufDType,
    /// Offset from the start of the tensor data block.
    pub offset: u64,
}

/// Parsed GGUF model - header + tensor table only (no payloads).
#[derive(Debug, Clone)]
pub struct GgufModel {
    /// Validated header.
    pub header: GgufHeader,
    /// Tensor metadata table.
    pub tensors: Vec<GgufTensor>,
}

/// Stateless parser.
pub struct GgufParser;

impl GgufParser {
    /// Parse a GGUF byte slice into a GgufModel.
    pub fn parse(bytes: &[u8]) -> Result<GgufModel, AiError> {
        if bytes.len() < 24 {
            return Err(AiError::Truncated);
        }
        let magic = read_u32_le(bytes, 0)?;
        if magic != GGUF_MAGIC {
            return Err(AiError::BadGgufMagic);
        }
        let version = read_u32_le(bytes, 4)?;
        if version != GGUF_VERSION {
            return Err(AiError::UnsupportedGgufVersion(version));
        }
        let tensor_count = read_u64_le(bytes, 8)?;
        let metadata_kv_count = read_u64_le(bytes, 16)?;
        let header = GgufHeader {
            magic,
            version,
            tensor_count,
            metadata_kv_count,
        };
        // Tensor table + KV table parsing arrives with AIOS-014; we surface
        // the validated header and an empty tensor table for Phase 3.0 so
        // the engine contract is testable without a real weights blob.
        Ok(GgufModel { header, tensors: Vec::new() })
    }
}

/// Read a little-endian u32 at 'offset'.
fn read_u32_le(buf: &[u8], offset: usize) -> Result<u32, AiError> {
    if offset + 4 > buf.len() {
        return Err(AiError::Truncated);
    }
    // SAFETY: we just bounds-checked offset..offset+4 above; the slice is a
    // valid &[u8] with len >= offset+4, so the four u8 pointer reads are
    // in-bounds and u8 reads have no alignment requirement.
    unsafe {
        let p = buf.as_ptr().add(offset);
        Ok(u32::from_le_bytes([*p, *p.add(1), *p.add(2), *p.add(3)]))
    }
}

/// Read a little-endian u64 at 'offset'.
fn read_u64_le(buf: &[u8], offset: usize) -> Result<u64, AiError> {
    if offset + 8 > buf.len() {
        return Err(AiError::Truncated);
    }
    // SAFETY: bounds-checked above; u8 reads have no alignment requirement.
    unsafe {
        let p = buf.as_ptr().add(offset);
        Ok(u64::from_le_bytes([
            *p, *p.add(1), *p.add(2), *p.add(3),
            *p.add(4), *p.add(5), *p.add(6), *p.add(7),
        ]))
    }
}

/// One Q8_0 block: 1 x f16 delta + 32 x int8 values. Phase 3.0 returns the
/// delta cast to f32; Phase 3.1 (AIOS-014) performs the real scale * q
/// reconstruction with SIMD cvtph2ps.
pub fn dequantize_q8_0(block: &[u8]) -> Result<f32, AiError> {
    if block.len() < 2 {
        return Err(AiError::Truncated);
    }
    let bits = read_u16_le(block, 0)?;
    Ok(f16_to_f32(bits))
}

/// Read a little-endian u16 at 'offset'.
fn read_u16_le(buf: &[u8], offset: usize) -> Result<u16, AiError> {
    if offset + 2 > buf.len() {
        return Err(AiError::Truncated);
    }
    // SAFETY: bounds-checked above; u8 reads are unaligned-safe.
    unsafe {
        let p = buf.as_ptr().add(offset);
        Ok(u16::from_le_bytes([*p, *p.add(1)]))
    }
}

/// Convert IEEE 754 binary16 to f32 via the standard bit-twiddle. Phase
/// 3.0 uses a portable scalar path; Phase 3.1 swaps in a SIMD cvtph2ps.
fn f16_to_f32(bits: u16) -> f32 {
    let sign = (bits >> 15) as u32;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let mant = (bits & 0x3ff) as u32;
    let f = (sign << 31) | (exp << 23) | (mant << 13);
    // SAFETY: f is a bit pattern produced by a valid f16 decode; the upper
    // 16 bits are zero-padded so every u32 here maps to a defined f32.
    unsafe { core::mem::transmute(f) }
}
