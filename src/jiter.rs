use crate::number_decoder::{NumberDecoder, NumberInt};
use crate::parse::Peak;
use crate::string_decoder::{StringDecoder, StringDecoderRange, Tape};
use crate::value::take_value;
use crate::{FilePosition, JsonError, JsonValue, NumberAny, Parser};

#[derive(Debug, Eq, PartialEq)]
pub enum JsonType {
    Null,
    Bool,
    Int,
    Float,
    String,
    Array,
    Object,
}

#[derive(Debug, Eq, PartialEq)]
pub enum JiterError {
    JsonError {
        error: JsonError,
        position: FilePosition,
    },
    WrongType {
        expected: JsonType,
        actual: JsonType,
        position: FilePosition,
    },
    StringFormat(FilePosition),
    NumericValue(FilePosition),
    // StringFormatSpeedate{
    //     speedate_error: speedate::ParseError,
    //     loc: Location,
    // },
    ArrayEnd,
    ObjectEnd,
    EndReached,
    UnknownError(FilePosition),
}

pub type JiterResult<T> = Result<T, JiterError>;

macro_rules! map_err {
    ($self:ident, $error:ident) => {
        JiterError::JsonError {
            error: $error,
            position: FilePosition::new(0, 0),
        }
    };
}

pub struct Jiter<'a> {
    data: &'a [u8],
    parser: Parser<'a>,
    tape: Tape,
}

impl<'a> Jiter<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            parser: Parser::new(data),
            tape: Tape::default(),
        }
    }

    pub fn peak(&mut self) -> JiterResult<Peak> {
        self.parser.peak().map_err(|e| map_err!(self, e))
    }

    pub fn next_null(&mut self) -> JiterResult<()> {
        let peak = self.peak()?;
        match peak {
            Peak::Null => {
                self.parser.consume_null().map_err(|e| map_err!(self, e))?;
                Ok(())
            }
            _ => Err(self.wrong_type(JsonType::Null, peak)),
        }
    }

    pub fn next_bool(&mut self) -> JiterResult<bool> {
        let peak = self.peak()?;
        match peak {
            Peak::True => {
                self.parser.consume_true().map_err(|e| map_err!(self, e))?;
                Ok(true)
            }
            Peak::False => {
                self.parser.consume_false().map_err(|e| map_err!(self, e))?;
                Ok(false)
            }
            _ => Err(self.wrong_type(JsonType::Bool, peak)),
        }
    }

    pub fn next_int(&mut self) -> JiterResult<NumberInt> {
        let peak = self.peak()?;
        match peak {
            Peak::Num(first) => self.known_int(first),
            _ => Err(self.wrong_type(JsonType::Int, peak)),
        }
    }

    pub fn next_float(&mut self) -> JiterResult<f64> {
        let peak = self.peak()?;
        match peak {
            Peak::Num(positive) => self.known_float(positive).map(|n| n.into()),
            _ => Err(self.wrong_type(JsonType::Int, peak)),
        }
    }

    pub fn next_str(&mut self) -> JiterResult<&str> {
        let peak = self.peak()?;
        match peak {
            Peak::String => self.known_string(),
            _ => Err(self.wrong_type(JsonType::String, peak)),
        }
    }

    pub fn next_bytes(&mut self) -> JiterResult<&[u8]> {
        let peak = self.peak()?;
        match peak {
            Peak::String => {
                let range = self
                    .parser
                    .consume_string::<StringDecoderRange>(&mut self.tape)
                    .map_err(|e| map_err!(self, e))?;
                Ok(&self.data[range])
            }
            _ => Err(self.wrong_type(JsonType::String, peak)),
        }
    }

    pub fn next_value(&mut self) -> JiterResult<JsonValue> {
        let peak = self.peak()?;
        take_value(peak, &mut self.parser, &mut self.tape).map_err(|e| map_err!(self, e))
    }

    pub fn next_array(&mut self) -> JiterResult<Option<Peak>> {
        let peak = self.peak()?;
        match peak {
            Peak::Array => self.array_first(),
            _ => Err(self.wrong_type(JsonType::Array, peak)),
        }
    }

    pub fn array_first(&mut self) -> JiterResult<Option<Peak>> {
        self.parser.array_first().map_err(|e| map_err!(self, e))
    }

    pub fn array_step(&mut self) -> JiterResult<bool> {
        self.parser.array_step().map_err(|e| map_err!(self, e))
    }

    pub fn next_object(&mut self) -> JiterResult<Option<&str>> {
        let peak = self.peak()?;
        match peak {
            Peak::Object => self
                .parser
                .object_first::<StringDecoder<'_>>(&mut self.tape)
                .map_err(|e| map_err!(self, e)),
            _ => Err(self.wrong_type(JsonType::Object, peak)),
        }
    }

    pub fn next_key(&mut self) -> JiterResult<Option<&str>> {
        self.parser
            .object_step::<StringDecoder>(&mut self.tape)
            .map_err(|e| map_err!(self, e))
    }

    pub fn finish(&mut self) -> JiterResult<()> {
        self.parser.finish().map_err(|e| map_err!(self, e))
    }

    pub fn known_string(&mut self) -> JiterResult<&str> {
        self.parser
            .consume_string::<StringDecoder>(&mut self.tape)
            .map_err(|e| map_err!(self, e))
    }

    pub fn known_int(&mut self, first: u8) -> JiterResult<NumberInt> {
        self.parser
            .consume_number::<NumberDecoder<NumberInt>>(first)
            .map_err(|e| map_err!(self, e))
    }

    pub fn known_float(&mut self, first: u8) -> JiterResult<NumberAny> {
        self.parser
            .consume_number::<NumberDecoder<NumberAny>>(first)
            .map_err(|e| map_err!(self, e))
    }

    fn wrong_type(&self, expected: JsonType, peak: Peak) -> JiterError {
        let position = self.parser.current_position();
        match peak {
            Peak::True | Peak::False => JiterError::WrongType {
                expected,
                actual: JsonType::Bool,
                position,
            },
            Peak::Null => JiterError::WrongType {
                expected,
                actual: JsonType::Null,
                position,
            },
            Peak::String => JiterError::WrongType {
                expected,
                actual: JsonType::String,
                position,
            },
            Peak::Num(first) => self.wrong_num(first, expected),
            Peak::Array => JiterError::WrongType {
                expected,
                actual: JsonType::Array,
                position,
            },
            Peak::Object => JiterError::WrongType {
                expected,
                actual: JsonType::Object,
                position,
            },
        }
    }

    fn wrong_num(&self, first: u8, expected: JsonType) -> JiterError {
        let mut parser2 = self.parser.clone();
        let actual = match parser2.consume_number::<NumberDecoder<NumberAny>>(first) {
            Ok(NumberAny::Int { .. }) => JsonType::Int,
            Ok(NumberAny::Float { .. }) => JsonType::Float,
            Err(e) => {
                return {
                    JiterError::JsonError {
                        error: e,
                        position: parser2.current_position(),
                    }
                }
            }
        };
        JiterError::WrongType {
            expected,
            actual,
            position: self.parser.current_position(),
        }
    }
}
