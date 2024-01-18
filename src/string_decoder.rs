use std::ops::Range;

use crate::errors::{json_err, json_error, JsonResult};

pub type Tape = Vec<u8>;

/// `'t` is the lifetime of the tape (reusable buffer), `'j` is the lifetime of the JSON data itself
/// data must outlive tape, so if you return data with the lifetime of tape,
/// a slice of data the original JSON data is okay too
pub trait AbstractStringDecoder<'t, 'j>
where
    'j: 't,
{
    type Output;

    fn decode(data: &'j [u8], index: usize, tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)>;
}

pub struct StringDecoder;

#[derive(Debug)]
pub enum StringOutput<'t, 'j>
where
    'j: 't,
{
    Tape(&'t str),
    Data(&'j str),
}

impl From<StringOutput<'_, '_>> for String {
    fn from(val: StringOutput) -> Self {
        match val {
            StringOutput::Tape(s) => s.to_owned(),
            StringOutput::Data(s) => s.to_owned(),
        }
    }
}

impl<'t, 'j> StringOutput<'t, 'j> {
    pub fn as_str(&self) -> &'t str {
        match self {
            Self::Tape(s) => s,
            Self::Data(s) => s,
        }
    }
}

impl<'t, 'j> AbstractStringDecoder<'t, 'j> for StringDecoder
where
    'j: 't,
{
    type Output = StringOutput<'t, 'j>;

    fn decode(data: &'j [u8], index: usize, tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)> {
        #[cfg(target_arch = "x86_64")]
        if is_x86_feature_detected!("avx2") {
            return unsafe { decode_simd(data, index, tape) };
        }

        #[cfg(target_arch = "aarch64")]
        {
            return decode_simd(data, index, tape);
        }

        #[cfg(not(target_arch = "aarch64"))]
        {
            return decode_onebyone(data, index, tape);
        }
    }
}

#[cfg(not(target_arch = "aarch64"))]
fn decode_onebyone<'j, 't>(
    data: &'j [u8],
    mut index: usize,
    tape: &'t mut Tape,
) -> JsonResult<(StringOutput<'t, 'j>, usize)>
where
    'j: 't,
{
    index += 1;

    let start = index;
    let mut last_escape = start;
    let mut found_escape = false;
    let mut ascii_only = true;

    while let Some(next) = data.get(index) {
        match next {
            b'"' => {
                return if found_escape {
                    tape.extend_from_slice(&data[last_escape..index]);
                    index += 1;
                    let s = to_str(tape, ascii_only, start)?;
                    Ok((StringOutput::Tape(s), index))
                } else {
                    let s = to_str(&data[start..index], ascii_only, start)?;
                    index += 1;
                    Ok((StringOutput::Data(s), index))
                };
            }
            b'\\' => {
                if !found_escape {
                    tape.clear();
                    found_escape = true;
                }
                tape.extend_from_slice(&data[last_escape..index]);
                index += 1;
                if let Some(next_inner) = data.get(index) {
                    match next_inner {
                        b'"' | b'\\' | b'/' => tape.push(*next_inner),
                        b'b' => tape.push(b'\x08'),
                        b'f' => tape.push(b'\x0C'),
                        b'n' => tape.push(b'\n'),
                        b'r' => tape.push(b'\r'),
                        b't' => tape.push(b'\t'),
                        b'u' => {
                            let (c, new_index) = parse_escape(data, index)?;
                            index = new_index;
                            tape.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
                        }
                        _ => return json_err!(InvalidEscape, index),
                    }
                    last_escape = index + 1;
                } else {
                    break;
                }
            }
            // all values below 32 are invalid
            next if *next < 32u8 => return json_err!(ControlCharacterWhileParsingString, index),
            next if *next >= 128u8 && ascii_only => {
                ascii_only = false;
            }
            _ => (),
        }
        index += 1;
    }
    json_err!(EofWhileParsingString, index)
}

#[cfg(target_arch = "aarch64")]
fn decode_simd<'j, 't>(
    data: &'j [u8],
    mut index: usize,
    tape: &'t mut Tape,
) -> JsonResult<(StringOutput<'t, 'j>, usize)>
where
    'j: 't,
{
    index += 1;

    let start = index;
    let mut last_escape = start;
    let mut found_escape = false;
    let mut ascii_only = true;

    'simd: {
        use std::arch::aarch64::{
            vceqq_u8 as simd_eq, vdupq_n_u8 as simd_duplicate, vld1q_u8 as simd_load, vorrq_u8 as simd_or, *,
        };

        const SIMD_STEP: usize = 16;

        fn is_vector_nonzero(vec: uint8x16_t) -> bool {
            unsafe { vmaxvq_u8(vec) != 0 }
        }

        unsafe fn simd_is_ascii_non_control(vec: uint8x16_t) -> uint8x16_t {
            simd_or(vcltq_u8(vec, vdupq_n_u8(32)), vcgeq_u8(vec, vdupq_n_u8(128)))
        }

        let simd_quote = unsafe { simd_duplicate(b'"') };
        let simd_backslash = unsafe { simd_duplicate(b'\\') };

        for remaining_chunk in data
            .get(index..)
            .into_iter()
            .flat_map(|remaining| remaining.chunks_exact(SIMD_STEP))
        {
            let remaining_chunk_v = unsafe { simd_load(remaining_chunk.as_ptr()) };

            let backslash = unsafe { simd_eq(remaining_chunk_v, simd_backslash) };
            let mask = unsafe { simd_is_ascii_non_control(remaining_chunk_v) };
            let backslash_or_mask = unsafe { simd_or(backslash, mask) };

            // go slow if backslash or mask found
            if is_vector_nonzero(backslash_or_mask) {
                break 'simd;
            }

            // Compare the remaining chunk with the special characters
            let compare_result = unsafe { simd_eq(remaining_chunk_v, simd_quote) };

            // Check if any element in the comparison result is true
            if is_vector_nonzero(compare_result) {
                // Found a match, return the index
                let j = unsafe { remaining_chunk.iter().position(|&x| x == b'"').unwrap_unchecked() };
                return Ok((
                    StringOutput::Data(unsafe { std::str::from_utf8_unchecked(&data[start..index + j]) }),
                    index + j + 1,
                ));
            }

            index += remaining_chunk.len();
        }
    }

    while let Some(next) = data.get(index) {
        match next {
            b'"' => {
                return if found_escape {
                    tape.extend_from_slice(&data[last_escape..index]);
                    index += 1;
                    let s = to_str(tape, ascii_only, start)?;
                    Ok((StringOutput::Tape(s), index))
                } else {
                    let s = to_str(&data[start..index], ascii_only, start)?;
                    index += 1;
                    Ok((StringOutput::Data(s), index))
                };
            }
            b'\\' => {
                if !found_escape {
                    tape.clear();
                    found_escape = true;
                }
                tape.extend_from_slice(&data[last_escape..index]);
                index += 1;
                if let Some(next_inner) = data.get(index) {
                    match next_inner {
                        b'"' | b'\\' | b'/' => tape.push(*next_inner),
                        b'b' => tape.push(b'\x08'),
                        b'f' => tape.push(b'\x0C'),
                        b'n' => tape.push(b'\n'),
                        b'r' => tape.push(b'\r'),
                        b't' => tape.push(b'\t'),
                        b'u' => {
                            let (c, new_index) = parse_escape(data, index)?;
                            index = new_index;
                            tape.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
                        }
                        _ => return json_err!(InvalidEscape, index),
                    }
                    last_escape = index + 1;
                } else {
                    break;
                }
            }
            // all values below 32 are invalid
            next if *next < 32u8 => return json_err!(ControlCharacterWhileParsingString, index),
            next if *next >= 128u8 && ascii_only => {
                ascii_only = false;
            }
            _ => (),
        }
        index += 1;
    }
    json_err!(EofWhileParsingString, index)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn decode_simd<'j, 't>(
    data: &'j [u8],
    mut index: usize,
    tape: &'t mut Tape,
) -> JsonResult<(StringOutput<'t, 'j>, usize)>
where
    'j: 't,
{
    pub use std::arch::x86_64::{
        _mm256_cmpeq_epi8 as simd_eq, _mm256_cmpgt_epi8 as simd_gt, _mm256_loadu_si256 as simd_load,
        _mm256_movemask_epi8 as simd_movemask, _mm256_or_si256 as simd_or, _mm256_set1_epi8 as simd_duplicate, *,
    };

    pub const SIMD_STEP: usize = 32;

    index += 1;

    let start = index;
    let mut last_escape = start;
    let mut found_escape = false;
    let mut ascii_only = true;

    let simd_quote = unsafe { simd_duplicate(b'"' as i8) };
    let simd_backslash = unsafe { simd_duplicate(b'\\' as i8) };
    let simd_minus1 = unsafe { simd_duplicate(-1) };
    let simd_31 = unsafe { simd_duplicate(31) };

    while index < data.len() {
        // Safety: on the last chunk this will read slightly past the end of the buffer, but we
        // don's care because index += offset will never advance past the end of the buffer.
        let remaining_chunk_v = unsafe { simd_load(data.as_ptr().add(index).cast()) };

        let chunk_size = std::cmp::min(SIMD_STEP, data.len() - index);
        let mut offset = chunk_size;

        let backslash_or_quote = unsafe {
            simd_or(
                simd_eq(remaining_chunk_v, simd_backslash),
                simd_eq(remaining_chunk_v, simd_quote),
            )
        };
        let backslash_or_quote_mask = unsafe { simd_movemask(backslash_or_quote) };

        if backslash_or_quote_mask != 0 {
            let backslash_or_quote_offset = backslash_or_quote_mask.trailing_zeros() as usize;
            if backslash_or_quote_offset < offset {
                offset = backslash_or_quote_offset;
            }
        }

        // signed comparison means that single check is of >31 hits the range
        // we desire; signed >31 is equivalent to unsigned >31,<128
        let is_gt_31_or_lt_128 = unsafe { simd_gt(remaining_chunk_v, simd_31) };
        let in_range_char_mask = unsafe { simd_movemask(is_gt_31_or_lt_128) };

        // Compare the remaining chunk with the special characters

        if in_range_char_mask != -1 {
            let ge_0_char_mask = unsafe { simd_movemask(simd_gt(remaining_chunk_v, simd_minus1)) };
            let control_char_mask = !in_range_char_mask & ge_0_char_mask;
            if control_char_mask != 0 {
                let control_char_offset = control_char_mask.trailing_zeros() as usize;
                if control_char_offset < offset {
                    offset = control_char_offset;
                }
            }
            if ge_0_char_mask != -1 {
                ascii_only = false;
            }
        }

        index += offset;

        if offset == chunk_size {
            continue;
        }

        match unsafe { *data.as_ptr().add(index) } {
            b'"' => {
                return if found_escape {
                    tape.extend_from_slice(&data[last_escape..index]);
                    index += 1;
                    let s = to_str(tape, ascii_only, start)?;
                    Ok((StringOutput::Tape(s), index))
                } else {
                    let s = to_str(&data[start..index], ascii_only, start)?;
                    index += 1;
                    Ok((StringOutput::Data(s), index))
                }
            }
            b'\\' => {
                if !found_escape {
                    tape.clear();
                    found_escape = true;
                }
                tape.extend_from_slice(&data[last_escape..index]);
                index += 1;
                if let Some(next_inner) = data.get(index) {
                    match next_inner {
                        b'"' | b'\\' | b'/' => tape.push(*next_inner),
                        b'b' => tape.push(b'\x08'),
                        b'f' => tape.push(b'\x0C'),
                        b'n' => tape.push(b'\n'),
                        b'r' => tape.push(b'\r'),
                        b't' => tape.push(b'\t'),
                        b'u' => {
                            let (c, new_index) = parse_escape(data, index)?;
                            index = new_index;
                            tape.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
                        }
                        _ => return json_err!(InvalidEscape, index),
                    }
                    last_escape = index + 1;
                    index += 1;
                } else {
                    break;
                }
            }
            other => {
                assert!(other < 32);
                return json_err!(ControlCharacterWhileParsingString, index);
            }
        }
    }
    json_err!(EofWhileParsingString, index)
}

fn to_str(bytes: &[u8], ascii_only: bool, start: usize) -> JsonResult<&str> {
    if ascii_only {
        // safety: in this case we've already confirmed that all characters are ascii, we can safely
        // transmute from bytes to str
        Ok(unsafe { std::str::from_utf8_unchecked(bytes) })
    } else {
        std::str::from_utf8(bytes).map_err(|e| json_error!(InvalidUnicodeCodePoint, start + e.valid_up_to() + 1))
    }
}

/// Taken approximately from https://github.com/serde-rs/json/blob/v1.0.107/src/read.rs#L872-L945
fn parse_escape(data: &[u8], index: usize) -> JsonResult<(char, usize)> {
    let (n, index) = parse_u4(data, index)?;
    match n {
        0xDC00..=0xDFFF => json_err!(LoneLeadingSurrogateInHexEscape, index),
        0xD800..=0xDBFF => match data.get(index + 1..index + 3) {
            Some(slice) if slice == b"\\u" => {
                let (n2, index) = parse_u4(data, index + 2)?;
                if !(0xDC00..=0xDFFF).contains(&n2) {
                    return json_err!(LoneLeadingSurrogateInHexEscape, index);
                }
                let n2 = (((n - 0xD800) as u32) << 10 | (n2 - 0xDC00) as u32) + 0x1_0000;

                match char::from_u32(n2) {
                    Some(c) => Ok((c, index)),
                    None => json_err!(EofWhileParsingString, index),
                }
            }
            Some(slice) if slice.starts_with(b"\\") => json_err!(UnexpectedEndOfHexEscape, index + 2),
            Some(_) => json_err!(UnexpectedEndOfHexEscape, index + 1),
            None => match data.get(index + 1) {
                Some(b'\\') | None => json_err!(EofWhileParsingString, data.len()),
                Some(_) => json_err!(UnexpectedEndOfHexEscape, index + 1),
            },
        },
        _ => match char::from_u32(n as u32) {
            Some(c) => Ok((c, index)),
            None => json_err!(InvalidEscape, index),
        },
    }
}

fn parse_u4(data: &[u8], mut index: usize) -> JsonResult<(u16, usize)> {
    let mut n = 0;
    let u4 = data
        .get(index + 1..index + 5)
        .ok_or_else(|| json_error!(EofWhileParsingString, data.len()))?;

    for c in u4.iter() {
        index += 1;
        let hex = match c {
            b'0'..=b'9' => (c & 0x0f) as u16,
            b'a'..=b'f' => (c - b'a' + 10) as u16,
            b'A'..=b'F' => (c - b'A' + 10) as u16,
            _ => return json_err!(InvalidEscape, index),
        };
        n = (n << 4) + hex;
    }
    Ok((n, index))
}

pub struct StringDecoderRange;

impl<'t, 'j> AbstractStringDecoder<'t, 'j> for StringDecoderRange
where
    'j: 't,
{
    type Output = Range<usize>;

    fn decode(data: &'j [u8], mut index: usize, _tape: &'t mut Tape) -> JsonResult<(Self::Output, usize)> {
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
                    index += 1;
                    if let Some(next_inner) = data.get(index) {
                        match next_inner {
                            // these escapes are easy to validate
                            b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => (),
                            // unicode escapes are harder to validate, we just prevent them here
                            b'u' => return json_err!(StringEscapeNotSupported, index),
                            _ => return json_err!(InvalidEscape, index),
                        }
                    } else {
                        return json_err!(EofWhileParsingString, index);
                    }
                    index += 1;
                }
                _ => {
                    index += 1;
                }
            }
        }
        json_err!(EofWhileParsingString, index)
    }
}
