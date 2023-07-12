use indexmap::IndexMap;

use crate::decode::Decoder;
use crate::element::{Element, JsonResult};
use crate::{FilePosition, JsonError};
use crate::parse::Parser;

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
    let element = parser.next_value()?;
    let v = take_value(element, parser, &decoder)?;
    parser.finish()?;
    Ok(v)
}

pub(crate) fn take_value(
    element: Element,
    parser: &mut Parser,
    decoder: &Decoder,
) -> JsonResult<JsonValue> {
    match element {
        Element::True => Ok(JsonValue::Bool(true)),
        Element::False => Ok(JsonValue::Bool(false)),
        Element::Null => Ok(JsonValue::Null),
        Element::String(range) => {
            let s = decoder.decode_string(range)?;
            Ok(JsonValue::String(s))
        }
        Element::Int {
            positive,
            range,
            exponent,
        } => {
            let i = decoder.decode_int(positive, range, exponent)?;
            Ok(JsonValue::Int(i))
        }
        Element::Float {
            positive,
            int_range,
            decimal_range,
            exponent,
        } => {
            let f = decoder.decode_float(positive, int_range, decimal_range, exponent)?;
            Ok(JsonValue::Float(f))
        }
        Element::ArrayStart => {
            // we could do something clever about guessing the size of the array
            let mut array: Vec<JsonValue> = Vec::new();
            if parser.array_first()? {
                loop {
                    let chunk = parser.next_value()?;
                    let v = take_value(chunk, parser, decoder)?;
                    array.push(v);
                    if !parser.array_step()? {
                        break
                    }
                }
            }
            Ok(JsonValue::Array(array))
        }
        Element::ObjectStart => {
            // same for objects
            let mut object = IndexMap::new();
            if let Some(key) = parser.object_first()? {
                let key = decoder.decode_string(key)?;
                let value_chunk = parser.next_value()?;
                let value = take_value(value_chunk, parser, decoder)?;
                object.insert(key, value);
                while let Some(key) = parser.object_step()? {
                    let key = decoder.decode_string(key)?;
                    let value_chunk = parser.next_value()?;
                    let value = take_value(value_chunk, parser, decoder)?;
                    object.insert(key, value);
                }
            }

            Ok(JsonValue::Object(object))
        }
        Element::ObjectEnd | Element::ArrayEnd | Element::Key(_) => unreachable!("{:?}", element),
    }
}
