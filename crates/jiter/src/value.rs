use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

#[cfg(feature = "num-bigint")]
use num_bigint::BigInt;
use smallvec::SmallVec;

use crate::PartialMode;
use crate::errors::{DEFAULT_RECURSION_LIMIT, JsonError, JsonResult, json_error};
use crate::number_decoder::{NumberAny, NumberInt, NumberRange};
use crate::parse::{Parser, Peek};
use crate::string_decoder::{StringDecoder, StringDecoderRange, StringOutput, Tape};

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

/// Parsed JSON array.
pub type JsonArray<'s> = Arc<Vec<JsonValue<'s>>>;
/// Parsed JSON object. Note that `jiter` does not attempt to deduplicate keys,
/// so it is possible that the key occurs multiple times in the object.
///
/// It is up to the user to handle this case and decide how to proceed.
pub type JsonObject<'s> = Arc<Vec<(Cow<'s, str>, JsonValue<'s>)>>;

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

    fn empty_array() -> JsonValue<'static> {
        static EMPTY_ARRAY: OnceLock<JsonArray<'static>> = OnceLock::new();
        JsonValue::Array(EMPTY_ARRAY.get_or_init(|| Arc::new(Vec::new())).clone())
    }

    fn empty_object() -> JsonValue<'static> {
        static EMPTY_OBJECT: OnceLock<JsonObject<'static>> = OnceLock::new();
        JsonValue::Object(EMPTY_OBJECT.get_or_init(|| Arc::new(Vec::new())).clone())
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
        JsonValue::Array(v) => JsonValue::Array(Arc::new(v.iter().map(JsonValue::to_static).collect())),
        JsonValue::Object(o) => JsonValue::Object(Arc::new(
            o.iter()
                .map(|(k, v)| (k.clone().into_owned().into(), v.to_static()))
                .collect(),
        )),
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
            let peek_first = match parser.array_first() {
                Ok(Some(peek)) => peek,
                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                Ok(None) | Err(_) => return Ok(JsonValue::empty_array()),
            };
            take_value_recursive(
                peek_first,
                RecursedValue::Array { base: 0 },
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
            let first_key = match parser.object_first::<StringDecoder>(tape) {
                Ok(Some(first_key)) => first_key,
                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                _ => return Ok(JsonValue::empty_object()),
            };
            let first_key = create_cow(first_key);
            match parser.peek() {
                Ok(peek) => take_value_recursive(
                    peek,
                    RecursedValue::Object {
                        base: 0,
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
                _ => Ok(JsonValue::empty_object()),
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

/// A container being parsed, as the position its contents start at in the stack shared by every
/// container of its kind, see [`take_value_recursive`].
enum RecursedValue<'s> {
    /// Array in progress; its elements start at `base` in the shared `elements` stack.
    Array { base: usize },
    /// Object in progress; its members start at `base` in the shared `members` stack; `next_key` awaits its value.
    Object { base: usize, next_key: Cow<'s, str> },
}

/// The contents of a container that has just closed, taken off the stack they were built on.
///
/// `base` of zero means nothing else is on that stack — no enclosing container of the same kind
/// has anything on it yet — so the stack itself is the container and can be handed over whole.
/// That is the whole document for the common single-container case, where copying it out would be
/// pure overhead, and the outermost array of a big one, which would otherwise be the largest
/// copy of all.
#[inline]
fn take_container<T>(stack: &mut Vec<T>, base: usize) -> Vec<T> {
    if base == 0 {
        std::mem::take(stack)
    } else {
        stack.split_off(base)
    }
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

    // Every array in the document shares one stack of elements, every object one stack of
    // members. Containers close in the order they opened, so the contents of the one being parsed
    // are always the top of the stack, from its `base` up; closing it copies them out into an
    // allocation of exactly the right size. A `Vec` per container has to guess that size instead,
    // and pays a run of reallocations for guessing low.
    // Only the root container's stack is allocated up front; a document that never opens a
    // container of the other kind never pays for its stack.
    let (mut elements, mut members): (Vec<JsonValue<'s>>, Vec<(Cow<'s, str>, JsonValue<'s>)>) = match &current_recursion
    {
        RecursedValue::Array { .. } => (Vec::with_capacity(8), Vec::new()),
        RecursedValue::Object { .. } => (Vec::new(), Vec::with_capacity(8)),
    };

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
            RecursedValue::Array { .. } => {
                loop {
                    let result = match peek {
                        Peek::True => parser.consume_true().map(|()| JsonValue::Bool(true)),
                        Peek::False => parser.consume_false().map(|()| JsonValue::Bool(false)),
                        Peek::Null => parser.consume_null().map(|()| JsonValue::Null),
                        Peek::String => parser
                            .consume_string::<StringDecoder>(tape, allow_partial.allow_trailing_str())
                            .map(|s| JsonValue::Str(create_cow(s))),
                        Peek::Array => {
                            match parser.array_first() {
                                Ok(Some(first_peek)) => {
                                    push_recursion!(first_peek, RecursedValue::Array { base: elements.len() });
                                    // immediately jump to process the first value in the array
                                    continue 'recursion;
                                }
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            }
                            Ok(JsonValue::empty_array())
                        }
                        Peek::Object => {
                            match parser.object_first::<StringDecoder>(tape) {
                                Ok(Some(first_key)) => match parser.peek() {
                                    Ok(peek) => {
                                        push_recursion!(
                                            peek,
                                            RecursedValue::Object {
                                                base: members.len(),
                                                next_key: create_cow(first_key),
                                            }
                                        );
                                        continue 'recursion;
                                    }
                                    Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                    _ => (),
                                },
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            }
                            Ok(JsonValue::empty_object())
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

                    let base = match result {
                        Ok(value) => {
                            // now try to advance position in the current array
                            match parser.array_step() {
                                Ok(Some(next_peek)) => {
                                    elements.push(value);
                                    peek = next_peek;
                                    // array continuing
                                    continue;
                                }
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            }

                            let RecursedValue::Array { base } = current_recursion else {
                                unreachable!("known to be in array recursion");
                            };
                            elements.push(value);
                            base
                        }
                        Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                        _ => {
                            let RecursedValue::Array { base } = current_recursion else {
                                unreachable!("known to be in array recursion");
                            };
                            base
                        }
                    };

                    break JsonValue::Array(Arc::new(take_container(&mut elements, base)));
                }
            }
            RecursedValue::Object { next_key, .. } => {
                loop {
                    let result = match peek {
                        Peek::True => parser.consume_true().map(|()| JsonValue::Bool(true)),
                        Peek::False => parser.consume_false().map(|()| JsonValue::Bool(false)),
                        Peek::Null => parser.consume_null().map(|()| JsonValue::Null),
                        Peek::String => parser
                            .consume_string::<StringDecoder>(tape, allow_partial.allow_trailing_str())
                            .map(|s| JsonValue::Str(create_cow(s))),
                        Peek::Array => {
                            match parser.array_first() {
                                Ok(Some(first_peek)) => {
                                    push_recursion!(first_peek, RecursedValue::Array { base: elements.len() });
                                    // immediately jump to process the first value in the array
                                    continue 'recursion;
                                }
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            }
                            Ok(JsonValue::empty_array())
                        }
                        Peek::Object => {
                            match parser.object_first::<StringDecoder>(tape) {
                                Ok(Some(first_key)) => match parser.peek() {
                                    Ok(peek) => {
                                        push_recursion!(
                                            peek,
                                            RecursedValue::Object {
                                                base: members.len(),
                                                next_key: create_cow(first_key),
                                            }
                                        );
                                        continue 'recursion;
                                    }
                                    Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                    _ => (),
                                },
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            }
                            Ok(JsonValue::empty_object())
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

                    let base = match result {
                        Ok(value) => {
                            // now try to advance position in the current object
                            match parser.object_step::<StringDecoder>(tape) {
                                Ok(Some(yet_another_key)) => {
                                    match parser.peek() {
                                        Ok(next_peek) => {
                                            // object continuing
                                            members.push((
                                                std::mem::replace(next_key, create_cow(yet_another_key)),
                                                value,
                                            ));
                                            peek = next_peek;
                                            continue;
                                        }
                                        Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                        _ => (),
                                    }
                                }
                                Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                                _ => (),
                            }

                            let RecursedValue::Object { base, next_key } = current_recursion else {
                                unreachable!("known to be in object recursion");
                            };
                            members.push((next_key, value));
                            base
                        }
                        Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                        _ => {
                            let RecursedValue::Object { base, .. } = current_recursion else {
                                unreachable!("known to be in object recursion");
                            };
                            base
                        }
                    };

                    break JsonValue::Object(Arc::new(take_container(&mut members, base)));
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
                RecursedValue::Array { base } => {
                    elements.push(value);
                    match parser.array_step() {
                        Ok(Some(next_peek)) => {
                            current_recursion = RecursedValue::Array { base };
                            break next_peek;
                        }
                        Err(e) if !(partial_active && e.allowed_if_partial()) => return Err(e),
                        _ => (),
                    }
                    JsonValue::Array(Arc::new(take_container(&mut elements, base)))
                }
                RecursedValue::Object { base, next_key } => {
                    members.push((next_key, value));

                    match parser.object_step::<StringDecoder>(tape) {
                        Ok(Some(next_key)) => match parser.peek() {
                            Ok(next_peek) => {
                                current_recursion = RecursedValue::Object {
                                    base,
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

                    JsonValue::Object(Arc::new(take_container(&mut members, base)))
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
        }

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
