use std::borrow::Cow;
use std::sync::Arc;

use num_bigint::BigInt;
use smallvec::SmallVec;

use crate::errors::{json_error, JsonError, JsonResult, DEFAULT_RECURSION_LIMIT};
use crate::lazy_index_map::LazyIndexMap;
use crate::number_decoder::{NumberAny, NumberInt};
use crate::parse::{Parser, Peek};
use crate::string_decoder::{StringDecoder, StringOutput, Tape};

/// Enum representing a JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue<'s> {
    Null,
    Bool(bool),
    Int(i64),
    BigInt(BigInt),
    Float(f64),
    Str(Cow<'s, str>),
    Array(JsonArray<'s>),
    Object(JsonObject<'s>),
}

pub type JsonArray<'s> = Arc<SmallVec<[JsonValue<'s>; 8]>>;
pub type JsonObject<'s> = Arc<LazyIndexMap<Cow<'s, str>, JsonValue<'s>>>;

#[cfg(feature = "python")]
impl pyo3::ToPyObject for JsonValue<'_> {
    fn to_object(&self, py: pyo3::Python<'_>) -> pyo3::PyObject {
        use pyo3::prelude::*;
        match self {
            Self::Null => py.None().to_object(py),
            Self::Bool(b) => b.to_object(py),
            Self::Int(i) => i.to_object(py),
            Self::BigInt(b) => b.to_object(py),
            Self::Float(f) => f.to_object(py),
            Self::Str(s) => s.to_object(py),
            Self::Array(v) => pyo3::types::PyList::new_bound(py, v.iter().map(|v| v.to_object(py))).to_object(py),
            Self::Object(o) => {
                let dict = pyo3::types::PyDict::new_bound(py);
                for (k, v) in o.iter() {
                    dict.set_item(k, v.to_object(py)).unwrap();
                }
                dict.to_object(py)
            }
        }
    }
}

impl<'j> JsonValue<'j> {
    /// Parse a JSON enum from a byte slice, returning a borrowed version of the enum - e.g. strings can be
    /// references into the original byte slice.
    pub fn parse(data: &'j [u8], allow_inf_nan: bool) -> Result<Self, JsonError> {
        let mut parser = Parser::new(data);

        let mut tape = Tape::default();
        let peek = parser.peek()?;
        let v = take_value_borrowed(peek, &mut parser, &mut tape, DEFAULT_RECURSION_LIMIT, allow_inf_nan)?;
        parser.finish()?;
        Ok(v)
    }

    /// Convert a borrowed JSON enum into an owned JSON enum.
    pub fn into_static(self) -> JsonValue<'static> {
        value_static(self)
    }
}

fn value_static(v: JsonValue<'_>) -> JsonValue<'static> {
    match v {
        JsonValue::Null => JsonValue::Null,
        JsonValue::Bool(b) => JsonValue::Bool(b),
        JsonValue::Int(i) => JsonValue::Int(i),
        JsonValue::BigInt(b) => JsonValue::BigInt(b),
        JsonValue::Float(f) => JsonValue::Float(f),
        JsonValue::Str(s) => JsonValue::Str(s.into_owned().into()),
        JsonValue::Array(v) => {
            // TODO is try_unwrap the right thing to do here?
            let vec = Arc::try_unwrap(v).unwrap();
            JsonValue::Array(Arc::new(vec.into_iter().map(value_static).collect::<SmallVec<_>>()))
        }
        JsonValue::Object(o) => {
            // TODO is try_unwrap the right thing to do here?
            let map = Arc::try_unwrap(o).unwrap();
            JsonValue::Object(Arc::new(map.into_owned()))
        } // JsonValue::Object(_) => todo!(),
    }
}

impl JsonValue<'static> {
    /// Parse a JSON enum from a byte slice, returning an owned version of the enum.
    pub fn parse_owned(data: &[u8], allow_inf_nan: bool) -> Result<Self, JsonError> {
        let mut parser = Parser::new(data);

        let mut tape = Tape::default();
        let peek = parser.peek()?;
        let v = take_value_owned(peek, &mut parser, &mut tape, DEFAULT_RECURSION_LIMIT, allow_inf_nan)?;
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

pub(crate) fn take_value_borrowed<'j>(
    peek: Peek,
    parser: &mut Parser<'j>,
    tape: &mut Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
) -> JsonResult<JsonValue<'j>> {
    take_value(
        peek,
        parser,
        tape,
        recursion_limit,
        allow_inf_nan,
        &|s: StringOutput<'_, 'j>| s.into(),
    )
}

pub(crate) fn take_value_owned<'j>(
    peek: Peek,
    parser: &mut Parser<'j>,
    tape: &mut Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
) -> JsonResult<JsonValue<'static>> {
    take_value(
        peek,
        parser,
        tape,
        recursion_limit,
        allow_inf_nan,
        &|s: StringOutput<'_, 'j>| Into::<String>::into(s).into(),
    )
}

fn take_value<'j, 's>(
    peek: Peek,
    parser: &mut Parser<'j>,
    tape: &mut Tape,
    mut recursion_limit: u8,
    allow_inf_nan: bool,
    create_cow: &impl Fn(StringOutput<'_, 'j>) -> Cow<'s, str>,
) -> JsonResult<JsonValue<'s>> {
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
            let s: StringOutput<'_, 'j> = parser.consume_string::<StringDecoder>(tape)?;
            Ok(JsonValue::Str(create_cow(s)))
        }
        Peek::Array => {
            // we could do something clever about guessing the size of the array
            let mut array: SmallVec<[JsonValue<'s>; 8]> = SmallVec::new();
            if let Some(peek_first) = parser.array_first()? {
                check_recursion!(recursion_limit, parser.index,
                    let v = take_value(peek_first, parser, tape, recursion_limit, allow_inf_nan, create_cow)?;
                );
                array.push(v);
                while let Some(peek) = parser.array_step()? {
                    check_recursion!(recursion_limit, parser.index,
                        let v = take_value(peek, parser, tape, recursion_limit, allow_inf_nan, create_cow)?;
                    );
                    array.push(v);
                }
            }
            Ok(JsonValue::Array(Arc::new(array)))
        }
        Peek::Object => {
            // same for objects
            let mut object: LazyIndexMap<Cow<'s, str>, JsonValue<'s>> = LazyIndexMap::new();
            if let Some(first_key) = parser.object_first::<StringDecoder>(tape)? {
                let first_key = create_cow(first_key);
                let peek = parser.peek()?;
                check_recursion!(recursion_limit, parser.index,
                    let first_value = take_value(peek, parser, tape, recursion_limit, allow_inf_nan, create_cow)?;
                );
                object.insert(first_key, first_value);
                while let Some(key) = parser.object_step::<StringDecoder>(tape)? {
                    let key = create_cow(key);
                    let peek = parser.peek()?;
                    check_recursion!(recursion_limit, parser.index,
                        let value = take_value(peek, parser, tape, recursion_limit, allow_inf_nan, create_cow)?;
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
