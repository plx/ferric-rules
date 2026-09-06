//! Resource limits applied through Serde before collection allocation/recursion.

use serde::de::{self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor};
use std::cell::Cell;
use std::fmt;

pub(super) const MAX_DEPTH: usize = 128;
pub(super) const MAX_ITEMS: usize = 1_000_000;

pub(super) struct Limited<T>(pub T);

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Limited<T> {
    fn deserialize<D: de::Deserializer<'de>>(inner: D) -> Result<Self, D::Error> {
        let remaining = Cell::new(MAX_ITEMS);
        T::deserialize(Decoder {
            inner,
            remaining: &remaining,
            depth: 0,
        })
        .map(Self)
    }
}

struct Decoder<'a, D> {
    inner: D,
    remaining: &'a Cell<usize>,
    depth: usize,
}
struct LimitVisitor<'a, V> {
    inner: V,
    remaining: &'a Cell<usize>,
    depth: usize,
}
struct Seed<'a, S> {
    inner: S,
    remaining: &'a Cell<usize>,
    depth: usize,
}
struct Access<'a, A> {
    inner: A,
    remaining: &'a Cell<usize>,
    depth: usize,
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<'_, S> {
    type Value = S::Value;
    fn deserialize<D: de::Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        self.inner.deserialize(Decoder {
            inner,
            remaining: self.remaining,
            depth: self.depth + 1,
        })
    }
}

macro_rules! forward {
    ($name:ident $(, $arg:ident: $ty:ty)*) => {
        fn $name<V: Visitor<'de>>(self, $($arg: $ty,)* visitor: V) -> Result<V::Value, Self::Error> {
            if self.depth > MAX_DEPTH { return Err(de::Error::custom("snapshot nesting limit exceeded")); }
            let Some(remaining) = self.remaining.get().checked_sub(1) else {
                return Err(de::Error::custom("snapshot item limit exceeded"));
            };
            self.remaining.set(remaining);
            self.inner.$name($($arg,)* LimitVisitor { inner: visitor, remaining: self.remaining, depth: self.depth })
        }
    };
}

impl<'de, D: de::Deserializer<'de>> de::Deserializer<'de> for Decoder<'_, D> {
    type Error = D::Error;
    forward!(deserialize_any);
    forward!(deserialize_bool);
    forward!(deserialize_i8);
    forward!(deserialize_i16);
    forward!(deserialize_i32);
    forward!(deserialize_i64);
    forward!(deserialize_i128);
    forward!(deserialize_u8);
    forward!(deserialize_u16);
    forward!(deserialize_u32);
    forward!(deserialize_u64);
    forward!(deserialize_u128);
    forward!(deserialize_f32);
    forward!(deserialize_f64);
    forward!(deserialize_char);
    forward!(deserialize_str);
    forward!(deserialize_string);
    forward!(deserialize_bytes);
    forward!(deserialize_byte_buf);
    forward!(deserialize_option);
    forward!(deserialize_unit);
    forward!(deserialize_unit_struct, name: &'static str);
    forward!(deserialize_newtype_struct, name: &'static str);
    forward!(deserialize_seq);
    forward!(deserialize_tuple, len: usize);
    forward!(deserialize_tuple_struct, name: &'static str, len: usize);
    forward!(deserialize_map);
    forward!(deserialize_struct, name: &'static str, fields: &'static [&'static str]);
    forward!(deserialize_enum, name: &'static str, variants: &'static [&'static str]);
    forward!(deserialize_identifier);
    forward!(deserialize_ignored_any);
    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

macro_rules! scalar {
    ($name:ident, $ty:ty) => {
        fn $name<E: de::Error>(self, value: $ty) -> Result<Self::Value, E> {
            self.inner.$name(value)
        }
    };
}

impl<'de, V: Visitor<'de>> Visitor<'de> for LimitVisitor<'_, V> {
    type Value = V::Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.expecting(f)
    }
    scalar!(visit_bool, bool);
    scalar!(visit_i8, i8);
    scalar!(visit_i16, i16);
    scalar!(visit_i32, i32);
    scalar!(visit_i64, i64);
    scalar!(visit_i128, i128);
    scalar!(visit_u8, u8);
    scalar!(visit_u16, u16);
    scalar!(visit_u32, u32);
    scalar!(visit_u64, u64);
    scalar!(visit_u128, u128);
    scalar!(visit_f32, f32);
    scalar!(visit_f64, f64);
    scalar!(visit_char, char);
    scalar!(visit_str, &str);
    scalar!(visit_borrowed_str, &'de str);
    scalar!(visit_string, String);
    scalar!(visit_bytes, &[u8]);
    scalar!(visit_borrowed_bytes, &'de [u8]);
    scalar!(visit_byte_buf, Vec<u8>);
    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_none()
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.inner.visit_unit()
    }
    fn visit_some<D: de::Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        self.inner.visit_some(Decoder {
            inner,
            remaining: self.remaining,
            depth: self.depth + 1,
        })
    }
    fn visit_newtype_struct<D: de::Deserializer<'de>>(
        self,
        inner: D,
    ) -> Result<Self::Value, D::Error> {
        self.inner.visit_newtype_struct(Decoder {
            inner,
            remaining: self.remaining,
            depth: self.depth + 1,
        })
    }
    fn visit_seq<A: SeqAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        check_hint(inner.size_hint(), self.remaining)?;
        self.inner.visit_seq(Access {
            inner,
            remaining: self.remaining,
            depth: self.depth,
        })
    }
    fn visit_map<A: MapAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        check_hint(inner.size_hint(), self.remaining)?;
        self.inner.visit_map(Access {
            inner,
            remaining: self.remaining,
            depth: self.depth,
        })
    }
    fn visit_enum<A: EnumAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_enum(Access {
            inner,
            remaining: self.remaining,
            depth: self.depth,
        })
    }
}

fn check_hint<E: de::Error>(hint: Option<usize>, remaining: &Cell<usize>) -> Result<(), E> {
    if hint.is_some_and(|size| size > remaining.get()) {
        return Err(E::custom("snapshot collection limit exceeded"));
    }
    Ok(())
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Access<'_, A> {
    type Error = A::Error;
    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        self.inner.next_element_seed(Seed {
            inner,
            remaining: self.remaining,
            depth: self.depth,
        })
    }
    // Do not trust the wire's allocation hint. Entries consume the shared budget.
    fn size_hint(&self) -> Option<usize> {
        None
    }
}
impl<'de, A: MapAccess<'de>> MapAccess<'de> for Access<'_, A> {
    type Error = A::Error;
    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        self.inner.next_key_seed(Seed {
            inner,
            remaining: self.remaining,
            depth: self.depth,
        })
    }
    fn next_value_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<S::Value, Self::Error> {
        self.inner.next_value_seed(Seed {
            inner,
            remaining: self.remaining,
            depth: self.depth,
        })
    }
    fn size_hint(&self) -> Option<usize> {
        None
    }
}
impl<'a, 'de, A: EnumAccess<'de>> EnumAccess<'de> for Access<'a, A> {
    type Error = A::Error;
    type Variant = Access<'a, A::Variant>;
    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        inner: S,
    ) -> Result<(S::Value, Self::Variant), Self::Error> {
        let (value, inner) = self.inner.variant_seed(Seed {
            inner,
            remaining: self.remaining,
            depth: self.depth,
        })?;
        Ok((
            value,
            Access {
                inner,
                remaining: self.remaining,
                depth: self.depth,
            },
        ))
    }
}
impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Access<'_, A> {
    type Error = A::Error;
    fn unit_variant(self) -> Result<(), Self::Error> {
        self.inner.unit_variant()
    }
    fn newtype_variant_seed<S: DeserializeSeed<'de>>(
        self,
        inner: S,
    ) -> Result<S::Value, Self::Error> {
        self.inner.newtype_variant_seed(Seed {
            inner,
            remaining: self.remaining,
            depth: self.depth,
        })
    }
    fn tuple_variant<V: Visitor<'de>>(self, len: usize, inner: V) -> Result<V::Value, Self::Error> {
        self.inner.tuple_variant(
            len,
            LimitVisitor {
                inner,
                remaining: self.remaining,
                depth: self.depth + 1,
            },
        )
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        inner: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.struct_variant(
            fields,
            LimitVisitor {
                inner,
                remaining: self.remaining,
                depth: self.depth + 1,
            },
        )
    }
}
