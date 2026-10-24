#[cfg(target_arch = "aarch64")]
mod aarch64;
mod fallback_int;
mod fallback_string;
mod number;
#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little")))]
mod structural;
mod swar_int;
#[cfg(target_arch = "x86_64")]
mod x86_64;

pub(crate) use fallback_int::decode_int_chunk as decode_int_chunk_small;
pub(crate) use number::{NumberChunk, decode_number_chunk};
#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little")))]
pub(crate) use structural::{MIN_INPUT as SKIP_MIN_INPUT, skip_container};

use crate::errors::JsonResult;
use crate::number_decoder::IntChunk;
use crate::string_decoder::StringChunk;

/// the number of digits consumed per `IntChunk::Ongoing` chunk from `decode_int_chunk_big`
#[cfg(all(
    feature = "num-bigint",
    any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little"))
))]
pub(crate) const ONGOING_CHUNK_MULTIPLIER: u64 = 10u64.pow(16);
#[cfg(all(
    feature = "num-bigint",
    not(any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little")))
))]
pub(crate) const ONGOING_CHUNK_MULTIPLIER: u64 = 10u64.pow(18);

#[inline(always)]
pub(crate) fn decode_string_chunk(
    data: &[u8],
    index: usize,
    ascii_only: bool,
    allow_partial: bool,
) -> JsonResult<(StringChunk, bool, usize)> {
    #[cfg(target_arch = "aarch64")]
    {
        aarch64::decode_string_chunk(data, index, ascii_only, allow_partial)
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { x86_64::decode_string_chunk(data, index, ascii_only, allow_partial) }
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        fallback_string::decode_string_chunk(data, index, ascii_only, allow_partial)
    }
}

/// Classify sixteen bytes, returning digit values in little-endian byte order and the digit
/// count. Bytes after the first non-digit have unspecified values.
#[inline(always)]
fn classify_digit_chunk(data: &[u8; 16]) -> ([u64; 2], u32) {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        // SAFETY: all supported aarch64 targets support neon intrinsics.
        unsafe { aarch64::classify_digit_chunk(data) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { x86_64::classify_digit_chunk(data) }
    }
    #[cfg(not(any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little"))))]
    {
        fallback_int::classify_digit_chunk(data)
    }
}

#[inline(always)]
pub(crate) fn find_digit_run_end(data: &[u8], index: usize, limit: usize) -> Option<usize> {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        // SAFETY: all supported aarch64 targets support neon intrinsics.
        unsafe { aarch64::find_digit_run_end(data, index, limit) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { x86_64::find_digit_run_end(data, index, limit) }
    }
    #[cfg(not(any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little"))))]
    {
        fallback_int::find_digit_run_end(data, index, limit)
    }
}

// only the bigint paths still decode the value of a long integer run; without `num-bigint`
// this and everything only it reaches would otherwise be reported as dead code
#[cfg_attr(not(feature = "num-bigint"), allow(dead_code))]
#[inline(always)]
pub(crate) fn decode_int_chunk_big(data: &[u8], index: usize) -> (IntChunk, usize) {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        // SAFETY: all supported aarch64 targets support neon intrinsics
        unsafe { aarch64::decode_int_chunk_big(data, index) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { x86_64::decode_int_chunk_big(data, index) }
    }
    #[cfg(not(any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little"))))]
    {
        fallback_int::decode_int_chunk(data, index, 0)
    }
}

/// Classify a 64-byte block into the bit-per-byte masks the structural skip works on. With
/// `outside_string`, a block without a quote skips the masks only strings need.
#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little")))]
#[inline(always)]
fn classify_block(block: &[u8; 64], outside_string: bool) -> structural::BlockMasks {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        // SAFETY: all supported aarch64 targets support neon intrinsics.
        unsafe { aarch64::classify_block(block, outside_string) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { x86_64::classify_block(block, outside_string) }
    }
}

/// Does the block hold a quote, a backslash or a control character? For a block that lies
/// entirely inside a string this is all the structural skip needs to know.
#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little")))]
#[inline(always)]
fn block_has_string_special(block: &[u8; 64]) -> bool {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        // SAFETY: all supported aarch64 targets support neon intrinsics.
        unsafe { aarch64::block_has_string_special(block) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { x86_64::block_has_string_special(block) }
    }
}

/// The digit and dot masks of a block, for checking a number token.
#[cfg(any(target_arch = "x86_64", all(target_arch = "aarch64", target_endian = "little")))]
#[inline(always)]
fn digit_masks(block: &[u8; 64]) -> structural::DigitMasks {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        // SAFETY: all supported aarch64 targets support neon intrinsics.
        unsafe { aarch64::digit_masks(block) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline.
        unsafe { x86_64::digit_masks(block) }
    }
}
