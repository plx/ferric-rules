//! Engine-scoped host values and transient fact handles.
//!
//! Core symbols and slot-map keys describe RETE state. These wrappers prevent
//! ordinary host input from silently treating another engine's keys as local.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ferric_rules_core::{Fact, FactBase, FactId, FerricString, Symbol, Value};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::{EngineError, StringEncoding};

/// Maximum host multifield nesting, shared with snapshot validation.
pub const HOST_VALUE_MAX_DEPTH: usize = 32;
/// Maximum values in one host assertion, including multifield elements.
pub const HOST_VALUE_MAX_ITEMS: usize = 1_000_000;

// Reserve a distinct opaque u64 namespace. This also exercises lossless host
// representations without millions of arena-generation churn operations.
static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1_u64 << 63);

fn identity() -> u64 {
    NEXT_IDENTITY
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .expect("host identity space exhausted")
}

/// An opaque, transient fact handle. Stable while that fact remains in one
/// engine; never a persisted application identity or a RETE slot-map key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FactHandle(u64);

impl FactHandle {
    /// Reconstitute the numeric representation used by embedding adapters.
    /// Unknown, foreign and stale numbers are rejected by engine lookups.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
    #[must_use]
    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

/// A cheap, copyable symbol bound to the engine that interned it. Reset keeps
/// symbols valid; clear and snapshot restoration establish a new owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymbolHandle {
    pub(crate) owner: u64,
    pub(crate) symbol: Symbol,
}

/// An owned host input. Primitives and strings are portable. Interned symbols,
/// including those nested in multifields, retain their engine ownership.
#[derive(Clone, Debug)]
pub struct HostValue {
    pub(crate) owner: Option<u64>,
    pub(crate) value: Value,
}

impl HostValue {
    /// Borrow the underlying value for low-level inspection. Raw core symbols
    /// extracted here are not accepted as ordinary assertion input.
    #[must_use]
    pub fn as_value(&self) -> &Value {
        &self.value
    }

    /// Build a multifield without erasing the ownership of its elements.
    ///
    /// # Errors
    /// Rejects mixed engine ownership and excessive nesting/element counts.
    pub fn multifield(values: Vec<Self>) -> Result<Self, EngineError> {
        let mut owner = None;
        let mut remaining = HOST_VALUE_MAX_ITEMS;
        for value in &values {
            // Validate before combining stamps: an unowned raw Symbol must
            // not gain provenance from an adjacent genuine owned value.
            value.validate(value.owner, StringEncoding::Utf8, &mut remaining)?;
            if let Some(origin) = value.owner {
                if owner.is_some_and(|prior| prior != origin) {
                    return Err(EngineError::ForeignHandle);
                }
                owner = Some(origin);
            }
        }
        let result = Self {
            owner,
            value: Value::Multifield(Box::new(values.into_iter().map(|v| v.value).collect())),
        };
        let mut remaining = HOST_VALUE_MAX_ITEMS;
        result.validate(owner, StringEncoding::Utf8, &mut remaining)?;
        Ok(result)
    }

    pub(crate) fn validate(
        &self,
        owner: Option<u64>,
        encoding: StringEncoding,
        remaining: &mut usize,
    ) -> Result<(), EngineError> {
        if self.owner.is_some() && self.owner != owner {
            return Err(EngineError::ForeignHandle);
        }
        let mut pending = SmallVec::<[(&Value, usize); 8]>::new();
        pending.push((&self.value, 0));
        while let Some((value, depth)) = pending.pop() {
            *remaining = remaining.checked_sub(1).ok_or_else(|| {
                EngineError::InvalidHostValue("too many values in one assertion".into())
            })?;
            match value {
                Value::Symbol(_) if self.owner.is_none() => return Err(EngineError::InvalidHostValue(
                    "raw core symbols have no host provenance; use engine.symbol_value or an owned fact value".into())),
                Value::Void => return Err(EngineError::InvalidHostValue(
                    "void cannot be stored in a fact, including inside a multifield".into())),
                Value::String(value) => {
                    let bytes = value.as_bytes();
                    if (matches!(value, FerricString::Ascii(_)) || encoding == StringEncoding::Ascii)
                        && !bytes.is_ascii() {
                        return Err(EngineError::InvalidHostValue("non-ASCII bytes in an ASCII string".into()));
                    }
                }
                Value::Multifield(values) => {
                    if depth >= HOST_VALUE_MAX_DEPTH {
                        return Err(EngineError::InvalidHostValue("multifield nesting exceeds 32".into()));
                    }
                    if values.len() > *remaining {
                        return Err(EngineError::InvalidHostValue("too many values in one assertion".into()));
                    }
                    pending.extend(values.iter().map(|value| (value, depth + 1)));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl From<SymbolHandle> for HostValue {
    fn from(symbol: SymbolHandle) -> Self {
        Self {
            owner: Some(symbol.owner),
            value: Value::Symbol(symbol.symbol),
        }
    }
}

impl From<Value> for HostValue {
    fn from(value: Value) -> Self {
        Self { owner: None, value }
    }
}

macro_rules! portable {
    ($($ty:ty),* $(,)?) => { $(
        impl From<$ty> for HostValue {
            fn from(value: $ty) -> Self { Value::from(value).into() }
        }
    )* };
}
portable!(i64, i32, f64, FerricString);

/// Collected host fields. Private contents prevent conversion traits from
/// manufacturing a provenance stamp.
pub struct HostFields(pub(crate) SmallVec<[HostValue; 8]>);

/// Accepted host field collections. Use `()` for an empty fact, a primitive or
/// `HostValue` for one field, and an array/vector for multiple fields.
pub trait IntoHostFields {
    fn into_host_fields(self) -> HostFields;
}
impl<T: Into<HostValue>> IntoHostFields for Vec<T> {
    fn into_host_fields(self) -> HostFields {
        HostFields(self.into_iter().map(Into::into).collect())
    }
}
impl<T: Into<HostValue>, const N: usize> IntoHostFields for [T; N] {
    fn into_host_fields(self) -> HostFields {
        HostFields(self.into_iter().map(Into::into).collect())
    }
}
impl IntoHostFields for () {
    fn into_host_fields(self) -> HostFields {
        HostFields(SmallVec::new())
    }
}
macro_rules! field {
    ($($ty:ty),* $(,)?) => { $(
        impl IntoHostFields for $ty {
            fn into_host_fields(self) -> HostFields { HostFields(std::iter::once(self.into()).collect()) }
        }
    )* };
}
field!(HostValue, SymbolHandle, Value, i64, i32, f64, FerricString);

/// An owned fact obtained from an engine, suitable for checked reassertion.
/// Cloning it retains the origin and template shape rather than exporting
/// unqualified arena keys as a new fact constructor.
#[derive(Clone, Debug)]
pub struct HostFact {
    pub(crate) owner: u64,
    pub(crate) fact: Fact,
    pub(crate) template: Option<Arc<crate::templates::RegisteredTemplate>>,
}
impl HostFact {
    /// Borrow the underlying representation for low-level inspection.
    #[must_use]
    pub fn as_fact(&self) -> &Fact {
        &self.fact
    }
    /// Clone an owned field/slot while preserving symbol provenance.
    /// Values without symbols remain portable, including nested multifields.
    #[must_use]
    pub fn value(&self, index: usize) -> Option<HostValue> {
        let values: &[Value] = match &self.fact {
            Fact::Ordered(fact) => &fact.fields,
            Fact::Template(fact) => &fact.slots,
        };
        values.get(index).map(|value| HostValue {
            owner: contains_symbol(value).then_some(self.owner),
            value: value.clone(),
        })
    }
}

fn contains_symbol(value: &Value) -> bool {
    let Value::Multifield(values) = value else {
        return matches!(value, Value::Symbol(_));
    };
    // Stored values already satisfy the engine's depth/item limits. Keep one
    // iterator per nesting level, rather than collecting every pending sibling.
    let mut parents = SmallVec::<[std::slice::Iter<'_, Value>; 4]>::new();
    let mut current = values.iter();
    loop {
        match current.next() {
            Some(Value::Symbol(_)) => return true,
            Some(Value::Multifield(values)) => {
                parents.push(current);
                current = values.iter();
            }
            Some(_) => {}
            None => match parents.pop() {
                Some(parent) => current = parent,
                None => return false,
            },
        }
    }
}

// Sparse indexes scale with exported identities; reading a high-index fact
// does not allocate for unexported arena slots. RHS removals are reclaimed
// by the existing bounded, amortized pruning policy.
#[derive(Default)]
struct FactHandles {
    by_handle: FxHashMap<FactHandle, FactId>,
    by_fact: FxHashMap<FactId, FactHandle>,
}

pub(crate) struct HostState {
    pub owner: u64,
    facts: Mutex<FactHandles>,
}
impl HostState {
    pub fn new() -> Self {
        Self {
            owner: identity(),
            facts: Mutex::default(),
        }
    }
    pub fn export(&self, fact: FactId) -> FactHandle {
        let mut handles = self.facts.lock().expect("host handle lock poisoned");
        if let Some(&handle) = handles.by_fact.get(&fact) {
            return handle;
        }
        let handle = FactHandle(identity());
        handles.by_fact.insert(fact, handle);
        handles.by_handle.insert(handle, fact);
        handle
    }
    pub fn resolve(&self, handle: FactHandle) -> Option<FactId> {
        self.facts
            .lock()
            .expect("host handle lock poisoned")
            .by_handle
            .get(&handle)
            .copied()
    }
    pub fn remove(&mut self, fact: FactId) {
        let handles = self.facts.get_mut().expect("host handle lock poisoned");
        if let Some(handle) = handles.by_fact.remove(&fact) {
            handles.by_handle.remove(&handle);
        }
    }
    pub fn clear_facts(&mut self) {
        *self.facts.get_mut().expect("host handle lock poisoned") = FactHandles::default();
    }
    /// Amortize cleanup of RHS retractions without walking live facts after
    /// every operation. Stale entries remain bounded by live facts plus 256.
    pub fn prune(&mut self, facts: &FactBase) {
        let handles = self.facts.get_mut().expect("host handle lock poisoned");
        if handles.by_fact.len() > facts.len().saturating_mul(2).saturating_add(256) {
            handles.by_fact.retain(|fact, _| facts.get(*fact).is_some());
            handles
                .by_handle
                .retain(|_, fact| facts.get(*fact).is_some());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Engine, RunLimit};

    #[test]
    fn rhs_retraction_reset_and_clear_reclaim_host_handle_storage() {
        let mut engine =
            Engine::with_rules("(defrule consume ?f <- (item ?id) => (retract ?f))").unwrap();
        engine.reset().unwrap();
        assert_eq!(engine.fact_count(), 0);
        assert!(engine.host.facts.lock().unwrap().by_fact.is_empty());
        for id in 0..512 {
            engine.assert_ordered("item", id).unwrap();
        }
        assert_eq!(engine.host.facts.lock().unwrap().by_fact.len(), 512);
        assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 512);
        assert!(engine.host.facts.lock().unwrap().by_fact.is_empty());
        assert!(engine.host.facts.lock().unwrap().by_handle.is_empty());
        for _ in 0..10 {
            engine.assert_ordered("item", 1_i64).unwrap();
            engine.reset().unwrap();
            assert!(engine.host.facts.lock().unwrap().by_fact.is_empty());
        }
        engine.assert_ordered("item", 2_i64).unwrap();
        engine.clear();
        assert!(engine.host.facts.lock().unwrap().by_handle.is_empty());
    }

    #[test]
    fn rhs_removal_bounds_storage_and_rejects_retired_identities() {
        let mut engine = Engine::with_rules(
            "(deftemplate changed (slot value))
             (defrule consume ?f <- (item ?id) => (retract ?f))
             (defrule change ?f <- (changed (value 0)) => (modify ?f (value 1)))",
        )
        .unwrap();
        engine.reset().unwrap();
        let stable = engine.assert_ordered("keep", 7_i64).unwrap();
        for value in 0..1024 {
            let retired = engine.assert_ordered("item", value).unwrap();
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert!(engine.get_fact(retired).unwrap().is_none());
            assert_eq!(engine.find_facts("keep").unwrap()[0].0, stable);
            let handles = engine.host.facts.lock().unwrap();
            // Pruning observes at most two live facts in this scenario.
            assert!(handles.by_fact.len() <= 260);
            assert_eq!(handles.by_handle.len(), handles.by_fact.len());
        }
        for _ in 0..256 {
            let retired = engine
                .assert_template_slots("changed", [("value", 0_i64)])
                .unwrap();
            assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
            assert!(engine.get_fact(retired).unwrap().is_none());
            let fresh = engine
                .facts()
                .unwrap()
                .find_map(|(id, fact)| {
                    matches!(fact, ferric_rules_core::Fact::Template(_)).then_some(id)
                })
                .unwrap();
            assert_ne!(fresh, retired);
            assert!(matches!(
                engine.get_fact_slot_by_name(fresh, "value"),
                Ok(ferric_rules_core::Value::Integer(1))
            ));
            assert_eq!(
                engine.facts().unwrap().find_map(|(id, fact)| {
                    matches!(fact, ferric_rules_core::Fact::Template(_)).then_some(id)
                }),
                Some(fresh)
            );
            engine.retract(fresh).unwrap();
            let handles = engine.host.facts.lock().unwrap();
            // The last pruning pass includes keep, changed, and the internal initial fact.
            assert!(handles.by_fact.len() <= 262);
            assert_eq!(handles.by_handle.len(), handles.by_fact.len());
        }
        engine.retract(stable).unwrap();
        engine.reset().unwrap();
        assert!(engine.host.facts.lock().unwrap().by_handle.is_empty());
        assert!(engine.host.facts.lock().unwrap().by_fact.is_empty());
    }

    #[test]
    fn concurrent_shared_exports_keep_one_identity_per_live_fact() {
        let mut engine = Engine::with_rules("(deffacts seeds (item 1))").unwrap();
        engine.reset().unwrap();
        // No host handle has been exported before threads start.
        assert!(engine.host.facts.lock().unwrap().by_handle.is_empty());
        let engine = &engine;
        std::thread::scope(|scope| {
            let readers: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(move || {
                        let first = engine.find_facts("item").unwrap()[0].0;
                        for _ in 0..100 {
                            assert_eq!(engine.find_facts("item").unwrap()[0].0, first);
                            assert!(engine.get_fact(first).unwrap().is_some());
                        }
                        first
                    })
                })
                .collect();
            let ids: Vec<_> = readers
                .into_iter()
                .map(|reader| reader.join().unwrap())
                .collect();
            assert!(ids.iter().all(|id| *id == ids[0]));
        });
        assert_eq!(engine.host.facts.lock().unwrap().by_handle.len(), 1);
        assert_eq!(engine.host.facts.lock().unwrap().by_fact.len(), 1);
    }
}
