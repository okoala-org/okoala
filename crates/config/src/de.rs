// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! A `serde` deserializer over a parsed TOML value that tracks the key path (for error messages) and loads
//! secret references when a field asks for one (see [`crate::secret`]).

use std::cell::RefCell;
use std::fmt;

use serde::de::value::StringDeserializer;
use serde::de::{
    self, DeserializeSeed, EnumAccess, Expected, IntoDeserializer, MapAccess, SeqAccess,
    Unexpected, VariantAccess, Visitor,
};
use serde::forward_to_deserialize_any;
use toml::Value;

use crate::secret::{BYTES_MARKER, TEXT_MARKER};
use crate::source::{Raw, SecretKind, SecretSource};

/// A deserialization error and the key path it happened at, once known.
#[derive(Debug)]
pub(crate) struct DeError {
    pub(crate) path: Option<Vec<String>>,
    pub(crate) message: String,
}

impl DeError {
    fn at(mut self, path: &[String]) -> Self {
        if self.path.is_none() {
            self.path = Some(path.to_vec());
        }
        self
    }
}

impl fmt::Display for DeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DeError {}

/// Strings and bytes are not echoed into errors: a secret put in the wrong field must not reach a log.
fn redact(unexpected: Unexpected<'_>) -> Unexpected<'_> {
    match unexpected {
        Unexpected::Str(_) => Unexpected::Other("a string"),
        Unexpected::Bytes(_) => Unexpected::Other("a byte string"),
        Unexpected::Char(_) => Unexpected::Other("a character"),
        other => other,
    }
}

impl de::Error for DeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self {
            path: None,
            message: msg.to_string(),
        }
    }

    fn invalid_type(unexpected: Unexpected<'_>, expected: &dyn Expected) -> Self {
        Self::custom(format_args!(
            "invalid type: {}, expected {expected}",
            redact(unexpected)
        ))
    }

    fn invalid_value(unexpected: Unexpected<'_>, expected: &dyn Expected) -> Self {
        Self::custom(format_args!(
            "invalid value: {}, expected {expected}",
            redact(unexpected)
        ))
    }
}

/// Shared by every deserializer of one load: where secrets come from, and the secret errors recorded so far
/// (recorded rather than returned, so that every secret error of a section is reported).
pub(crate) struct Ctx<'a> {
    pub(crate) secrets: SecretSource<'a>,
    pub(crate) secret_errors: RefCell<Vec<DeError>>,
}

pub(crate) struct ValueDe<'c, 'a> {
    value: Value,
    path: Vec<String>,
    ctx: &'c Ctx<'a>,
}

impl<'c, 'a> ValueDe<'c, 'a> {
    pub(crate) fn new(value: Value, ctx: &'c Ctx<'a>) -> Self {
        Self {
            value,
            path: Vec::new(),
            ctx,
        }
    }

    fn child(&self, value: Value, segment: String) -> Self {
        let mut path = self.path.clone();
        path.push(segment);
        Self {
            value,
            path,
            ctx: self.ctx,
        }
    }

    fn load_secret<'de, V: Visitor<'de>>(
        self,
        kind: SecretKind,
        visitor: V,
    ) -> Result<V::Value, DeError> {
        match self.ctx.secrets.load(self.value, kind) {
            Ok(Raw::Text(text)) => visitor.visit_string(text),
            Ok(Raw::Bytes(bytes)) => visitor.visit_byte_buf(bytes),
            Err(message) => {
                self.ctx.secret_errors.borrow_mut().push(DeError {
                    path: Some(self.path),
                    message,
                });
                // A placeholder so the rest of the section is still checked; the load fails anyway because an
                // error was recorded, so the placeholder never leaves the loader.
                match kind {
                    SecretKind::Text => visitor.visit_string(String::new()),
                    SecretKind::Bytes => visitor.visit_byte_buf(Vec::new()),
                }
            }
        }
    }
}

impl<'de> de::Deserializer<'de> for ValueDe<'_, '_> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        let ValueDe { value, path, ctx } = self;
        match value {
            Value::String(s) => visitor.visit_string(s),
            Value::Integer(i) => visitor.visit_i64(i),
            Value::Float(f) => visitor.visit_f64(f),
            Value::Boolean(b) => visitor.visit_bool(b),
            Value::Datetime(d) => visitor.visit_string(d.to_string()),
            Value::Array(items) => {
                let len = items.len();
                let mut seq = Seq {
                    items: items.into_iter(),
                    index: 0,
                    parent: ValueDe {
                        value: Value::Boolean(false),
                        path,
                        ctx,
                    },
                };
                let result = visitor.visit_seq(&mut seq);
                let path = &seq.parent.path;
                let value = result.map_err(|e| e.at(path))?;
                if seq.items.len() == 0 {
                    Ok(value)
                } else {
                    Err(
                        <DeError as de::Error>::invalid_length(len, &"fewer elements in the array")
                            .at(path),
                    )
                }
            }
            Value::Table(table) => {
                let mut map = Map {
                    entries: table.into_iter(),
                    pending: None,
                    parent: ValueDe {
                        value: Value::Boolean(false),
                        path,
                        ctx,
                    },
                };
                let result = visitor.visit_map(&mut map);
                result.map_err(|e| {
                    // An error raised by the visitor right after reading a key (an unknown field) belongs to that
                    // key; any other error at this level belongs to the table.
                    let mut path = map.parent.path.clone();
                    if let Some((key, _)) = &map.pending {
                        path.push(key.clone());
                    }
                    e.at(&path)
                })
            }
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, DeError> {
        if name == TEXT_MARKER {
            self.load_secret(SecretKind::Text, visitor)
        } else if name == BYTES_MARKER {
            self.load_secret(SecretKind::Bytes, visitor)
        } else {
            visitor.visit_newtype_struct(self)
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        let ValueDe { value, path, ctx } = self;
        match value {
            Value::String(variant) => {
                let access: StringDeserializer<DeError> = variant.into_deserializer();
                visitor.visit_enum(access).map_err(|e| e.at(&path))
            }
            Value::Table(table) if table.len() == 1 => {
                let Some((variant, value)) = table.into_iter().next() else {
                    return Err(
                        <DeError as de::Error>::custom("expected a table with one key").at(&path),
                    );
                };
                let mut content_path = path.clone();
                content_path.push(variant.clone());
                let content = ValueDe {
                    value,
                    path: content_path,
                    ctx,
                };
                visitor
                    .visit_enum(Enum { variant, content })
                    .map_err(|e| e.at(&path))
            }
            _ => Err(<DeError as de::Error>::custom(
                "expected a string, or a table with one key naming the variant",
            )
            .at(&path)),
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf unit unit_struct
        seq tuple tuple_struct map struct identifier ignored_any
    }
}

struct Seq<'c, 'a> {
    items: std::vec::IntoIter<Value>,
    index: usize,
    parent: ValueDe<'c, 'a>,
}

impl<'de> SeqAccess<'de> for Seq<'_, '_> {
    type Error = DeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, DeError> {
        let Some(item) = self.items.next() else {
            return Ok(None);
        };
        let child = self.parent.child(item, format!("[{}]", self.index));
        self.index = self.index.saturating_add(1);
        let path = child.path.clone();
        seed.deserialize(child).map(Some).map_err(|e| e.at(&path))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.items.len())
    }
}

struct Map<'c, 'a> {
    entries: toml::map::IntoIter<String, Value>,
    pending: Option<(String, Value)>,
    parent: ValueDe<'c, 'a>,
}

impl<'de> MapAccess<'de> for Map<'_, '_> {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, DeError> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        self.pending = Some((key.clone(), value));
        let key: StringDeserializer<DeError> = key.into_deserializer();
        seed.deserialize(key).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, DeError> {
        let Some((key, value)) = self.pending.take() else {
            return Err(
                <DeError as de::Error>::custom("value requested before its key")
                    .at(&self.parent.path),
            );
        };
        let child = self.parent.child(value, key);
        let path = child.path.clone();
        seed.deserialize(child).map_err(|e| e.at(&path))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len())
    }
}

struct Enum<'c, 'a> {
    variant: String,
    content: ValueDe<'c, 'a>,
}

impl<'de, 'c, 'a> EnumAccess<'de> for Enum<'c, 'a> {
    type Error = DeError;
    type Variant = ValueDe<'c, 'a>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), DeError> {
        let variant: StringDeserializer<DeError> = self.variant.into_deserializer();
        let value = seed.deserialize(variant)?;
        Ok((value, self.content))
    }
}

impl<'de> VariantAccess<'de> for ValueDe<'_, '_> {
    type Error = DeError;

    fn unit_variant(self) -> Result<(), DeError> {
        Err(
            <DeError as de::Error>::custom("a unit variant is given as a plain string")
                .at(&self.path),
        )
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, DeError> {
        let path = self.path.clone();
        seed.deserialize(self).map_err(|e| e.at(&path))
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, DeError> {
        de::Deserializer::deserialize_any(self, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        de::Deserializer::deserialize_any(self, visitor)
    }
}
