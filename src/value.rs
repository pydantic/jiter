use indexmap::IndexMap;

use crate::chunk::{Chunk, ChunkInfo, Chunker};
use crate::decode::Decoder;
use crate::threaded::threaded_parse;
use crate::JsonResult;

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
    pub fn threaded_parse(data: &[u8]) -> JsonResult<Self> {
        threaded_parse(data, |consumer| {
            let decoder = Decoder::new(data);
            let chunk = consumer.next().unwrap()?;
            take_chunk(chunk, consumer, &decoder)
        })
        .unwrap()
    }

    pub fn parse(data: &[u8]) -> JsonResult<Self> {
        let mut chunker = Chunker::new(data);
        let decoder = Decoder::new(data);
        let chunk = chunker.next().unwrap()?;
        take_chunk(chunk, &mut chunker, &decoder)
    }
}

fn take_chunk(
    chunk: ChunkInfo,
    json_iter: &mut impl Iterator<Item = JsonResult<ChunkInfo>>,
    decoder: &Decoder,
) -> JsonResult<JsonValue> {
    match chunk.chunk_type {
        Chunk::True => Ok(JsonValue::Bool(true)),
        Chunk::False => Ok(JsonValue::Bool(false)),
        Chunk::Null => Ok(JsonValue::Null),
        Chunk::String(range) => {
            let s = decoder.decode_string(range, chunk.loc)?;
            Ok(JsonValue::String(s))
        }
        Chunk::Int {
            positive,
            range,
            exponent,
        } => {
            let i = decoder.decode_int(positive, range, exponent, chunk.loc)?;
            Ok(JsonValue::Int(i))
        }
        Chunk::Float {
            positive,
            int_range,
            decimal_range,
            exponent,
        } => {
            let f = decoder.decode_float(positive, int_range, decimal_range, exponent, chunk.loc)?;
            Ok(JsonValue::Float(f))
        }
        Chunk::ArrayStart => {
            // we could do something clever about guessing the size of the array
            let mut array: Vec<JsonValue> = Vec::new();
            loop {
                let chunk = json_iter.next().unwrap()?;
                match chunk.chunk_type {
                    Chunk::ArrayEnd => break,
                    _ => {
                        let v = take_chunk(chunk, json_iter, decoder)?;
                        array.push(v);
                    }
                }
            }
            Ok(JsonValue::Array(array))
        }
        Chunk::ObjectStart => {
            // same for objects
            let mut object = IndexMap::new();
            loop {
                let key = json_iter.next().unwrap()?;
                match key.chunk_type {
                    Chunk::ObjectEnd => break,
                    Chunk::Key(key_range) => {
                        let key = decoder.decode_string(key_range, key.loc)?;
                        let value_chunk = json_iter.next().unwrap()?;
                        let value = take_chunk(value_chunk, json_iter, decoder)?;
                        object.insert(key, value);
                    }
                    _ => unreachable!(),
                }
            }
            Ok(JsonValue::Object(object))
        }
        Chunk::ObjectEnd | Chunk::ArrayEnd | Chunk::Key(_) => unreachable!(),
    }
}

