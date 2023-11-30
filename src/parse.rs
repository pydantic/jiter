use crate::errors::{json_err, JsonResult, LinePosition};
use crate::number_decoder::AbstractNumberDecoder;
use crate::string_decoder::{AbstractStringDecoder, Tape};

/// Enum used to describe the next expected value in JSON.
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
            // `-` negative, `I` Infinity, `N` NaN
            b'-' | b'I' | b'N' => Some(Self::Num(next)),
            _ => None,
        }
    }
}

static TRUE_REST: &[u8; 3] = b"rue";
static FALSE_REST: &[u8; 4] = b"alse";
static NULL_REST: &[u8; 3] = b"ull";
static NAN_REST: &[u8; 2] = b"aN";
static INFINITY_REST: &[u8; 7] = b"nfinity";

#[derive(Debug, Clone)]
pub(crate) struct Parser<'j> {
    data: &'j [u8],
    pub index: usize,
}

impl<'j> Parser<'j> {
    pub fn new(data: &'j [u8]) -> Self {
        Self { data, index: 0 }
    }
}

impl<'j> Parser<'j> {
    pub fn current_position(&self) -> LinePosition {
        LinePosition::find(self.data, self.index)
    }

    pub fn peak(&mut self) -> JsonResult<Peak> {
        if let Some(next) = self.eat_whitespace() {
            match Peak::new(next) {
                Some(p) => Ok(p),
                None => json_err!(ExpectedSomeValue, self.index),
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
                self.array_peak()
            }
        } else {
            json_err!(EofWhileParsingList, self.index)
        }
    }

    pub fn array_step(&mut self) -> JsonResult<Option<Peak>> {
        if let Some(next) = self.eat_whitespace() {
            match next {
                b',' => {
                    self.index += 1;
                    self.array_peak()
                }
                b']' => {
                    self.index += 1;
                    Ok(None)
                }
                _ => {
                    json_err!(ExpectedListCommaOrEnd, self.index)
                }
            }
        } else {
            json_err!(EofWhileParsingList, self.index)
        }
    }

    pub fn object_first<'t, D: AbstractStringDecoder<'t, 'j>>(
        &mut self,
        tape: &'t mut Tape,
    ) -> JsonResult<Option<D::Output>>
    where
        'j: 't,
    {
        self.index += 1;
        if let Some(next) = self.eat_whitespace() {
            match next {
                b'"' => self.object_key::<D>(tape).map(Some),
                b'}' => {
                    self.index += 1;
                    Ok(None)
                }
                _ => json_err!(KeyMustBeAString, self.index),
            }
        } else {
            json_err!(EofWhileParsingObject, self.index)
        }
    }

    pub fn object_step<'t, D: AbstractStringDecoder<'t, 'j>>(
        &mut self,
        tape: &'t mut Tape,
    ) -> JsonResult<Option<D::Output>>
    where
        'j: 't,
    {
        if let Some(next) = self.eat_whitespace() {
            match next {
                b',' => {
                    self.index += 1;
                    match self.eat_whitespace() {
                        Some(b'"') => self.object_key::<D>(tape).map(Some),
                        Some(b'}') => json_err!(TrailingComma, self.index),
                        Some(_) => json_err!(KeyMustBeAString, self.index),
                        None => json_err!(EofWhileParsingValue, self.index),
                    }
                }
                b'}' => {
                    self.index += 1;
                    Ok(None)
                }
                _ => json_err!(ExpectedObjectCommaOrEnd, self.index),
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

    pub fn consume_string<'t, D: AbstractStringDecoder<'t, 'j>>(&mut self, tape: &'t mut Tape) -> JsonResult<D::Output>
    where
        'j: 't,
    {
        let (output, index) = D::decode(self.data, self.index, tape)?;
        self.index = index;
        Ok(output)
    }

    pub fn consume_number<D: AbstractNumberDecoder>(
        &mut self,
        first: u8,
        allow_inf_nan: bool,
    ) -> JsonResult<D::Output> {
        let (output, index) = D::decode(self.data, self.index, first, allow_inf_nan)?;
        self.index = index;
        Ok(output)
    }

    /// private method to get an object key, then consume the colon which should follow
    fn object_key<'t, D: AbstractStringDecoder<'t, 'j>>(&mut self, tape: &'t mut Tape) -> JsonResult<D::Output>
    where
        'j: 't,
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

    fn consume_ident<const SIZE: usize>(&mut self, expected: &'static [u8; SIZE]) -> JsonResult<()> {
        self.index = consume_ident(self.data, self.index, expected)?;
        Ok(())
    }

    fn array_peak(&mut self) -> JsonResult<Option<Peak>> {
        if let Some(next) = self.eat_whitespace() {
            match Peak::new(next) {
                Some(p) => Ok(Some(p)),
                None => {
                    // if next is a `]`, we have a "trailing comma" error
                    if next == b']' {
                        json_err!(TrailingComma, self.index)
                    } else {
                        json_err!(ExpectedSomeValue, self.index)
                    }
                }
            }
        } else {
            json_err!(EofWhileParsingValue, self.index)
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

pub(crate) fn consume_infinity(data: &[u8], index: usize) -> JsonResult<usize> {
    consume_ident(data, index, INFINITY_REST)
}

pub(crate) fn consume_nan(data: &[u8], index: usize) -> JsonResult<usize> {
    consume_ident(data, index, NAN_REST)
}

fn consume_ident<const SIZE: usize>(data: &[u8], index: usize, expected: &'static [u8; SIZE]) -> JsonResult<usize> {
    let Some(slice) = data.get(index + 1..) else {
        return json_err!(EofWhileParsingValue, data.len());
    };

    let mut expected = expected.iter();

    if let Some(error) = slice.iter().zip(expected.by_ref()).position(|(a, b)| a != b) {
        return json_err!(ExpectedSomeIdent, index + 1 + error);
    }

    if expected.next().is_some() {
        return json_err!(EofWhileParsingValue, data.len());
    }

    Ok(index + 1 + SIZE)
}
