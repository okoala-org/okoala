// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! A `serde` deserializer over a parsed TOML value that tracks the key path (for error messages) and loads
//! secret references when a field asks for one (see [`crate::secret`]).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;

use serde::de::value::StringDeserializer;
use serde::de::{
    self, DeserializeSeed, EnumAccess, Expected, IntoDeserializer, MapAccess, SeqAccess,
    Unexpected, VariantAccess, Visitor,
};
use serde::forward_to_deserialize_any;
use toml::Value;

use crate::secret::{BYTES_MARKER, LOADED_VARIANT, TEXT_MARKER};
use crate::source::{
    Raw, Reference, SecretKind, SecretSource, discard_bytes, finish, parse_reference,
};

/// A deserialization error and the key path it happened at, once known.
#[derive(Debug)]
pub(crate) struct DeError {
    pub(crate) path: Option<Vec<String>>,
    pub(crate) message: String,
}

impl DeError {
    /// An error raised by okoala-config's own code, whose message is trusted not to contain values.
    pub(crate) fn own(message: impl fmt::Display) -> Self {
        Self {
            path: None,
            message: message.to_string(),
        }
    }

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

struct OneOf(&'static [&'static str]);

impl fmt::Display for OneOf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            [] => f.write_str("there are none"),
            [only] => write!(f, "expected `{only}`"),
            names => {
                f.write_str("expected one of ")?;
                for (i, name) in names.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "`{name}`")?;
                }
                Ok(())
            }
        }
    }
}

/// Every message is built here from trusted parts: the expected types and names come from the section types,
/// and field names are keys. Values from the file or environment are never included, and messages from the
/// field types themselves (`custom`, which a `FromStr`-based type may fill with its input) are replaced by a
/// generic one: the key path, added by the deserializer, says where the problem is.
impl de::Error for DeError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        Self::own("invalid value (rejected by the field's type; the value is not shown)")
    }

    fn invalid_type(unexpected: Unexpected<'_>, expected: &dyn Expected) -> Self {
        Self::own(format_args!(
            "invalid type: {}, expected {expected}",
            redact(unexpected)
        ))
    }

    fn invalid_value(unexpected: Unexpected<'_>, expected: &dyn Expected) -> Self {
        Self::own(format_args!(
            "invalid value: {}, expected {expected}",
            redact(unexpected)
        ))
    }

    fn invalid_length(len: usize, expected: &dyn Expected) -> Self {
        Self::own(format_args!("invalid length {len}, expected {expected}"))
    }

    fn unknown_variant(_variant: &str, expected: &'static [&'static str]) -> Self {
        Self::own(format_args!("unknown variant; {}", OneOf(expected)))
    }

    fn unknown_field(field: &str, expected: &'static [&'static str]) -> Self {
        Self::own(format_args!("unknown field `{field}`; {}", OneOf(expected)))
    }

    fn missing_field(field: &'static str) -> Self {
        Self::own(format_args!("missing field `{field}`"))
    }

    fn duplicate_field(field: &'static str) -> Self {
        Self::own(format_args!("duplicate field `{field}`"))
    }
}

/// Shared by every deserializer of one load: where secrets come from, and the secret errors recorded so far
/// (recorded rather than returned, so that every secret error of a section is reported).
pub(crate) struct Ctx<'a> {
    pub(crate) secrets: SecretSource<'a>,
    pub(crate) secret_errors: RefCell<Vec<DeError>>,
    /// The secret references of the section being loaded, fetched before deserialization (so that references
    /// `serde` never reaches, after a structural error, are still checked), by key path. Deserialization
    /// takes the ones it reaches.
    pub(crate) fetched: RefCell<Fetched>,
}

pub(crate) type Fetched = BTreeMap<Vec<String>, (Reference, Result<Vec<u8>, String>)>;

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
        let raw = parse_reference(self.value).and_then(|reference| {
            let fetched = match self.ctx.fetched.borrow_mut().remove(&self.path) {
                Some((fetched_ref, fetched)) if fetched_ref == reference => fetched,
                Some((_, stale)) => {
                    if let Ok(bytes) = stale {
                        discard_bytes(bytes);
                    }
                    self.ctx.secrets.fetch(&reference)
                }
                None => self.ctx.secrets.fetch(&reference),
            };
            finish(fetched?, kind, &reference)
        });
        let raw = match raw {
            Ok(raw) => raw,
            Err(message) => {
                self.ctx.secret_errors.borrow_mut().push(DeError {
                    path: Some(self.path),
                    message,
                });
                // A placeholder so the rest of the section is still checked; the load fails anyway because an
                // error was recorded, so the placeholder never leaves the loader.
                match kind {
                    SecretKind::Text => Raw::Text(String::new()),
                    SecretKind::Bytes => Raw::Bytes(Vec::new()),
                }
            }
        };
        visitor.visit_enum(Loaded { raw })
    }
}

/// How the loader hands a secret to the visitor of [`crate::secret`]: as the newtype variant
/// [`LOADED_VARIANT`] of an enum, a form no TOML value deserializes to, so those visitors can tell a value from
/// the loader apart from one some other deserializer produced.
struct Loaded {
    raw: Raw,
}

impl<'de> EnumAccess<'de> for Loaded {
    type Error = DeError;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), DeError> {
        let tag: StringDeserializer<DeError> = LOADED_VARIANT.to_owned().into_deserializer();
        Ok((seed.deserialize(tag)?, self))
    }
}

impl<'de> VariantAccess<'de> for Loaded {
    type Error = DeError;

    fn unit_variant(self) -> Result<(), DeError> {
        Err(DeError::own("a loaded secret is a newtype variant"))
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, DeError> {
        seed.deserialize(Payload(self.raw))
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, _visitor: V) -> Result<V::Value, DeError> {
        Err(DeError::own("a loaded secret is a newtype variant"))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, DeError> {
        Err(DeError::own("a loaded secret is a newtype variant"))
    }
}

/// The secret itself, handed over by value (`visit_string` or `visit_byte_buf`) so it is never copied.
struct Payload(Raw);

impl<'de> de::Deserializer<'de> for Payload {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self.0 {
            Raw::Text(text) => visitor.visit_string(text),
            Raw::Bytes(bytes) => visitor.visit_byte_buf(bytes),
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
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
                    return Err(DeError::own("expected a table with one key").at(&path));
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
            _ => Err(
                DeError::own("expected a string, or a table with one key naming the variant")
                    .at(&path),
            ),
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
            return Err(DeError::own("value requested before its key").at(&self.parent.path));
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
        Err(DeError::own("a unit variant is given as a plain string").at(&self.path))
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
