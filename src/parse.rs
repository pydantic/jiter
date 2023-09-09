use strum::{Display, EnumMessage};

use crate::number_decoder::AbstractNumberDecoder;
use crate::string_decoder::AbstractStringDecoder;
use crate::FilePosition;

#[derive(Debug, Clone)]
pub struct Parser<'a> {
    data: &'a [u8],
    length: usize,
    pub index: usize,
}

impl<'a> Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            length: data.len(),
            index: 0,
        }
    }
}

#[derive(Debug, Display, EnumMessage, PartialEq, Eq, Clone)]
#[strum(serialize_all = "snake_case")]
pub enum JsonError {
    UnexpectedCharacter,
    UnexpectedEnd,
    InvalidTrue,
    InvalidFalse,
    InvalidNull,
    InvalidString(usize),
    InvalidStringEscapeSequence(usize),
    InvalidNumber,
    IntTooLarge,
    FloatExpectingInt,
    InternalError,
}

pub type JsonResult<T> = Result<T, JsonError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Peak {
    Null,
    True,
    False,
    Num(u8),
    String,
    Array,
    Object,
}

static TRUE_REST: [u8; 3] = [b'r', b'u', b'e'];
static FALSE_REST: [u8; 4] = [b'a', b'l', b's', b'e'];
static NULL_REST: [u8; 3] = [b'u', b'l', b'l'];

impl<'a> Parser<'a> {
    pub fn current_position(&self) -> FilePosition {
        FilePosition::find(self.data, self.index)
    }

    /// we should enable PGO, then add `#[inline(always)]` so this method can be optimised
    /// for each call from Jiter.
    pub fn peak(&mut self) -> JsonResult<Peak> {
        if let Some(next) = self.eat_whitespace() {
            match next {
                b'[' => Ok(Peak::Array),
                b'{' => Ok(Peak::Object),
                b'"' => Ok(Peak::String),
                b't' => Ok(Peak::True),
                b'f' => Ok(Peak::False),
                b'n' => Ok(Peak::Null),
                b'0'..=b'9' => Ok(Peak::Num(next)),
                b'-' => Ok(Peak::Num(next)),
                _ => Err(JsonError::UnexpectedCharacter),
            }
        } else {
            Err(JsonError::UnexpectedEnd)
        }
    }

    pub fn array_first(&mut self) -> JsonResult<bool> {
        self.index += 1;
        if let Some(next) = self.eat_whitespace() {
            if next == b']' {
                self.index += 1;
                Ok(false)
            } else {
                Ok(true)
            }
        } else {
            Err(JsonError::UnexpectedEnd)
        }
    }

    pub fn array_step(&mut self) -> JsonResult<bool> {
        if let Some(next) = self.eat_whitespace() {
            match next {
                b',' => {
                    self.index += 1;
                    Ok(true)
                }
                b']' => {
                    self.index += 1;
                    Ok(false)
                }
                _ => Err(JsonError::UnexpectedCharacter),
            }
        } else {
            Err(JsonError::UnexpectedEnd)
        }
    }

    pub fn object_first<D: AbstractStringDecoder>(&mut self) -> JsonResult<Option<D::Output>> {
        self.index += 1;
        if let Some(next) = self.eat_whitespace() {
            match next {
                b'"' => self.object_key::<D>().map(Some),
                b'}' => {
                    self.index += 1;
                    Ok(None)
                }
                _ => Err(JsonError::UnexpectedCharacter),
            }
        } else {
            Err(JsonError::UnexpectedEnd)
        }
    }

    pub fn object_step<D: AbstractStringDecoder>(&mut self) -> JsonResult<Option<D::Output>> {
        if let Some(next) = self.eat_whitespace() {
            match next {
                b',' => {
                    self.index += 1;
                    if let Some(next) = self.eat_whitespace() {
                        if next == b'"' {
                            self.object_key::<D>().map(Some)
                        } else {
                            Err(JsonError::UnexpectedCharacter)
                        }
                    } else {
                        Err(JsonError::UnexpectedEnd)
                    }
                }
                b'}' => {
                    self.index += 1;
                    Ok(None)
                }
                _ => Err(JsonError::UnexpectedCharacter),
            }
        } else {
            Err(JsonError::UnexpectedEnd)
        }
    }

    pub fn finish(&mut self) -> JsonResult<()> {
        if self.eat_whitespace().is_none() {
            Ok(())
        } else {
            Err(JsonError::UnexpectedCharacter)
        }
    }

    pub fn consume_true(&mut self) -> JsonResult<()> {
        if self.index + 3 >= self.length {
            return Err(JsonError::UnexpectedEnd);
        }
        let v = unsafe {
            [
                *self.data.get_unchecked(self.index + 1),
                *self.data.get_unchecked(self.index + 2),
                *self.data.get_unchecked(self.index + 3),
            ]
        };
        if v == TRUE_REST {
            self.index += 4;
            Ok(())
        } else {
            Err(JsonError::InvalidTrue)
        }
    }

    pub fn consume_false(&mut self) -> JsonResult<()> {
        if self.index + 4 >= self.length {
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
        if v == FALSE_REST {
            self.index += 5;
            Ok(())
        } else {
            Err(JsonError::InvalidFalse)
        }
    }

    pub fn consume_null(&mut self) -> JsonResult<()> {
        if self.index + 3 >= self.length {
            return Err(JsonError::UnexpectedEnd);
        }
        let v = unsafe {
            [
                *self.data.get_unchecked(self.index + 1),
                *self.data.get_unchecked(self.index + 2),
                *self.data.get_unchecked(self.index + 3),
            ]
        };
        if v == NULL_REST {
            self.index += 4;
            Ok(())
        } else {
            Err(JsonError::InvalidNull)
        }
    }

    pub fn consume_string<D: AbstractStringDecoder>(&mut self) -> JsonResult<D::Output> {
        let (output, index) = D::decode(self.data, self.index)?;
        self.index = index;
        Ok(output)
    }

    pub fn consume_number<D: AbstractNumberDecoder>(&mut self, first: u8) -> JsonResult<D::Output> {
        let (output, index) = D::decode(self.data, self.index, first)?;
        self.index = index;
        Ok(output)
    }

    /// private method to get an object key, then consume the colon which should follow
    fn object_key<D: AbstractStringDecoder>(&mut self) -> JsonResult<D::Output> {
        let output = self.consume_string::<D>()?;
        if let Some(next) = self.eat_whitespace() {
            match next {
                b':' => {
                    self.index += 1;
                    Ok(output)
                }
                _ => Err(JsonError::UnexpectedCharacter),
            }
        } else {
            Err(JsonError::UnexpectedEnd)
        }
    }

    fn eat_whitespace(&mut self) -> Option<u8> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => self.index += 1,
                _ => return Some(*next),
            }
        }
        None
    }
}
