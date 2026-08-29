use std::mem::transmute;
#[rustfmt::skip]
use std::arch::aarch64::{
    uint8x16_t,
    uint16x8_t,
    uint32x4_t,
    uint64x2_t,
    uint8x8_t,
    uint16x4_t,
    uint32x2_t,
    uint64x1_t,
    // 16 byte methods
    vld1q_u8 as simd_load_16,
    vcgtq_u8 as simd_gt_16,
    vcltq_u8 as simd_lt_16,
    vorrq_u8 as simd_or_16,
    vceqq_u8 as simd_eq_16,
    // mask narrowing: 16x8-bit mask -> u64 of 16 nibbles
    vreinterpretq_u16_u8 as simd_cast_u16_8,
    vshrn_n_u16 as simd_shift_narrow_u16_8,
    vreinterpret_u64_u8 as simd_cast_u64_1,
    vget_lane_u64 as simd_get_lane_u64,
    vextq_u8 as combine_vecs_16,
    vsubq_u8 as simd_sub_16,
    vmulq_u8 as simd_mul_16,
    vpaddlq_u8 as simd_add_16,
    vmulq_u16 as simd_mul_u16_8,
    vpaddlq_u16 as simd_add_u16_8,
    vmulq_u32 as simd_mul_u32_4,
    vpaddlq_u32 as simd_add_u32_4,
    // 8 byte methods
    vget_low_u8 as simd_get_low,
    vext_u8 as combine_vecs_8,
    vsub_u8 as simd_sub_8,
    vmul_u8 as simd_mul_8,
    vpaddl_u8 as simd_add_8,
    vmul_u16 as simd_mul_u16_4,
    vpaddl_u16 as simd_add_u16_4,
    vmul_u32 as simd_mul_u32_2,
    vpaddl_u32 as simd_add_u32_2,
};
use crate::errors::{JsonResult, json_err};

use crate::number_decoder::IntChunk;
use crate::string_decoder::StringChunk;

use super::fallback_int::decode_int_chunk;
use super::fallback_string::{CHAR_TYPE, CharType, JSON_ASCII};

type SimdVecu8_16 = uint8x16_t;
type SimdVecu16_8 = uint16x8_t;
type SimdVecu32_4 = uint32x4_t;
type SimdVecu64_2 = uint64x2_t;

type SimdVecu8_8 = uint8x8_t;
type SimdVecu16_4 = uint16x4_t;
type SimdVecu32_2 = uint32x2_t;
type SimdVecu64_1 = uint64x1_t;
const SIMD_STEP: usize = 16;

macro_rules! simd_const {
    ($array:expr) => {
        // SAFETY: callers transmute arrays to SIMD vectors of the same size and element type.
        unsafe { transmute($array) }
    };
}

const ZERO_DIGIT_U8_8: SimdVecu8_8 = simd_const!([b'0'; 8]);
const ZERO_VAL_U8_8: SimdVecu8_8 = simd_const!([0u8; 8]);
const ALT_MUL_U8_8: SimdVecu8_8 = simd_const!([10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8]);
const ALT_MUL_U16_4: SimdVecu16_4 = simd_const!([100u16, 1u16, 100u16, 1u16]);
const ALT_MUL_U32_2: SimdVecu32_2 = simd_const!([10000u32, 1u32]);
const ZERO_DIGIT_16: SimdVecu8_16 = simd_const!([b'0'; 16]);
const NINE_DIGIT_16: SimdVecu8_16 = simd_const!([b'9'; 16]);

const ZERO_VAL_U8_16: SimdVecu8_16 = simd_const!([0u8; 16]);
const ALT_MUL_U8_16: SimdVecu8_16 = simd_const!([
    10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8
]);
const ALT_MUL_U16_8: SimdVecu16_8 = simd_const!([100u16, 1u16, 100u16, 1u16, 100u16, 1u16, 100u16, 1u16]);
const ALT_MUL_U32_4: SimdVecu32_4 = simd_const!([10000u32, 1u32, 10000u32, 1u32]);

#[target_feature(enable = "neon")]
pub(crate) fn decode_int_chunk_big(data: &[u8], index: usize) -> (IntChunk, usize) {
    if let Some(byte_chunk) = data.get(index..index + SIMD_STEP) {
        let byte_chunk: &[u8; SIMD_STEP] = byte_chunk.try_into().unwrap();
        let byte_vec = load_slice(byte_chunk);

        let digit_mask = get_digit_mask(byte_vec);
        if is_zero(digit_mask) {
            // all lanes are digits, parse the full vector
            let value = full_calc(byte_vec, 16);
            (IntChunk::Ongoing(value), index + SIMD_STEP)
        } else {
            // some lanes are not digits, transmute to a pair of u64 and find the first non-digit
            let last_digit = find_end(digit_mask);
            let index = index + last_digit as usize;
            let value = if last_digit <= 8 {
                // none-digit in the first 8 bytes
                first_half_calc(byte_vec, last_digit)
            } else {
                // none-digit in the last 8 bytes
                // SAFETY: `last_digit` is in 9..=15 and all preceding lanes are digits.
                full_calc(byte_vec, last_digit)
            };
            if next_is_float(data, index) {
                (IntChunk::Float(value), index)
            } else {
                (IntChunk::Done(value), index)
            }
        }
    } else {
        // we got near the end of the string, fall back to the slow path
        decode_int_chunk(data, index, 0)
    }
}

#[rustfmt::skip]
#[target_feature(enable = "neon")]
fn get_digit_mask(byte_vec: SimdVecu8_16) -> SimdVecu8_16 {
    simd_or_16(
        simd_lt_16(byte_vec, ZERO_DIGIT_16),
        simd_gt_16(byte_vec, NINE_DIGIT_16),
    )
}

#[target_feature(enable = "neon")]
fn first_half_calc(byte_vec: SimdVecu8_16, last_digit: u32) -> u64 {
    let small_byte_vec = simd_get_low(byte_vec);
    // subtract ascii '0' from every byte to get the digit values
    let digits: SimdVecu8_8 = simd_sub_8(small_byte_vec, ZERO_DIGIT_U8_8);
    let digits = match last_digit {
        0 => return 0,
        1 => {
            // SAFETY: transmute the 8x8-bit lanes into an array;
            let t: [u8; 8] = unsafe { transmute(digits) };
            return t[0] as u64;
        }
        2 => combine_vecs_8::<2>(ZERO_VAL_U8_8, digits),
        3 => combine_vecs_8::<3>(ZERO_VAL_U8_8, digits),
        4 => combine_vecs_8::<4>(ZERO_VAL_U8_8, digits),
        5 => combine_vecs_8::<5>(ZERO_VAL_U8_8, digits),
        6 => combine_vecs_8::<6>(ZERO_VAL_U8_8, digits),
        7 => combine_vecs_8::<7>(ZERO_VAL_U8_8, digits),
        8 => digits,
        _ => unreachable!("last_digit should be less than 8"),
    };
    // multiple every other digit by 10
    let x: SimdVecu8_8 = simd_mul_8(digits, ALT_MUL_U8_8);
    // add the value together and combine the 8x8-bit lanes into 4x16-bit lanes
    let x: SimdVecu16_4 = simd_add_8(x);
    // multiple every other digit by 100
    let x: SimdVecu16_4 = simd_mul_u16_4(x, ALT_MUL_U16_4);
    // add the value together and combine the 4x16-bit lanes into 2x32-bit lanes
    let x: SimdVecu32_2 = simd_add_u16_4(x);
    // multiple the first value 10000
    let x: SimdVecu32_2 = simd_mul_u32_2(x, ALT_MUL_U32_2);
    // add the value together and combine the 2x32-bit lanes into 1x64-bit lane
    let x: SimdVecu64_1 = simd_add_u32_2(x);
    // SAFETY: transmute the 64-bit lane into a u64
    unsafe { transmute(x) }
}

/// # Safety
///
/// `last_digit` must be in `9..=16`, and all preceding lanes must contain ASCII digits.
#[target_feature(enable = "neon")]
fn full_calc(byte_vec: SimdVecu8_16, last_digit: u32) -> u64 {
    // subtract ascii '0' from every byte to get the digit values
    let digits: SimdVecu8_16 = simd_sub_16(byte_vec, ZERO_DIGIT_16);
    let digits = match last_digit {
        9 => combine_vecs_16::<9>(ZERO_VAL_U8_16, digits),
        10 => combine_vecs_16::<10>(ZERO_VAL_U8_16, digits),
        11 => combine_vecs_16::<11>(ZERO_VAL_U8_16, digits),
        12 => combine_vecs_16::<12>(ZERO_VAL_U8_16, digits),
        13 => combine_vecs_16::<13>(ZERO_VAL_U8_16, digits),
        14 => combine_vecs_16::<14>(ZERO_VAL_U8_16, digits),
        15 => combine_vecs_16::<15>(ZERO_VAL_U8_16, digits),
        16 => digits,
        _ => unreachable!("last_digit should be between 9 and 16"),
    };
    // multiple every other digit by 10
    let x: SimdVecu8_16 = simd_mul_16(digits, ALT_MUL_U8_16);
    // add the value together and combine the 16x8-bit lanes into 8x16-bit lanes
    let x: SimdVecu16_8 = simd_add_16(x);
    // multiple every other digit by 100
    let x: SimdVecu16_8 = simd_mul_u16_8(x, ALT_MUL_U16_8);
    // add the value together and combine the 8x16-bit lanes into 4x32-bit lanes
    let x: SimdVecu32_4 = simd_add_u16_8(x);
    // multiple every other digit by 10000
    let x: SimdVecu32_4 = simd_mul_u32_4(x, ALT_MUL_U32_4);
    // add the value together and combine the 4x32-bit lanes into 2x64-bit lane
    let x: SimdVecu64_2 = simd_add_u32_4(x);

    // SAFETY: transmute the 2x64-bit lane into an array;
    let t: [u64; 2] = unsafe { transmute(x) };
    // since the data started out as digits, it's safe to assume the result fits in a u64
    t[0].wrapping_mul(100_000_000).wrapping_add(t[1])
}

fn next_is_float(data: &[u8], index: usize) -> bool {
    // SAFETY: callers derive `index` from the first non-digit lane in a loaded 16-byte chunk.
    let next = unsafe { data.get_unchecked(index) };
    matches!(next, b'.' | b'e' | b'E')
}

const QUOTE_16: SimdVecu8_16 = simd_const!([b'"'; 16]);
const BACKSLASH_16: SimdVecu8_16 = simd_const!([b'\\'; 16]);
// values below 32 are control characters
const CONTROL_16: SimdVecu8_16 = simd_const!([32u8; 16]);
const ASCII_MAX_16: SimdVecu8_16 = simd_const!([127u8; 16]);

#[inline(always)]
pub(crate) fn decode_string_chunk(
    data: &[u8],
    mut index: usize,
    mut ascii_only: bool,
    allow_partial: bool,
) -> JsonResult<(StringChunk, bool, usize)> {
    #[cfg(target_arch = "aarch64")]
    while let Some(byte_chunk) = data.get(index..index + SIMD_STEP) {
        let byte_chunk: &[u8; SIMD_STEP] = byte_chunk.try_into().unwrap();
        let byte_vec = load_slice(byte_chunk);

        // SAFETY: all supported aarch64 targets support neon intrinsics.
        if unsafe { mask_to_u64(string_ascii_mask(byte_vec)) } != 0 {
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
                            // SAFETY: all supported aarch64 targets support neon intrinsics.
                            let stop_mask = unsafe { mask_to_u64(string_stop_mask(byte_vec)) };
                            if stop_mask != 0 {
                                let stop_pos = (stop_mask.trailing_zeros() / 4) as usize;
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
#[target_feature(enable = "neon")]
/// returns a mask where any non-zero byte means we don't have a simple ascii character, either
/// quote, backslash, control character, or non-ascii (above 127)
fn string_ascii_mask(byte_vec: SimdVecu8_16) -> SimdVecu8_16 {
    simd_or_16(
        simd_eq_16(byte_vec, QUOTE_16),
        simd_or_16(
            simd_eq_16(byte_vec, BACKSLASH_16),
            simd_or_16(
                simd_gt_16(byte_vec, ASCII_MAX_16),
                simd_lt_16(byte_vec, CONTROL_16),
            )
        )
    )
}

#[rustfmt::skip]
#[target_feature(enable = "neon")]
/// returns a mask where any non-zero byte is a character that stops the string scan: either
/// a quote, backslash, or control character
fn string_stop_mask(byte_vec: SimdVecu8_16) -> SimdVecu8_16 {
    simd_or_16(
        simd_eq_16(byte_vec, QUOTE_16),
        simd_or_16(
            simd_eq_16(byte_vec, BACKSLASH_16),
            simd_lt_16(byte_vec, CONTROL_16),
        )
    )
}

fn find_end(digit_mask: SimdVecu8_16) -> u32 {
    // SAFETY: the SIMD vector and integer array are both 16 bytes.
    let t: [u64; 2] = unsafe { transmute(digit_mask) };
    if t[0] != 0 {
        // non-digit in the first 8 bytes
        t[0].trailing_zeros() / 8
    } else {
        t[1].trailing_zeros() / 8 + 8
    }
}

/// return true if all bytes are zero
fn is_zero(vec: SimdVecu8_16) -> bool {
    // SAFETY: the SIMD vector and integer array are both 16 bytes.
    let t: [u64; 2] = unsafe { transmute(vec) };
    t[0] == 0 && t[1] == 0
}

/// narrow a 16x8-bit comparison mask (each lane 0x00 or 0xFF) to a u64 where each byte becomes
/// a 0x0 or 0xF nibble - this only needs a single vector->general register transfer, unlike
/// transmuting to `[u64; 2]`
#[target_feature(enable = "neon")]
fn mask_to_u64(mask: SimdVecu8_16) -> u64 {
    let nibble_mask = simd_shift_narrow_u16_8::<4>(simd_cast_u16_8(mask));
    simd_get_lane_u64::<0>(simd_cast_u64_1(nibble_mask))
}

fn load_slice(bytes: &[u8; SIMD_STEP]) -> SimdVecu8_16 {
    // SAFETY: the array reference is valid for reading 16 contiguous bytes.
    unsafe { simd_load_16(bytes.as_ptr()) }
}
