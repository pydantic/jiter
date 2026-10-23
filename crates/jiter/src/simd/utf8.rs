//! UTF-8 validation for string content.
//!
//! The lookup algorithm of Keiser and Lemire ("Validating UTF-8 In Less Than One Instruction Per
//! Byte", 2021), as used by simdjson and simdutf8: each block's bytes are classified by three
//! nibble table lookups into the error classes a byte pair can form, the continuation bytes
//! required by 3 and 4 byte leads are checked with two shifted compares, and an all-ASCII block
//! costs one reduction. On aarch64 it runs on NEON; on x86_64 on SSSE3, whose byte shuffle it
//! needs, or on AVX2 for inputs of 64 bytes and more, as the CPU allows. Elsewhere validation is
//! std's.
//!
//! The kernels only say whether the bytes are valid. On rejection the caller asks
//! `core::str::from_utf8` for the position, so the error, `valid_up_to` and `error_len` are
//! exactly std's.

use std::str::{Utf8Error, from_utf8_unchecked};

/// `core::str::from_utf8`, with the valid case decided by the fast validator. Out of line: the
/// caller's ASCII path must stay small enough to inline into the decoders, and a string that
/// gets here has at least a block of content with non-ASCII in it to check, which dwarfs a call.
#[inline(never)]
pub(crate) fn from_utf8(bytes: &[u8]) -> Result<&str, Utf8Error> {
    // below one block, std's scalar loop costs about what the kernel's setup does
    if is_valid_utf8(bytes) {
        // SAFETY: `is_valid_utf8` accepts exactly the byte strings `core::str::from_utf8` accepts:
        // the algorithm and tables are simdjson's, and the tests below check every two and three
        // byte window across the block edges, and every lead with its boundary continuations,
        // against std.
        Ok(unsafe { from_utf8_unchecked(bytes) })
    } else {
        core::str::from_utf8(bytes)
    }
}

/// Whether `bytes` are valid UTF-8, by the kernel the CPU supports. Correct for any length; the
/// decoder keeps inputs shorter than a block away from it for speed, not correctness.
#[inline]
fn is_valid_utf8(bytes: &[u8]) -> bool {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        // SAFETY: all supported aarch64 targets support neon intrinsics.
        unsafe { neon::is_valid_utf8(bytes) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // the 256-bit kernel's setup and tail only pay off past a couple of its blocks
        if bytes.len() >= 64 && std::is_x86_feature_detected!("avx2") {
            // SAFETY: AVX2 was just detected.
            unsafe { avx2::is_valid_utf8(bytes) }
        } else if std::is_x86_feature_detected!("ssse3") {
            // SAFETY: SSSE3 was just detected.
            unsafe { ssse3::is_valid_utf8(bytes) }
        } else {
            core::str::from_utf8(bytes).is_ok()
        }
    }
    #[cfg(not(any(all(target_arch = "aarch64", target_endian = "little"), target_arch = "x86_64")))]
    {
        core::str::from_utf8(bytes).is_ok()
    }
}

#[cfg(any(all(target_arch = "aarch64", target_endian = "little"), target_arch = "x86_64"))]
mod tables {
    // The error classes a lead byte (`prev1`, the byte before the one examined) and the byte after
    // it can combine into, as bits; a set bit in all three lookups is an error. Two of the bits are
    // shared between classes that cannot meet.
    const TOO_SHORT: u8 = 1 << 0;
    const TOO_LONG: u8 = 1 << 1;
    const OVERLONG_3: u8 = 1 << 2;
    const TOO_LARGE: u8 = 1 << 3;
    const SURROGATE: u8 = 1 << 4;
    const OVERLONG_2: u8 = 1 << 5;
    const TOO_LARGE_1000: u8 = 1 << 6;
    const OVERLONG_4: u8 = 1 << 6;
    const TWO_CONTS: u8 = 1 << 7;
    const CARRY: u8 = TOO_SHORT | TOO_LONG | TWO_CONTS;

    /// Indexed by the high nibble of the previous byte.
    #[rustfmt::skip]
    pub(super) static BYTE_1_HIGH: [u8; 16] = [
        TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG, TOO_LONG,
        TWO_CONTS, TWO_CONTS, TWO_CONTS, TWO_CONTS,
        TOO_SHORT | OVERLONG_2,
        TOO_SHORT,
        TOO_SHORT | OVERLONG_3 | SURROGATE,
        TOO_SHORT | TOO_LARGE | TOO_LARGE_1000 | OVERLONG_4,
    ];

    /// Indexed by the low nibble of the previous byte.
    #[rustfmt::skip]
    pub(super) static BYTE_1_LOW: [u8; 16] = [
        CARRY | OVERLONG_3 | OVERLONG_2 | OVERLONG_4,
        CARRY | OVERLONG_2,
        CARRY, CARRY,
        CARRY | TOO_LARGE,
        CARRY | TOO_LARGE | TOO_LARGE_1000, CARRY | TOO_LARGE | TOO_LARGE_1000, CARRY | TOO_LARGE | TOO_LARGE_1000,
        CARRY | TOO_LARGE | TOO_LARGE_1000, CARRY | TOO_LARGE | TOO_LARGE_1000, CARRY | TOO_LARGE | TOO_LARGE_1000,
        CARRY | TOO_LARGE | TOO_LARGE_1000, CARRY | TOO_LARGE | TOO_LARGE_1000,
        CARRY | TOO_LARGE | TOO_LARGE_1000 | SURROGATE,
        CARRY | TOO_LARGE | TOO_LARGE_1000, CARRY | TOO_LARGE | TOO_LARGE_1000,
    ];

    /// Indexed by the high nibble of the current byte.
    #[rustfmt::skip]
    pub(super) static BYTE_2_HIGH: [u8; 16] = [
        TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT,
        TOO_LONG | OVERLONG_2 | TWO_CONTS | OVERLONG_3 | TOO_LARGE_1000 | OVERLONG_4,
        TOO_LONG | OVERLONG_2 | TWO_CONTS | OVERLONG_3 | TOO_LARGE,
        TOO_LONG | OVERLONG_2 | TWO_CONTS | SURROGATE | TOO_LARGE,
        TOO_LONG | OVERLONG_2 | TWO_CONTS | SURROGATE | TOO_LARGE,
        TOO_SHORT, TOO_SHORT, TOO_SHORT, TOO_SHORT,
    ];

    /// The largest value a byte may have at each of a block's last three positions for the block
    /// not to end inside a sequence: a 4-byte lead needs three more bytes, a 3-byte lead two, a
    /// 2-byte lead one. `saturating_sub` against this is non-zero exactly where a sequence is cut.
    #[rustfmt::skip]
    pub(super) static MAX_COMPLETE: [u8; 16] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xF0 - 1, 0xE0 - 1, 0xC0 - 1,
    ];
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
mod neon {
    use std::arch::aarch64::{
        uint8x16_t, vandq_u8, vcgtq_u8, vdupq_n_u8, veorq_u8, vextq_u8, vld1q_u8, vmaxvq_u8, vorrq_u8, vqsubq_u8,
        vqtbl1q_u8, vshrq_n_u8,
    };

    use super::tables::{BYTE_1_HIGH, BYTE_1_LOW, BYTE_2_HIGH, MAX_COMPLETE};

    /// Sixteen bytes into a register.
    #[inline(always)]
    fn load(bytes: &[u8; 16]) -> uint8x16_t {
        // SAFETY: the array is valid for reading 16 contiguous bytes.
        unsafe { vld1q_u8(bytes.as_ptr()) }
    }

    struct Tables {
        byte_1_high: uint8x16_t,
        byte_1_low: uint8x16_t,
        byte_2_high: uint8x16_t,
        max_complete: uint8x16_t,
        low_nibble: uint8x16_t,
        high_bit: uint8x16_t,
        zero: uint8x16_t,
    }

    /// The errors in one block, given the block before it.
    #[inline]
    #[target_feature(enable = "neon")]
    fn check_block(t: &Tables, prev: uint8x16_t, input: uint8x16_t) -> uint8x16_t {
        let prev1 = vextq_u8::<15>(prev, input);
        let special_cases = vandq_u8(
            vandq_u8(
                vqtbl1q_u8(t.byte_1_high, vshrq_n_u8::<4>(prev1)),
                vqtbl1q_u8(t.byte_1_low, vandq_u8(prev1, t.low_nibble)),
            ),
            vqtbl1q_u8(t.byte_2_high, vshrq_n_u8::<4>(input)),
        );
        // a byte two or three behind that leads a 3 or 4 byte sequence makes this one a required
        // continuation; the lookups above set TWO_CONTS (bit 7) where a continuation is present,
        // so XOR-ing the two leaves bit 7 set exactly where one is required but absent or present
        // but not required
        let prev2 = vextq_u8::<14>(prev, input);
        let prev3 = vextq_u8::<13>(prev, input);
        let must_continue = vcgtq_u8(
            vorrq_u8(
                vqsubq_u8(prev2, vdupq_n_u8(0xE0 - 1)),
                vqsubq_u8(prev3, vdupq_n_u8(0xF0 - 1)),
            ),
            t.zero,
        );
        veorq_u8(vandq_u8(must_continue, t.high_bit), special_cases)
    }

    #[target_feature(enable = "neon")]
    pub(super) fn is_valid_utf8(bytes: &[u8]) -> bool {
        let t = Tables {
            byte_1_high: load(&BYTE_1_HIGH),
            byte_1_low: load(&BYTE_1_LOW),
            byte_2_high: load(&BYTE_2_HIGH),
            max_complete: load(&MAX_COMPLETE),
            low_nibble: vdupq_n_u8(0x0F),
            high_bit: vdupq_n_u8(0x80),
            zero: vdupq_n_u8(0),
        };
        let mut prev = t.zero;
        // where the previous block ended inside a sequence; an error if that block was followed
        // by ASCII, which the next non-ASCII block would otherwise not notice
        let mut incomplete = t.zero;
        let mut error = t.zero;

        let (chunks, rest) = bytes.as_chunks::<16>();
        let (quads, singles) = chunks.as_chunks::<4>();
        // four blocks at a time while they are all ASCII, which one reduction settles
        for quad in quads {
            let v = [load(&quad[0]), load(&quad[1]), load(&quad[2]), load(&quad[3])];
            let any = vorrq_u8(vorrq_u8(v[0], v[1]), vorrq_u8(v[2], v[3]));
            if vmaxvq_u8(any) < 0x80 {
                error = vorrq_u8(error, incomplete);
                incomplete = t.zero;
            } else {
                for input in v {
                    error = vorrq_u8(error, check_block(&t, prev, input));
                    prev = input;
                }
                incomplete = vqsubq_u8(v[3], t.max_complete);
            }
            prev = v[3];
        }
        for chunk in singles {
            let input = load(chunk);
            if vmaxvq_u8(input) < 0x80 {
                error = vorrq_u8(error, incomplete);
                incomplete = t.zero;
            } else {
                error = vorrq_u8(error, check_block(&t, prev, input));
                incomplete = vqsubq_u8(input, t.max_complete);
            }
            prev = input;
        }
        if rest.is_empty() {
            error = vorrq_u8(error, incomplete);
        } else {
            // the tail padded with zeros: a sequence cut by the end of the input is then a lead
            // followed by ASCII, which the lookups reject
            let mut tail = [0u8; 16];
            tail[..rest.len()].copy_from_slice(rest);
            let input = load(&tail);
            if vmaxvq_u8(input) < 0x80 {
                error = vorrq_u8(error, incomplete);
            } else {
                error = vorrq_u8(error, check_block(&t, prev, input));
            }
        }
        vmaxvq_u8(error) == 0
    }
}

#[cfg(target_arch = "x86_64")]
mod ssse3 {
    use std::arch::x86_64::{
        __m128i, _mm_alignr_epi8, _mm_and_si128, _mm_cmpeq_epi8, _mm_cmpgt_epi8, _mm_loadu_si128, _mm_movemask_epi8,
        _mm_or_si128, _mm_set1_epi8, _mm_setzero_si128, _mm_shuffle_epi8, _mm_srli_epi16, _mm_subs_epu8, _mm_xor_si128,
    };

    use super::tables::{BYTE_1_HIGH, BYTE_1_LOW, BYTE_2_HIGH, MAX_COMPLETE};

    /// Sixteen bytes into a register.
    #[inline(always)]
    fn load(bytes: &[u8; 16]) -> __m128i {
        // SAFETY: the array is valid for reading 16 contiguous bytes; the load is unaligned.
        unsafe { _mm_loadu_si128(bytes.as_ptr().cast()) }
    }

    struct Tables {
        byte_1_high: __m128i,
        byte_1_low: __m128i,
        byte_2_high: __m128i,
        max_complete: __m128i,
        low_nibble: __m128i,
        high_bit: __m128i,
        zero: __m128i,
    }

    /// The errors in one block, given the block before it.
    #[inline]
    #[target_feature(enable = "ssse3")]
    fn check_block(t: &Tables, prev: __m128i, input: __m128i) -> __m128i {
        let prev1 = _mm_alignr_epi8::<15>(input, prev);
        // there is no byte shift: shift the 16-bit lanes and mask the halves back to nibbles
        let high_nibbles = |v: __m128i| _mm_and_si128(_mm_srli_epi16::<4>(v), t.low_nibble);
        let special_cases = _mm_and_si128(
            _mm_and_si128(
                _mm_shuffle_epi8(t.byte_1_high, high_nibbles(prev1)),
                _mm_shuffle_epi8(t.byte_1_low, _mm_and_si128(prev1, t.low_nibble)),
            ),
            _mm_shuffle_epi8(t.byte_2_high, high_nibbles(input)),
        );
        // a byte two or three behind that leads a 3 or 4 byte sequence makes this one a required
        // continuation; the lookups above set TWO_CONTS (bit 7) where a continuation is present,
        // so XOR-ing the two leaves bit 7 set exactly where one is required but absent or present
        // but not required
        let prev2 = _mm_alignr_epi8::<14>(input, prev);
        let prev3 = _mm_alignr_epi8::<13>(input, prev);
        // the saturated differences are at most 0x20 and 0x10, so the signed compare is exact
        let must_continue = _mm_cmpgt_epi8(
            _mm_or_si128(
                _mm_subs_epu8(prev2, _mm_set1_epi8((0xE0u8 - 1) as i8)),
                _mm_subs_epu8(prev3, _mm_set1_epi8((0xF0u8 - 1) as i8)),
            ),
            t.zero,
        );
        _mm_xor_si128(_mm_and_si128(must_continue, t.high_bit), special_cases)
    }

    #[inline]
    #[target_feature(enable = "ssse3")]
    fn is_ascii(v: __m128i) -> bool {
        _mm_movemask_epi8(v) == 0
    }

    #[target_feature(enable = "ssse3")]
    pub(super) fn is_valid_utf8(bytes: &[u8]) -> bool {
        let t = Tables {
            byte_1_high: load(&BYTE_1_HIGH),
            byte_1_low: load(&BYTE_1_LOW),
            byte_2_high: load(&BYTE_2_HIGH),
            max_complete: load(&MAX_COMPLETE),
            low_nibble: _mm_set1_epi8(0x0F),
            high_bit: _mm_set1_epi8(0x80u8 as i8),
            zero: _mm_setzero_si128(),
        };
        let mut prev = t.zero;
        // where the previous block ended inside a sequence; an error if that block was followed
        // by ASCII, which the next non-ASCII block would otherwise not notice
        let mut incomplete = t.zero;
        let mut error = t.zero;

        let (chunks, rest) = bytes.as_chunks::<16>();
        let (quads, singles) = chunks.as_chunks::<4>();
        // four blocks at a time while they are all ASCII, which one reduction settles
        for quad in quads {
            let v = [load(&quad[0]), load(&quad[1]), load(&quad[2]), load(&quad[3])];
            let any = _mm_or_si128(_mm_or_si128(v[0], v[1]), _mm_or_si128(v[2], v[3]));
            if is_ascii(any) {
                error = _mm_or_si128(error, incomplete);
                incomplete = t.zero;
            } else {
                for input in v {
                    error = _mm_or_si128(error, check_block(&t, prev, input));
                    prev = input;
                }
                incomplete = _mm_subs_epu8(v[3], t.max_complete);
            }
            prev = v[3];
        }
        for chunk in singles {
            let input = load(chunk);
            if is_ascii(input) {
                error = _mm_or_si128(error, incomplete);
                incomplete = t.zero;
            } else {
                error = _mm_or_si128(error, check_block(&t, prev, input));
                incomplete = _mm_subs_epu8(input, t.max_complete);
            }
            prev = input;
        }
        if rest.is_empty() {
            error = _mm_or_si128(error, incomplete);
        } else {
            // the tail padded with zeros: a sequence cut by the end of the input is then a lead
            // followed by ASCII, which the lookups reject
            let mut tail = [0u8; 16];
            tail[..rest.len()].copy_from_slice(rest);
            let input = load(&tail);
            if is_ascii(input) {
                error = _mm_or_si128(error, incomplete);
            } else {
                error = _mm_or_si128(error, check_block(&t, prev, input));
            }
        }
        _mm_movemask_epi8(_mm_cmpeq_epi8(error, t.zero)) == 0xFFFF
    }
}

#[cfg(target_arch = "x86_64")]
mod avx2 {
    use std::arch::x86_64::{
        __m256i, _mm_loadu_si128, _mm256_alignr_epi8, _mm256_and_si256, _mm256_broadcastsi128_si256, _mm256_cmpgt_epi8,
        _mm256_loadu_si256, _mm256_movemask_epi8, _mm256_or_si256, _mm256_permute2x128_si256, _mm256_set1_epi8,
        _mm256_setzero_si256, _mm256_shuffle_epi8, _mm256_srli_epi16, _mm256_subs_epu8, _mm256_testz_si256,
        _mm256_xor_si256,
    };

    use super::tables::{BYTE_1_HIGH, BYTE_1_LOW, BYTE_2_HIGH, MAX_COMPLETE};

    /// `MAX_COMPLETE` for a 32-byte block: only its last three positions can cut a sequence.
    static MAX_COMPLETE_32: [u8; 32] = {
        let mut table = [0xFFu8; 32];
        let mut i = 0;
        while i < 16 {
            table[16 + i] = MAX_COMPLETE[i];
            i += 1;
        }
        table
    };

    /// Thirty-two bytes into a register.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn load(bytes: &[u8; 32]) -> __m256i {
        // SAFETY: the array is valid for reading 32 contiguous bytes; the load is unaligned.
        unsafe { _mm256_loadu_si256(bytes.as_ptr().cast()) }
    }

    /// A 16-byte table in both lanes, as the in-lane shuffle wants it.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn table(bytes: &[u8; 16]) -> __m256i {
        // SAFETY: the array is valid for reading 16 contiguous bytes; the load is unaligned.
        _mm256_broadcastsi128_si256(unsafe { _mm_loadu_si128(bytes.as_ptr().cast()) })
    }

    struct Tables {
        byte_1_high: __m256i,
        byte_1_low: __m256i,
        byte_2_high: __m256i,
        max_complete: __m256i,
        low_nibble: __m256i,
        high_bit: __m256i,
        zero: __m256i,
    }

    /// The block shifted right by `N` bytes with the previous block's last bytes shifted in.
    /// `alignr` works within 16-byte lanes, so each lane is first paired with its predecessor:
    /// the previous block's high lane for the low lane, the low lane for the high lane.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn prev_bytes<const N: i32>(prev: __m256i, input: __m256i) -> __m256i {
        const { assert!(N >= 1 && N <= 3) }
        let lanes = _mm256_permute2x128_si256::<0x21>(prev, input);
        match N {
            1 => _mm256_alignr_epi8::<15>(input, lanes),
            2 => _mm256_alignr_epi8::<14>(input, lanes),
            _ => _mm256_alignr_epi8::<13>(input, lanes),
        }
    }

    /// The errors in one block, given the block before it.
    #[inline]
    #[target_feature(enable = "avx2")]
    fn check_block(t: &Tables, prev: __m256i, input: __m256i) -> __m256i {
        let prev1 = prev_bytes::<1>(prev, input);
        // three nibble lookups AND-ed: the error classes the byte pair (prev1, input) can form
        let high_nibbles = |v: __m256i| _mm256_and_si256(_mm256_srli_epi16::<4>(v), t.low_nibble);
        let special_cases = _mm256_and_si256(
            _mm256_and_si256(
                _mm256_shuffle_epi8(t.byte_1_high, high_nibbles(prev1)),
                _mm256_shuffle_epi8(t.byte_1_low, _mm256_and_si256(prev1, t.low_nibble)),
            ),
            _mm256_shuffle_epi8(t.byte_2_high, high_nibbles(input)),
        );
        let prev2 = prev_bytes::<2>(prev, input);
        let prev3 = prev_bytes::<3>(prev, input);
        // as in the 16-byte kernels: a 3 or 4 byte lead two or three behind requires a
        // continuation here, and XOR against TWO_CONTS leaves an error where that disagrees;
        // the saturated differences are at most 0x20 and 0x10, so the signed compare is exact
        let must_continue = _mm256_cmpgt_epi8(
            _mm256_or_si256(
                _mm256_subs_epu8(prev2, _mm256_set1_epi8((0xE0u8 - 1) as i8)),
                _mm256_subs_epu8(prev3, _mm256_set1_epi8((0xF0u8 - 1) as i8)),
            ),
            t.zero,
        );
        _mm256_xor_si256(_mm256_and_si256(must_continue, t.high_bit), special_cases)
    }

    #[inline]
    #[target_feature(enable = "avx2")]
    fn is_ascii(v: __m256i) -> bool {
        _mm256_movemask_epi8(v) == 0
    }

    #[target_feature(enable = "avx2")]
    pub(super) fn is_valid_utf8(bytes: &[u8]) -> bool {
        let t = Tables {
            byte_1_high: table(&BYTE_1_HIGH),
            byte_1_low: table(&BYTE_1_LOW),
            byte_2_high: table(&BYTE_2_HIGH),
            max_complete: load(&MAX_COMPLETE_32),
            low_nibble: _mm256_set1_epi8(0x0F),
            high_bit: _mm256_set1_epi8(0x80u8 as i8),
            zero: _mm256_setzero_si256(),
        };
        let mut prev = t.zero;
        // where the previous block ended inside a sequence; an error if that block was followed
        // by ASCII, which the next non-ASCII block would otherwise not notice
        let mut incomplete = t.zero;
        let mut error = t.zero;

        let (chunks, rest) = bytes.as_chunks::<32>();
        let (pairs, singles) = chunks.as_chunks::<2>();
        // two blocks at a time while both are ASCII
        for pair in pairs {
            let v = [load(&pair[0]), load(&pair[1])];
            if is_ascii(_mm256_or_si256(v[0], v[1])) {
                error = _mm256_or_si256(error, incomplete);
                incomplete = t.zero;
            } else {
                for input in v {
                    error = _mm256_or_si256(error, check_block(&t, prev, input));
                    prev = input;
                }
                incomplete = _mm256_subs_epu8(v[1], t.max_complete);
            }
            prev = v[1];
        }
        for chunk in singles {
            let input = load(chunk);
            if is_ascii(input) {
                error = _mm256_or_si256(error, incomplete);
                incomplete = t.zero;
            } else {
                error = _mm256_or_si256(error, check_block(&t, prev, input));
                incomplete = _mm256_subs_epu8(input, t.max_complete);
            }
            prev = input;
        }
        if rest.is_empty() {
            error = _mm256_or_si256(error, incomplete);
        } else {
            // the tail padded with zeros: a sequence cut by the end of the input is then a lead
            // followed by ASCII, which the lookups reject
            let mut tail = [0u8; 32];
            tail[..rest.len()].copy_from_slice(rest);
            let input = load(&tail);
            if is_ascii(input) {
                error = _mm256_or_si256(error, incomplete);
            } else {
                error = _mm256_or_si256(error, check_block(&t, prev, input));
            }
        }
        _mm256_testz_si256(error, error) == 1
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// The validators must agree with std, the oracle, and with simdutf8: accepted or not, and
    /// on rejection the same position and error length (which come from std by construction).
    fn assert_agree(bytes: &[u8]) {
        let want = core::str::from_utf8(bytes);
        let reference = simdutf8::compat::from_utf8(bytes);
        assert_eq!(
            reference.is_ok(),
            want.is_ok(),
            "simdutf8 disagrees with std on {bytes:x?}"
        );
        assert_eq!(is_valid_utf8(bytes), want.is_ok(), "fast validator on {bytes:x?}");
        #[cfg(target_arch = "x86_64")]
        {
            if std::is_x86_feature_detected!("avx2") {
                // SAFETY: AVX2 was just detected.
                assert_eq!(
                    unsafe { avx2::is_valid_utf8(bytes) },
                    want.is_ok(),
                    "avx2 kernel on {bytes:x?}"
                );
            }
            if std::is_x86_feature_detected!("ssse3") {
                // SAFETY: SSSE3 was just detected.
                assert_eq!(
                    unsafe { ssse3::is_valid_utf8(bytes) },
                    want.is_ok(),
                    "ssse3 kernel on {bytes:x?}"
                );
            }
        }
        match (from_utf8(bytes), want) {
            (Ok(got), Ok(want)) => assert_eq!(got, want),
            (Err(got), Err(want)) => {
                assert_eq!(got.valid_up_to(), want.valid_up_to(), "valid_up_to on {bytes:x?}");
                assert_eq!(got.error_len(), want.error_len(), "error_len on {bytes:x?}");
            }
            (got, want) => panic!("{got:?} but std says {want:?} on {bytes:x?}"),
        }
    }

    fn padded(pad: usize, middle: &[u8], tail: &[u8]) -> Vec<u8> {
        let mut bytes = vec![b'a'; pad];
        bytes.extend_from_slice(middle);
        bytes.extend_from_slice(tail);
        bytes
    }

    /// Bytes weighted towards the interesting ranges: continuations, leads of every length, the
    /// values that bound overlongs, surrogates and the code point limit, and plain ASCII.
    fn interesting_byte() -> impl Strategy<Value = u8> {
        prop_oneof![
            3 => 0x80u8..=0xBF,
            2 => 0xC0u8..=0xDF,
            2 => 0xE0u8..=0xEF,
            2 => 0xF0u8..=0xF7,
            1 => prop::sample::select(vec![0xC0u8, 0xC1, 0xC2, 0xE0, 0xE1, 0xED, 0xEF, 0xF0, 0xF1, 0xF4, 0xF5, 0xFF, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF]),
            1 => Just(0u8),
            3 => b' '..=b'~',
        ]
    }

    /// One sequence to plant: a valid character, or one of the classic invalid shapes.
    fn sequence() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            4 => any::<char>().prop_map(|c| c.to_string().into_bytes()),
            2 => prop::collection::vec(interesting_byte(), 1..5),
            1 => prop::sample::select(vec![
                vec![0xC0, 0x80],             // overlong 2-byte NUL
                vec![0xE0, 0x80, 0x80],       // overlong 3-byte
                vec![0xF0, 0x80, 0x80, 0x80], // overlong 4-byte
                vec![0xED, 0xA0, 0x80],       // surrogate D800
                vec![0xED, 0xBF, 0xBF],       // surrogate DFFF
                vec![0xF4, 0x90, 0x80, 0x80], // U+110000
                vec![0xF5, 0x80, 0x80, 0x80], // lead above F4
                vec![0x80],                   // stray continuation
                vec![0xC3],                   // cut 2-byte
                vec![0xE2, 0x82],             // cut 3-byte
                vec![0xF0, 0x9F, 0x98],       // cut 4-byte
                vec![0xE2, 0x82, 0xAC, 0xAC], // one continuation too many
                vec![0xC3, 0xA9, 0xC3],       // valid then cut
            ]),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        #[test]
        fn valid_text_is_accepted(text in any::<String>(), pad in 0usize..70) {
            assert_agree(&padded(pad, text.as_bytes(), b""));
        }

        #[test]
        fn random_bytes_agree(bytes in prop::collection::vec(interesting_byte(), 0..200)) {
            assert_agree(&bytes);
        }

        #[test]
        fn a_sequence_at_every_offset(pad in 0usize..70, seq in sequence(), tail in prop::collection::vec(interesting_byte(), 0..20)) {
            assert_agree(&padded(pad, &seq, &tail));
        }

        #[test]
        fn edited_text_agrees(text in any::<String>(), at in any::<prop::sample::Index>(), byte in interesting_byte(), pad in 0usize..70) {
            let mut bytes = padded(pad, text.as_bytes(), b"");
            if !bytes.is_empty() {
                let i = at.index(bytes.len());
                bytes[i] = byte;
            }
            assert_agree(&bytes);
        }
    }

    /// Every pair of bytes, at offsets that put it on both sides of a 16 and a 64-byte block
    /// edge, with ASCII around it.
    #[test]
    fn every_two_byte_window_at_block_edges() {
        for offset in [0usize, 13, 14, 15, 16, 31, 62, 63] {
            for a in 0..=255u8 {
                for b in 0..=255u8 {
                    assert_agree(&padded(offset, &[a, b], b"aaa"));
                }
            }
        }
    }

    /// Every triple of bytes crossing a 16 and a 32-byte block edge, a 64-byte group edge and at
    /// a block start. Sixteen million cases per offset: run in release with `--ignored`.
    #[test]
    #[ignore = "sixteen million cases per offset: run in release with --ignored"]
    fn every_three_byte_window() {
        for offset in [14usize, 30, 62, 0] {
            for a in 0..=255u8 {
                for b in 0..=255u8 {
                    for c in 0..=255u8 {
                        assert_agree(&padded(offset, &[a, b, c], b"aa"));
                    }
                }
            }
        }
    }

    /// Every four-byte lead with every second and third byte, and each fourth byte class.
    #[test]
    fn four_byte_sequences() {
        for lead in [0xF0u8, 0xF1, 0xF3, 0xF4, 0xF5] {
            for b in 0x7Fu8..=0xC0 {
                for c in 0x7Fu8..=0xC0 {
                    for d in [0x7Fu8, 0x80, 0xBF, 0xC0] {
                        assert_agree(&padded(13, &[lead, b, c, d], b"aa"));
                    }
                }
            }
        }
    }

    /// Sequences cut by the end of the input, after every prefix length up to a block edge and
    /// beyond, so the cut lands on every position of a block.
    #[test]
    fn sequences_cut_by_the_end() {
        for text in ["é", "€", "😀", "aé", "a€", "a😀"] {
            let bytes = text.as_bytes();
            for cut in 1..bytes.len() {
                for pad in 0..70 {
                    assert_agree(&padded(pad, &bytes[..cut], b""));
                    assert_agree(&padded(pad, bytes, &bytes[..cut]));
                }
            }
        }
    }

    #[test]
    fn ascii_of_every_length_and_a_stray_continuation() {
        for len in 0..200 {
            assert_agree(&vec![b'a'; len]);
            assert_agree(&padded(len, &[0x80], b""));
            assert_agree(&padded(len, "é".as_bytes(), b""));
        }
    }

    /// The block check reduces the OR of four chunks, so a lane is exactly 0x80 only when the
    /// other chunks hold NUL there: one special byte in a field of NULs, in every lane.
    #[test]
    fn one_byte_in_a_field_of_nuls() {
        for lane in 0..80 {
            for byte in [0x80u8, 0xBF, 0xC3, 0xE2, 0xF0, 0xF8, 0xFF, 0x7F] {
                let mut bytes = vec![0u8; 80];
                bytes[lane] = byte;
                assert_agree(&bytes);
            }
            let mut bytes = vec![0u8; 82];
            bytes[lane..lane + 2].copy_from_slice("é".as_bytes());
            assert_agree(&bytes);
        }
    }

    /// The continuation values where the nibble tables change, after every lead byte, two and
    /// three deep, on both sides of the 16 and 32-byte block edges and a 64-byte group edge.
    #[test]
    fn every_lead_with_boundary_continuations() {
        const EDGES: [u8; 8] = [0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0];
        for offset in [0usize, 13, 14, 15, 29, 30, 31, 61, 62, 63] {
            for lead in 0xC0..=0xFFu8 {
                for &b in &EDGES {
                    for &c in &EDGES {
                        assert_agree(&padded(offset, &[lead, b, c], b"aa"));
                        if lead >= 0xF0 {
                            for &d in &EDGES {
                                assert_agree(&padded(offset, &[lead, b, c, d], b"aa"));
                            }
                        }
                    }
                }
            }
        }
    }

    /// A sequence cut short and then ASCII, with the cut landing on every position of a 64-byte
    /// group and the ASCII running on for less than a block, a lone block, a group and more, and
    /// two groups: the ASCII fast path skips groups and lone blocks and must still see the cut
    /// before them.
    #[test]
    fn cut_sequences_followed_by_ascii() {
        for cut in [
            &[0xC3u8][..],
            &[0xE2, 0x82],
            &[0xF0, 0x9F, 0x98],
            &[0xE0],
            &[0xF4, 0x8F],
            &[0xED, 0x9F],
        ] {
            for pad in 0..70 {
                for tail_len in [12, 40, 72, 140] {
                    assert_agree(&padded(pad, cut, &vec![b'b'; tail_len]));
                    assert_agree(&padded(pad, cut, &vec![0u8; tail_len]));
                }
            }
        }
    }
}
