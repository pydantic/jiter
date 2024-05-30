use std::borrow::Cow;
use std::sync::Arc;

use num_bigint::BigInt;
use smallvec::SmallVec;

use crate::errors::{json_error, JsonError, JsonResult, DEFAULT_RECURSION_LIMIT};
use crate::lazy_index_map::LazyIndexMap;
use crate::number_decoder::{NumberAny, NumberInt, NumberRange};
use crate::parse::{Parser, Peek};
use crate::string_decoder::{StringDecoder, StringDecoderRange, StringOutput, Tape};

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

    /// Copy a borrowed JSON enum into an owned JSON enum.
    pub fn to_static(&self) -> JsonValue<'static> {
        value_static(self.clone())
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
        JsonValue::Array(v) => JsonValue::Array(Arc::new(v.iter().map(JsonValue::to_static).collect::<SmallVec<_>>())),
        JsonValue::Object(o) => JsonValue::Object(Arc::new(o.to_static())),
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
            let s: StringOutput<'_, 'j> = parser.consume_string::<StringDecoder>(tape, false)?;
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

/// like `take_value`, but nothing is returned, should be faster than `take_value`, useful when you don't care
/// about the value, but just want to consume it
pub(crate) fn take_value_skip(
    peek: Peek,
    parser: &mut Parser,
    tape: &mut Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
) -> JsonResult<()> {
    match peek {
        Peek::True => parser.consume_true(),
        Peek::False => parser.consume_false(),
        Peek::Null => parser.consume_null(),
        Peek::String => parser.consume_string::<StringDecoderRange>(tape, false).map(drop),
        Peek::Array => {
            if let Some(next_peek) = parser.array_first()? {
                take_value_skip_recursive(next_peek, ARRAY, parser, tape, recursion_limit, allow_inf_nan)
            } else {
                Ok(())
            }
        }
        Peek::Object => {
            if parser.object_first::<StringDecoderRange>(tape)?.is_some() {
                take_value_skip_recursive(parser.peek()?, OBJECT, parser, tape, recursion_limit, allow_inf_nan)
            } else {
                Ok(())
            }
        }
        _ => parser
            .consume_number::<NumberRange>(peek.into_inner(), allow_inf_nan)
            .map(drop)
            .map_err(|e| {
                if !peek.is_num() {
                    json_error!(ExpectedSomeValue, parser.index)
                } else {
                    e
                }
            }),
    }
}

const ARRAY: bool = false;
const OBJECT: bool = true;

#[inline(never)] // this is an iterative algo called only from take_value_skip, no point in inlining
fn take_value_skip_recursive(
    mut peek: Peek,
    mut current_recursion: bool,
    parser: &mut Parser,
    tape: &mut Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
) -> JsonResult<()> {
    let mut recursion_stack = bitvec::bitarr![0; 256];
    let recursion_limit: usize = recursion_limit.into();
    let mut current_recursion_depth = 0;

    macro_rules! push_recursion {
        ($next_peek:expr, $value:expr) => {
            peek = $next_peek;
            recursion_stack.set(
                current_recursion_depth,
                std::mem::replace(&mut current_recursion, $value),
            );
            current_recursion_depth += 1;
            if current_recursion_depth >= recursion_limit {
                return Err(json_error!(RecursionLimitExceeded, parser.index));
            }
        };
    }

    loop {
        match peek {
            Peek::True => parser.consume_true()?,
            Peek::False => parser.consume_false()?,
            Peek::Null => parser.consume_null()?,
            Peek::String => {
                parser.consume_string::<StringDecoderRange>(tape, false)?;
            }
            Peek::Array => {
                if let Some(next_peek) = parser.array_first()? {
                    push_recursion!(next_peek, ARRAY);
                    // immediately jump to process the first value in the array
                    continue;
                }
            }
            Peek::Object => {
                if parser.object_first::<StringDecoderRange>(tape)?.is_some() {
                    push_recursion!(parser.peek()?, OBJECT);
                    // immediately jump to process the first value in the object
                    continue;
                }
            }
            _ => {
                parser
                    .consume_number::<NumberRange>(peek.into_inner(), allow_inf_nan)
                    .map_err(|e| {
                        if !peek.is_num() {
                            json_error!(ExpectedSomeValue, parser.index)
                        } else {
                            e
                        }
                    })?;
            }
        };

        // now try to advance position in the current array or object
        peek = loop {
            match current_recursion {
                ARRAY => {
                    if let Some(next_peek) = parser.array_step()? {
                        break next_peek;
                    }
                }
                OBJECT => {
                    if parser.object_step::<StringDecoderRange>(tape)?.is_some() {
                        break parser.peek()?;
                    }
                }
            }

            current_recursion_depth = match current_recursion_depth.checked_sub(1) {
                Some(r) => r,
                // no recursion left, we are done
                None => return Ok(()),
            };

            current_recursion = recursion_stack[current_recursion_depth];
        };
    }
}
