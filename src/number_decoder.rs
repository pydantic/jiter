use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;
use std::ops::Range;

use lexical_core::{format as lexical_format, parse_partial_with_options, Error as LexicalError, ParseFloatOptions};

use crate::errors::{json_err, JsonResult};

pub trait AbstractNumberDecoder {
    type Output;

    fn decode(data: &[u8], index: usize, first: u8) -> JsonResult<(Self::Output, usize)>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum NumberInt {
    Int(i64),
    BigInt(BigInt),
}

impl NumberInt {
    fn negate(self) -> Self {
        match self {
            Self::Int(int) => Self::Int(-int),
            Self::BigInt(big_int) => Self::BigInt(-big_int),
        }
    }
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

    fn decode(data: &[u8], index: usize, first: u8) -> JsonResult<(Self::Output, usize)> {
        let (int_parse, index) = IntParse::parse(data, index, first)?;
        match int_parse {
            IntParse::Int(positive, int) => {
                if positive {
                    Ok((int, index))
                } else {
                    Ok((int.negate(), index))
                }
            }
            IntParse::Float => json_err!(FloatExpectingInt, index),
        }
    }
}

pub struct NumberFloat;

impl AbstractNumberDecoder for NumberFloat {
    type Output = f64;

    fn decode(data: &[u8], index: usize, _first: u8) -> JsonResult<(Self::Output, usize)> {
        let start = index;
        const JSON: u128 = lexical_format::JSON;
        let options = ParseFloatOptions::new();
        match parse_partial_with_options::<f64, JSON>(&data[start..], &options) {
            Ok((float, index)) => Ok((float, index + start)),
            Err(e) => {
                // dbg!(e);
                match e {
                    LexicalError::EmptyExponent(_) => json_err!(EofWhileParsingValue, index),
                    _ => json_err!(InvalidNumber, index),
                }
            }
        }
    }
}

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

    fn decode(data: &[u8], index: usize, first: u8) -> JsonResult<(Self::Output, usize)> {
        let start = index;
        let (int_parse, index) = IntParse::parse(data, index, first)?;
        match int_parse {
            IntParse::Int(positive, int) => {
                if positive {
                    Ok((Self::Int(int), index))
                } else {
                    Ok((Self::Int(int.negate()), index))
                }
            }
            _ => NumberFloat::decode(data, start, first).map(|(f, index)| (Self::Float(f), index)),
        }
    }
}

#[derive(Debug)]
enum IntParse {
    Int(bool, NumberInt),
    Float,
}

impl IntParse {
    /// Turns out this is faster than fancy bit manipulation, see
    /// https://github.com/Alexhuszagh/rust-lexical/blob/main/lexical-parse-integer/docs/Algorithm.md
    /// for some context
    fn parse(data: &[u8], mut index: usize, first: u8) -> JsonResult<(Self, usize)> {
        let positive = first != b'-';
        if !positive {
            // we started with a minus sign, so the first digit is at index + 1
            index += 1;
        };
        let mut value = match data.get(index) {
            Some(b'0') => return Ok((Self::Float, index)),
            Some(digit) if (b'1'..=b'9').contains(digit) => (digit & 0x0f) as i64,
            Some(_) => return json_err!(InvalidNumber, index),
            None => return json_err!(EofWhileParsingValue, index),
        };
        // i64::MAX = 9223372036854775807 - 18 chars
        for _ in 1..18 {
            index += 1;
            match data.get(index) {
                Some(digit) if digit.is_ascii_digit() => {
                    // we use wrapping add to avoid branching - we know the value cannot wrap
                    value = value.wrapping_mul(10).wrapping_add((digit & 0x0f) as i64);
                }
                Some(b'.') => return Ok((Self::Float, index)),
                Some(b'e') | Some(b'E') => return Ok((Self::Float, index)),
                _ => return Ok((Self::Int(positive, NumberInt::Int(value)), index)),
            }
        }
        let mut big_value: BigInt = value.into();
        let mut length = 18;
        loop {
            value = 0;
            for pow in 0..18 {
                index += 1;
                match data.get(index) {
                    Some(digit) if digit.is_ascii_digit() => {
                        // we use wrapping add to avoid branching - we know the value cannot wrap
                        value = value.wrapping_mul(10).wrapping_add((digit & 0x0f) as i64);
                    }
                    Some(b'.') => return Ok((Self::Float, index)),
                    Some(b'e') | Some(b'E') => return Ok((Self::Float, index)),
                    _ => {
                        big_value *= 10u64.pow(pow as u32);
                        let big_int = NumberInt::BigInt(big_value + value);
                        return Ok((Self::Int(positive, big_int), index));
                    }
                }
            }
            length += 18;
            if length > 4300 {
                return json_err!(NumberOutOfRange, index);
            }
            big_value *= 10u64.pow(18);
            big_value += value;
        }
    }
}

pub struct NumberRange;

impl AbstractNumberDecoder for NumberRange {
    type Output = Range<usize>;

    fn decode(data: &[u8], mut index: usize, first: u8) -> JsonResult<(Self::Output, usize)> {
        let start = index;
        let positive = first != b'-';
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
                    _ => return Ok((start..index, index)),
                };
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
