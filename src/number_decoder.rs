use std::ops::Range;
use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;

use lexical_parse_float::{format as lexical_format, FromLexicalWithOptions, Options as ParseFloatOptions};

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
                match f64::from_lexical_partial_with_options::<JSON>(&data[start..], &options) {
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
        // TODO x86_64: use simd

        #[cfg(target_arch = "aarch64")]
        {
            crate::simd_aarch64::decode_int_chunk(data, index)
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            decode_int_chunk_fallback(data, index)
        }
    }
}

pub(crate) fn decode_int_chunk_fallback(data: &[u8], mut index: usize) -> (IntChunk, usize) {
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
