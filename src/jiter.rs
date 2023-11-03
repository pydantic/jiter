use crate::errors::{FilePosition, JiterError, JsonType, DEFAULT_RECURSION_LIMIT};
use crate::number_decoder::{NumberAny, NumberFloat, NumberInt, NumberRange};
use crate::parse::{Parser, Peak};
use crate::string_decoder::{StringDecoder, StringDecoderRange, Tape};
use crate::value::{take_value, JsonValue};

pub type JiterResult<T> = Result<T, JiterError>;

/// A JSON iterator.
pub struct Jiter<'j> {
    data: &'j [u8],
    parser: Parser<'j>,
    tape: Tape,
    allow_inf_nan: bool,
}

impl<'j> Jiter<'j> {
    /// Constructs a new `Jiter`.
    ///
    /// # Arguments
    /// - `data`: The JSON data to be parsed.
    /// - `allow_inf_nan`: Whether to allow `NaN`, `Infinity` and `-Infinity` as numbers.
    pub fn new(data: &'j [u8], allow_inf_nan: bool) -> Self {
        Self {
            data,
            parser: Parser::new(data),
            tape: Tape::default(),
            allow_inf_nan,
        }
    }

    /// Get the current [FilePosition] of the parser.
    pub fn current_position(&self) -> FilePosition {
        self.parser.current_position()
    }

    /// Convert an error index to a [FilePosition].
    ///
    /// # Arguments
    /// - `index`: The index of the error to find the position of.
    pub fn error_position(&self, index: usize) -> FilePosition {
        FilePosition::find(self.data, index)
    }

    /// Peak at the next JSON value without consuming it.
    pub fn peak(&mut self) -> JiterResult<Peak> {
        self.parser.peak().map_err(Into::into)
    }

    /// Assuming the next value is `null`, consume it. Error if it is not `null`, or is invalid JSON.
    pub fn next_null(&mut self) -> JiterResult<()> {
        let peak = self.peak()?;
        match peak {
            Peak::Null => self.known_null(),
            _ => Err(self.wrong_type(JsonType::Null, peak)),
        }
    }

    /// Knowing the next value is `null`, consume it.
    pub fn known_null(&mut self) -> JiterResult<()> {
        self.parser.consume_null()?;
        Ok(())
    }

    /// Assuming the next value is `true` or `false`, consume it. Error if it is not a boolean, or is invalid JSON.
    ///
    /// # Returns
    /// The boolean value.
    pub fn next_bool(&mut self) -> JiterResult<bool> {
        let peak = self.peak()?;
        self.known_bool(peak)
    }

    /// Knowing the next value is `true` or `false`, parse it.
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

    /// Assuming the next value is a number, consume it. Error if it is not a number, or is invalid JSON.
    ///
    /// # Returns
    /// A [NumberAny] representing the number.
    pub fn next_number(&mut self) -> JiterResult<NumberAny> {
        let peak = self.peak()?;
        self.known_number(peak)
    }

    /// Knowing the next value is a number, parse it.
    pub fn known_number(&mut self, peak: Peak) -> JiterResult<NumberAny> {
        match peak {
            Peak::Num(first) => self
                .parser
                .consume_number::<NumberAny>(first, self.allow_inf_nan)
                .map_err(Into::into),
            _ => Err(self.wrong_type(JsonType::Int, peak)),
        }
    }

    /// Assuming the next value is an integer, consume it. Error if it is not an integer, or is invalid JSON.
    pub fn next_int(&mut self) -> JiterResult<NumberInt> {
        let peak = self.peak()?;
        self.known_int(peak)
    }

    /// Knowing the next value is an integer, parse it.
    pub fn known_int(&mut self, peak: Peak) -> JiterResult<NumberInt> {
        match peak {
            Peak::Num(first) => self
                .parser
                .consume_number::<NumberInt>(first, self.allow_inf_nan)
                .map_err(Into::into),
            _ => Err(self.wrong_type(JsonType::Int, peak)),
        }
    }

    /// Assuming the next value is a float, consume it. Error if it is not a float, or is invalid JSON.
    pub fn next_float(&mut self) -> JiterResult<f64> {
        let peak = self.peak()?;
        self.known_float(peak)
    }

    /// Knowing the next value is a float, parse it.
    pub fn known_float(&mut self, peak: Peak) -> JiterResult<f64> {
        match peak {
            Peak::Num(first) => self
                .parser
                .consume_number::<NumberFloat>(first, self.allow_inf_nan)
                .map_err(Into::into),
            _ => Err(self.wrong_type(JsonType::Int, peak)),
        }
    }

    /// Assuming the next value is a number, consume it and return bytes from the original JSON data.
    pub fn next_number_bytes(&mut self) -> JiterResult<&[u8]> {
        let peak = self.peak()?;
        match peak {
            Peak::Num(first) => {
                let range = self.parser.consume_number::<NumberRange>(first, self.allow_inf_nan)?;
                Ok(&self.data[range])
            }
            _ => Err(self.wrong_type(JsonType::Float, peak)),
        }
    }

    /// Assuming the next value is a string, consume it. Error if it is not a string, or is invalid JSON.
    pub fn next_str(&mut self) -> JiterResult<&str> {
        let peak = self.peak()?;
        match peak {
            Peak::String => self.known_str(),
            _ => Err(self.wrong_type(JsonType::String, peak)),
        }
    }

    /// Knowing the next value is a string, parse it.
    pub fn known_str(&mut self) -> JiterResult<&str> {
        match self.parser.consume_string::<StringDecoder>(&mut self.tape) {
            Ok(output) => Ok(output.as_str()),
            Err(e) => Err(e.into()),
        }
    }

    /// Assuming the next value is a string, consume it and return bytes from the original JSON data.
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

    /// Parse the next JSON value and return it as a [JsonValue]. Error if it is invalid JSON.
    pub fn next_value(&mut self) -> JiterResult<JsonValue> {
        let peak = self.peak()?;
        self.known_value(peak)
    }

    /// Parse the next JSON value and return it as a [JsonValue]. Error if it is invalid JSON.
    ///
    /// # Arguments
    /// - `peak`: The [Peak] of the next JSON value.
    pub fn known_value(&mut self, peak: Peak) -> JiterResult<JsonValue> {
        take_value(
            peak,
            &mut self.parser,
            &mut self.tape,
            DEFAULT_RECURSION_LIMIT,
            self.allow_inf_nan,
        )
        .map_err(Into::into)
    }

    /// Assuming the next value is an array, peak at the first value.
    /// Error if it is not an array, or is invalid JSON.
    ///
    /// # Returns
    /// The `Some(peak)` of the first value in the array is not empty, `None` if it is empty.
    pub fn next_array(&mut self) -> JiterResult<Option<Peak>> {
        let peak = self.peak()?;
        match peak {
            Peak::Array => self.known_array(),
            _ => Err(self.wrong_type(JsonType::Array, peak)),
        }
    }

    /// Assuming the next value is an array, peat at the first value.
    pub fn known_array(&mut self) -> JiterResult<Option<Peak>> {
        self.parser.array_first().map_err(Into::into)
    }

    /// Peak at the next value in an array.
    pub fn array_step(&mut self) -> JiterResult<Option<Peak>> {
        self.parser.array_step().map_err(Into::into)
    }

    /// Assuming the next value is an object, consume the first key.
    /// Error if it is not an object, or is invalid JSON.
    ///
    /// # Returns
    /// The `Some(key)` of the first key in the object is not empty, `None` if it is empty.
    pub fn next_object(&mut self) -> JiterResult<Option<&str>> {
        let peak = self.peak()?;
        match peak {
            Peak::Object => self.known_object(),
            _ => Err(self.wrong_type(JsonType::Object, peak)),
        }
    }

    /// Assuming the next value is an object, conssume the first key and return bytes from the original JSON data.
    pub fn known_object(&mut self) -> JiterResult<Option<&str>> {
        let op_str = self.parser.object_first::<StringDecoder>(&mut self.tape)?;
        Ok(op_str.map(|s| s.as_str()))
    }

    /// Assuming the next value is an object, peak at the first key.
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

    /// Get the next key in an object, or `None` if there are no more keys.
    pub fn next_key(&mut self) -> JiterResult<Option<&str>> {
        let strs = self.parser.object_step::<StringDecoder>(&mut self.tape)?;
        Ok(strs.map(|s| s.as_str()))
    }

    /// Get the next key in an object as bytes, or `None` if there are no more keys.
    pub fn next_key_bytes(&mut self) -> JiterResult<Option<&[u8]>> {
        let op_range = self.parser.object_step::<StringDecoderRange>(&mut self.tape)?;
        Ok(op_range.map(|r| &self.data[r]))
    }

    /// Finish parsing the JSON data. Error if there is more data to be parsed.
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
        let actual = match parser2.consume_number::<NumberAny>(first, self.allow_inf_nan) {
            Ok(NumberAny::Int { .. }) => JsonType::Int,
            Ok(NumberAny::Float { .. }) => JsonType::Float,
            Err(e) => return e.into(),
        };
        JiterError::wrong_type(expected, actual, self.parser.index)
    }
}
