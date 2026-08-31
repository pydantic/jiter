use crate::number_decoder::{INT_CHAR_MAP, IntChunk};

#[inline(always)]
pub(crate) fn find_digit_run_end(data: &[u8], mut index: usize) -> usize {
    while let Some(digit) = data.get(index) {
        if !INT_CHAR_MAP[*digit as usize] {
            break;
        }
        index += 1;
    }
    index
}

/// Turns out this is faster than fancy bit manipulation, see
/// https://github.com/Alexhuszagh/rust-lexical/blob/main/lexical-parse-integer/docs/Algorithm.md
/// for some context
#[inline(always)]
pub(crate) fn decode_int_chunk(data: &[u8], mut index: usize, mut value: u64) -> (IntChunk, usize) {
    // i64::MAX = 9223372036854775807 (19 chars) - so 18 chars is always valid as an i64
    for _ in 0..18 {
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
