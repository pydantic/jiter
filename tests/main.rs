use donervan::{Chunk, ChunkInfo, Chunker, DonervanResult, Exponent};

#[test]
fn chunk_string() {
    let chunks: Vec<ChunkInfo> = Chunker::new(r#""foobar""#.as_bytes())
        .collect::<DonervanResult<_>>()
        .unwrap();
    assert_eq!(chunks.len(), 1);
    let first_chunk = chunks[0].clone();
    assert_eq!(
        first_chunk,
        ChunkInfo {
            key: None,
            chunk_type: Chunk::String(1..7),
            line: 0,
            col: 0
        }
    );
}

#[test]
fn chunk_int() {
    let chunks: Vec<ChunkInfo> = Chunker::new("-1234".as_bytes()).collect::<DonervanResult<_>>().unwrap();
    assert_eq!(chunks.len(), 1);
    let first_chunk = chunks[0].clone();
    assert_eq!(
        first_chunk,
        ChunkInfo {
            key: None,
            chunk_type: Chunk::Int {
                positive: false,
                range: 1..5
            },
            line: 0,
            col: 0
        }
    );
}
#[test]
fn chunk_int_exp() {
    let chunker = Chunker::new("20e10".as_bytes());
    let chunks: Vec<ChunkInfo> = chunker.collect::<DonervanResult<_>>().unwrap();
    assert_eq!(chunks.len(), 1);
    let first_chunk = chunks[0].clone();
    assert_eq!(
        first_chunk,
        ChunkInfo {
            key: None,
            chunk_type: Chunk::IntExponent {
                positive: true,
                range: 0..2,
                exponent: Exponent {
                    positive: true,
                    range: 3..5
                }
            },
            line: 0,
            col: 0
        }
    );
}

#[test]
fn chunk_float() {
    let chunks: Vec<ChunkInfo> = Chunker::new("12.34".as_bytes()).collect::<DonervanResult<_>>().unwrap();
    assert_eq!(chunks.len(), 1);
    let first_chunk = chunks[0].clone();
    assert_eq!(
        first_chunk,
        ChunkInfo {
            key: None,
            chunk_type: Chunk::Float {
                positive: true,
                range: (0, 3, 5)
            },
            line: 0,
            col: 0
        }
    );
}

#[test]
fn chunk_float_exp() {
    let chunker = Chunker::new("2.2e10".as_bytes());
    let chunks: Vec<ChunkInfo> = chunker.collect::<DonervanResult<_>>().unwrap();
    assert_eq!(chunks.len(), 1);
    let first_chunk = chunks[0].clone();
    assert_eq!(
        first_chunk,
        ChunkInfo {
            key: None,
            chunk_type: Chunk::FloatExponent {
                positive: true,
                range: (0, 2, 3),
                exponent: Exponent {
                    positive: true,
                    range: 4..6
                }
            },
            line: 0,
            col: 0
        }
    );
}

#[test]
fn chunk_null() {
    let json = "null";
    let chunks: Vec<ChunkInfo> = Chunker::new(json.as_bytes()).collect::<DonervanResult<_>>().unwrap();
    assert_eq!(chunks.len(), 1);
    let first_chunk = chunks[0].clone();
    assert_eq!(
        first_chunk,
        ChunkInfo {
            key: None,
            chunk_type: Chunk::Null,
            line: 0,
            col: 0
        }
    );
}

#[test]
fn chunk_array() {
    let json = "[true, false]";
    let chunks: Vec<ChunkInfo> = Chunker::new(json.as_bytes()).collect::<DonervanResult<_>>().unwrap();
    assert_eq!(
        chunks,
        vec![
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ArrayStart,
                line: 0,
                col: 0,
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::True,
                line: 0,
                col: 1,
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::False,
                line: 0,
                col: 5,
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ArrayEnd,
                line: 0,
                col: 12,
            },
        ]
    );
}

#[test]
fn chunk_object() {
    let json = r#"{"foobar": null}"#;
    let chunks: Vec<ChunkInfo> = Chunker::new(json.as_bytes()).collect::<DonervanResult<_>>().unwrap();
    assert_eq!(
        chunks,
        vec![
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ObjectStart,
                line: 0,
                col: 0,
            },
            ChunkInfo {
                key: Some(2..8,),
                chunk_type: Chunk::Null,
                line: 0,
                col: 1,
            },
            ChunkInfo {
                key: None,
                chunk_type: Chunk::ObjectEnd,
                line: 0,
                col: 15,
            },
        ]
    );
}
