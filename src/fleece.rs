use std::ops::Range;

// use num_bigint::BigInt;
// use speedate::{Date, Time, DateTime, Duration};

use crate::{Decoder, Element, FilePosition, JsonError, JsonResult, JsonValue, Parser};
use crate::value::take_value;

#[derive(Debug, Eq, PartialEq)]
pub enum JsonType {
    Null,
    Bool,
    Int,
    Float,
    String,
    Array,
    Object,
}

#[derive(Debug, Eq, PartialEq)]
pub enum FleeceError {
    JsonError {
        error: JsonError,
        position: FilePosition,
    },
    WrongType {
        expected: JsonType,
        actual: JsonType,
        position: FilePosition,
    },
    StringFormat(FilePosition),
    NumericValue(FilePosition),
    // StringFormatSpeedate{
    //     speedate_error: speedate::ParseError,
    //     loc: Location,
    // },
    ArrayEnd,
    ObjectEnd,
    EndReached,
    UnknownError(FilePosition),
}

pub type FleeceResult<T> = Result<T, FleeceError>;

pub struct Fleece<'a> {
    data: &'a [u8],
    parser: Parser<'a>,
    decoder: Decoder<'a>,
}

// #[derive(Debug, Clone)]
// pub enum FleeceInt {
//     Int(i64),
//     BigInt(BigInt),
// }

impl<'a> Fleece<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            parser: Parser::new(data),
            decoder: Decoder::new(data),
        }
    }

    pub fn next_null(&mut self) -> FleeceResult<()> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        match element {
            Element::Null => Ok(()),
            _ => Err(self.wrong_type(JsonType::Null, element))
        }
    }

    pub fn next_bool_strict(&mut self) -> FleeceResult<bool> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        match element {
            Element::True => Ok(true),
            Element::False => Ok(false),
            _ => Err(self.wrong_type(JsonType::Bool, element))
        }
    }

    pub fn next_bool_lax(&mut self) -> FleeceResult<bool> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        match element {
            Element::True => Ok(true),
            Element::False => Ok(false),
            Element::String(range) => {
                let bytes = &self.data[range];
                // matches pydantic

                return if bytes == b"0"
                    || bytes.eq_ignore_ascii_case(b"f")
                    || bytes.eq_ignore_ascii_case(b"n")
                    || bytes.eq_ignore_ascii_case(b"no")
                    || bytes.eq_ignore_ascii_case(b"off")
                    || bytes.eq_ignore_ascii_case(b"false")
                {
                    Ok(false)
                } else if bytes == b"1"
                    || bytes.eq_ignore_ascii_case(b"t")
                    || bytes.eq_ignore_ascii_case(b"y")
                    || bytes.eq_ignore_ascii_case(b"on")
                    || bytes.eq_ignore_ascii_case(b"yes")
                    || bytes.eq_ignore_ascii_case(b"true")
                {
                    Ok(true)
                } else {
                    Err(FleeceError::StringFormat(FilePosition::new(0, 0)))
                }
            },
            Element::Int {positive, range, ..} => {
                let bytes = &self.data[range];
                if bytes == b"0" {
                    Ok(false)
                } else if positive && bytes == b"1" {
                    Ok(true)
                } else {
                    Err(FleeceError::NumericValue(FilePosition::new(0, 0)))
                }
            }
            // TODO float
            _ => Err(self.wrong_type(JsonType::Bool, element))
        }
    }

    pub fn next_int_strict(&mut self) -> FleeceResult<i64> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        match element {
            Element::Int {positive, range, exponent} => {
                self.decoder.decode_int(positive, range, exponent).map_err(|e| self.map_err(e))
            },
            _ => Err(self.wrong_type(JsonType::Int, element))
        }
    }

    pub fn next_int_lax(&mut self) -> FleeceResult<i64> {
        todo!("next_int_lax");
    }

    pub fn next_float_strict(&mut self) -> FleeceResult<f64> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        match element {
            Element::Float {positive, int_range, decimal_range, exponent} => {
                self.decoder.decode_float(positive, int_range, decimal_range, exponent).map_err(|e| self.map_err(e))
            },
            _ => Err(self.wrong_type(JsonType::Float, element))
        }
    }

    pub fn next_float_lax(&mut self) -> FleeceResult<f64> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        match element {
            Element::Float {positive, int_range, decimal_range, exponent} => {
                self.decoder.decode_float(positive, int_range, decimal_range, exponent).map_err(|e| self.map_err(e))
            },
            Element::Int {positive, range, exponent} => {
                let int = self.decoder.decode_int(positive, range, exponent).map_err(|e| self.map_err(e))?;
                Ok(int as f64)
            },
            _ => Err(self.wrong_type(JsonType::Float, element))
        }
    }

    pub fn next_str(&mut self) -> FleeceResult<String> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        match element {
            Element::String(range) => {
                self.decoder.decode_string(range).map_err(|e| self.map_err(e))
            },
            _ => Err(self.wrong_type(JsonType::String, element))
        }
    }

    pub fn next_bytes(&mut self) -> FleeceResult<&[u8]> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        match element {
            Element::String(range) => Ok(&self.data[range]),
            _ => Err(self.wrong_type(JsonType::String, element))
        }
    }

    // pub fn next_date(&mut self) -> FleeceResult<Date> {
    //     todo!("next_date");
    // }
    // pub fn next_time(&mut self) -> FleeceResult<Time> {
    //     todo!("next_time");
    // }
    // pub fn next_datetime(&mut self) -> FleeceResult<DateTime> {
    //     todo!("next_datetime");
    // }
    // pub fn next_duration(&mut self) -> FleeceResult<Duration> {
    //     todo!("next_duration");
    // }

    pub fn next_value(&mut self) -> FleeceResult<JsonValue> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        take_value(element, &mut self.parser, &self.decoder).map_err(|e| self.map_err(e))
    }

    pub fn next_array(&mut self) -> FleeceResult<bool> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        match element {
            Element::ArrayStart => self.parser.array_first().map_err(|e| self.map_err(e)),
            _ => Err(self.wrong_type(JsonType::Array, element))
        }
    }

    pub fn array_step(&mut self) -> FleeceResult<bool> {
        self.parser.array_step().map_err(|e| self.map_err(e))
    }

    pub fn next_object(&mut self) -> FleeceResult<Option<String>> {
        let element = self.parser.next_value().map_err(|e| self.map_err(e))?;
        match element {
            Element::ObjectStart => {
                let result = self.parser.object_first();
                self.key_result(result)
            },
            _ => Err(self.wrong_type(JsonType::Object, element))
        }
    }

    pub fn next_key(&mut self) -> FleeceResult<Option<String>> {
        let result = self.parser.object_step();
        self.key_result(result)
    }

    pub fn finish(&mut self) -> FleeceResult<()> {
        self.parser.finish().map_err(|e| self.map_err(e))
    }

    fn key_result(&self, result: JsonResult<Option<Range<usize>>>) -> FleeceResult<Option<String>> {
        match result {
            Ok(Some(key)) => {
                let s = self.decoder.decode_string(key).map_err(|e| self.map_err(e))?;
                Ok(Some(s))
            },
            Ok(None) => Ok(None),
            Err(e) => Err(self.map_err(e))
        }
    }

    fn map_err(&self, error: JsonError) -> FleeceError {
        FleeceError::JsonError{
            error,
            position: self.parser.current_position()
        }
    }

    fn wrong_type(&self, expected: JsonType, element: Element) -> FleeceError {
        let position = self.parser.last_position();
        match element {
            Element::ArrayStart => FleeceError::WrongType {
                expected,
                actual: JsonType::Array,
                position,
            },
            Element::ObjectStart => FleeceError::WrongType {
                expected,
                actual: JsonType::Object,
                position,
            },
            Element::True => FleeceError::WrongType {
                expected,
                actual: JsonType::Bool,
                position,
            },
            Element::False => FleeceError::WrongType {
                expected,
                actual: JsonType::Bool,
                position,
            },
            Element::Null => FleeceError::WrongType {
                expected,
                actual: JsonType::Null,
                position,
            },
            Element::String(_) => FleeceError::WrongType {
                expected,
                actual: JsonType::String,
                position,
            },
            Element::Int{..} => FleeceError::WrongType {
                expected,
                actual: JsonType::Int,
                position,
            },
            Element::Float{..} => FleeceError::WrongType {
                expected,
                actual: JsonType::Float,
                position,
            },
        }
    }
}

