use std::intrinsics::{likely, unlikely};
use std::ops::Range;

use crate::element::{Element, ElementInfo, ElementKey, ErrorInfo, Exponent, JsonError, JsonResult, Location};

#[derive(Debug, Clone)]
pub struct Parser<'a> {
    data: &'a [u8],
    length: usize,
    index: usize,
    line: usize,
    col_offset: usize,
}

impl<'a> Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            length: data.len(),
            index: 0,
            line: 1,
            col_offset: 0,
        }
    }
}

static TRUE_REST: [u8; 3] = [b'r', b'u', b'e'];
static FALSE_REST: [u8; 4] = [b'a', b'l', b's', b'e'];
static NULL_REST: [u8; 3] = [b'u', b'l', b'l'];

impl<'a> Parser<'a> {
    /// we should enable PGO, then add `#[inline(always)]` so this method can be optimised
    /// for each call from Fleece.
    pub fn next_value(&mut self) -> JsonResult<ElementInfo> {
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
                    self.index += 1;
                    return ElementInfo::next(Element::ArrayStart, loc);
                }
                b'{' => {
                    let loc = self.loc();
                    self.index += 1;
                    return ElementInfo::next(Element::ObjectStart, loc);
                }
                b'"' => {
                    let loc = self.loc();
                    return match self.next_string(loc) {
                        Ok(range) => ElementInfo::next(Element::String(range), loc),
                        Err(e) => Err(e),
                    }
                }
                b't' => return self.next_true(),
                b'f' => return self.next_false(),
                b'n' => return self.next_null(),
                b'0'..=b'9' => return self.next_number(true),
                b'-' => return self.next_number(false),
                _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
            }
            self.index += 1;
        }
        ErrorInfo::next(JsonError::End, self.loc())
    }

    pub fn array_first(&mut self) -> JsonResult<bool> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                // this method is called from whitespace, more whitespace is always fine
                b' ' | b'\r' | b'\t' => (),
                b'\n' => {
                    self.line += 1;
                    self.col_offset = self.index + 1;
                }
                b']' => {
                    self.index += 1;
                    return Ok(false)
                }
                _ => return Ok(true),
            }
            self.index += 1;
        }
        ErrorInfo::next(JsonError::UnexpectedEnd, self.loc())
    }

    pub fn array_step(&mut self) -> JsonResult<bool> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                // this method is called from whitespace, more whitespace is always fine
                b' ' | b'\r' | b'\t' => (),
                b'\n' => {
                    self.line += 1;
                    self.col_offset = self.index + 1;
                }
                b',' => {
                    self.index += 1;
                    return Ok(true)
                },
                b']' => {
                    self.index += 1;
                    return Ok(false)
                }
                _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
            }
            self.index += 1;
        }
        ErrorInfo::next(JsonError::End, self.loc())
    }

    pub fn object_first(&mut self) -> JsonResult<Option<ElementKey>> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                // this method is called from whitespace, more whitespace is always fine
                b' ' | b'\r' | b'\t' => (),
                b'\n' => {
                    self.line += 1;
                    self.col_offset = self.index + 1;
                },
                b'}' => {
                    self.index += 1;
                    return Ok(None);
                },
                b'"' => return self.object_key(),
                _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
            }
            self.index += 1;
        }
        ErrorInfo::next(JsonError::UnexpectedEnd, self.loc())
    }

    pub fn object_step(&mut self) -> JsonResult<Option<ElementKey>> {
        loop {
            if let Some(next) = self.data.get(self.index) {
                match next {
                    // this method is called from whitespace, more whitespace is always fine
                    b' ' | b'\r' | b'\t' => (),
                    b'\n' => {
                        self.line += 1;
                        self.col_offset = self.index + 1;
                    }
                    b',' => {
                        self.index += 1;
                        while let Some(next) = self.data.get(self.index) {
                            match next {
                                // this method is called from whitespace, more whitespace is always fine
                                b' ' | b'\r' | b'\t' => (),
                                b'\n' => {
                                    self.line += 1;
                                    self.col_offset = self.index + 1;
                                }
                                b'"' => return self.object_key(),
                                _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
                            }
                            self.index += 1;
                        }
                        return ErrorInfo::next(JsonError::UnexpectedEnd, self.loc())
                    },
                    b'}' => {
                        self.index += 1;
                        return Ok(None);
                    },
                    _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
                }
                self.index += 1;
            } else {
                return ErrorInfo::next(JsonError::UnexpectedEnd, self.loc())
            }
        }
    }

    pub fn finish(&mut self) -> JsonResult<()> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                // this method is called from whitespace, more whitespace is always fine
                b' ' | b'\r' | b'\t' => (),
                b'\n' => {
                    self.line += 1;
                    self.col_offset = self.index + 1;
                }
                _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
            }
            self.index += 1;
        }
        Ok(())
    }

    fn object_key(&mut self) -> JsonResult<Option<ElementKey>> {
        let loc = self.loc();
        let range = self.next_string(loc)?;
        let key = ElementKey { range, loc };
        loop {
            if let Some(next) = self.data.get(self.index) {
                match next {
                    b' ' | b'\r' | b'\t' => (),
                    b'\n' => {
                        self.line += 1;
                        self.col_offset = self.index + 1;
                    }
                    b':' => {
                        self.index += 1;
                        return Ok(Some(key));
                    },
                    _ => return ErrorInfo::next(JsonError::UnexpectedCharacter, self.loc()),
                }
                self.index += 1;
            } else {
                return ErrorInfo::next(JsonError::UnexpectedEnd, self.loc())
            }
        }
    }

    fn loc(&self) -> Location {
        (self.line, self.index - self.col_offset + 1)
    }

    fn next_true(&mut self) -> JsonResult<ElementInfo> {
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

    fn next_false(&mut self) -> JsonResult<ElementInfo> {
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

    fn next_null(&mut self) -> JsonResult<ElementInfo> {
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

    fn next_number(&mut self, positive: bool) -> JsonResult<ElementInfo> {
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
                        Err(e) => return Err(e),
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

    fn float_decimal(&mut self, start: usize, positive: bool) -> JsonResult<ElementInfo> {
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
                            Err(e) => return Err(e),
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
