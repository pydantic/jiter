// use num_bigint::BigInt;
// use speedate::{Date, Time, DateTime, Duration};

use crate::{Decoder, Element, ElementInfo, JsonValue, Parser};
use crate::element::{ErrorInfo, Location};
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
    Key,
}

#[derive(Debug, Eq, PartialEq)]
pub enum FleeceError {
    JsonError(ErrorInfo),
    WrongType {
        expected: JsonType,
        actual: Option<JsonType>,
        loc: Location
    },
    StringFormat(Location),
    NumericValue(Location),
    // StringFormatSpeedate{
    //     speedate_error: speedate::ParseError,
    //     loc: Location,
    // },
    ArrayEnd,
    ObjectEnd,
    EndReached,
    UnknownError(Location),
}

impl From<ErrorInfo> for FleeceError {
    fn from(err: ErrorInfo) -> Self {
        FleeceError::JsonError(err)
    }
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
        let chunk = self.parser.next_value()?;
        match chunk.element {
            Element::Null => Ok(()),
            _ => Err(wrong_type(JsonType::Null, chunk))
        }
    }

    pub fn next_bool_strict(&mut self) -> FleeceResult<bool> {
        let chunk = self.parser.next_value()?;
        match chunk.element {
            Element::True => Ok(true),
            Element::False => Ok(false),
            _ => Err(wrong_type(JsonType::Bool, chunk))
        }
    }

    pub fn next_bool_lax(&mut self) -> FleeceResult<bool> {
        let chunk = self.parser.next_value()?;
        match chunk.element {
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
                    Err(FleeceError::StringFormat(chunk.loc))
                }
            },
            Element::Int {positive, range, ..} => {
                let bytes = &self.data[range];
                if bytes == b"0" {
                    Ok(false)
                } else if positive && bytes == b"1" {
                    Ok(true)
                } else {
                    Err(FleeceError::NumericValue(chunk.loc))
                }
            }
            // TODO float
            _ => Err(wrong_type(JsonType::Bool, chunk))
        }
    }

    pub fn next_int_strict(&mut self) -> FleeceResult<i64> {
        let chunk = self.parser.next_value()?;
        match chunk.element {
            Element::Int {positive, range, exponent} => {
                Ok(self.decoder.decode_int(positive, range, exponent, chunk.loc)?)
            },
            _ => Err(wrong_type(JsonType::Int, chunk))
        }
    }

    pub fn next_int_lax(&mut self) -> FleeceResult<i64> {
        todo!("next_int_lax");
    }

    pub fn next_float_strict(&mut self) -> FleeceResult<f64> {
        let chunk = self.parser.next_value()?;
        match chunk.element {
            Element::Float {positive, int_range, decimal_range, exponent} => {
                Ok(self.decoder.decode_float(positive, int_range, decimal_range, exponent, chunk.loc)?)
            },
            _ => Err(wrong_type(JsonType::Float, chunk))
        }
    }

    pub fn next_float_lax(&mut self) -> FleeceResult<f64> {
        todo!("next_float_lax");
    }

    pub fn next_str(&mut self) -> FleeceResult<String> {
        let chunk = self.parser.next_value()?;
        match chunk.element {
            Element::String(range) => {
                Ok(self.decoder.decode_string(range, chunk.loc)?)
            },
            _ => Err(wrong_type(JsonType::String, chunk))
        }
    }

    pub fn next_bytes(&mut self) -> FleeceResult<&[u8]> {
        let chunk = self.parser.next_value()?;
        match chunk.element {
            Element::String(range) => Ok(&self.data[range]),
            _ => Err(wrong_type(JsonType::String, chunk))
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
        let chunk = self.parser.next_value()?;
        Ok(take_value(chunk, &mut self.parser, &self.decoder)?)
    }

    pub fn next_array(&mut self) -> FleeceResult<()> {
        let chunk = self.parser.next_value()?;
        match chunk.element {
            Element::ArrayStart => Ok(()),
            _ => Err(wrong_type(JsonType::Array, chunk))
        }
    }

    pub fn array_step(&mut self) -> FleeceResult<bool> {
        Ok(self.parser.array_step()?)
    }

    pub fn next_object(&mut self) -> FleeceResult<()> {
        let chunk = self.parser.next_value()?;
        match chunk.element {
            Element::ObjectStart => Ok(()),
            _ => Err(wrong_type(JsonType::Object, chunk))
        }
    }

    pub fn first_key(&mut self) -> FleeceResult<Option<String>> {
        match self.parser.object_first() {
            Ok(Some(key)) => Ok(Some(self.decoder.decode_string(key.range, key.loc)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(e.into())
        }
    }

    pub fn next_key(&mut self) -> FleeceResult<Option<String>> {
        match self.parser.object_step() {
            Ok(Some(key)) => Ok(Some(self.decoder.decode_string(key.range, key.loc)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(e.into())
        }
    }
}

fn wrong_type(expected: JsonType, chunk: ElementInfo) -> FleeceError {
    match chunk.element {
        Element::ArrayEnd => FleeceError::ArrayEnd,
        Element::ObjectEnd => FleeceError::ObjectEnd,
        Element::ArrayStart => FleeceError::WrongType {
            expected,
            actual: Some(JsonType::Array),
            loc: chunk.loc
        },
        Element::ObjectStart => FleeceError::WrongType {
            expected,
            actual: Some(JsonType::Object),
            loc: chunk.loc
        },
        Element::True => FleeceError::WrongType {
            expected,
            actual: Some(JsonType::Bool),
            loc: chunk.loc
        },
        Element::False => FleeceError::WrongType {
            expected,
            actual: Some(JsonType::Bool),
            loc: chunk.loc
        },
        Element::Null => FleeceError::WrongType {
            expected,
            actual: Some(JsonType::Null),
            loc: chunk.loc
        },
        Element::Key(_) => FleeceError::WrongType {
            expected,
            actual: Some(JsonType::Key),
            loc: chunk.loc
        },
        Element::String(_) => FleeceError::WrongType {
            expected,
            actual: Some(JsonType::String),
            loc: chunk.loc
        },
        Element::Int{..} => FleeceError::WrongType {
            expected,
            actual: Some(JsonType::Int),
            loc: chunk.loc
        },
        Element::Float{..} => FleeceError::WrongType {
            expected,
            actual: Some(JsonType::Float),
            loc: chunk.loc
        },
    }
}
