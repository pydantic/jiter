use crate::errors::{json_err, JsonResult, LinePosition};
use crate::number_decoder::AbstractNumberDecoder;
use crate::string_decoder::{AbstractStringDecoder, Tape};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Peak(u8);

#[allow(non_upper_case_globals)] // while testing
impl Peak {
    pub const Null: Self = Self(b'n');
    pub const True: Self = Self(b't');
    pub const False: Self = Self(b'f');
    pub const Zero: Self = Self(b'0');
    pub const One: Self = Self(b'1');
    pub const Two: Self = Self(b'2');
    pub const Three: Self = Self(b'3');
    pub const Four: Self = Self(b'4');
    pub const Five: Self = Self(b'5');
    pub const Six: Self = Self(b'6');
    pub const Seven: Self = Self(b'7');
    pub const Eight: Self = Self(b'8');
    pub const Nine: Self = Self(b'9');
    pub const Minus: Self = Self(b'-');
    pub const Plus: Self = Self(b'+');
    pub const Infinity: Self = Self(b'I');
    pub const NaN: Self = Self(b'N');
    pub const String: Self = Self(b'"');
    pub const Array: Self = Self(b'[');
    pub const Object: Self = Self(b'{');
}

impl Peak {
    const fn new(next: u8) -> Self {
        Self(next)
    }

    pub const fn is_num(self) -> bool {
        matches!(
            self,
            Self::Zero
                | Self::One
                | Self::Two
                | Self::Three
                | Self::Four
                | Self::Five
                | Self::Six
                | Self::Seven
                | Self::Eight
                | Self::Nine
                | Self::Minus
                | Self::Plus
                | Self::Infinity
                | Self::NaN
        )
    }

    pub const fn into_inner(self) -> u8 {
        self.0
    }
}

static TRUE_REST: [u8; 3] = [b'r', b'u', b'e'];
static FALSE_REST: [u8; 4] = [b'a', b'l', b's', b'e'];
static NULL_REST: [u8; 3] = [b'u', b'l', b'l'];
static NAN_REST: [u8; 2] = [b'a', b'N'];
static INFINITY_REST: [u8; 7] = [b'n', b'f', b'i', b'n', b'i', b't', b'y'];

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
            Ok(Peak::new(next))
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
                Ok(Some(Peak::new(next)))
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
                    let next = self.array_peak()?;
                    if next.is_none() {
                        json_err!(TrailingComma, self.index)
                    } else {
                        Ok(next)
                    }
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

    fn consume_ident<const SIZE: usize>(&mut self, expected: [u8; SIZE]) -> JsonResult<()> {
        self.index = consume_ident(self.data, self.index, expected)?;
        Ok(())
    }

    fn array_peak(&mut self) -> JsonResult<Option<Peak>> {
        if let Some(next) = self.eat_whitespace() {
            match next {
                b']' => Ok(None),
                _ => Ok(Some(Peak::new(next))),
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

fn consume_ident<const SIZE: usize>(data: &[u8], mut index: usize, expected: [u8; SIZE]) -> JsonResult<usize> {
    match data.get(index + 1..index + SIZE + 1) {
        Some(s) if s == expected => Ok(index + SIZE + 1),
        // TODO very sadly iterating over expected cause extra branches in the generated assembly
        //   and is significantly slower than just returning an error
        _ => {
            index += 1;
            for c in expected.iter() {
                match data.get(index) {
                    Some(v) if v == c => index += 1,
                    Some(_) => return json_err!(ExpectedSomeIdent, index),
                    _ => break,
                }
            }
            json_err!(EofWhileParsingValue, index)
        }
    }
}
