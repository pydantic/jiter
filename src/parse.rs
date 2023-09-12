use crate::errors::{json_err, FilePosition, JsonResult};
use crate::number_decoder::AbstractNumberDecoder;
use crate::string_decoder::{AbstractStringDecoder, Tape};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Peak {
    Null,
    True,
    False,
    // we keep the first character of the number as we'll need it when decoding
    Num(u8),
    String,
    Array,
    Object,
}

impl Peak {
    fn new(next: u8, index: usize) -> JsonResult<Self> {
        match next {
            b'[' => Ok(Self::Array),
            b'{' => Ok(Self::Object),
            b'"' => Ok(Self::String),
            b't' => Ok(Self::True),
            b'f' => Ok(Self::False),
            b'n' => Ok(Self::Null),
            b'0'..=b'9' => Ok(Self::Num(next)),
            b'-' => Ok(Self::Num(next)),
            _ => json_err!(UnexpectedCharacter, index),
        }
    }
}

static TRUE_REST: [u8; 3] = [b'r', b'u', b'e'];
static FALSE_REST: [u8; 4] = [b'a', b'l', b's', b'e'];
static NULL_REST: [u8; 3] = [b'u', b'l', b'l'];

#[derive(Debug, Clone)]
pub struct Parser<'a> {
    data: &'a [u8],
    pub index: usize,
}

impl<'a> Parser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, index: 0 }
    }
}

impl<'a> Parser<'a> {
    pub fn current_position(&self) -> FilePosition {
        FilePosition::find(self.data, self.index)
    }

    pub fn error_position(&self, index: usize) -> FilePosition {
        FilePosition::find(self.data, index)
    }

    /// we should enable PGO, then add `#[inline(always)]` so this method can be optimised
    /// for each call from Jiter.
    pub fn peak(&mut self) -> JsonResult<Peak> {
        if let Some(next) = self.eat_whitespace() {
            Peak::new(next, self.index)
        } else {
            json_err!(UnexpectedEnd, self.index)
        }
    }

    pub fn array_first(&mut self) -> JsonResult<Option<Peak>> {
        self.index += 1;
        if let Some(next) = self.eat_whitespace() {
            if next == b']' {
                self.index += 1;
                Ok(None)
            } else {
                Peak::new(next, self.index).map(Some)
            }
        } else {
            json_err!(UnexpectedEnd, self.index)
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
                _ => json_err!(UnexpectedCharacter, self.index),
            }
        } else {
            json_err!(UnexpectedEnd, self.index)
        }
    }

    pub fn object_first<'s, 't, D: AbstractStringDecoder<'t>>(
        &'s mut self,
        tape: &'t mut Tape,
    ) -> JsonResult<Option<D::Output>>
    where
        's: 't,
    {
        self.index += 1;
        if let Some(next) = self.eat_whitespace() {
            match next {
                b'"' => self.object_key::<D>(tape).map(Some),
                b'}' => {
                    self.index += 1;
                    Ok(None)
                }
                _ => json_err!(UnexpectedCharacter, self.index),
            }
        } else {
            json_err!(UnexpectedEnd, self.index)
        }
    }

    pub fn object_step<'s, 't, D: AbstractStringDecoder<'t>>(
        &'s mut self,
        tape: &'t mut Tape,
    ) -> JsonResult<Option<D::Output>>
    where
        's: 't,
    {
        if let Some(next) = self.eat_whitespace() {
            match next {
                b',' => {
                    self.index += 1;
                    if let Some(next) = self.eat_whitespace() {
                        if next == b'"' {
                            self.object_key::<D>(tape).map(Some)
                        } else {
                            json_err!(UnexpectedCharacter, self.index)
                        }
                    } else {
                        json_err!(UnexpectedEnd, self.index)
                    }
                }
                b'}' => {
                    self.index += 1;
                    Ok(None)
                }
                _ => json_err!(UnexpectedCharacter, self.index),
            }
        } else {
            json_err!(UnexpectedEnd, self.index)
        }
    }

    pub fn finish(&mut self) -> JsonResult<()> {
        if self.eat_whitespace().is_none() {
            Ok(())
        } else {
            json_err!(UnexpectedCharacter, self.index)
        }
    }

    pub fn consume_true(&mut self) -> JsonResult<()> {
        if self.index + 3 >= self.data.len() {
            return json_err!(UnexpectedEnd, self.index);
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
            json_err!(InvalidTrue, self.index)
        }
    }

    pub fn consume_false(&mut self) -> JsonResult<()> {
        if self.index + 4 >= self.data.len() {
            return json_err!(UnexpectedEnd, self.index);
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
            json_err!(InvalidFalse, self.index)
        }
    }

    pub fn consume_null(&mut self) -> JsonResult<()> {
        if self.index + 3 >= self.data.len() {
            return json_err!(UnexpectedEnd, self.index);
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
            json_err!(InvalidNull, self.index)
        }
    }

    pub fn consume_string<'s, 't, D: AbstractStringDecoder<'t>>(
        &'s mut self,
        tape: &'t mut Tape,
    ) -> JsonResult<D::Output>
    where
        's: 't,
    {
        let (output, index) = D::decode(self.data, self.index, tape)?;
        self.index = index;
        Ok(output)
    }

    pub fn consume_number<D: AbstractNumberDecoder>(&mut self, first: u8) -> JsonResult<D::Output> {
        let (output, index) = D::decode(self.data, self.index, first)?;
        self.index = index;
        Ok(output)
    }

    /// private method to get an object key, then consume the colon which should follow
    fn object_key<'s, 't, D: AbstractStringDecoder<'t>>(&'s mut self, tape: &'t mut Tape) -> JsonResult<D::Output>
    where
        's: 't,
    {
        let (output, index) = D::decode(self.data, self.index, tape)?;
        self.index = index;
        if let Some(next) = self.eat_whitespace() {
            if next == b':' {
                self.index += 1;
                Ok(output)
            } else {
                json_err!(UnexpectedCharacter, self.index)
            }
        } else {
            json_err!(UnexpectedEnd, self.index)
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
