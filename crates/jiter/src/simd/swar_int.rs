//! Uses in-register instructions to decode ASCII digits into a `u64`.
//!
//! See <https://lemire.me/blog/2022/01/21/swar-explained-parsing-eight-digits/>

/// Convert validated ASCII digits to a magnitude that must fit in a `u64`.
///
/// Will produce a garbage value if the input is not valid ASCII digits (max 19 digits).
#[inline]
pub(crate) fn decode_digits(data: &[u8]) -> u64 {
    debug_assert!(data.iter().all(|&digit| (b'0'..=b'9').contains(&digit)));
    debug_assert!(data.len() <= 19, "maximum 19 digits allowed");

    let mut value = 0;
    let (chunks, remainder) = data.as_chunks::<8>();
    for chunk in chunks {
        value = value * 100_000_000 + decode_eight_digits(u64::from_le_bytes(*chunk));
    }
    remainder
        .iter()
        .fold(value, |value, digit| value * 10 + u64::from(digit & 0x0f))
}

/// Convert eight validated ASCII digits packed in little-endian order.
#[inline]
fn decode_eight_digits(word: u64) -> u64 {
    const MASK: u64 = 0x0000_00FF_0000_00FF;
    let value = word.wrapping_sub(0x3030_3030_3030_3030);
    let value = value.wrapping_mul(2561) >> 8;
    (value & MASK)
        .wrapping_mul(0x000F_4240_0000_0064)
        .wrapping_add(((value >> 16) & MASK).wrapping_mul(0x0000_2710_0000_0001))
        >> 32
}
