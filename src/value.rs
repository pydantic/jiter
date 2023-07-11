use indexmap::IndexMap;

use crate::decode::Decoder;
use crate::element::{Element, ElementInfo, JsonResult};
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

impl JsonValue {
    pub fn parse(data: &[u8]) -> JsonResult<Self> {
        let mut parser = Parser::new(data);
        let decoder = Decoder::new(data);
        let chunk = parser.next_value()?;
        let v = take_value(chunk, &mut parser, &decoder)?;
        parser.finish()?;
        Ok(v)
    }
}

pub(crate) fn take_value(
    chunk: ElementInfo,
    json_iter: &mut Parser,
    decoder: &Decoder,
) -> JsonResult<JsonValue> {
    match chunk.element {
        Element::True => Ok(JsonValue::Bool(true)),
        Element::False => Ok(JsonValue::Bool(false)),
        Element::Null => Ok(JsonValue::Null),
        Element::String(range) => {
            let s = decoder.decode_string(range, chunk.loc)?;
            Ok(JsonValue::String(s))
        }
        Element::Int {
            positive,
            range,
            exponent,
        } => {
            let i = decoder.decode_int(positive, range, exponent, chunk.loc)?;
            Ok(JsonValue::Int(i))
        }
        Element::Float {
            positive,
            int_range,
            decimal_range,
            exponent,
        } => {
            let f = decoder.decode_float(positive, int_range, decimal_range, exponent, chunk.loc)?;
            Ok(JsonValue::Float(f))
        }
        Element::ArrayStart => {
            // we could do something clever about guessing the size of the array
            let mut array: Vec<JsonValue> = Vec::new();
            if json_iter.array_first()? {
                loop {
                    let chunk = json_iter.next_value()?;
                    let v = take_value(chunk, json_iter, decoder)?;
                    array.push(v);
                    if !json_iter.array_step()? {
                        break
                    }
                }
            }
            Ok(JsonValue::Array(array))
        }
        Element::ObjectStart => {
            // same for objects
            let mut object = IndexMap::new();
            if let Some(key) = json_iter.object_first()? {
                let key = decoder.decode_string(key.range, key.loc)?;
                let value_chunk = json_iter.next_value()?;
                let value = take_value(value_chunk, json_iter, decoder)?;
                object.insert(key, value);
                while let Some(key) = json_iter.object_step()? {
                    let key = decoder.decode_string(key.range, key.loc)?;
                    let value_chunk = json_iter.next_value()?;
                    let value = take_value(value_chunk, json_iter, decoder)?;
                    object.insert(key, value);
                }
            }

            Ok(JsonValue::Object(object))
        }
        Element::ObjectEnd | Element::ArrayEnd | Element::Key(_) => unreachable!("{:?}", chunk),
    }
}
