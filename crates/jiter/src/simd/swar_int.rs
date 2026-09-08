//! Uses in-register instructions to decode digit values into a `u64`.
//!
//! See <https://lemire.me/blog/2022/01/21/swar-explained-parsing-eight-digits/>

/// Convert 1–16 validated digits, packed in little-endian byte order with values 0–9.
/// Bytes after `count` are ignored.
#[inline]
pub(super) fn decode_digits(digits: [u64; 2], count: u32) -> u64 {
    const POW10: [u64; 9] = [1, 10, 100, 1000, 10_000, 100_000, 1_000_000, 10_000_000, 100_000_000];
    debug_assert!((1..=16).contains(&count));
    if count <= 8 {
        decode_eight_digits(digits[0] << ((8 - count) * 8))
    } else {
        decode_eight_digits(digits[0]) * POW10[(count - 8) as usize]
            + decode_eight_digits(digits[1] << ((16 - count) * 8))
    }
}

#[inline]
fn decode_eight_digits(value: u64) -> u64 {
    const MASK: u64 = 0x0000_00FF_0000_00FF;
    let value = value.wrapping_mul(2561) >> 8;
    (value & MASK)
        .wrapping_mul(0x000F_4240_0000_0064)
        .wrapping_add(((value >> 16) & MASK).wrapping_mul(0x0000_2710_0000_0001))
        >> 32
}
