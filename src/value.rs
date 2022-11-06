use std::thread::sleep;
use std::mem::MaybeUninit;
use std::sync::{Arc};
use std::time::Duration;
use indexmap::IndexMap;

// use crossbeam_channel::{bounded, Receiver};
use crossbeam_utils::thread;
use ringbuf::{Consumer, HeapRb, SharedRb};

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

type OptRJson = Option<JsonResult<ChunkInfo>>;

struct JsonConsumer(Consumer<OptRJson, Arc<SharedRb<OptRJson, Vec<MaybeUninit<OptRJson>>>>>);

impl Iterator for JsonConsumer {
    type Item = JsonResult<ChunkInfo>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut r = self.0.pop();
        loop {
            r = match r {
                Some(v) => return v,
                None => {
                    sleep(Duration::from_micros(1));
                    self.0.pop()
                }
            };
        }
    }
}

impl JsonValue {
    pub fn parse(data: &[u8]) -> JsonResult<Self> {
        thread::scope(|scope| {
            let buf = HeapRb::<OptRJson>::new(32);
            let (mut producer, consumer) = buf.split();
            let handle = scope.spawn(move |_| {
                let chunker = Chunker::new(data);
                for chunk in chunker {
                    let mut r = producer.push(Some(chunk));
                    while let Err(e) = r {
                        sleep(Duration::from_micros(1));
                        r = producer.push(e);
                    }
                }
            });

            let decoder = Decoder::new(data);
            let mut consumer = JsonConsumer(consumer);
            let chunk = consumer.next().unwrap()?;
            let s = take_chunk(chunk, &mut consumer, &decoder);
            handle.join().unwrap();
            s
        })
        .unwrap()
    }
}

fn take_chunk(
    chunk: ChunkInfo,
    consumer: &mut JsonConsumer,
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
            let mut array: Vec<JsonValue> = Vec::new();
            loop {
                let chunk = consumer.next().unwrap()?;
                match chunk.chunk_type {
                    Chunk::ArrayEnd => break,
                    _ => {
                        let v = take_chunk(chunk, consumer, decoder)?;
                        array.push(v);
                    }
                }
            }
            Ok(JsonValue::Array(array))
        }
        Chunk::ObjectStart => {
            let mut object = IndexMap::with_capacity(100);
            loop {
                let key = consumer.next().unwrap()?;
                match key.chunk_type {
                    Chunk::ObjectEnd => break,
                    Chunk::Key(key_range) => {
                        let key = decoder.decode_string(key_range, key.loc)?;
                        let value_chunk = consumer.next().unwrap()?;
                        let value = take_chunk(value_chunk, consumer, decoder)?;
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
