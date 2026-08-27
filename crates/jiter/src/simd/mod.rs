#[cfg(target_arch = "aarch64")]
mod aarch64;
mod fallback_int;
mod fallback_string;
#[cfg(target_arch = "x86_64")]
mod x86_64;

#[cfg(target_arch = "aarch64")]
pub(crate) use aarch64::decode_string_chunk;
pub(crate) use fallback_int::decode_int_chunk as decode_int_chunk_small;
#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
pub(crate) use fallback_string::decode_string_chunk;
#[cfg(target_arch = "x86_64")]
pub(crate) use x86_64::decode_string_chunk;

use crate::number_decoder::IntChunk;

/// the number of digits consumed per `IntChunk::Ongoing` chunk from `decode_int_chunk_big`
#[cfg(all(feature = "num-bigint", any(target_arch = "aarch64", target_arch = "x86_64")))]
pub(crate) const ONGOING_CHUNK_MULTIPLIER: u64 = 10u64.pow(16);
#[cfg(all(feature = "num-bigint", not(any(target_arch = "aarch64", target_arch = "x86_64"))))]
pub(crate) const ONGOING_CHUNK_MULTIPLIER: u64 = 10u64.pow(18);

#[inline(always)]
pub(crate) fn decode_int_chunk_big(data: &[u8], index: usize) -> (IntChunk, usize) {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: all supported aarch64 targets support neon intrinsics
        unsafe { aarch64::decode_int_chunk_big(data, index) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        x86_64::decode_int_chunk_big(data, index)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        fallback_int::decode_int_chunk(data, index, 0)
    }
}
