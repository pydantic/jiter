use std::fmt;

use bytemuck::PodCastError;
use serde::ser::Error;
use simdutf8::basic::Utf8Error;

pub type EncodeResult<T> = Result<T, EncodeError>;

#[derive(Debug, Copy, Clone)]
pub enum EncodeError {
    StrTooLong,
}

pub type DecodeResult<T> = Result<T, DecodeError>;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct DecodeError {
    pub index: usize,
    pub error_type: DecodeErrorType,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error at index {}: {}", self.index, self.error_type)
    }
}

impl From<DecodeError> for serde_json::Error {
    fn from(e: DecodeError) -> Self {
        serde_json::Error::custom(e.to_string())
    }
}

impl DecodeError {
    pub fn new(index: usize, error_type: DecodeErrorType) -> Self {
        Self { index, error_type }
    }

    pub fn from_utf8_error(index: usize, error: Utf8Error) -> Self {
        Self::new(index, DecodeErrorType::Utf8Error(error))
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum DecodeErrorType {
    EOF,
    ObjectBodyIndexInvalid,
    HeaderInvalid { value: u8, ty: &'static str },
    Utf8Error(Utf8Error),
    PodCastError(PodCastError),
}

impl fmt::Display for DecodeErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeErrorType::EOF => write!(f, "Unexpected end of file"),
            DecodeErrorType::ObjectBodyIndexInvalid => write!(f, "Object body index is invalid"),
            DecodeErrorType::HeaderInvalid { value, ty } => {
                write!(f, "Header value {value} is invalid for type {ty}")
            }
            DecodeErrorType::Utf8Error(e) => write!(f, "UTF-8 error: {e}"),
            DecodeErrorType::PodCastError(e) => write!(f, "Pod cast error: {e}"),
        }
    }
}

pub type ToJsonResult<T> = Result<T, ToJsonError>;

#[derive(Debug)]
pub enum ToJsonError {
    Str(&'static str),
    DecodeError(DecodeError),
    JsonError(serde_json::Error),
}

impl From<&'static str> for ToJsonError {
    fn from(e: &'static str) -> Self {
        Self::Str(e)
    }
}

impl From<DecodeError> for ToJsonError {
    fn from(e: DecodeError) -> Self {
        Self::DecodeError(e)
    }
}

impl From<serde_json::Error> for ToJsonError {
    fn from(e: serde_json::Error) -> Self {
        Self::JsonError(e)
    }
}
