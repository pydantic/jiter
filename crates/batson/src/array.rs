use bytemuck::NoUninit;
use jiter::{JsonArray, JsonValue};
use smallvec::SmallVec;
use std::mem::size_of;
use std::sync::Arc;

use crate::decoder::Decoder;
use crate::encoder::Encoder;
use crate::errors::{DecodeResult, EncodeResult, ToJsonResult};
use crate::header::{Category, Header, Length, NumberHint, Primitive};
use crate::json_writer::JsonWriter;
use crate::object::minimum_value_size_estimate;
use crate::EncodeError;

#[cfg(target_endian = "big")]
compile_error!("big-endian architectures are not yet supported as we use `bytemuck` for zero-copy header decoding.");

/// Batson heterogeneous array representation
#[derive(Debug)]
pub(crate) struct HetArray<'b> {
    offsets: HetArrayOffsets<'b>,
}

impl<'b> HetArray<'b> {
    pub fn decode_header(d: &mut Decoder<'b>, length: Length) -> DecodeResult<Self> {
        let offsets = match length {
            Length::Empty => HetArrayOffsets::U8(&[]),
            Length::U32 => HetArrayOffsets::U32(take_slice_as(d, length)?),
            Length::U16 => HetArrayOffsets::U16(take_slice_as(d, length)?),
            _ => HetArrayOffsets::U8(take_slice_as(d, length)?),
        };
        Ok(Self { offsets })
    }

    pub fn len(&self) -> usize {
        match self.offsets {
            HetArrayOffsets::U8(v) => v.len(),
            HetArrayOffsets::U16(v) => v.len(),
            HetArrayOffsets::U32(v) => v.len(),
        }
    }

    pub fn get(&self, d: &mut Decoder<'b>, index: usize) -> bool {
        let opt_offset = match &self.offsets {
            HetArrayOffsets::U8(v) => v.get(index).map(|&o| o as usize),
            HetArrayOffsets::U16(v) => v.get(index).map(|&o| o as usize),
            HetArrayOffsets::U32(v) => v.get(index).map(|&o| o as usize),
        };
        if let Some(offset) = opt_offset {
            d.index += offset;
            true
        } else {
            false
        }
    }

    pub fn to_value(&self, d: &mut Decoder<'b>) -> DecodeResult<JsonArray<'b>> {
        (0..self.len())
            .map(|_| d.take_value())
            .collect::<DecodeResult<SmallVec<_, 8>>>()
            .map(Arc::new)
    }

    pub fn write_json(&self, d: &mut Decoder<'b>, writer: &mut JsonWriter) -> ToJsonResult<()> {
        let mut steps = 0..self.len();
        writer.start_array();
        if steps.next().is_some() {
            d.write_json(writer)?;
            for _ in steps {
                writer.comma();
                d.write_json(writer)?;
            }
        }
        writer.end_array();
        Ok(())
    }

    pub fn move_to_end(&self, d: &mut Decoder<'b>) -> DecodeResult<()> {
        d.index += match &self.offsets {
            HetArrayOffsets::U8(v) => v.last().copied().unwrap() as usize,
            HetArrayOffsets::U16(v) => v.last().copied().unwrap() as usize,
            HetArrayOffsets::U32(v) => v.last().copied().unwrap() as usize,
        };
        let header = d.take_header()?;
        d.move_to_end(header)
    }
}

fn take_slice_as<'b, T: bytemuck::Pod>(d: &mut Decoder<'b>, length: Length) -> DecodeResult<&'b [T]> {
    let length = length.decode(d)?;
    d.take_slice_as(length)
}

#[derive(Debug)]
enum HetArrayOffsets<'b> {
    U8(&'b [u8]),
    U16(&'b [u16]),
    U32(&'b [u32]),
}

pub(crate) fn header_array_get(d: &mut Decoder, length: Length, index: usize) -> DecodeResult<Option<Header>> {
    u8_array_get(d, length, index)?
        .map(|b| Header::decode(b, d))
        .transpose()
}

pub(crate) fn header_array_to_json<'b>(d: &mut Decoder<'b>, length: Length) -> DecodeResult<JsonArray<'b>> {
    let length = length.decode(d)?;
    d.take_slice(length)?
        .iter()
        .map(|b| Header::decode(*b, d).map(|h| h.header_as_value(d)))
        .collect::<DecodeResult<_>>()
        .map(Arc::new)
}

pub(crate) fn header_array_write_to_json(d: &mut Decoder, length: Length, writer: &mut JsonWriter) -> ToJsonResult<()> {
    let length = length.decode(d)?;
    let s = d.take_slice(length)?;
    let mut iter = s.iter();

    writer.start_array();
    if let Some(b) = iter.next() {
        let h = Header::decode(*b, d)?;
        h.write_json_header_only(writer)?;
        for b in iter {
            writer.comma();
            let h = Header::decode(*b, d)?;
            h.write_json_header_only(writer)?;
        }
    }
    writer.end_array();
    Ok(())
}

pub(crate) fn u8_array_get(d: &mut Decoder, length: Length, index: usize) -> DecodeResult<Option<u8>> {
    let length = length.decode(d)?;
    let v = d.take_slice(length)?;
    Ok(v.get(index).copied())
}

pub(crate) fn u8_array_to_json<'b>(d: &mut Decoder<'b>, length: Length) -> DecodeResult<JsonArray<'b>> {
    let v = u8_array_slice(d, length)?
        .iter()
        .map(|b| JsonValue::Int(i64::from(*b)))
        .collect();
    Ok(Arc::new(v))
}

pub(crate) fn u8_array_slice<'b>(d: &mut Decoder<'b>, length: Length) -> DecodeResult<&'b [u8]> {
    let length = length.decode(d)?;
    d.take_slice(length)
}

pub(crate) fn i64_array_get(d: &mut Decoder, length: Length, index: usize) -> DecodeResult<Option<i64>> {
    let length = length.decode(d)?;
    d.align::<i64>();
    let s: &[i64] = d.take_slice_as(length)?;
    Ok(s.get(index).copied())
}

pub(crate) fn i64_array_to_json<'b>(d: &mut Decoder<'b>, length: Length) -> DecodeResult<JsonArray<'b>> {
    let s = i64_array_slice(d, length)?;
    let v = s.iter().copied().map(JsonValue::Int).collect();
    Ok(Arc::new(v))
}

pub(crate) fn i64_array_slice<'b>(d: &mut Decoder<'b>, length: Length) -> DecodeResult<&'b [i64]> {
    let length = length.decode(d)?;
    d.take_slice_as(length)
}

pub(crate) fn encode_array(encoder: &mut Encoder, array: &JsonArray) -> EncodeResult<()> {
    if array.is_empty() {
        // shortcut but also no alignment!
        encoder.encode_length(Category::HetArray, 0)
    } else if let Some(packed_array) = PackedArray::new(array) {
        match packed_array {
            PackedArray::Header(array) => {
                encoder.encode_length(Category::HeaderArray, array.len())?;
                // no alignment necessary, it's a vec of u8
                encoder.extend(&array);
            }
            PackedArray::I64(array) => {
                encoder.encode_length(Category::I64Array, array.len())?;
                encoder.align::<i64>();
                encoder.extend(bytemuck::cast_slice(&array));
            }
            PackedArray::U8(array) => {
                encoder.encode_length(Category::U8Array, array.len())?;
                // no alignment necessary, it's a vec of u8
                encoder.extend(&array);
            }
        }
        Ok(())
    } else {
        let min_size = minimum_array_size_estimate(array);
        let encoder_position = encoder.position();

        if min_size <= u8::MAX as usize {
            encoder.encode_length(Category::HetArray, array.len())?;
            if encode_array_sized::<u8>(encoder, array)? {
                return Ok(());
            }
            encoder.reset_position(encoder_position);
        }

        if min_size <= u16::MAX as usize {
            encoder.encode_len_u16(Category::HetArray, u16::try_from(array.len()).unwrap());
            if encode_array_sized::<u16>(encoder, array)? {
                return Ok(());
            }
            encoder.reset_position(encoder_position);
        }

        encoder.encode_len_u32(Category::HetArray, array.len())?;
        if encode_array_sized::<u32>(encoder, array)? {
            Ok(())
        } else {
            Err(EncodeError::ArrayTooLarge)
        }
    }
}

fn encode_array_sized<T: TryFrom<usize> + NoUninit>(encoder: &mut Encoder, array: &JsonArray) -> EncodeResult<bool> {
    let mut offsets: Vec<T> = Vec::with_capacity(array.len());
    encoder.align::<T>();
    let positions_start = encoder.ring_fence(array.len() * size_of::<T>());

    let offset_start = encoder.position();
    for value in array.iter() {
        let Ok(offset) = T::try_from(encoder.position() - offset_start) else {
            return Ok(false);
        };
        offsets.push(offset);
        encoder.encode_value(value)?;
    }
    encoder.set_range(positions_start, bytemuck::cast_slice(&offsets));
    Ok(true)
}

/// Estimate the minimize amount of space needed to encode the object.
///
/// This is NOT recursive, instead it makes very optimistic guesses about how long arrays and objects might be.
fn minimum_array_size_estimate(array: &JsonArray) -> usize {
    array.iter().map(minimum_value_size_estimate).sum()
}

#[derive(Debug)]
enum PackedArray {
    Header(Vec<u8>),
    U8(Vec<u8>),
    I64(Vec<i64>),
}

impl PackedArray {
    fn new(array: &JsonArray) -> Option<Self> {
        let mut header_only: Option<Vec<u8>> = Some(Vec::with_capacity(array.len()));
        let mut u8_only: Option<Vec<u8>> = Some(Vec::with_capacity(array.len()));
        let mut i64_only: Option<Vec<i64>> = Some(Vec::with_capacity(array.len()));

        macro_rules! push_len {
            ($cat: expr, $is_empty: expr) => {{
                u8_only = None;
                i64_only = None;
                if $is_empty {
                    header_only.as_mut()?.push($cat.encode_with(Length::Empty as u8));
                } else {
                    header_only = None;
                }
            }};
        }

        for element in array.iter() {
            match element {
                JsonValue::Null => {
                    u8_only = None;
                    i64_only = None;
                    header_only
                        .as_mut()?
                        .push(Category::Primitive.encode_with(Primitive::Null as u8));
                }
                JsonValue::Bool(b) => {
                    u8_only = None;
                    i64_only = None;
                    let right: Primitive = (*b).into();
                    header_only.as_mut()?.push(Category::Primitive.encode_with(right as u8));
                }
                JsonValue::Int(i) => {
                    if let Some(i64_only) = &mut i64_only {
                        i64_only.push(*i);
                    }
                    // if u8_only is still alive, push to it if we can
                    if let Some(u8_only_) = &mut u8_only {
                        if let Ok(u8) = u8::try_from(*i) {
                            u8_only_.push(u8);
                        } else {
                            u8_only = None;
                        }
                    }
                    // if header_only is still alive, push to it if we can
                    if let Some(h) = &mut header_only {
                        if let Some(n) = NumberHint::header_only_i64(*i) {
                            h.push(Category::Int.encode_with(n as u8));
                        } else {
                            header_only = None;
                        }
                    }
                }
                JsonValue::BigInt(_) => return None,
                JsonValue::Float(f) => {
                    u8_only = None;
                    i64_only = None;
                    if let Some(n) = NumberHint::header_only_f64(*f) {
                        header_only.as_mut()?.push(Category::Float.encode_with(n as u8));
                    } else {
                        header_only = None;
                    }
                }
                JsonValue::Str(s) => push_len!(Category::Str, s.is_empty()),
                // TODO could use a header only array if it's empty
                JsonValue::Array(a) => push_len!(Category::HetArray, a.is_empty()),
                JsonValue::Object(o) => push_len!(Category::Object, o.is_empty()),
            }
            if header_only.is_none() && i64_only.is_none() {
                // stop early if neither work
                return None;
            }
        }
        // u8 array is preferable to header array as it's the pure binary representation
        if let Some(u8_array) = u8_only {
            Some(Self::U8(u8_array))
        } else if let Some(header_only) = header_only {
            Some(Self::Header(header_only))
        } else {
            i64_only.map(Self::I64)
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use smallvec::smallvec;

    use crate::compare_json_values;
    use crate::decoder::Decoder;
    use crate::encoder::Encoder;
    use crate::header::Header;

    use super::*;

    /// hack while waiting for <https://github.com/pydantic/jiter/pull/131>
    macro_rules! assert_arrays_eq {
        ($a: expr, $b: expr) => {{
            assert_eq!($a.len(), $b.len());
            for (a, b) in $a.iter().zip($b.iter()) {
                assert!(compare_json_values(a, b));
            }
        }};
    }

    #[test]
    fn array_round_trip() {
        let array = Arc::new(smallvec![JsonValue::Null, JsonValue::Int(123), JsonValue::Bool(false),]);
        let min_size = minimum_array_size_estimate(&array);
        assert_eq!(min_size, 4);

        let mut encoder = Encoder::new();
        encoder.encode_array(&array).unwrap();
        let bytes: Vec<u8> = encoder.into();

        let mut decoder = Decoder::new(&bytes);
        let header = decoder.take_header().unwrap();
        assert_eq!(header, Header::HetArray(3.into()));

        let het_array = HetArray::decode_header(&mut decoder, 3.into()).unwrap();
        assert_eq!(het_array.len(), 3);

        let offsets = match het_array.offsets {
            HetArrayOffsets::U8(v) => v,
            _ => panic!("expected u8 offsets"),
        };

        assert_eq!(offsets, &[0, 1, 3]);
        let decode_array = het_array.to_value(&mut decoder).unwrap();
        assert_arrays_eq!(decode_array, array);
    }

    #[test]
    fn array_round_trip_empty() {
        let array = Arc::new(smallvec![]);
        let mut encoder = Encoder::new();
        encoder.encode_array(&array).unwrap();
        let bytes: Vec<u8> = encoder.into();
        assert_eq!(bytes.len(), 1);

        let mut decoder = Decoder::new(&bytes);
        let header = decoder.take_header().unwrap();
        assert_eq!(header, Header::HetArray(0.into()));

        let het_array = HetArray::decode_header(&mut decoder, 0.into()).unwrap();
        assert_eq!(het_array.len(), 0);
        let decode_array = het_array.to_value(&mut decoder).unwrap();
        assert_arrays_eq!(decode_array, array);
    }

    #[test]
    fn header_array_round_trip() {
        let array = Arc::new(smallvec![
            JsonValue::Null,
            JsonValue::Bool(false),
            JsonValue::Bool(true),
            JsonValue::Int(7),
            JsonValue::Float(4.0),
        ]);
        let mut encoder = Encoder::new();
        encoder.encode_array(&array).unwrap();
        let bytes: Vec<u8> = encoder.into();
        assert_eq!(bytes.len(), 6);

        let mut decoder = Decoder::new(&bytes);
        let header = decoder.take_header().unwrap();
        assert_eq!(header, Header::HeaderArray(5.into()));

        let header_array = header_array_to_json(&mut decoder, 5.into()).unwrap();
        assert_arrays_eq!(header_array, array);
    }

    #[test]
    fn u8_array_round_trip() {
        let array = Arc::new(smallvec![JsonValue::Int(7), JsonValue::Int(4), JsonValue::Int(123),]);
        let mut encoder = Encoder::new();
        encoder.encode_array(&array).unwrap();
        let bytes: Vec<u8> = encoder.into();
        assert_eq!(bytes.len(), 4);

        let mut decoder = Decoder::new(&bytes);
        let header = decoder.take_header().unwrap();
        assert_eq!(header, Header::U8Array(3.into()));

        let mut decoder = Decoder::new(&bytes);
        let v = decoder.take_value().unwrap();
        assert!(compare_json_values(&v, &JsonValue::Array(array)));
    }

    #[test]
    fn i64_array_round_trip() {
        let array = Arc::new(smallvec![
            JsonValue::Int(7),
            JsonValue::Int(i64::MAX),
            JsonValue::Int(i64::MIN),
            JsonValue::Int(1234),
            JsonValue::Int(1_234_567_890),
        ]);
        let mut encoder = Encoder::new();
        encoder.encode_array(&array).unwrap();
        let bytes: Vec<u8> = encoder.into();
        assert_eq!(bytes.len(), 6 * 8);

        let mut decoder = Decoder::new(&bytes);
        let header = decoder.take_header().unwrap();
        assert_eq!(header, Header::I64Array(5.into()));

        let i64_array = i64_array_to_json(&mut decoder, 5.into()).unwrap();
        assert_arrays_eq!(i64_array, array);
    }

    #[test]
    fn test_u16_array() {
        let mut array = vec![JsonValue::Bool(true); 100];
        array.extend(vec![JsonValue::Int(i64::MAX); 100]);
        let array = Arc::new(array.into());

        let mut encoder = Encoder::new();
        encoder.encode_array(&array).unwrap();
        let bytes: Vec<u8> = encoder.into();

        let mut decoder = Decoder::new(&bytes);
        let header = decoder.take_header().unwrap();
        assert_eq!(header, Header::HetArray(Length::U16));

        let het_array = HetArray::decode_header(&mut decoder, Length::U16).unwrap();
        assert_eq!(het_array.len(), 200);

        let offsets = match het_array.offsets {
            HetArrayOffsets::U16(v) => v,
            _ => panic!("expected U16 offsets"),
        };
        assert_eq!(offsets.len(), 200);
        assert_eq!(offsets[0], 0);
        assert_eq!(offsets[1], 1);

        let mut d = decoder.clone();
        assert!(het_array.get(&mut d, 0));
        assert!(compare_json_values(&d.take_value().unwrap(), &JsonValue::Bool(true)));

        let mut d = decoder.clone();
        assert!(het_array.get(&mut d, 99));
        assert!(compare_json_values(&d.take_value().unwrap(), &JsonValue::Bool(true)));

        let mut d = decoder.clone();
        assert!(het_array.get(&mut d, 100));
        assert!(compare_json_values(&d.take_value().unwrap(), &JsonValue::Int(i64::MAX)));

        let mut d = decoder.clone();
        assert!(het_array.get(&mut d, 199));
        assert!(compare_json_values(&d.take_value().unwrap(), &JsonValue::Int(i64::MAX)));

        let mut d = decoder.clone();
        assert!(!het_array.get(&mut d, 200));

        let decode_array = het_array.to_value(&mut decoder).unwrap();
        assert_arrays_eq!(decode_array, array);
    }

    #[test]
    fn test_u32_array() {
        let long_string = "a".repeat(u16::MAX as usize);
        let array = Arc::new(smallvec![
            JsonValue::Str(long_string.clone().into()),
            JsonValue::Int(42),
        ]);

        let mut encoder = Encoder::new();
        encoder.encode_array(&array).unwrap();
        let bytes: Vec<u8> = encoder.into();

        let mut decoder = Decoder::new(&bytes);
        let header = decoder.take_header().unwrap();
        assert_eq!(header, Header::HetArray(Length::U32));

        let het_array = HetArray::decode_header(&mut decoder, Length::U32).unwrap();
        assert_eq!(het_array.len(), 2);

        let offsets = match het_array.offsets {
            HetArrayOffsets::U32(v) => v,
            _ => panic!("expected U32 offsets"),
        };
        assert_eq!(offsets, [0, 65538]);

        let mut d = decoder.clone();
        assert!(het_array.get(&mut d, 0));
        assert!(compare_json_values(
            &d.take_value().unwrap(),
            &JsonValue::Str(long_string.into())
        ));

        let mut d = decoder.clone();
        assert!(het_array.get(&mut d, 1));
        assert!(compare_json_values(&d.take_value().unwrap(), &JsonValue::Int(42)));
    }
}
