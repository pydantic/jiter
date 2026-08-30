#[cfg(feature = "num-bigint")]
use num_bigint::BigInt;
#[cfg(feature = "num-bigint")]
use num_traits::cast::ToPrimitive;
#[cfg(feature = "python")]
use pyo3::{IntoPyObject, IntoPyObjectRef};

use std::ops::Range;

use lexical_parse_float::{
    FromLexicalWithOptions, Options as ParseFloatOptions,
    float::{RawFloat, extended_to_float},
    format as lexical_format,
    number::Number,
    parse::moderate_path,
};

use crate::{
    errors::{JsonError, JsonResult, json_err, json_error},
    simd::{decode_int_chunk_big, decode_int_chunk_small},
};

pub trait AbstractNumberDecoder: Sized {
    fn decode(data: &[u8], index: usize, first: u8, allow_inf_nan: bool) -> JsonResult<(Self, usize)>;
}

/// A number that can be either an [i64] or a [BigInt](num_bigint::BigInt)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "python", derive(IntoPyObject, IntoPyObjectRef))]
pub enum NumberInt {
    Int(i64),
    #[cfg(feature = "num-bigint")]
    BigInt(BigInt),
}

impl From<NumberInt> for f64 {
    fn from(num: NumberInt) -> Self {
        match num {
            NumberInt::Int(int) => int as f64,
            #[cfg(feature = "num-bigint")]
            NumberInt::BigInt(big_int) => big_int.to_f64().unwrap_or(f64::NAN),
        }
    }
}

impl TryFrom<&[u8]> for NumberInt {
    type Error = JsonError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(value)
    }
}

impl NumberInt {
    /// Parse `data` as a JSON integer, erroring if the input is not a valid integer,
    /// is empty, or contains trailing bytes.
    pub fn from_bytes(data: &[u8]) -> JsonResult<Self> {
        let first = *data.first().ok_or_else(|| json_error!(InvalidNumber, 0))?;
        let (int_parse, index) = IntParse::parse(data, 0, first)?;
        match int_parse {
            IntParse::Int(int) if index == data.len() => Ok(int),
            _ => json_err!(InvalidNumber, index),
        }
    }
}

impl AbstractNumberDecoder for NumberInt {
    fn decode(data: &[u8], index: usize, first: u8, _allow_inf_nan: bool) -> JsonResult<(Self, usize)> {
        let (int_parse, index) = IntParse::parse(data, index, first)?;
        match int_parse {
            IntParse::Int(int) => Ok((int, index)),
            _ => json_err!(FloatExpectingInt, index),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberFloat(pub f64);

impl From<NumberFloat> for f64 {
    fn from(num: NumberFloat) -> Self {
        num.0
    }
}

impl NumberFloat {
    /// Parse `data` as a JSON float, erroring if the input is empty or contains trailing bytes.
    pub fn from_bytes(data: &[u8], allow_inf_nan: bool) -> JsonResult<Self> {
        from_bytes_complete(data, allow_inf_nan)
    }
}

impl AbstractNumberDecoder for NumberFloat {
    fn decode(data: &[u8], mut index: usize, first: u8, allow_inf_nan: bool) -> JsonResult<(Self, usize)> {
        let start = index;

        let positive = match first {
            b'N' => {
                let (f, end) = consume_nan(data, index, allow_inf_nan)?;
                return Ok((Self(f), end));
            }
            b'-' => false,
            _ => true,
        };
        if !positive {
            // we started with a minus sign, so the first digit is at index + 1
            index += 1;
        }
        let first2 = if positive { Some(&first) } else { data.get(index) };

        if let Some(digit) = first2 {
            if INT_CHAR_MAP[*digit as usize] {
                let (float, next_index) = decode_float(data, start, index, allow_inf_nan)?;
                Ok((Self(float), next_index))
            } else if digit == &b'I' {
                let (f, end) = consume_inf_f64(data, index, positive, allow_inf_nan)?;
                Ok((Self(f), end))
            } else {
                json_err!(InvalidNumber, index)
            }
        } else {
            json_err!(EofWhileParsingValue, index)
        }
    }
}

/// Decode a float starting at `start`, on error uses `NumberRange::decode` to get the right error.
#[inline(always)]
fn parse_json_float(data: &[u8], start: usize, allow_inf_nan: bool) -> JsonResult<(f64, usize)> {
    let options = ParseFloatOptions::new();
    if let Ok((float, index)) = f64::from_lexical_partial_with_options::<JSON_FMT>(&data[start..], &options) {
        Ok((float, index + start))
    } else {
        float_error(data, start, allow_inf_nan)
    }
}

/// Decode a float starting at `start` whose first digit is at `int_start`, scanning the integer
/// part to try [`parse_float_dot`] before falling back to the general path.
fn decode_float(data: &[u8], start: usize, int_start: usize, allow_inf_nan: bool) -> JsonResult<(f64, usize)> {
    let (chunk, terminator_index) = decode_int_chunk_small(data, int_start, 0);
    if let IntChunk::Float(int_mantissa) = chunk
        // a `0`-prefixed integer part like `01.2` is invalid JSON, the general path rejects it
        && (data.get(int_start) != Some(&b'0') || terminator_index == int_start + 1)
        && let Some(result) = parse_float_dot(data, start, terminator_index, int_mantissa)
    {
        return Ok(result);
    }
    parse_json_float(data, start, allow_inf_nan)
}

/// Decode a float whose integer part [`IntParse`] already scanned.
/// `terminator_index` points at the `.`, `e` or `E` which ended the integer part.
///
/// Non-inlined so `NumberAny::decode`'s integer hot path stays small.
#[inline(never)]
fn decode_any_float(
    data: &[u8],
    start: usize,
    terminator_index: usize,
    int_mantissa: u64,
    allow_inf_nan: bool,
) -> JsonResult<(f64, usize)> {
    if let Some(result) = parse_float_dot(data, start, terminator_index, int_mantissa) {
        Ok(result)
    } else {
        parse_json_float(data, start, allow_inf_nan)
    }
}

/// It's impossible to work out the right error from LexicalError, so we parse again
/// with `NumberRange` and use that error.
#[cold]
#[inline(never)]
fn float_error(data: &[u8], start: usize, allow_inf_nan: bool) -> JsonResult<(f64, usize)> {
    let first = data.get(start).expect("float data to start within string");
    match NumberRange::decode(data, start, *first, allow_inf_nan) {
        Err(e) => Err(e),
        Ok(_) => {
            unreachable!("NumberRange should return an err if lexical-parse-float did")
        }
    }
}

const JSON_FMT: u128 = lexical_format::JSON;

const POW_10: [u64; 18] = [
    10u64.pow(0),
    10u64.pow(1),
    10u64.pow(2),
    10u64.pow(3),
    10u64.pow(4),
    10u64.pow(5),
    10u64.pow(6),
    10u64.pow(7),
    10u64.pow(8),
    10u64.pow(9),
    10u64.pow(10),
    10u64.pow(11),
    10u64.pow(12),
    10u64.pow(13),
    10u64.pow(14),
    10u64.pow(15),
    10u64.pow(16),
    10u64.pow(17),
];

/// Convert a float of the form `123.456` (no exponent) to an `f64`, `dot_index` points at the
/// byte which ended the integer part.
/// All digits are accumulated into a `u64` mantissa and converted with lexical's fast/moderate
/// (Eisel-Lemire) algorithms, so the result is identical to a full lexical parse.
/// Returns `None` when the number needs the general parsing path: an exponent instead of a dot,
/// more significant digits than a `u64` holds exactly, or when lexical would need its slow
/// algorithm.
fn parse_float_dot(data: &[u8], start: usize, dot_index: usize, int_mantissa: u64) -> Option<(f64, usize)> {
    if data.get(dot_index) != Some(&b'.') {
        return None;
    }
    let frac_start = dot_index + 1;
    let long_frac = next_8_are_digits(data, frac_start);
    if long_frac && next_8_are_digits(data, frac_start + 8) {
        // 16 or more fraction digits: too many for an exact u64 mantissa
        return None;
    }

    let is_negative = data.get(start) == Some(&b'-');
    let int_start = start + usize::from(is_negative);
    let int_digits = dot_index - int_start;
    if int_digits > 18 {
        // `int_mantissa` only covers 18 digits, and the mantissa couldn't be exact anyway
        return None;
    }
    // leading zeros are invalid JSON, so a zero integer part is exactly one digit contributing
    // no significant digits
    let int_digits = if int_mantissa == 0 { 0 } else { int_digits };

    let (frac_value, end) = if long_frac && data.len() >= frac_start + 16 {
        parse_long_fraction(data, frac_start)
    } else {
        // short fraction, or a long one at the very end of the data
        let (chunk, end) = decode_int_chunk_small(data, frac_start, 0);
        match chunk {
            IntChunk::Done(value) | IntChunk::Float(value) => (value, end),
            IntChunk::Ongoing(_) => return None,
        }
    };
    // an exponent needs the general path; any other terminator (e.g. the second dot of
    // `1.2.3`) just ends the number
    if matches!(data.get(end), Some(b'e' | b'E')) {
        return None;
    }
    let frac_digits = end - frac_start;
    // any 19-digit decimal fits in a u64, so up to 19 significant digits the mantissa is exact;
    // `frac_digits == 0` (e.g. `123.`) is invalid and left to the general path to error
    if frac_digits == 0 || int_digits + frac_digits > 19 {
        return None;
    }
    let mantissa = int_mantissa * POW_10[frac_digits] + frac_value;

    let num = Number {
        exponent: -(frac_digits as i64),
        mantissa,
        is_negative,
        many_digits: false,
        // digit slices are only used by lexical's slow path, which we never invoke
        integer: &[],
        fraction: None,
    };
    if mantissa <= f64::MAX_MANTISSA_FAST_PATH
        && let Some(value) = num.try_fast_path::<f64, JSON_FMT>()
    {
        return Some((value, end));
    }
    let fp = moderate_path::<f64, JSON_FMT>(&num, false);
    if fp.exp < 0 {
        return None;
    }
    let mut float = extended_to_float::<f64>(fp);
    if is_negative {
        float = -float;
    }
    Some((float, end))
}

/// Decode a fraction of 8-15 digits at `frac_start`, returning its value and the index of the
/// byte after the last digit.
///
/// The caller has established that the 8 bytes at `frac_start` are digits, that the 8 after
/// them are within `data` but not all digits, and discards the value if the total significant
/// digits exceed an exact u64 mantissa. Pure SWAR, so unlike the SIMD chunk decoder the
/// returned index doesn't wait on a vector->GPR transfer.
fn parse_long_fraction(data: &[u8], frac_start: usize) -> (u64, usize) {
    let first8 = u64::from_le_bytes(data[frac_start..frac_start + 8].try_into().unwrap());
    let second8 = u64::from_le_bytes(data[frac_start + 8..frac_start + 16].try_into().unwrap());
    // the same digit test as `next_8_are_digits`, kept per byte: a non-zero high nibble marks
    // a non-digit byte
    let x = second8 ^ 0x3030_3030_3030_3030;
    let non_digit_mask = (x.wrapping_add(0x0606_0606_0606_0606) | x) & 0xF0F0_F0F0_F0F0_F0F0;
    debug_assert!(
        non_digit_mask != 0,
        "caller must establish a non-digit in the second 8 bytes"
    );
    let extra_digits = (non_digit_mask.trailing_zeros() / 8) as usize;
    let value8 = parse_8digits(first8);
    let value = if extra_digits == 0 {
        value8
    } else {
        // move the extra digits to the top bytes and fill below them with ascii zeros, so the
        // 8-digit parse yields exactly their value
        let padded = (second8 << (8 * (8 - extra_digits))) | (0x3030_3030_3030_3030 >> (8 * extra_digits));
        value8 * POW_10[extra_digits] + parse_8digits(padded)
    };
    (value, frac_start + 8 + extra_digits)
}

/// Parse 8 ASCII digit bytes, in memory order, as a `u64`. The classic SWAR reduction, see
/// <https://github.com/simdjson/simdjson/blob/master/src/generic/numberparsing.h>.
fn parse_8digits(value: u64) -> u64 {
    const MASK: u64 = 0x0000_00FF_0000_00FF;
    const MUL1: u64 = 0x000F_4240_0000_0064;
    const MUL2: u64 = 0x0000_2710_0000_0001;
    let value = value.wrapping_sub(0x3030_3030_3030_3030);
    let value = value.wrapping_mul(2561) >> 8;
    (value & MASK)
        .wrapping_mul(MUL1)
        .wrapping_add(((value >> 16) & MASK).wrapping_mul(MUL2))
        >> 32
}

/// SWAR check whether the 8 bytes at `index` are all ASCII digits.
/// Returns `false` when fewer than 8 bytes remain.
fn next_8_are_digits(data: &[u8], index: usize) -> bool {
    if let Some(chunk) = data.get(index..index + 8) {
        let value = u64::from_le_bytes(chunk.try_into().unwrap());
        let x = value ^ 0x3030_3030_3030_3030;
        (x.wrapping_add(0x0606_0606_0606_0606) | x) & 0xF0F0_F0F0_F0F0_F0F0 == 0
    } else {
        false
    }
}

/// A number that can be either a [NumberInt] or an [f64]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "python", derive(IntoPyObject, IntoPyObjectRef))]
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

impl NumberAny {
    /// Parse `data` as a JSON number, erroring if the input is empty or contains trailing bytes.
    pub fn from_bytes(data: &[u8], allow_inf_nan: bool) -> JsonResult<Self> {
        from_bytes_complete(data, allow_inf_nan)
    }
}

impl AbstractNumberDecoder for NumberAny {
    fn decode(data: &[u8], index: usize, first: u8, allow_inf_nan: bool) -> JsonResult<(Self, usize)> {
        let start = index;
        let (int_parse, index) = IntParse::parse(data, index, first)?;
        match int_parse {
            IntParse::Int(int) => Ok((Self::Int(int), index)),
            IntParse::Float(int_mantissa) => {
                let (float, next_index) = decode_any_float(data, start, index, int_mantissa, allow_inf_nan)?;
                Ok((Self::Float(float), next_index))
            }
            IntParse::FloatInf(positive) => {
                consume_inf_f64(data, index, positive, allow_inf_nan).map(|(f, index)| (Self::Float(f), index))
            }
            IntParse::FloatNaN => consume_nan(data, index, allow_inf_nan).map(|(f, index)| (Self::Float(f), index)),
        }
    }
}

fn from_bytes_complete<D: AbstractNumberDecoder>(data: &[u8], allow_inf_nan: bool) -> JsonResult<D> {
    let first = *data.first().ok_or_else(|| json_error!(InvalidNumber, 0))?;
    let (output, index) = D::decode(data, 0, first, allow_inf_nan)?;
    if index == data.len() {
        Ok(output)
    } else {
        json_err!(InvalidNumber, index)
    }
}

fn consume_inf(data: &[u8], index: usize, positive: bool, allow_inf_nan: bool) -> JsonResult<usize> {
    if allow_inf_nan {
        crate::parse::consume_infinity(data, index)
    } else if positive {
        json_err!(ExpectedSomeValue, index)
    } else {
        json_err!(InvalidNumber, index)
    }
}

fn consume_inf_f64(data: &[u8], index: usize, positive: bool, allow_inf_nan: bool) -> JsonResult<(f64, usize)> {
    let end = consume_inf(data, index, positive, allow_inf_nan)?;
    if positive {
        Ok((f64::INFINITY, end))
    } else {
        Ok((f64::NEG_INFINITY, end))
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
    /// carries the integer part's value so [`parse_float_dot`] doesn't rescan it; only valid
    /// while the integer part has 18 digits or fewer, which `parse_float_dot` checks
    Float(u64),
    FloatInf(bool),
    FloatNaN,
}

impl IntParse {
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
        }
        let first2 = if positive { Some(&first) } else { data.get(index) };
        let first_value = match first2 {
            Some(b'0') => {
                index += 1;
                return match data.get(index) {
                    Some(b'.') => Ok((Self::Float(0), index)),
                    Some(b'e' | b'E') => Ok((Self::Float(0), index)),
                    Some(digit) if digit.is_ascii_digit() => json_err!(InvalidNumber, index),
                    _ => Ok((Self::Int(NumberInt::Int(0)), index)),
                };
            }
            Some(b'I') => return Ok((Self::FloatInf(positive), index)),
            Some(digit) if (b'1'..=b'9').contains(digit) => (digit & 0x0f) as u64,
            Some(_) => return json_err!(InvalidNumber, index),
            None => return json_err!(EofWhileParsingValue, index),
        };

        index += 1;
        let (chunk, new_index) = decode_int_chunk_small(data, index, first_value);

        let ongoing: u64 = match chunk {
            IntChunk::Ongoing(value) => value,
            IntChunk::Done(value) => {
                let mut value_i64 = value as i64;
                if !positive {
                    value_i64 = -value_i64;
                }
                return Ok((Self::Int(NumberInt::Int(value_i64)), new_index));
            }
            IntChunk::Float(value) => return Ok((Self::Float(value), new_index)),
        };

        // number is too big for i64, we need to use a BigInt,
        // or error out if num-bigint is not enabled

        #[cfg(not(feature = "num-bigint"))]
        {
            // silence unused variable warning
            let _ = (ongoing, start);
            return json_err!(NumberOutOfRange, index);
        }

        #[cfg(feature = "num-bigint")]
        {
            use crate::simd::ONGOING_CHUNK_MULTIPLIER;

            let mut big_value: BigInt = ongoing.into();
            index = new_index;

            loop {
                let (chunk, new_index) = decode_int_chunk_big(data, index);
                if (new_index - start) > 4300 {
                    return json_err!(NumberOutOfRange, start + 4301);
                }
                match chunk {
                    IntChunk::Ongoing(value) => {
                        big_value *= ONGOING_CHUNK_MULTIPLIER;
                        big_value += value;
                        index = new_index;
                    }
                    IntChunk::Done(value) => {
                        big_value *= POW_10[new_index - index];
                        big_value += value;
                        if !positive {
                            big_value = -big_value;
                        }
                        return Ok((Self::Int(NumberInt::BigInt(big_value)), new_index));
                    }
                    IntChunk::Float(_) => return Ok((Self::Float(0), new_index)),
                }
            }
        }
    }
}

pub(crate) enum IntChunk {
    Ongoing(u64),
    Done(u64),
    /// number ended with a dot or exponent at the returned index; carries the value of
    /// the digits so far, used by [`parse_float_dot`]
    Float(u64),
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

pub struct NumberRange {
    pub range: Range<usize>,
    // in some cfg configurations, this field is never read.
    #[allow(dead_code)]
    pub is_int: bool,
}

impl NumberRange {
    fn int(data: Range<usize>) -> Self {
        Self {
            range: data,
            is_int: true,
        }
    }

    fn float(data: Range<usize>) -> Self {
        Self {
            range: data,
            is_int: false,
        }
    }
}

impl AbstractNumberDecoder for NumberRange {
    fn decode(data: &[u8], mut index: usize, first: u8, allow_inf_nan: bool) -> JsonResult<(Self, usize)> {
        let start = index;

        let positive = match first {
            b'N' => {
                let (_, end) = consume_nan(data, index, allow_inf_nan)?;
                return Ok((Self::float(start..end), end));
            }
            b'-' => false,
            _ => true,
        };
        if !positive {
            // we started with a minus sign, so the first digit is at index + 1
            index += 1;
        }

        match data.get(index) {
            Some(b'0') => {
                // numbers start with zero must be floats, next char must be a dot
                index += 1;
                return match data.get(index) {
                    Some(b'.') => {
                        index += 1;
                        let end = consume_decimal(data, index)?;
                        Ok((Self::float(start..end), end))
                    }
                    Some(b'e' | b'E') => {
                        index += 1;
                        let end = consume_exponential(data, index)?;
                        Ok((Self::float(start..end), end))
                    }
                    Some(digit) if digit.is_ascii_digit() => json_err!(InvalidNumber, index),
                    _ => return Ok((Self::int(start..index), index)),
                };
            }
            Some(b'I') => {
                let end = consume_inf(data, index, positive, allow_inf_nan)?;
                return Ok((Self::float(start..end), end));
            }
            Some(digit) if (b'1'..=b'9').contains(digit) => (),
            Some(_) => return json_err!(InvalidNumber, index),
            None => return json_err!(EofWhileParsingValue, index),
        }

        index += 1;
        for _ in 0..18 {
            if let Some(digit) = data.get(index) {
                if INT_CHAR_MAP[*digit as usize] {
                    index += 1;
                    continue;
                } else if matches!(digit, b'.') {
                    index += 1;
                    let end = consume_decimal(data, index)?;
                    return Ok((Self::float(start..end), end));
                } else if matches!(digit, b'e' | b'E') {
                    index += 1;
                    let end = consume_exponential(data, index)?;
                    return Ok((Self::float(start..end), end));
                }
            }
            return Ok((Self::int(start..index), index));
        }
        loop {
            let (chunk, new_index) = decode_int_chunk_big(data, index);
            if (new_index - start) > 4300 {
                return json_err!(NumberOutOfRange, start + 4301);
            }
            #[allow(clippy::single_match_else)]
            match chunk {
                IntChunk::Ongoing(_) => {
                    index = new_index;
                }
                IntChunk::Done(_) => return Ok((Self::int(start..new_index), new_index)),
                IntChunk::Float(_) => {
                    return match data.get(new_index) {
                        Some(b'.') => {
                            index = new_index + 1;
                            let end = consume_decimal(data, index)?;
                            Ok((Self::float(start..end), end))
                        }
                        _ => {
                            index = new_index + 1;
                            let end = consume_exponential(data, index)?;
                            Ok((Self::float(start..end), end))
                        }
                    };
                }
            }
        }
    }
}

fn consume_exponential(data: &[u8], mut index: usize) -> JsonResult<usize> {
    match data.get(index) {
        Some(b'-' | b'+') => {
            index += 1;
        }
        Some(v) if v.is_ascii_digit() => (),
        Some(_) => return json_err!(InvalidNumber, index),
        None => return json_err!(EofWhileParsingValue, index),
    }

    match data.get(index) {
        Some(v) if v.is_ascii_digit() => (),
        Some(_) => return json_err!(InvalidNumber, index),
        None => return json_err!(EofWhileParsingValue, index),
    }
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
    }
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
