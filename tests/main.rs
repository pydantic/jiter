use donervan::{Chunk, ChunkInfo, Chunker, JsonResult, JsonError};

macro_rules! single_expect_ok_or_error {
    ($name:ident, ok, $json:literal, $expected:expr) => {
        paste::item! {
            #[test]
            fn [< single_chunk_ok_ $name >]() {
                let chunks: Vec<ChunkInfo> = Chunker::new($json.as_bytes()).collect::<JsonResult<_>>().unwrap();
                assert_eq!(chunks.len(), 1);
                let first_chunk = chunks[0].clone();
                let debug = format!("{:?}", first_chunk);
                assert_eq!(debug, $expected);
            }
        }
    };
    ($name:ident, err, $json:literal, $error:expr) => {
       paste::item! {
           #[test]
           fn [< single_chunk_xerror_ $name _ $error:snake _error >]() {
               let result: JsonResult<Vec<ChunkInfo>> = Chunker::new($json.as_bytes()).collect();
               match result {
                   Ok(t) => panic!("unexpectedly valid: {:?} -> {:?}", $json, t),
                   Err(e) => assert_eq!(e.error_type, JsonError::$error),
               }
           }
       }
    };
}

/// macro to define many tests for expected values
macro_rules! single_tests {
    ($($name:ident: $ok_or_err:ident => $input:literal, $expected:expr;)*) => {
        $(
            single_expect_ok_or_error!($name, $ok_or_err, $input, $expected);
        )*
    }
}

single_tests! {
    string: ok => r#""foobar""#, "ChunkInfo { key: None, chunk_type: String(1..7), loc: (0, 0) }";
    int_neg: ok => "-1234", "ChunkInfo { key: None, chunk_type: Int { positive: false, range: 1..5, exponent: None }, loc: (0, 0) }";
    int_pos: ok => "1234", "ChunkInfo { key: None, chunk_type: Int { positive: true, range: 0..4, exponent: None }, loc: (0, 0) }";
    int_exp: ok => "20e10", "ChunkInfo { key: None, chunk_type: Int { positive: true, range: 0..2, exponent: Some(Exponent { positive: true, range: 3..5 }) }, loc: (0, 0) }";
    float: ok => "12.34", "ChunkInfo { key: None, chunk_type: Float { positive: true, int_range: 0..2, decimal_range: 3..5, exponent: None }, loc: (0, 0) }";
    float_exp: ok => "2.2e10", "ChunkInfo { key: None, chunk_type: Float { positive: true, int_range: 0..1, decimal_range: 2..3, exponent: Some(Exponent { positive: true, range: 4..6 }) }, loc: (0, 0) }";
    null: ok => "null", "ChunkInfo { key: None, chunk_type: Null, loc: (0, 0) }";
    v_true: ok => "true", "ChunkInfo { key: None, chunk_type: True, loc: (0, 0) }";
    v_false: ok => "false", "ChunkInfo { key: None, chunk_type: False, loc: (0, 0) }";
    offset_true: ok => "  true", "ChunkInfo { key: None, chunk_type: True, loc: (0, 2) }";
    string_unclosed: err => r#""foobar"#, UnexpectedEnd;
    bad_int: err => "-", InvalidNumber;
    bad_true: err => "truX", InvalidTrue;
    bad_true: err => "tru", UnexpectedEnd;
    bad_false: err => "falsX", InvalidFalse;
    bad_false: err => "fals", UnexpectedEnd;
    bad_null: err => "nulX", InvalidNull;
    bad_null: err => "nul", UnexpectedEnd;
}

#[test]
fn chunk_array() {
    let json = "[true, false]";
    let chunks: Vec<ChunkInfo> = Chunker::new(json.as_bytes()).collect::<JsonResult<_>>().unwrap();
    assert_eq!(
        chunks,
        vec![
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ArrayStart,
                loc: (0, 0),
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::True,
                loc: (0, 1),
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::False,
                loc: (0, 5),
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ArrayEnd,
                loc: (0, 12),
            },
        ]
    );
}

#[test]
fn chunk_object() {
    let json = r#"{"foobar": null}"#;
    let chunks: Vec<ChunkInfo> = Chunker::new(json.as_bytes()).collect::<JsonResult<_>>().unwrap();
    assert_eq!(
        chunks,
        vec![
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ObjectStart,
                loc: (0, 0),
            },
            ChunkInfo {
                key: Some(2..8,),
                chunk_type: Chunk::Null,
                loc: (0, 1),
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ObjectEnd,
                loc: (0, 15),
            },
        ]
    );
}
