//! Reading a small JSON object field by field.
//!
//! The bridge reads every message a bundle sends and every request a host
//! makes for it. Reading those through serde compiles a visitor for each of
//! their types; in the browser engine that was most of the bridge's size.
//! [`Fields`] reads them all with one non-generic reader that keeps serde's
//! rules for these shapes: only the named fields (`deny_unknown_fields`), a
//! required field present with its type, an optional one absent or `null`, a
//! defaulted one absent. Any other value is refused with `None`.
use serde_json::{Map, Value};

/// An object whose fields are taken one by one.
#[derive(Debug)]
pub struct Fields(Map<String, Value>);

impl Fields {
    /// `value` when it is an object whose every field is in `known`.
    pub fn new(value: Value, known: &[&str]) -> Option<Self> {
        match value {
            Value::Object(map) if map.keys().all(|key| known.contains(&key.as_str())) => {
                Some(Self(map))
            }
            _ => None,
        }
    }

    /// The field as given (`null` included); `None` when absent.
    pub fn take(&mut self, key: &str) -> Option<Value> {
        self.0.remove(key)
    }

    /// An `Option<Value>` field: absent and `null` read as `None`.
    pub fn optional(&mut self, key: &str) -> Option<Value> {
        self.take(key).filter(|value| !value.is_null())
    }

    /// A required field of any value, `null` included.
    pub fn value(&mut self, key: &str) -> Option<Value> {
        self.take(key)
    }

    /// A field with a default: absent reads as `default()`.
    pub fn value_or(&mut self, key: &str, default: fn() -> Value) -> Value {
        self.take(key).unwrap_or_else(default)
    }

    /// A required string.
    pub fn string(&mut self, key: &str) -> Option<String> {
        match self.take(key)? {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    /// An `Option<String>` field: `Some(None)` when absent or `null`,
    /// `None` when it is something else.
    pub fn opt_string(&mut self, key: &str) -> Option<Option<String>> {
        match self.optional(key) {
            None => Some(None),
            Some(Value::String(text)) => Some(Some(text)),
            Some(_) => None,
        }
    }

    /// A string with a default (`#[serde(default)] String`): absent reads as
    /// empty; `null` is refused.
    pub fn string_or_empty(&mut self, key: &str) -> Option<String> {
        match self.take(key) {
            None => Some(String::new()),
            Some(Value::String(text)) => Some(text),
            Some(_) => None,
        }
    }

    /// A required `u32`.
    pub fn u32(&mut self, key: &str) -> Option<u32> {
        self.take(key)?
            .as_u64()
            .and_then(|number| u32::try_from(number).ok())
    }

    /// A `#[serde(default)] bool`: absent reads as `false`; `null` is refused.
    pub fn bool_or_false(&mut self, key: &str) -> Option<bool> {
        match self.take(key) {
            None => Some(false),
            Some(Value::Bool(flag)) => Some(flag),
            Some(_) => None,
        }
    }

    /// An `Option<Map>` field: `Some(None)` when absent or `null`, `None`
    /// when it is something else.
    pub fn opt_object(&mut self, key: &str) -> Option<Option<Map<String, Value>>> {
        match self.optional(key) {
            None => Some(None),
            Some(Value::Object(map)) => Some(Some(map)),
            Some(_) => None,
        }
    }

    /// A `#[serde(default)] Map` field: absent reads as empty; `null` is
    /// refused.
    pub fn object_or_empty(&mut self, key: &str) -> Option<Map<String, Value>> {
        match self.take(key) {
            None => Some(Map::new()),
            Some(Value::Object(map)) => Some(map),
            Some(_) => None,
        }
    }

    /// A required array of strings.
    pub fn strings(&mut self, key: &str) -> Option<Vec<String>> {
        match self.take(key)? {
            Value::Array(items) => items
                .into_iter()
                .map(|item| match item {
                    Value::String(text) => Some(text),
                    _ => None,
                })
                .collect(),
            _ => None,
        }
    }

    /// A required name of a unit enum, read with `parse`.
    pub fn name<T>(&mut self, key: &str, parse: fn(&str) -> Option<T>) -> Option<T> {
        match self.take(key)? {
            Value::String(text) => parse(&text),
            _ => None,
        }
    }

    /// A unit enum with a default: absent reads as `default`; `null` is
    /// refused.
    pub fn name_or<T>(&mut self, key: &str, parse: fn(&str) -> Option<T>, default: T) -> Option<T> {
        match self.take(key) {
            None => Some(default),
            Some(Value::String(text)) => parse(&text),
            Some(_) => None,
        }
    }

    /// An `Option` of a unit enum: `Some(None)` when absent or `null`,
    /// `None` when it is not one of its names.
    pub fn opt_name<T>(&mut self, key: &str, parse: fn(&str) -> Option<T>) -> Option<Option<T>> {
        match self.opt_string(key)? {
            None => Some(None),
            Some(text) => parse(&text).map(Some),
        }
    }
}
