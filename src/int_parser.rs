use crate::errors::json_err;
use crate::{JsonResult, NumberInt};
use num_bigint::BigInt;

#[derive(Debug)]
pub(crate) enum IntParse {
    Int(NumberInt),
    Float,
    FloatInf(bool),
    FloatNaN,
}

impl IntParse {
    /// Turns out this is faster than fancy bit manipulation, see
    /// https://github.com/Alexhuszagh/rust-lexical/blob/main/lexical-parse-integer/docs/Algorithm.md
    /// for some context
    pub(crate) fn parse(data: &[u8], mut index: usize, first: u8) -> JsonResult<(Self, usize)> {
        let start = index;
        let positive = match first {
            b'N' => return Ok((Self::FloatNaN, index)),
            b'-' => false,
            _ => true,
        };
        if !positive {
            // we started with a minus sign, so the first digit is at index + 1
            index += 1;
        };
        let first2 = if positive { Some(&first) } else { data.get(index) };
        match first2 {
            Some(b'0') => {
                index += 1;
                return match data.get(index) {
                    Some(b'.') => Ok((Self::Float, index)),
                    Some(b'e') | Some(b'E') => Ok((Self::Float, index)),
                    Some(digit) if digit.is_ascii_digit() => json_err!(InvalidNumber, index),
                    _ => Ok((Self::Int(NumberInt::Int(0)), index)),
                };
            }
            Some(b'I') => return Ok((Self::FloatInf(positive), index)),
            Some(digit) if (b'1'..=b'9').contains(digit) => (),
            Some(_) => return json_err!(InvalidNumber, index),
            None => return json_err!(EofWhileParsingValue, index),
        };

        let (chunk, new_index) = ParseChunk::parse(data, index);

        let mut big_value: BigInt = match chunk {
            ParseChunk::Ongoing(value) => value.into(),
            ParseChunk::Done(value) => {
                let mut value_i64 = value as i64;
                if !positive {
                    value_i64 = -value_i64
                }
                return Ok((Self::Int(NumberInt::Int(value_i64)), new_index));
            }
            ParseChunk::Float => return Ok((Self::Float, new_index)),
        };
        index = new_index;

        // number is too big for i64, we need ot use a big int
        loop {
            let (chunk, new_index) = ParseChunk::parse(data, index);
            match chunk {
                ParseChunk::Ongoing(value) => {
                    big_value *= POW_10[new_index - index];
                    big_value += value;
                    index = new_index;
                }
                ParseChunk::Done(value) => {
                    if (new_index - start) > 4300 {
                        return json_err!(NumberOutOfRange, start + 4301);
                    }
                    big_value *= POW_10[new_index - index];
                    big_value += value;
                    if !positive {
                        big_value = -big_value;
                    }
                    return Ok((Self::Int(NumberInt::BigInt(big_value)), new_index));
                }
                ParseChunk::Float => return Ok((Self::Float, new_index)),
            }
        }
    }
}

static POW_10: [u64; 18] = [
    1,
    10,
    100,
    1000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
    10_000_000_000,
    100_000_000_000,
    1_000_000_000_000,
    10_000_000_000_000,
    100_000_000_000_000,
    1_000_000_000_000_000,
    10_000_000_000_000_000,
    100_000_000_000_000_000,
];

enum ParseChunk {
    Ongoing(u64),
    Done(u64),
    Float,
}

impl ParseChunk {
    fn parse(data: &[u8], index: usize) -> (Self, usize) {
        // TODO x86_64: use simd

        #[cfg(target_arch = "aarch64")]
        {
            parse_int_chunk_aarch64(data, index)
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            parse_int_chunk_fallback(data, index)
        }
    }
}

#[cfg(target_arch = "aarch64")]
fn parse_int_chunk_aarch64(data: &[u8], index: usize) -> (ParseChunk, usize) {
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
    type SimdVecu8_16 = uint8x16_t;
    type SimdVecu16_8 = uint16x8_t;
    type SimdVecu32_4 = uint32x4_t;
    type SimdVecu64_2 = uint64x2_t;

    type SimdVecu8_8 = uint8x8_t;
    type SimdVecu16_4 = uint16x4_t;
    type SimdVecu32_2 = uint32x2_t;
    type SimdVecu64_1 = uint64x1_t;
    const SIMD_STEP: usize = 16;

    /// return true if all bytes are zero
    fn is_vector_zero(vec: SimdVecu8_16) -> bool {
        unsafe { simd_max_16(vec) == 0 }
    }

    const ZERO_DIGIT_16: SimdVecu8_16 = unsafe { transmute([b'0'; 16]) };
    const NINE_DIGIT_16: SimdVecu8_16 = unsafe { transmute([b'9'; 16]) };

    unsafe fn get_digit_mask(byte_vec: SimdVecu8_16) -> SimdVecu8_16 {
        simd_or_16(simd_lt_16(byte_vec, ZERO_DIGIT_16), simd_gt_16(byte_vec, NINE_DIGIT_16))
    }

    const ZERO_DIGIT_U8_8: SimdVecu8_8 = unsafe { transmute([b'0'; 8]) };
    const ZERO_VAL_U8_8: SimdVecu8_8 = unsafe { transmute([0u8; 8]) };
    const ALT_MUL_U8_8: SimdVecu8_8 = unsafe { transmute([10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8]) };
    const ALT_MUL_U16_4: SimdVecu16_4 = unsafe { transmute([100u16, 1u16, 100u16, 1u16]) };
    const ALT_MUL_U32_2: SimdVecu32_2 = unsafe { transmute([10000u32, 1u32]) };

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

    const ZERO_VAL_U8_16: SimdVecu8_16 = unsafe { transmute([0u8; 16]) };
    const ALT_MUL_U8_16: SimdVecu8_16 = unsafe {
        transmute([
            10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8, 10u8, 1u8,
        ])
    };
    const ALT_MUL_U16_8: SimdVecu16_8 = unsafe { transmute([100u16, 1u16, 100u16, 1u16, 100u16, 1u16, 100u16, 1u16]) };
    const ALT_MUL_32: SimdVecu32_4 = unsafe { transmute([10000u32, 1u32, 10000u32, 1u32]) };

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
        let x: SimdVecu32_4 = simd_mul_u32_4(x, ALT_MUL_32);
        // add the value together and combine the 4x32-bit lanes into 2x64-bit lane
        let x: SimdVecu64_2 = simd_add_u32_4(x);

        // transmute the 2x64-bit lane into an array;
        let t: [u64; 2] = transmute(x);
        // since the data started out as digits, it's safe to assume the result fits in a u64
        t[0] * 100000000 + t[1]
    }

    unsafe fn next_is_float(data: &[u8], index: usize) -> bool {
        let next = data.get_unchecked(index);
        matches!(next, b'.' | b'e' | b'E')
    }

    unsafe {
        if let Some(byte_chunk) = data.get(index..index + SIMD_STEP) {
            let byte_vec = simd_load_16(byte_chunk.as_ptr());

            let digit_mask = get_digit_mask(byte_vec);
            return if is_vector_zero(digit_mask) {
                // all lanes are digits, parse the full vector
                let value = full_calc(byte_vec, 16);
                (ParseChunk::Ongoing(value), index + SIMD_STEP)
            } else {
                // some lanes are not digits, transmute to a pair of u64 and find the first non-digit
                let t: [u64; 2] = transmute(digit_mask);
                if t[0] != 0 {
                    // none-digit in the first 8 bytes
                    let last_digit = t[0].trailing_zeros() / 8;
                    let index = index + last_digit as usize;
                    if next_is_float(data, index) {
                        (ParseChunk::Float, index)
                    } else {
                        let value = first_half_calc(byte_vec, last_digit);
                        (ParseChunk::Done(value), index)
                    }
                } else {
                    // none-digit in the last 8 bytes
                    let last_digit = t[1].trailing_zeros() / 8 + 8;
                    if last_digit == 8 {
                        // all bytes in the second half are digits
                        let index = index + 8;
                        if next_is_float(data, index) {
                            (ParseChunk::Float, index)
                        } else {
                            let value = first_half_calc(byte_vec, 8);
                            (ParseChunk::Done(value), index)
                        }
                    } else {
                        let index = index + last_digit as usize;
                        if next_is_float(data, index) {
                            (ParseChunk::Float, index)
                        } else {
                            let value = full_calc(byte_vec, last_digit);
                            (ParseChunk::Done(value), index)
                        }
                    }
                }
            };
        }
    }
    // we got near the end of the string, fall back to the slow path
    parse_int_chunk_fallback(data, index)
}

fn parse_int_chunk_fallback(data: &[u8], mut index: usize) -> (ParseChunk, usize) {
    let mut value = 0u64;
    // i64::MAX = 9223372036854775807 - 18 chars is always enough
    for _ in 1..18 {
        if let Some(digit) = data.get(index) {
            if INT_CHAR_MAP[*digit as usize] {
                // we use wrapping add to avoid branching - we know the value cannot wrap
                value = value.wrapping_mul(10).wrapping_add((digit & 0x0f) as u64);
                index += 1;
                continue;
            } else if matches!(digit, b'.' | b'e' | b'E') {
                return (ParseChunk::Float, index);
            }
        }
        return (ParseChunk::Done(value), index);
    }
    (ParseChunk::Ongoing(value), index)
}

pub(crate) static INT_CHAR_MAP: [bool; 256] = {
    const NU: bool = true;
    const __: bool = false;
    [
        //   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 0
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 1
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 2
        NU, NU, NU, NU, NU, NU, NU, NU, NU, NU, __, __, __, __, __, __, // 3
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 4
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 5
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 6
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 7
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 8
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 9
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // A
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // B
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // C
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // D
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // E
        __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // F
    ]
};
