use indexmap::IndexMap;

use crossbeam_channel::{bounded, Receiver};
use crossbeam_utils::thread;

use crate::chunk::{Chunk, ChunkInfo, Chunker};
use crate::decode::Decoder;
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
        thread::scope(|scope| {
            let (sender, receiver) = bounded(2);
            let handle = scope.spawn(move |_| {
                let chunker = Chunker::new(data);
                for chunk in chunker {
                    sender.send(chunk).unwrap();
                }
            });

            let chunk = receiver.recv().unwrap()?;
            let decoder = Decoder::new(data);
            let s = take_chunk(chunk, &receiver, &decoder);
            handle.join().unwrap();
            s
        })
        .unwrap()
    }
}

fn take_chunk(
    chunk: ChunkInfo,
    receiver: &Receiver<JsonResult<ChunkInfo>>,
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
            let array = receiver
                .iter()
                .take_while(|r| match r {
                    Ok(c) => match c.chunk_type {
                        Chunk::ArrayEnd => false,
                        _ => true,
                    },
                    _ => false,
                })
                .map(|r| take_chunk(r?, receiver, decoder))
                .collect::<JsonResult<Vec<_>>>()?;
            Ok(JsonValue::Array(array))
        }
        Chunk::ObjectStart => {
            let mut object = IndexMap::with_capacity(100);
            loop {
                let key = receiver.recv().unwrap()?;
                match key.chunk_type {
                    Chunk::ObjectEnd => break,
                    Chunk::Key(key_range) => {
                        let key = decoder.decode_string(key_range, key.loc)?;
                        let value_chunk = receiver.recv().unwrap()?;
                        let value = take_chunk(value_chunk, receiver, decoder)?;
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
