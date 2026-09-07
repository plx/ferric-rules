//! Engine-scoped host values and transient fact handles.
//!
//! Core symbols and slot-map keys describe RETE state. These wrappers prevent
//! ordinary host input from silently treating another engine's keys as local.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

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
        // Keep only the unvisited sibling slices on the stack. Primitive
        // fields need no allocation, and a wide multifield does not allocate
        // one temporary entry per element. Stack length is bounded by depth.
        let mut pending = SmallVec::<[(&[Value], usize); 8]>::new();
        let mut values = std::slice::from_ref(&self.value);
        let mut depth = 0;
        loop {
            let Some((value, siblings)) = values.split_first() else {
                let Some((next, next_depth)) = pending.pop() else {
                    break;
                };
                values = next;
                depth = next_depth;
                continue;
            };
            values = siblings;
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
                Value::Multifield(children) => {
                    if depth >= HOST_VALUE_MAX_DEPTH {
                        return Err(EngineError::InvalidHostValue("multifield nesting exceeds 32".into()));
                    }
                    if children.len() > *remaining {
                        return Err(EngineError::InvalidHostValue("too many values in one assertion".into()));
                    }
                    if !siblings.is_empty() {
                        pending.push((siblings, depth));
                    }
                    values = children;
                    depth += 1;
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
    pub(crate) template: Option<crate::templates::RegisteredTemplate>,
}
impl HostFact {
    /// Borrow the underlying representation for low-level inspection.
    #[must_use]
    pub fn as_fact(&self) -> &Fact {
        &self.fact
    }
    /// Clone an owned field/slot while preserving symbol provenance.
    #[must_use]
    pub fn value(&self, index: usize) -> Option<HostValue> {
        let values: &[Value] = match &self.fact {
            Fact::Ordered(fact) => &fact.fields,
            Fact::Template(fact) => &fact.slots,
        };
        values.get(index).map(|value| HostValue {
            owner: Some(self.owner),
            value: value.clone(),
        })
    }
}

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
        Self::export_locked(&mut handles, fact)
    }
    fn export_locked(handles: &mut FactHandles, fact: FactId) -> FactHandle {
        if let Some(&handle) = handles.by_fact.get(&fact) {
            return handle;
        }
        let handle = FactHandle(identity());
        handles.by_fact.insert(fact, handle);
        handles.by_handle.insert(handle, fact);
        handle
    }
    /// An assertion already needs the map to export its result. Reclaim stale
    /// RHS handles under that same lock, retaining the ordinary prune policy.
    pub fn export_after_prune(&self, fact: FactId, facts: &FactBase) -> FactHandle {
        let mut handles = self.facts.lock().expect("host handle lock poisoned");
        Self::prune_locked(&mut handles, facts);
        Self::export_locked(&mut handles, fact)
    }
    pub fn resolve(&self, handle: FactHandle) -> Option<FactId> {
        self.facts
            .lock()
            .expect("host handle lock poisoned")
            .by_handle
            .get(&handle)
            .copied()
    }
    pub fn remove(&self, fact: FactId) {
        let mut handles = self.facts.lock().expect("host handle lock poisoned");
        if let Some(handle) = handles.by_fact.remove(&fact) {
            handles.by_handle.remove(&handle);
        }
    }
    pub fn clear_facts(&self) {
        *self.facts.lock().expect("host handle lock poisoned") = FactHandles::default();
    }
    /// Amortize cleanup of RHS retractions without walking live facts after
    /// every operation. Stale entries remain bounded by live facts plus 256.
    pub fn prune(&self, facts: &FactBase) {
        let mut handles = self.facts.lock().expect("host handle lock poisoned");
        Self::prune_locked(&mut handles, facts);
    }
    fn prune_locked(handles: &mut FactHandles, facts: &FactBase) {
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
}
