#![doc = include_str ! ("../README.md")]

use strum::{Display, EnumMessage};

mod chunk;
pub mod parse;
mod value;

pub use chunk::{Chunk, ChunkInfo, Chunker, Exponent};
pub use value::{JsonArray, JsonObject, JsonValue};

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

type Location = (usize, usize);

#[derive(Debug)]
pub struct ErrorInfo {
    pub error_type: JsonError,
    pub loc: Location,
}

impl ErrorInfo {
    pub fn new(error_type: JsonError, loc: Location) -> Self {
        Self { error_type, loc }
    }

    pub(crate) fn next<T>(error_type: JsonError, loc: Location) -> Option<JsonResult<T>> {
        Some(Err(Self::new(error_type, loc)))
    }
}

pub type JsonResult<T> = Result<T, ErrorInfo>;
