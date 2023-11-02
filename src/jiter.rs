use crate::errors::{FilePosition, JiterError, JsonType, DEFAULT_RECURSION_LIMIT};
use crate::number_decoder::{NumberAny, NumberFloat, NumberInt, NumberRange};
use crate::parse::{Parser, Peak};
use crate::string_decoder::{StringDecoder, StringDecoderRange, Tape};
use crate::value::{take_value, JsonValue};

pub type JiterResult<T> = Result<T, JiterError>;

pub struct Jiter<'j> {
    data: &'j [u8],
    parser: Parser<'j>,
    tape: Tape,
}

impl<'j> Jiter<'j> {
    pub fn new(data: &'j [u8]) -> Self {
        Self {
            data,
            parser: Parser::new(data),
            tape: Tape::default(),
        }
    }

    pub fn error_position(&self, error: &JiterError) -> FilePosition {
        FilePosition::find(self.data, error.index)
    }

    pub fn peak(&mut self) -> JiterResult<Peak> {
        self.parser.peak().map_err(Into::into)
    }

    pub fn next_null(&mut self) -> JiterResult<()> {
        let peak = self.peak()?;
        match peak {
            Peak::Null => {
                self.parser.consume_null()?;
                Ok(())
            }
            _ => Err(self.wrong_type(JsonType::Null, peak)),
        }
    }

    pub fn next_bool(&mut self) -> JiterResult<bool> {
        let peak = self.peak()?;
        self.known_bool(peak)
    }

    pub fn known_bool(&mut self, peak: Peak) -> JiterResult<bool> {
        match peak {
            Peak::True => {
                self.parser.consume_true()?;
                Ok(true)
            }
            Peak::False => {
                self.parser.consume_false()?;
                Ok(false)
            }
            _ => Err(self.wrong_type(JsonType::Bool, peak)),
        }
    }

    pub fn next_number(&mut self) -> JiterResult<NumberAny> {
        let peak = self.peak()?;
        match peak {
            Peak::Num(first) => self.known_number(first),
            _ => Err(self.wrong_type(JsonType::Int, peak)),
        }
    }

    pub fn known_number(&mut self, first: u8) -> JiterResult<NumberAny> {
        self.parser.consume_number::<NumberAny>(first).map_err(Into::into)
    }

    pub fn next_int(&mut self) -> JiterResult<NumberInt> {
        let peak = self.peak()?;
        self.known_int(peak)
    }

    pub fn known_int(&mut self, peak: Peak) -> JiterResult<NumberInt> {
        match peak {
            Peak::Num(first) => self.parser.consume_number::<NumberInt>(first).map_err(Into::into),
            _ => Err(self.wrong_type(JsonType::Int, peak)),
        }
    }

    pub fn next_float(&mut self) -> JiterResult<f64> {
        let peak = self.peak()?;
        self.known_float(peak)
    }

    pub fn known_float(&mut self, peak: Peak) -> JiterResult<f64> {
        match peak {
            Peak::Num(first) => self.parser.consume_number::<NumberFloat>(first).map_err(Into::into),
            _ => Err(self.wrong_type(JsonType::Int, peak)),
        }
    }

    pub fn next_number_bytes(&mut self) -> JiterResult<&[u8]> {
        let peak = self.peak()?;
        match peak {
            Peak::Num(first) => {
                let range = self.parser.consume_number::<NumberRange>(first)?;
                Ok(&self.data[range])
            }
            _ => Err(self.wrong_type(JsonType::Float, peak)),
        }
    }

    pub fn next_str(&mut self) -> JiterResult<&str> {
        let peak = self.peak()?;
        match peak {
            Peak::String => self.known_str(),
            _ => Err(self.wrong_type(JsonType::String, peak)),
        }
    }

    pub fn known_str(&mut self) -> JiterResult<&str> {
        match self.parser.consume_string::<StringDecoder>(&mut self.tape) {
            Ok(output) => Ok(output.as_str()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn next_bytes(&mut self) -> JiterResult<&[u8]> {
        let peak = self.peak()?;
        match peak {
            Peak::String => {
                let range = self.parser.consume_string::<StringDecoderRange>(&mut self.tape)?;
                Ok(&self.data[range])
            }
            _ => Err(self.wrong_type(JsonType::String, peak)),
        }
    }

    pub fn next_value(&mut self) -> JiterResult<JsonValue> {
        let peak = self.peak()?;
        self.known_value(peak)
    }

    pub fn known_value(&mut self, peak: Peak) -> JiterResult<JsonValue> {
        take_value(peak, &mut self.parser, &mut self.tape, DEFAULT_RECURSION_LIMIT).map_err(Into::into)
    }

    pub fn next_array(&mut self) -> JiterResult<Option<Peak>> {
        let peak = self.peak()?;
        match peak {
            Peak::Array => self.array_first(),
            _ => Err(self.wrong_type(JsonType::Array, peak)),
        }
    }

    pub fn array_first(&mut self) -> JiterResult<Option<Peak>> {
        self.parser.array_first().map_err(Into::into)
    }

    pub fn array_step(&mut self) -> JiterResult<Option<Peak>> {
        self.parser.array_step().map_err(Into::into)
    }

    pub fn next_object(&mut self) -> JiterResult<Option<&str>> {
        let peak = self.peak()?;
        let strs = match peak {
            Peak::Object => self.parser.object_first::<StringDecoder>(&mut self.tape)?,
            _ => return Err(self.wrong_type(JsonType::Object, peak)),
        };
        Ok(strs.map(|s| s.as_str()))
    }

    pub fn next_object_bytes(&mut self) -> JiterResult<Option<&[u8]>> {
        let peak = self.peak()?;
        match peak {
            Peak::Object => {
                let op_range = self.parser.object_first::<StringDecoderRange>(&mut self.tape)?;
                Ok(op_range.map(|r| &self.data[r]))
            }
            _ => Err(self.wrong_type(JsonType::Object, peak)),
        }
    }

    pub fn next_key(&mut self) -> JiterResult<Option<&str>> {
        let strs = self.parser.object_step::<StringDecoder>(&mut self.tape)?;
        Ok(strs.map(|s| s.as_str()))
    }

    pub fn next_key_bytes(&mut self) -> JiterResult<Option<&[u8]>> {
        let op_range = self.parser.object_step::<StringDecoderRange>(&mut self.tape)?;
        Ok(op_range.map(|r| &self.data[r]))
    }

    pub fn finish(&mut self) -> JiterResult<()> {
        self.parser.finish().map_err(Into::into)
    }

    fn wrong_type(&self, expected: JsonType, peak: Peak) -> JiterError {
        match peak {
            Peak::True | Peak::False => JiterError::wrong_type(expected, JsonType::Bool, self.parser.index),
            Peak::Null => JiterError::wrong_type(expected, JsonType::Null, self.parser.index),
            Peak::String => JiterError::wrong_type(expected, JsonType::String, self.parser.index),
            Peak::Num(first) => self.wrong_num(first, expected),
            Peak::Array => JiterError::wrong_type(expected, JsonType::Array, self.parser.index),
            Peak::Object => JiterError::wrong_type(expected, JsonType::Object, self.parser.index),
        }
    }

    fn wrong_num(&self, first: u8, expected: JsonType) -> JiterError {
        let mut parser2 = self.parser.clone();
        let actual = match parser2.consume_number::<NumberAny>(first) {
            Ok(NumberAny::Int { .. }) => JsonType::Int,
            Ok(NumberAny::Float { .. }) => JsonType::Float,
            Err(e) => return e.into(),
        };
        JiterError::wrong_type(expected, actual, self.parser.index)
    }
}
