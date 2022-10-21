#![doc = include_str ! ("../README.md")]

use strum::{Display, EnumMessage};

mod chunk;
pub mod parse;
// mod value;

pub use chunk::{Chunk, ChunkInfo, Chunker, Exponent};

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
}

#[derive(Debug)]
pub struct ErrorInfo {
    pub error_type: JsonError,
    pub loc: (usize, usize),
}

pub type JsonResult<T> = Result<T, ErrorInfo>;
