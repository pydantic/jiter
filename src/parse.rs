use std::intrinsics::{likely, unlikely};
use std::ops::Range;

use crate::element::{Element, Exponent, JsonError, JsonResult};
use crate::FilePosition;

#[derive(Debug, Clone)]
pub struct Parser<'a> {
    data: &'a [u8],
    length: usize,
    pub index: usize,
    last: usize,
}

impl<'a> Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            length: data.len(),
            index: 0,
            last: 0,
        }
    }
}

static TRUE_REST: [u8; 3] = [b'r', b'u', b'e'];
static FALSE_REST: [u8; 4] = [b'a', b'l', b's', b'e'];
static NULL_REST: [u8; 3] = [b'u', b'l', b'l'];

impl<'a> Parser<'a> {
    pub fn current_position(&self) -> FilePosition {
        FilePosition::find(self.data, self.index)
    }

    pub fn last_position(&self) -> FilePosition {
        FilePosition::find(self.data, self.last)
    }

    /// we should enable PGO, then add `#[inline(always)]` so this method can be optimised
    /// for each call from Fleece.
    pub fn next_value(&mut self) -> JsonResult<Element> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => {
                    self.index += 1
                },
                b'[' => {
                    self.last = self.index;
                    self.index += 1;
                    return Ok(Element::ArrayStart);
                }
                b'{' => {
                    self.last = self.index;
                    self.index += 1;
                    return Ok(Element::ObjectStart);
                }
                b'"' => {
                    return match self.next_string() {
                        Ok(range) => Ok(Element::String(range)),
                        Err(e) => Err(e),
                    }
                }
                b't' => return self.next_true(),
                b'f' => return self.next_false(),
                b'n' => return self.next_null(),
                b'0'..=b'9' => return self.next_number(true),
                b'-' => return self.next_number(false),
                _ => return Err(JsonError::UnexpectedCharacter),
            }
        }
        Err(JsonError::End)
    }

    pub fn array_first(&mut self) -> JsonResult<bool> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => {
                    self.index += 1
                }
                b']' => {
                    self.index += 1;
                    return Ok(false)
                }
                _ => return Ok(true),
            }
        }
        Err(JsonError::UnexpectedEnd)
    }

    pub fn array_step(&mut self) -> JsonResult<bool> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => {
                    self.index += 1
                }
                b',' => {
                    self.index += 1;
                    return Ok(true)
                },
                b']' => {
                    self.index += 1;
                    return Ok(false)
                }
                _ => return Err(JsonError::UnexpectedCharacter),
            }
        }
        Err(JsonError::End)
    }

    pub fn object_first(&mut self) -> JsonResult<Option<Range<usize>>> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => {
                    self.index += 1
                }
                b'}' => {
                    self.index += 1;
                    return Ok(None);
                },
                b'"' => return self.object_key(),
                _ => return Err(JsonError::UnexpectedCharacter),
            }
        }
        Err(JsonError::UnexpectedEnd)
    }

    pub fn object_step(&mut self) -> JsonResult<Option<Range<usize>>> {
        loop {
            if let Some(next) = self.data.get(self.index) {
                match next {
                    b' ' | b'\r' | b'\t' | b'\n' => {
                        self.index += 1
                    }
                    b',' => {
                        self.index += 1;
                        while let Some(next) = self.data.get(self.index) {
                            match next {
                                b' ' | b'\r' | b'\t'| b'\n' => (),
                                b'"' => return self.object_key(),
                                _ => return Err(JsonError::UnexpectedCharacter),
                            }
                            self.index += 1;
                        }
                        return Err(JsonError::UnexpectedEnd)
                    },
                    b'}' => {
                        self.index += 1;
                        return Ok(None);
                    },
                    _ => return Err(JsonError::UnexpectedCharacter),
                }
            } else {
                return Err(JsonError::UnexpectedEnd)
            }
        }
    }

    pub fn finish(&mut self) -> JsonResult<()> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => {
                    self.index += 1
                }
                _ => { return Err(JsonError::UnexpectedCharacter); },
            }
        }
        Ok(())
    }

    fn object_key(&mut self) -> JsonResult<Option<Range<usize>>> {
        let range = self.next_string()?;
        loop {
            if let Some(next) = self.data.get(self.index) {
                match next {
                    b' ' | b'\r' | b'\t' | b'\n' => {
                        self.index += 1
                    }
                    b':' => {
                        self.index += 1;
                        return Ok(Some(range));
                    },
                    _ => return Err(JsonError::UnexpectedCharacter),
                }
            } else {
                return Err(JsonError::UnexpectedEnd)
            }
        }
    }

    fn next_true(&mut self) -> JsonResult<Element> {
        self.last = self.index;
        if unlikely(self.index + 3 >= self.length) {
            return Err(JsonError::UnexpectedEnd);
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
            Ok(Element::True)
        } else {
            Err(JsonError::InvalidTrue)
        }
    }

    fn next_false(&mut self) -> JsonResult<Element> {
        self.last = self.index;
        if unlikely(self.index + 4 >= self.length) {
            return Err(JsonError::UnexpectedEnd);
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
            Ok(Element::False)
        } else {
            Err(JsonError::InvalidFalse)
        }
    }

    fn next_null(&mut self) -> JsonResult<Element> {
        self.last = self.index;
        if unlikely(self.index + 3 >= self.length) {
            return Err(JsonError::UnexpectedEnd);
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
            Ok(Element::Null)
        } else {
            Err(JsonError::InvalidNull)
        }
    }

    fn next_string(&mut self) -> JsonResult<Range<usize>> {
        self.last = self.index;
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
        Err(JsonError::UnexpectedEnd)
    }

    fn next_number(&mut self, positive: bool) -> JsonResult<Element> {
        self.last = self.index;
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
                    return Ok(element);
                }
                _ => break,
            }
            self.index += 1;
        }
        if start == self.index {
            Err(JsonError::InvalidNumber)
        } else {
            let element = Element::Int {
                positive,
                range: start..self.index,
                exponent: None,
            };
            Ok(element)
        }
    }

    fn float_decimal(&mut self, start: usize, positive: bool) -> JsonResult<Element> {
        let mut first = true;
        self.index += 1;
        let int_range = start..self.index - 1;
        let decimal_start = self.index;
        while let Some(next) = self.data.get(self.index) {
            match next {
                b'0'..=b'9' => (),
                b'e' | b'E' => {
                    return if first {
                        Err(JsonError::InvalidNumber)
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
                        Ok(element)
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
            let element = Element::Float {
                positive,
                int_range,
                decimal_range: decimal_start..self.index,
                exponent: None,
            };
            Ok(element)
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
}
