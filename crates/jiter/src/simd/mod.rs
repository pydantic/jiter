#[cfg(target_arch = "aarch64")]
mod aarch64;
mod fallback_int;
mod fallback_string;

pub(crate) use fallback_int::decode_int_chunk as decode_int_chunk_small;

use crate::errors::{JsonResult, json_err};
use crate::number_decoder::IntChunk;
use crate::string_decoder::StringChunk;
use fallback_string::{CHAR_TYPE, CharType, JSON_ASCII};

/// bytes scanned byte-by-byte before entering the SIMD loop
const SCALAR_PREFIX: usize = 8;

#[inline(always)]
pub(crate) fn decode_string_chunk(
    data: &[u8],
    mut index: usize,
    mut ascii_only: bool,
    allow_partial: bool,
) -> JsonResult<(StringChunk, bool, usize)> {
    let prefix_end = data.len().min(index + SCALAR_PREFIX);
    while index < prefix_end {
        let next = data[index];
        if !JSON_ASCII[next as usize] {
            match &CHAR_TYPE[next as usize] {
                CharType::Quote => return Ok((StringChunk::StringEnd, ascii_only, index)),
                CharType::Backslash => return Ok((StringChunk::Backslash, ascii_only, index)),
                CharType::ControlChar => return json_err!(ControlCharacterWhileParsingString, index),
                CharType::Other => ascii_only = false,
            }
        }
        index += 1;
    }
    #[cfg(target_arch = "aarch64")]
    {
        aarch64::decode_string_chunk(data, index, ascii_only, allow_partial)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        fallback_string::decode_string_chunk(data, index, ascii_only, allow_partial)
    }
}

#[inline(always)]
pub(crate) fn decode_int_chunk_big(data: &[u8], index: usize) -> (IntChunk, usize) {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: all supported aarch64 targets support neon intrinsics
        unsafe { aarch64::decode_int_chunk_big(data, index) }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        fallback_int::decode_int_chunk(data, index, 0)
    }
}
