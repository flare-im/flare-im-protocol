//! Reading a type from a [`serde_json::Value`], as `serde_json::from_value`
//! does, except that a struct is read from an object only.
//!
//! serde lets a derived struct be written as an array of its fields in field
//! order, and `serde_json::from_value` compiles that second reader for every
//! struct it reads. No JSON a Flare client reads writes a struct that way, so
//! in the browser bundle those readers were dead code, about a tenth of it.
//! Reading through [`from_value`] leaves them out.
//!
//! Everything else reads as through `serde_json::from_value`: the same
//! values, the same rules and the same errors, with one difference: a struct
//! given as an array is refused (`invalid type: sequence, expected struct
//! …`). A tuple, a tuple struct and a sequence still read from an array.
//!
//! A client reads all of its JSON values through here, its own types
//! included (the Flare client SDK does), so that each type is compiled once.
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    Unexpected, VariantAccess, Visitor,
};
use serde_json::{Error, Map, Value};

/// Read `T` from `value`; see the module documentation.
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    T::deserialize(ValueDeserializer(value))
}

/// The deserializer [`from_value`] reads with.
#[derive(Debug)]
pub struct ValueDeserializer(pub Value);

fn unexpected(value: &Value) -> Unexpected<'_> {
    match value {
        Value::Null => Unexpected::Unit,
        Value::Bool(b) => Unexpected::Bool(*b),
        Value::Number(n) => {
            if let Some(n) = n.as_u64() {
                Unexpected::Unsigned(n)
            } else if let Some(n) = n.as_i64() {
                Unexpected::Signed(n)
            } else {
                Unexpected::Float(n.as_f64().unwrap_or(f64::NAN))
            }
        }
        Value::String(s) => Unexpected::Str(s),
        Value::Array(_) => Unexpected::Seq,
        Value::Object(_) => Unexpected::Map,
    }
}

fn invalid_type<'de, V: Visitor<'de>>(value: &Value, visitor: &V) -> Error {
    de::Error::invalid_type(unexpected(value), visitor)
}

fn visit_array<'de, V: Visitor<'de>>(array: Vec<Value>, visitor: V) -> Result<V::Value, Error> {
    let len = array.len();
    let mut seq = SeqDeserializer(array.into_iter());
    let value = visitor.visit_seq(&mut seq)?;
    if seq.0.len() == 0 {
        Ok(value)
    } else {
        Err(de::Error::invalid_length(len, &"fewer elements in array"))
    }
}

fn visit_object<'de, V: Visitor<'de>>(
    object: Map<String, Value>,
    visitor: V,
) -> Result<V::Value, Error> {
    let len = object.len();
    let mut map = MapDeserializer {
        iter: object.into_iter(),
        value: None,
    };
    let value = visitor.visit_map(&mut map)?;
    if map.iter.len() == 0 {
        Ok(value)
    } else {
        Err(de::Error::invalid_length(len, &"fewer elements in map"))
    }
}

macro_rules! forward_to_value {
    ($($method:ident)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
                de::Deserializer::$method(self.0, visitor)
            }
        )*
    };
}

impl<'de> de::Deserializer<'de> for ValueDeserializer {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Array(array) => visit_array(array, visitor),
            Value::Object(object) => visit_object(object, visitor),
            scalar => de::Deserializer::deserialize_any(scalar, visitor),
        }
    }

    // Scalars read exactly as from the value itself; none of them nests.
    forward_to_value! {
        deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64
        deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64
        deserialize_u128 deserialize_f32 deserialize_f64 deserialize_char deserialize_str
        deserialize_string deserialize_identifier
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_byte_buf(visitor)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::String(s) => visitor.visit_string(s),
            Value::Array(array) => visit_array(array, visitor),
            other => Err(invalid_type(&other, &visitor)),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Null => visitor.visit_unit(),
            other => Err(invalid_type(&other, &visitor)),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Array(array) => visit_array(array, visitor),
            other => Err(invalid_type(&other, &visitor)),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Value::Object(object) => visit_object(object, visitor),
            other => Err(invalid_type(&other, &visitor)),
        }
    }

    /// From an object only: the one difference from `serde_json::from_value`.
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.0 {
            Value::Object(object) => {
                let mut entries = object.into_iter();
                match (entries.next(), entries.next()) {
                    (Some((variant, value)), None) => visitor.visit_enum(EnumDeserializer {
                        variant,
                        value: Some(value),
                    }),
                    _ => Err(de::Error::invalid_value(
                        Unexpected::Map,
                        &"map with a single key",
                    )),
                }
            }
            Value::String(variant) => visitor.visit_enum(EnumDeserializer {
                variant,
                value: None,
            }),
            other => Err(de::Error::invalid_type(
                unexpected(&other),
                &"string or map",
            )),
        }
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        drop(self);
        visitor.visit_unit()
    }
}

struct SeqDeserializer(std::vec::IntoIter<Value>);

impl<'de> SeqAccess<'de> for SeqDeserializer {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        match self.0.next() {
            Some(value) => seed.deserialize(ValueDeserializer(value)).map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.0.len())
    }
}

struct MapDeserializer {
    iter: <Map<String, Value> as IntoIterator>::IntoIter,
    value: Option<Value>,
}

impl<'de> MapAccess<'de> for MapDeserializer {
    type Error = Error;

    fn next_key_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                seed.deserialize(KeyDeserializer(key)).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value, Error> {
        match self.value.take() {
            Some(value) => seed.deserialize(ValueDeserializer(value)),
            None => Err(de::Error::custom("value is missing")),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

/// An object key: a string, read as a number, a boolean or a unit variant
/// where the key type asks for one, as `serde_json` reads keys.
struct KeyDeserializer(String);

macro_rules! numeric_key {
    ($($method:ident => $visit:ident: $ty:ty),* $(,)?) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
                match self.0.parse::<$ty>() {
                    Ok(number) if self.0.starts_with(|c: char| c == '-' || c.is_ascii_digit()) => {
                        visitor.$visit(number)
                    }
                    _ => Err(de::Error::custom("expected numeric key")),
                }
            }
        )*
    };
}

impl<'de> de::Deserializer<'de> for KeyDeserializer {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_string(self.0)
    }

    numeric_key! {
        deserialize_i8 => visit_i8: i8,
        deserialize_i16 => visit_i16: i16,
        deserialize_i32 => visit_i32: i32,
        deserialize_i64 => visit_i64: i64,
        deserialize_i128 => visit_i128: i128,
        deserialize_u8 => visit_u8: u8,
        deserialize_u16 => visit_u16: u16,
        deserialize_u32 => visit_u32: u32,
        deserialize_u64 => visit_u64: u64,
        deserialize_u128 => visit_u128: u128,
        deserialize_f32 => visit_f32: f32,
        deserialize_f64 => visit_f64: f64,
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0.as_str() {
            "true" => visitor.visit_bool(true),
            "false" => visitor.visit_bool(false),
            _ => Err(de::Error::invalid_type(Unexpected::Str(&self.0), &visitor)),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_enum(self.0.into_deserializer())
    }

    serde::forward_to_deserialize_any! {
        char str string bytes byte_buf unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

struct EnumDeserializer {
    variant: String,
    value: Option<Value>,
}

impl<'de> EnumAccess<'de> for EnumDeserializer {
    type Error = Error;
    type Variant = VariantDeserializer;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, VariantDeserializer), Error> {
        let variant: de::value::StringDeserializer<Error> = self.variant.into_deserializer();
        let value = VariantDeserializer(self.value);
        seed.deserialize(variant).map(|v| (v, value))
    }
}

struct VariantDeserializer(Option<Value>);

impl<'de> VariantAccess<'de> for VariantDeserializer {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        match self.0 {
            Some(value) => de::Deserialize::deserialize(ValueDeserializer(value)),
            None => Ok(()),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        match self.0 {
            Some(value) => seed.deserialize(ValueDeserializer(value)),
            None => Err(de::Error::invalid_type(
                Unexpected::UnitVariant,
                &"newtype variant",
            )),
        }
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            Some(Value::Array(array)) if array.is_empty() => visitor.visit_unit(),
            Some(Value::Array(array)) => visit_array(array, visitor),
            Some(other) => Err(de::Error::invalid_type(
                unexpected(&other),
                &"tuple variant",
            )),
            None => Err(de::Error::invalid_type(
                Unexpected::UnitVariant,
                &"tuple variant",
            )),
        }
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.0 {
            Some(Value::Object(object)) => visit_object(object, visitor),
            Some(other) => Err(de::Error::invalid_type(
                unexpected(&other),
                &"struct variant",
            )),
            None => Err(de::Error::invalid_type(
                Unexpected::UnitVariant,
                &"struct variant",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Sample {
        name: String,
        #[serde(default)]
        count: Option<u32>,
        tags: Vec<String>,
        pair: (u8, bool),
        by_number: BTreeMap<u64, String>,
        kind: Kind,
        shape: Shape,
        extra: Value,
    }
    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "lowercase")]
    enum Kind {
        One,
        Two,
    }
    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "lowercase")]
    enum Shape {
        Dot,
        Line(u32),
        Box { w: u32, h: u32 },
        Pair(u8, u8),
    }

    fn both<T: DeserializeOwned + PartialEq + std::fmt::Debug>(value: Value) {
        let ours = from_value::<T>(value.clone()).map_err(|e| e.to_string());
        let theirs = serde_json::from_value::<T>(value).map_err(|e| e.to_string());
        assert_eq!(ours, theirs);
    }

    #[test]
    fn reads_as_serde_json_reads_a_value() {
        let sample = json!({
            "name": "a", "count": 3, "tags": ["x", "y"], "pair": [7, true],
            "byNumber": {"1": "one", "20": "twenty"}, "kind": "two",
            "shape": {"box": {"w": 1, "h": 2}}, "extra": {"any": [1, null]}
        });
        both::<Sample>(sample.clone());
        assert!(from_value::<Sample>(sample.clone()).is_ok());
        for shape in [
            json!("dot"),
            json!({"line": 4}),
            json!({"pair": [1, 2]}),
            json!({"box": {"w": 1}}),
            json!("line"),
            json!({"dot": null, "line": 1}),
            json!(3),
        ] {
            both::<Shape>(shape);
        }
        // Errors read the same.
        for broken in [
            json!({"name": 1}),
            json!({"name": "a", "tags": [], "pair": [1], "byNumber": {}, "kind": "one", "shape": "dot", "extra": null}),
            json!({"name": "a", "tags": [], "pair": [1, true, 3], "byNumber": {}, "kind": "one", "shape": "dot", "extra": null}),
            json!({"name": "a", "unknown": 1}),
            json!({"name": "a", "tags": [], "pair": [1, true], "byNumber": {"x": "?"}, "kind": "one", "shape": "dot", "extra": null}),
            json!({"name": "a", "tags": [], "pair": [1, true], "byNumber": {}, "kind": "three", "shape": "dot", "extra": null}),
        ] {
            let ours = from_value::<Sample>(broken.clone());
            let theirs = serde_json::from_value::<Sample>(broken);
            assert!(ours.is_err() && theirs.is_err());
        }
        both::<Option<Vec<(String, u8)>>>(json!([["a", 1]]));
        both::<Option<u8>>(json!(null));
        both::<Value>(json!({"a": [1, 2.5, -3, "s", true, null, {}]}));
    }

    #[test]
    fn a_struct_reads_from_an_object_only() {
        let fields = json!(["a", 3, [], [1, true], {}, "one", "dot", null]);
        assert!(serde_json::from_value::<Sample>(fields.clone()).is_ok());
        let error = from_value::<Sample>(fields).unwrap_err().to_string();
        assert_eq!(error, "invalid type: sequence, expected struct Sample");
    }
}
