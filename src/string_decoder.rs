use crate::{JsonError, JsonResult};
use std::ops::Range;

pub trait AbstractStringDecoder {
    type Output;

    fn decode(data: &[u8], index: usize) -> JsonResult<(Self::Output, usize)>;
}

pub struct StringDecoder;

impl AbstractStringDecoder for StringDecoder {
    type Output = String;

    fn decode(data: &[u8], mut index: usize) -> JsonResult<(Self::Output, usize)> {
        index += 1;
        let mut bytes = Vec::new();
        let start = index;
        while let Some(next) = data.get(index) {
            match next {
                b'"' => {
                    index += 1;
                    let s = unsafe { String::from_utf8_unchecked(bytes) };
                    return Ok((s, index));
                }
                b'\\' => {
                    index += 1;
                    if let Some(next_inner) = data.get(index) {
                        match next_inner {
                            b'"' | b'\\' | b'/' => bytes.push(*next_inner),
                            b'b' => bytes.push(b'\x08'),
                            b'f' => bytes.push(b'\x0C'),
                            b'n' => bytes.push(b'\n'),
                            b'r' => bytes.push(b'\r'),
                            b't' => bytes.push(b'\t'),
                            b'u' => {
                                let (c, new_index) = parse_escape(data, index, start)?;
                                index = new_index;
                                bytes.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
                            }
                            _ => return Err(JsonError::InvalidString(index - start)),
                        }
                    } else {
                        return Err(JsonError::UnexpectedEnd);
                    }
                }
                // all values below 32 are invalid
                next if *next < 32u8 => return Err(JsonError::InvalidString(index - start)),
                _ => bytes.push(*next),
            }
            index += 1;
        }
        Err(JsonError::UnexpectedEnd)
    }
}

/// Taken from https://github.com/serde-rs/json/blob/45f10ec816e3f2765ac08f7ca73752326b0475d7/src/read.rs#L873-L928
fn parse_escape(data: &[u8], index: usize, start: usize) -> JsonResult<(char, usize)> {
    let (n, index) = parse_u4(data, index, start)?;
    match n {
        0xDC00..=0xDFFF => Err(JsonError::InvalidStringEscapeSequence(index - start)),
        0xD800..=0xDBFF => match (data.get(index + 1), data.get(index + 2)) {
            (Some(b'\\'), Some(b'u')) => {
                let (n2, index) = parse_u4(data, index + 2, start)?;
                if !(0xDC00..=0xDFFF).contains(&n2) {
                    return Err(JsonError::InvalidStringEscapeSequence(index - start));
                }
                let n2 = (((n - 0xD800) as u32) << 10 | (n2 - 0xDC00) as u32) + 0x1_0000;

                match char::from_u32(n2) {
                    Some(c) => Ok((c, index)),
                    None => Err(JsonError::InvalidString(index - start)),
                }
            }
            _ => Err(JsonError::InvalidStringEscapeSequence(index - start)),
        },
        _ => match char::from_u32(n as u32) {
            Some(c) => Ok((c, index)),
            None => Err(JsonError::InvalidString(index - start)),
        },
    }
}

fn parse_u4(data: &[u8], mut index: usize, start: usize) -> JsonResult<(u16, usize)> {
    let mut n = 0;
    for _ in 0..4 {
        index += 1;
        let c = match data.get(index) {
            Some(c) => *c,
            None => return Err(JsonError::InvalidString(index - start)),
        };
        let hex = match c {
            b'0'..=b'9' => (c & 0x0f) as u16,
            b'a'..=b'f' => (c - b'a' + 10) as u16,
            b'A'..=b'F' => (c - b'A' + 10) as u16,
            _ => return Err(JsonError::InvalidStringEscapeSequence(index - start)),
        };
        n = (n << 4) + hex;
    }
    Ok((n, index))
}

pub struct StringDecoderRange;

impl AbstractStringDecoder for StringDecoderRange {
    type Output = Range<usize>;

    fn decode(data: &[u8], mut index: usize) -> JsonResult<(Self::Output, usize)> {
        index += 1;
        let start = index;
        while let Some(next) = data.get(index) {
            match next {
                b'"' => {
                    let r = start..index;
                    index += 1;
                    return Ok((r, index));
                }
                b'\\' => {
                    index += 2;
                    // TODO check hex escape sequence
                }
                _ => {
                    index += 1;
                }
            }
        }
        Err(JsonError::UnexpectedEnd)
    }
}
