use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;
use std::ops::Range;

use lexical_core::{format as lexical_format, parse_partial_with_options, ParseFloatOptions};

use crate::errors::{json_err, JsonResult};
use crate::int_parser::{IntParse, INT_CHAR_MAP};

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
