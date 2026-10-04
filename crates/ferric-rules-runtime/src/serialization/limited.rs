//! Resource limits applied through Serde before collection allocation/recursion.

use serde::de::{self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor};
use std::cell::Cell;
use std::fmt;

pub(super) const MAX_DEPTH: usize = 128;
// Charge Serde operations, including keys and wrappers, independently of the
// actual encoded length. The complete input still has its separate byte cap.
pub(super) const MAX_STEPS: usize = super::MAX_SNAPSHOT_BYTES / 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LimitKind {
    Steps,
    Depth,
    Collection,
}

impl LimitKind {
    pub(super) const fn description(self) -> &'static str {
        match self {
            Self::Steps => "decoder step",
            Self::Depth => "decoder nesting",
            Self::Collection => "decoder collection length",
        }
    }
}

struct Budget {
    remaining: Cell<usize>,
    max_depth: usize,
    failure: Cell<Option<LimitKind>>,
}

impl Budget {
    fn fail<E: de::Error>(&self, kind: LimitKind) -> E {
        if self.failure.get().is_none() {
            self.failure.set(Some(kind));
        }
        E::custom(format_args!(
            "snapshot {} limit exceeded",
            kind.description()
        ))
    }

    fn check_depth<E: de::Error>(&self, depth: usize) -> Result<(), E> {
        if depth > self.max_depth {
            return Err(self.fail(LimitKind::Depth));
        }
        Ok(())
    }

    fn enter<E: de::Error>(&self, depth: usize) -> Result<(), E> {
        self.check_depth(depth)?;
        let Some(remaining) = self.remaining.get().checked_sub(1) else {
            return Err(self.fail(LimitKind::Steps));
        };
        self.remaining.set(remaining);
        Ok(())
    }

    fn check_hint<E: de::Error>(&self, hint: Option<usize>) -> Result<(), E> {
        if hint.is_some_and(|size| size > self.remaining.get()) {
            return Err(self.fail(LimitKind::Collection));
        }
        Ok(())
    }
}

/// Internal report carrier: a recorded limit must survive codec error erasure.
/// The caller must inspect this result before checking for trailing input, since
/// reaching a limit deliberately stops consuming the payload.
pub(super) struct Limited<T, const STEPS: usize = MAX_STEPS, const DEPTH: usize = MAX_DEPTH>(
    pub Result<T, LimitKind>,
);

impl<'de, T: serde::Deserialize<'de>, const STEPS: usize, const DEPTH: usize>
    serde::Deserialize<'de> for Limited<T, STEPS, DEPTH>
{
    fn deserialize<D: de::Deserializer<'de>>(inner: D) -> Result<Self, D::Error> {
        let budget = Budget {
            remaining: Cell::new(STEPS),
            max_depth: DEPTH,
            failure: Cell::new(None),
        };
        let result = T::deserialize(Decoder {
            inner,
            budget: &budget,
            depth: 0,
        });
        match budget.failure.get() {
            Some(kind) => Ok(Self(Err(kind))),
            None => result.map(|value| Self(Ok(value))),
        }
    }
}

struct Decoder<'a, D> {
    inner: D,
    budget: &'a Budget,
    depth: usize,
}
struct LimitVisitor<'a, V> {
    inner: V,
    budget: &'a Budget,
    depth: usize,
}
struct Seed<'a, S> {
    inner: S,
    budget: &'a Budget,
    depth: usize,
}
struct Access<'a, A> {
    inner: A,
    budget: &'a Budget,
    depth: usize,
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<'_, S> {
    type Value = S::Value;
    fn deserialize<D: de::Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        self.inner.deserialize(Decoder {
            inner,
            budget: self.budget,
            depth: self.depth + 1,
        })
    }
}

macro_rules! forward {
    ($name:ident $(, $arg:ident: $ty:ty)*) => {
        fn $name<V: Visitor<'de>>(self, $($arg: $ty,)* visitor: V) -> Result<V::Value, Self::Error> {
            self.budget.enter(self.depth)?;
            self.inner.$name($($arg,)* LimitVisitor { inner: visitor, budget: self.budget, depth: self.depth })
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
    // Native JSON ignored_any skips entire subtrees outside these wrappers.
    // Traversing through any keeps ignored descendants under the same guards.
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_any(visitor)
    }
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
            budget: self.budget,
            depth: self.depth + 1,
        })
    }
    fn visit_newtype_struct<D: de::Deserializer<'de>>(
        self,
        inner: D,
    ) -> Result<Self::Value, D::Error> {
        self.inner.visit_newtype_struct(Decoder {
            inner,
            budget: self.budget,
            depth: self.depth + 1,
        })
    }
    fn visit_seq<A: SeqAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.budget.check_hint(inner.size_hint())?;
        self.inner.visit_seq(Access {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }
    fn visit_map<A: MapAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.budget.check_hint(inner.size_hint())?;
        self.inner.visit_map(Access {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }
    fn visit_enum<A: EnumAccess<'de>>(self, inner: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_enum(Access {
            inner,
            budget: self.budget,
            depth: self.depth,
        })
    }
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Access<'_, A> {
    type Error = A::Error;
    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        self.inner.next_element_seed(Seed {
            inner,
            budget: self.budget,
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
            budget: self.budget,
            depth: self.depth,
        })
    }
    fn next_value_seed<S: DeserializeSeed<'de>>(
        &mut self,
        inner: S,
    ) -> Result<S::Value, Self::Error> {
        self.inner.next_value_seed(Seed {
            inner,
            budget: self.budget,
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
            budget: self.budget,
            depth: self.depth,
        })?;
        Ok((
            value,
            Access {
                inner,
                budget: self.budget,
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
            budget: self.budget,
            depth: self.depth,
        })
    }
    fn tuple_variant<V: Visitor<'de>>(self, len: usize, inner: V) -> Result<V::Value, Self::Error> {
        self.budget.check_depth(self.depth + 1)?;
        self.inner.tuple_variant(
            len,
            LimitVisitor {
                inner,
                budget: self.budget,
                depth: self.depth + 1,
            },
        )
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        inner: V,
    ) -> Result<V::Value, Self::Error> {
        self.budget.check_depth(self.depth + 1)?;
        self.inner.struct_variant(
            fields,
            LimitVisitor {
                inner,
                budget: self.budget,
                depth: self.depth + 1,
            },
        )
    }
}

#[cfg(test)]
mod tests;
