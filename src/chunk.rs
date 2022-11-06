use std::fmt;
use std::intrinsics::{likely, unlikely};
use std::ops::Range;

use crate::{ErrorInfo, JsonError, JsonResult, Location};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exponent {
    pub positive: bool,
    pub range: Range<usize>,
}

impl fmt::Display for Exponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.positive {
            write!(f, "e+")?;
        } else {
            write!(f, "e-")?;
        }
        write!(f, "{:?}", self.range)
    }
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

impl Default for Chunk {
    fn default() -> Self {
        Chunk::Null
    }
}

impl fmt::Display for Chunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectStart => write!(f, "{{"),
            Self::ObjectEnd => write!(f, "}}"),
            Self::ArrayStart => write!(f, "["),
            Self::ArrayEnd => write!(f, "]"),
            Self::True => write!(f, "true"),
            Self::False => write!(f, "false"),
            Self::Null => write!(f, "null"),
            Self::Key(range) => write!(f, "Key({:?})", range),
            Self::String(range) => write!(f, "String({:?})", range),
            Self::Int {
                positive,
                range,
                exponent,
            } => {
                let prefix = if *positive { "+" } else { "-" };
                match exponent {
                    Some(exp) => write!(f, "{}Int({:?}{})", prefix, range, exp),
                    None => write!(f, "{}Int({:?})", prefix, range),
                }
            }
            Self::Float {
                positive,
                int_range,
                decimal_range,
                exponent,
            } => {
                let prefix = if *positive { "+" } else { "-" };
                match exponent {
                    Some(exp) => write!(f, "{}Float({:?}.{:?}{})", prefix, int_range, decimal_range, exp),
                    None => write!(f, "{}Float({:?}.{:?})", prefix, int_range, decimal_range),
                }
            }
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct ChunkInfo {
    pub chunk_type: Chunk,
    pub loc: Location,
}

impl fmt::Display for ChunkInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} @ {}:{}", self.chunk_type, self.loc.0, self.loc.1)
    }
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
        Self {
            data,
            length: data.len(),
            state_heap: vec![],
            state: State::Start,
            index: 0,
            line: 1,
            col_offset: 0,
        }
    }
}

impl<'a> Iterator for Chunker<'a> {
    type Item = JsonResult<ChunkInfo>;

    fn next(&mut self) -> Option<Self::Item> {
        // annoyingly, doing it this way instead of calling a method which returns `JsonResult<Option<ChunkInfo>>`
        // is significantly quicker so we keep it like this although it's uglier
        while let Some(next) = self.data.get(self.index) {
            match next {
                // this method is called from whitespace, more whitespace is always fine
                b' ' | b'\r' | b'\t' => (),
                b'\n' => {
                    self.line += 1;
                    self.col_offset = self.index + 1;
                }
                b'[' => {
                    let loc = self.loc();

                    let push_state = match self.state {
                        State::Start => State::Finished,
                        State::ArrayStart | State::ArrayPostComma => State::ArrayPostValue,
                        State::ObjectPostColon => State::ObjectPostValue,
                        _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    };
                    self.state_heap.push(push_state);
                    self.state = State::ArrayStart;
                    self.index += 1;
                    return ChunkInfo::next(Chunk::ArrayStart, loc);
                }
                b',' => match self.state {
                    State::ArrayPostValue => {
                        self.state = State::ArrayPostComma;
                    }
                    State::ObjectPostValue => {
                        self.state = State::ObjectPostComma;
                    }
                    _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
                },
                b']' => {
                    let loc = self.loc();
                    return match self.state {
                        State::ArrayStart | State::ArrayPostValue => {
                            self.state = self.state_heap.pop().unwrap();
                            self.index += 1;
                            ChunkInfo::next(Chunk::ArrayEnd, loc)
                        }
                        _ => ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    };
                }
                b'{' => {
                    let loc = self.loc();
                    let push_state = match self.state {
                        State::Start => State::Finished,
                        State::ArrayStart | State::ArrayPostComma => State::ArrayPostValue,
                        State::ObjectPostColon => State::ObjectPostValue,
                        _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    };
                    self.state_heap.push(push_state);
                    self.state = State::ObjectStart;
                    self.index += 1;
                    return ChunkInfo::next(Chunk::ObjectStart, loc);
                }
                b':' => match self.state {
                    State::ObjectPreColon => {
                        self.state = State::ObjectPostColon;
                    }
                    _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
                },
                b'}' => {
                    let loc = self.loc();
                    return match self.state {
                        State::ObjectStart | State::ObjectPostValue => {
                            self.state = self.state_heap.pop().unwrap();
                            self.index += 1;
                            ChunkInfo::next(Chunk::ObjectEnd, loc)
                        }
                        _ => ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    };
                }
                b'"' => {
                    let loc = self.loc();
                    return match self.state {
                        State::ObjectStart | State::ObjectPostComma => {
                            self.state = State::ObjectPreColon;
                            let loc = self.loc();
                            match self.next_string(loc) {
                                Ok(range) => ChunkInfo::next(Chunk::Key(range), loc),
                                Err(e) => Some(Err(e)),
                            }
                        }
                        _ => match self.on_value() {
                            None => {
                                let range = match self.next_string(loc) {
                                    Ok(range) => range,
                                    Err(e) => return Some(Err(e)),
                                };
                                ChunkInfo::next(Chunk::String(range), loc)
                            }
                            Some(e) => Some(Err(e)),
                        },
                    };
                }
                b't' => {
                    return match self.on_value() {
                        None => self.next_true(),
                        Some(e) => Some(Err(e)),
                    }
                }
                b'f' => {
                    return match self.on_value() {
                        None => self.next_false(),
                        Some(e) => Some(Err(e)),
                    };
                }
                b'n' => {
                    return match self.on_value() {
                        None => self.next_null(),
                        Some(e) => Some(Err(e)),
                    };
                }
                b'0'..=b'9' => {
                    return match self.on_value() {
                        None => self.next_number(true),
                        Some(e) => Some(Err(e)),
                    };
                }
                b'-' => {
                    return match self.on_value() {
                        None => self.next_number(false),
                        Some(e) => Some(Err(e)),
                    };
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

static TRUE_REST: [u8; 3] = [b'r', b'u', b'e'];
static FALSE_REST: [u8; 4] = [b'a', b'l', b's', b'e'];
static NULL_REST: [u8; 3] = [b'u', b'l', b'l'];

impl<'a> Chunker<'a> {
    fn loc(&self) -> Location {
        (self.line, self.index - self.col_offset + 1)
    }

    fn on_value(&mut self) -> Option<ErrorInfo> {
        self.state = match self.state {
            State::Start => State::Finished,
            State::ArrayStart => State::ArrayPostValue,
            State::ArrayPostComma => State::ArrayPostValue,
            State::ObjectPostColon => State::ObjectPostValue,
            _ => return Some(ErrorInfo::new(JsonError::UnexpectedCharacter, self.loc())),
        };
        None
    }

    fn next_true(&mut self) -> Option<JsonResult<ChunkInfo>> {
        let loc = self.loc();
        if unlikely(self.index + 3 >= self.length) {
            return ErrorInfo::next(JsonError::UnexpectedEnd, loc);
        }
        let v = unsafe {
            [
                *self.data.get_unchecked(self.index + 1),
                *self.data.get_unchecked(self.index + 2),
                *self.data.get_unchecked(self.index + 3),
            ]
        };
        if likely(v == TRUE_REST) {
            self.index += 4;
            ChunkInfo::next(Chunk::True, loc)
        } else {
            ErrorInfo::next(JsonError::InvalidTrue, loc)
        }
    }

    fn next_false(&mut self) -> Option<JsonResult<ChunkInfo>> {
        let loc = self.loc();
        if unlikely(self.index + 4 >= self.length) {
            return ErrorInfo::next(JsonError::UnexpectedEnd, loc);
        }
        let v = unsafe {
            [
                *self.data.get_unchecked(self.index + 1),
                *self.data.get_unchecked(self.index + 2),
                *self.data.get_unchecked(self.index + 3),
                *self.data.get_unchecked(self.index + 4),
            ]
        };
        if likely(v == FALSE_REST) {
            self.index += 5;
            ChunkInfo::next(Chunk::False, loc)
        } else {
            ErrorInfo::next(JsonError::InvalidFalse, loc)
        }
    }

    fn next_null(&mut self) -> Option<JsonResult<ChunkInfo>> {
        let loc = self.loc();
        if unlikely(self.index + 3 >= self.length) {
            return ErrorInfo::next(JsonError::UnexpectedEnd, loc);
        }
        let v = unsafe {
            [
                *self.data.get_unchecked(self.index + 1),
                *self.data.get_unchecked(self.index + 2),
                *self.data.get_unchecked(self.index + 3),
            ]
        };
        if likely(v == NULL_REST) {
            self.index += 4;
            ChunkInfo::next(Chunk::Null, loc)
        } else {
            ErrorInfo::next(JsonError::InvalidNull, loc)
        }
    }

    fn next_string(&mut self, loc: Location) -> JsonResult<Range<usize>> {
        self.index += 1;
        let start = self.index;
        while let Some(next) = self.data.get(self.index) {
            match next {
                b'"' => {
                    let r = start..self.index;
                    self.index += 1;
                    return Ok(r);
                }
                b'\\' => {
                    self.index += 1;
                    let next = match self.data.get(self.index) {
                        Some(n) => n,
                        None => break,
                    };
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
        while let Some(next) = self.data.get(self.index) {
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
                        exponent,
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
            ChunkInfo::next(chunk, loc)
        }
    }

    fn float_decimal(&mut self, start: usize, positive: bool) -> Option<JsonResult<ChunkInfo>> {
        let loc = self.loc();
        let mut first = true;
        self.index += 1;
        let int_range = start..self.index - 1;
        let decimal_start = self.index;
        while let Some(next) = self.data.get(self.index) {
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
        while let Some(next) = self.data.get(self.index) {
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
