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
    vandq_u8 as simd_and_16,
    vceqq_u8 as simd_eq_16,
    vmaxvq_u8 as simd_max_16,
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
use crate::JsonResult;

use crate::number_decoder::{decode_int_chunk_fallback, IntChunk};
use crate::string_decoder::StringChunk;

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
const ALT_MUL_U8_16: SimdVecu8_16 =
    simd_const!([10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8]);
const ALT_MUL_U16_8: SimdVecu16_8 = simd_const!([100u16, 1u16, 100u16, 1u16, 100u16, 1u16, 100u16, 1u16]);
const ALT_MUL_U32_4: SimdVecu32_4 = simd_const!([10000u32, 1u32, 10000u32, 1u32]);

pub(crate) fn decode_int_chunk(data: &[u8], index: usize) -> (IntChunk, usize) {
    #[rustfmt::skip]
    fn get_digit_mask(byte_vec: SimdVecu8_16) -> SimdVecu8_16 {
        unsafe {
            simd_or_16(
                simd_lt_16(byte_vec, ZERO_DIGIT_16),
                simd_gt_16(byte_vec, NINE_DIGIT_16),
            )
        }
    }

    unsafe fn first_half_calc(byte_vec: SimdVecu8_16, last_digit: u32) -> u64 {
        let small_byte_vec = simd_get_low(byte_vec);
        // subtract ascii '0' from every byte to get the digit values
        let digits: SimdVecu8_8 = simd_sub_8(small_byte_vec, ZERO_DIGIT_U8_8);
        let digits = match last_digit {
            0 => return 0,
            1 => {
                let t: [u8; 8] = transmute(digits);
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
        // transmute the 64-bit lane into a u64
        transmute(x)
    }

    unsafe fn full_calc(byte_vec: SimdVecu8_16, last_digit: u32) -> u64 {
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

        // transmute the 2x64-bit lane into an array;
        let t: [u64; 2] = transmute(x);
        // since the data started out as digits, it's safe to assume the result fits in a u64
        t[0].wrapping_mul(100000000).wrapping_add(t[1])
    }

    fn next_is_float(data: &[u8], index: usize) -> bool {
        let next = unsafe { data.get_unchecked(index) };
        matches!(next, b'.' | b'e' | b'E')
    }

    if let Some(byte_chunk) = data.get(index..index + SIMD_STEP) {
        let byte_vec = load_slice(byte_chunk);

        let digit_mask = get_digit_mask(byte_vec);
        return if is_zero(digit_mask) {
            // all lanes are digits, parse the full vector
            let value = unsafe { full_calc(byte_vec, 16) };
            (IntChunk::Ongoing(value), index + SIMD_STEP)
        } else {
            // some lanes are not digits, transmute to a pair of u64 and find the first non-digit
            let t: [u64; 2] = unsafe { transmute(digit_mask) };
            if t[0] != 0 {
                // none-digit in the first 8 bytes
                let last_digit = t[0].trailing_zeros() / 8;
                let index = index + last_digit as usize;
                if next_is_float(data, index) {
                    (IntChunk::Float, index)
                } else {
                    let value = unsafe { first_half_calc(byte_vec, last_digit) };
                    (IntChunk::Done(value), index)
                }
            } else {
                // none-digit in the last 8 bytes
                let last_digit = t[1].trailing_zeros() / 8 + 8;
                if last_digit == 8 {
                    // all bytes in the first half are digits
                    let index = index + 8;
                    if next_is_float(data, index) {
                        (IntChunk::Float, index)
                    } else {
                        let value = unsafe { first_half_calc(byte_vec, 8) };
                        (IntChunk::Done(value), index)
                    }
                } else {
                    let index = index + last_digit as usize;
                    if next_is_float(data, index) {
                        (IntChunk::Float, index)
                    } else {
                        let value = unsafe { full_calc(byte_vec, last_digit) };
                        (IntChunk::Done(value), index)
                    }
                }
            }
        };
    }
    // we got near the end of the string, fall back to the slow path
    decode_int_chunk_fallback(data, index)
}

const JSON_MIN_16: SimdVecu8_16 = simd_const!([32u8; 16]);
const ASCII_MAX_16: SimdVecu8_16 = simd_const!([127u8; 16]);
const QUOTE_16: SimdVecu8_16 = simd_const!([b'"'; 16]);
const BACKSLASH_16: SimdVecu8_16 = simd_const!([b'\\'; 16]);

pub fn decode_string_chunk(
    data: &[u8],
    mut index: usize,
    mut ascii_only: bool,
) -> JsonResult<(StringChunk, bool, usize)> {
    #[rustfmt::skip]
    /// returns a mask where any non-zero byte means we should parsing a JSON string
    fn json_stop_mask(byte_vec: SimdVecu8_16) -> SimdVecu8_16 {
        unsafe {
            simd_or_16(
                simd_eq_16(byte_vec, QUOTE_16),
                simd_or_16(
                    simd_eq_16(byte_vec, BACKSLASH_16),
                    simd_lt_16(byte_vec, JSON_MIN_16),
                )
            )
        }
    }

    let chunks = data
        .get(index..)
        .into_iter()
        .flat_map(|remaining| remaining.chunks_exact(SIMD_STEP));

    for byte_chunk in chunks {
        let byte_vec = load_slice(byte_chunk);

        let stop_mask = json_stop_mask(byte_vec);
        if is_zero(stop_mask) {
            if ascii_only {
                // check if there are any non-ascii bytes
                let non_ascii_mask = unsafe { simd_gt_16(byte_vec, ASCII_MAX_16) };
                ascii_only = is_zero(non_ascii_mask);
            }
            index += SIMD_STEP;
            continue;
        }
        // some lane(s) are a stop character, means we have to stop and find the first non-zero byte
        let t: [u64; 2] = unsafe { transmute(stop_mask) };
        let stop_index = if t[0] != 0 {
            // stop char in the first 8 bytes
            (t[0].trailing_zeros() / 8) as usize
        } else {
            // stop char in the second 8 bytes
            (t[1].trailing_zeros() / 8 + 8) as usize
        };
        if ascii_only {
            // we need to mask out the bit after the last char, then check if there are any non-ascii bytes
            let stop_mask = unsafe { STOP_MASKS.get_unchecked(stop_index) };
            let bytes_masked = unsafe { simd_and_16(byte_vec, *stop_mask) };
            let non_ascii_mask = unsafe { simd_gt_16(bytes_masked, ASCII_MAX_16) };
            ascii_only = is_zero(non_ascii_mask);
        }

        let last_char = unsafe { byte_chunk.get_unchecked(stop_index) };
        return StringChunk::decode_finish(*last_char, ascii_only, index + stop_index);
    }
    // we got near the end of the string, fall back to the slow path
    StringChunk::decode_fallback(data, index, ascii_only)
}

/// mask for each position of the stop char
const STOP_MASKS: [SimdVecu8_16; 16] = {
    const XX: u8 = 255;
    const __: u8 = 0;
    [
        simd_const!([__, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __]),
        simd_const!([XX, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __]),
        simd_const!([XX, XX, __, __, __, __, __, __, __, __, __, __, __, __, __, __]),
        simd_const!([XX, XX, XX, __, __, __, __, __, __, __, __, __, __, __, __, __]),
        simd_const!([XX, XX, XX, XX, __, __, __, __, __, __, __, __, __, __, __, __]),
        simd_const!([XX, XX, XX, XX, XX, __, __, __, __, __, __, __, __, __, __, __]),
        simd_const!([XX, XX, XX, XX, XX, XX, __, __, __, __, __, __, __, __, __, __]),
        simd_const!([XX, XX, XX, XX, XX, XX, XX, __, __, __, __, __, __, __, __, __]),
        simd_const!([XX, XX, XX, XX, XX, XX, XX, XX, __, __, __, __, __, __, __, __]),
        simd_const!([XX, XX, XX, XX, XX, XX, XX, XX, XX, __, __, __, __, __, __, __]),
        simd_const!([XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, __, __, __, __, __, __]),
        simd_const!([XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, __, __, __, __, __]),
        simd_const!([XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, __, __, __, __]),
        simd_const!([XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, __, __, __]),
        simd_const!([XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, __, __]),
        simd_const!([XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, __]),
    ]
};

/// return true if all bytes are zero
fn is_zero(vec: SimdVecu8_16) -> bool {
    unsafe { simd_max_16(vec) == 0 }
}

fn load_slice(bytes: &[u8]) -> SimdVecu8_16 {
    debug_assert_eq!(bytes.len(), 16);
    unsafe { simd_load_16(bytes.as_ptr()) }
}
