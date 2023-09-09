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
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => {
                    self.index += 1;
                }
                b'[' => return Ok(Peak::Array),
                b'{' => return Ok(Peak::Object),
                b'"' => return Ok(Peak::String),
                b't' => return Ok(Peak::True),
                b'f' => return Ok(Peak::False),
                b'n' => return Ok(Peak::Null),
                b'0'..=b'9' => return Ok(Peak::Num(*next)),
                b'-' => return Ok(Peak::Num(*next)),
                _ => return Err(JsonError::UnexpectedCharacter),
            }
        }
        Err(JsonError::UnexpectedEnd)
    }

    pub fn array_first(&mut self) -> JsonResult<bool> {
        self.index += 1;
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => self.index += 1,
                b']' => {
                    self.index += 1;
                    return Ok(false);
                }
                _ => return Ok(true),
            }
        }
        Err(JsonError::UnexpectedEnd)
    }

    pub fn array_step(&mut self) -> JsonResult<bool> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => self.index += 1,
                b',' => {
                    self.index += 1;
                    return Ok(true);
                }
                b']' => {
                    self.index += 1;
                    return Ok(false);
                }
                _ => return Err(JsonError::UnexpectedCharacter),
            }
        }
        Err(JsonError::UnexpectedEnd)
    }

    pub fn object_first<D: AbstractStringDecoder>(&mut self) -> JsonResult<Option<D::Output>> {
        self.index += 1;
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => self.index += 1,
                b'}' => {
                    self.index += 1;
                    return Ok(None);
                }
                b'"' => return self.object_key::<D>(),
                _ => return Err(JsonError::UnexpectedCharacter),
            }
        }
        Err(JsonError::UnexpectedEnd)
    }

    pub fn object_step<D: AbstractStringDecoder>(&mut self) -> JsonResult<Option<D::Output>> {
        loop {
            if let Some(next) = self.data.get(self.index) {
                match next {
                    b' ' | b'\r' | b'\t' | b'\n' => self.index += 1,
                    b',' => {
                        self.index += 1;
                        while let Some(next) = self.data.get(self.index) {
                            match next {
                                b' ' | b'\r' | b'\t' | b'\n' => (),
                                b'"' => return self.object_key::<D>(),
                                _ => return Err(JsonError::UnexpectedCharacter),
                            }
                            self.index += 1;
                        }
                        return Err(JsonError::UnexpectedEnd);
                    }
                    b'}' => {
                        self.index += 1;
                        return Ok(None);
                    }
                    _ => return Err(JsonError::UnexpectedCharacter),
                }
            } else {
                return Err(JsonError::UnexpectedEnd);
            }
        }
    }

    fn object_key<D: AbstractStringDecoder>(&mut self) -> JsonResult<Option<D::Output>> {
        let output = self.consume_string::<D>()?;
        loop {
            if let Some(next) = self.data.get(self.index) {
                match next {
                    b' ' | b'\r' | b'\t' | b'\n' => self.index += 1,
                    b':' => {
                        self.index += 1;
                        return Ok(Some(output));
                    }
                    _ => return Err(JsonError::UnexpectedCharacter),
                }
            } else {
                return Err(JsonError::UnexpectedEnd);
            }
        }
    }

    pub fn finish(&mut self) -> JsonResult<()> {
        while let Some(next) = self.data.get(self.index) {
            match next {
                b' ' | b'\r' | b'\t' | b'\n' => self.index += 1,
                _ => return Err(JsonError::UnexpectedCharacter),
            }
        }
        Ok(())
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
}
