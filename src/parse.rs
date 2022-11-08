use std::intrinsics::{likely, unlikely};
use std::ops::Range;

use crate::element::{Element, ElementInfo, ErrorInfo, Exponent, JsonError, JsonResult, Location};

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
pub struct Parser<'a> {
    data: &'a [u8],
    length: usize,
    state_heap: Vec<State>,
    state: State,
    index: usize,
    line: usize,
    col_offset: usize,
}

impl<'a> Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            length: data.len(),
            state_heap: Vec::with_capacity(16),
            state: State::Start,
            index: 0,
            line: 1,
            col_offset: 0,
        }
    }
}

impl<'a> Iterator for Parser<'a> {
    type Item = JsonResult<ElementInfo>;

    fn next(&mut self) -> Option<Self::Item> {
        // annoyingly, doing it this way instead of calling a method which returns `JsonResult<Option<ElementInfo>>`
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

                    // `next_state` here refers to the state we'll be in after leaving this array
                    let next_state = match self.state {
                        State::Start => State::Finished,
                        State::ArrayStart | State::ArrayPostComma => State::ArrayPostValue,
                        State::ObjectPostColon => State::ObjectPostValue,
                        _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    };
                    self.state_heap.push(next_state);
                    self.state = State::ArrayStart;
                    self.index += 1;
                    return ElementInfo::next(Element::ArrayStart, loc);
                }
                b',' => {
                    self.state = match self.state {
                        State::ArrayPostValue => State::ArrayPostComma,
                        State::ObjectPostValue => State::ObjectPostComma,
                        _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
                    }
                }
                b']' => {
                    let loc = self.loc();
                    return match self.state {
                        State::ArrayStart | State::ArrayPostValue => {
                            self.state = self.state_heap.pop().unwrap();
                            self.index += 1;
                            ElementInfo::next(Element::ArrayEnd, loc)
                        }
                        _ => ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    };
                }
                b'{' => {
                    let loc = self.loc();

                    // `next_state` here refers to the state we'll be in after leaving this array
                    let next_state = match self.state {
                        State::Start => State::Finished,
                        State::ArrayStart | State::ArrayPostComma => State::ArrayPostValue,
                        State::ObjectPostColon => State::ObjectPostValue,
                        _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, loc),
                    };
                    self.state_heap.push(next_state);
                    self.state = State::ObjectStart;
                    self.index += 1;
                    return ElementInfo::next(Element::ObjectStart, loc);
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
                            ElementInfo::next(Element::ObjectEnd, loc)
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
                                Ok(range) => ElementInfo::next(Element::Key(range), loc),
                                Err(e) => Some(Err(e)),
                            }
                        }
                        _ => match self.on_value() {
                            None => {
                                let range = match self.next_string(loc) {
                                    Ok(range) => range,
                                    Err(e) => return Some(Err(e)),
                                };
                                ElementInfo::next(Element::String(range), loc)
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

impl<'a> Parser<'a> {
    fn loc(&self) -> Location {
        (self.line, self.index - self.col_offset + 1)
    }

    fn on_value(&mut self) -> Option<ErrorInfo> {
        self.state = match self.state {
            State::Start => State::Finished,
            State::ArrayStart | State::ArrayPostComma => State::ArrayPostValue,
            State::ObjectPostColon => State::ObjectPostValue,
            _ => return Some(ErrorInfo::new(JsonError::UnexpectedCharacter, self.loc())),
        };
        None
    }

    fn next_true(&mut self) -> Option<JsonResult<ElementInfo>> {
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
            ElementInfo::next(Element::True, loc)
        } else {
            ErrorInfo::next(JsonError::InvalidTrue, loc)
        }
    }

    fn next_false(&mut self) -> Option<JsonResult<ElementInfo>> {
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
            ElementInfo::next(Element::False, loc)
        } else {
            ErrorInfo::next(JsonError::InvalidFalse, loc)
        }
    }

    fn next_null(&mut self) -> Option<JsonResult<ElementInfo>> {
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
            ElementInfo::next(Element::Null, loc)
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
                    self.index += 2;
                    // we don't do any further checks on the next character here,
                    // instead we leave checks to string decoding
                }
                // similarly, we don't check for control characters here and just leave it to decoding
                _ => {
                    self.index += 1;
                }
            }
        }
        Err(ErrorInfo::new(JsonError::UnexpectedEnd, loc))
    }

    fn next_number(&mut self, positive: bool) -> Option<JsonResult<ElementInfo>> {
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
                    let element = Element::Int {
                        positive,
                        range: start..end,
                        exponent,
                    };
                    return ElementInfo::next(element, loc);
                }
                _ => break,
            }
            self.index += 1;
        }
        if start == self.index {
            ErrorInfo::next(JsonError::InvalidNumber, loc)
        } else {
            let element = Element::Int {
                positive,
                range: start..self.index,
                exponent: None,
            };
            ElementInfo::next(element, loc)
        }
    }

    fn float_decimal(&mut self, start: usize, positive: bool) -> Option<JsonResult<ElementInfo>> {
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
                        let element = Element::Float {
                            positive,
                            int_range,
                            decimal_range: decimal_start..decimal_end,
                            exponent,
                        };
                        ElementInfo::next(element, loc)
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
            let element = Element::Float {
                positive,
                int_range,
                decimal_range: decimal_start..self.index,
                exponent: None,
            };
            ElementInfo::next(element, loc)
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
