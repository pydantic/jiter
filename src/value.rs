use indexmap::IndexMap;

use crate::chunk::{Chunk, ChunkInfo, Chunker};
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
    pub fn parse(data: &[u8]) -> JsonResult<Self> {
        let mut chunker = Chunker::new(data);
        let chunk = chunker.next().unwrap()?;
        Self::parse_chunk(chunk, &mut chunker)
    }

    fn parse_chunk(chunk: ChunkInfo, chunker: &mut Chunker) -> JsonResult<Self> {
        match chunk.chunk_type {
            Chunk::True => Ok(JsonValue::Bool(true)),
            Chunk::False => Ok(JsonValue::Bool(false)),
            Chunk::Null => Ok(JsonValue::Null),
            Chunk::String(range) => {
                let s = chunker.decode_string(range, chunk.loc)?;
                Ok(JsonValue::String(s))
            }
            Chunk::Int {
                positive,
                range,
                exponent,
            } => {
                let i = chunker.decode_int(positive, range, exponent, chunk.loc)?;
                Ok(JsonValue::Int(i))
            }
            Chunk::Float {
                positive,
                int_range,
                decimal_range,
                exponent,
            } => {
                let f = chunker.decode_float(positive, int_range, decimal_range, exponent, chunk.loc)?;
                Ok(JsonValue::Float(f))
            }
            Chunk::ArrayStart => {
                let mut array = Vec::new();
                loop {
                    let chunk = chunker.next().unwrap()?;
                    match chunk.chunk_type {
                        Chunk::ArrayEnd => break,
                        _ => {
                            let value = Self::parse_chunk(chunk, chunker)?;
                            array.push(value);
                        }
                    }
                }
                Ok(JsonValue::Array(array))
            }
            Chunk::ObjectStart => {
                let mut object = IndexMap::new();
                loop {
                    let chunk = chunker.next().unwrap()?;
                    match chunk.chunk_type {
                        Chunk::ObjectEnd => break,
                        _ => {
                            let key_range = chunk.key.clone().unwrap();
                            let key = chunker.decode_string(key_range, chunk.loc)?;
                            let value = Self::parse_chunk(chunk, chunker)?;
                            object.insert(key, value);
                        }
                    }
                }
                Ok(JsonValue::Object(object))
            }
            Chunk::ObjectEnd | Chunk::ArrayEnd => unreachable!(),
        }
    }
}
