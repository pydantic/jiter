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
#[derive(Debug)]
pub struct ErrorInfo {
    pub error_type: JsonError,
    pub loc: (usize, usize),
}

impl ErrorInfo {
    pub fn new(error_type: JsonError, loc: (usize, usize)) -> Self {
        Self { error_type, loc }
    }
}

pub type JsonResult<T> = Result<T, ErrorInfo>;
