use std::fmt;
use std::ops::Range;
use strum::{Display, EnumMessage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exponent {
    pub positive: bool,
    pub range: Range<usize>,
}

impl fmt::Display for Exponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.positive {
            write!(f, "e+")?;
        } else {
            write!(f, "e-")?;
        }
        write!(f, "{:?}", self.range)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    ObjectStart,
    ObjectEnd,
    ArrayStart,
    ArrayEnd,
    True,
    False,
    Null,
    Key(Range<usize>),
    String(Range<usize>),
    Int {
        positive: bool,
        range: Range<usize>,
        exponent: Option<Exponent>,
    },
    Float {
        positive: bool,
        int_range: Range<usize>,
        decimal_range: Range<usize>,
        exponent: Option<Exponent>,
    },
}

impl Default for Element {
    fn default() -> Self {
        Element::Null
    }
}

impl fmt::Display for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectStart => write!(f, "{{"),
            Self::ObjectEnd => write!(f, "}}"),
            Self::ArrayStart => write!(f, "["),
            Self::ArrayEnd => write!(f, "]"),
            Self::True => write!(f, "true"),
            Self::False => write!(f, "false"),
            Self::Null => write!(f, "null"),
            Self::Key(range) => write!(f, "Key({:?})", range),
            Self::String(range) => write!(f, "String({:?})", range),
            Self::Int {
                positive,
                range,
                exponent,
            } => {
                let prefix = if *positive { "+" } else { "-" };
                match exponent {
                    Some(exp) => write!(f, "{}Int({:?}{})", prefix, range, exp),
                    None => write!(f, "{}Int({:?})", prefix, range),
                }
            }
            Self::Float {
                positive,
                int_range,
                decimal_range,
                exponent,
            } => {
                let prefix = if *positive { "+" } else { "-" };
                match exponent {
                    Some(exp) => write!(f, "{}Float({:?}.{:?}{})", prefix, int_range, decimal_range, exp),
                    None => write!(f, "{}Float({:?}.{:?})", prefix, int_range, decimal_range),
                }
            }
        }
    }
}

#[derive(Debug, Display, EnumMessage, PartialEq, Eq, Clone)]
#[strum(serialize_all = "snake_case")]
pub enum JsonError {
    UnexpectedCharacter,
    UnexpectedEnd,
    ExpectingColon,
    ExpectingArrayNext,
    ExpectingObjectNext,
    ExpectingKey,
    ExpectingValue,
    InvalidTrue,
    InvalidFalse,
    InvalidNull,
    InvalidString(usize),
    InvalidStringEscapeSequence(usize),
    InvalidNumber,
    IntTooLarge,
    InternalError,
    End,
}

pub type JsonResult<T> = Result<T, JsonError>;
