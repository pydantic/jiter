use std::borrow::Cow;
use std::fmt::Debug;
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
pub enum JsonValueBase<S: StrOwnership> {
    Null,
    Bool(bool),
    Int(i64),
    BigInt(BigInt),
    Float(f64),
    Str(S::Output),
    Array(JsonArray<S>),
    Object(JsonObject<S>),
}

pub type JsonArray<S> = Arc<SmallVec<[JsonValueBase<S>; 8]>>;
pub type JsonObject<S> = Arc<LazyIndexMap<<S as StrOwnership>::Output, JsonValueBase<S>>>;

pub type JsonValue<'j> = JsonValueBase<StrBorrowed<'j>>;
pub type JsonValueOwned = JsonValueBase<StrOwned>;

#[cfg(feature = "python")]
impl<S: StrOwnership> pyo3::ToPyObject for JsonValueBase<S> {
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

impl<'j, S: StrOwnership> JsonValueBase<S> {
    /// Parse a JSON enum from a byte slice.
    pub fn parse(data: &'j [u8], allow_inf_nan: bool) -> Result<Self, JsonError> {
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

pub(crate) fn take_value<'t, 'j, S: StrOwnership>(
    peek: Peek,
    parser: &mut Parser<'j>,
    tape: &'t mut Tape,
    mut recursion_limit: u8,
    allow_inf_nan: bool,
) -> JsonResult<JsonValueBase<S>>
where
    'j: 't,
{
    match peek {
        Peek::True => {
            parser.consume_true()?;
            Ok(JsonValueBase::Bool(true))
        }
        Peek::False => {
            parser.consume_false()?;
            Ok(JsonValueBase::Bool(false))
        }
        Peek::Null => {
            parser.consume_null()?;
            Ok(JsonValueBase::Null)
        }
        Peek::String => {
            let s: StringOutput<'t, 'j> = parser.consume_string::<StringDecoder>(tape)?;
            Ok(JsonValueBase::Str(S::take(s)))
        }
        Peek::Array => {
            // we could do something clever about guessing the size of the array
            let mut array: SmallVec<[JsonValueBase<S>; 8]> = SmallVec::new();
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
            Ok(JsonValueBase::Array(Arc::new(array)))
        }
        Peek::Object => {
            // same for objects
            let mut object: LazyIndexMap<S::Output, JsonValueBase<S>> = LazyIndexMap::new();
            if let Some(first_key) = parser.object_first::<StringDecoder>(tape)? {
                let first_key = S::take(first_key);
                let peek = parser.peek()?;
                check_recursion!(recursion_limit, parser.index,
                    let first_value = take_value(peek, parser, tape, recursion_limit, allow_inf_nan)?;
                );
                object.insert(first_key, first_value);
                while let Some(key) = parser.object_step::<StringDecoder>(tape)? {
                    let key = S::take(key);
                    let peek = parser.peek()?;
                    check_recursion!(recursion_limit, parser.index,
                        let value = take_value(peek, parser, tape, recursion_limit, allow_inf_nan)?;
                    );
                    object.insert(key, value);
                }
            }

            Ok(JsonValueBase::Object(Arc::new(object)))
        }
        _ => {
            let n = parser.consume_number::<NumberAny>(peek.into_inner(), allow_inf_nan);
            match n {
                Ok(NumberAny::Int(NumberInt::Int(int))) => Ok(JsonValueBase::Int(int)),
                Ok(NumberAny::Int(NumberInt::BigInt(big_int))) => Ok(JsonValueBase::BigInt(big_int)),
                Ok(NumberAny::Float(float)) => Ok(JsonValueBase::Float(float)),
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

pub trait StrOwnership: Debug {
    #[cfg(feature = "python")]
    type Output: Debug + Clone + Eq + std::hash::Hash + pyo3::ToPyObject;
    #[cfg(not(feature = "python"))]
    type Output: Debug + Clone + Eq + std::hash::Hash;

    fn take(str_output: StringOutput<'_, '_>) -> Self::Output;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StrBorrowed<'j> {
    _marker: std::marker::PhantomData<&'j ()>,
}

impl<'j> StrOwnership for StrBorrowed<'j> {
    type Output = Cow<'j, str>;

    fn take(str_output: StringOutput<'_, 'j>) -> Self::Output {
        str_output.into()
    }
    // fn take(str_output: StringOutput<'_, '_>) -> Self::Output {
    //     let s: String = str_output.into();
    //     Cow::Owned(s)
    // }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StrOwned;

impl StrOwnership for StrOwned {
    type Output = String;

    fn take(str_output: StringOutput<'_, '_>) -> Self::Output {
        str_output.into()
    }
}
