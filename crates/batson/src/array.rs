use std::mem::size_of;
use std::sync::Arc;

use jiter::{JsonArray, JsonValue};

use crate::decoder::Decoder;
use crate::encoder::Encoder;
use crate::errors::{DecodeResult, EncodeResult, ToJsonResult};
use crate::header::{Category, Header, Length, NumberHint, Primitive};
use crate::json_writer::JsonWriter;

#[cfg(target_endian = "big")]
compile_error!("big-endian architectures are not yet supported as we use `bytemuck` for zero-copy header decoding.");

/// Batson heterogeneous array representation
#[derive(Debug)]
pub(crate) struct HetArray<'b> {
    offsets: &'b [u32],
}

impl<'b> HetArray<'b> {
    pub fn decode_header(d: &mut Decoder<'b>, length: Length) -> DecodeResult<Self> {
        if matches!(length, Length::Empty) {
            Ok(Self { offsets: &[] })
        } else {
            let length = length.decode(d)?;
            let positions = d.take_slice_as(length)?;
            Ok(Self { offsets: positions })
        }
    }

    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn get(&self, d: &mut Decoder<'b>, index: usize) -> bool {
        if let Some(offset) = self.offsets.get(index) {
            d.index += *offset as usize;
            true
        } else {
            false
        }
    }

    pub fn to_json(&self, d: &mut Decoder<'b>) -> DecodeResult<JsonArray<'b>> {
        self.offsets
            .iter()
            .map(|_| d.take_value())
            .collect::<DecodeResult<_>>()
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
        .map(|b| Header::decode(*b, d).map(|h| h.as_value(d)))
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
                encoder.push(Category::HeaderArray.encode_with(array.len() as u8));
                // no alignment necessary, it's a vec of u8
                encoder.extend(&array);
            }
            PackedArray::I64(array) => {
                encoder.push(Category::I64Array.encode_with(array.len() as u8));
                encoder.align::<i64>();
                encoder.extend(bytemuck::cast_slice(&array));
            }
            PackedArray::U8(array) => {
                encoder.push(Category::U8Array.encode_with(array.len() as u8));
                // no alignment necessary, it's a vec of u8
                encoder.extend(&array);
            }
        }
        Ok(())
    } else {
        encoder.encode_length(Category::HetArray, array.len())?;

        let mut offsets: Vec<u32> = Vec::with_capacity(array.len());
        encoder.align::<u32>();
        let positions_start = encoder.ring_fence(array.len() * size_of::<u32>());

        let offset_start = encoder.position();
        for value in array.iter() {
            offsets.push((encoder.position() - offset_start) as u32);
            encoder.encode_value(value)?;
        }
        encoder.set_range(positions_start, bytemuck::cast_slice(&offsets));
        Ok(())
    }
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
                JsonValue::BigInt(b) => todo!("BigInt {b:?}"),
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
        let array = Arc::new(vec![JsonValue::Null, JsonValue::Int(123), JsonValue::Bool(false)]);
        let mut encoder = Encoder::new();
        encoder.encode_array(&array).unwrap();
        let bytes: Vec<u8> = encoder.into();

        let mut decoder = Decoder::new(&bytes);
        let header = decoder.take_header().unwrap();
        assert_eq!(header, Header::HetArray(3.into()));

        let het_array = HetArray::decode_header(&mut decoder, 3.into()).unwrap();
        assert_eq!(het_array.len(), 3);
        assert_eq!(het_array.offsets, &[0, 1, 3]);
        let decode_array = het_array.to_json(&mut decoder).unwrap();
        assert_arrays_eq!(decode_array, array);
    }

    #[test]
    fn array_round_trip_empty() {
        let array = Arc::new(vec![]);
        let mut encoder = Encoder::new();
        encoder.encode_array(&array).unwrap();
        let bytes: Vec<u8> = encoder.into();
        assert_eq!(bytes.len(), 1);

        let mut decoder = Decoder::new(&bytes);
        let header = decoder.take_header().unwrap();
        assert_eq!(header, Header::HetArray(0.into()));

        let het_array = HetArray::decode_header(&mut decoder, 0.into()).unwrap();
        assert_eq!(het_array.len(), 0);
        let decode_array = het_array.to_json(&mut decoder).unwrap();
        assert_arrays_eq!(decode_array, array);
    }

    #[test]
    fn header_array_round_trip() {
        let array = Arc::new(vec![
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
        let array = Arc::new(vec![JsonValue::Int(7), JsonValue::Int(4), JsonValue::Int(123)]);
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
        let array = Arc::new(vec![
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
}
