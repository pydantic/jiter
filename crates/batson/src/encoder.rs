use jiter::{JsonArray, JsonObject, JsonValue};
use num_bigint::{BigInt, Sign};
use std::mem::align_of;

use crate::array::encode_array;
use crate::errors::{EncodeError, EncodeResult};
use crate::header::{Category, Length, NumberHint, Primitive};
use crate::object::encode_object;

#[derive(Debug)]
pub(crate) struct Encoder {
    data: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn align<T>(&mut self) {
        let align = align_of::<T>();
        // same calculation as in `Decoder::align`
        let new_len = (self.data.len() + align - 1) & !(align - 1);
        self.data.resize(new_len, 0);
    }

    pub fn ring_fence(&mut self, size: usize) -> usize {
        let start = self.data.len();
        self.data.resize(start + size, 0);
        start
    }

    pub fn encode_value(&mut self, value: &JsonValue<'_>) -> EncodeResult<()> {
        match value {
            JsonValue::Null => self.encode_null(),
            JsonValue::Bool(b) => self.encode_bool(*b),
            JsonValue::Int(int) => self.encode_i64(*int),
            JsonValue::BigInt(big_int) => self.encode_big_int(big_int)?,
            JsonValue::Float(f) => self.encode_f64(*f),
            JsonValue::Str(s) => self.encode_str(s.as_ref())?,
            JsonValue::Array(array) => self.encode_array(array)?,
            JsonValue::Object(obj) => self.encode_object(obj)?,
        };
        Ok(())
    }

    pub fn position(&self) -> usize {
        self.data.len()
    }

    pub fn reset_position(&mut self, position: usize) {
        self.data.truncate(position);
    }

    pub fn encode_null(&mut self) {
        let h = Category::Primitive.encode_with(Primitive::Null as u8);
        self.push(h);
    }

    pub fn encode_bool(&mut self, bool: bool) {
        let right: Primitive = bool.into();
        let h = Category::Primitive.encode_with(right as u8);
        self.push(h);
    }

    pub fn encode_i64(&mut self, value: i64) {
        if (0..=10).contains(&value) {
            self.push(Category::Int.encode_with(value as u8));
        } else if let Ok(size_8) = i8::try_from(value) {
            self.push(Category::Int.encode_with(NumberHint::Size8 as u8));
            self.extend(&size_8.to_le_bytes());
        } else if let Ok(size_32) = i32::try_from(value) {
            self.push(Category::Int.encode_with(NumberHint::Size32 as u8));
            self.extend(&size_32.to_le_bytes());
        } else {
            self.push(Category::Int.encode_with(NumberHint::Size64 as u8));
            self.extend(&value.to_le_bytes());
        }
    }

    pub fn encode_f64(&mut self, value: f64) {
        match value {
            0.0 => self.push(Category::Float.encode_with(NumberHint::Zero as u8)),
            1.0 => self.push(Category::Float.encode_with(NumberHint::One as u8)),
            2.0 => self.push(Category::Float.encode_with(NumberHint::Two as u8)),
            3.0 => self.push(Category::Float.encode_with(NumberHint::Three as u8)),
            4.0 => self.push(Category::Float.encode_with(NumberHint::Four as u8)),
            5.0 => self.push(Category::Float.encode_with(NumberHint::Five as u8)),
            6.0 => self.push(Category::Float.encode_with(NumberHint::Six as u8)),
            7.0 => self.push(Category::Float.encode_with(NumberHint::Seven as u8)),
            8.0 => self.push(Category::Float.encode_with(NumberHint::Eight as u8)),
            9.0 => self.push(Category::Float.encode_with(NumberHint::Nine as u8)),
            10.0 => self.push(Category::Float.encode_with(NumberHint::Ten as u8)),
            _ => {
                // should we do something with f32 here?
                self.push(Category::Float.encode_with(NumberHint::Size64 as u8));
                self.extend(&value.to_le_bytes());
            }
        }
    }

    pub fn encode_big_int(&mut self, int: &BigInt) -> EncodeResult<()> {
        let (sign, bytes) = int.to_bytes_le();
        match sign {
            Sign::Minus => self.encode_length(Category::BigIntNeg, bytes.len())?,
            _ => self.encode_length(Category::BigIntPos, bytes.len())?,
        }
        self.extend(&bytes);
        Ok(())
    }

    pub fn encode_str(&mut self, s: &str) -> EncodeResult<()> {
        self.encode_length(Category::Str, s.len())?;
        self.extend(s.as_bytes());
        Ok(())
    }

    pub fn encode_object(&mut self, object: &JsonObject) -> EncodeResult<()> {
        encode_object(self, object)
    }

    pub fn encode_array(&mut self, array: &JsonArray) -> EncodeResult<()> {
        encode_array(self, array)
    }

    pub fn extend(&mut self, s: &[u8]) {
        self.data.extend_from_slice(s);
    }

    pub fn set_range(&mut self, start: usize, s: &[u8]) {
        self.data[start..start + s.len()].as_mut().copy_from_slice(s);
    }

    pub fn encode_length(&mut self, cat: Category, len: usize) -> EncodeResult<()> {
        match len {
            0 => self.push(cat.encode_with(Length::Empty as u8)),
            1 => self.push(cat.encode_with(Length::One as u8)),
            2 => self.push(cat.encode_with(Length::Two as u8)),
            3 => self.push(cat.encode_with(Length::Three as u8)),
            4 => self.push(cat.encode_with(Length::Four as u8)),
            5 => self.push(cat.encode_with(Length::Five as u8)),
            6 => self.push(cat.encode_with(Length::Six as u8)),
            7 => self.push(cat.encode_with(Length::Seven as u8)),
            8 => self.push(cat.encode_with(Length::Eight as u8)),
            9 => self.push(cat.encode_with(Length::Nine as u8)),
            10 => self.push(cat.encode_with(Length::Ten as u8)),
            _ => {
                if let Ok(s) = u8::try_from(len) {
                    self.push(cat.encode_with(Length::U8 as u8));
                    self.push(s);
                } else if let Ok(int) = u16::try_from(len) {
                    self.encode_len_u16(cat, int);
                } else {
                    self.encode_len_u32(cat, len)?;
                }
            }
        }
        Ok(())
    }

    pub fn encode_len_u16(&mut self, cat: Category, int: u16) {
        self.push(cat.encode_with(Length::U16 as u8));
        self.extend(&int.to_le_bytes());
    }

    pub fn encode_len_u32(&mut self, cat: Category, len: usize) -> EncodeResult<()> {
        let int = u32::try_from(len).map_err(|_| match cat {
            Category::Str => EncodeError::StrTooLong,
            Category::HetArray => EncodeError::ObjectTooLarge,
            _ => EncodeError::ArrayTooLarge,
        })?;
        self.push(cat.encode_with(Length::U32 as u8));
        self.extend(&int.to_le_bytes());
        Ok(())
    }

    pub fn push(&mut self, h: u8) {
        self.data.push(h);
    }
}

impl From<Encoder> for Vec<u8> {
    fn from(encoder: Encoder) -> Self {
        encoder.data
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::decoder::Decoder;
    use crate::header::Header;

    #[test]
    fn encode_int() {
        let mut enc = Encoder::new();
        enc.encode_i64(0);
        let h = Decoder::new(&enc.data).take_header().unwrap();
        assert_eq!(h, Header::Int(NumberHint::Zero));

        let mut enc = Encoder::new();
        enc.encode_i64(7);
        let h = Decoder::new(&enc.data).take_header().unwrap();
        assert_eq!(h, Header::Int(NumberHint::Seven));
    }
}
