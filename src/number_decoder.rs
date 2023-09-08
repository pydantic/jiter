use std::fmt;
use std::marker::PhantomData;
use num_bigint::BigInt;
use crate::{JsonError, JsonResult};

pub trait AbstractNumberDecoder {
    type Output;

    fn decode(data: &[u8], index: usize, positive: bool) -> JsonResult<(Self::Output, usize)>;
}

pub trait AbstractNumber: fmt::Debug + Default {
    fn new(digit: &u8) -> Self;

    fn add_digit(&mut self, digit: &u8);

    fn apply_exponential(&mut self, exponent: NumberInt);

    fn apply_decimal(self, data: &[u8], index: usize) -> JsonResult<(Self, usize)>;

    fn negate(&mut self);
}

#[derive(Debug, Clone)]
pub enum NumberInt {
    Int(i64),
    BigInt(BigInt),
}

impl Default for NumberInt {
    fn default() -> Self {
        Self::Int(0)
    }
}

impl AbstractNumber for NumberInt {
    fn new(digit: &u8) -> Self {
        Self::Int((digit & 0x0f) as i64)
    }

    fn add_digit(&mut self, digit: &u8) {
        let digit_int = digit & 0x0f;
        match self {
            Self::Int(int_64) => {
                if let Some(mult_10) = int_64.checked_mul(10) {
                    if let Some(add_digit) = mult_10.checked_add((digit_int) as i64) {
                        *int_64 = add_digit;
                    } else {
                        let mut big_int: BigInt = mult_10.into();
                        big_int += digit_int;
                        *self = Self::BigInt(big_int);
                    }
                } else {
                    let mut big_int: BigInt = (*int_64).into();
                    big_int *= 10;
                    big_int += digit_int;
                    *self = Self::BigInt(big_int);
                }
            }
            Self::BigInt(ref mut big_int) => {
                *big_int *= 10;
                *big_int += digit_int;
            }
        }
    }

    fn apply_exponential(&mut self, exponent: NumberInt) {
        todo!("finalise {:?}", exponent)
    }

    fn apply_decimal(self, _data: &[u8], _index: usize) -> JsonResult<(Self, usize)> {
        Err(JsonError::FloatExpectingInt)
    }

    fn negate(&mut self) {
        match self {
            Self::Int(int) => *int = -*int,
            Self::BigInt(big_int) => {
                *big_int *= -1;
            },
        }
    }
}
pub struct NumberDecoder<Num: AbstractNumber> {
    phantom: PhantomData<Num>
}

impl<Num: AbstractNumber> AbstractNumberDecoder for NumberDecoder<Num> {
    type Output = Num;

    fn decode(data: &[u8], mut index: usize, positive: bool) -> JsonResult<(Self::Output, usize)> {
        if !positive {
            // we started with a minus sign, so the first digit is at index + 1
            index += 1;
        };
        let mut num = match data.get(index) {
            Some(digit) if (b'0'..=b'9').contains(digit) => Num::new(digit),
            Some(_) => return Err(JsonError::UnexpectedCharacter),
            None => return Err(JsonError::UnexpectedEnd),
        };

        while let Some(next) = data.get(index) {
            match next {
                b'0'..=b'9' => num.add_digit(next),
                b'.' => return num.apply_decimal(data, index),
                b'e' | b'E' => {
                    let (exponent, end) = exponent_int(data, index)?;
                    num.apply_exponential(exponent);
                    return Ok((num, end));
                }
                _ => break,
            }
            index += 1;
        }
        if positive {
            Ok((num, index))
        } else {
            num.negate();
            Ok((num, index))
        }
    }
}

fn exponent_int(data: &[u8], mut index: usize) -> JsonResult<(NumberInt, usize)> {
    index += 1;

    let positive = match data.get(index) {
        Some(b'-') => {
            index += 1;
            false
        }
        Some(b'+') => {
            index += 1;
            true
        }
        Some(v) if (b'0'..=b'9').contains(v) => true,
        Some(_) => return Err(JsonError::UnexpectedEnd),
        None => return Err(JsonError::UnexpectedEnd),
    };
    let mut num = NumberInt::default();

    while let Some(next) = data.get(index) {
        match next {
            b'0'..=b'9' => num.add_digit(next),
            _ => break,
        }
        index += 1;
    }

    if positive {
        Ok((num, index))
    } else {
        num.negate();
        Ok((num, index))
    }
}
