#[cfg(feature = "num-bigint")]
use num_bigint::BigInt;
#[cfg(feature = "num-bigint")]
use num_traits::cast::ToPrimitive;
#[cfg(feature = "python")]
use pyo3::{IntoPyObject, IntoPyObjectRef};

use std::ops::Range;

use lexical_parse_float::{FromLexicalWithOptions, Options as ParseFloatOptions, format as lexical_format};

#[cfg(feature = "num-bigint")]
use crate::simd::decode_int_chunk_big;
use crate::{
    JsonErrorType::FloatExpectingInt,
    errors::{JsonError, JsonResult, json_err, json_error},
    simd::{NumberChunk, decode_int_chunk_small, decode_number_chunk, find_digit_run_end},
};
use lexical_format::JSON;

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
        match NumberInt::parse(data, 0, first) {
            Ok((int, index)) if index == data.len() => Ok(int),
            Ok((_, index))
            | Err(JsonError {
                error_type: FloatExpectingInt,
                index,
            }) => json_err!(InvalidNumber, index),
            Err(other) => Err(other),
        }
    }
}

impl AbstractNumberDecoder for NumberInt {
    fn decode(data: &[u8], index: usize, first: u8, _allow_inf_nan: bool) -> JsonResult<(Self, usize)> {
        Self::parse(data, index, first)
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
    fn decode(data: &[u8], mut index: usize, mut first: u8, allow_inf_nan: bool) -> JsonResult<(Self, usize)> {
        let start = index;

        let positive = match first {
            b'N' => {
                let (f, end) = consume_nan(data, index, allow_inf_nan)?;
                return Ok((Self(f), end));
            }
            b'-' => {
                index += 1;
                first = *data
                    .get(index)
                    .ok_or_else(|| json_error!(EofWhileParsingValue, index))?;
                false
            }
            _ => true,
        };

        match first {
            b'0'..=b'9' => parse_json_float(data, start, allow_inf_nan).map(|(float, end)| (Self(float), end)),
            b'I' => {
                let (f, end) = consume_inf_f64(data, index, positive, allow_inf_nan)?;
                Ok((Self(f), end))
            }
            _ => json_err!(InvalidNumber, index),
        }
    }
}

/// Parse a JSON float prefix, preserving jiter's error kinds and positions.
#[inline(always)]
fn parse_json_float(data: &[u8], start: usize, allow_inf_nan: bool) -> JsonResult<(f64, usize)> {
    let options = ParseFloatOptions::new();
    if let Ok((float, index)) = f64::from_lexical_partial_with_options::<JSON>(&data[start..], &options) {
        Ok((float, index + start))
    } else {
        float_error(data, start, allow_inf_nan)
    }
}

/// Recover a precise JSON error when lexical cannot parse a float.
#[cold]
#[inline(never)]
fn float_error(data: &[u8], start: usize, allow_inf_nan: bool) -> JsonResult<(f64, usize)> {
    let first = *data.get(start).expect("float_error called with empty slice");
    match NumberRange::decode(data, start, first, allow_inf_nan) {
        Err(e) => Err(e),
        Ok(_) => unreachable!("NumberRange should return an error if lexical-parse-float did"),
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
    /// Decode integers or dispatch floats to lexical's public parser without constructing a
    /// speculative bigint.
    fn decode(data: &[u8], mut index: usize, mut first: u8, allow_inf_nan: bool) -> JsonResult<(Self, usize)> {
        let start = index;
        let positive = match first {
            b'N' => {
                return consume_nan(data, index, allow_inf_nan).map(|(float, end)| (Self::Float(float), end));
            }
            b'-' => {
                index += 1;
                first = *data
                    .get(index)
                    .ok_or_else(|| json_error!(EofWhileParsingValue, index))?;
                false
            }
            _ => true,
        };

        let digit_start = index;
        let scan_digits = match first {
            b'I' => {
                return consume_inf_f64(data, index, positive, allow_inf_nan)
                    .map(|(float, end)| (Self::Float(float), end));
            }
            b'0' => {
                index += 1;
                match data.get(index) {
                    Some(digit) if digit.is_ascii_digit() => return json_err!(InvalidNumber, index),
                    Some(b'.' | b'e' | b'E') => false,
                    _ => return Ok((Self::Int(NumberInt::Int(0)), index)),
                }
            }
            b'1'..=b'9' => match decode_number_chunk(data, digit_start) {
                (NumberChunk::Int(magnitude), end) => {
                    let int = if let Some(value) = signed_integer(magnitude, positive) {
                        NumberInt::Int(value)
                    } else {
                        decode_bigint(data, digit_start, end, positive, Some(magnitude))?
                    };
                    return Ok((Self::Int(int), end));
                }
                (NumberChunk::Float, _) => false,
                (NumberChunk::Ongoing, end) => {
                    index = end;
                    true
                }
            },
            _ => return json_err!(InvalidNumber, index),
        };

        if scan_digits {
            let limit = digit_start.saturating_add(4300);
            index = find_digit_run_end(data, index, limit)
                .ok_or_else(|| json_error!(NumberOutOfRange, digit_start + 4301))?;
            if !matches!(data.get(index), Some(b'.' | b'e' | b'E')) {
                let int = decode_bigint(data, digit_start, index, positive, None)?;
                return Ok((Self::Int(int), index));
            }
        }

        parse_json_float(data, start, allow_inf_nan).map(|(float, end)| (Self::Float(float), end))
    }
}

fn signed_integer(magnitude: u64, positive: bool) -> Option<i64> {
    let max_magnitude = if positive { i64::MAX as u64 } else { i64::MAX as u64 + 1 };
    if magnitude > max_magnitude {
        return None;
    }
    Some(if positive {
        magnitude as i64
    } else {
        (magnitude as i64).wrapping_neg()
    })
}

/// Convert a validated integer outside the signed i64 range, optionally using its decoded magnitude.
#[cfg_attr(
    feature = "num-bigint",
    allow(clippy::unnecessary_wraps, reason = "conversion can fail without num-bigint")
)]
fn decode_bigint(
    data: &[u8],
    digit_start: usize,
    end: usize,
    positive: bool,
    magnitude: Option<u64>,
) -> JsonResult<NumberInt> {
    #[cfg(not(feature = "num-bigint"))]
    {
        let _ = (data, end, positive, magnitude);
        json_err!(NumberOutOfRange, digit_start + 1)
    }

    #[cfg(feature = "num-bigint")]
    {
        let (mut value, mut index) = if let Some(magnitude) = magnitude {
            (BigInt::from(magnitude), end)
        } else {
            let first_chunk_len = (end - digit_start - 1) % 16 + 1;
            let first_chunk_end = digit_start + first_chunk_len;
            let first_chunk = data[digit_start..first_chunk_end]
                .iter()
                .fold(0u64, |value, digit| value * 10 + u64::from(digit & 0x0f));
            (BigInt::from(first_chunk), first_chunk_end)
        };
        while index < end {
            let (chunk, new_index) = decode_int_chunk_big(data, index);
            let (chunk, multiplier) = match chunk {
                IntChunk::Ongoing(value) => (value, crate::simd::ONGOING_CHUNK_MULTIPLIER),
                IntChunk::Done(value) => (value, 10u64.pow((new_index - index) as u32)),
                IntChunk::Float => unreachable!("known integer digit run to contain a float marker"),
            };
            value *= multiplier;
            value += chunk;
            index = new_index;
        }
        if !positive {
            value = -value;
        }
        Ok(NumberInt::BigInt(value))
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

impl NumberInt {
    fn parse(data: &[u8], mut index: usize, mut first: u8) -> JsonResult<(Self, usize)> {
        let start = index;
        let positive = match first {
            b'N' => return json_err!(FloatExpectingInt, index),
            b'-' => {
                index += 1;
                first = *data
                    .get(index)
                    .ok_or_else(|| json_error!(EofWhileParsingValue, index))?;
                false
            }
            _ => true,
        };
        let first_value = match first {
            b'0' => {
                index += 1;
                return match data.get(index) {
                    Some(b'.') => json_err!(FloatExpectingInt, index),
                    Some(b'e' | b'E') => json_err!(FloatExpectingInt, index),
                    Some(digit) if digit.is_ascii_digit() => json_err!(InvalidNumber, index),
                    _ => Ok((NumberInt::Int(0), index)),
                };
            }
            b'I' => return json_err!(FloatExpectingInt, index),
            digit @ b'1'..=b'9' => (digit & 0x0f) as u64,
            _ => return json_err!(InvalidNumber, index),
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
                return Ok((NumberInt::Int(value_i64), new_index));
            }
            IntChunk::Float => return json_err!(FloatExpectingInt, new_index),
        };

        // number is too big for i64, we need to use a BigInt,
        // or error out if num-bigint is not enabled

        #[cfg(not(feature = "num-bigint"))]
        {
            // silence unused variable warning
            let _ = (ongoing, start);
            json_err!(NumberOutOfRange, index)
        }

        #[cfg(feature = "num-bigint")]
        {
            use crate::simd::ONGOING_CHUNK_MULTIPLIER;

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

            let mut big_value: BigInt = ongoing.into();
            index = new_index;

            let digit_start = start + usize::from(!positive);
            loop {
                let (chunk, new_index) = decode_int_chunk_big(data, index);
                if (new_index - digit_start) > 4300 {
                    return json_err!(NumberOutOfRange, digit_start + 4301);
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
                        return Ok((NumberInt::BigInt(big_value), new_index));
                    }
                    IntChunk::Float => return json_err!(FloatExpectingInt, new_index),
                }
            }
        }
    }
}

pub(crate) enum IntChunk {
    Ongoing(u64),
    Done(u64),
    Float,
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
    fn decode(data: &[u8], mut index: usize, mut first: u8, allow_inf_nan: bool) -> JsonResult<(Self, usize)> {
        let start = index;

        let positive = match first {
            b'N' => {
                let (_, end) = consume_nan(data, index, allow_inf_nan)?;
                return Ok((Self::float(start..end), end));
            }
            b'-' => {
                index += 1;
                first = *data
                    .get(index)
                    .ok_or_else(|| json_error!(EofWhileParsingValue, index))?;
                false
            }
            _ => true,
        };

        match first {
            b'0' => {
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
            b'I' => {
                let end = consume_inf(data, index, positive, allow_inf_nan)?;
                return Ok((Self::float(start..end), end));
            }
            b'1'..=b'9' => (),
            _ => return json_err!(InvalidNumber, index),
        }

        // `first` is the leading significant digit. Only the extent of the integer run is
        // needed here, not its value, so find its end and classify the byte that ends it.
        // The scan is bounded so an over-long integer is rejected without walking it in full.
        index += 1;
        let digit_start = start + usize::from(!positive);
        index = digit_run_end(data, index, digit_start + 4300)
            .ok_or_else(|| json_error!(NumberOutOfRange, digit_start + 4301))?;
        match data.get(index) {
            Some(b'.') => {
                let end = consume_decimal(data, index + 1)?;
                Ok((Self::float(start..end), end))
            }
            Some(b'e' | b'E') => {
                let end = consume_exponential(data, index + 1)?;
                Ok((Self::float(start..end), end))
            }
            _ => Ok((Self::int(start..index), index)),
        }
    }
}

/// How many digits of a run are taken one byte at a time before switching to the wide scan.
///
/// The scan's result sits on the parser's critical path: it waits on where the number ends.
/// On x86_64 that result is a `pmovmskb` and a `tzcnt` away, cheaper than any byte loop, so
/// every run goes straight to the scan: measured on a Xeon, a prefix of 4 to 16 digits costs
/// 20 to 40% more cycles on arrays of floats and long integers and gains nothing on short
/// numbers. On aarch64 the mask must first cross from a vector to a general register, and that
/// latency loses to a short byte loop: without a prefix, short numbers are 10 to 90% slower on
/// an M-series core, while 4 to 16 digits are within noise of each other. Eight keeps the
/// common shapes on the byte loop there.
#[cfg(not(target_arch = "x86_64"))]
const SCALAR_DIGITS: usize = 8;

/// The end of the run of ASCII digits starting at `index`: the first `SCALAR_DIGITS` one byte
/// at a time where that pays, the rest with [`find_digit_run_end`], whose contract this shares:
/// `None` if the run reaches `limit`, which must lie at least `SCALAR_DIGITS` bytes past `index`
/// or at the end of `data`.
#[inline(always)]
fn digit_run_end(data: &[u8], index: usize, limit: usize) -> Option<usize> {
    #[cfg(not(target_arch = "x86_64"))]
    let index = {
        let mut index = index;
        for _ in 0..SCALAR_DIGITS {
            match data.get(index) {
                Some(digit) if digit.is_ascii_digit() => index += 1,
                _ => return Some(index),
            }
        }
        index
    };
    find_digit_run_end(data, index, limit)
}

/// The index just past the run of ASCII digits starting at `index`, or `data.len()`.
#[inline(always)]
fn digit_run_end_unbounded(data: &[u8], index: usize) -> usize {
    // a run that reaches the limit, the end of the data, ends there
    digit_run_end(data, index, data.len()).unwrap_or(data.len())
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

    // at least one exponent digit is required
    match data.get(index) {
        Some(v) if v.is_ascii_digit() => (),
        Some(_) => return json_err!(InvalidNumber, index),
        None => return json_err!(EofWhileParsingValue, index),
    }

    Ok(digit_run_end_unbounded(data, index + 1))
}

fn consume_decimal(data: &[u8], index: usize) -> JsonResult<usize> {
    // at least one fractional digit is required
    match data.get(index) {
        Some(v) if v.is_ascii_digit() => (),
        Some(_) => return json_err!(InvalidNumber, index),
        None => return json_err!(EofWhileParsingValue, index),
    }

    let index = digit_run_end_unbounded(data, index + 1);
    match data.get(index) {
        Some(b'e' | b'E') => consume_exponential(data, index + 1),
        _ => Ok(index),
    }
}

#[cfg(test)]
mod tests {
    use super::{AbstractNumberDecoder, NumberAny, NumberRange, consume_decimal, consume_exponential};
    use crate::errors::{JsonErrorType, JsonResult, json_err};
    use proptest::prelude::*;

    fn norm(r: JsonResult<usize>) -> Result<usize, (JsonErrorType, usize)> {
        r.map_err(|e| (e.error_type, e.index))
    }

    /// Byte-at-a-time reference: `consume_exponential` as it was before the digit-run scan.
    fn consume_exponential_scalar(data: &[u8], mut index: usize) -> JsonResult<usize> {
        match data.get(index) {
            Some(b'-' | b'+') => index += 1,
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
        while let Some(b'0'..=b'9') = data.get(index) {
            index += 1;
        }
        Ok(index)
    }

    /// Byte-at-a-time reference: `consume_decimal` as it was before the digit-run scan.
    fn consume_decimal_scalar(data: &[u8], mut index: usize) -> JsonResult<usize> {
        match data.get(index) {
            Some(v) if v.is_ascii_digit() => (),
            Some(_) => return json_err!(InvalidNumber, index),
            None => return json_err!(EofWhileParsingValue, index),
        }
        index += 1;
        while let Some(next) = data.get(index) {
            match next {
                b'0'..=b'9' => index += 1,
                b'e' | b'E' => return consume_exponential_scalar(data, index + 1),
                _ => break,
            }
        }
        Ok(index)
    }

    /// Bytes biased towards digits and number punctuation, long enough to cross the
    /// 16-byte SIMD chunks the digit-run scan works in.
    fn number_soup() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(
            prop_oneof![
                12 => b'0'..=b'9',
                1 => Just(b'e'),
                1 => Just(b'E'),
                1 => Just(b'+'),
                1 => Just(b'-'),
                1 => Just(b'.'),
                1 => Just(b','),
                1 => any::<u8>(),
            ],
            0..48,
        )
    }

    /// `NumberRange::decode` reduced to `(range, end)` on success and `(kind, index)` on error.
    type RangeResult = Result<(std::ops::Range<usize>, usize), (JsonErrorType, usize)>;

    fn number_range(data: &[u8]) -> RangeResult {
        match NumberRange::decode(data, 0, data[0], false) {
            Ok((number, end)) => Ok((number.range, end)),
            Err(e) => Err((e.error_type, e.index)),
        }
    }

    /// The integer-part digit limit applies to floats too, and is checked before the
    /// fraction is looked at.
    #[test]
    fn float_integer_part_digit_limit() {
        let ok = format!("{}.5", "1".repeat(4300));
        assert_eq!(number_range(ok.as_bytes()), Ok((0..ok.len(), ok.len())));

        let too_long = format!("{}.5", "1".repeat(4301));
        assert_eq!(
            number_range(too_long.as_bytes()),
            Err((JsonErrorType::NumberOutOfRange, 4301))
        );

        let negative = format!("-{}.5", "1".repeat(4301));
        assert_eq!(
            number_range(negative.as_bytes()),
            Err((JsonErrorType::NumberOutOfRange, 4302))
        );
    }

    /// Runs that end just before, at, and after the scalar prefix and the 16-byte SIMD
    /// chunk must all be measured exactly, in every position a run can occur.
    #[test]
    fn digit_runs_around_the_scan_boundaries() {
        for len in [1, 7, 8, 9, 15, 16, 17, 23, 24, 25, 40] {
            let digits = "7".repeat(len);
            for number in [
                digits.clone(),
                format!("-{digits}"),
                format!("1.{digits}"),
                format!("1.5e{digits}"),
                format!("{digits}.{digits}e-{digits}"),
            ] {
                for trail in ["", ",", "]", " "] {
                    let json = format!("{number}{trail}");
                    assert_eq!(
                        number_range(json.as_bytes()),
                        Ok((0..number.len(), number.len())),
                        "{json:?}"
                    );
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2048))]

        /// The digit consumers must match the scalar loops they replaced on arbitrary input:
        /// the same end index on success, the same error kind and position otherwise.
        #[test]
        fn consume_decimal_matches_scalar(data in number_soup(), start in 0usize..4) {
            let start = start.min(data.len());
            prop_assert_eq!(norm(consume_decimal(&data, start)), norm(consume_decimal_scalar(&data, start)));
        }

        #[test]
        fn consume_exponential_matches_scalar(data in number_soup(), start in 0usize..4) {
            let start = start.min(data.len());
            prop_assert_eq!(norm(consume_exponential(&data, start)), norm(consume_exponential_scalar(&data, start)));
        }

        /// `NumberRange` must cover exactly the number and stop where `NumberAny` stops, with
        /// digit runs long enough to cross the 16-byte SIMD chunks.
        #[test]
        fn number_range_covers_number(
            number in r"-?(0|[1-9][0-9]{0,40})(\.[0-9]{1,40})?([eE][+-]?[0-9]{1,20})?",
            trail in prop::sample::select(vec!["", ",", "]", "}", " ", "x"]),
        ) {
            let json = format!("{number}{trail}");
            let data = json.as_bytes();
            prop_assert_eq!(number_range(data), Ok((0..number.len(), number.len())));
            match NumberAny::decode(data, 0, data[0], false) {
                Ok((_, any_end)) => prop_assert_eq!(any_end, number.len()),
                // only possible without `num-bigint`, for integers beyond i64
                Err(e) => prop_assert_eq!(e.error_type, JsonErrorType::NumberOutOfRange),
            }
        }
    }
}
