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
        let mut chars = Vec::new();
        let start = index;
        while let Some(next) = data.get(index) {
            match next {
                b'"' => {
                    index += 1;
                    let s = unsafe { String::from_utf8_unchecked(chars) };
                    return Ok((s, index));
                }
                b'\\' => {
                    index += 1;
                    if let Some(next_inner) = data.get(index) {
                        match next_inner {
                            b'"' | b'\\' | b'/' => chars.push(*next_inner),
                            b'b' => chars.push(b'\x08'),
                            b'f' => chars.push(b'\x0C'),
                            b'n' => chars.push(b'\n'),
                            b'r' => chars.push(b'\r'),
                            b't' => chars.push(b'\t'),
                            b'u' => {
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
                                        _ => return Err(JsonError::InvalidStringEscapeSequence(index)),
                                    };
                                    n = (n << 4) + hex;
                                }
                                match char::from_u32(n as u32) {
                                    Some(c) => {
                                        for b in c.to_string().bytes() {
                                            chars.push(b);
                                        }
                                    }
                                    None => return Err(JsonError::InvalidString(index - start)),
                                }
                            }
                            _ => return Err(JsonError::InvalidString(index - start)),
                        }
                    } else {
                        return Err(JsonError::UnexpectedEnd);
                    }
                }
                // all values below 32 are invalid
                next if *next < 32u8 => return Err(JsonError::InvalidString(index - start)),
                _ => chars.push(*next),
            }
            index += 1;
        }
        Err(JsonError::UnexpectedEnd)
    }
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
