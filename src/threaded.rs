use std::thread::{sleep, Result as StdThreadResult};
use std::time::Duration;

use crossbeam_utils::thread::scope;
use rtrb::{Consumer, PopError, Producer, PushError, RingBuffer};

use crate::element::{JsonResult, ElementInfo};
use crate::parse::Parser;

type OptRJson = Option<JsonResult<ElementInfo>>;
const GROUP_SIZE: usize = 32;
const RING_BUFFER_CAPACITY: usize = 100;
type ResultGroup = [OptRJson; GROUP_SIZE];

pub fn threaded_parse<F, R>(data: &[u8], f: F) -> StdThreadResult<R>
where
    F: FnOnce(&mut JsonConsumer) -> R,
{
    scope(|scope| {
        let (mut producer, mut consumer) = create_ring_buffer();
        let handle = scope.spawn(move |_| {
            let parser = Parser::new(data);
            for element_result in parser {
                producer.push(element_result);
            }
            producer.finish();
        });

        let output = f(&mut consumer);
        handle.join().unwrap();
        output
    })
}

pub fn create_ring_buffer() -> (JsonProducer, JsonConsumer) {
    let (producer, consumer) = RingBuffer::<ResultGroup>::new(RING_BUFFER_CAPACITY);
    let start_group = new_group();
    let producer = JsonProducer {
        producer,
        counter: 0,
        group: start_group.clone(),
    };

    let consumer = JsonConsumer {
        consumer,
        counter: 0,
        cache: start_group,
    };
    (producer, consumer)
}

pub struct JsonProducer {
    producer: Producer<ResultGroup>,
    counter: usize,
    group: ResultGroup,
}

impl JsonProducer {
    pub fn push(&mut self, element_result: JsonResult<ElementInfo>) {
        let index = self.counter % GROUP_SIZE;
        self.counter += 1;
        self.group[index] = Some(element_result);
        if index == GROUP_SIZE - 1 {
            self._push()
        }
    }

    pub fn finish(&mut self) {
        let next_index = self.counter % GROUP_SIZE + 1;
        if next_index != GROUP_SIZE {
            // we need to send the remaining elements in a final group
            for index in next_index..GROUP_SIZE {
                self.group[index] = None;
            }
            self._push()
        }
    }

    fn _push(&mut self) {
        let mut r = self.producer.push(self.group.clone());
        while let Err(e) = r {
            sleep_ns(50);
            // producer.sync();
            r = match e {
                PushError::Full(e) => self.producer.push(e),
            };
        }
    }
}

pub struct JsonConsumer {
    consumer: Consumer<ResultGroup>,
    counter: usize,
    cache: ResultGroup,
}

impl Iterator for JsonConsumer {
    type Item = JsonResult<ElementInfo>;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.counter % GROUP_SIZE;
        if index == 0 {
            let mut r = self.consumer.pop();
            loop {
                match r {
                    Ok(v) => {
                        self.cache = v;
                        break;
                    }
                    Err(e) => {
                        sleep_ns(50);
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

fn new_group() -> ResultGroup {
    // let v: Vec<OptRJson> = (0..GROUP_SIZE).map(|_| None).collect();
    // v.try_into().unwrap()
    Default::default()
}

fn sleep_ns(nanos: u64) {
    sleep(Duration::from_nanos(nanos));
}
