use std::sync::Arc;

use num_bigint::BigInt;
use smallvec::SmallVec;

use crate::errors::{json_error, JsonError, JsonResult, DEFAULT_RECURSION_LIMIT};
use crate::lazy_index_map::LazyIndexMap;
use crate::number_decoder::{NumberAny, NumberInt};
use crate::parse::{Parser, Peek};
use crate::string_decoder::{StringDecoder, Tape};

/// Enum representing a JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    BigInt(BigInt),
    Float(f64),
    Str(String),
    Array(JsonArray),
    Object(JsonObject),
}

pub type JsonArray = Arc<SmallVec<[JsonValue; 8]>>;
pub type JsonObject = Arc<LazyIndexMap<String, JsonValue>>;

#[cfg(feature = "python")]
impl pyo3::ToPyObject for JsonValue {
    fn to_object(&self, py: pyo3::Python<'_>) -> pyo3::PyObject {
        match self {
            Self::Null => py.None(),
            Self::Bool(b) => b.to_object(py),
            Self::Int(i) => i.to_object(py),
            Self::BigInt(b) => b.to_object(py),
            Self::Float(f) => f.to_object(py),
            Self::Str(s) => s.to_object(py),
            Self::Array(v) => pyo3::types::PyList::new(py, v.iter().map(|v| v.to_object(py))).to_object(py),
            Self::Object(o) => {
                let dict = pyo3::types::PyDict::new(py);
                for (k, v) in o.iter() {
                    dict.set_item(k, v.to_object(py)).unwrap();
                }
                dict.to_object(py)
            }
        }
    }
}

impl JsonValue {
    /// Parse a JSON value from a byte slice.
    pub fn parse(data: &[u8], allow_inf_nan: bool) -> Result<Self, JsonError> {
        let mut parser = Parser::new(data);

        let mut tape = Tape::default();
        let peek = parser.peek()?;
        let v = take_value(peek, &mut parser, &mut tape, DEFAULT_RECURSION_LIMIT, allow_inf_nan)?;
        parser.finish()?;
        Ok(v)
    }
}

macro_rules! check_recursion {
    ($recursion_limit:ident, $index:expr, $($body:tt)*) => {
        $recursion_limit = match $recursion_limit.checked_sub(1) {
            Some(limit) => limit,
            None => return crate::errors::json_err!(RecursionLimitExceeded, $index),
        };

        $($body)*

        $recursion_limit += 1;
    };
}

pub(crate) fn take_value(
    peek: Peek,
    parser: &mut Parser,
    tape: &mut Tape,
    mut recursion_limit: u8,
    allow_inf_nan: bool,
) -> JsonResult<JsonValue> {
    match peek {
        Peek::True => {
            parser.consume_true()?;
            Ok(JsonValue::Bool(true))
        }
        Peek::False => {
            parser.consume_false()?;
            Ok(JsonValue::Bool(false))
        }
        Peek::Null => {
            parser.consume_null()?;
            Ok(JsonValue::Null)
        }
        Peek::String => {
            let s = parser.consume_string::<StringDecoder>(tape)?;
            Ok(JsonValue::Str(s.into()))
        }
        Peek::Array => {
            // we could do something clever about guessing the size of the array
            let mut array: SmallVec<[JsonValue; 8]> = SmallVec::new();
            if let Some(peek_first) = parser.array_first()? {
                check_recursion!(recursion_limit, parser.index,
                    let v = take_value(peek_first, parser, tape, recursion_limit, allow_inf_nan)?;
                );
                array.push(v);
                while let Some(peek) = parser.array_step()? {
                    check_recursion!(recursion_limit, parser.index,
                        let v = take_value(peek, parser, tape, recursion_limit, allow_inf_nan)?;
                    );
                    array.push(v);
                }
            }
            Ok(JsonValue::Array(Arc::new(array)))
        }
        Peek::Object => {
            // same for objects
            let mut object: LazyIndexMap<String, JsonValue> = LazyIndexMap::new();
            if let Some(first_key) = parser.object_first::<StringDecoder>(tape)? {
                let first_key = first_key.into();
                let peek = parser.peek()?;
                check_recursion!(recursion_limit, parser.index,
                    let first_value = take_value(peek, parser, tape, recursion_limit, allow_inf_nan)?;
                );
                object.insert(first_key, first_value);
                while let Some(key) = parser.object_step::<StringDecoder>(tape)? {
                    let key = key.into();
                    let peek = parser.peek()?;
                    check_recursion!(recursion_limit, parser.index,
                        let value = take_value(peek, parser, tape, recursion_limit, allow_inf_nan)?;
                    );
                    object.insert(key, value);
                }
            }

            Ok(JsonValue::Object(Arc::new(object)))
        }
        _ => {
            let n = parser.consume_number::<NumberAny>(peek.into_inner(), allow_inf_nan);
            match n {
                Ok(NumberAny::Int(NumberInt::Int(int))) => Ok(JsonValue::Int(int)),
                Ok(NumberAny::Int(NumberInt::BigInt(big_int))) => Ok(JsonValue::BigInt(big_int)),
                Ok(NumberAny::Float(float)) => Ok(JsonValue::Float(float)),
                Err(e) => {
                    if !peek.is_num() {
                        Err(json_error!(ExpectedSomeValue, parser.index))
                    } else {
                        Err(e)
                    }
                }
            }
        }
    }
}
