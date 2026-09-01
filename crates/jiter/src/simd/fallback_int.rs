use crate::number_decoder::{INT_CHAR_MAP, IntChunk};

#[inline(always)]
pub(crate) fn find_digit_run_end(data: &[u8], mut index: usize, limit: usize) -> Option<usize> {
    while index < limit {
        let Some(digit) = data.get(index) else {
            return Some(index);
        };
        if !INT_CHAR_MAP[*digit as usize] {
            return Some(index);
        }
        index += 1;
    }
    match data.get(index) {
        Some(digit) if INT_CHAR_MAP[*digit as usize] => None,
        _ => Some(index),
    }
}

/// Fuse termination and value decoding for numbers shorter than four digits. Returns `None` when
/// at least four digits are present so the caller can scan the run from its SIMD-friendly start.
#[inline(always)]
pub(crate) fn decode_number_prefix(data: &[u8], index: usize) -> Option<(IntChunk, usize)> {
    if data.get(index + 3).is_some_and(u8::is_ascii_digit) {
        None
    } else {
        Some(decode_int_chunk_limit::<4>(data, index, 0))
    }
}

/// Turns out this is faster than fancy bit manipulation, see
/// https://github.com/Alexhuszagh/rust-lexical/blob/main/lexical-parse-integer/docs/Algorithm.md
/// for some context
#[inline(always)]
pub(crate) fn decode_int_chunk(data: &[u8], index: usize, value: u64) -> (IntChunk, usize) {
    // i64::MAX = 9223372036854775807 (19 chars) - so 18 chars is always valid as an i64
    decode_int_chunk_limit::<18>(data, index, value)
}

#[inline(always)]
fn decode_int_chunk_limit<const LIMIT: usize>(data: &[u8], mut index: usize, mut value: u64) -> (IntChunk, usize) {
    for _ in 0..LIMIT {
        if let Some(digit) = data.get(index) {
            if INT_CHAR_MAP[*digit as usize] {
                // we use wrapping add to avoid branching - we know the value cannot wrap
                value = value.wrapping_mul(10).wrapping_add((digit & 0x0f) as u64);
                index += 1;
                continue;
            } else if matches!(digit, b'.' | b'e' | b'E') {
                return (IntChunk::Float, index);
            }
        }
        return (IntChunk::Done(value), index);
    }
    (IntChunk::Ongoing(value), index)
}
