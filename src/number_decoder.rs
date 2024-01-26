use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;
use std::ops::Range;

use lexical_core::{format as lexical_format, parse_partial_with_options, ParseFloatOptions};

use crate::errors::{json_err, JsonResult};

pub trait AbstractNumberDecoder {
    type Output;

    fn decode(data: &[u8], index: usize, first: u8, allow_inf_nan: bool) -> JsonResult<(Self::Output, usize)>;
}

/// A number that can be either an [i64] or a [BigInt](num_bigint::BigInt)
#[derive(Debug, Clone, PartialEq)]
pub enum NumberInt {
    Int(i64),
    BigInt(BigInt),
}

impl From<NumberInt> for f64 {
    fn from(num: NumberInt) -> Self {
        match num {
            NumberInt::Int(int) => int as f64,
            NumberInt::BigInt(big_int) => match big_int.to_f64() {
                Some(f) => f,
                None => f64::NAN,
            },
        }
    }
}

impl AbstractNumberDecoder for NumberInt {
    type Output = NumberInt;

    fn decode(data: &[u8], index: usize, first: u8, _allow_inf_nan: bool) -> JsonResult<(Self::Output, usize)> {
        let (int_parse, index) = IntParse::parse(data, index, first)?;
        match int_parse {
            IntParse::Int(int) => Ok((int, index)),
            _ => json_err!(FloatExpectingInt, index),
        }
    }
}

pub struct NumberFloat;

impl AbstractNumberDecoder for NumberFloat {
    type Output = f64;

    fn decode(data: &[u8], mut index: usize, first: u8, allow_inf_nan: bool) -> JsonResult<(Self::Output, usize)> {
        let start = index;

        let positive = match first {
            b'N' => return consume_nan(data, index, allow_inf_nan),
            b'-' => false,
            _ => true,
        };
        if !positive {
            // we started with a minus sign, so the first digit is at index + 1
            index += 1;
        };
        let first2 = if positive { Some(&first) } else { data.get(index) };

        if let Some(digit) = first2 {
            if INT_CHAR_MAP[*digit as usize] {
                const JSON: u128 = lexical_format::JSON;
                let options = ParseFloatOptions::new();
                match parse_partial_with_options::<f64, JSON>(&data[start..], &options) {
                    Ok((float, index)) => Ok((float, index + start)),
                    Err(_) => {
                        // it's impossible to work out the right error from LexicalError here, so we parse again
                        // with NumberRange and use that error
                        match NumberRange::decode(data, start, first, allow_inf_nan) {
                            Err(e) => Err(e),
                            // NumberRange should always raise an error if `parse_partial_with_options`
                            // except for Infinity and -Infinity, which are handled above
                            Ok(_) => unreachable!("NumberRange should always return an error"),
                        }
                    }
                }
            } else if digit == &b'I' {
                consume_inf(data, index, positive, allow_inf_nan)
            } else {
                json_err!(InvalidNumber, index)
            }
        } else {
            json_err!(EofWhileParsingValue, index)
        }
    }
}

/// A number that can be either a [NumberInt] or an [f64]
#[derive(Debug, Clone, PartialEq)]
pub enum NumberAny {
    Int(NumberInt),
    Float(f64),
}

impl From<NumberAny> for f64 {
    fn from(num: NumberAny) -> Self {
        match num {
            NumberAny::Int(int) => int.into(),
            NumberAny::Float(f) => f,
        }
    }
}

impl AbstractNumberDecoder for NumberAny {
    type Output = NumberAny;

    fn decode(data: &[u8], index: usize, first: u8, allow_inf_nan: bool) -> JsonResult<(Self::Output, usize)> {
        let start = index;
        let (int_parse, index) = IntParse::parse(data, index, first)?;
        match int_parse {
            IntParse::Int(int) => Ok((Self::Int(int), index)),
            IntParse::Float => {
                NumberFloat::decode(data, start, first, allow_inf_nan).map(|(f, index)| (Self::Float(f), index))
            }
            IntParse::FloatInf(positive) => {
                consume_inf(data, index, positive, allow_inf_nan).map(|(f, index)| (Self::Float(f), index))
            }
            IntParse::FloatNaN => consume_nan(data, index, allow_inf_nan).map(|(f, index)| (Self::Float(f), index)),
        }
    }
}

fn consume_inf(data: &[u8], index: usize, positive: bool, allow_inf_nan: bool) -> JsonResult<(f64, usize)> {
    if allow_inf_nan {
        let end = crate::parse::consume_infinity(data, index)?;
        if positive {
            Ok((f64::INFINITY, end))
        } else {
            Ok((f64::NEG_INFINITY, end))
        }
    } else if positive {
        json_err!(ExpectedSomeValue, index)
    } else {
        json_err!(InvalidNumber, index)
    }
}

fn consume_nan(data: &[u8], index: usize, allow_inf_nan: bool) -> JsonResult<(f64, usize)> {
    if allow_inf_nan {
        let end = crate::parse::consume_nan(data, index)?;
        Ok((f64::NAN, end))
    } else {
        json_err!(ExpectedSomeValue, index)
    }
}

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

        let (chunk, new_index) = IntChunk::parse(data, index);

        let mut big_value: BigInt = match chunk {
            IntChunk::Ongoing(value) => value.into(),
            IntChunk::Done(value) => {
                let mut value_i64 = value as i64;
                if !positive {
                    value_i64 = -value_i64
                }
                return Ok((Self::Int(NumberInt::Int(value_i64)), new_index));
            }
            IntChunk::Float => return Ok((Self::Float, new_index)),
        };
        index = new_index;

        // number is too big for i64, we need ot use a big int
        loop {
            let (chunk, new_index) = IntChunk::parse(data, index);
            match chunk {
                IntChunk::Ongoing(value) => {
                    big_value *= POW_10[new_index - index];
                    big_value += value;
                    index = new_index;
                }
                IntChunk::Done(value) => {
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
                IntChunk::Float => return Ok((Self::Float, new_index)),
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

pub(crate) enum IntChunk {
    Ongoing(u64),
    Done(u64),
    Float,
}

impl IntChunk {
    fn parse(data: &[u8], index: usize) -> (Self, usize) {
        let (chunk, index) = decode_8(data, index);
        if let IntChunk::Ongoing(left_value) = chunk {
            let (chunk, new_index) = decode_8(data, index);
            match chunk {
                IntChunk::Ongoing(right_value) => {
                    let value = left_value * POW_10[new_index - index] + right_value;
                    (IntChunk::Ongoing(value), new_index)
                }
                IntChunk::Done(right_value) => {
                    let value = left_value * POW_10[new_index - index] + right_value;
                    (IntChunk::Done(value), new_index)
                }
                IntChunk::Float => (IntChunk::Float, new_index),
            }
        } else {
            (chunk, index)
        }
        // // TODO x86_64: use simd
        //
        // #[cfg(target_arch = "aarch64")]
        // {
        //     crate::simd_aarch64::decode_int_chunk(data, index)
        // }
        // #[cfg(not(target_arch = "aarch64"))]
        // {
        //     decode_int_chunk_fallback(data, index)
        // }
    }
}

// 11000000 * 8 - if either of the highest two bits are set, then the byte is not a digit
const MASK_HIGH: u64 = 0xc0c0c0c0c0c0c0c0;
// 00110000 * 8 - if either of the next two bits are NOT set, then the byte is not a digit
const MASK_LOW: u64 = 0x3030303030303030;

pub(crate) fn decode_8(data: &[u8], mut index: usize) -> (IntChunk, usize) {
    if let Some(digits) = data.get(index..index + 8) {
        let digits_number = u64::from_le_bytes(digits.try_into().unwrap());

        let mask_high = digits_number & MASK_HIGH;

        let inverted = !digits_number;
        let mask_low = inverted & MASK_LOW;

        let mask_either = mask_high | mask_low;
        if mask_either == 0 {
            (IntChunk::Ongoing(calc_8(digits_number)), index + 8)
        } else {
            let stop_byte_id = mask_either.trailing_zeros() / 8;
            let stop_byte = digits[stop_byte_id as usize];
            let return_index = index + stop_byte_id as usize;
            if matches!(stop_byte, b'.' | b'e' | b'E') {
                (IntChunk::Float, return_index)
            } else {
                let shifted = digits_number << ((8 - stop_byte_id) * 8);
                (IntChunk::Done(calc_8(shifted)), return_index)
            }
        }
    } else {
        decode_int_chunk_fallback(data, index)
    }
}

/// See https://rust-malaysia.github.io/code/2020/07/11/faster-integer-parsing.html#the-divide-and-conquer-insight
/// for explanation of the technique.
/// Assuming the number is `12345678`, the bytes are reversed as we look at them (because we're on LE),
/// so we have `87654321` - 8 the least significant digit is first.
/// `dbg!(format!("{eight_numbers:#018x}"));`
/// `eight_numbers = 0x38|37|36|35|34|33|32|31`
fn calc_8(raw: u64) -> u64 {
    // take 8, 6, 4, 2, apply mask to get their numeric values and shift them to the right by 1 byte
    let lower = (raw & 0x0f000f000f000f00) >> 8;
    // dbg!(format!("{lower:#018x}"));
    // lower = 0x00|08|00|06|00|04|00|02

    // take 7, 5, 3, 1, apply mask to get their numeric values and multiply them by 10
    let upper = (raw & 0x000f000f000f000f) * 10;
    // dbg!(format!("{upper:#018x}"));
    // upper = 0x46|00|32|00|1e|00|0a|00 = 0x46 is 70 - 7 * 10 ... 0x0a is 10 - 1 * 10
    let four_numbers = lower + upper;
    // dbg!(format!("{four_numbers:#018x}"), four_numbers.to_be_bytes());
    // four_numbers = 0x00|4e|00|38|00|22|00|0c = 0x4e is 78 - 70 + 8 ... we're turned 8 numbers into 8

    // take 78 and 34, apply mask to get their numeric values and shift them to the right by 2 bytes
    let lower = (four_numbers & 0x00ff000000ff0000) >> 16;
    // dbg!(format!("{lower:#018x}"), lower.to_be_bytes());
    // lower = 0x00|00|00|4e|00|00|00|22 - 0x4e is 78, 0x22 is 34

    // take 56 and 12, apply mask to get their numeric values and multiply them by 100
    let upper = (four_numbers & 0x000000ff000000ff) * 100;
    // dbg!(format!("{upper:#018x}"));
    // upper = 0x000015e0|000004b0 - 0x000015e0 is 5600, 0x000004b0 is 1200

    let two_numbers = lower + upper;
    // dbg!(format!("{two_numbers:#018x}"));
    // two_numbers = 0x0000162e|000004d2 - 0x0000162e is 5678, 0x000004d2 is 1234

    // take 5678, apply mask to get it's numeric values and shift it to the right by 4 bytes
    let lower = (two_numbers & 0x0000ffff00000000) >> 32;
    // dbg!(format!("{lower:#018x}"));
    // lower = 0x000000000000162e - in base 10 is 5678

    let upper = (two_numbers & 0x000000000000ffff) * 10000;
    // dbg!(format!("{upper:#018x}"));
    // upper = 0x0000000000bc4b20 - in base 10 is 1234_0000

    // combine to get the result!
    // we know this can't wrap around because we're only dealing with 8 digits
    lower.wrapping_add(upper)
}

pub(crate) fn decode_int_chunk_fallback(data: &[u8], mut index: usize) -> (IntChunk, usize) {
    let mut value = 0u64;
    // 8 to match decode_8
    for _ in 1..8 {
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

pub struct NumberRange;

impl AbstractNumberDecoder for NumberRange {
    type Output = Range<usize>;

    fn decode(data: &[u8], mut index: usize, first: u8, allow_inf_nan: bool) -> JsonResult<(Self::Output, usize)> {
        let start = index;

        let positive = match first {
            b'N' => {
                let (_, end) = consume_nan(data, index, allow_inf_nan)?;
                return Ok((start..end, end));
            }
            b'-' => false,
            _ => true,
        };
        if !positive {
            // we started with a minus sign, so the first digit is at index + 1
            index += 1;
        };

        match data.get(index) {
            Some(b'0') => {
                // numbers start with zero must be floats, next char must be a dot
                index += 1;
                return match data.get(index) {
                    Some(b'.') => {
                        index += 1;
                        let end = consume_decimal(data, index)?;
                        Ok((start..end, end))
                    }
                    Some(b'e') | Some(b'E') => {
                        index += 1;
                        let end = consume_exponential(data, index)?;
                        Ok((start..end, end))
                    }
                    Some(_) => return json_err!(InvalidNumber, index),
                    None => return Ok((start..index, index)),
                };
            }
            Some(b'I') => {
                let (_, end) = consume_inf(data, index, positive, allow_inf_nan)?;
                return Ok((start..end, end));
            }
            Some(digit) if (b'1'..=b'9').contains(digit) => (),
            Some(_) => return json_err!(InvalidNumber, index),
            None => return json_err!(EofWhileParsingValue, index),
        };

        index += 1;
        while let Some(next) = data.get(index) {
            match next {
                b'0'..=b'9' => (),
                b'.' => {
                    index += 1;
                    let end = consume_decimal(data, index)?;
                    return Ok((start..end, end));
                }
                b'e' | b'E' => {
                    index += 1;
                    let end = consume_exponential(data, index)?;
                    return Ok((start..end, end));
                }
                _ => break,
            }
            index += 1;
        }

        Ok((start..index, index))
    }
}

fn consume_exponential(data: &[u8], mut index: usize) -> JsonResult<usize> {
    match data.get(index) {
        Some(b'-') | Some(b'+') => {
            index += 1;
        }
        Some(v) if v.is_ascii_digit() => (),
        Some(_) => return json_err!(InvalidNumber, index),
        None => return json_err!(EofWhileParsingValue, index),
    };

    match data.get(index) {
        Some(v) if v.is_ascii_digit() => (),
        Some(_) => return json_err!(InvalidNumber, index),
        None => return json_err!(EofWhileParsingValue, index),
    };
    index += 1;

    while let Some(next) = data.get(index) {
        match next {
            b'0'..=b'9' => (),
            _ => break,
        }
        index += 1;
    }

    Ok(index)
}

fn consume_decimal(data: &[u8], mut index: usize) -> JsonResult<usize> {
    match data.get(index) {
        Some(v) if v.is_ascii_digit() => (),
        Some(_) => return json_err!(InvalidNumber, index),
        None => return json_err!(EofWhileParsingValue, index),
    };
    index += 1;

    while let Some(next) = data.get(index) {
        match next {
            b'0'..=b'9' => (),
            b'e' | b'E' => {
                index += 1;
                return consume_exponential(data, index);
            }
            _ => break,
        }
        index += 1;
    }

    Ok(index)
}
