//! A [`serde`] [`Deserializer`](::serde::Deserializer) backed by jiter's parser.
//!
//! Unescaped strings are deserialized zero-copy: the parser leaves them in the input, so a borrowed
//! `&'de str` is handed to [`visit_borrowed_str`]. Escaped strings are decoded onto a scratch tape
//! and passed transiently to [`visit_str`]. Borrowing a string into the target therefore requires
//! the input to outlive the deserialized value.
//!
//! [`visit_borrowed_str`]: ::serde::de::Visitor::visit_borrowed_str
//! [`visit_str`]: ::serde::de::Visitor::visit_str
//!
//! ## Numbers
//!
//! Integers up to `i64`/`u64` deserialize losslessly. Larger integers need the `num-bigint` feature
//! (on by default): values widen to `i128`/`u128`, falling back to `f64` past `u128`. Without it,
//! integers outside the `i64` range error with [`JsonErrorType::NumberOutOfRange`].
//!
//! Non-finite floats (`NaN`/`Infinity`, enabled via [`JiterDeserializer::with_allow_inf_nan`], or
//! produced by overflow) reach a typed `f64`/`f32` target as the real value, but a
//! `deserialize_any` target (which can't hold them) receives the canonical string
//! `"NaN"`/`"Infinity"`/`"-Infinity"`.
//!
//! ```rust
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, PartialEq, Debug)]
//! struct Person<'a> {
//!     name: &'a str,
//!     age: u8,
//! }
//!
//! let person: Person = jiter::serde::from_str(r#"{"name": "John", "age": 43}"#).unwrap();
//! assert_eq!(person, Person { name: "John", age: 43 });
//! ```

use std::fmt::{self, Display};

use serde::de::{self, Deserializer as _, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess, Visitor};
use serde::forward_to_deserialize_any;

use crate::errors::{DEFAULT_RECURSION_LIMIT, JsonError, JsonErrorType, LinePosition};
use crate::number_decoder::{NumberAny, NumberInt};
use crate::parse::{Parser, Peek};
use crate::string_decoder::{StringDecoder, StringOutput, StringOutputType, Tape};
use crate::value::take_value_skip;

/// Ways a JSON value can fail to match an enum's expected shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnumError {
    /// neither a string (unit variant) nor a single-key object
    NotStringOrSingleKeyObject,
    /// the object did not contain exactly one key
    NotSingleKey,
}

impl Display for EnumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotStringOrSingleKeyObject => "expected a string or single-key object for an enum",
            Self::NotSingleKey => "expected a single-key object for an enum",
        })
    }
}

/// An error from deserializing JSON with [`JiterDeserializer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A JSON syntax error from the parser.
    Syntax(JsonError),
    /// A `serde` error (type mismatch, missing field, …); `index` is set when the position is known.
    Data { message: String, index: Option<usize> },
    /// A malformed enum encoding.
    Enum { kind: EnumError, index: usize },
}

impl Error {
    /// The byte index where the error occurred, if known.
    pub fn index(&self) -> Option<usize> {
        match self {
            Self::Syntax(err) => Some(err.index),
            Self::Data { index, .. } => *index,
            Self::Enum { index, .. } => Some(*index),
        }
    }

    /// Line/column of the error within `data`, if the position is known.
    pub fn get_position(&self, data: &[u8]) -> Option<LinePosition> {
        self.index().map(|index| LinePosition::find(data, index))
    }

    /// The error with its line/column resolved against `data` (when known).
    pub fn description(&self, data: &[u8]) -> String {
        match self {
            Self::Syntax(err) => err.description(data),
            Self::Data {
                message,
                index: Some(index),
            } => format!("{message} at {}", LinePosition::find(data, *index)),
            Self::Data { message, index: None } => message.to_string(),
            Self::Enum { kind, index } => format!("{kind} at {}", LinePosition::find(data, *index)),
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(err) => write!(f, "{err}"),
            Self::Data {
                message,
                index: Some(index),
            } => write!(f, "{message} at index {index}"),
            Self::Data { message, index: None } => f.write_str(message),
            Self::Enum { kind, index } => write!(f, "{kind} at index {index}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Syntax(err) => Some(err),
            Self::Data { .. } | Self::Enum { .. } => None,
        }
    }
}

impl From<JsonError> for Error {
    fn from(err: JsonError) -> Self {
        Self::Syntax(err)
    }
}

impl de::Error for Error {
    fn custom<T: Display>(msg: T) -> Self {
        // no position available here; `stamp_index` fills it in as the error unwinds
        Self::Data {
            message: msg.to_string(),
            index: None,
        }
    }
}

/// Fill in the position of an as-yet-unpositioned [`Error::Data`] as it unwinds through
/// `deserialize_any`, where the parser is frozen at the failing value.
fn stamp_index(mut err: Error, index: usize) -> Error {
    if let Error::Data { index: slot @ None, .. } = &mut err {
        *slot = Some(index);
    }
    err
}

/// Deserialize an instance of `T` from a slice of JSON bytes.
pub fn from_slice<'a, T>(data: &'a [u8]) -> Result<T, Error>
where
    T: serde::Deserialize<'a>,
{
    let mut de = JiterDeserializer::new(data);
    let value = T::deserialize(&mut de)?;
    de.finish()?;
    Ok(value)
}

/// Deserialize an instance of `T` from a string of JSON.
pub fn from_str<'a, T>(data: &'a str) -> Result<T, Error>
where
    T: serde::Deserialize<'a>,
{
    from_slice(data.as_bytes())
}

/// A [`serde::Deserializer`] backed by jiter's parser.
#[derive(Debug)]
pub struct JiterDeserializer<'j> {
    parser: Parser<'j>,
    tape: Tape,
    allow_inf_nan: bool,
    /// Max nesting depth; `None` disables the check.
    recursion_limit: Option<usize>,
    /// Current nesting depth.
    depth: usize,
}

impl<'j> JiterDeserializer<'j> {
    /// Construct a new `JiterDeserializer` from a slice of JSON bytes.
    pub fn new(data: &'j [u8]) -> Self {
        Self {
            parser: Parser::new(data),
            tape: Tape::new(),
            allow_inf_nan: false,
            recursion_limit: Some(usize::from(DEFAULT_RECURSION_LIMIT)),
            depth: 0,
        }
    }

    /// Allow `NaN`, `Infinity` and `-Infinity` in numbers (off by default).
    pub fn with_allow_inf_nan(mut self) -> Self {
        self.allow_inf_nan = true;
        self
    }

    /// Set the max nesting depth (default [`DEFAULT_RECURSION_LIMIT`]); deeper input errors with
    /// [`JsonErrorType::RecursionLimitExceeded`].
    pub fn with_recursion_limit(mut self, limit: usize) -> Self {
        self.recursion_limit = Some(limit);
        self
    }

    /// Disable the recursion limit; deeply nested input may then overflow the stack.
    pub fn disable_recursion_limit(mut self) -> Self {
        self.recursion_limit = None;
        self
    }

    /// Error unless all input has been consumed.
    pub fn finish(&mut self) -> Result<(), Error> {
        self.parser.finish().map_err(Error::from)
    }

    /// Enter a nested array/object, enforcing the recursion limit.
    fn enter(&mut self) -> Result<(), Error> {
        self.depth += 1;
        match self.recursion_limit {
            Some(limit) if self.depth > limit => Err(Error::Syntax(JsonError::new(
                JsonErrorType::RecursionLimitExceeded,
                self.parser.index,
            ))),
            _ => Ok(()),
        }
    }

    /// Leave a nested array/object.
    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Remaining depth budget for [`take_value_skip`] (`u8`-limited); `u8::MAX` when disabled.
    fn skip_recursion_budget(&self) -> u8 {
        match self.recursion_limit {
            None => u8::MAX,
            Some(limit) => limit.saturating_sub(self.depth).try_into().unwrap_or(u8::MAX),
        }
    }
}

impl<'de> de::Deserializer<'de> for &mut JiterDeserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let peek = self.parser.peek()?;
        // value start, used to position a serde error raised by the visitor
        let start = self.parser.index;
        match peek {
            Peek::Null => {
                self.parser.consume_null()?;
                visitor.visit_unit()
            }
            Peek::True => {
                self.parser.consume_true()?;
                visitor.visit_bool(true)
            }
            Peek::False => {
                self.parser.consume_false()?;
                visitor.visit_bool(false)
            }
            Peek::String => {
                let output = self.parser.consume_string::<StringDecoder>(&mut self.tape, false)?;
                visit_str_output(&output, visitor)
            }
            Peek::Array => {
                self.enter()?;
                let mut seq = JiterSeqAccess {
                    de: &mut *self,
                    first: true,
                    ended: false,
                };
                let value = visitor.visit_seq(&mut seq)?;
                // consume `]`, rejecting any elements a fixed-size consumer left behind
                seq.finish()?;
                self.leave();
                Ok(value)
            }
            Peek::Object => {
                self.enter()?;
                let value = visitor.visit_map(JiterMapAccess {
                    de: &mut *self,
                    first: true,
                })?;
                self.leave();
                Ok(value)
            }
            // otherwise a number (or an invalid token, which `consume_number` reports)
            _ => match self
                .parser
                .consume_number::<NumberAny>(peek.into_inner(), self.allow_inf_nan)?
            {
                NumberAny::Int(int) => visit_int(int, visitor),
                NumberAny::Float(f) if f.is_finite() => visitor.visit_f64(f),
                // this target can't hold a non-finite float; emit the canonical string instead.
                // typed `f64`/`f32` bypass this via `deserialize_number` and get the real value.
                NumberAny::Float(f) => visitor.visit_borrowed_str(non_finite_str(f)),
            },
        }
        .map_err(|e| stamp_index(e, start))
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.parser.peek()? == Peek::Null {
            self.parser.consume_null()?;
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        match self.parser.peek()? {
            // bare string: unit variant
            Peek::String => visitor.visit_enum(JiterEnumAccess { de: self, unit: true }),
            // single-key object: the wrapper counts toward the recursion limit like any nesting,
            // otherwise a recursive enum could bypass the limit and overflow the stack
            Peek::Object => {
                self.enter()?;
                let value = visitor.visit_enum(JiterEnumAccess {
                    de: &mut *self,
                    unit: false,
                })?;
                self.leave();
                Ok(value)
            }
            _ => Err(Error::Enum {
                kind: EnumError::NotStringOrSingleKeyObject,
                index: self.parser.index,
            }),
        }
    }

    /// Typed float target: receives the real value, including non-finite (unlike `deserialize_any`).
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        deserialize_number(self, visitor)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        deserialize_number(self, visitor)
    }

    /// Skip the next value without decoding it.
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let peek = self.parser.peek()?;
        let recursion_limit = self.skip_recursion_budget();
        take_value_skip(
            peek,
            &mut self.parser,
            &mut self.tape,
            recursion_limit,
            self.allow_inf_nan,
        )?;
        visitor.visit_unit()
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier
    }
}

/// Deserialize a number for a typed float target, keeping non-finite values as real `f64`s.
fn deserialize_number<'de, V: Visitor<'de>>(de: &mut JiterDeserializer<'de>, visitor: V) -> Result<V::Value, Error> {
    let peek = de.parser.peek()?;
    if peek.is_num() {
        match de
            .parser
            .consume_number::<NumberAny>(peek.into_inner(), de.allow_inf_nan)?
        {
            NumberAny::Int(int) => visit_int(int, visitor),
            NumberAny::Float(f) => visitor.visit_f64(f),
        }
    } else {
        // not a number: let `deserialize_any` produce the type error
        (&mut *de).deserialize_any(visitor)
    }
}

/// Canonical text for a non-finite float.
fn non_finite_str(f: f64) -> &'static str {
    if f.is_nan() {
        "NaN"
    } else if f < 0.0 {
        "-Infinity"
    } else {
        "Infinity"
    }
}

/// Visit a decoded string, borrowing from the input when it wasn't escaped.
fn visit_str_output<'de, V: Visitor<'de>>(output: &StringOutput<'_, 'de>, visitor: V) -> Result<V::Value, Error> {
    match &output.data {
        // unescaped: borrow `&'de str`
        StringOutputType::Data(s) => visitor.visit_borrowed_str(s),
        // escaped: transient tape borrow
        StringOutputType::Tape(s) => visitor.visit_str(s),
    }
}

/// Visit an integer, narrowing big ints to the smallest primitive that fits.
fn visit_int<'de, V: Visitor<'de>>(int: NumberInt, visitor: V) -> Result<V::Value, Error> {
    match int {
        NumberInt::Int(i) => visitor.visit_i64(i),
        #[cfg(feature = "num-bigint")]
        NumberInt::BigInt(big) => {
            use num_traits::ToPrimitive;
            if let Some(i) = big.to_i128() {
                visitor.visit_i128(i)
            } else if let Some(u) = big.to_u128() {
                visitor.visit_u128(u)
            } else {
                visitor.visit_f64(big.to_f64().unwrap_or(f64::NAN))
            }
        }
    }
}

struct JiterSeqAccess<'a, 'de> {
    de: &'a mut JiterDeserializer<'de>,
    first: bool,
    /// `true` once the closing `]` has been consumed.
    ended: bool,
}

impl JiterSeqAccess<'_, '_> {
    /// The next element's [`Peek`], or `None` (consuming `]`) at the end.
    fn step(&mut self) -> Result<Option<Peek>, Error> {
        let peek = if std::mem::take(&mut self.first) {
            self.de.parser.array_first()?
        } else {
            self.de.parser.array_step()?
        };
        if peek.is_none() {
            self.ended = true;
        }
        Ok(peek)
    }

    /// Consume `]`; error if elements remain (a fixed-size target took fewer than the array holds).
    fn finish(&mut self) -> Result<(), Error> {
        while !self.ended {
            if self.step()?.is_some() {
                return Err(Error::Syntax(JsonError::new(
                    JsonErrorType::TrailingCharacters,
                    self.de.parser.index,
                )));
            }
        }
        Ok(())
    }
}

impl<'de> SeqAccess<'de> for &mut JiterSeqAccess<'_, 'de> {
    type Error = Error;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error> {
        // the parser is left at the next value, which `deserialize_any`'s `peek` reads idempotently
        match self.step()? {
            Some(_) => seed.deserialize(&mut *self.de).map(Some),
            None => Ok(None),
        }
    }
}

struct JiterMapAccess<'a, 'de> {
    de: &'a mut JiterDeserializer<'de>,
    first: bool,
}

impl<'de> MapAccess<'de> for JiterMapAccess<'_, 'de> {
    type Error = Error;

    fn next_key_seed<K: de::DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error> {
        let output = if std::mem::take(&mut self.first) {
            self.de.parser.object_first::<StringDecoder>(&mut self.de.tape)?
        } else {
            self.de.parser.object_step::<StringDecoder>(&mut self.de.tape)?
        };
        // the key parse also consumes the colon; the key is fully deserialized here, before the
        // value reuses the tape, so a tape-backed key is safe
        match output {
            Some(output) => seed.deserialize(MapKeyDeserializer { key: output.data }).map(Some),
            None => Ok(None),
        }
    }

    fn next_value_seed<V: de::DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Self::Error> {
        seed.deserialize(&mut *self.de)
    }
}

/// Deserializer for object keys: borrows the key string, or parses it for primitive targets.
struct MapKeyDeserializer<'t, 'de> {
    key: StringOutputType<'t, 'de>,
}

impl MapKeyDeserializer<'_, '_> {
    fn as_str(&self) -> &str {
        match self.key {
            StringOutputType::Tape(s) | StringOutputType::Data(s) => s,
        }
    }
}

macro_rules! deserialize_key_parsed {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            match self.as_str().parse::<$ty>() {
                Ok(value) => visitor.$visit(value),
                Err(_) => self.deserialize_any(visitor),
            }
        }
    };
}

impl<'de> de::Deserializer<'de> for MapKeyDeserializer<'_, 'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.key {
            StringOutputType::Data(s) => visitor.visit_borrowed_str(s),
            StringOutputType::Tape(s) => visitor.visit_str(s),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        // the key string is the (unit) variant name
        let de: de::value::StrDeserializer<Error> = self.as_str().into_deserializer();
        de.deserialize_enum(name, variants, visitor)
    }

    // parse the key string for primitive targets (e.g. integer keys); fall back to the string on
    // failure so the visitor reports the type error
    deserialize_key_parsed!(deserialize_bool, visit_bool, bool);
    deserialize_key_parsed!(deserialize_i8, visit_i8, i8);
    deserialize_key_parsed!(deserialize_i16, visit_i16, i16);
    deserialize_key_parsed!(deserialize_i32, visit_i32, i32);
    deserialize_key_parsed!(deserialize_i64, visit_i64, i64);
    deserialize_key_parsed!(deserialize_i128, visit_i128, i128);
    deserialize_key_parsed!(deserialize_u8, visit_u8, u8);
    deserialize_key_parsed!(deserialize_u16, visit_u16, u16);
    deserialize_key_parsed!(deserialize_u32, visit_u32, u32);
    deserialize_key_parsed!(deserialize_u64, visit_u64, u64);
    deserialize_key_parsed!(deserialize_u128, visit_u128, u128);
    deserialize_key_parsed!(deserialize_f32, visit_f32, f32);
    deserialize_key_parsed!(deserialize_f64, visit_f64, f64);

    forward_to_deserialize_any! {
        char str string bytes byte_buf option unit unit_struct newtype_struct
        seq tuple tuple_struct map struct identifier ignored_any
    }
}

struct JiterEnumAccess<'a, 'de> {
    de: &'a mut JiterDeserializer<'de>,
    /// `true` for the bare-string (unit variant) form.
    unit: bool,
}

impl<'a, 'de> EnumAccess<'de> for JiterEnumAccess<'a, 'de> {
    type Error = Error;
    type Variant = JiterVariantAccess<'a, 'de>;

    fn variant_seed<V: de::DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error> {
        let key = if self.unit {
            self.de
                .parser
                .consume_string::<StringDecoder>(&mut self.de.tape, false)?
                .data
        } else {
            let start = self.de.parser.index;
            // consumes `{` and the variant key, leaving the parser at the variant's value
            match self.de.parser.object_first::<StringDecoder>(&mut self.de.tape)? {
                Some(output) => output.data,
                None => {
                    return Err(Error::Enum {
                        kind: EnumError::NotSingleKey,
                        index: start,
                    });
                }
            }
        };
        let value = seed.deserialize(MapKeyDeserializer { key })?;
        Ok((
            value,
            JiterVariantAccess {
                de: self.de,
                unit: self.unit,
            },
        ))
    }
}

struct JiterVariantAccess<'a, 'de> {
    de: &'a mut JiterDeserializer<'de>,
    unit: bool,
}

impl JiterVariantAccess<'_, '_> {
    /// Consume the closing `}` of an object variant, erroring on extra keys.
    fn finish(&mut self) -> Result<(), Error> {
        if self.unit {
            Ok(())
        } else {
            match self.de.parser.object_step::<StringDecoder>(&mut self.de.tape)? {
                None => Ok(()),
                Some(_) => Err(Error::Enum {
                    kind: EnumError::NotSingleKey,
                    index: self.de.parser.index,
                }),
            }
        }
    }
}

impl<'de> VariantAccess<'de> for JiterVariantAccess<'_, 'de> {
    type Error = Error;

    fn unit_variant(mut self) -> Result<(), Self::Error> {
        if self.unit {
            // bare-string form
            Ok(())
        } else {
            // object form: the content must be `()` (i.e. `null`); consume it, then close the object
            <() as de::Deserialize>::deserialize(&mut *self.de)?;
            self.finish()
        }
    }

    fn newtype_variant_seed<T: de::DeserializeSeed<'de>>(mut self, seed: T) -> Result<T::Value, Self::Error> {
        let value = seed.deserialize(&mut *self.de)?;
        self.finish()?;
        Ok(value)
    }

    fn tuple_variant<V: Visitor<'de>>(mut self, _len: usize, visitor: V) -> Result<V::Value, Self::Error> {
        let value = (&mut *self.de).deserialize_seq(visitor)?;
        self.finish()?;
        Ok(value)
    }

    fn struct_variant<V: Visitor<'de>>(
        mut self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let value = (&mut *self.de).deserialize_map(visitor)?;
        self.finish()?;
        Ok(value)
    }
}
