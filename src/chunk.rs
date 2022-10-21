use std::ops::Range;

use crate::{JsonResult, JsonError, ErrorInfo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exponent {
    pub positive: bool,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    End,
    ObjectStart,
    ObjectEnd,
    ArrayStart,
    ArrayEnd,
    True,
    False,
    Null,
    String(Range<usize>),
    Int {
        positive: bool,
        range: Range<usize>,
        exponent: Option<Exponent>,
    },
    Float {
        positive: bool,
        int_range: Range<usize>,
        decimal_range: Range<usize>,
        exponent: Option<Exponent>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkInfo {
    pub key: Option<Range<usize>>,
    pub chunk_type: Chunk,
    pub loc: (usize, usize),
}

#[derive(Debug, Copy, Clone)]
enum State {
    Start,
    Finished,
    StartArray,
    MidArray,
    StartObject,
    MidObject,
}

#[derive(Debug)]
pub struct Chunker<'a> {
    data: &'a [u8],
    length: usize,
    state_heap: Vec<State>,
    state: State,
    started: bool,
    index: usize,
    line: usize,
    col_offset: usize,
}

impl<'a> Chunker<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        return Self {
            data,
            length: data.len(),
            state_heap: vec![],
            state: State::Start,
            started: false,
            index: 0,
            line: 0,
            col_offset: 0,
        };
    }
}

impl<'a> Iterator for Chunker<'a> {
    type Item = JsonResult<ChunkInfo>;

    fn next(&mut self) -> Option<Self::Item> {
        self.eat_whitespace();

        let start_index = self.index;
        let result = match self.state {
            State::Start => self.parse_next(),
            State::StartArray => self.array_start(),
            State::MidArray => self.array_mid(),
            State::StartObject => self.object_start(),
            State::MidObject => self.object_mid(),
            State::Finished => return None,
        };

        let loc = (self.line, start_index - self.col_offset);
        match result {
            Ok((key, chunk_type)) => {
                if chunk_type == Chunk::End {
                    self.state = State::Finished;
                    None
                } else {
                    Some(Ok(ChunkInfo { key, chunk_type, loc }))
                }
            }
            Err(error_type) => Some(Err(ErrorInfo { error_type, loc })),
        }
    }
}

type ChunkerResult<T> = Result<T, JsonError>;

impl<'a> Chunker<'a> {
    fn eat_whitespace(&mut self) {
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b' ' | b'\r' | b'\t' => self.index += 1,
                b'\n' => {
                    self.index += 1;
                    self.line += 1;
                    self.col_offset = self.index;
                }
                _ => break,
            }
        }
    }

    // if we're in an array consume the next comma and whitespace
    fn array_start(&mut self) -> ChunkerResult<(Option<Range<usize>>, Chunk)> {
        if self.next_is(b']')? {
            self.index += 1;
            self.state = self.state_heap.pop().unwrap();
            Ok((None, Chunk::ArrayEnd))
        } else {
            self.state = State::MidArray;
            self.parse_next()
        }
    }

    // if we're in an array consume the next comma and whitespace
    fn array_mid(&mut self) -> ChunkerResult<(Option<Range<usize>>, Chunk)> {
        if self.index >= self.length {
            Err(JsonError::UnexpectedEnd)
        } else {
            let next = unsafe { self.data.get_unchecked(self.index) };
            if next == &b']' {
                self.index += 1;
                self.state = self.state_heap.pop().unwrap();
                Ok((None, Chunk::ArrayEnd))
            } else if next == &b',' {
                self.index += 1;
                self.eat_whitespace();
                self.parse_next()
            } else {
                Err(JsonError::ExpectingArrayNext)
            }
        }
    }

    fn object_start(&mut self) -> ChunkerResult<(Option<Range<usize>>, Chunk)> {
        if self.next_is(b'}')? {
            self.index += 1;
            self.state = self.state_heap.pop().unwrap();
            Ok((None, Chunk::ObjectEnd))
        } else {
            self.state = State::MidObject;
            self.object_next()
        }
    }

    fn object_mid(&mut self) -> ChunkerResult<(Option<Range<usize>>, Chunk)> {
        if self.index >= self.length {
            Err(JsonError::UnexpectedEnd)
        } else {
            let next = unsafe { self.data.get_unchecked(self.index) };
            if next == &b'}' {
                self.index += 1;
                self.state = self.state_heap.pop().unwrap();
                Ok((None, Chunk::ObjectEnd))
            } else if next == &b',' {
                self.index += 1;
                self.eat_whitespace();
                self.object_next()
            } else {
                Err(JsonError::ExpectingObjectNext)
            }
        }
    }

    fn object_next(&mut self) -> ChunkerResult<(Option<Range<usize>>, Chunk)> {
        if self.next_is(b'"')? {
            let string_range = self.parse_string()?;
            self.eat_whitespace();
            if self.next_is(b':')? {
                self.index += 1;
                self.eat_whitespace();
                let (_, value) = self.parse_next()?;
                Ok((Some(string_range), value))
            } else {
                Err(JsonError::ExpectingColon)
            }
        } else {
            Err(JsonError::ExpectingKey)
        }
    }

    fn parse_next(&mut self) -> ChunkerResult<(Option<Range<usize>>, Chunk)> {
        if self.index >= self.length {
            return match self.state {
                State::Start => {
                    if self.started {
                        Ok((None, Chunk::End))
                    } else {
                        Err(JsonError::UnexpectedEnd)
                    }
                }
                _ => Err(JsonError::UnexpectedEnd),
            };
        }

        let next = unsafe { self.data.get_unchecked(self.index) };
        let chunk_type = match next {
            b'{' => {
                self.index += 1;
                self.state_heap.push(self.state);
                self.state = State::StartObject;
                Ok(Chunk::ObjectStart)
            }
            b'[' => {
                self.index += 1;
                self.state_heap.push(self.state);
                self.state = State::StartArray;
                Ok(Chunk::ArrayStart)
            }
            b't' => self.parse_true(),
            b'f' => self.parse_false(),
            b'n' => self.parse_null(),
            b'"' => {
                let string_range = self.parse_string()?;
                Ok(Chunk::String(string_range))
            }
            b'0'..=b'9' => self.parse_number(true),
            b'-' => self.parse_number(false),
            _ => Err(JsonError::UnexpectedCharacter),
        }?;
        self.started = true;
        Ok((None, chunk_type))
    }

    fn parse_true(&mut self) -> ChunkerResult<Chunk> {
        if self.index + 3 >= self.length {
            return Err(JsonError::UnexpectedEnd);
        }
        // this could be a SIMD operation and possibly faster?
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'r' {
            return Err(JsonError::InvalidTrue);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'u' {
            return Err(JsonError::InvalidTrue);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'e' {
            return Err(JsonError::InvalidTrue);
        }
        self.index += 1;
        Ok(Chunk::True)
    }

    fn parse_false(&mut self) -> ChunkerResult<Chunk> {
        if self.index + 4 >= self.length {
            return Err(JsonError::UnexpectedEnd);
        }
        // this could be a SIMD operation and possibly faster?
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'a' {
            return Err(JsonError::InvalidFalse);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'l' {
            return Err(JsonError::InvalidFalse);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b's' {
            return Err(JsonError::InvalidFalse);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'e' {
            return Err(JsonError::InvalidFalse);
        }
        self.index += 1;
        Ok(Chunk::False)
    }

    fn parse_null(&mut self) -> ChunkerResult<Chunk> {
        if self.index + 3 >= self.length {
            return Err(JsonError::UnexpectedEnd);
        }
        // this could be a SIMD operation and possibly faster?
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'u' {
            return Err(JsonError::InvalidNull);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'l' {
            return Err(JsonError::InvalidNull);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'l' {
            return Err(JsonError::InvalidNull);
        }
        self.index += 1;
        Ok(Chunk::Null)
    }

    fn parse_string(&mut self) -> ChunkerResult<Range<usize>> {
        self.index += 1;
        let start = self.index;
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b'"' => {
                    let r = start..self.index;
                    self.index += 1;
                    return Ok(r);
                }
                b'\\' => {
                    self.index += 1;
                    if self.index >= self.length {
                        break;
                    }
                    let next = unsafe { self.data.get_unchecked(self.index) };
                    match next {
                        // we don't check the 4 digit unicode escape sequence here, we just move on
                        b'"' | b'\\' | b'/' | b'u' => (),
                        // 8 = backspace, 9 = tab, 10 = newline, 12 = formfeed, 13 = carriage return
                        8 | 9 | 10 | 12 | 13 => (),
                        _ => return Err(JsonError::InvalidString(self.index - start)),
                    }
                }
                // 8 = backspace, 9 = tab, 10 = newline, 12 = formfeed, 13 = carriage return
                8 | 9 | 10 | 12 | 13 => return Err(JsonError::InvalidString(self.index - start)),
                _ => (),
            }
            self.index += 1;
        }
        Err(JsonError::UnexpectedEnd)
    }

    fn parse_number(&mut self, positive: bool) -> ChunkerResult<Chunk> {
        let start: usize = if positive {
            self.index
        } else {
            // we started with a minus sign, so the first digit is at index + 1
            self.index + 1
        };
        self.index += 1;
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b'0'..=b'9' => (),
                b'.' => return self.float_decimal(start, positive),
                b'e' | b'E' => {
                    let end = self.index;
                    return Ok(Chunk::Int {
                        positive,
                        range: start..end,
                        exponent: Some(self.exponent()?),
                    });
                }
                _ => break,
            }
            self.index += 1;
        }
        if start == self.index {
            Err(JsonError::InvalidNumber)
        } else {
            Ok(Chunk::Int {
                positive,
                range: start..self.index,
                exponent: None,
            })
        }
    }

    fn float_decimal(&mut self, start: usize, positive: bool) -> ChunkerResult<Chunk> {
        let mut first = true;
        self.index += 1;
        let int_range = start..self.index - 1;
        let decimal_start = self.index;
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b'0'..=b'9' => (),
                b'e' | b'E' => {
                    return if first {
                        Err(JsonError::InvalidNumber)
                    } else {
                        let decimal_end = self.index;
                        Ok(Chunk::Float {
                            positive,
                            int_range,
                            decimal_range: decimal_start..decimal_end,
                            exponent: Some(self.exponent()?),
                        })
                    }
                }
                _ => break,
            }
            first = false;
            self.index += 1;
        }
        if decimal_start == self.index {
            Err(JsonError::InvalidNumber)
        } else {
            Ok(Chunk::Float {
                positive,
                int_range,
                decimal_range: decimal_start..self.index,
                exponent: None,
            })
        }
    }

    fn exponent(&mut self) -> ChunkerResult<Exponent> {
        let mut first = true;
        let mut positive = true;
        self.index += 1;
        let mut start = self.index;
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b'-' => {
                    if !first {
                        return Err(JsonError::InvalidNumber);
                    }
                    positive = false;
                    start += 1;
                }
                b'+' => {
                    if !first {
                        return Err(JsonError::InvalidNumber);
                    }
                    start += 1;
                }
                b'0'..=b'9' => (),
                _ => break,
            }
            first = false;
            self.index += 1;
        }

        if start == self.index {
            Err(JsonError::InvalidNumber)
        } else {
            Ok(Exponent {
                positive,
                range: start..self.index,
            })
        }
    }

    fn next_is(&self, byte: u8) -> ChunkerResult<bool> {
        if self.index >= self.length {
            Err(JsonError::UnexpectedEnd)
        } else {
            let next = unsafe { self.data.get_unchecked(self.index) };
            Ok(next == &byte)
        }
    }
}
