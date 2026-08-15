#[cfg(target_arch = "aarch64")]
mod aarch64;
mod fallback_int;
mod fallback_string;

#[cfg(target_arch = "aarch64")]
pub(crate) use aarch64::decode_string_chunk;
pub(crate) use fallback_int::decode_int_chunk as decode_int_chunk_small;
#[cfg(not(target_arch = "aarch64"))]
pub(crate) use fallback_string::decode_string_chunk;

use crate::number_decoder::IntChunk;

#[inline(always)]
pub(crate) fn decode_int_chunk_big(data: &[u8], index: usize) -> (IntChunk, usize) {
    #[cfg(target_arch = "aarch64")]
    {
        aarch64::decode_int_chunk_big(data, index)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        fallback_int::decode_int_chunk(data, index, 0)
    }
}
