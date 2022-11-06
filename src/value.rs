use std::thread::{sleep};
use std::mem::MaybeUninit;
use std::sync::{Arc};
use std::time::Duration;
use indexmap::IndexMap;

// use crossbeam_channel::{bounded, Receiver};
use crossbeam_utils::thread;
use ringbuf::{Consumer, HeapRb, SharedRb};
use ringbuf::ring_buffer::{RbReadCache, RbWrap};

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

struct JsonConsumer(Consumer<OptRJson, RbWrap<RbReadCache<OptRJson, Arc<SharedRb<OptRJson, Vec<MaybeUninit<OptRJson>>>>>>>);
// struct JsonConsumer(Consumer<OptRJson, Arc<SharedRb<OptRJson, Vec<MaybeUninit<OptRJson>>>>>);

impl JsonConsumer {
    fn next(&mut self) -> OptRJson {
        let mut r = self.0.pop();
        let i: usize = 0;
        loop {
            if i % 50 == 0 {
                self.0.sync();
            }
            r = match r {
                Some(v) => return v,
                None => {
                    sleep(Duration::from_nanos(100));
                    self.0.sync();
                    self.0.pop()
                }
            };
        }
    }
}

impl JsonValue {
    pub fn parse(data: &[u8]) -> JsonResult<Self> {
        thread::scope(|scope| {
            let buf = HeapRb::<OptRJson>::new(200);
            let (producer, consumer) = buf.split();
            let mut producer = producer.into_postponed();
            let handle = scope.spawn(move |_| {
                let chunker = Chunker::new(data);
                for (i, chunk) in chunker.enumerate() {
                // for chunk in chunker {
                    let mut r = producer.push(Some(chunk));
                    while let Err(e) = r {
                        sleep(Duration::from_nanos(100));
                        producer.sync();
                        r = producer.push(e);
                    }
                    if i % 50 == 0 {
                        producer.sync();
                    }
                }
                producer.sync();
            });

            let decoder = Decoder::new(data);
            let mut consumer = JsonConsumer(consumer.into_postponed());
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
            let mut array: Vec<JsonValue> = Vec::with_capacity(25);
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
            let mut object = IndexMap::with_capacity(25);
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
