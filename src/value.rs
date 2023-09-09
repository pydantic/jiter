use indexmap::IndexMap;
use num_bigint::BigInt;

use crate::number_decoder::{NumberAny, NumberDecoder, NumberInt};
use crate::parse::{JsonResult, Parser, Peak};
use crate::string_decoder::StringDecoder;
use crate::{FilePosition, JsonError};

/// similar to serde `Value` but with int and float split
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    BigInt(BigInt),
    Float(f64),
    String(String),
    Array(JsonArray),
    Object(JsonObject),
}
pub type JsonArray = Vec<JsonValue>;
pub type JsonObject = IndexMap<String, JsonValue>;

#[derive(Clone, Debug)]
pub struct JsonErrorPosition {
    pub error: JsonError,
    pub position: FilePosition,
}

impl JsonValue {
    pub fn parse(data: &[u8]) -> Result<Self, JsonErrorPosition> {
        let mut parser = Parser::new(data);

        _parse(&mut parser).map_err(|e| JsonErrorPosition {
            error: e,
            position: FilePosition::find(data, parser.index),
        })
    }
}

fn _parse(parser: &mut Parser) -> Result<JsonValue, JsonError> {
    let peak = parser.peak()?;
    let v = take_value(peak, parser)?;
    parser.finish()?;
    Ok(v)
}

pub(crate) fn take_value(peak: Peak, parser: &mut Parser) -> JsonResult<JsonValue> {
    match peak {
        Peak::True => {
            parser.consume_true()?;
            Ok(JsonValue::Bool(true))
        }
        Peak::False => {
            parser.consume_false()?;
            Ok(JsonValue::Bool(false))
        }
        Peak::Null => {
            parser.consume_null()?;
            Ok(JsonValue::Null)
        }
        Peak::String => {
            let s = parser.consume_string::<StringDecoder>()?;
            Ok(JsonValue::String(s))
        }
        Peak::Num(positive) => {
            let n = parser.consume_number::<NumberDecoder<NumberAny>>(positive)?;
            match n {
                NumberAny::Int(NumberInt::Int(int)) => Ok(JsonValue::Int(int)),
                NumberAny::Int(NumberInt::BigInt(big_int)) => Ok(JsonValue::BigInt(big_int)),
                NumberAny::Int(NumberInt::Zero) => Ok(JsonValue::Int(0)),
                NumberAny::Float(float) => Ok(JsonValue::Float(float)),
            }
        }
        Peak::Array => {
            // we could do something clever about guessing the size of the array
            let mut array: Vec<JsonValue> = Vec::new();
            if let Some(peak_first) = parser.array_first()? {
                let v = take_value(peak_first, parser)?;
                array.push(v);
                while parser.array_step()? {
                    let peak = parser.peak()?;
                    let v = take_value(peak, parser)?;
                    array.push(v);
                }
            }
            Ok(JsonValue::Array(array))
        }
        Peak::Object => {
            // same for objects
            let mut object = IndexMap::new();
            if let Some(first_key) = parser.object_first::<StringDecoder>()? {
                let peak = parser.peak()?;
                let first_value = take_value(peak, parser)?;
                object.insert(first_key, first_value);
                while let Some(key) = parser.object_step::<StringDecoder>()? {
                    let peak = parser.peak()?;
                    let value = take_value(peak, parser)?;
                    object.insert(key, value);
                }
            }

            Ok(JsonValue::Object(object))
        }
    }
}
