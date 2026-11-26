//! Kani proof harnesses for `snapshot` (see docs/verification.md).
//!
//! What is proved, on the production code (expected results are written
//! here from the documented semantics: the runtime's public fact numbering,
//! "resource limits remain distinct from malformed graph or value
//! metadata", and the checks' messages):
//!
//! * `work_spend_is_checked_subtraction`: for every budget and cost,
//!   `Work::spend` succeeds exactly when the cost fits and then subtracts it;
//!   otherwise it reports the typed "validation work" limit and leaves the
//!   budget unchanged (no underflow); `step` spends exactly one.
//! * `snapshot_limit_is_exact`: `snapshot_limit` passes exactly when allowed
//!   and otherwise reports a `LimitExceeded` error.
//! * `fact_address_validation_never_panics`: for arbitrary address metadata
//!   (any key, epoch, timestamp, public index, or the dummy address),
//!   arbitrary current epoch, initial-fact choice and numbering mode, over a
//!   fact base of two facts with arbitrary timestamps and counter,
//!   `FactBase::validate_snapshot_fact_address_checked` never panics or
//!   overflows, and every rejection is `InvalidState` (never a limit).
//! * `current_epoch_address_accepts_exactly_the_assigned_index`: in a fact
//!   base satisfying its own invariants (distinct timestamps below the
//!   counter), an address of the current epoch for a live fact carrying that
//!   fact's timestamp is accepted exactly when its public index is the one
//!   the runtime assigns (0 for the initial fact; the timestamp when
//!   numbering is zero-based or the initial fact came first; otherwise the
//!   timestamp plus one), for every timestamp, index, epoch and mode. So
//!   every address captured during normal operation is accepted, and no
//!   other index is.
//! * `current_epoch_address_of_retracted_fact_accepts_exactly_the_assigned_index`:
//!   the same for an address whose fact has since been retracted (its key no
//!   longer resolves; retained query members keep such addresses): accepted
//!   exactly when its timestamp is below the counter and its index is the
//!   one the runtime assigned, for every timestamp, counter, index, epoch and
//!   mode (the protected initial fact stays live within an epoch).
//! * `older_epoch_addresses_are_checked_by_shape`: an address from an older
//!   epoch is accepted exactly when its key is not null, its timestamp is
//!   below `u64::MAX`, and its index is 0, the timestamp, or the timestamp
//!   plus one.
//! * `future_epoch_addresses_are_rejected`: always `InvalidState`.
//! * `index_key_metadata_check_is_exact`: for two keys, each a non-address
//!   atom, the dummy address, or a real address with arbitrary identity and
//!   metadata, `validate_index_key_metadata` fails exactly when both are fact
//!   addresses whose public spellings differ (two real addresses with
//!   different indexes, or the dummy against a real address), and then as
//!   `InvalidState`.
//! * `symbol_value_is_accepted_exactly_when_it_resolves`: over a symbol
//!   table holding one ASCII symbol, `validate_snapshot_value_checked`
//!   accepts a symbol value exactly when its id (any ASCII or UTF-8 index)
//!   resolves, and rejects it as `InvalidState` otherwise.
//! * `external_address_value_is_rejected`: external addresses are rejected
//!   as `InvalidState` (unsupported identity); integers are accepted.
//! * `fact_address_constant_is_rejected`: `validate_constant` rejects an
//!   equality test against any fact-address constant as `InvalidState` (the
//!   six single-constant operators share that match arm; the separate
//!   `EqualAny` arm is not covered, see below).
//!
//! Bounds: two facts in the fact base, one interned symbol, one value or
//! constant test per call; address metadata, symbol ids, integers, budgets
//! and costs are fully symbolic. The only assumptions are the fact base's
//! own invariants (distinct timestamps below the counter, checked by
//! `FactBase::validate_snapshot_checked`, snapshot.rs:244-251) and the
//! epoch ordering that names each harness's case.
//!
//! Not covered by Kani, with the reason:
//!
//! * `ReteNetwork::validate_snapshot_checked`, `validate_ncc_paths`,
//!   `alpha_memory_entry_types` and `ReteCompiler::validate_snapshot_checked`
//!   need a `ReteNetwork`, which CBMC cannot build here (measured by the
//!   beta and rete end-to-end proof modules: `BetaNetwork::new`'s single
//!   node-map insertion ran out of memory after 318 s, and `ReteNetwork::new`
//!   reaches `RandomState`'s `getrandom` through the std `HashSet` of
//!   disabled rules and ran out of memory after about 420 s). The alpha-cache
//!   and join-cache parts of `ReteCompiler::validate_snapshot_checked` are
//!   modeled in verification/verus/core_compiler.rs and proved to accept
//!   every state produced by `ensure_alpha_path` and `ensure_join_node`
//!   (work budget and rule-ID checks omitted there).
//! * Nested values and nested disjunctions (`validate_snapshot_value_checked`
//!   on multifields, the 32-level depth limit, nested `Any` in
//!   `validate_constant`) and `constant_test_cost`: their worklists read
//!   child pointers back from heap buffers, which CBMC does not track
//!   precisely (see compiler/kani_proofs.rs). Measured: `constant_test_cost`
//!   on single `EqualAny` tests of 0-3 keys plus one empty disjunction did
//!   not finish in 600 s (its `Any` and `Sequence` arms dereference child
//!   vectors through the imprecise pointer); a harness checking five atom
//!   values in a row ran out of memory after 136 s, and four source
//!   constants in a row after 458 s. Single calls that reach the
//!   multifield arm with string or nested data are also out of reach: an
//!   ASCII-string value ran out of memory after 95 s, a symbol constant
//!   after 442 s, and an external-address constant plus a field-count test
//!   after 510 s; a sweep of all seven constant-carrying operators (a
//!   fact-address and an integer constant each, 14 calls) after 568 s; and
//!   a single `EqualAny` holding one fact-address alternative after 562 s
//!   (two alternatives: 522 s). The
//!   depth limit is covered by the unit test
//!   `snapshot_limits_remain_distinct_from_invalid_symbols_and_instances`.
//! * `SymbolTable::validate_snapshot_checked`, `VarMap::validate_snapshot_checked`,
//!   `FactBase::validate_snapshot_checked` and `same_members` hash keys into
//!   `FxHashMap`/`FxHashSet` tables, which are measured as very expensive
//!   under Kani; they are not attempted here.

use super::*;
use crate::alpha::{ConstantTest, SlotIndex};
use crate::fact::{FactEntry, FactId, OrderedFact, Timestamp};
use crate::symbol::{Symbol, SymbolId};
use crate::value::{ExternalAddress, ExternalTypeId, FactAddress};

fn fact_id(raw: u64) -> FactId {
    FactId::from(slotmap::KeyData::from_ffi(raw))
}

/// Insert a fact directly into the slot map, at a chosen timestamp, without
/// touching the derived indexes (which `validate_snapshot_fact_address`
/// does not read).
fn insert_fact(facts: &mut FactBase, timestamp: u64) -> FactId {
    facts.facts.insert_with_key(|id| FactEntry {
        fact: Fact::Ordered(OrderedFact {
            relation: Symbol(SymbolId::Ascii(0)),
            fields: smallvec::SmallVec::new(),
        }),
        id,
        timestamp: Timestamp::new(timestamp),
    })
}

/// Reference for the public index of a live fact, written from the runtime's
/// documented rule (runtime `fact_address.rs`: "CLIPS reserves index zero
/// for `initial-fact`; application facts start at one. Host-created facts can
/// precede the protected fact in assertion chronology."): the initial fact is
/// 0; with zero-based numbering, or when the initial fact was asserted before
/// it, a fact's index is its timestamp; otherwise it is its timestamp plus one.
fn reference_public_index(
    is_initial: bool,
    initial_precedes: bool,
    zero_based: bool,
    ts: u64,
) -> u64 {
    if is_initial {
        0
    } else if zero_based || initial_precedes {
        ts
    } else {
        ts + 1
    }
}

fn is_invalid(result: &Result<(), SnapshotValidationError>) -> bool {
    matches!(result, Err(SnapshotValidationError::InvalidState(_)))
}

/// A resource-limit error naming `limit` (identified by length, which avoids
/// a string comparison loop; the names used here differ in length).
fn is_limit(result: &Result<(), SnapshotValidationError>, limit: &str) -> bool {
    matches!(result, Err(SnapshotValidationError::LimitExceeded(name)) if name.len() == limit.len())
}

// ---------------------------------------------------------------------------
// Work budget and limits
// ---------------------------------------------------------------------------

#[kani::proof]
#[kani::unwind(3)]
fn work_spend_is_checked_subtraction() {
    let budget: usize = kani::any();
    let count: usize = kani::any();
    let mut work = Work(budget);
    let result = work.spend(count);
    if count <= budget {
        assert!(result.is_ok());
        assert_eq!(work.0, budget - count);
    } else {
        // Exhaustion is a typed resource limit and leaves the budget intact.
        assert!(is_limit(&result, "validation work"));
        assert_eq!(work.0, budget);
    }
    let mut stepped = Work(budget);
    let step = stepped.step();
    assert_eq!(step.is_ok(), budget >= 1);
    assert_eq!(stepped.0, if budget >= 1 { budget - 1 } else { 0 });
    std::mem::forget(result);
    std::mem::forget(step);
}

#[kani::proof]
#[kani::unwind(3)]
fn snapshot_limit_is_exact() {
    let allowed: bool = kani::any();
    let result = snapshot_limit(allowed, "65,536 rule variables");
    assert_eq!(result.is_ok(), allowed);
    if !allowed {
        assert!(is_limit(&result, "65,536 rule variables"));
    }
    std::mem::forget(result);
}

// ---------------------------------------------------------------------------
// Fact addresses
// ---------------------------------------------------------------------------

/// Arbitrary metadata (address fields, epoch, initial fact, numbering) over a
/// fact base of two facts with arbitrary timestamps and counter: validation
/// returns a result and never panics or overflows.
#[kani::proof]
#[kani::unwind(4)]
fn fact_address_validation_never_panics() {
    let mut facts = FactBase::new();
    let first = insert_fact(&mut facts, kani::any());
    let second = insert_fact(&mut facts, kani::any());
    facts.next_timestamp = Timestamp::new(kani::any());
    let id = match kani::any::<u8>() {
        0 => first,
        1 => second,
        _ => fact_id(kani::any()),
    };
    let address = if kani::any() {
        FactAddress::dummy()
    } else {
        FactAddress::new(id, kani::any(), Timestamp::new(kani::any()), kani::any())
    };
    let initial = match kani::any::<u8>() {
        0 => None,
        1 => Some(first),
        2 => Some(second),
        _ => Some(fact_id(kani::any())),
    };
    let result =
        facts.validate_snapshot_fact_address_checked(&address, kani::any(), initial, kani::any());
    // Never a resource limit: every rejection is malformed state.
    assert!(result.is_ok() || is_invalid(&result));
    kani::cover!(
        result.is_ok() && !address.is_dummy(),
        "real address accepted"
    );
    kani::cover!(result.is_err(), "rejected");
    std::mem::forget(result);
    std::mem::forget(address);
    std::mem::forget(facts);
}

/// In the current epoch, an address of a live fact that carries the fact's
/// own timestamp is accepted exactly when its public index is the one the
/// runtime assigns; in particular every address captured during normal
/// operation is accepted.
#[kani::proof]
#[kani::unwind(4)]
fn current_epoch_address_accepts_exactly_the_assigned_index() {
    let initial_ts: u64 = kani::any();
    let target_ts: u64 = kani::any();
    let next: u64 = kani::any();
    // FactBase invariants (checked by `FactBase::validate_snapshot_checked`,
    // snapshot.rs:244-251): distinct timestamps below the counter.
    kani::assume(initial_ts != target_ts && initial_ts < next && target_ts < next);
    let mut facts = FactBase::new();
    let initial = insert_fact(&mut facts, initial_ts);
    let target = insert_fact(&mut facts, target_ts);
    facts.next_timestamp = Timestamp::new(next);
    let has_initial: bool = kani::any();
    let address_is_initial: bool = kani::any();
    kani::assume(!address_is_initial || has_initial);
    let zero_based: bool = kani::any();
    let (id, ts) = if address_is_initial {
        (initial, initial_ts)
    } else {
        (target, target_ts)
    };
    let initial_precedes = has_initial && initial_ts < ts;
    let expected = reference_public_index(address_is_initial, initial_precedes, zero_based, ts);
    let index: u64 = kani::any();
    let epoch: u64 = kani::any();
    let address = FactAddress::new(id, epoch, Timestamp::new(ts), index);
    let initial_id = if has_initial { Some(initial) } else { None };
    let result =
        facts.validate_snapshot_fact_address_checked(&address, epoch, initial_id, zero_based);
    assert_eq!(result.is_ok(), index == expected);
    kani::cover!(
        result.is_ok() && address_is_initial,
        "initial fact accepted"
    );
    kani::cover!(
        result.is_ok() && !address_is_initial && initial_precedes,
        "after initial fact"
    );
    kani::cover!(
        result.is_ok() && !has_initial && !zero_based,
        "one-based numbering"
    );
    kani::cover!(result.is_err() && index == ts, "off by one rejected");
    std::mem::forget(result);
    std::mem::forget(address);
    std::mem::forget(facts);
}

/// Addresses outlive their facts: a retained query member keeps the address
/// captured while its fact was live (runtime `fact_address.rs`), and the
/// protected initial fact cannot be retracted within an epoch (runtime
/// `engine.rs` refuses to retract it). So in the current epoch an address of
/// a retracted fact is accepted exactly when its timestamp is below the
/// counter and its public index is the one the runtime assigned.
#[kani::proof]
#[kani::unwind(4)]
fn current_epoch_address_of_retracted_fact_accepts_exactly_the_assigned_index() {
    let initial_ts: u64 = kani::any();
    let target_ts: u64 = kani::any();
    let next: u64 = kani::any();
    // FactBase invariants for the live initial fact (snapshot.rs:244-251),
    // and the target's timestamp came from the same counter.
    kani::assume(initial_ts != target_ts && initial_ts < next);
    let mut facts = FactBase::new();
    let initial = insert_fact(&mut facts, initial_ts);
    let target = insert_fact(&mut facts, target_ts);
    // Retract the target: its key no longer resolves.
    let removed = facts.facts.remove(target);
    std::mem::forget(removed);
    facts.next_timestamp = Timestamp::new(next);
    let has_initial: bool = kani::any();
    let zero_based: bool = kani::any();
    let initial_precedes = has_initial && initial_ts < target_ts;
    let index: u64 = kani::any();
    let epoch: u64 = kani::any();
    let address = FactAddress::new(target, epoch, Timestamp::new(target_ts), index);
    let initial_id = if has_initial { Some(initial) } else { None };
    let result =
        facts.validate_snapshot_fact_address_checked(&address, epoch, initial_id, zero_based);
    let expected_ok = target_ts < next
        && index == reference_public_index(false, initial_precedes, zero_based, target_ts);
    assert_eq!(result.is_ok(), expected_ok);
    assert!(result.is_ok() || is_invalid(&result));
    kani::cover!(result.is_ok() && initial_precedes, "after initial fact");
    kani::cover!(
        result.is_ok() && !zero_based && !initial_precedes,
        "one-based numbering"
    );
    kani::cover!(
        result.is_err() && target_ts >= next,
        "future timestamp rejected"
    );
    std::mem::forget(result);
    std::mem::forget(address);
    std::mem::forget(facts);
}

/// Addresses from older epochs are checked by shape only: a non-null key, a
/// representable successor timestamp, and an index that some numbering could
/// have produced (0, the timestamp, or the timestamp plus one).
#[kani::proof]
#[kani::unwind(4)]
fn older_epoch_addresses_are_checked_by_shape() {
    let mut facts = FactBase::new();
    let live = insert_fact(&mut facts, kani::any());
    facts.next_timestamp = Timestamp::new(kani::any());
    let id = if kani::any() {
        live
    } else {
        fact_id(kani::any())
    };
    let address_epoch: u64 = kani::any();
    let epoch: u64 = kani::any();
    kani::assume(address_epoch < epoch);
    let ts: u64 = kani::any();
    let index: u64 = kani::any();
    let address = FactAddress::new(id, address_epoch, Timestamp::new(ts), index);
    let initial = if kani::any() { Some(live) } else { None };
    let result =
        facts.validate_snapshot_fact_address_checked(&address, epoch, initial, kani::any());
    let shape_ok = !slotmap::Key::is_null(&id)
        && ts < u64::MAX
        && (index == 0 || index == ts || index == ts + 1);
    assert_eq!(result.is_ok(), shape_ok);
    kani::cover!(
        result.is_ok() && index == ts + 1,
        "one-based index accepted"
    );
    kani::cover!(
        result.is_err() && ts == u64::MAX,
        "exhausted timestamp rejected"
    );
    std::mem::forget(result);
    std::mem::forget(address);
    std::mem::forget(facts);
}

/// Addresses from future epochs are always rejected.
#[kani::proof]
#[kani::unwind(4)]
fn future_epoch_addresses_are_rejected() {
    let facts = FactBase::new();
    let address_epoch: u64 = kani::any();
    let epoch: u64 = kani::any();
    kani::assume(address_epoch > epoch);
    let address = FactAddress::new(
        fact_id(kani::any()),
        address_epoch,
        Timestamp::new(kani::any()),
        kani::any(),
    );
    let result = facts.validate_snapshot_fact_address_checked(&address, epoch, None, kani::any());
    assert!(is_invalid(&result));
    kani::cover!(epoch == 0, "rejected against epoch zero");
    std::mem::forget(result);
    std::mem::forget(address);
    std::mem::forget(facts);
}

/// The public spelling of an index key, recorded when the key is built (not
/// read back through the production accessors): `None` for a non-address
/// atom, `Some(None)` for the dummy address (it has no public index), and
/// `Some(Some(index))` for a real address with arbitrary identity.
fn any_index_key() -> (AtomKey, Option<Option<u64>>) {
    match kani::any::<u8>() {
        0 => (AtomKey::Integer(kani::any()), None),
        1 => (AtomKey::FactAddress(FactAddress::dummy()), Some(None)),
        _ => {
            let index: u64 = kani::any();
            let address = FactAddress::new(
                fact_id(kani::any()),
                kani::any(),
                Timestamp::new(kani::any()),
                index,
            );
            (AtomKey::FactAddress(address), Some(Some(index)))
        }
    }
}

/// Index keys compare fact addresses by identity, so a rebuilt key may carry
/// a different public spelling; the metadata check rejects exactly two
/// fact-address keys whose spellings differ (including a dummy address
/// against a real one) and accepts every other pair.
#[kani::proof]
#[kani::unwind(4)]
fn index_key_metadata_check_is_exact() {
    let (actual, actual_spelling) = any_index_key();
    let (expected, expected_spelling) = any_index_key();
    let result = validate_index_key_metadata(&actual, &expected);
    let expected_ok = match (actual_spelling, expected_spelling) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    };
    assert_eq!(result.is_ok(), expected_ok);
    assert!(result.is_ok() || is_invalid(&result));
    kani::cover!(
        !expected_ok
            && matches!(actual_spelling, Some(Some(_)))
            && matches!(expected_spelling, Some(Some(_))),
        "index mismatch"
    );
    kani::cover!(
        !expected_ok && actual_spelling == Some(None),
        "dummy against real address"
    );
    kani::cover!(
        expected_ok
            && matches!(actual_spelling, Some(Some(_)))
            && matches!(expected_spelling, Some(Some(_))),
        "matching real addresses"
    );
    kani::cover!(
        expected_ok && actual_spelling.is_none() && expected_spelling.is_some(),
        "non-address against address"
    );
    std::mem::forget((result, actual, expected));
}

// ---------------------------------------------------------------------------
// Values and constants
// ---------------------------------------------------------------------------

/// A symbol table holding one ASCII symbol (`a`, id 0) and no UTF-8 symbols.
fn one_symbol_table() -> SymbolTable {
    let mut symbols = SymbolTable::new();
    symbols.ascii_strings.push(Box::from(&b"a"[..]));
    symbols
}

#[kani::proof]
#[kani::unwind(4)]
fn symbol_value_is_accepted_exactly_when_it_resolves() {
    let symbols = one_symbol_table();
    let index: u32 = kani::any();
    let ascii: bool = kani::any();
    let id = if ascii {
        SymbolId::Ascii(index)
    } else {
        SymbolId::Utf8(index)
    };
    let value = Value::Symbol(Symbol(id));
    let result = symbols.validate_snapshot_value_checked(&value);
    // The table holds ASCII symbol 0 only.
    assert_eq!(result.is_ok(), ascii && index == 0);
    assert!(result.is_ok() || is_invalid(&result));
    kani::cover!(result.is_ok(), "resolvable symbol");
    std::mem::forget((result, value, symbols));
}

#[kani::proof]
#[kani::unwind(4)]
fn external_address_value_is_rejected() {
    let symbols = SymbolTable::new();
    let value = Value::ExternalAddress(ExternalAddress {
        type_id: ExternalTypeId(kani::any()),
        token: kani::any(),
    });
    let result = symbols.validate_snapshot_value_checked(&value);
    assert!(is_invalid(&result));
    std::mem::forget((result, value));
    let value = Value::Integer(kani::any());
    let result = symbols.validate_snapshot_value_checked(&value);
    assert!(result.is_ok());
    std::mem::forget((result, value, symbols));
}

#[kani::proof]
#[kani::unwind(4)]
fn fact_address_constant_is_rejected() {
    let symbols = SymbolTable::new();
    let address = ConstantTest {
        slot: SlotIndex::Ordered(kani::any()),
        test_type: ConstantTestType::Equal(AtomKey::FactAddress(FactAddress::new(
            fact_id(kani::any()),
            kani::any(),
            Timestamp::new(kani::any()),
            kani::any(),
        ))),
    };
    let result = validate_constant(&address, &symbols);
    assert!(is_invalid(&result));
    std::mem::forget((result, address, symbols));
}
