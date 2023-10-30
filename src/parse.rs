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
    fn new(next: u8) -> Option<Self> {
        match next {
            b'[' => Some(Self::Array),
            b'{' => Some(Self::Object),
            b'"' => Some(Self::String),
            b't' => Some(Self::True),
            b'f' => Some(Self::False),
            b'n' => Some(Self::Null),
            b'0'..=b'9' => Some(Self::Num(next)),
            b'-' => Some(Self::Num(next)),
            _ => None,
        }
    }

    pub fn display_type(&self) -> &'static str {
        match self {
            Self::Null => "a null",
            Self::True => "a true",
            Self::False => "a false",
            Self::Num(_) => "a number",
            Self::String => "a string",
            Self::Array => "an array",
            Self::Object => "an object",
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

    pub fn peak(&mut self) -> JsonResult<Peak> {
        if let Some(next) = self.eat_whitespace() {
            match Peak::new(next) {
                Some(p) => Ok(p),
                None => json_err!(ExpectedSomeValue, self.index + 1),
            }
        } else {
            json_err!(EofWhileParsingValue, self.index)
        }
    }

    pub fn peak_array_step(&mut self) -> JsonResult<Peak> {
        if let Some(next) = self.eat_whitespace() {
            match Peak::new(next) {
                Some(p) => Ok(p),
                None => {
                    // if next is a `]`, we have a "trailing comma" error
                    if next == b']' {
                        json_err!(TrailingComma, self.index + 1)
                    } else {
                        json_err!(ExpectedSomeValue, self.index + 2)
                    }
                }
            }
        } else {
            json_err!(EofWhileParsingValue, self.index)
        }
    }

    pub fn array_first(&mut self) -> JsonResult<Option<Peak>> {
        self.index += 1;
        if let Some(next) = self.eat_whitespace() {
            if next == b']' {
                self.index += 1;
                Ok(None)
            } else {
                self.peak_array_step().map(Some)
            }
        } else {
            json_err!(EofWhileParsingList, self.index + 1)
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
                _ => json_err!(ExpectedListCommaOrEnd, self.index + 1),
            }
        } else {
            json_err!(EofWhileParsingList, self.index)
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
                _ => json_err!(KeyMustBeAString, self.index + 1),
            }
        } else {
            json_err!(EofWhileParsingObject, self.index)
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
                    match self.eat_whitespace() {
                        Some(b'"') => self.object_key::<D>(tape).map(Some),
                        Some(b'}') => json_err!(TrailingComma, self.index + 1),
                        Some(_) => json_err!(KeyMustBeAString, self.index + 1),
                        None => json_err!(EofWhileParsingValue, self.index),
                    }
                }
                b'}' => {
                    self.index += 1;
                    Ok(None)
                }
                _ => json_err!(ExpectedObjectCommaOrEnd, self.index + 1),
            }
        } else {
            json_err!(EofWhileParsingObject, self.index)
        }
    }

    pub fn finish(&mut self) -> JsonResult<()> {
        if self.eat_whitespace().is_none() {
            Ok(())
        } else {
            json_err!(TrailingCharacters, self.index)
        }
    }

    pub fn consume_true(&mut self) -> JsonResult<()> {
        self.consume_ident(TRUE_REST)
    }

    pub fn consume_false(&mut self) -> JsonResult<()> {
        self.consume_ident(FALSE_REST)
    }

    pub fn consume_null(&mut self) -> JsonResult<()> {
        self.consume_ident(NULL_REST)
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
                json_err!(ExpectedColon, self.index)
            }
        } else {
            json_err!(EofWhileParsingObject, self.index)
        }
    }

    fn consume_ident<const SIZE: usize>(&mut self, expected: [u8; SIZE]) -> JsonResult<()> {
        match self.data.get(self.index + 1..self.index + SIZE + 1) {
            Some(s) if s == expected => {
                self.index += SIZE + 1;
                Ok(())
            }
            _ => {
                self.index += 1;
                for c in expected.iter() {
                    match self.data.get(self.index) {
                        Some(v) if v == c => self.index += 1,
                        Some(_) => return json_err!(ExpectedSomeIdent, self.index),
                        _ => break,
                    }
                }
                json_err!(EofWhileParsingValue, self.data.len())
            }
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
