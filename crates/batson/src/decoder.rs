use std::fmt;

use jiter::JsonValue;

use crate::array::{
    header_array_to_json, header_array_write_to_json, i64_array_slice, i64_array_to_json, u8_array_slice,
    u8_array_to_json, HetArray,
};
use crate::errors::{DecodeError, DecodeErrorType, DecodeResult, ToJsonResult};
use crate::header::{Header, Length};
use crate::json_writer::JsonWriter;
use crate::object::Object;

#[cfg(target_endian = "big")]
compile_error!("big-endian architectures are not yet supported as we use `bytemuck` for zero-copy header decoding.");
// see `decode_slice_as` for more information

#[derive(Clone)]
pub(crate) struct Decoder<'b> {
    bytes: &'b [u8],
    pub index: usize,
}

impl fmt::Debug for Decoder<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let upcoming = self.bytes.get(self.index..).unwrap_or_default();
        f.debug_struct("Decoder")
            .field("total_length", &self.bytes.len())
            .field("upcoming_length", &upcoming.len())
            .field("index", &self.index)
            .field("upcoming", &upcoming)
            .finish()
    }
}

impl<'b> Decoder<'b> {
    pub fn new(bytes: &'b [u8]) -> Self {
        Self { bytes, index: 0 }
    }

    pub fn take_header(&mut self) -> DecodeResult<Header> {
        let byte = self.next().ok_or_else(|| self.eof())?;
        Header::decode(byte, self)
    }

    pub fn align<T>(&mut self) {
        let align = align_of::<T>();
        // I've checked and this is equivalent to: `self.index = self.index + align - (self.index % align)`
        // is it actually faster?
        self.index = (self.index + align - 1) & !(align - 1);
    }

    pub fn take_value(&mut self) -> DecodeResult<JsonValue<'b>> {
        match self.take_header()? {
            Header::Null => Ok(JsonValue::Null),
            Header::Bool(b) => Ok(JsonValue::Bool(b)),
            Header::Int(n) => n.decode_i64(self).map(JsonValue::Int),
            Header::IntBig(i) => todo!("decoding for bigint {i:?}"),
            Header::Float(n) => n.decode_f64(self).map(JsonValue::Float),
            Header::Str(l) => self.decode_str(l).map(|s| JsonValue::Str(s.into())),
            Header::Object(length) => {
                let obj = Object::decode_header(self, length)?;
                obj.to_json(self).map(JsonValue::Object)
            }
            Header::HetArray(length) => {
                let het = HetArray::decode_header(self, length)?;
                het.to_json(self).map(JsonValue::Array)
            }
            Header::U8Array(length) => u8_array_to_json(self, length).map(JsonValue::Array),
            Header::HeaderArray(length) => header_array_to_json(self, length).map(JsonValue::Array),
            Header::I64Array(length) => i64_array_to_json(self, length).map(JsonValue::Array),
        }
    }

    pub fn write_json(&mut self, writer: &mut JsonWriter) -> ToJsonResult<()> {
        match self.take_header()? {
            Header::Null => writer.write_null(),
            Header::Bool(b) => writer.write_value(b)?,
            Header::Int(n) => {
                let i = n.decode_i64(self)?;
                writer.write_value(i)?;
            }
            Header::IntBig(i) => todo!("decoding for bigint {i:?}"),
            Header::Float(n) => {
                let f = n.decode_f64(self)?;
                writer.write_value(f)?;
            }
            Header::Str(l) => {
                let s = self.decode_str(l)?;
                writer.write_value(s)?;
            }
            Header::Object(length) => {
                let obj = Object::decode_header(self, length)?;
                obj.write_json(self, writer)?;
            }
            Header::HetArray(length) => {
                let het = HetArray::decode_header(self, length)?;
                het.write_json(self, writer)?;
            }
            Header::U8Array(length) => {
                let a = u8_array_slice(self, length)?;
                writer.write_seq(a.iter())?;
            }
            Header::HeaderArray(length) => header_array_write_to_json(self, length, writer)?,
            Header::I64Array(length) => {
                let a = i64_array_slice(self, length)?;
                writer.write_seq(a.iter())?;
            }
        };
        Ok(())
    }

    pub fn take_slice(&mut self, size: usize) -> DecodeResult<&'b [u8]> {
        let end = self.index + size;
        let s = self.bytes.get(self.index..end).ok_or_else(|| self.eof())?;
        self.index = end;
        Ok(s)
    }

    pub fn take_slice_as<T: bytemuck::Pod>(&mut self, length: usize) -> DecodeResult<&'b [T]> {
        self.align::<T>();
        let size = length * size_of::<T>();
        let end = self.index + size;
        let s = self.bytes.get(self.index..end).ok_or_else(|| self.eof())?;

        let t: &[T] = bytemuck::try_cast_slice(s).map_err(|e| self.error(DecodeErrorType::PodCastError(e)))?;

        self.index = end;
        Ok(t)
    }

    pub fn decode_str(&mut self, length: Length) -> DecodeResult<&'b str> {
        let len = length.decode(self)?;
        if len == 0 {
            Ok("")
        } else {
            self.take_str(len)
        }
    }

    pub fn decode_bytes(&mut self, length: Length) -> DecodeResult<&'b [u8]> {
        let len = length.decode(self)?;
        if len == 0 {
            Ok(b"")
        } else {
            self.take_slice(len)
        }
    }

    pub fn take_str(&mut self, length: usize) -> DecodeResult<&'b str> {
        let end = self.index + length;
        let slice = self.bytes.get(self.index..end).ok_or_else(|| self.eof())?;
        let s = simdutf8::basic::from_utf8(slice).map_err(|e| DecodeError::from_utf8_error(self.index, e))?;
        self.index = end;
        Ok(s)
    }

    pub fn take_u8(&mut self) -> DecodeResult<u8> {
        self.next().ok_or_else(|| self.eof())
    }

    pub fn take_u16(&mut self) -> DecodeResult<u16> {
        let slice = self.take_slice(2)?;
        Ok(u16::from_le_bytes(slice.try_into().unwrap()))
    }

    pub fn take_u32(&mut self) -> DecodeResult<u32> {
        let slice = self.take_slice(4)?;
        Ok(u32::from_le_bytes(slice.try_into().unwrap()))
    }

    pub fn take_i8(&mut self) -> DecodeResult<i8> {
        match self.next() {
            Some(byte) => Ok(byte as i8),
            None => Err(self.eof()),
        }
    }

    pub fn take_i32(&mut self) -> DecodeResult<i32> {
        let slice = self.take_slice(4)?;
        Ok(i32::from_le_bytes(slice.try_into().unwrap()))
    }

    pub fn take_i64(&mut self) -> DecodeResult<i64> {
        let slice = self.take_slice(8)?;
        Ok(i64::from_le_bytes(slice.try_into().unwrap()))
    }

    pub fn take_f32(&mut self) -> DecodeResult<f32> {
        let slice = self.take_slice(4)?;
        Ok(f32::from_le_bytes(slice.try_into().unwrap()))
    }

    pub fn take_f64(&mut self) -> DecodeResult<f64> {
        let slice = self.take_slice(8)?;
        Ok(f64::from_le_bytes(slice.try_into().unwrap()))
    }

    pub fn eof(&self) -> DecodeError {
        self.error(DecodeErrorType::EOF)
    }

    pub fn error(&self, error_type: DecodeErrorType) -> DecodeError {
        DecodeError::new(self.index, error_type)
    }
}

impl<'b> Iterator for Decoder<'b> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(byte) = self.bytes.get(self.index) {
            self.index += 1;
            Some(*byte)
        } else {
            None
        }
    }
}
