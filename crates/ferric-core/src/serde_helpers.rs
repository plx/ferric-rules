//! Serde helpers for types without native serde support.
//!
//! Provides `serialize` / `deserialize` function pairs for use with
//! `#[serde(with = "...")]` on fields whose types lack `Serialize`/`Deserialize`
//! impls (e.g. `FxHashMap`, `FxHashSet` from `rustc-hash`).

/// Serialize/deserialize `FxHashMap<K, V>` as `Vec<(K, V)>`.
pub mod fx_hash_map {
    use rustc_hash::FxHashMap;
    use serde::de::Deserializer;
    use serde::ser::Serializer;
    use serde::{Deserialize, Serialize};

    #[allow(clippy::implicit_hasher)]
    pub fn serialize<K, V, S>(map: &FxHashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        K: Serialize,
        V: Serialize,
        S: Serializer,
    {
        let entries: Vec<(&K, &V)> = map.iter().collect();
        entries.serialize(serializer)
    }

    pub fn deserialize<'de, K, V, D>(deserializer: D) -> Result<FxHashMap<K, V>, D::Error>
    where
        K: Deserialize<'de> + Eq + std::hash::Hash,
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        let entries: Vec<(K, V)> = Vec::deserialize(deserializer)?;
        Ok(entries.into_iter().collect())
    }
}

/// Serialize/deserialize `FxHashSet<T>` as `Vec<T>`.
pub mod fx_hash_set {
    use rustc_hash::FxHashSet;
    use serde::de::Deserializer;
    use serde::ser::Serializer;
    use serde::{Deserialize, Serialize};

    #[allow(clippy::implicit_hasher)]
    pub fn serialize<T, S>(set: &FxHashSet<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: Serialize,
        S: Serializer,
    {
        let items: Vec<&T> = set.iter().collect();
        items.serialize(serializer)
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<FxHashSet<T>, D::Error>
    where
        T: Deserialize<'de> + Eq + std::hash::Hash,
        D: Deserializer<'de>,
    {
        let items: Vec<T> = Vec::deserialize(deserializer)?;
        Ok(items.into_iter().collect())
    }
}

/// Serialize/deserialize `FxHashMap<K, FxHashSet<V>>` as `Vec<(K, Vec<V>)>`.
pub mod fx_hash_map_of_fx_hash_set {
    use rustc_hash::{FxHashMap, FxHashSet};
    use serde::de::Deserializer;
    use serde::ser::Serializer;
    use serde::{Deserialize, Serialize};

    #[allow(clippy::implicit_hasher)]
    pub fn serialize<K, V, S>(
        map: &FxHashMap<K, FxHashSet<V>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        K: Serialize,
        V: Serialize,
        S: Serializer,
    {
        let entries: Vec<(&K, Vec<&V>)> =
            map.iter().map(|(k, vs)| (k, vs.iter().collect())).collect();
        entries.serialize(serializer)
    }

    #[allow(clippy::implicit_hasher)]
    pub fn deserialize<'de, K, V, D>(
        deserializer: D,
    ) -> Result<FxHashMap<K, FxHashSet<V>>, D::Error>
    where
        K: Deserialize<'de> + Eq + std::hash::Hash,
        V: Deserialize<'de> + Eq + std::hash::Hash,
        D: Deserializer<'de>,
    {
        let entries: Vec<(K, Vec<V>)> = Vec::deserialize(deserializer)?;
        Ok(entries
            .into_iter()
            .map(|(k, vs)| (k, vs.into_iter().collect()))
            .collect())
    }
}

/// Serialize/deserialize `FxHashMap<K, FxHashMap<K2, V>>` as `Vec<(K, Vec<(K2, V)>)>`.
pub mod fx_hash_map_of_fx_hash_map {
    use rustc_hash::FxHashMap;
    use serde::de::Deserializer;
    use serde::ser::Serializer;
    use serde::{Deserialize, Serialize};

    #[allow(clippy::implicit_hasher, clippy::type_complexity)]
    pub fn serialize<K, K2, V, S>(
        map: &FxHashMap<K, FxHashMap<K2, V>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        K: Serialize,
        K2: Serialize,
        V: Serialize,
        S: Serializer,
    {
        let entries: Vec<(&K, Vec<(&K2, &V)>)> = map
            .iter()
            .map(|(k, inner)| (k, inner.iter().collect()))
            .collect();
        entries.serialize(serializer)
    }

    #[allow(clippy::implicit_hasher, clippy::type_complexity)]
    pub fn deserialize<'de, K, K2, V, D>(
        deserializer: D,
    ) -> Result<FxHashMap<K, FxHashMap<K2, V>>, D::Error>
    where
        K: Deserialize<'de> + Eq + std::hash::Hash,
        K2: Deserialize<'de> + Eq + std::hash::Hash,
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        let entries: Vec<(K, Vec<(K2, V)>)> = Vec::deserialize(deserializer)?;
        Ok(entries
            .into_iter()
            .map(|(k, inner)| (k, inner.into_iter().collect()))
            .collect())
    }
}

/// Serialize/deserialize `FxHashMap<K, FxHashMap<K2, FxHashSet<V>>>` as
/// `Vec<(K, Vec<(K2, Vec<V>)>)>`.
pub mod fx_hash_map_of_fx_hash_map_of_fx_hash_set {
    use rustc_hash::{FxHashMap, FxHashSet};
    use serde::de::Deserializer;
    use serde::ser::Serializer;
    use serde::{Deserialize, Serialize};

    #[allow(clippy::implicit_hasher, clippy::type_complexity)]
    pub fn serialize<K, K2, V, S>(
        map: &FxHashMap<K, FxHashMap<K2, FxHashSet<V>>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        K: Serialize,
        K2: Serialize,
        V: Serialize,
        S: Serializer,
    {
        let entries: Vec<(&K, Vec<(&K2, Vec<&V>)>)> = map
            .iter()
            .map(|(k, inner)| {
                (
                    k,
                    inner
                        .iter()
                        .map(|(k2, vs)| (k2, vs.iter().collect()))
                        .collect(),
                )
            })
            .collect();
        entries.serialize(serializer)
    }

    #[allow(clippy::implicit_hasher, clippy::type_complexity)]
    pub fn deserialize<'de, K, K2, V, D>(
        deserializer: D,
    ) -> Result<FxHashMap<K, FxHashMap<K2, FxHashSet<V>>>, D::Error>
    where
        K: Deserialize<'de> + Eq + std::hash::Hash,
        K2: Deserialize<'de> + Eq + std::hash::Hash,
        V: Deserialize<'de> + Eq + std::hash::Hash,
        D: Deserializer<'de>,
    {
        let entries: Vec<(K, Vec<(K2, Vec<V>)>)> = Vec::deserialize(deserializer)?;
        Ok(entries
            .into_iter()
            .map(|(k, inner)| {
                (
                    k,
                    inner
                        .into_iter()
                        .map(|(k2, vs)| (k2, vs.into_iter().collect()))
                        .collect(),
                )
            })
            .collect())
    }
}

/// Serialize/deserialize `std::collections::HashSet<T>` as `Vec<T>`.
///
/// std `HashSet` has native serde support, but this helper is provided
/// for cases where it's used alongside `FxHash` types and consistency is desired.
pub mod std_hash_set {
    use serde::de::Deserializer;
    use serde::ser::Serializer;
    use serde::{Deserialize, Serialize};
    use std::collections::HashSet;

    #[allow(clippy::implicit_hasher)]
    pub fn serialize<T, S>(set: &HashSet<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: Serialize,
        S: Serializer,
    {
        let items: Vec<&T> = set.iter().collect();
        items.serialize(serializer)
    }

    #[allow(clippy::implicit_hasher)]
    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<HashSet<T>, D::Error>
    where
        T: Deserialize<'de> + Eq + std::hash::Hash,
        D: Deserializer<'de>,
    {
        let items: Vec<T> = Vec::deserialize(deserializer)?;
        Ok(items.into_iter().collect())
    }
}
