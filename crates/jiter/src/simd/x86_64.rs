use std::mem::transmute;
#[rustfmt::skip]
use std::arch::x86_64::{
    __m128i,
    _mm_loadu_si128 as simd_load_16,
    _mm_cmpeq_epi8 as simd_eq_16,
    _mm_cmplt_epi8 as simd_lt_signed_16,
    _mm_min_epu8 as simd_min_16,
    _mm_or_si128 as simd_or_16,
    _mm_movemask_epi8 as simd_movemask_16,
    _mm_sub_epi8 as simd_sub_16,
    _mm_slli_si128 as simd_shift_bytes_16,
    _mm_and_si128 as simd_and_16,
    _mm_srli_epi16 as simd_shift_right_u16_8,
    _mm_mullo_epi16 as simd_mul_u16_8,
    _mm_add_epi16 as simd_add_u16_8,
    _mm_madd_epi16 as simd_mul_add_u16_8,
    _mm_mul_epu32 as simd_mul_u32_4,
    _mm_srli_epi64 as simd_shift_right_u64_2,
    _mm_add_epi64 as simd_add_u64_2,
};
use crate::errors::{JsonResult, json_err};

use crate::number_decoder::IntChunk;
use crate::string_decoder::StringChunk;

use super::fallback_int::decode_int_chunk;
use super::fallback_string::{CHAR_TYPE, CharType, JSON_ASCII};

type SimdVec = __m128i;
const SIMD_STEP: usize = 16;

macro_rules! simd_const {
    ($array:expr) => {
        unsafe { transmute($array) }
    };
}

const ZERO_DIGIT_16: SimdVec = simd_const!([b'0'; 16]);
const NINE_VAL_16: SimdVec = simd_const!([9u8; 16]);
const LOW_BYTE_U16_8: SimdVec = simd_const!([0x00ffu16; 8]);
const TEN_U16_8: SimdVec = simd_const!([10u16; 8]);
const ALT_MUL_U16_8: SimdVec = simd_const!([100u16, 1u16, 100u16, 1u16, 100u16, 1u16, 100u16, 1u16]);
const ALT_MUL_U32_4: SimdVec = simd_const!([10000u32, 0u32, 10000u32, 0u32]);

#[inline]
#[target_feature(enable = "sse2")]
pub(crate) fn decode_int_chunk_big(data: &[u8], index: usize) -> (IntChunk, usize) {
    if let Some(byte_chunk) = data.get(index..index + SIMD_STEP) {
        let byte_vec = load_slice(byte_chunk);
        let digits = simd_sub_16(byte_vec, ZERO_DIGIT_16);

        let last_digit = first_non_digit(digits);
        if last_digit == 16 {
            let value = unsafe { full_calc(digits, 16) };
            (IntChunk::Ongoing(value), index + SIMD_STEP)
        } else {
            let index = index + last_digit as usize;
            if next_is_float(data, index) {
                (IntChunk::Float, index)
            } else {
                let value = unsafe { full_calc(digits, last_digit) };
                (IntChunk::Done(value), index)
            }
        }
    } else {
        decode_int_chunk(data, index, 0)
    }
}

/// position of the first byte that is not a digit, 16 if all bytes are digits
#[target_feature(enable = "sse2")]
fn first_non_digit(digits: SimdVec) -> u32 {
    let digit_mask = simd_eq_16(simd_min_16(digits, NINE_VAL_16), digits);
    (!mask_to_u32(digit_mask)).trailing_zeros()
}

// TODO: SSSE3 `_mm_maddubs_epi16` would replace the and/shift/mullo/add byte->u16 step
// TODO: SSE4.1 `_mm_packus_epi32` could pair lanes and shorten the u32->u64 reduction
#[target_feature(enable = "sse2")]
unsafe fn full_calc(digits: SimdVec, last_digit: u32) -> u64 {
    unsafe {
        let digits = match last_digit {
            0 => return 0,
            1 => simd_shift_bytes_16::<15>(digits),
            2 => simd_shift_bytes_16::<14>(digits),
            3 => simd_shift_bytes_16::<13>(digits),
            4 => simd_shift_bytes_16::<12>(digits),
            5 => simd_shift_bytes_16::<11>(digits),
            6 => simd_shift_bytes_16::<10>(digits),
            7 => simd_shift_bytes_16::<9>(digits),
            8 => simd_shift_bytes_16::<8>(digits),
            9 => simd_shift_bytes_16::<7>(digits),
            10 => simd_shift_bytes_16::<6>(digits),
            11 => simd_shift_bytes_16::<5>(digits),
            12 => simd_shift_bytes_16::<4>(digits),
            13 => simd_shift_bytes_16::<3>(digits),
            14 => simd_shift_bytes_16::<2>(digits),
            15 => simd_shift_bytes_16::<1>(digits),
            16 => digits,
            _ => unreachable!("last_digit should be at most 16"),
        };
        // 16x8-bit lanes -> 8x16-bit lanes: first digit * 10 + second digit
        let lo = simd_and_16(digits, LOW_BYTE_U16_8);
        let hi = simd_shift_right_u16_8::<8>(digits);
        let x = simd_add_u16_8(simd_mul_u16_8(lo, TEN_U16_8), hi);
        // 8x16-bit lanes -> 4x32-bit lanes: first * 100 + second
        let x = simd_mul_add_u16_8(x, ALT_MUL_U16_8);
        // 4x32-bit lanes -> 2x64-bit lanes: first * 10000 + second
        let x = simd_add_u64_2(simd_mul_u32_4(x, ALT_MUL_U32_4), simd_shift_right_u64_2::<32>(x));

        let t: [u64; 2] = transmute(x);
        t[0].wrapping_mul(100_000_000).wrapping_add(t[1])
    }
}

fn next_is_float(data: &[u8], index: usize) -> bool {
    let next = unsafe { data.get_unchecked(index) };
    matches!(next, b'.' | b'e' | b'E')
}

const QUOTE_16: SimdVec = simd_const!([b'"'; 16]);
const BACKSLASH_16: SimdVec = simd_const!([b'\\'; 16]);
const CONTROL_16: SimdVec = simd_const!([32u8; 16]);
const CONTROL_MAX_16: SimdVec = simd_const!([31u8; 16]);

// TODO: an AVX2 32-byte loop behind `is_x86_feature_detected!` may help long strings
#[inline]
#[target_feature(enable = "sse2")]
pub(crate) fn decode_string_chunk(
    data: &[u8],
    mut index: usize,
    mut ascii_only: bool,
    allow_partial: bool,
) -> JsonResult<(StringChunk, bool, usize)> {
    while let Some(byte_chunk) = data.get(index..index + SIMD_STEP) {
        let byte_vec = load_slice(byte_chunk);

        if mask_to_u32(string_ascii_mask(byte_vec)) != 0 {
            // this chunk contains a special character, classify the first one with a scalar scan.
            // this looks like it defeats the point of SIMD, but the byte-by-byte scan branches
            // are predictable and crucially keep the returned index off the (slow) vector->general
            // register transfer path, unlike computing the position from the mask
            for (pos, next) in byte_chunk.iter().enumerate() {
                if !JSON_ASCII[*next as usize] {
                    match &CHAR_TYPE[*next as usize] {
                        CharType::Quote => return Ok((StringChunk::StringEnd, ascii_only, index + pos)),
                        CharType::Backslash => return Ok((StringChunk::Backslash, ascii_only, index + pos)),
                        CharType::ControlChar => return json_err!(ControlCharacterWhileParsingString, index + pos),
                        CharType::Other => {
                            // non-ascii character: use the mask to jump over the rest of the
                            // chunk instead of scanning it byte-by-byte
                            ascii_only = false;
                            let stop_mask = mask_to_u32(string_stop_mask(byte_vec));
                            if stop_mask != 0 {
                                let stop_pos = stop_mask.trailing_zeros() as usize;
                                index += stop_pos;
                                return match byte_chunk[stop_pos] {
                                    b'"' => Ok((StringChunk::StringEnd, false, index)),
                                    b'\\' => Ok((StringChunk::Backslash, false, index)),
                                    _ => json_err!(ControlCharacterWhileParsingString, index),
                                };
                            }
                            // no stop character in this chunk, continue to the next chunk
                            break;
                        }
                    }
                }
            }
        }
        index += SIMD_STEP;
    }
    // we got near the end of the string, fall back to the slow path
    super::fallback_string::decode_string_chunk(data, index, ascii_only, allow_partial)
}

#[rustfmt::skip]
/// returns a mask where any non-zero byte means we don't have a simple ascii character, either
/// quote, backslash, control character, or non-ascii (above 127). The signed comparison against
/// 32 catches both control characters and bytes above 127, which are negative as i8
#[target_feature(enable = "sse2")]
fn string_ascii_mask(byte_vec: SimdVec) -> SimdVec {
    simd_or_16(
        simd_eq_16(byte_vec, QUOTE_16),
        simd_or_16(
            simd_eq_16(byte_vec, BACKSLASH_16),
            simd_lt_signed_16(byte_vec, CONTROL_16),
        )
    )
}

#[rustfmt::skip]
/// returns a mask where any non-zero byte is a character that stops the string scan: either
/// a quote, backslash, or control character
#[target_feature(enable = "sse2")]
fn string_stop_mask(byte_vec: SimdVec) -> SimdVec {
    simd_or_16(
        simd_eq_16(byte_vec, QUOTE_16),
        simd_or_16(
            simd_eq_16(byte_vec, BACKSLASH_16),
            simd_eq_16(simd_min_16(byte_vec, CONTROL_MAX_16), byte_vec),
        )
    )
}

/// one bit per byte lane, set where the comparison mask lane is 0xFF
#[target_feature(enable = "sse2")]
fn mask_to_u32(mask: SimdVec) -> u32 {
    simd_movemask_16(mask).cast_unsigned()
}

#[target_feature(enable = "sse2")]
fn load_slice(bytes: &[u8]) -> SimdVec {
    debug_assert_eq!(bytes.len(), 16);
    unsafe { simd_load_16(bytes.as_ptr().cast()) }
}
