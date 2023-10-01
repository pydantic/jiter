use num_bigint::BigInt;
use smallvec::SmallVec;

use crate::errors::{FilePosition, JsonResult, JsonValueError, DEFAULT_RECURSION_LIMIT};
use crate::lazy_index_map::LazyIndexMap;
use crate::number_decoder::{NumberAny, NumberInt};
use crate::parse::{Parser, Peak};
use crate::string_decoder::{StringDecoder, Tape};
use crate::JsonError;

#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    BigInt(BigInt),
    Float(f64),
    String(String),
    Array(JsonArray),
    Object(JsonObject),
}
pub type JsonArray = Box<SmallVec<[JsonValue; 8]>>;
pub type JsonObject = Box<LazyIndexMap<String, JsonValue>>;

#[cfg(feature = "python")]
impl pyo3::ToPyObject for JsonValue {
    fn to_object(&self, py: pyo3::Python<'_>) -> pyo3::PyObject {
        match self {
            Self::Null => py.None(),
            Self::Bool(b) => b.to_object(py),
            Self::Int(i) => i.to_object(py),
            Self::BigInt(b) => b.to_object(py),
            Self::Float(f) => f.to_object(py),
            Self::String(s) => s.to_object(py),
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
    pub fn parse(data: &[u8]) -> Result<Self, JsonValueError> {
        let mut parser = Parser::new(data);

        let map_err = |e: JsonError| {
            let position = FilePosition::find(data, e.index);
            JsonValueError {
                error_type: e.error_type,
                index: e.index,
                position,
            }
        };

        let mut tape = Tape::default();
        let peak = parser.peak().map_err(map_err)?;
        let v = take_value(peak, &mut parser, &mut tape, DEFAULT_RECURSION_LIMIT).map_err(map_err)?;
        parser.finish().map_err(map_err)?;
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
    peak: Peak,
    parser: &mut Parser,
    tape: &mut Tape,
    mut recursion_limit: u8,
) -> JsonResult<JsonValue> {
    match peak {
        Peak::True => {
            parser.consume_true()?;
            Ok(JsonValue::Bool(true))
        }
        Peak::False => {
            parser.consume_false()?;
            Ok(JsonValue::Bool(false))
        }
        Peak::Null => {
            parser.consume_null()?;
            Ok(JsonValue::Null)
        }
        Peak::String => {
            let s = parser.consume_string::<StringDecoder>(tape)?;
            Ok(JsonValue::String(s.to_string()))
        }
        Peak::Num(first) => {
            let n = parser.consume_number::<NumberAny>(first)?;
            match n {
                NumberAny::Int(NumberInt::Int(int)) => Ok(JsonValue::Int(int)),
                NumberAny::Int(NumberInt::BigInt(big_int)) => Ok(JsonValue::BigInt(big_int)),
                NumberAny::Float(float) => Ok(JsonValue::Float(float)),
            }
        }
        Peak::Array => {
            // we could do something clever about guessing the size of the array
            let mut array: SmallVec<[JsonValue; 8]> = SmallVec::new();
            if let Some(peak_first) = parser.array_first()? {
                check_recursion!(recursion_limit, parser.index,
                    let v = take_value(peak_first, parser, tape, recursion_limit)?;
                );
                array.push(v);
                while parser.array_step()? {
                    let peak = parser.peak()?;
                    check_recursion!(recursion_limit, parser.index,
                        let v = take_value(peak, parser, tape, recursion_limit)?;
                    );
                    array.push(v);
                }
            }
            Ok(JsonValue::Array(Box::new(array)))
        }
        Peak::Object => {
            // same for objects
            let mut object = LazyIndexMap::new();
            if let Some(first_key) = parser.object_first::<StringDecoder>(tape)? {
                let first_key = first_key.to_string();
                let peak = parser.peak()?;
                check_recursion!(recursion_limit, parser.index,
                    let first_value = take_value(peak, parser, tape, recursion_limit)?;
                );
                object.insert(first_key, first_value);
                while let Some(key) = parser.object_step::<StringDecoder>(tape)? {
                    let key = key.to_string();
                    let peak = parser.peak()?;
                    check_recursion!(recursion_limit, parser.index,
                        let value = take_value(peak, parser, tape, recursion_limit)?;
                    );
                    object.insert(key, value);
                }
            }

            Ok(JsonValue::Object(Box::new(object)))
        }
    }
}
