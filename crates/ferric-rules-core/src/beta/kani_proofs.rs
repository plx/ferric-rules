//! Kani proof harnesses for `beta` (see docs/verification.md).
//!
//! What is proved, on the production code:
//!
//! * `beta_memory_refines_insertion_sequence`: for every sequence of 5
//!   `insert` / `remove` / `clear` operations over two tokens, `BetaMemory`
//!   (without NCC ranks or variable indices) agrees after every step with an
//!   independent model, a duplicate-free list in insertion order: `contains`,
//!   `len`, `is_empty`, `iter` (newest first) and the member order (oldest
//!   first) match. Covers show that every transition of `OrderedSet`'s
//!   two-endpoint representation is reached. The harness also calls
//!   `validate_ncc_parent_ranks`, but ranks are never enabled here, so that
//!   check is trivially `Ok` and proves nothing about ranks.
//! * `compact_memories_is_order_preserving_renumbering`: for 4 memories and
//!   every retention mask (all 16), `compact_memories` keeps exactly the
//!   retained memories in their original order, renumbers them densely from
//!   0 through the callback, and returns the matching old-to-new mapping.
//! * `node_capacity_check_matches_allocation`: for every counter value and
//!   demand, `ensure_node_capacity(required)` succeeds exactly when `required`
//!   further IDs can be allocated; when it admits a demand of up to 3, that
//!   many `allocate_node_id` calls succeed and return consecutive IDs.
//! * `allocate_node_id_panics_when_exhausted`: allocation at `u32::MAX` panics
//!   (the documented behaviour) instead of wrapping.
//!
//! Bounds: two distinct tokens (so `OrderedSet` stays in its table-free
//! endpoint representation), 5 operations, 4 memories, 3 allocations.
//!
//! Known gaps: nothing that calls `BetaNetwork::new` is covered. Its single
//! node-map insertion of a `BetaNode` already exhausted CBMC's memory (7 GB
//! limit): `validate_consistency` on the fresh network after 318 s, and a
//! harness for the root-ID overflow defect (`BetaNetwork::new(NodeId(u32::MAX))`
//! computes `root + 1` unchecked; reproduced by ordinary execution instead:
//! debug builds panic, release builds allocate `NodeId(0)` next, and
//! `validate_consistency` accepts that network) after 321 s. NCC parent ranks
//! are not covered either: a harness inserting a second member into a
//! one-member ranked memory (two insertions into the rank table) did not
//! finish within 20 minutes, and neither did a review variant that started
//! from an empty rank table created with spare capacity (no resize) and a
//! symbolic counter, inserting two tokens (the symbolic counter makes every
//! insertion branch into the rebase path). Variable-index maintenance
//! (`insert_indexed`/`remove_indexed`, `request_var_index`) and populated
//! networks need several hash-table insertions, including into tables nested
//! in map values, which did not finish within 20 minutes in comparable
//! harnesses (see negative/kani_proofs.rs). The rank invariant (including the
//! renumbering at counter exhaustion) and the variable index's agreement with
//! a naive scan are proved without bounds on a model in
//! verification/verus/beta_memory.rs. `JoinTest`/`JoinTestType` are plain
//! data with derived traits and carry no logic to verify.

use super::*;

fn tok(n: u32) -> TokenId {
    TokenId::from(slotmap::KeyData::from_ffi(u64::from(n)))
}

/// Independent model of a beta memory: the tokens in insertion order,
/// without duplicates (from the `BetaMemory` documentation: inserting a
/// present token and removing an absent token are no-ops; `iter` yields
/// newest first).
struct ListModel {
    items: [u32; 2],
    len: usize,
}

impl ListModel {
    fn contains(&self, t: u32) -> bool {
        (self.len > 0 && self.items[0] == t) || (self.len > 1 && self.items[1] == t)
    }

    fn insert(&mut self, t: u32) {
        if !self.contains(t) {
            self.items[self.len] = t;
            self.len += 1;
        }
    }

    fn remove(&mut self, t: u32) {
        if self.len > 0 && self.items[0] == t {
            self.items[0] = self.items[1];
            self.len -= 1;
        } else if self.len > 1 && self.items[1] == t {
            self.len -= 1;
        }
    }
}

/// One symbolic operation on both the memory and the model, followed by a
/// full comparison.
fn step(memory: &mut BetaMemory, model: &mut ListModel, tokens: &[TokenId; 2]) {
    let which: u8 = kani::any_where(|w| *w < 2);
    let op: u8 = kani::any_where(|o| *o < 3);
    let t = u32::from(which) + 1;
    // Every transition of the endpoint representation is reached.
    kani::cover!(
        op == 1 && model.len == 2 && model.items[0] == t,
        "remove the older of two"
    );
    kani::cover!(
        op == 1 && model.len == 2 && model.items[1] == t,
        "remove the newer of two"
    );
    kani::cover!(
        op == 1 && model.len == 1 && model.items[0] == t,
        "remove the only token"
    );
    kani::cover!(
        op == 1 && model.len == 1 && model.items[0] != t,
        "remove an absent token"
    );
    kani::cover!(op == 0 && model.len == 2, "re-insert into a full pair");
    kani::cover!(
        op == 0 && model.len == 1 && model.items[0] == t,
        "re-insert the only token"
    );
    match op {
        0 => {
            memory.insert(tokens[usize::from(which)]);
            model.insert(t);
        }
        1 => {
            memory.remove(tokens[usize::from(which)]);
            model.remove(t);
        }
        _ => {
            memory.clear();
            model.len = 0;
        }
    }
    assert_eq!(memory.len(), model.len);
    assert_eq!(memory.is_empty(), model.len == 0);
    assert_eq!(memory.contains(tokens[0]), model.contains(1));
    assert_eq!(memory.contains(tokens[1]), model.contains(2));
    // `iter` is newest first.
    let mut iter = memory.iter();
    if model.len == 2 {
        assert_eq!(iter.next(), Some(tok(model.items[1])));
    }
    if model.len >= 1 {
        assert_eq!(iter.next(), Some(tok(model.items[0])));
    }
    assert_eq!(iter.next(), None);
    // The underlying member order, oldest first, is what rank backfill
    // (`enable_ncc_parent_ranks`) and `request_var_index` traverse.
    let mut members = memory.tokens.iter();
    if model.len >= 1 {
        assert_eq!(members.next(), Some(&tok(model.items[0])));
    }
    if model.len == 2 {
        assert_eq!(members.next(), Some(&tok(model.items[1])));
    }
    assert_eq!(members.next(), None);
    // Ranks are not enabled on this memory, so this is the trivial `None`
    // path of `validate_ncc_parent_ranks`.
    assert!(memory.validate_ncc_parent_ranks().is_ok());
}

#[kani::proof]
#[kani::unwind(4)]
fn beta_memory_refines_insertion_sequence() {
    let tokens = [tok(1), tok(2)];
    let mut memory = BetaMemory::new(BetaMemoryId(0));
    let mut model = ListModel {
        items: [0, 0],
        len: 0,
    };
    step(&mut memory, &mut model, &tokens);
    step(&mut memory, &mut model, &tokens);
    step(&mut memory, &mut model, &tokens);
    kani::cover!(
        model.len == 2 && model.items[0] == 2,
        "reverse order reached"
    );
    step(&mut memory, &mut model, &tokens);
    step(&mut memory, &mut model, &tokens);
    kani::cover!(
        model.len == 1 && model.items[0] == 2,
        "survivor after removal"
    );
    kani::cover!(model.len == 2 && model.items[0] == 1, "both, in order");
}

struct Tagged {
    id: u32,
    tag: u32,
}

#[kani::proof]
#[kani::unwind(6)]
fn compact_memories_is_order_preserving_renumbering() {
    // Callers pass one retention flag per memory (beta.rs:649-652,
    // `retain_nodes`). A symbolic memory count (0..=4, via `truncate`) made
    // CBMC run out of memory during propositional reduction, so the count is
    // fixed at 4.
    let mask: [bool; 4] = kani::any();
    let mut memories = vec![
        Tagged { id: 0, tag: 10 },
        Tagged { id: 1, tag: 11 },
        Tagged { id: 2, tag: 12 },
        Tagged { id: 3, tag: 13 },
    ];
    let mapping = compact_memories(&mut memories, &mask, |memory, id| memory.id = id);

    assert_eq!(mapping.len(), 4);
    let mut kept = 0usize;
    for old in 0..4 {
        if mask[old] {
            assert_eq!(mapping[old], Some(kept as u32));
            assert_eq!(memories[kept].id, kept as u32);
            assert_eq!(memories[kept].tag, 10 + old as u32);
            kept += 1;
        } else {
            assert_eq!(mapping[old], None);
        }
    }
    assert_eq!(memories.len(), kept);
    kani::cover!(kept == 4, "all four retained");
    kani::cover!(kept == 0, "all four dropped");
    kani::cover!(
        !mask[0] && mask[1] && !mask[2] && mask[3],
        "renumbering across gaps"
    );
}

/// A network with no nodes: only the ID counters matter for allocation.
fn empty_network(next_node_id: u32) -> BetaNetwork {
    BetaNetwork {
        nodes: HashMap::default(),
        memories: Vec::new(),
        neg_memories: Vec::new(),
        ncc_memories: Vec::new(),
        exists_memories: Vec::new(),
        root_id: NodeId(0),
        next_node_id,
        next_memory_id: 0,
        next_neg_memory_id: 0,
        next_ncc_memory_id: 0,
        next_exists_memory_id: 0,
        alpha_to_joins: HashMap::default(),
        alpha_to_negatives: HashMap::default(),
        alpha_to_exists: HashMap::default(),
        ncc_entry_waits: None,
    }
}

#[kani::proof]
#[kani::unwind(4)]
fn node_capacity_check_matches_allocation() {
    let next: u32 = kani::any();
    let required: usize = kani::any();
    let mut network = empty_network(next);
    let admitted = network.ensure_node_capacity(required).is_ok();
    // Exact characterization: room for `required` more IDs below u32::MAX.
    assert_eq!(
        admitted,
        u128::from(next) + required as u128 <= u128::from(u32::MAX)
    );
    if admitted && required <= 3 {
        let mut k = 0;
        while k < required {
            assert_eq!(network.allocate_node_id(), NodeId(next + k as u32));
            k += 1;
        }
        assert_eq!(network.next_node_id, next + required as u32);
    }
    kani::cover!(admitted && required == 3, "three admitted allocations");
    kani::cover!(admitted && next == u32::MAX - 3, "admitted at the boundary");
    kani::cover!(!admitted && required == 1, "boundary rejection");
}

#[kani::proof]
#[kani::should_panic]
fn allocate_node_id_panics_when_exhausted() {
    let mut network = empty_network(u32::MAX);
    network.allocate_node_id();
}
