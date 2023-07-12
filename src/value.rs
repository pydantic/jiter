use indexmap::IndexMap;

use crate::decode::Decoder;
use crate::element::{JsonResult};
use crate::{FilePosition, JsonError};
use crate::parse::{Number, Parser, Peak};

/// similar to serde `Value` but with int and float split
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
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

        _parse(&mut parser, data).map_err(|e| JsonErrorPosition {
            error: e,
            position: FilePosition::find(data, parser.index),
        })
    }
}

fn _parse(parser: &mut Parser, data: &[u8]) -> Result<JsonValue, JsonError> {
    let decoder = Decoder::new(data);
    let peak = parser.peak()?;
    let v = take_value(peak, parser, &decoder)?;
    parser.finish()?;
    Ok(v)
}

pub(crate) fn take_value(
    peak: Peak,
    parser: &mut Parser,
    decoder: &Decoder,
) -> JsonResult<JsonValue> {
    match peak {
        Peak::True => {
            parser.consume_true()?;
            Ok(JsonValue::Bool(true))
        },
        Peak::False => {
            parser.consume_false()?;
            Ok(JsonValue::Bool(false))
        },
        Peak::Null => {
            parser.consume_null()?;
            Ok(JsonValue::Null)
        },
        Peak::String => {
            let range = parser.consume_string_range()?;
            let s = decoder.decode_string(range)?;
            Ok(JsonValue::String(s))
        }
        Peak::NumPos => parse_number(true, parser, decoder),
        Peak::NumNeg => parse_number(false, parser, decoder),
        Peak::Array => {
            // we could do something clever about guessing the size of the array
            let mut array: Vec<JsonValue> = Vec::new();
            if parser.array_first()? {
                loop {
                    let peak = parser.peak()?;
                    let v = take_value(peak, parser, decoder)?;
                    array.push(v);
                    if !parser.array_step()? {
                        break
                    }
                }
            }
            Ok(JsonValue::Array(array))
        }
        Peak::Object => {
            // same for objects
            let mut object = IndexMap::new();
            if let Some(key) = parser.object_first()? {
                let key = decoder.decode_string(key)?;
                let peak = parser.peak()?;
                let value = take_value(peak, parser, decoder)?;
                object.insert(key, value);
                while let Some(key) = parser.object_step()? {
                    let key = decoder.decode_string(key)?;
                    let peak = parser.peak()?;
                    let value = take_value(peak, parser, decoder)?;
                    object.insert(key, value);
                }
            }

            Ok(JsonValue::Object(object))
        }
    }
}

fn parse_number(
    positive: bool,
    parser: &mut Parser,
    decoder: &Decoder,
) -> JsonResult<JsonValue> {
    let number = parser.next_number(positive)?;
    match number {
        Number::Int {positive, range, exponent} => {
            let i = decoder.decode_int(positive, range, exponent)?;
            Ok(JsonValue::Int(i))
        }
        Number::Float {positive, int_range, decimal_range, exponent} => {
            let f = decoder.decode_float(positive, int_range, decimal_range, exponent)?;
            Ok(JsonValue::Float(f))
        }
    }
}
