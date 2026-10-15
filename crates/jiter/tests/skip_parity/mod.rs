//! Comparing `next_skip` with `JsonValue::parse`, shared by the `main` and `json_cases` suites:
//! both must accept the same documents and, when they reject one, report the same error at the
//! same index.

use jiter::{Jiter, JiterErrorType, JsonErrorType, JsonValue};

/// Accepted, or rejected with an error kind at an index.
pub type Outcome = Result<(), (JsonErrorType, usize)>;

pub fn parse_outcome(json_data: &[u8], allow_inf_nan: bool) -> Outcome {
    JsonValue::parse(json_data, allow_inf_nan)
        .map(drop)
        .map_err(|e| (e.error_type, e.index))
}

/// `next_skip` followed by `finish`, so trailing input counts against the document as it does
/// for `JsonValue::parse`.
pub fn skip_outcome(json_data: &[u8], allow_inf_nan: bool) -> Outcome {
    let mut jiter = Jiter::new(json_data);
    if allow_inf_nan {
        jiter = jiter.with_allow_inf_nan();
    }
    jiter
        .next_skip()
        .and_then(|()| jiter.finish())
        .map_err(|e| match e.error_type {
            JiterErrorType::JsonError(error_type) => (error_type, e.index),
            JiterErrorType::WrongType { .. } => unreachable!("skipping cannot report a type mismatch"),
        })
}
