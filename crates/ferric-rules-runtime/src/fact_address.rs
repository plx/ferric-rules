//! Engine-owned fact identities and their stable public spelling.

use ferric_rules_core::{FactAddress, FactBase, FactId};

/// Capture an address while its fact is live. Retained query members keep this
/// metadata after retraction rather than reconstructing it from a reused slot.
pub(crate) fn make_fact_address(
    facts: &FactBase,
    initial_fact_id: Option<FactId>,
    epoch: u64,
    zero_based: bool,
    fact_id: FactId,
) -> Option<FactAddress> {
    let entry = facts.get(fact_id)?;
    let index = public_fact_index(facts, initial_fact_id, zero_based, fact_id)?;
    Some(FactAddress::new(fact_id, epoch, entry.timestamp, index))
}

/// Resolve a typed address only within the lifecycle and assertion that made it.
pub(crate) fn live_fact_id(facts: &FactBase, epoch: u64, address: &FactAddress) -> Option<FactId> {
    if address.epoch()? != epoch {
        return None;
    }
    let fact_id = address.fact_id()?;
    let entry = facts.get(fact_id)?;
    (Some(entry.timestamp) == address.timestamp()).then_some(fact_id)
}

/// CLIPS reserves index zero for `initial-fact`; application facts start at one.
/// Host-created facts can precede the protected fact in assertion chronology.
pub(crate) fn public_fact_index(
    facts: &FactBase,
    initial_fact_id: Option<FactId>,
    zero_based: bool,
    fact_id: FactId,
) -> Option<u64> {
    let entry = facts.get(fact_id)?;
    if initial_fact_id == Some(fact_id) {
        return Some(0);
    }
    let initial_precedes = initial_fact_id
        .and_then(|id| facts.get(id))
        .is_some_and(|initial| initial.timestamp < entry.timestamp);
    if initial_precedes || zero_based {
        Some(entry.timestamp.get())
    } else {
        entry.timestamp.get().checked_add(1)
    }
}
