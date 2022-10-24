use std::ops::Range;

use crate::parse::{parse_float, parse_int, parse_string};
use crate::{ErrorInfo, JsonError, JsonResult, Location};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exponent {
    pub positive: bool,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    ObjectStart,
    ObjectEnd,
    ArrayStart,
    ArrayEnd,
    True,
    False,
    Null,
    Key(Range<usize>),
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
    pub chunk_type: Chunk,
    pub loc: Location,
}

impl ChunkInfo {
    pub fn as_bool(&self) -> Option<bool> {
        match self.chunk_type {
            Chunk::True => Some(true),
            Chunk::False => Some(false),
            _ => None,
        }
    }

    fn next(chunk_type: Chunk, loc: Location) -> Option<JsonResult<Self>> {
        Some(Ok(Self { chunk_type, loc }))
    }

    pub fn is_null(&self) -> bool {
        matches!(self.chunk_type, Chunk::Null)
    }

    pub fn is_string(&self) -> bool {
        matches!(self.chunk_type, Chunk::String(_))
    }

    pub fn is_int(&self) -> bool {
        matches!(self.chunk_type, Chunk::Int { .. })
    }

    pub fn is_float(&self) -> bool {
        matches!(self.chunk_type, Chunk::Float { .. })
    }
}

#[derive(Debug, Copy, Clone)]
enum State {
    // new data to parse
    Start,
    // after `[`, expecting value or `]`
    ArrayStart,
    // after a value in an array, before `,` or `]`
    ArrayPostValue,
    // after `,` in an array, expecting a value
    ArrayPostComma,
    // after `{`, expecting a key or `}`
    ObjectStart,
    // after a key in an object, before `:`
    ObjectPreColon,
    // after `:`, expecting a value
    ObjectPostColon,
    // after a value in an object, before `,` or `}`
    ObjectPostValue,
    // after `,` in an object, expecting a key
    ObjectPostComma,
    // finishing parsing - state_heap is empty and we've parsed something
    Finished,
}

#[derive(Debug, Clone)]
pub struct Chunker<'a> {
    data: &'a [u8],
    length: usize,
    state_heap: Vec<State>,
    state: State,
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
            index: 0,
            line: 1,
            col_offset: 0,
        };
    }

    pub fn decode_string(&self, range: Range<usize>, loc: Location) -> JsonResult<String> {
        parse_string(&self.data, range).map_err(|e| ErrorInfo::new(e, loc))
    }

    pub fn decode_int(
        &self,
        positive: bool,
        range: Range<usize>,
        _exponent: Option<Exponent>,
        loc: Location,
    ) -> JsonResult<i64> {
        // assert!(exponent.is_none());
        parse_int(&self.data, positive, range).map_err(|e| ErrorInfo::new(e, loc))
    }

    pub fn decode_float(
        &self,
        positive: bool,
        int_range: Range<usize>,
        decimal_range: Range<usize>,
        _exponent: Option<Exponent>,
        loc: Location,
    ) -> JsonResult<f64> {
        // assert!(exponent.is_none());
        parse_float(&self.data, positive, int_range, decimal_range).map_err(|e| ErrorInfo::new(e, loc))
    }
}

impl<'a> Iterator for Chunker<'a> {
    type Item = JsonResult<ChunkInfo>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b' ' | b'\r' | b'\t' => (),
                b'\n' => {
                    self.line += 1;
                    self.col_offset = self.index + 1;
                }
                b'[' => {
                    let loc = self.loc();
                    return match self.state {
                        State::Start | State::ArrayPostComma | State::ObjectPostColon => {
                            self.push_state();
                            self.state = State::ArrayStart;
                            self.index += 1;
                            ChunkInfo::next(Chunk::ArrayStart, loc)
                        }
                        _ => ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    }
                }
                b',' => {
                    match self.state {
                        State::ArrayPostValue => {
                            self.state = State::ArrayPostComma;
                        }
                        State::ObjectPostValue => {
                            self.state = State::ObjectPostComma;
                        }
                        _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
                    }
                }
                b']' => {
                    let loc = self.loc();
                    return match self.state {
                        State::ArrayStart | State::ArrayPostValue => {
                            self.state = self.state_heap.pop().unwrap();
                            self.index += 1;
                            ChunkInfo::next(Chunk::ArrayEnd, loc)
                        }
                        _ => ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    }
                }
                b'{' => {
                    let loc = self.loc();
                    return match self.state {
                        State::Start | State::ArrayPostComma | State::ObjectPostColon => {
                            self.push_state();
                            self.state = State::ObjectStart;
                            self.index += 1;
                            ChunkInfo::next(Chunk::ObjectStart, loc)
                        }
                        _ => ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    }
                }
                b':' => {
                    match self.state {
                        State::ObjectPreColon => {
                            self.state = State::ObjectPostColon;
                        }
                        _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
                    }
                }
                b'}' => {
                    let loc = self.loc();
                    return match self.state {
                        State::ObjectStart | State::ObjectPostValue => {
                            self.state = self.state_heap.pop().unwrap();
                            self.index += 1;
                            ChunkInfo::next(Chunk::ObjectEnd, loc)
                        }
                        _ => ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    }
                }
                b'"' => {
                    let loc = self.loc();
                    return match self.state {
                        State::Start | State::ArrayStart | State::ArrayPostComma | State::ObjectPostColon => {
                            self.on_value();
                            let range = match self.next_string(loc) {
                                Ok(range) => range,
                                Err(e) => return Some(Err(e)),
                            };
                            ChunkInfo::next(Chunk::String(range), loc)
                        }
                        State::ObjectStart | State::ObjectPostComma => {
                            self.state = State::ObjectPreColon;
                            let loc = self.loc();
                            let range = match self.next_string(loc) {
                                Ok(range) => range,
                                Err(e) => return Some(Err(e)),
                            };
                            ChunkInfo::next(Chunk::Key(range), loc)
                        }
                        _ => ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    }
                }
                b't' => {
                    self.on_value();
                    return self.next_true()
                }
                b'f' => {
                    self.on_value();
                    return self.next_false()
                }
                b'n' => {
                    self.on_value();
                    return self.next_null()
                }
                b'0'..=b'9' => {
                    self.on_value();
                    return self.next_number(true)
                }
                b'-' => {
                    self.on_value();
                    return self.next_number(false)
                }
                _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
            }
            self.index += 1;
        }
        match self.state {
            State::Finished => None,
            _ => Some(Err(ErrorInfo::new(JsonError::UnexpectedEnd, self.loc()))),
        }
    }
}

impl<'a> Chunker<'a> {
    fn loc(&self) -> Location {
        (self.line, self.index - self.col_offset)
    }

    fn push_state(&mut self) {
        let s = match self.state {
            State::Start => State::Finished,
            State::ArrayPostComma => State::ArrayPostValue,
            State::ObjectPostColon => State::ObjectPostValue,
            _ => unreachable!(),
        };
        self.state_heap.push(s);
    }

    fn on_value(&mut self) {
        self.state = match self.state {
            State::Start => State::Finished,
            State::ArrayStart => State::ArrayPostValue,
            State::ArrayPostComma => State::ArrayPostValue,
            State::ObjectPostColon => State::ObjectPostValue,
            _ => unreachable!(),
        };
    }

    fn next_true(&mut self) -> Option<JsonResult<ChunkInfo>> {
        let loc = self.loc();
        if self.index + 3 >= self.length {
            return ErrorInfo::next(JsonError::UnexpectedEnd, loc);
        }
        // this could be a SIMD operation and possibly faster?
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'r' {
            return ErrorInfo::next(JsonError::InvalidTrue, loc);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'u' {
            return ErrorInfo::next(JsonError::InvalidTrue, loc);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'e' {
            return ErrorInfo::next(JsonError::InvalidTrue, loc);
        }
        self.index += 1;
        ChunkInfo::next(Chunk::True, loc)
    }

    fn next_false(&mut self) -> Option<JsonResult<ChunkInfo>> {
        let loc = self.loc();
        if self.index + 4 >= self.length {
            return ErrorInfo::next(JsonError::UnexpectedEnd, loc);
        }
        // this could be a SIMD operation and possibly faster?
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'a' {
            return ErrorInfo::next(JsonError::InvalidFalse, loc);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'l' {
            return ErrorInfo::next(JsonError::InvalidFalse, loc);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b's' {
            return ErrorInfo::next(JsonError::InvalidFalse, loc);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'e' {
            return ErrorInfo::next(JsonError::InvalidFalse, loc);
        }
        self.index += 1;
        ChunkInfo::next(Chunk::False, loc)
    }

    fn next_null(&mut self) -> Option<JsonResult<ChunkInfo>> {
        let loc = self.loc();
        if self.index + 3 >= self.length {
            return ErrorInfo::next(JsonError::UnexpectedEnd, loc);
        }
        // this could be a SIMD operation and possibly faster?
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'u' {
            return ErrorInfo::next(JsonError::InvalidNull, loc);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'l' {
            return ErrorInfo::next(JsonError::InvalidNull, loc);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'l' {
            return ErrorInfo::next(JsonError::InvalidNull, loc);
        }
        self.index += 1;
        ChunkInfo::next(Chunk::Null, loc)
    }

    fn next_string(&mut self, loc: Location) -> JsonResult<Range<usize>> {
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
                        // TODO we need to make sure the 4 characters after u are valid hex to confirm is valid JSON
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | b'u' => (),
                        _ => return Err(ErrorInfo::new(JsonError::InvalidString(self.index - start), loc)),
                    }
                }
                // 8 = backspace, 9 = tab, 10 = newline, 12 = formfeed, 13 = carriage return
                8 | 9 | 10 | 12 | 13 => return Err(ErrorInfo::new(JsonError::InvalidString(self.index - start), loc)),
                _ => (),
            }
            self.index += 1;
        }
        Err(ErrorInfo::new(JsonError::UnexpectedEnd, loc))
    }

    fn next_number(&mut self, positive: bool) -> Option<JsonResult<ChunkInfo>> {
        let loc = self.loc();
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
                    // TODO cope with case where this is the first character
                    let end = self.index;
                    let exponent = match self.exponent() {
                        Ok(exponent) => Some(exponent),
                        Err(e) => return Some(Err(e)),
                    };
                    let chunk = Chunk::Int {
                        positive,
                        range: start..end,
                        exponent
                    };
                    return ChunkInfo::next(chunk, loc);
                }
                _ => break,
            }
            self.index += 1;
        }
        if start == self.index {
            ErrorInfo::next(JsonError::InvalidNumber, loc)
        } else {
            let chunk = Chunk::Int {
                positive,
                range: start..self.index,
                exponent: None,
            };
            return ChunkInfo::next(chunk, loc);
        }
    }

    fn float_decimal(&mut self, start: usize, positive: bool) -> Option<JsonResult<ChunkInfo>> {
        let loc = self.loc();
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
                        ErrorInfo::next(JsonError::InvalidNumber, loc)
                    } else {
                        let decimal_end = self.index;
                        let exponent = match self.exponent() {
                            Ok(exponent) => Some(exponent),
                            Err(e) => return Some(Err(e)),
                        };
                        let chunk = Chunk::Float {
                            positive,
                            int_range,
                            decimal_range: decimal_start..decimal_end,
                            exponent,
                        };
                        ChunkInfo::next(chunk, loc)
                    }
                }
                _ => break,
            }
            first = false;
            self.index += 1;
        }
        if decimal_start == self.index {
            ErrorInfo::next(JsonError::InvalidNumber, loc)
        } else {
            let chunk = Chunk::Float {
                positive,
                int_range,
                decimal_range: decimal_start..self.index,
                exponent: None,
            };
            ChunkInfo::next(chunk, loc)
        }
    }

    fn exponent(&mut self) -> JsonResult<Exponent> {
        let mut first = true;
        let mut positive = true;
        self.index += 1;
        let mut start = self.index;
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b'-' => {
                    if !first {
                        return Err(ErrorInfo::new(JsonError::InvalidNumber, self.loc()));
                    }
                    positive = false;
                    start += 1;
                }
                b'+' => {
                    if !first {
                        return Err(ErrorInfo::new(JsonError::InvalidNumber, self.loc()));
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
            Err(ErrorInfo::new(JsonError::InvalidNumber, self.loc()))
        } else {
            Ok(Exponent {
                positive,
                range: start..self.index,
            })
        }
    }
}
