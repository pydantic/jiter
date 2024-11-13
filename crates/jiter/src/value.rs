use std::borrow::Cow;
use std::sync::Arc;

#[cfg(feature = "num-bigint")]
use num_bigint::BigInt;
use smallvec::SmallVec;

use crate::errors::{json_error, JsonError, JsonResult, DEFAULT_RECURSION_LIMIT};
use crate::lazy_index_map::LazyIndexMap;
use crate::number_decoder::{NumberAny, NumberInt, NumberRange};
use crate::parse::{Parser, Peek};
use crate::string_decoder::{StringDecoder, StringDecoderRange, StringOutput, Tape};
use crate::PartialMode;

/// Enum representing a JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue<'s> {
    Null,
    Bool(bool),
    Int(i64),
    #[cfg(feature = "num-bigint")]
    BigInt(BigInt),
    Float(f64),
    Str(Cow<'s, str>),
    Array(JsonArray<'s>),
    Object(JsonObject<'s>),
}

pub type JsonArray<'s> = Arc<SmallVec<[JsonValue<'s>; 8]>>;
pub type JsonObject<'s> = Arc<LazyIndexMap<Cow<'s, str>, JsonValue<'s>>>;

#[cfg(feature = "python")]
#[allow(deprecated)] // keeping around for sake of allowing downstream to migrate
impl pyo3::ToPyObject for JsonValue<'_> {
    fn to_object(&self, py: pyo3::Python<'_>) -> pyo3::PyObject {
        use pyo3::prelude::*;
        match self {
            Self::Null => py.None().to_object(py),
            Self::Bool(b) => b.to_object(py),
            Self::Int(i) => i.to_object(py),
            #[cfg(feature = "num-bigint")]
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

#[cfg(feature = "python")]
impl<'py> pyo3::IntoPyObject<'py> for JsonValue<'_> {
    type Error = pyo3::PyErr;
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, pyo3::PyAny>;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        use pyo3::prelude::*;
        match self {
            Self::Null => Ok(py.None().into_pyobject(py)?),
            Self::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any()),
            Self::Int(i) => Ok(i.into_pyobject(py)?.into_any()),
            #[cfg(feature = "num-bigint")]
            Self::BigInt(b) => Ok(b.into_pyobject(py)?.into_any()),
            Self::Float(f) => Ok(f.into_pyobject(py)?.into_any()),
            Self::Str(s) => Ok(s.into_pyobject(py)?.into_any()),
            Self::Array(v) => Ok(pyo3::types::PyList::new(py, v.iter())?.into_any()),
            Self::Object(o) => {
                let dict = pyo3::types::PyDict::new(py);
                for (k, v) in o.iter() {
                    dict.set_item(k, v).unwrap();
                }
                Ok(dict.into_any())
            }
        }
    }
}

#[cfg(feature = "python")]
impl<'py> pyo3::IntoPyObject<'py> for &'_ JsonValue<'_> {
    type Error = pyo3::PyErr;
    type Target = pyo3::PyAny;
    type Output = pyo3::Bound<'py, pyo3::PyAny>;

    fn into_pyobject(self, py: pyo3::Python<'py>) -> Result<Self::Output, Self::Error> {
        use pyo3::prelude::*;
        match self {
            JsonValue::Null => Ok(py.None().into_pyobject(py)?),
            JsonValue::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any()),
            JsonValue::Int(i) => Ok(i.into_pyobject(py)?.into_any()),
            #[cfg(feature = "num-bigint")]
            JsonValue::BigInt(b) => Ok(b.into_pyobject(py)?.into_any()),
            JsonValue::Float(f) => Ok(f.into_pyobject(py)?.into_any()),
            JsonValue::Str(s) => Ok(s.into_pyobject(py)?.into_any()),
            JsonValue::Array(v) => Ok(pyo3::types::PyList::new(py, v.iter())?.into_any()),
            JsonValue::Object(o) => {
                let dict = pyo3::types::PyDict::new(py);
                for (k, v) in o.iter() {
                    dict.set_item(k, v).unwrap();
                }
                Ok(dict.into_any())
            }
        }
    }
}

impl<'j> JsonValue<'j> {
    /// Parse a JSON enum from a byte slice, returning a borrowed version of the enum - e.g. strings can be
    /// references into the original byte slice.
    pub fn parse(data: &'j [u8], allow_inf_nan: bool) -> Result<Self, JsonError> {
        Self::parse_with_config(data, allow_inf_nan, PartialMode::Off)
    }

    pub fn parse_with_config(
        data: &'j [u8],
        allow_inf_nan: bool,
        allow_partial: PartialMode,
    ) -> Result<Self, JsonError> {
        let mut parser = Parser::new(data);

        let mut tape = Tape::default();
        let peek = parser.peek()?;
        let v = take_value_borrowed(
            peek,
            &mut parser,
            &mut tape,
            DEFAULT_RECURSION_LIMIT,
            allow_inf_nan,
            allow_partial,
        )?;
        if !allow_partial.is_active() {
            parser.finish()?;
        }
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
        #[cfg(feature = "num-bigint")]
        JsonValue::BigInt(b) => JsonValue::BigInt(b),
        JsonValue::Float(f) => JsonValue::Float(f),
        JsonValue::Str(s) => JsonValue::Str(s.into_owned().into()),
        JsonValue::Array(v) => JsonValue::Array(Arc::new(v.iter().map(JsonValue::to_static).collect::<SmallVec<_>>())),
        JsonValue::Object(o) => JsonValue::Object(Arc::new(o.to_static())),
    }
}

impl JsonValue<'static> {
    /// Parse a JSON enum from a byte slice, returning an owned version of the enum.
    pub fn parse_owned(data: &[u8], allow_inf_nan: bool, allow_partial: PartialMode) -> Result<Self, JsonError> {
        let mut parser = Parser::new(data);

        let mut tape = Tape::default();
        let peek = parser.peek()?;
        let v = take_value_owned(
            peek,
            &mut parser,
            &mut tape,
            DEFAULT_RECURSION_LIMIT,
            allow_inf_nan,
            allow_partial,
        )?;
        parser.finish()?;
        Ok(v)
    }
}

pub(crate) fn take_value_borrowed<'j>(
    peek: Peek,
    parser: &mut Parser<'j>,
    tape: &mut Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
    allow_partial: PartialMode,
) -> JsonResult<JsonValue<'j>> {
    take_value(
        peek,
        parser,
        tape,
        recursion_limit,
        allow_inf_nan,
        allow_partial,
        &|s: StringOutput<'_, 'j>| s.into(),
    )
}

pub(crate) fn take_value_owned<'j>(
    peek: Peek,
    parser: &mut Parser<'j>,
    tape: &mut Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
    allow_partial: PartialMode,
) -> JsonResult<JsonValue<'static>> {
    take_value(
        peek,
        parser,
        tape,
        recursion_limit,
        allow_inf_nan,
        allow_partial,
        &|s: StringOutput<'_, 'j>| Into::<String>::into(s).into(),
    )
}

fn take_value<'j, 's>(
    peek: Peek,
    parser: &mut Parser<'j>,
    tape: &mut Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
    allow_partial: PartialMode,
    create_cow: &impl Fn(StringOutput<'_, 'j>) -> Cow<'s, str>,
) -> JsonResult<JsonValue<'s>> {
    let partial_active = allow_partial.is_active();
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
            let s: StringOutput<'_, 'j> =
                parser.consume_string::<StringDecoder>(tape, allow_partial.allow_trailing_str())?;
            Ok(JsonValue::Str(create_cow(s)))
        }
        Peek::Array => {
            let array = Arc::new(SmallVec::new());
            let peek_first = match parser.array_first() {
                Ok(Some(peek)) => peek,
                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                Ok(None) | Err(_) => return Ok(JsonValue::Array(array)),
            };
            take_value_recursive(
                peek_first,
                RecursedValue::Array(array),
                parser,
                tape,
                recursion_limit,
                allow_inf_nan,
                allow_partial,
                create_cow,
            )
        }
        Peek::Object => {
            // same for objects
            let object = Arc::new(LazyIndexMap::new());
            let first_key = match parser.object_first::<StringDecoder>(tape) {
                Ok(Some(first_key)) => first_key,
                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                _ => return Ok(JsonValue::Object(object)),
            };
            let first_key = create_cow(first_key);
            match parser.peek() {
                Ok(peek) => take_value_recursive(
                    peek,
                    RecursedValue::Object {
                        partial: object,
                        next_key: first_key,
                    },
                    parser,
                    tape,
                    recursion_limit,
                    allow_inf_nan,
                    allow_partial,
                    create_cow,
                ),
                Err(e) if !(partial_active && e.allowed_if_partial()) => Err(e),
                _ => Ok(JsonValue::Object(object)),
            }
        }
        _ => {
            let n = parser.consume_number::<NumberAny>(peek.into_inner(), allow_inf_nan);
            match n {
                Ok(NumberAny::Int(NumberInt::Int(int))) => Ok(JsonValue::Int(int)),
                #[cfg(feature = "num-bigint")]
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

enum RecursedValue<'s> {
    Array(JsonArray<'s>),
    Object {
        partial: JsonObject<'s>,
        next_key: Cow<'s, str>,
    },
}

#[inline(never)] // this is an iterative algo called only from take_value, no point in inlining
#[allow(clippy::too_many_lines)] // FIXME?
#[allow(clippy::too_many_arguments)]
fn take_value_recursive<'j, 's>(
    mut peek: Peek,
    mut current_recursion: RecursedValue<'s>,
    parser: &mut Parser<'j>,
    tape: &mut Tape,
    recursion_limit: u8,
    allow_inf_nan: bool,
    allow_partial: PartialMode,
    create_cow: &impl Fn(StringOutput<'_, 'j>) -> Cow<'s, str>,
) -> JsonResult<JsonValue<'s>> {
    let recursion_limit: usize = recursion_limit.into();

    let mut recursion_stack: SmallVec<[RecursedValue; 8]> = SmallVec::new();
    let partial_active = allow_partial.is_active();

    macro_rules! push_recursion {
        ($next_peek:expr, $value:expr) => {
            peek = $next_peek;
            recursion_stack.push(std::mem::replace(&mut current_recursion, $value));
            if recursion_stack.len() >= recursion_limit {
                return Err(json_error!(RecursionLimitExceeded, parser.index));
            }
        };
    }

    'recursion: loop {
        let mut value = match &mut current_recursion {
            RecursedValue::Array(array) => {
                let array = Arc::get_mut(array).expect("sole writer");
                loop {
                    let result = match peek {
                        Peek::True => parser.consume_true().map(|()| JsonValue::Bool(true)),
                        Peek::False => parser.consume_false().map(|()| JsonValue::Bool(false)),
                        Peek::Null => parser.consume_null().map(|()| JsonValue::Null),
                        Peek::String => parser
                            .consume_string::<StringDecoder>(tape, allow_partial.allow_trailing_str())
                            .map(|s| JsonValue::Str(create_cow(s))),
                        Peek::Array => {
                            let array = Arc::new(SmallVec::new());
                            match parser.array_first() {
                                Ok(Some(first_peek)) => {
                                    push_recursion!(first_peek, RecursedValue::Array(array));
                                    // immediately jump to process the first value in the array
                                    continue 'recursion;
                                }
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            };
                            Ok(JsonValue::Array(array))
                        }
                        Peek::Object => {
                            let object = Arc::new(LazyIndexMap::new());
                            match parser.object_first::<StringDecoder>(tape) {
                                Ok(Some(first_key)) => match parser.peek() {
                                    Ok(peek) => {
                                        push_recursion!(
                                            peek,
                                            RecursedValue::Object {
                                                partial: object,
                                                next_key: create_cow(first_key)
                                            }
                                        );
                                        continue 'recursion;
                                    }
                                    Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                    _ => (),
                                },
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            };
                            Ok(JsonValue::Object(object))
                        }
                        _ => parser
                            .consume_number::<NumberAny>(peek.into_inner(), allow_inf_nan)
                            .map_err(|e| {
                                if !peek.is_num() {
                                    json_error!(ExpectedSomeValue, parser.index)
                                } else {
                                    e
                                }
                            })
                            .map(|n| match n {
                                NumberAny::Int(NumberInt::Int(int)) => JsonValue::Int(int),
                                #[cfg(feature = "num-bigint")]
                                NumberAny::Int(NumberInt::BigInt(big_int)) => JsonValue::BigInt(big_int),
                                NumberAny::Float(float) => JsonValue::Float(float),
                            }),
                    };

                    let array = match result {
                        Ok(value) => {
                            // now try to advance position in the current array
                            match parser.array_step() {
                                Ok(Some(next_peek)) => {
                                    array.push(value);
                                    peek = next_peek;
                                    // array continuing
                                    continue;
                                }
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            };

                            let RecursedValue::Array(mut array) = current_recursion else {
                                unreachable!("known to be in array recursion");
                            };

                            Arc::get_mut(&mut array).expect("sole writer to value").push(value);
                            array
                        }
                        Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                        _ => {
                            let RecursedValue::Array(array) = current_recursion else {
                                unreachable!("known to be in array recursion");
                            };
                            array
                        }
                    };

                    break JsonValue::Array(array);
                }
            }
            RecursedValue::Object { partial, next_key } => {
                let partial = Arc::get_mut(partial).expect("sole writer");
                loop {
                    let result = match peek {
                        Peek::True => parser.consume_true().map(|()| JsonValue::Bool(true)),
                        Peek::False => parser.consume_false().map(|()| JsonValue::Bool(false)),
                        Peek::Null => parser.consume_null().map(|()| JsonValue::Null),
                        Peek::String => parser
                            .consume_string::<StringDecoder>(tape, allow_partial.allow_trailing_str())
                            .map(|s| JsonValue::Str(create_cow(s))),
                        Peek::Array => {
                            let array = Arc::new(SmallVec::new());
                            match parser.array_first() {
                                Ok(Some(first_peek)) => {
                                    push_recursion!(first_peek, RecursedValue::Array(array));
                                    // immediately jump to process the first value in the array
                                    continue 'recursion;
                                }
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            };
                            Ok(JsonValue::Array(array))
                        }
                        Peek::Object => {
                            let object = Arc::new(LazyIndexMap::new());
                            match parser.object_first::<StringDecoder>(tape) {
                                Ok(Some(first_key)) => match parser.peek() {
                                    Ok(peek) => {
                                        push_recursion!(
                                            peek,
                                            RecursedValue::Object {
                                                partial: object,
                                                next_key: create_cow(first_key)
                                            }
                                        );
                                        continue 'recursion;
                                    }
                                    Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                    _ => (),
                                },
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            };
                            Ok(JsonValue::Object(object))
                        }
                        _ => parser
                            .consume_number::<NumberAny>(peek.into_inner(), allow_inf_nan)
                            .map_err(|e| {
                                if !peek.is_num() {
                                    json_error!(ExpectedSomeValue, parser.index)
                                } else {
                                    e
                                }
                            })
                            .map(|n| match n {
                                NumberAny::Int(NumberInt::Int(int)) => JsonValue::Int(int),
                                #[cfg(feature = "num-bigint")]
                                NumberAny::Int(NumberInt::BigInt(big_int)) => JsonValue::BigInt(big_int),
                                NumberAny::Float(float) => JsonValue::Float(float),
                            }),
                    };

                    let object = match result {
                        Ok(value) => {
                            // now try to advance position in the current object
                            match parser.object_step::<StringDecoder>(tape) {
                                Ok(Some(yet_another_key)) => {
                                    match parser.peek() {
                                        Ok(next_peek) => {
                                            // object continuing
                                            partial.insert(
                                                std::mem::replace(next_key, create_cow(yet_another_key)),
                                                value,
                                            );
                                            peek = next_peek;
                                            continue;
                                        }
                                        Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                        _ => (),
                                    };
                                }
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            }

                            let RecursedValue::Object { mut partial, next_key } = current_recursion else {
                                unreachable!("known to be in object recursion");
                            };

                            Arc::get_mut(&mut partial).expect("sole writer").insert(next_key, value);
                            partial
                        }
                        Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                        _ => {
                            let RecursedValue::Object { partial, .. } = current_recursion else {
                                unreachable!("known to be in object recursion");
                            };
                            partial
                        }
                    };

                    break JsonValue::Object(object);
                }
            }
        };

        // current array or object has finished;
        // try to pop and continue with the parent
        peek = loop {
            if let Some(next_recursion) = recursion_stack.pop() {
                current_recursion = next_recursion;
            } else {
                return Ok(value);
            }

            value = match current_recursion {
                RecursedValue::Array(mut array) => {
                    Arc::get_mut(&mut array).expect("sole writer").push(value);
                    match parser.array_step() {
                        Ok(Some(next_peek)) => {
                            current_recursion = RecursedValue::Array(array);
                            break next_peek;
                        }
                        Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                        _ => (),
                    }
                    JsonValue::Array(array)
                }
                RecursedValue::Object { mut partial, next_key } => {
                    Arc::get_mut(&mut partial).expect("sole writer").insert(next_key, value);

                    match parser.object_step::<StringDecoder>(tape) {
                        Ok(Some(next_key)) => match parser.peek() {
                            Ok(next_peek) => {
                                current_recursion = RecursedValue::Object {
                                    partial,
                                    next_key: create_cow(next_key),
                                };
                                break next_peek;
                            }
                            Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                            _ => (),
                        },
                        Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                        _ => (),
                    }

                    JsonValue::Object(partial)
                }
            }
        };
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
