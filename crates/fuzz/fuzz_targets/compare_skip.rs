#![no_main]

use jiter::{JsonValue, Jiter, JiterError, JsonError, JiterErrorType};

use libfuzzer_sys::fuzz_target;
fn errors_equal(value_error: &JsonError, jiter_error: &JiterError) {
    let jiter_error_type = match &jiter_error.error_type {
        JiterErrorType::JsonError(json_error_type) => json_error_type,
        JiterErrorType::WrongType { .. } => panic!("Expected JsonError, found WrongType"),
    };
    assert_eq!(&value_error.error_type, jiter_error_type);
    assert_eq!(value_error.index, jiter_error.index);
}

fuzz_target!(|json: String| {
    let json_data = json.as_bytes();
// fuzz_target!(|json_data: &[u8]| {
    match JsonValue::parse(json_data, false) {
        Ok(_) => {
            let mut jiter = Jiter::new(json_data, false);
            jiter.next_skip().unwrap();
            jiter.finish().unwrap();
        }
        Err(json_error) => {
            let mut jiter = Jiter::new(json_data, false);
            let jiter_error = match jiter.next_skip() {
                Ok(_) => jiter.finish().unwrap_err(),
                Err(e) => e
            };
            errors_equal(&json_error, &jiter_error);
        },
    };
});
