use std::sync::Arc;

use jiter::{JsonValue, LazyIndexMap};
use num_bigint::Sign;
use smallvec::smallvec;

use crate::decoder::Decoder;
use crate::errors::{DecodeErrorType, DecodeResult};
use crate::json_writer::JsonWriter;
use crate::ToJsonResult;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum Header {
    Null,
    Bool(bool),
    Int(NumberHint),
    IntBig(Sign, Length),
    Float(NumberHint),
    Str(Length),
    Object(Length),
    // array types in order of their complexity
    #[allow(clippy::enum_variant_names)]
    HeaderArray(Length),
    U8Array(Length),
    I64Array(Length),
    HetArray(Length),
}

impl Header {
    /// Decode the next byte from a decoder into a header value
    pub fn decode(byte: u8, d: &Decoder) -> DecodeResult<Self> {
        let (left, right) = split_byte(byte);
        let cat = Category::from_u8(left, d)?;
        match cat {
            Category::Primitive => Primitive::from_u8(right, d).map(Primitive::header_value),
            Category::Int => NumberHint::from_u8(right, d).map(Self::Int),
            Category::BigIntPos => Length::from_u8(right, d).map(|l| Self::IntBig(Sign::Plus, l)),
            Category::BigIntNeg => Length::from_u8(right, d).map(|l| Self::IntBig(Sign::Minus, l)),
            Category::Float => NumberHint::from_u8(right, d).map(Self::Float),
            Category::Str => Length::from_u8(right, d).map(Self::Str),
            Category::Object => Length::from_u8(right, d).map(Self::Object),
            Category::HeaderArray => Length::from_u8(right, d).map(Self::HeaderArray),
            Category::U8Array => Length::from_u8(right, d).map(Self::U8Array),
            Category::I64Array => Length::from_u8(right, d).map(Self::I64Array),
            Category::HetArray => Length::from_u8(right, d).map(Self::HetArray),
        }
    }

    /// TODO `'static` should be okay as return lifetime, I don't know why it's not
    pub fn header_as_value<'b>(self, _: &Decoder<'b>) -> JsonValue<'b> {
        match self {
            Header::Null => JsonValue::Null,
            Header::Bool(b) => JsonValue::Bool(b),
            Header::Int(n) => JsonValue::Int(n.decode_i64_header()),
            Header::IntBig(..) => unreachable!("Big ints are not supported as header only values"),
            Header::Float(n) => JsonValue::Float(n.decode_f64_header()),
            Header::Str(_) => JsonValue::Str("".into()),
            Header::Object(_) => JsonValue::Object(Arc::new(LazyIndexMap::default())),
            _ => JsonValue::Array(Arc::new(smallvec![])),
        }
    }

    pub fn write_json_header_only(self, writer: &mut JsonWriter) -> ToJsonResult<()> {
        match self {
            Header::Null => writer.write_null(),
            Header::Bool(b) => writer.write_value(b)?,
            Header::Int(n) => writer.write_value(n.decode_i64_header())?,
            Header::IntBig(..) => return Err("Big ints are not supported as header only values".into()),
            Header::Float(n) => writer.write_value(n.decode_f64_header())?,
            // TODO check the
            Header::Str(len) => {
                len.check_empty()?;
                writer.write_value("")?;
            }
            Header::Object(len) => {
                len.check_empty()?;
                writer.write_empty_object();
            }
            Self::HeaderArray(len) | Self::U8Array(len) | Self::I64Array(len) | Self::HetArray(len) => {
                len.check_empty()?;
                writer.write_empty_array();
            }
        }
        Ok(())
    }

    pub fn into_bool(self) -> Option<bool> {
        match self {
            Header::Bool(b) => Some(b),
            _ => None,
        }
    }
}

macro_rules! impl_from_u8 {
    ($header_enum:ty, $max_value:literal) => {
        impl $header_enum {
            fn from_u8(value: u8, p: &Decoder) -> DecodeResult<Self> {
                if value <= $max_value {
                    Ok(unsafe { std::mem::transmute::<u8, $header_enum>(value) })
                } else {
                    Err(p.error(DecodeErrorType::HeaderInvalid {
                        value,
                        ty: stringify!($header_enum),
                    }))
                }
            }
        }
    };
}

/// Left half of the first header byte determines the category of the value
/// Up to 16 categories are possible
#[derive(Debug, Copy, Clone)]
pub(crate) enum Category {
    Primitive = 0,
    Int = 1,
    BigIntPos = 2,
    BigIntNeg = 3,
    Float = 4,
    Str = 5,
    Object = 6,
    HeaderArray = 7,
    U8Array = 8,
    I64Array = 9,
    HetArray = 10,
}
impl_from_u8!(Category, 10);

impl Category {
    pub fn encode_with(self, right: u8) -> u8 {
        let left = self as u8;
        (left << 4) | right
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum Primitive {
    Null = 0,
    True = 1,
    False = 2,
}
impl_from_u8!(Primitive, 2);

impl From<bool> for Primitive {
    fn from(value: bool) -> Self {
        if value {
            Self::True
        } else {
            Self::False
        }
    }
}

impl Primitive {
    fn header_value(self) -> Header {
        match self {
            Primitive::Null => Header::Null,
            Primitive::True => Header::Bool(true),
            Primitive::False => Header::Bool(false),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum NumberHint {
    Zero = 0,
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    // larger numbers
    Size8 = 11,
    Size32 = 12,
    Size64 = 13,
}
impl_from_u8!(NumberHint, 13);

impl NumberHint {
    pub fn decode_i64(self, d: &mut Decoder) -> DecodeResult<i64> {
        match self {
            NumberHint::Size8 => d.take_i8().map(i64::from),
            NumberHint::Size32 => d.take_i32().map(i64::from),
            NumberHint::Size64 => d.take_i64(),
            // TODO check this has same performance as inline match
            _ => Ok(self.decode_i64_header()),
        }
    }

    #[inline]
    pub fn decode_i64_header(self) -> i64 {
        match self {
            NumberHint::Zero => 0,
            NumberHint::One => 1,
            NumberHint::Two => 2,
            NumberHint::Three => 3,
            NumberHint::Four => 4,
            NumberHint::Five => 5,
            NumberHint::Six => 6,
            NumberHint::Seven => 7,
            NumberHint::Eight => 8,
            NumberHint::Nine => 9,
            NumberHint::Ten => 10,
            _ => unreachable!("Expected concrete value, got {self:?}"),
        }
    }

    pub fn decode_f64(self, d: &mut Decoder) -> DecodeResult<f64> {
        match self {
            // f8 doesn't exist, and currently we don't use f32 anywhere
            NumberHint::Size8 | NumberHint::Size32 => Err(d.error(DecodeErrorType::HeaderInvalid {
                value: self as u8,
                ty: "f64",
            })),
            NumberHint::Size64 => d.take_f64(),
            // TODO check this has same performance as inline match
            _ => Ok(self.decode_f64_header()),
        }
    }

    #[inline]
    fn decode_f64_header(self) -> f64 {
        match self {
            NumberHint::Zero => 0.0,
            NumberHint::One => 1.0,
            NumberHint::Two => 2.0,
            NumberHint::Three => 3.0,
            NumberHint::Four => 4.0,
            NumberHint::Five => 5.0,
            NumberHint::Six => 6.0,
            NumberHint::Seven => 7.0,
            NumberHint::Eight => 8.0,
            NumberHint::Nine => 9.0,
            NumberHint::Ten => 10.0,
            _ => unreachable!("Expected concrete value, got {self:?}"),
        }
    }

    pub fn header_only_i64(int: i64) -> Option<Self> {
        match int {
            0 => Some(NumberHint::Zero),
            1 => Some(NumberHint::One),
            2 => Some(NumberHint::Two),
            3 => Some(NumberHint::Three),
            4 => Some(NumberHint::Four),
            5 => Some(NumberHint::Five),
            6 => Some(NumberHint::Six),
            7 => Some(NumberHint::Seven),
            8 => Some(NumberHint::Eight),
            9 => Some(NumberHint::Nine),
            10 => Some(NumberHint::Ten),
            _ => None,
        }
    }

    pub fn header_only_f64(float: f64) -> Option<Self> {
        match float {
            0.0 => Some(NumberHint::Zero),
            1.0 => Some(NumberHint::One),
            2.0 => Some(NumberHint::Two),
            3.0 => Some(NumberHint::Three),
            4.0 => Some(NumberHint::Four),
            5.0 => Some(NumberHint::Five),
            6.0 => Some(NumberHint::Six),
            7.0 => Some(NumberHint::Seven),
            8.0 => Some(NumberHint::Eight),
            9.0 => Some(NumberHint::Nine),
            10.0 => Some(NumberHint::Ten),
            _ => None,
        }
    }

    /// Get the length of the data that follows the header
    pub fn data_length(self) -> usize {
        match self {
            Self::Size8 => 1,
            Self::Size32 => 4,
            Self::Size64 => 8,
            _ => 0,
        }
    }
}

/// String, object, and array lengths
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum Length {
    Empty = 0,
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    // larger numbers
    U8 = 11,
    U16 = 12,
    U32 = 13,
}
impl_from_u8!(Length, 13);

impl From<u64> for Length {
    fn from(len: u64) -> Self {
        match len {
            0 => Self::Empty,
            1 => Self::One,
            2 => Self::Two,
            3 => Self::Three,
            4 => Self::Four,
            5 => Self::Five,
            6 => Self::Six,
            7 => Self::Seven,
            8 => Self::Eight,
            9 => Self::Nine,
            10 => Self::Ten,
            len if len <= u64::from(u8::MAX) => Self::U8,
            len if len <= u64::from(u16::MAX) => Self::U16,
            _ => Self::U32,
        }
    }
}

impl Length {
    pub fn decode(self, d: &mut Decoder) -> DecodeResult<usize> {
        match self {
            Self::Empty => Ok(0),
            Self::One => Ok(1),
            Self::Two => Ok(2),
            Self::Three => Ok(3),
            Self::Four => Ok(4),
            Self::Five => Ok(5),
            Self::Six => Ok(6),
            Self::Seven => Ok(7),
            Self::Eight => Ok(8),
            Self::Nine => Ok(9),
            Self::Ten => Ok(10),
            Self::U8 => d.take_u8().map(|s| s as usize),
            Self::U16 => d.take_u16().map(|s| s as usize),
            Self::U32 => d.take_u32().map(|s| s as usize),
        }
    }

    pub fn check_empty(self) -> ToJsonResult<()> {
        if matches!(self, Self::Empty) {
            Ok(())
        } else {
            Err("Expected empty length, got non-empty".into())
        }
    }
}

/// Split a byte into two 4-bit halves - u8 numbers with a range of 0-15
fn split_byte(byte: u8) -> (u8, u8) {
    let left = byte >> 4; // Shift the byte right by 4 bits
    let right = byte & 0b0000_1111; // Mask the byte with 00001111
    (left, right)
}
