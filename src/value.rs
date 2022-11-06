use std::thread::{sleep};
use std::time::Duration;
use indexmap::IndexMap;

use crossbeam_utils::thread;
use rtrb::{RingBuffer, PushError, PopError, Consumer};

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
const GROUP_SIZE: usize = 32;
type ResultGroup = [OptRJson; GROUP_SIZE];

fn create_group() -> ResultGroup {
    // let v: Vec<OptRJson> = (0..GROUP_SIZE).map(|_| None).collect();
    // v.try_into().unwrap()
    Default::default()
}

struct JsonConsumer {
    consumer: Consumer<ResultGroup>,
    counter: usize,
    cache: ResultGroup,
}

impl JsonConsumer {
    fn new(consumer: Consumer<ResultGroup>) -> Self {
        Self {
            consumer,
            counter: 0,
            cache: create_group(),
        }
    }
    fn next(&mut self) -> OptRJson {
        let index = self.counter % GROUP_SIZE;
        if index == 0 {
            let mut r = self.consumer.pop();
            loop {
                match r {
                    Ok(v) => {
                        self.cache = v;
                        break;
                    },
                    Err(e) => {
                        sleep(Duration::from_nanos(50));
                        r = match e {
                            PopError::Empty => self.consumer.pop(),
                        };
                    }
                };
            }
        }
        let r = self.cache[index].take();
        self.counter += 1;
        r
    }
}

impl JsonValue {
    pub fn parse(data: &[u8]) -> JsonResult<Self> {
        thread::scope(|scope| {
            let (mut producer, consumer) = RingBuffer::<ResultGroup>::new(100);
            let handle = scope.spawn(move |_| {
                let chunker = Chunker::new(data);
                let mut group: ResultGroup = create_group();
                let mut i: usize = 0;
                for chunk in chunker {
                    let index = i % GROUP_SIZE;
                    i += 1;
                    group[index] = Some(chunk);
                    if index != GROUP_SIZE - 1 {
                        continue;
                    }
                    let mut r = producer.push(group.clone());
                    while let Err(e) = r {
                        sleep(Duration::from_nanos(50));
                        r = match e {
                            PushError::Full(e) => producer.push(e),
                        };
                    }
                }
                let next_index = i % GROUP_SIZE;
                if next_index != GROUP_SIZE {
                    // we need to send the remaining chunks in a final group
                    for index in i % GROUP_SIZE + 1..GROUP_SIZE {
                        group[index] = None;
                    }
                    let mut r = producer.push(group.clone());
                    while let Err(e) = r {
                        sleep(Duration::from_nanos(50));
                        // producer.sync();
                        r = match e {
                            PushError::Full(e) => producer.push(e),
                        };
                    }
                }
            });

            let decoder = Decoder::new(data);
            // let mut consumer = JsonConsumer(consumer.into_postponed());
            let mut consumer = JsonConsumer::new(consumer);
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
