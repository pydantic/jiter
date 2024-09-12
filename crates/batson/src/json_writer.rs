use serde::ser::Serializer as _;
use serde_json::ser::Serializer;

use crate::errors::ToJsonResult;

pub(crate) struct JsonWriter {
    vec: Vec<u8>,
}

impl JsonWriter {
    pub fn new() -> Self {
        Self {
            vec: Vec::with_capacity(128),
        }
    }

    pub fn write_null(&mut self) {
        self.vec.extend_from_slice(b"null");
    }

    #[allow(clippy::needless_pass_by_value)]
    pub fn write_value(&mut self, v: impl WriteJson) -> ToJsonResult<()> {
        v.write_json(self)
    }

    pub fn write_seq<'a>(&mut self, mut v: impl Iterator<Item = &'a (impl WriteJson + 'a)>) -> ToJsonResult<()> {
        self.start_array();

        if let Some(first) = v.next() {
            first.write_json(self)?;
            for value in v {
                self.comma();
                value.write_json(self)?;
            }
        }
        self.end_array();
        Ok(())
    }

    pub fn write_empty_array(&mut self) {
        self.vec.extend_from_slice(b"[]");
    }

    pub fn start_array(&mut self) {
        self.vec.push(b'[');
    }

    pub fn end_array(&mut self) {
        self.vec.push(b']');
    }

    pub fn write_key(&mut self, key: &str) -> ToJsonResult<()> {
        self.write_value(key)?;
        self.vec.push(b':');
        Ok(())
    }

    pub fn write_empty_object(&mut self) {
        self.vec.extend_from_slice(b"{}");
    }

    pub fn start_object(&mut self) {
        self.vec.push(b'{');
    }

    pub fn end_object(&mut self) {
        self.vec.push(b'}');
    }

    pub fn comma(&mut self) {
        self.vec.push(b',');
    }
}

impl From<JsonWriter> for Vec<u8> {
    fn from(writer: JsonWriter) -> Self {
        writer.vec
    }
}

pub(crate) trait WriteJson {
    fn write_json(&self, writer: &mut JsonWriter) -> ToJsonResult<()>;
}

impl WriteJson for &str {
    fn write_json(&self, writer: &mut JsonWriter) -> ToJsonResult<()> {
        let mut ser = Serializer::new(&mut writer.vec);
        ser.serialize_str(self).map_err(Into::into)
    }
}

impl WriteJson for bool {
    fn write_json(&self, writer: &mut JsonWriter) -> ToJsonResult<()> {
        writer.vec.extend_from_slice(if *self { b"true" } else { b"false" });
        Ok(())
    }
}

impl WriteJson for u8 {
    fn write_json(&self, writer: &mut JsonWriter) -> ToJsonResult<()> {
        let mut ser = Serializer::new(&mut writer.vec);
        ser.serialize_u8(*self).map_err(Into::into)
    }
}

impl WriteJson for i64 {
    fn write_json(&self, writer: &mut JsonWriter) -> ToJsonResult<()> {
        let mut ser = Serializer::new(&mut writer.vec);
        ser.serialize_i64(*self).map_err(Into::into)
    }
}

impl WriteJson for f64 {
    fn write_json(&self, writer: &mut JsonWriter) -> ToJsonResult<()> {
        let mut ser = Serializer::new(&mut writer.vec);
        ser.serialize_f64(*self).map_err(Into::into)
    }
}
