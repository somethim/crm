//! Test-only serde round-trip through an in-memory value tree.
//!
//! crm-domain depends only on serde (no serde_json), so the serde tests use this
//! minimal self-describing data model. It supports what the entities use:
//! integers, booleans, strings, options, sequences and structs.

use serde::de::value::{MapDeserializer, SeqDeserializer};
use serde::de::{self, IntoDeserializer, Visitor};
use serde::ser::{self, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    I64(i64),
    Str(String),
    Seq(Vec<Value>),
    Map(Vec<(String, Value)>),
}

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error(msg.to_string())
    }
}

impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error(msg.to_string())
    }
}

pub fn to_value<T: Serialize>(value: &T) -> Value {
    value.serialize(ValueSerializer).expect("serialize")
}

pub fn from_value<T: de::DeserializeOwned>(value: Value) -> T {
    T::deserialize(value).expect("deserialize")
}

struct ValueSerializer;

fn unsupported<T>(what: &str) -> Result<T, Error> {
    Err(Error(format!("unsupported in test_value: {what}")))
}

pub struct SeqSer(Vec<Value>);
pub struct MapSer(Vec<(String, Value)>, Option<String>);

impl ser::Serializer for ValueSerializer {
    type Ok = Value;
    type Error = Error;
    type SerializeSeq = SeqSer;
    type SerializeTuple = SeqSer;
    type SerializeTupleStruct = SeqSer;
    type SerializeTupleVariant = ser::Impossible<Value, Error>;
    type SerializeMap = MapSer;
    type SerializeStruct = MapSer;
    type SerializeStructVariant = ser::Impossible<Value, Error>;

    fn serialize_bool(self, v: bool) -> Result<Value, Error> {
        Ok(Value::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<Value, Error> {
        Ok(Value::I64(v.into()))
    }
    fn serialize_i16(self, v: i16) -> Result<Value, Error> {
        Ok(Value::I64(v.into()))
    }
    fn serialize_i32(self, v: i32) -> Result<Value, Error> {
        Ok(Value::I64(v.into()))
    }
    fn serialize_i64(self, v: i64) -> Result<Value, Error> {
        Ok(Value::I64(v))
    }
    fn serialize_u8(self, v: u8) -> Result<Value, Error> {
        Ok(Value::I64(v.into()))
    }
    fn serialize_u16(self, v: u16) -> Result<Value, Error> {
        Ok(Value::I64(v.into()))
    }
    fn serialize_u32(self, v: u32) -> Result<Value, Error> {
        Ok(Value::I64(v.into()))
    }
    fn serialize_u64(self, v: u64) -> Result<Value, Error> {
        i64::try_from(v)
            .map(Value::I64)
            .or_else(|_| unsupported("u64"))
    }
    fn serialize_f32(self, _: f32) -> Result<Value, Error> {
        unsupported("f32")
    }
    fn serialize_f64(self, _: f64) -> Result<Value, Error> {
        unsupported("f64")
    }
    fn serialize_char(self, v: char) -> Result<Value, Error> {
        Ok(Value::Str(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<Value, Error> {
        Ok(Value::Str(v.to_owned()))
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<Value, Error> {
        unsupported("bytes")
    }
    fn serialize_none(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Value, Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Value, Error> {
        Ok(Value::Null)
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<Value, Error> {
        Ok(Value::Str(variant.to_owned()))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<Value, Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<Value, Error> {
        unsupported("newtype variant")
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<SeqSer, Error> {
        Ok(SeqSer(Vec::new()))
    }
    fn serialize_tuple(self, _: usize) -> Result<SeqSer, Error> {
        Ok(SeqSer(Vec::new()))
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<SeqSer, Error> {
        Ok(SeqSer(Vec::new()))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        unsupported("tuple variant")
    }
    fn serialize_map(self, _: Option<usize>) -> Result<MapSer, Error> {
        Ok(MapSer(Vec::new(), None))
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<MapSer, Error> {
        Ok(MapSer(Vec::new(), None))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        unsupported("struct variant")
    }
}

impl ser::SerializeSeq for SeqSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.0.push(value.serialize(ValueSerializer)?);
        Ok(())
    }
    fn end(self) -> Result<Value, Error> {
        Ok(Value::Seq(self.0))
    }
}

impl ser::SerializeTuple for SeqSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Value, Error> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SeqSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Value, Error> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeMap for MapSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        match key.serialize(ValueSerializer)? {
            Value::Str(k) => {
                self.1 = Some(k);
                Ok(())
            }
            _ => unsupported("non-string map key"),
        }
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let key = self.1.take().expect("key before value");
        self.0.push((key, value.serialize(ValueSerializer)?));
        Ok(())
    }
    fn end(self) -> Result<Value, Error> {
        Ok(Value::Map(self.0))
    }
}

impl ser::SerializeStruct for MapSer {
    type Ok = Value;
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.0
            .push((key.to_owned(), value.serialize(ValueSerializer)?));
        Ok(())
    }
    fn end(self) -> Result<Value, Error> {
        Ok(Value::Map(self.0))
    }
}

impl<'de> IntoDeserializer<'de, Error> for Value {
    type Deserializer = Value;
    fn into_deserializer(self) -> Value {
        self
    }
}

impl<'de> de::Deserializer<'de> for Value {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(b),
            Value::I64(n) => visitor.visit_i64(n),
            Value::Str(s) => visitor.visit_string(s),
            Value::Seq(items) => visitor.visit_seq(SeqDeserializer::new(items.into_iter())),
            Value::Map(entries) => visitor.visit_map(MapDeserializer::new(entries.into_iter())),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self {
            Value::Null => visitor.visit_none(),
            other => visitor.visit_some(other),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct enum
        identifier ignored_any
    }
}
