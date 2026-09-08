use super::{classify_digit_chunk, fallback_int, swar_int};
use crate::number_decoder::IntChunk;

pub(crate) enum NumberChunk {
    Int(u64),
    Float,
    Ongoing,
}

/// Decode a number prefix starting with a nonzero digit. On `Ongoing`, resume scanning at
/// the returned index.
#[inline(always)]
pub(crate) fn decode_number_chunk(data: &[u8], mut index: usize) -> (NumberChunk, usize) {
    let Some(bytes) = data.get(index..index + 16) else {
        return decode_tail(data, index);
    };
    let (digits, count) = classify_digit_chunk(bytes.try_into().unwrap());
    if count < 16 {
        let end = index + count as usize;
        return match data.get(end) {
            Some(b'.' | b'e' | b'E') => (NumberChunk::Float, end),
            _ => (NumberChunk::Int(swar_int::decode_digits(digits, count)), end),
        };
    }

    index += 16;
    let mut tail = 0;
    let mut multiplier = 1;
    for _ in 0..3 {
        let Some(digit) = data.get(index).filter(|digit| digit.is_ascii_digit()) else {
            break;
        };
        tail = tail * 10 + u64::from(digit & 0x0f);
        multiplier *= 10;
        index += 1;
    }
    match data.get(index) {
        Some(digit) if digit.is_ascii_digit() => (NumberChunk::Ongoing, index),
        Some(b'.' | b'e' | b'E') => (NumberChunk::Float, index),
        _ => (
            NumberChunk::Int(swar_int::decode_digits(digits, 16) * multiplier + tail),
            index,
        ),
    }
}

#[cold]
#[inline(never)]
fn decode_tail(data: &[u8], index: usize) -> (NumberChunk, usize) {
    let (chunk, end) = fallback_int::decode_int_chunk(data, index, 0);
    let value = match chunk {
        IntChunk::Done(value) => NumberChunk::Int(value),
        IntChunk::Float => NumberChunk::Float,
        IntChunk::Ongoing(_) => unreachable!("fewer than sixteen bytes remain"),
    };
    (value, end)
}
