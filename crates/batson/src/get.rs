#![allow(clippy::module_name_repetitions)]

use crate::array::{header_array_get, i64_array_get, u8_array_get, HetArray};
use crate::decoder::Decoder;
use crate::encoder::Encoder;
use crate::errors::{DecodeError, DecodeResult};
use crate::header::Header;
use crate::object::Object;
use std::borrow::Cow;

#[derive(Debug)]
pub enum BatsonPath<'s> {
    Key(&'s str),
    Index(usize),
}

impl From<usize> for BatsonPath<'_> {
    fn from(index: usize) -> Self {
        Self::Index(index)
    }
}

impl<'s> From<&'s str> for BatsonPath<'s> {
    fn from(key: &'s str) -> Self {
        Self::Key(key)
    }
}

pub fn get_bool(bytes: &[u8], path: &[BatsonPath]) -> DecodeResult<Option<bool>> {
    GetValue::get(bytes, path).map(|v| v.and_then(Into::into))
}

pub fn get_str<'b>(bytes: &'b [u8], path: &[BatsonPath]) -> DecodeResult<Option<&'b str>> {
    get_try_into(bytes, path)
}

pub fn get_int(bytes: &[u8], path: &[BatsonPath]) -> DecodeResult<Option<i64>> {
    get_try_into(bytes, path)
}

pub fn get_batson<'b>(bytes: &'b [u8], path: &[BatsonPath]) -> DecodeResult<Option<Cow<'b, [u8]>>> {
    if let Some(v) = GetValue::get(bytes, path)? {
        v.into_batson().map(Some)
    } else {
        Ok(None)
    }
}

pub fn contains(bytes: &[u8], path: &[BatsonPath]) -> DecodeResult<bool> {
    GetValue::get(bytes, path).map(|v| v.is_some())
}

pub fn get_length(bytes: &[u8], path: &[BatsonPath]) -> DecodeResult<Option<usize>> {
    if let Some(v) = GetValue::get(bytes, path)? {
        v.into_length()
    } else {
        Ok(None)
    }
}

fn get_try_into<'b, T>(bytes: &'b [u8], path: &[BatsonPath]) -> DecodeResult<Option<T>>
where
    Option<T>: TryFrom<GetValue<'b>, Error = DecodeError>,
{
    if let Some(v) = GetValue::get(bytes, path)? {
        v.try_into()
    } else {
        Ok(None)
    }
}

#[derive(Debug)]
enum GetValue<'b> {
    Header(Decoder<'b>, Header),
    U8(u8),
    I64(i64),
}

impl<'b> GetValue<'b> {
    fn get(bytes: &'b [u8], path: &[BatsonPath]) -> DecodeResult<Option<Self>> {
        let mut decoder = Decoder::new(bytes);
        let mut opt_header: Option<Header> = Some(decoder.take_header()?);
        let mut value: Option<GetValue> = None;
        for element in path {
            let Some(header) = opt_header.take() else {
                return Ok(None);
            };
            match element {
                BatsonPath::Key(key) => {
                    if let Header::Object(length) = header {
                        let object = Object::decode_header(&mut decoder, length)?;
                        if object.get(&mut decoder, key)? {
                            opt_header = Some(decoder.take_header()?);
                        }
                    }
                }
                BatsonPath::Index(index) => match header {
                    Header::HeaderArray(length) => {
                        opt_header = header_array_get(&mut decoder, length, *index)?;
                    }
                    Header::U8Array(length) => {
                        if let Some(u8_value) = u8_array_get(&mut decoder, length, *index)? {
                            value = Some(GetValue::U8(u8_value));
                        }
                    }
                    Header::I64Array(length) => {
                        if let Some(i64_value) = i64_array_get(&mut decoder, length, *index)? {
                            value = Some(GetValue::I64(i64_value));
                        }
                    }
                    Header::HetArray(length) => {
                        let a = HetArray::decode_header(&mut decoder, length)?;
                        if a.get(&mut decoder, *index) {
                            opt_header = Some(decoder.take_header()?);
                        }
                    }
                    _ => {}
                },
            }
        }
        if let Some(header) = opt_header {
            Ok(Some(Self::Header(decoder, header)))
        } else if let Some(value) = value {
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    fn header(self) -> Option<Header> {
        match self {
            Self::Header(_, header) => Some(header),
            _ => None,
        }
    }

    fn into_length(self) -> DecodeResult<Option<usize>> {
        let Self::Header(mut decoder, header) = self else {
            return Ok(None);
        };
        match header {
            Header::Str(length)
            | Header::Object(length)
            | Header::HeaderArray(length)
            | Header::U8Array(length)
            | Header::I64Array(length)
            | Header::HetArray(length) => length.decode(&mut decoder).map(Some),
            _ => Ok(None),
        }
    }

    fn into_batson(self) -> DecodeResult<Cow<'b, [u8]>> {
        match self {
            Self::Header(mut decoder, header) => {
                let start = decoder.index - 1;
                decoder.move_to_end(header)?;
                let end = decoder.index;
                decoder.get_range(start, end).map(Cow::Borrowed)
            }
            Self::U8(int) => {
                let mut encoder = Encoder::with_capacity(2);
                encoder.encode_i64(int.into());
                Ok(Cow::Owned(encoder.into()))
            }
            Self::I64(int) => {
                let mut encoder = Encoder::with_capacity(9);
                encoder.encode_i64(int);
                Ok(Cow::Owned(encoder.into()))
            }
        }
    }
}

impl From<GetValue<'_>> for Option<bool> {
    fn from(v: GetValue) -> Self {
        v.header().and_then(Header::into_bool)
    }
}

impl<'b> TryFrom<GetValue<'b>> for Option<&'b str> {
    type Error = DecodeError;

    fn try_from(v: GetValue<'b>) -> DecodeResult<Self> {
        match v {
            GetValue::Header(mut decoder, Header::Str(length)) => {
                let length = length.decode(&mut decoder)?;
                decoder.take_str(length).map(Some)
            }
            _ => Ok(None),
        }
    }
}

impl TryFrom<GetValue<'_>> for Option<i64> {
    type Error = DecodeError;

    fn try_from(v: GetValue) -> DecodeResult<Self> {
        match v {
            GetValue::Header(mut decoder, Header::Int(n)) => n.decode_i64(&mut decoder).map(Some),
            GetValue::I64(i64) => Ok(Some(i64)),
            GetValue::U8(u8) => Ok(Some(i64::from(u8))),
            GetValue::Header(..) => Ok(None),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::encode_from_json;
    use crate::header::{Header, NumberHint};
    use jiter::JsonValue;
    use std::sync::Arc;

    use super::*;

    #[test]
    fn get_object() {
        let v: JsonValue<'static> = JsonValue::Object(Arc::new(vec![
            ("null".into(), JsonValue::Null),
            ("true".into(), JsonValue::Bool(true)),
        ]));
        let bytes = encode_from_json(&v).unwrap();

        let v = GetValue::get(&bytes, &["null".into()]).unwrap().unwrap();
        assert_eq!(v.header(), Some(Header::Null));
        let v = GetValue::get(&bytes, &["true".into()]).unwrap().unwrap();
        assert_eq!(v.header(), Some(Header::Bool(true)));

        assert!(GetValue::get(&bytes, &["foo".into()]).unwrap().is_none());
        assert!(GetValue::get(&bytes, &[1.into()]).unwrap().is_none());
        assert!(GetValue::get(&bytes, &["null".into(), 1.into()]).unwrap().is_none());
    }

    #[test]
    fn get_header_array() {
        let v: JsonValue<'static> = JsonValue::Array(Arc::new(vec![JsonValue::Null, JsonValue::Bool(true)]));
        let bytes = encode_from_json(&v).unwrap();

        let v = GetValue::get(&bytes, &[0.into()]).unwrap().unwrap();
        assert_eq!(v.header(), Some(Header::Null));

        let v = GetValue::get(&bytes, &[1.into()]).unwrap().unwrap();
        assert_eq!(v.header(), Some(Header::Bool(true)));

        assert!(GetValue::get(&bytes, &["foo".into()]).unwrap().is_none());
        assert!(GetValue::get(&bytes, &[2.into()]).unwrap().is_none());
    }

    #[test]
    fn get_het_array() {
        let v: JsonValue<'static> =
            JsonValue::Array(Arc::new(vec![JsonValue::Int(42), JsonValue::Str("foobar".into())]));
        let bytes = encode_from_json(&v).unwrap();

        let v = GetValue::get(&bytes, &[0.into()]).unwrap().unwrap();
        assert_eq!(v.header(), Some(Header::Int(NumberHint::Size8)));
    }

    fn value_u8(v: &GetValue) -> Option<u8> {
        match v {
            GetValue::U8(u8) => Some(*u8),
            _ => None,
        }
    }

    fn value_i64(v: &GetValue) -> Option<i64> {
        match v {
            GetValue::I64(i64) => Some(*i64),
            _ => None,
        }
    }

    #[test]
    fn get_u8_array() {
        let v: JsonValue<'static> = JsonValue::Array(Arc::new(vec![JsonValue::Int(42), JsonValue::Int(255)]));
        let bytes = encode_from_json(&v).unwrap();

        let v = GetValue::get(&bytes, &[0.into()]).unwrap().unwrap();
        assert_eq!(value_u8(&v), Some(42));

        let v = GetValue::get(&bytes, &[1.into()]).unwrap().unwrap();
        assert_eq!(value_u8(&v), Some(255));

        assert!(GetValue::get(&bytes, &[2.into()]).unwrap().is_none());
    }

    #[test]
    fn get_i64_array() {
        let v: JsonValue<'static> = JsonValue::Array(Arc::new(vec![JsonValue::Int(42), JsonValue::Int(i64::MAX)]));
        let bytes = encode_from_json(&v).unwrap();

        let v = GetValue::get(&bytes, &[0.into()]).unwrap().unwrap();
        assert_eq!(value_i64(&v), Some(42));

        let v = GetValue::get(&bytes, &[1.into()]).unwrap().unwrap();
        assert_eq!(value_i64(&v), Some(i64::MAX));

        assert!(GetValue::get(&bytes, &[2.into()]).unwrap().is_none());
    }
}
