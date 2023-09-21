use std::fmt;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum JsonErrorType {
    UnexpectedCharacter,
    UnexpectedEnd,
    InvalidTrue,
    InvalidFalse,
    InvalidNull,
    // the usize here is the position within the string that is invalid
    InvalidString(usize),
    // same
    InvalidStringEscapeSequence(usize),
    // same
    StringEscapeNotSupported(usize),
    InvalidNumber,
    NumberTooLarge,
    FloatExpectingInt,
    RecursionLimitExceeded,
    // These are designed to match serde
    NonStringKey,
    ExpectedIdent,
    ExpectedValue,
    EofWhileParsingString,
    TrailingComma,
}

impl std::fmt::Display for JsonErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // the outputs here are chosen to match serde
        match self {
            JsonErrorType::UnexpectedCharacter => write!(f, "unexpected_character"),
            JsonErrorType::UnexpectedEnd => write!(f, "unexpected_end"),
            JsonErrorType::InvalidTrue => write!(f, "invalid_true"),
            JsonErrorType::InvalidFalse => write!(f, "invalid_false"),
            JsonErrorType::InvalidNull => write!(f, "invalid_null"),
            JsonErrorType::InvalidString(_) => write!(f, "invalid_string"),
            JsonErrorType::InvalidStringEscapeSequence(_) => {
                write!(f, "invalid_string_escape_sequence")
            }
            JsonErrorType::StringEscapeNotSupported(_) => {
                write!(f, "string_escape_not_supported")
            }
            JsonErrorType::InvalidNumber => write!(f, "invalid_number"),
            JsonErrorType::FloatExpectingInt => write!(f, "float_expecting_int"),
            JsonErrorType::RecursionLimitExceeded => write!(f, "recursion_limit_exceeded"),
            // These are designed to match serde
            JsonErrorType::NonStringKey => write!(f, "key must be a string"),
            JsonErrorType::ExpectedIdent => write!(f, "expected ident"),
            JsonErrorType::ExpectedValue => write!(f, "expected value"),
            JsonErrorType::EofWhileParsingString => write!(f, "EOF while parsing a string"),
            JsonErrorType::TrailingComma => write!(f, "trailing comma"),
        }
    }
}

pub type JsonResult<T> = Result<T, JsonError>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct JsonError {
    pub error_type: JsonErrorType,
    pub index: usize,
}

impl JsonError {
    pub fn new(error_type: JsonErrorType, index: usize) -> Self {
        Self { error_type, index }
    }
}

macro_rules! json_error {
    ($error_type:ident, $index:expr) => {
        crate::errors::JsonError::new(crate::errors::JsonErrorType::$error_type, $index)
    };

    ($error_type:ident, $error_value: expr, $index:expr) => {
        crate::errors::JsonError::new(crate::errors::JsonErrorType::$error_type($error_value), $index)
    };
}

pub(crate) use json_error;

macro_rules! json_err {
    ($error_type:ident, $index:expr) => {
        Err(crate::errors::json_error!($error_type, $index))
    };

    ($error_type:ident, $error_value: expr, $index:expr) => {
        Err(crate::errors::json_error!($error_type, $error_value, $index))
    };
}

pub(crate) use json_err;

pub(crate) const DEFAULT_RECURSION_LIMIT: u8 = 200;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum JsonType {
    Null,
    Bool,
    Int,
    Float,
    String,
    Array,
    Object,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum JiterErrorType {
    JsonError(JsonErrorType),
    WrongType { expected: JsonType, actual: JsonType },
    StringFormat,
    NumericValue,
    UnknownError,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct JiterError {
    pub error_type: JiterErrorType,
    pub index: usize,
}

impl JiterError {
    pub(crate) fn new(error_type: JiterErrorType, index: usize) -> Self {
        Self { error_type, index }
    }

    pub(crate) fn wrong_type(expected: JsonType, actual: JsonType, index: usize) -> Self {
        Self::new(JiterErrorType::WrongType { expected, actual }, index)
    }
}

impl From<JsonError> for JiterError {
    fn from(error: JsonError) -> Self {
        Self {
            error_type: JiterErrorType::JsonError(error.error_type),
            index: error.index,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JsonValueError {
    pub error_type: JsonErrorType,
    pub index: usize,
    pub position: FilePosition,
}

impl std::fmt::Display for JsonValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.error_type, self.position)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePosition {
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for FilePosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {} column {}", self.line, self.column)
    }
}

impl FilePosition {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    /// Find the line and column of a byte index in a string.
    pub fn find(data: &[u8], find: usize) -> Self {
        let mut line = 1;
        let mut last_line_start = 0;
        let mut index = 0;
        while let Some(next) = data.get(index) {
            if index == find {
                break;
            } else if *next == b'\n' {
                line += 1;
                last_line_start = index + 1;
            }
            index += 1;
        }
        Self {
            line,
            column: index - last_line_start + 1,
        }
    }
}
