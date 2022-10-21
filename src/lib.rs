#![doc = include_str ! ("../README.md")]

use strum::{Display, EnumMessage};

mod chunk;

pub use chunk::{Chunk, ChunkInfo, Chunker, Exponent};

#[derive(Debug, Display, EnumMessage, PartialEq, Eq, Clone)]
#[strum(serialize_all = "snake_case")]
pub enum Error {
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
    InvalidNumber,
}

#[derive(Debug)]
pub struct ErrorInfo {
    error_type: Error,
    line: usize,
    col: usize,
}

pub type DonervanResult<T> = Result<T, ErrorInfo>;
