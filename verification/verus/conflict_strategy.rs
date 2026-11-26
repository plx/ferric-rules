//! Verus model of ferric-rules-core's conflict-resolution ordering and agenda.
//!
//! Production code modelled (crates/ferric-rules-core/src/...):
//! strategy.rs:11 `ConflictResolutionStrategy`; agenda.rs:100 `Activation`,
//! agenda.rs:119 `StrategyOrd` and agenda.rs:135 `AgendaKey` with their derived
//! `Ord`; agenda.rs:207 `Agenda::build_key`; and the `Agenda` operations
//! `with_strategy` (:192), `add` (:245, including the exhaustion path through
//! `rebase_sequences` :276), `pop` (:304), `pop_matching` (:324),
//! `remove_activations_for_token` (:407), `remove_activations_for_rule` (:435),
//! `iter_ordered` (:484), `set_strategy` (:500) and `clear` (:520), against the
//! consistency invariant checked by `validate_consistency` (:541).
//!
//! What is proved (every result is unbounded: recency vectors of any length,
//! agendas of any size, all integer values):
//! 1. Exec transcriptions (`cmp_vecs`, `cmp_strategy_ord`, `cmp_key`,
//!    `sort_desc`, `build_key`, `cmp_activations`) compute exactly the spec
//!    functions below. These exec functions are what
//!    crates/ferric-rules-core/src/agenda/kani_refinement.rs transcribes and
//!    proves equal to production on bounded inputs.
//! 2. `theorem_key_order_is_total`: the derived `Ord` on `AgendaKey` (all
//!    `StrategyOrd` variants, cross-variant pairs included) is a total order
//!    (vstd `total_ordering`: reflexive, antisymmetric, transitive, strongly
//!    connected), its strict part is a strict total order, `cmp(a, b)` is the
//!    reverse of `cmp(b, a)`, and `Equal` coincides with equality.
//! 3. `theorem_activation_order_is_total` (each of Depth, Breadth, LEX, MEA):
//!    the induced comparison of activations is a total preorder with
//!    antisymmetric `cmp` that only ties activations with the same sequence
//!    number; `corollary_activation_order_total_on_agenda`: a total order on
//!    any set of activations with distinct sequence numbers.
//! 4. Documented semantics: salience precedes everything
//!    (`theorem_salience_precedes_everything`); Depth = newest first, Breadth =
//!    oldest first, and neither consults recency or complexity; LEX/MEA depend
//!    only on the multiset of recency tags (MEA also on the first tag); LEX's
//!    exact characterization (`theorem_lex_exact`: first differing tag of the
//!    descending-sorted bases, longer basis on an equal prefix, then higher
//!    complexity, then older), the most-recent-fact and longer-prefix
//!    corollaries, and monotonicity (`theorem_lex_prefers_strictly_larger_basis`:
//!    a basis extended by at least one more tag always wins under LEX, and
//!    under MEA when the first tags agree); MEA's first pattern dominates and
//!    MEA equals LEX on a tie;
//!    order-preserving renumbering (what `rebase_sequences` does) never
//!    changes a comparison.
//! 5. Ordered collection: `map_insert`/`map_remove` (models of
//!    `BTreeMap::insert`/`remove`) preserve strict sortedness and update
//!    exactly the targeted entry; the first entry is the minimum.
//! 6. Agenda: `inv` (models `validate_consistency` checks 1-4 and the
//!    sequence/key checks, plus the BTreeMap's sortedness) holds for a new or
//!    cleared agenda and is preserved by `add` (both paths), `rebase`, `pop`,
//!    `pop_matching`, `remove_activations_for_token`,
//!    `remove_activations_for_rule` and `set_strategy`. `pop` returns `None`
//!    exactly on an empty agenda and otherwise the activation that strictly
//!    precedes every other live activation (the maximum priority);
//!    `pop_matching` likewise among the activations satisfying the predicate;
//!    `iter_ordered` (modelled with its `filter_map`, `iter_ordered_spec`)
//!    drops no entry and yields every live activation exactly once, in
//!    strictly decreasing priority; the removal operations remove exactly
//!    the targeted activations; `set_strategy` and `rebase` keep every
//!    activation (rebase changing only sequence numbers, and preserving every
//!    pairwise comparison). `theorem_set_strategy_deterministic`: the rebuilt
//!    agenda does not depend on the SlotMap iteration order (via
//!    `lemma_sorted_entries_unique`). `example_add_then_pop` shows `inv` is
//!    satisfiable by a non-empty agenda.
//! 7. Agenda-level behaviour: `theorem_pops_in_priority_order` (any number of
//!    successive pops returns distinct live activations in strictly
//!    decreasing priority, stopping early only once all were returned);
//!    where `add` places the new activation: under Depth it precedes every
//!    live activation of lower or equal salience and is popped next when no
//!    live activation has higher salience
//!    (`theorem_depth_add_places_new_first`); under Breadth it follows every
//!    live activation of higher or equal salience, so the next pop is
//!    unchanged when one exists (`theorem_breadth_add_places_new_last`);
//!    under LEX an activation with the most recent fact, and under MEA one
//!    with the most recent first-pattern fact, is popped next at maximal
//!    salience (`theorem_lex_add_most_recent_fact_fires_next`,
//!    `theorem_mea_add_most_recent_first_pattern_fires_next`).
//!
//! Abstractions and simplifications (fidelity notes):
//! - Newtypes are unwrapped (`Salience` -> i32, `ActivationSeq`/`RecencyTag`
//!   -> u64, ids/tokens/rules -> nat in specs). `std::cmp::Reverse` fields are
//!   not stored; the comparison swaps the operands exactly where production
//!   wraps a field in `Reverse`. `SmallVec<[RecencyTag; 4]>` -> `Vec<u64>` /
//!   `Seq<u64>` (`SmallVec::cmp` delegates to the slice order). The
//!   activation's `timestamp` field is omitted (never read by ordering code).
//! - Slice `Ord` is modelled by `cmp_seq` (element-wise, then length);
//!   `cmp_vecs` is a loop transcription of core's `SliceOrd::compare`.
//! - `sort_unstable_by(|l, r| r.cmp(l))` is specified as vstd's `sort_by` with
//!   the descending order; the exec model is an insertion sort, and
//!   `lemma_sorted_unique` makes any correct sort produce the same output.
//! - `BTreeMap` is modelled by its in-order entry sequence; `insert`/`remove`
//!   are a linear in-order search (the node structure is abstracted away);
//!   `pop_first` is `(s[0], s.drop_first())`. `SlotMap`/`SecondaryMap`/
//!   `FxHashMap` are modelled as `Map`s. A fresh SlotMap key is a parameter
//!   required not to be live; SlotMap iteration orders are parameters
//!   (any duplicate-free enumeration of the live ids); the
//!   `sort_unstable_by_key` result in `rebase_sequences` is a parameter
//!   required to list the live ids in strictly increasing sequence order.
//! - `pop_matching`'s predicate is a pure `spec_fn`. The `?` early returns
//!   in `pop`/`pop_matching` on a missing activation are unreachable under
//!   `inv`; the model reads the map directly.
//! - The lazily built `rule_ordering` index, `pop_matching_rule` and
//!   `validate_consistency`'s rule-index check are not modelled.
//!
//! Trusted base: the Verus verifier and vstd (including its specifications of
//! `Vec::new`/`insert`/`len`/indexing and its proven `sort_by`,
//! `lemma_sort_by_ensures`, `lemma_sorted_unique` and multiset/filter lemmas).
//! This file uses no `assume`, `admit`, `external_body`, `external`,
//! `assume_specification` or `exec_allows_no_decreases_clause`.
//!
//! Known gaps: the link between these exec functions and production is the
//! bounded Kani refinement in agenda/kani_refinement.rs (comparisons and key
//! construction only, not the agenda operations); fidelity of the agenda
//! operations to production is by inspection (each model cites its source
//! lines); `pop_matching_rule` and the per-rule index are not covered; the
//! agenda model assumes fewer than `u64::MAX` live activations on the
//! exhaustion path. Production does not check this itself (the `expect`s in
//! `rebase_sequences`, agenda.rs:286-291, only guard `usize -> u64`
//! conversions, which cannot fail on 64-bit targets); it holds because the
//! `activations` SlotMap panics with "SlotMap is full" before reaching
//! `u32::MAX` slots (slotmap 1.1.1 src/basic.rs:413-415). With `u64::MAX`
//! live activations, `ActivationSeq::next` (agenda.rs:46-48) would overflow
//! after the rebase.

use vstd::prelude::*;
use vstd::relations::*;
use vstd::seq_lib::*;
use core::cmp::Ordering;

verus! {

// =====================================================================
// Section 1. Scalar comparison helpers
// =====================================================================

/// `Ord::cmp` for the primitive integers used in agenda keys (i32, u16, u64),
/// lifted to mathematical integers.
pub open spec fn cmp_int(a: int, b: int) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a == b {
        Ordering::Equal
    } else {
        Ordering::Greater
    }
}

/// `Ordering::reverse`.
pub open spec fn flip(o: Ordering) -> Ordering {
    match o {
        Ordering::Less => Ordering::Greater,
        Ordering::Equal => Ordering::Equal,
        Ordering::Greater => Ordering::Less,
    }
}

/// `Ordering::then`: the chaining step that `#[derive(Ord)]` generates between
/// consecutive fields (the next field is consulted only on `Equal`).
pub open spec fn then(first: Ordering, next: Ordering) -> Ordering {
    match first {
        Ordering::Equal => next,
        _ => first,
    }
}

// Exec transcriptions of `Ord::cmp` on the primitive field types.
pub fn cmp_u64(a: u64, b: u64) -> (r: Ordering)
    ensures
        r == cmp_int(a as int, b as int),
{
    if a < b {
        Ordering::Less
    } else if a == b {
        Ordering::Equal
    } else {
        Ordering::Greater
    }
}

pub fn cmp_i32(a: i32, b: i32) -> (r: Ordering)
    ensures
        r == cmp_int(a as int, b as int),
{
    if a < b {
        Ordering::Less
    } else if a == b {
        Ordering::Equal
    } else {
        Ordering::Greater
    }
}

pub fn cmp_u16(a: u16, b: u16) -> (r: Ordering)
    ensures
        r == cmp_int(a as int, b as int),
{
    if a < b {
        Ordering::Less
    } else if a == b {
        Ordering::Equal
    } else {
        Ordering::Greater
    }
}

// =====================================================================
// Section 2. Lexicographic comparison of recency vectors
// =====================================================================

/// Models `<SmallVec<[RecencyTag; 4]> as Ord>::cmp`, which delegates to
/// `<[RecencyTag] as Ord>::cmp` (smallvec 1.15.2 lib.rs `impl Ord for
/// SmallVec`): element-wise comparison of the common prefix, then the
/// shorter sequence is smaller.
pub open spec fn cmp_seq(a: Seq<u64>, b: Seq<u64>) -> Ordering
    decreases a.len(),
{
    if a.len() == 0 {
        if b.len() == 0 {
            Ordering::Equal
        } else {
            Ordering::Less
        }
    } else if b.len() == 0 {
        Ordering::Greater
    } else {
        then(cmp_int(a[0] as int, b[0] as int), cmp_seq(a.drop_first(), b.drop_first()))
    }
}

/// Exec transcription of core's slice `Ord` (core::slice::cmp `SliceOrd::compare`):
/// compare `min(len)` elements in order, then compare lengths.
pub fn cmp_vecs(left: &Vec<u64>, right: &Vec<u64>) -> (r: Ordering)
    ensures
        r == cmp_seq(left@, right@),
{
    let l = if left.len() < right.len() {
        left.len()
    } else {
        right.len()
    };
    let mut i: usize = 0;
    assert(left@.skip(0) =~= left@);
    assert(right@.skip(0) =~= right@);
    while i < l
        invariant
            i <= l,
            l <= left@.len(),
            l <= right@.len(),
            l == left@.len() || l == right@.len(),
            cmp_seq(left@, right@) == cmp_seq(left@.skip(i as int), right@.skip(i as int)),
        decreases l - i,
    {
        let ghost a = left@.skip(i as int);
        let ghost b = right@.skip(i as int);
        assert(a.drop_first() =~= left@.skip(i + 1));
        assert(b.drop_first() =~= right@.skip(i + 1));
        match cmp_u64(left[i], right[i]) {
            Ordering::Equal => {},
            non_eq => {
                return non_eq;
            },
        }
        i += 1;
    }
    cmp_u64(left.len() as u64, right.len() as u64)
}

// ---------------------------------------------------------------------
// Order lemmas for cmp_seq (arbitrary lengths)
// ---------------------------------------------------------------------

pub proof fn lemma_cmp_seq_refl(a: Seq<u64>)
    ensures
        cmp_seq(a, a) == Ordering::Equal,
    decreases a.len(),
{
    if a.len() > 0 {
        lemma_cmp_seq_refl(a.drop_first());
    }
}

pub proof fn lemma_cmp_seq_flip(a: Seq<u64>, b: Seq<u64>)
    ensures
        cmp_seq(a, b) == flip(cmp_seq(b, a)),
    decreases a.len(),
{
    if a.len() > 0 && b.len() > 0 {
        lemma_cmp_seq_flip(a.drop_first(), b.drop_first());
    }
}

pub proof fn lemma_cmp_seq_equal(a: Seq<u64>, b: Seq<u64>)
    ensures
        cmp_seq(a, b) == Ordering::Equal <==> a == b,
    decreases a.len(),
{
    if a == b {
        lemma_cmp_seq_refl(a);
    }
    if a.len() == 0 && b.len() == 0 {
        assert(a =~= b);
    }
    if a.len() > 0 && b.len() > 0 {
        lemma_cmp_seq_equal(a.drop_first(), b.drop_first());
        if cmp_seq(a, b) == Ordering::Equal {
            assert forall|i: int| 0 <= i < a.len() implies a[i] == b[i] by {
                if i > 0 {
                    assert(a[i] == a.drop_first()[i - 1]);
                    assert(b[i] == b.drop_first()[i - 1]);
                }
            }
            assert(a =~= b);
        }
    }
}

pub proof fn lemma_cmp_seq_trans(a: Seq<u64>, b: Seq<u64>, c: Seq<u64>)
    requires
        cmp_seq(a, b) == Ordering::Less,
        cmp_seq(b, c) == Ordering::Less,
    ensures
        cmp_seq(a, c) == Ordering::Less,
    decreases a.len(),
{
    if a.len() > 0 && b.len() > 0 && c.len() > 0 {
        if a[0] == b[0] && b[0] == c[0] {
            lemma_cmp_seq_trans(a.drop_first(), b.drop_first(), c.drop_first());
        }
    }
}

// =====================================================================
// Section 3. Model types (exec transcriptions + spec views)
// =====================================================================

/// Models `ConflictResolutionStrategy` (crates/ferric-rules-core/src/strategy.rs:11).
#[derive(Clone, Copy)]
pub enum Strategy {
    Depth,
    Breadth,
    Lex,
    Mea,
}

/// Models `Activation` (crates/ferric-rules-core/src/agenda.rs:100).
/// Newtypes are unwrapped: ActivationId/TokenId -> u64, RuleId(u32) -> u32,
/// Salience(i32) -> i32, ActivationSeq(u64) -> u64, RecencyTag(u64) -> u64,
/// SmallVec<[RecencyTag; 4]> -> Vec<u64>. `timestamp` is omitted: no agenda
/// ordering code reads it.
pub struct Activation {
    pub id: u64,
    pub rule: u32,
    pub token: u64,
    pub salience: i32,
    pub complexity: u16,
    pub activation_seq: u64,
    pub recency: Vec<u64>,
}

/// Spec view of `Activation`.
pub struct SAct {
    pub id: nat,
    pub rule: nat,
    pub token: nat,
    pub salience: i32,
    pub complexity: u16,
    pub activation_seq: u64,
    pub recency: Seq<u64>,
}

impl View for Activation {
    type V = SAct;

    open spec fn view(&self) -> SAct {
        SAct {
            id: self.id as nat,
            rule: self.rule as nat,
            token: self.token as nat,
            salience: self.salience,
            complexity: self.complexity,
            activation_seq: self.activation_seq,
            recency: self.recency@,
        }
    }
}

/// Models `StrategyOrd` (crates/ferric-rules-core/src/agenda.rs:119). The `std::cmp::Reverse` wrappers
/// are not represented as data; instead `cmp_strategy_ord` swaps the operands
/// exactly where production wraps a field in `Reverse`
/// (`Reverse(a).cmp(&Reverse(b)) == b.cmp(&a)`).
pub enum StrategyOrd {
    Depth(u64),
    Breadth(u64),
    Lex(Vec<u64>),
    Mea { first_recency: u64, sorted_recency: Vec<u64> },
}

/// Spec view of `StrategyOrd`.
pub enum SOrd {
    Depth(u64),
    Breadth(u64),
    Lex(Seq<u64>),
    Mea { first_recency: u64, sorted_recency: Seq<u64> },
}

impl View for StrategyOrd {
    type V = SOrd;

    open spec fn view(&self) -> SOrd {
        match self {
            StrategyOrd::Depth(x) => SOrd::Depth(*x),
            StrategyOrd::Breadth(x) => SOrd::Breadth(*x),
            StrategyOrd::Lex(v) => SOrd::Lex(v@),
            StrategyOrd::Mea { first_recency, sorted_recency } => SOrd::Mea {
                first_recency: *first_recency,
                sorted_recency: sorted_recency@,
            },
        }
    }
}

/// Models `AgendaKey` (crates/ferric-rules-core/src/agenda.rs:135); field order is the derive order.
pub struct AgendaKey {
    pub salience: i32,
    pub strategy_ord: StrategyOrd,
    pub complexity: u16,
    pub seq: u64,
}

/// Spec view of `AgendaKey`.
pub struct SKey {
    pub salience: i32,
    pub strategy_ord: SOrd,
    pub complexity: u16,
    pub seq: u64,
}

impl View for AgendaKey {
    type V = SKey;

    open spec fn view(&self) -> SKey {
        SKey {
            salience: self.salience,
            strategy_ord: self.strategy_ord@,
            complexity: self.complexity,
            seq: self.seq,
        }
    }
}

// =====================================================================
// Section 4. Key comparison (the derived `Ord` impls)
// =====================================================================

/// Discriminant used by `#[derive(Ord)]` on `StrategyOrd` for cross-variant
/// comparisons (declaration order).
pub open spec fn variant_index(o: SOrd) -> int {
    match o {
        SOrd::Depth(_) => 0,
        SOrd::Breadth(_) => 1,
        SOrd::Lex(_) => 2,
        SOrd::Mea { .. } => 3,
    }
}

/// Models `<StrategyOrd as Ord>::cmp` (derived; crates/ferric-rules-core/src/agenda.rs:117-127).
pub open spec fn cmp_strategy_ord_spec(a: SOrd, b: SOrd) -> Ordering {
    match (a, b) {
        // Depth(Reverse<ActivationSeq>): operands swapped.
        (SOrd::Depth(x), SOrd::Depth(y)) => cmp_int(y as int, x as int),
        (SOrd::Breadth(x), SOrd::Breadth(y)) => cmp_int(x as int, y as int),
        // Lex(Reverse<SmallVec>): operands swapped.
        (SOrd::Lex(x), SOrd::Lex(y)) => cmp_seq(y, x),
        // Mea { Reverse<RecencyTag>, Reverse<SmallVec> }: both swapped, chained.
        (
            SOrd::Mea { first_recency: f1, sorted_recency: s1 },
            SOrd::Mea { first_recency: f2, sorted_recency: s2 },
        ) => then(cmp_int(f2 as int, f1 as int), cmp_seq(s2, s1)),
        _ => cmp_int(variant_index(a), variant_index(b)),
    }
}

/// Models `<AgendaKey as Ord>::cmp` (derived; crates/ferric-rules-core/src/agenda.rs:133-144):
/// Reverse(salience), strategy_ord, Reverse(complexity), seq.
pub open spec fn cmp_key_spec(a: SKey, b: SKey) -> Ordering {
    then(
        cmp_int(b.salience as int, a.salience as int),
        then(
            cmp_strategy_ord_spec(a.strategy_ord, b.strategy_ord),
            then(
                cmp_int(b.complexity as int, a.complexity as int),
                cmp_int(a.seq as int, b.seq as int),
            ),
        ),
    )
}

/// Exec transcription of the derive-generated discriminant read.
pub fn discriminant(o: &StrategyOrd) -> (r: u64)
    ensures
        r as int == variant_index(o@),
{
    match o {
        StrategyOrd::Depth(_) => 0,
        StrategyOrd::Breadth(_) => 1,
        StrategyOrd::Lex(_) => 2,
        StrategyOrd::Mea { .. } => 3,
    }
}

/// Exec transcription of the derived `<StrategyOrd as Ord>::cmp` (crates/ferric-rules-core/src/agenda.rs:117).
pub fn cmp_strategy_ord(a: &StrategyOrd, b: &StrategyOrd) -> (r: Ordering)
    ensures
        r == cmp_strategy_ord_spec(a@, b@),
{
    match (a, b) {
        (StrategyOrd::Depth(x), StrategyOrd::Depth(y)) => cmp_u64(*y, *x),
        (StrategyOrd::Breadth(x), StrategyOrd::Breadth(y)) => cmp_u64(*x, *y),
        (StrategyOrd::Lex(x), StrategyOrd::Lex(y)) => cmp_vecs(y, x),
        (
            StrategyOrd::Mea { first_recency: f1, sorted_recency: s1 },
            StrategyOrd::Mea { first_recency: f2, sorted_recency: s2 },
        ) => {
            match cmp_u64(*f2, *f1) {
                Ordering::Equal => {},
                non_eq => {
                    return non_eq;
                },
            }
            cmp_vecs(s2, s1)
        },
        _ => cmp_u64(discriminant(a), discriminant(b)),
    }
}

/// Exec transcription of the derived `<AgendaKey as Ord>::cmp` (crates/ferric-rules-core/src/agenda.rs:133).
pub fn cmp_key(a: &AgendaKey, b: &AgendaKey) -> (r: Ordering)
    ensures
        r == cmp_key_spec(a@, b@),
{
    match cmp_i32(b.salience, a.salience) {
        Ordering::Equal => {},
        non_eq => {
            return non_eq;
        },
    }
    match cmp_strategy_ord(&a.strategy_ord, &b.strategy_ord) {
        Ordering::Equal => {},
        non_eq => {
            return non_eq;
        },
    }
    match cmp_u16(b.complexity, a.complexity) {
        Ordering::Equal => {},
        non_eq => {
            return non_eq;
        },
    }
    cmp_u64(a.seq, b.seq)
}

// =====================================================================
// Section 5. Key construction (Agenda::build_key)
// =====================================================================

/// The descending order used by `sort_unstable_by(|l, r| r.cmp(l))`.
pub open spec fn desc_leq() -> spec_fn(u64, u64) -> bool {
    |x: u64, y: u64| y <= x
}

/// The (unique) descending sort of a recency vector.
pub open spec fn sort_desc_spec(s: Seq<u64>) -> Seq<u64> {
    s.sort_by(desc_leq())
}

/// `recency.first().copied().unwrap_or(RecencyTag::ABSENT)` (crates/ferric-rules-core/src/agenda.rs:221-225).
pub open spec fn first_or_absent(s: Seq<u64>) -> u64 {
    if s.len() > 0 {
        s[0]
    } else {
        0
    }
}

/// Models `Agenda::build_key` (crates/ferric-rules-core/src/agenda.rs:207).
pub open spec fn build_key_spec(strategy: Strategy, a: SAct) -> SKey {
    SKey {
        salience: a.salience,
        strategy_ord: match strategy {
            Strategy::Depth => SOrd::Depth(a.activation_seq),
            Strategy::Breadth => SOrd::Breadth(a.activation_seq),
            Strategy::Lex => SOrd::Lex(sort_desc_spec(a.recency)),
            Strategy::Mea => SOrd::Mea {
                first_recency: first_or_absent(a.recency),
                sorted_recency: sort_desc_spec(a.recency),
            },
        },
        complexity: a.complexity,
        seq: a.activation_seq,
    }
}

/// Conflict-resolution comparison of two activations under `strategy`:
/// `Less` means `a` fires before `b`.
pub open spec fn cmp_act_spec(strategy: Strategy, a: SAct, b: SAct) -> Ordering {
    cmp_key_spec(build_key_spec(strategy, a), build_key_spec(strategy, b))
}

pub proof fn lemma_desc_leq_total()
    ensures
        total_ordering(desc_leq()),
{
}

/// Exec model of `sorted_recency.sort_unstable_by(|l, r| r.cmp(l))`
/// (crates/ferric-rules-core/src/agenda.rs:214-215): an insertion sort into descending order. Any
/// correct sort yields the same output (`lemma_sorted_unique`), so the
/// choice of algorithm is immaterial.
pub fn sort_desc(v: &Vec<u64>) -> (r: Vec<u64>)
    ensures
        r@ == sort_desc_spec(v@),
{
    broadcast use group_to_multiset_ensures;

    let mut out: Vec<u64> = Vec::new();
    let mut i: usize = 0;
    while i < v.len()
        invariant
            i <= v.len(),
            sorted_by(out@, desc_leq()),
            out@.to_multiset() == v@.take(i as int).to_multiset(),
        decreases v.len() - i,
    {
        let x = v[i];
        let mut j: usize = 0;
        while j < out.len() && out[j] >= x
            invariant
                j <= out.len(),
                forall|k: int| 0 <= k < j ==> out@[k] >= x,
            decreases out.len() - j,
        {
            j += 1;
        }
        let ghost before = out@;
        out.insert(j, x);
        proof {
            assert(out@ == before.insert(j as int, x));
            assert forall|p: int, q: int| 0 <= p < q < out@.len() implies #[trigger] desc_leq()(
                out@[p],
                out@[q],
            ) by {
                if q < j {
                    assert(desc_leq()(before[p], before[q]));
                } else if q == j {
                } else if p < j {
                    assert(desc_leq()(before[p], before[q - 1]));
                } else if p == j {
                    if q - 1 > j {
                        assert(desc_leq()(before[j as int], before[q - 1]));
                    }
                } else {
                    assert(desc_leq()(before[p - 1], before[q - 1]));
                }
            }
            assert(v@.take(i + 1) =~= v@.take(i as int).push(x));
            to_multiset_insert(before, j as int, x);
            to_multiset_build(v@.take(i as int), x);
        }
        i += 1;
    }
    proof {
        assert(v@.take(v@.len() as int) =~= v@);
        lemma_desc_leq_total();
        v@.lemma_sort_by_ensures(desc_leq());
        lemma_sorted_unique(out@, sort_desc_spec(v@), desc_leq());
    }
    out
}

/// Exec transcription of `Agenda::build_key` (crates/ferric-rules-core/src/agenda.rs:207-239) with
/// `self.strategy` passed explicitly.
pub fn build_key(strategy: Strategy, activation: &Activation) -> (k: AgendaKey)
    ensures
        k@ == build_key_spec(strategy, activation@),
{
    let strategy_ord = match strategy {
        Strategy::Depth => StrategyOrd::Depth(activation.activation_seq),
        Strategy::Breadth => StrategyOrd::Breadth(activation.activation_seq),
        Strategy::Lex | Strategy::Mea => {
            let sorted_recency = sort_desc(&activation.recency);
            if let Strategy::Lex = strategy {
                StrategyOrd::Lex(sorted_recency)
            } else {
                StrategyOrd::Mea {
                    first_recency: if activation.recency.len() > 0 {
                        activation.recency[0]
                    } else {
                        0
                    },
                    sorted_recency,
                }
            }
        },
    };
    AgendaKey {
        salience: activation.salience,
        strategy_ord,
        complexity: activation.complexity,
        seq: activation.activation_seq,
    }
}

/// Exec: the comparison the agenda's `BTreeMap` performs between the keys of
/// two activations under one strategy.
pub fn cmp_activations(strategy: Strategy, a: &Activation, b: &Activation) -> (r: Ordering)
    ensures
        r == cmp_act_spec(strategy, a@, b@),
{
    let ka = build_key(strategy, a);
    let kb = build_key(strategy, b);
    cmp_key(&ka, &kb)
}

// =====================================================================
// Section 6. Total-order theorems (inputs of any size)
// =====================================================================

pub proof fn lemma_sord_refl(a: SOrd)
    ensures
        cmp_strategy_ord_spec(a, a) == Ordering::Equal,
{
    match a {
        SOrd::Lex(x) => lemma_cmp_seq_refl(x),
        SOrd::Mea { first_recency, sorted_recency } => lemma_cmp_seq_refl(sorted_recency),
        _ => {},
    }
}

pub proof fn lemma_sord_flip(a: SOrd, b: SOrd)
    ensures
        cmp_strategy_ord_spec(a, b) == flip(cmp_strategy_ord_spec(b, a)),
{
    match (a, b) {
        (SOrd::Lex(x), SOrd::Lex(y)) => lemma_cmp_seq_flip(y, x),
        (
            SOrd::Mea { first_recency: f1, sorted_recency: s1 },
            SOrd::Mea { first_recency: f2, sorted_recency: s2 },
        ) => lemma_cmp_seq_flip(s2, s1),
        _ => {},
    }
}

pub proof fn lemma_sord_equal(a: SOrd, b: SOrd)
    ensures
        cmp_strategy_ord_spec(a, b) == Ordering::Equal <==> a == b,
{
    lemma_sord_refl(a);
    match (a, b) {
        (SOrd::Lex(x), SOrd::Lex(y)) => lemma_cmp_seq_equal(y, x),
        (
            SOrd::Mea { first_recency: f1, sorted_recency: s1 },
            SOrd::Mea { first_recency: f2, sorted_recency: s2 },
        ) => lemma_cmp_seq_equal(s2, s1),
        _ => {},
    }
}

pub proof fn lemma_sord_trans(a: SOrd, b: SOrd, c: SOrd)
    requires
        cmp_strategy_ord_spec(a, b) == Ordering::Less,
        cmp_strategy_ord_spec(b, c) == Ordering::Less,
    ensures
        cmp_strategy_ord_spec(a, c) == Ordering::Less,
{
    match (a, b, c) {
        (SOrd::Lex(x), SOrd::Lex(y), SOrd::Lex(z)) => lemma_cmp_seq_trans(z, y, x),
        (
            SOrd::Mea { first_recency: f1, sorted_recency: s1 },
            SOrd::Mea { first_recency: f2, sorted_recency: s2 },
            SOrd::Mea { first_recency: f3, sorted_recency: s3 },
        ) => {
            if f1 == f2 && f2 == f3 {
                lemma_cmp_seq_trans(s3, s2, s1);
            }
        },
        _ => {},
    }
}

pub proof fn lemma_key_refl(a: SKey)
    ensures
        cmp_key_spec(a, a) == Ordering::Equal,
{
    lemma_sord_refl(a.strategy_ord);
}

pub proof fn lemma_key_flip(a: SKey, b: SKey)
    ensures
        cmp_key_spec(a, b) == flip(cmp_key_spec(b, a)),
{
    lemma_sord_flip(a.strategy_ord, b.strategy_ord);
}

pub proof fn lemma_key_equal(a: SKey, b: SKey)
    ensures
        cmp_key_spec(a, b) == Ordering::Equal <==> a == b,
{
    lemma_key_refl(a);
    lemma_sord_equal(a.strategy_ord, b.strategy_ord);
}

pub proof fn lemma_key_trans(a: SKey, b: SKey, c: SKey)
    requires
        cmp_key_spec(a, b) == Ordering::Less,
        cmp_key_spec(b, c) == Ordering::Less,
    ensures
        cmp_key_spec(a, c) == Ordering::Less,
{
    lemma_sord_equal(a.strategy_ord, b.strategy_ord);
    lemma_sord_equal(b.strategy_ord, c.strategy_ord);
    lemma_sord_equal(a.strategy_ord, c.strategy_ord);
    if cmp_strategy_ord_spec(a.strategy_ord, b.strategy_ord) == Ordering::Less
        && cmp_strategy_ord_spec(b.strategy_ord, c.strategy_ord) == Ordering::Less {
        lemma_sord_trans(a.strategy_ord, b.strategy_ord, c.strategy_ord);
    }
}

pub proof fn lemma_key_le_trans(a: SKey, b: SKey, c: SKey)
    requires
        cmp_key_spec(a, b) != Ordering::Greater,
        cmp_key_spec(b, c) != Ordering::Greater,
    ensures
        cmp_key_spec(a, c) != Ordering::Greater,
        cmp_key_spec(a, b) == Ordering::Less || cmp_key_spec(b, c) == Ordering::Less
            ==> cmp_key_spec(a, c) == Ordering::Less,
{
    lemma_key_equal(a, b);
    lemma_key_equal(b, c);
    if cmp_key_spec(a, b) == Ordering::Less && cmp_key_spec(b, c) == Ordering::Less {
        lemma_key_trans(a, b, c);
    }
}

/// "a precedes or equals b" on agenda keys (BTreeMap order; smaller = fires first).
pub open spec fn key_le() -> spec_fn(SKey, SKey) -> bool {
    |a: SKey, b: SKey| cmp_key_spec(a, b) != Ordering::Greater
}

/// "a strictly precedes b" on agenda keys.
pub open spec fn key_lt() -> spec_fn(SKey, SKey) -> bool {
    |a: SKey, b: SKey| cmp_key_spec(a, b) == Ordering::Less
}

/// THEOREM (key order). The modelled derived `Ord` on `AgendaKey` — over keys
/// of every strategy variant and recency vectors of any length — is a total
/// order (reflexive, antisymmetric, transitive, total), its strict part is a
/// strict total order, `cmp` is antisymmetric in the `Ordering` sense, and
/// `Equal` coincides with equality.
pub proof fn theorem_key_order_is_total()
    ensures
        total_ordering(key_le()),
        strict_total_ordering(key_lt()),
        forall|a: SKey, b: SKey| #[trigger] cmp_key_spec(a, b) == flip(cmp_key_spec(b, a)),
        forall|a: SKey, b: SKey| #[trigger] cmp_key_spec(a, b) == Ordering::Equal <==> a == b,
{
    assert forall|a: SKey, b: SKey| #[trigger] cmp_key_spec(a, b) == flip(cmp_key_spec(b, a)) by {
        lemma_key_flip(a, b);
    }
    assert forall|a: SKey, b: SKey| #[trigger] cmp_key_spec(a, b) == Ordering::Equal <==> a == b by {
        lemma_key_equal(a, b);
    }
    assert forall|x: SKey| #[trigger] key_le()(x, x) by {
        lemma_key_refl(x);
    }
    assert forall|x: SKey| #[trigger] key_lt()(x, x) == false by {
        lemma_key_refl(x);
    }
    assert forall|x: SKey, y: SKey| #[trigger] key_le()(x, y) && #[trigger] key_le()(y, x) implies x
        == y by {
        lemma_key_flip(x, y);
        lemma_key_equal(x, y);
    }
    assert forall|x: SKey, y: SKey| #[trigger] key_lt()(x, y) && #[trigger] key_lt()(y, x) implies x
        == y by {
        lemma_key_flip(x, y);
    }
    assert forall|x: SKey, y: SKey, z: SKey| #[trigger] key_le()(x, y) && #[trigger] key_le()(
        y,
        z,
    ) implies key_le()(x, z) by {
        lemma_key_le_trans(x, y, z);
    }
    assert forall|x: SKey, y: SKey, z: SKey| #[trigger] key_lt()(x, y) && #[trigger] key_lt()(
        y,
        z,
    ) implies key_lt()(x, z) by {
        lemma_key_trans(x, y, z);
    }
    assert forall|x: SKey, y: SKey| #[trigger] key_le()(x, y) || #[trigger] key_le()(y, x) by {
        lemma_key_flip(x, y);
    }
    assert forall|x: SKey, y: SKey| x != y implies #[trigger] key_lt()(x, y) || #[trigger] key_lt()(
        y,
        x,
    ) by {
        lemma_key_flip(x, y);
        lemma_key_equal(x, y);
    }
}

/// "a fires no later than b" under `strategy`.
pub open spec fn act_le(strategy: Strategy) -> spec_fn(SAct, SAct) -> bool {
    |a: SAct, b: SAct| cmp_act_spec(strategy, a, b) != Ordering::Greater
}

/// Activations whose sequence numbers identify them (agenda invariant:
/// `validate_consistency` rejects duplicate sequences, crates/ferric-rules-core/src/agenda.rs:633-639).
pub open spec fn seqs_identify(acts: Set<SAct>) -> bool {
    forall|a: SAct, b: SAct|
        #[trigger] acts.contains(a) && #[trigger] acts.contains(b) && a.activation_seq
            == b.activation_seq ==> a == b
}

/// THEOREM (activation order, per strategy). For each of Depth, Breadth, LEX
/// and MEA the induced comparison of activations is a total preorder
/// (reflexive, transitive, total) with antisymmetric `cmp`; two activations
/// compare `Equal` only if they share salience, complexity and sequence
/// number, so on any set of activations whose sequence numbers are distinct
/// (as in every consistent agenda) it is a total order.
pub proof fn theorem_activation_order_is_total(strategy: Strategy)
    ensures
        pre_ordering(act_le(strategy)),
        strongly_connected(act_le(strategy)),
        forall|a: SAct, b: SAct| #[trigger]
            cmp_act_spec(strategy, a, b) == flip(cmp_act_spec(strategy, b, a)),
        forall|a: SAct, b: SAct| #[trigger]
            cmp_act_spec(strategy, a, b) == Ordering::Equal ==> a.activation_seq
                == b.activation_seq && a.salience == b.salience && a.complexity == b.complexity,
        forall|a: SAct, b: SAct| #[trigger] act_le(strategy)(a, b) && #[trigger] act_le(strategy)(b, a)
            ==> a.activation_seq == b.activation_seq,
{
    assert forall|a: SAct, b: SAct| #[trigger]
        cmp_act_spec(strategy, a, b) == flip(cmp_act_spec(strategy, b, a)) by {
        lemma_key_flip(build_key_spec(strategy, a), build_key_spec(strategy, b));
    }
    assert forall|a: SAct, b: SAct| #[trigger] cmp_act_spec(strategy, a, b) == Ordering::Equal implies a.activation_seq
        == b.activation_seq && a.salience == b.salience && a.complexity == b.complexity by {
        lemma_key_equal(build_key_spec(strategy, a), build_key_spec(strategy, b));
    }
    assert forall|x: SAct| #[trigger] act_le(strategy)(x, x) by {
        lemma_key_refl(build_key_spec(strategy, x));
    }
    assert forall|x: SAct, y: SAct, z: SAct| #[trigger] act_le(strategy)(x, y) && #[trigger] act_le(
        strategy,
    )(y, z) implies act_le(strategy)(x, z) by {
        lemma_key_le_trans(
            build_key_spec(strategy, x),
            build_key_spec(strategy, y),
            build_key_spec(strategy, z),
        );
    }
    assert forall|x: SAct, y: SAct| #[trigger] act_le(strategy)(x, y) || #[trigger] act_le(
        strategy,
    )(y, x) by {
        lemma_key_flip(build_key_spec(strategy, x), build_key_spec(strategy, y));
    }
    assert forall|a: SAct, b: SAct| #[trigger] act_le(strategy)(a, b) && #[trigger] act_le(
        strategy,
    )(b, a) implies a.activation_seq == b.activation_seq by {
        lemma_key_flip(build_key_spec(strategy, a), build_key_spec(strategy, b));
        lemma_key_equal(build_key_spec(strategy, a), build_key_spec(strategy, b));
    }
}

/// COROLLARY. Restricted to activations with distinct sequence numbers (every
/// consistent agenda), each strategy's order is antisymmetric, hence a total
/// order.
pub proof fn corollary_activation_order_total_on_agenda(strategy: Strategy, acts: Set<SAct>)
    requires
        seqs_identify(acts),
    ensures
        forall|a: SAct, b: SAct|
            acts.contains(a) && acts.contains(b) && #[trigger] act_le(strategy)(a, b)
                && #[trigger] act_le(strategy)(b, a) ==> a == b,
{
    theorem_activation_order_is_total(strategy);
}

// =====================================================================
// Section 7. Documented semantic properties of each strategy
// =====================================================================

/// Facts about the descending sort of a recency vector.
pub proof fn lemma_sort_desc_props(s: Seq<u64>)
    ensures
        sorted_by(sort_desc_spec(s), desc_leq()),
        sort_desc_spec(s).to_multiset() == s.to_multiset(),
        sort_desc_spec(s).len() == s.len(),
        forall|x: u64|
            #![trigger sort_desc_spec(s).contains(x)]
            #![trigger s.contains(x)]
            sort_desc_spec(s).contains(x) <==> s.contains(x),
{
    broadcast use group_to_multiset_ensures;

    lemma_desc_leq_total();
    s.lemma_sort_by_ensures(desc_leq());
    let t = sort_desc_spec(s);
    assert(t.len() == t.to_multiset().len());
    assert(s.len() == s.to_multiset().len());
    assert forall|x: u64| t.contains(x) <==> s.contains(x) by {
        assert(t.to_multiset().count(x) == s.to_multiset().count(x));
    }
}

/// The LEX/MEA basis depends only on the multiset of recency tags, so the
/// order of patterns in a rule does not affect LEX ordering.
pub proof fn lemma_sort_desc_depends_only_on_multiset(x: Seq<u64>, y: Seq<u64>)
    requires
        x.to_multiset() == y.to_multiset(),
    ensures
        sort_desc_spec(x) == sort_desc_spec(y),
{
    lemma_sort_desc_props(x);
    lemma_sort_desc_props(y);
    lemma_desc_leq_total();
    lemma_sorted_unique(sort_desc_spec(x), sort_desc_spec(y), desc_leq());
}

/// The head of the sorted basis is the most recent tag of the activation.
pub proof fn lemma_sort_desc_head_is_max(s: Seq<u64>)
    requires
        s.len() > 0,
    ensures
        sort_desc_spec(s).len() > 0,
        s.contains(sort_desc_spec(s)[0]),
        forall|i: int| 0 <= i < s.len() ==> s[i] <= #[trigger] sort_desc_spec(s)[0],
{
    lemma_sort_desc_props(s);
    let t = sort_desc_spec(s);
    assert(t.contains(t[0]));
    assert forall|i: int| 0 <= i < s.len() implies s[i] <= #[trigger] sort_desc_spec(s)[0] by {
        assert(s.contains(s[i]));
        assert(t.contains(s[i]));
        let k = choose|k: int| 0 <= k < t.len() && t[k] == s[i];
        if k > 0 {
            assert(desc_leq()(t[0], t[k]));
        }
    }
}

/// A proper prefix compares `Less` (slice `Ord`).
pub proof fn lemma_cmp_seq_proper_prefix(x: Seq<u64>, y: Seq<u64>)
    requires
        x.len() < y.len(),
        forall|i: int| 0 <= i < x.len() ==> x[i] == y[i],
    ensures
        cmp_seq(x, y) == Ordering::Less,
    decreases x.len(),
{
    if x.len() > 0 {
        assert forall|i: int| 0 <= i < x.drop_first().len() implies x.drop_first()[i]
            == y.drop_first()[i] by {
            assert(x[i + 1] == y[i + 1]);
        }
        lemma_cmp_seq_proper_prefix(x.drop_first(), y.drop_first());
    }
}

/// crates/ferric-rules-core/src/strategy.rs:3 "Salience takes precedence for every strategy": higher
/// salience fires first whatever the recency, complexity or chronology.
pub proof fn theorem_salience_precedes_everything(strategy: Strategy, a: SAct, b: SAct)
    requires
        a.salience > b.salience,
    ensures
        cmp_act_spec(strategy, a, b) == Ordering::Less,
{
}

/// Depth (crates/ferric-rules-core/src/agenda.rs:120 "Newest activation first"): at equal salience the
/// newer activation fires first, and nothing else is consulted.
pub proof fn theorem_depth_newest_first(a: SAct, b: SAct)
    requires
        a.salience == b.salience,
        a.activation_seq != b.activation_seq,
    ensures
        cmp_act_spec(Strategy::Depth, a, b) == cmp_int(
            b.activation_seq as int,
            a.activation_seq as int,
        ),
{
}

/// Breadth (crates/ferric-rules-core/src/agenda.rs:121 "Oldest activation first").
pub proof fn theorem_breadth_oldest_first(a: SAct, b: SAct)
    requires
        a.salience == b.salience,
        a.activation_seq != b.activation_seq,
    ensures
        cmp_act_spec(Strategy::Breadth, a, b) == cmp_int(
            a.activation_seq as int,
            b.activation_seq as int,
        ),
{
}

/// Depth and Breadth ignore recency and complexity (crates/ferric-rules-core/src/agenda.rs:2479 test
/// `depth_and_breadth_ignore_complexity_and_recency`): replacing those
/// fields arbitrarily never changes the outcome between activations with
/// distinct sequence numbers.
pub proof fn theorem_depth_breadth_ignore_recency_and_complexity(
    strategy: Strategy,
    a: SAct,
    b: SAct,
    a2: SAct,
    b2: SAct,
)
    requires
        strategy is Depth || strategy is Breadth,
        a.activation_seq != b.activation_seq,
        a2.salience == a.salience && a2.activation_seq == a.activation_seq,
        b2.salience == b.salience && b2.activation_seq == b.activation_seq,
    ensures
        cmp_act_spec(strategy, a, b) == cmp_act_spec(strategy, a2, b2),
{
}

/// LEX/MEA: the order in which a rule's patterns are listed is irrelevant for
/// LEX, and for MEA once the first pattern's tag is fixed: only the multiset
/// of recency tags matters (crates/ferric-rules-core/src/agenda.rs:214-215 sorts the entire basis).
pub proof fn theorem_lex_mea_basis_order_irrelevant(strategy: Strategy, a: SAct, a2: SAct, b: SAct)
    requires
        strategy is Lex || strategy is Mea,
        a2.salience == a.salience,
        a2.complexity == a.complexity,
        a2.activation_seq == a.activation_seq,
        a2.recency.to_multiset() == a.recency.to_multiset(),
        strategy is Mea ==> first_or_absent(a2.recency) == first_or_absent(a.recency),
    ensures
        cmp_act_spec(strategy, a, b) == cmp_act_spec(strategy, a2, b),
        cmp_act_spec(strategy, b, a) == cmp_act_spec(strategy, b, a2),
{
    lemma_sort_desc_depends_only_on_multiset(a.recency, a2.recency);
}

/// LEX: at equal salience, an activation containing a fact more recent than
/// every fact of the other activation fires first (complexity and
/// chronology are not consulted).
pub proof fn theorem_lex_most_recent_fact_wins(a: SAct, b: SAct, i: int)
    requires
        a.salience == b.salience,
        0 <= i < a.recency.len(),
        forall|j: int| 0 <= j < b.recency.len() ==> b.recency[j] < a.recency[i],
    ensures
        cmp_act_spec(Strategy::Lex, a, b) == Ordering::Less,
{
    lemma_sort_desc_head_is_max(a.recency);
    lemma_sort_desc_props(b.recency);
    if b.recency.len() > 0 {
        lemma_sort_desc_head_is_max(b.recency);
        let sb = sort_desc_spec(b.recency);
        let j = choose|j: int| 0 <= j < b.recency.len() && b.recency[j] == sb[0];
        assert(b.recency[j] < a.recency[i]);
    }
}

/// LEX: when one sorted basis is a proper prefix of the other, the longer
/// one fires first (crates/ferric-rules-core/src/agenda.rs:2439 test
/// `lex_sorts_the_entire_basis_and_prefers_a_longer_equal_prefix`).
pub proof fn theorem_lex_longer_equal_prefix_wins(a: SAct, b: SAct)
    requires
        a.salience == b.salience,
        sort_desc_spec(b.recency).len() < sort_desc_spec(a.recency).len(),
        forall|i: int|
            0 <= i < sort_desc_spec(b.recency).len() ==> sort_desc_spec(b.recency)[i]
                == sort_desc_spec(a.recency)[i],
    ensures
        cmp_act_spec(Strategy::Lex, a, b) == Ordering::Less,
{
    lemma_cmp_seq_proper_prefix(sort_desc_spec(b.recency), sort_desc_spec(a.recency));
}

/// `a` is a proper prefix of `b`.
pub open spec fn proper_prefix(a: Seq<u64>, b: Seq<u64>) -> bool {
    a.len() < b.len() && forall|i: int| 0 <= i < a.len() ==> a[i] == b[i]
}

/// `a` and `b` first differ at index `k`, where `a` is smaller.
pub open spec fn first_difference_below(a: Seq<u64>, b: Seq<u64>, k: int) -> bool {
    &&& 0 <= k < a.len()
    &&& k < b.len()
    &&& a[k] < b[k]
    &&& forall|i: int| 0 <= i < k ==> a[i] == b[i]
}

/// Exact, non-recursive characterization of the slice order used for
/// recency vectors: `a < b` iff `a` is a proper prefix of `b`, or `a` is
/// smaller at the first index where they differ.
pub proof fn lemma_cmp_seq_characterization(a: Seq<u64>, b: Seq<u64>)
    ensures
        cmp_seq(a, b) == Ordering::Less <==> (proper_prefix(a, b) || exists|k: int|
            first_difference_below(a, b, k)),
    decreases a.len(),
{
    if a.len() == 0 {
        if b.len() == 0 {
            assert forall|k: int| !first_difference_below(a, b, k) by {}
        }
    } else if b.len() == 0 {
        assert forall|k: int| !first_difference_below(a, b, k) by {}
    } else if a[0] < b[0] {
        assert(first_difference_below(a, b, 0));
    } else if a[0] > b[0] {
        assert forall|k: int| !first_difference_below(a, b, k) by {
            if 0 < k && first_difference_below(a, b, k) {
                assert(a[0] == b[0]);
            }
        }
        assert(!proper_prefix(a, b));
    } else {
        let a1 = a.drop_first();
        let b1 = b.drop_first();
        lemma_cmp_seq_characterization(a1, b1);
        assert(proper_prefix(a, b) <==> proper_prefix(a1, b1)) by {
            if proper_prefix(a1, b1) {
                assert forall|i: int| 0 <= i < a.len() implies a[i] == b[i] by {
                    if i > 0 {
                        assert(a[i] == a1[i - 1]);
                        assert(b[i] == b1[i - 1]);
                    }
                }
            }
            if proper_prefix(a, b) {
                assert forall|i: int| 0 <= i < a1.len() implies a1[i] == b1[i] by {
                    assert(a[i + 1] == b[i + 1]);
                }
            }
        }
        if exists|k: int| first_difference_below(a1, b1, k) {
            let k = choose|k: int| first_difference_below(a1, b1, k);
            assert forall|i: int| 0 <= i < k + 1 implies a[i] == b[i] by {
                if i > 0 {
                    assert(a[i] == a1[i - 1]);
                    assert(b[i] == b1[i - 1]);
                }
            }
            assert(first_difference_below(a, b, k + 1));
        }
        if exists|k: int| first_difference_below(a, b, k) {
            let k = choose|k: int| first_difference_below(a, b, k);
            assert(k != 0);
            assert forall|i: int| 0 <= i < k - 1 implies a1[i] == b1[i] by {
                assert(a[i + 1] == b[i + 1]);
            }
            assert(first_difference_below(a1, b1, k - 1));
        }
    }
}

/// LEX, exact characterization (crates/ferric-rules-core/src/strategy.rs:4-5 "sorted fact recencies,
/// specificity, then older activations"): at equal salience, `a` fires
/// before `b` iff, comparing the descending-sorted bases, `b`'s is a proper
/// prefix of `a`'s, or `a`'s is larger at the first index where they differ;
/// or the bases are equal and `a` has higher complexity; or the bases and
/// complexities are equal and `a` is older.
pub proof fn theorem_lex_exact(a: SAct, b: SAct)
    requires
        a.salience == b.salience,
    ensures
        cmp_act_spec(Strategy::Lex, a, b) == Ordering::Less <==> {
            let sa = sort_desc_spec(a.recency);
            let sb = sort_desc_spec(b.recency);
            ||| proper_prefix(sb, sa)
            ||| (exists|k: int| first_difference_below(sb, sa, k))
            ||| (sa == sb && a.complexity > b.complexity)
            ||| (sa == sb && a.complexity == b.complexity && a.activation_seq
                < b.activation_seq)
        },
{
    let sa = sort_desc_spec(a.recency);
    let sb = sort_desc_spec(b.recency);
    lemma_cmp_seq_characterization(sb, sa);
    lemma_cmp_seq_equal(sb, sa);
    if sa == sb {
        assert(!proper_prefix(sb, sa));
        assert forall|k: int| !first_difference_below(sb, sa, k) by {}
    }
}

/// Position at which `t` enters a descending sequence: the first index whose
/// tag is below `t` (or the end).
pub open spec fn desc_insert_pos(x: Seq<u64>, t: u64) -> int
    decreases x.len(),
{
    if x.len() == 0 {
        0
    } else if x[0] < t {
        0
    } else {
        1 + desc_insert_pos(x.drop_first(), t)
    }
}

pub proof fn lemma_desc_insert_pos(x: Seq<u64>, t: u64)
    ensures
        0 <= desc_insert_pos(x, t) <= x.len(),
        forall|i: int| 0 <= i < desc_insert_pos(x, t) ==> x[i] >= t,
        desc_insert_pos(x, t) < x.len() ==> x[desc_insert_pos(x, t)] < t,
    decreases x.len(),
{
    if x.len() > 0 && x[0] >= t {
        lemma_desc_insert_pos(x.drop_first(), t);
        assert forall|i: int| 0 <= i < desc_insert_pos(x, t) implies x[i] >= t by {
            if i > 0 {
                assert(x[i] == x.drop_first()[i - 1]);
            }
        }
    }
}

/// Adding one tag to a basis makes its descending sort strictly larger in
/// the slice order (so LEX strictly prefers it).
pub proof fn lemma_sort_desc_push_is_larger(s: Seq<u64>, t: u64)
    ensures
        cmp_seq(sort_desc_spec(s), sort_desc_spec(s.push(t))) == Ordering::Less,
{
    broadcast use group_to_multiset_ensures;

    let x = sort_desc_spec(s);
    lemma_sort_desc_props(s);
    lemma_sort_desc_props(s.push(t));
    lemma_desc_insert_pos(x, t);
    let p = desc_insert_pos(x, t);
    let y = x.insert(p, t);
    // y is sorted descending.
    assert forall|i: int, j: int| 0 <= i < j < y.len() implies #[trigger] desc_leq()(y[i], y[j]) by {
        if j < p {
            assert(desc_leq()(x[i], x[j]));
        } else if j == p {
            assert(x[i] >= t);
        } else if i < p {
            assert(desc_leq()(x[i], x[j - 1]));
        } else if i == p {
            if j - 1 > p {
                assert(desc_leq()(x[p], x[j - 1]));
            }
        } else {
            assert(desc_leq()(x[i - 1], x[j - 1]));
        }
    }
    // ... and has the multiset of s.push(t), hence it is sort_desc(s.push(t)).
    to_multiset_insert(x, p, t);
    assert(y.to_multiset() == s.push(t).to_multiset());
    lemma_desc_leq_total();
    lemma_sorted_unique(y, sort_desc_spec(s.push(t)), desc_leq());
    // x < y in the slice order.
    lemma_cmp_seq_characterization(x, y);
    if p < x.len() {
        assert forall|i: int| 0 <= i < p implies x[i] == y[i] by {}
        assert(first_difference_below(x, y, p));
    } else {
        assert forall|i: int| 0 <= i < x.len() implies x[i] == y[i] by {}
        assert(proper_prefix(x, y));
    }
}

/// Adding any non-empty set of tags makes the descending sort strictly larger.
pub proof fn lemma_sort_desc_append_is_larger(s: Seq<u64>, extra: Seq<u64>)
    requires
        extra.len() > 0,
    ensures
        cmp_seq(sort_desc_spec(s), sort_desc_spec(s + extra)) == Ordering::Less,
    decreases extra.len(),
{
    let init = extra.drop_last();
    let t = extra.last();
    assert(s + extra =~= (s + init).push(t));
    lemma_sort_desc_push_is_larger(s + init, t);
    if init.len() == 0 {
        assert(s + init =~= s);
    } else {
        lemma_sort_desc_append_is_larger(s, init);
        lemma_cmp_seq_trans(sort_desc_spec(s), sort_desc_spec(s + init), sort_desc_spec(s + extra));
    }
}

/// LEX is monotone in the matched facts: at equal salience, an activation
/// whose basis is the other's basis plus at least one more tag (in any
/// order) fires first, whatever the complexities and chronology. The same
/// holds for MEA when the first patterns' tags agree.
pub proof fn theorem_lex_prefers_strictly_larger_basis(a: SAct, b: SAct, extra: Seq<u64>)
    requires
        a.salience == b.salience,
        extra.len() > 0,
        a.recency.to_multiset() == (b.recency + extra).to_multiset(),
    ensures
        cmp_act_spec(Strategy::Lex, a, b) == Ordering::Less,
        first_or_absent(a.recency) == first_or_absent(b.recency) ==> cmp_act_spec(
            Strategy::Mea,
            a,
            b,
        ) == Ordering::Less,
{
    lemma_sort_desc_depends_only_on_multiset(a.recency, b.recency + extra);
    lemma_sort_desc_append_is_larger(b.recency, extra);
}

/// LEX/MEA tie-breaks (crates/ferric-rules-core/src/strategy.rs:4-5, crates/ferric-rules-core/src/agenda.rs:140-143): with equal
/// salience and the same basis, higher complexity fires first, and at equal
/// complexity the older activation fires first.
pub proof fn theorem_lex_mea_specificity_then_chronology(strategy: Strategy, a: SAct, b: SAct)
    requires
        strategy is Lex || strategy is Mea,
        a.salience == b.salience,
        a.recency.to_multiset() == b.recency.to_multiset(),
        strategy is Mea ==> first_or_absent(a.recency) == first_or_absent(b.recency),
    ensures
        cmp_act_spec(strategy, a, b) == then(
            cmp_int(b.complexity as int, a.complexity as int),
            cmp_int(a.activation_seq as int, b.activation_seq as int),
        ),
{
    lemma_sort_desc_depends_only_on_multiset(a.recency, b.recency);
    lemma_cmp_seq_refl(sort_desc_spec(a.recency));
}

/// MEA (crates/ferric-rules-core/src/strategy.rs:17 "first-pattern recency, then the LEX comparison"):
/// at equal salience a more recent first pattern wins outright.
pub proof fn theorem_mea_first_pattern_dominates(a: SAct, b: SAct)
    requires
        a.salience == b.salience,
        first_or_absent(a.recency) > first_or_absent(b.recency),
    ensures
        cmp_act_spec(Strategy::Mea, a, b) == Ordering::Less,
{
}

/// MEA: when the first patterns tie, MEA orders exactly as LEX (crates/ferric-rules-core/src/agenda.rs:1124
/// test `mea_falls_back_to_lex_on_tie`). Holds for any saliences.
pub proof fn theorem_mea_tie_is_lex(a: SAct, b: SAct)
    requires
        first_or_absent(a.recency) == first_or_absent(b.recency),
    ensures
        cmp_act_spec(Strategy::Mea, a, b) == cmp_act_spec(Strategy::Lex, a, b),
{
}

/// Sequence rebasing (crates/ferric-rules-core/src/agenda.rs:276 `rebase_sequences`) renumbers live
/// activations by an order-preserving map; this never changes the outcome of
/// any comparison, under any strategy.
pub proof fn theorem_order_preserving_renumbering_preserves_order(
    strategy: Strategy,
    a: SAct,
    b: SAct,
    a2: SAct,
    b2: SAct,
)
    requires
        a2 == (SAct { activation_seq: a2.activation_seq, ..a }),
        b2 == (SAct { activation_seq: b2.activation_seq, ..b }),
        a.activation_seq < b.activation_seq <==> a2.activation_seq < b2.activation_seq,
        a.activation_seq == b.activation_seq <==> a2.activation_seq == b2.activation_seq,
    ensures
        cmp_act_spec(strategy, a, b) == cmp_act_spec(strategy, a2, b2),
{
}

// =====================================================================
// Section 8. The ordered collection: BTreeMap<AgendaKey, ActivationId>
// =====================================================================
//
// The production agenda keeps `ordering: BTreeMap<AgendaKey, ActivationId>`
// (crates/ferric-rules-core/src/agenda.rs:158). It is modelled by its in-order entry sequence. The
// B-tree's node structure is abstracted away; `map_insert`/`map_remove`
// model the search-then-update semantics of `BTreeMap::insert`/`remove`
// as a linear in-order search using the key order `cmp_key_spec`, and
// `pop_first` is `(s[0], s.drop_first())`.

pub type Entries = Seq<(SKey, nat)>;

/// BTreeMap structural invariant: in-order keys strictly increase.
pub open spec fn strictly_sorted(s: Entries) -> bool {
    forall|i: int, j: int|
        0 <= i < j < s.len() ==> #[trigger] cmp_key_spec(s[i].0, s[j].0) == Ordering::Less
}

pub open spec fn has_key(s: Entries, k: SKey) -> bool {
    exists|i: int| 0 <= i < s.len() && #[trigger] s[i].0 == k
}

/// `BTreeMap::insert(k, v)`: find the first entry whose key is not below `k`;
/// replace its value on an equal key, otherwise insert before it.
pub open spec fn map_insert(s: Entries, k: SKey, v: nat) -> Entries
    decreases s.len(),
{
    if s.len() == 0 {
        seq![(k, v)]
    } else {
        match cmp_key_spec(k, s[0].0) {
            Ordering::Less => seq![(k, v)] + s,
            Ordering::Equal => seq![(k, v)] + s.drop_first(),
            Ordering::Greater => seq![s[0]] + map_insert(s.drop_first(), k, v),
        }
    }
}

/// `BTreeMap::remove(&k)`: the search stops at the first key not below `k`.
pub open spec fn map_remove(s: Entries, k: SKey) -> Entries
    decreases s.len(),
{
    if s.len() == 0 {
        s
    } else {
        match cmp_key_spec(k, s[0].0) {
            Ordering::Less => s,
            Ordering::Equal => s.drop_first(),
            Ordering::Greater => seq![s[0]] + map_remove(s.drop_first(), k),
        }
    }
}

pub proof fn lemma_sorted_drop_first(s: Entries)
    requires
        strictly_sorted(s),
        s.len() > 0,
    ensures
        strictly_sorted(s.drop_first()),
{
    assert forall|i: int, j: int| 0 <= i < j < s.drop_first().len() implies #[trigger] cmp_key_spec(
        s.drop_first()[i].0,
        s.drop_first()[j].0,
    ) == Ordering::Less by {
        assert(cmp_key_spec(s[i + 1].0, s[j + 1].0) == Ordering::Less);
    }
}

/// Prepending a key below every key of a sorted sequence keeps it sorted.
pub proof fn lemma_sorted_prepend(e: (SKey, nat), s: Entries)
    requires
        strictly_sorted(s),
        forall|j: int| 0 <= j < s.len() ==> cmp_key_spec(e.0, #[trigger] s[j].0) == Ordering::Less,
    ensures
        strictly_sorted(seq![e] + s),
{
    let t = seq![e] + s;
    assert forall|i: int, j: int| 0 <= i < j < t.len() implies #[trigger] cmp_key_spec(
        t[i].0,
        t[j].0,
    ) == Ordering::Less by {
        if i == 0 {
            assert(t[j] == s[j - 1]);
        } else {
            assert(t[i] == s[i - 1]);
            assert(t[j] == s[j - 1]);
            assert(cmp_key_spec(s[i - 1].0, s[j - 1].0) == Ordering::Less);
        }
    }
}

/// In a strictly sorted sequence, the first entry precedes all others, and
/// keys are pairwise distinct.
pub proof fn lemma_sorted_first_is_min(s: Entries)
    requires
        strictly_sorted(s),
    ensures
        forall|j: int| 0 < j < s.len() ==> cmp_key_spec(s[0].0, #[trigger] s[j].0) == Ordering::Less,
        forall|i: int, j: int|
            0 <= i < s.len() && 0 <= j < s.len() && i != j ==> #[trigger] s[i].0 != #[trigger] s[j].0,
{
    assert forall|i: int, j: int| 0 <= i < s.len() && 0 <= j < s.len() && i != j implies #[trigger] s[i].0
        != #[trigger] s[j].0 by {
        lemma_key_refl(s[i].0);
        if i < j {
            assert(cmp_key_spec(s[i].0, s[j].0) == Ordering::Less);
        } else {
            assert(cmp_key_spec(s[j].0, s[i].0) == Ordering::Less);
        }
    }
    assert forall|j: int| 0 < j < s.len() implies cmp_key_spec(s[0].0, #[trigger] s[j].0)
        == Ordering::Less by {
        assert(cmp_key_spec(s[0].0, s[j].0) == Ordering::Less);
    }
}

/// The postcondition of `map_insert` (sortedness and exact contents).
pub open spec fn map_insert_post(s: Entries, k: SKey, v: nat, r: Entries) -> bool {
    &&& strictly_sorted(r)
    &&& forall|e: (SKey, nat)| #[trigger] r.contains(e) <==> (e == (k, v) || (s.contains(e) && e.0 != k))
    &&& !has_key(s, k) ==> r.len() == s.len() + 1
}

/// `map_insert`, case `k` below the first key: `(k, v)` is prepended.
pub proof fn lemma_map_insert_less(s: Entries, k: SKey, v: nat)
    requires
        strictly_sorted(s),
        s.len() > 0,
        cmp_key_spec(k, s[0].0) == Ordering::Less,
    ensures
        map_insert_post(s, k, v, seq![(k, v)] + s),
{
    let r = seq![(k, v)] + s;
    assert forall|j: int| 0 <= j < s.len() implies cmp_key_spec(k, #[trigger] s[j].0)
        == Ordering::Less by {
        if j > 0 {
            assert(cmp_key_spec(s[0].0, s[j].0) == Ordering::Less);
            lemma_key_trans(k, s[0].0, s[j].0);
        }
    }
    lemma_sorted_prepend((k, v), s);
    lemma_key_refl(k);
    assert forall|e: (SKey, nat)| #[trigger] r.contains(e) <==> (e == (k, v) || (s.contains(e)
        && e.0 != k)) by {
        if r.contains(e) {
            let i = choose|i: int| 0 <= i < r.len() && r[i] == e;
            if i > 0 {
                assert(s[i - 1] == e);
            }
        }
        if e == (k, v) {
            assert(r[0] == e);
        }
        if s.contains(e) && e.0 != k {
            let i = choose|i: int| 0 <= i < s.len() && s[i] == e;
            assert(r[i + 1] == e);
        }
    }
    assert(!has_key(s, k));
}

/// `map_insert`, case `k` equal to the first key: its value is replaced.
pub proof fn lemma_map_insert_equal(s: Entries, k: SKey, v: nat)
    requires
        strictly_sorted(s),
        s.len() > 0,
        cmp_key_spec(k, s[0].0) == Ordering::Equal,
    ensures
        map_insert_post(s, k, v, seq![(k, v)] + s.drop_first()),
{
    let rest = s.drop_first();
    let r = seq![(k, v)] + rest;
    lemma_key_equal(k, s[0].0);
    lemma_sorted_drop_first(s);
    assert forall|j: int| 0 <= j < rest.len() implies cmp_key_spec(k, #[trigger] rest[j].0)
        == Ordering::Less by {
        assert(rest[j] == s[j + 1]);
        assert(cmp_key_spec(s[0].0, s[j + 1].0) == Ordering::Less);
    }
    lemma_sorted_prepend((k, v), rest);
    lemma_sorted_first_is_min(s);
    assert forall|e: (SKey, nat)| #[trigger] r.contains(e) <==> (e == (k, v) || (s.contains(e)
        && e.0 != k)) by {
        if r.contains(e) {
            let i = choose|i: int| 0 <= i < r.len() && r[i] == e;
            if i > 0 {
                assert(rest[i - 1] == e);
                assert(s[i] == e);
            }
        }
        if e == (k, v) {
            assert(r[0] == e);
        }
        if s.contains(e) && e.0 != k {
            let i = choose|i: int| 0 <= i < s.len() && s[i] == e;
            assert(i != 0);
            assert(r[i] == e);
        }
    }
    assert(s[0].0 == k);
    assert(has_key(s, k));
}

/// `map_insert`, case `k` above the first key: the first entry is kept and
/// the insertion proceeds in the tail (whose result `r2` is given).
pub proof fn lemma_map_insert_greater(s: Entries, k: SKey, v: nat, r2: Entries)
    requires
        strictly_sorted(s),
        s.len() > 0,
        cmp_key_spec(k, s[0].0) == Ordering::Greater,
        map_insert_post(s.drop_first(), k, v, r2),
    ensures
        map_insert_post(s, k, v, seq![s[0]] + r2),
{
    let s0 = s[0];
    let rest = s.drop_first();
    let r = seq![s0] + r2;
    lemma_key_flip(k, s0.0);
    lemma_key_equal(k, s0.0);
    assert forall|j: int| 0 <= j < r2.len() implies cmp_key_spec(s0.0, #[trigger] r2[j].0)
        == Ordering::Less by {
        assert(r2.contains(r2[j]));
        if r2[j] != (k, v) {
            let i = choose|i: int| 0 <= i < rest.len() && rest[i] == r2[j];
            assert(rest[i] == s[i + 1]);
            assert(cmp_key_spec(s[0].0, s[i + 1].0) == Ordering::Less);
        }
    }
    lemma_sorted_prepend(s0, r2);
    assert forall|e: (SKey, nat)| #[trigger] r.contains(e) <==> (e == (k, v) || (s.contains(e)
        && e.0 != k)) by {
        if r.contains(e) {
            let i = choose|i: int| 0 <= i < r.len() && r[i] == e;
            if i > 0 {
                assert(r2[i - 1] == e);
                assert(r2.contains(e));
                if e != (k, v) {
                    let m = choose|m: int| 0 <= m < rest.len() && rest[m] == e;
                    assert(s[m + 1] == e);
                }
            } else {
                assert(s[0] == e);
            }
        }
        if e == (k, v) {
            assert(r2.contains(e));
            let m = choose|m: int| 0 <= m < r2.len() && r2[m] == e;
            assert(r[m + 1] == e);
        }
        if s.contains(e) && e.0 != k {
            let i = choose|i: int| 0 <= i < s.len() && s[i] == e;
            if i == 0 {
                assert(r[0] == e);
            } else {
                assert(rest[i - 1] == e);
                assert(rest.contains(e));
                assert(r2.contains(e));
                let m = choose|m: int| 0 <= m < r2.len() && r2[m] == e;
                assert(r[m + 1] == e);
            }
        }
    }
    if !has_key(s, k) {
        assert forall|i: int| 0 <= i < rest.len() implies #[trigger] rest[i].0 != k by {
            assert(rest[i] == s[i + 1]);
        }
        assert(!has_key(rest, k));
    }
}

/// `map_insert` keeps the sequence sorted and updates exactly the entry for `k`.
pub proof fn lemma_map_insert(s: Entries, k: SKey, v: nat)
    requires
        strictly_sorted(s),
    ensures
        strictly_sorted(map_insert(s, k, v)),
        forall|e: (SKey, nat)| #[trigger]
            map_insert(s, k, v).contains(e) <==> (e == (k, v) || (s.contains(e) && e.0 != k)),
        !has_key(s, k) ==> map_insert(s, k, v).len() == s.len() + 1,
    decreases s.len(),
{
    let r = map_insert(s, k, v);
    if s.len() == 0 {
        assert(r =~= seq![(k, v)]);
        assert forall|e: (SKey, nat)| #[trigger] r.contains(e) <==> (e == (k, v) || (s.contains(e)
            && e.0 != k)) by {
            if r.contains(e) {
                let i = choose|i: int| 0 <= i < r.len() && r[i] == e;
                assert(i == 0);
            }
            if e == (k, v) {
                assert(r[0] == e);
            }
        }
    } else {
        match cmp_key_spec(k, s[0].0) {
            Ordering::Less => lemma_map_insert_less(s, k, v),
            Ordering::Equal => lemma_map_insert_equal(s, k, v),
            Ordering::Greater => {
                lemma_sorted_drop_first(s);
                lemma_map_insert(s.drop_first(), k, v);
                lemma_map_insert_greater(s, k, v, map_insert(s.drop_first(), k, v));
            },
        }
        assert(map_insert_post(s, k, v, r));
    }
}


/// `map_remove` keeps the sequence sorted and deletes exactly the entry for `k`.
pub proof fn lemma_map_remove(s: Entries, k: SKey)
    requires
        strictly_sorted(s),
    ensures
        strictly_sorted(map_remove(s, k)),
        forall|e: (SKey, nat)| #[trigger]
            map_remove(s, k).contains(e) <==> (s.contains(e) && e.0 != k),
    decreases s.len(),
{
    let r = map_remove(s, k);
    if s.len() > 0 {
        let s0 = s[0];
        let rest = s.drop_first();
        lemma_sorted_drop_first(s);
        lemma_sorted_first_is_min(s);
        lemma_key_equal(k, s0.0);
        lemma_key_flip(k, s0.0);
        match cmp_key_spec(k, s0.0) {
            Ordering::Less => {
                assert forall|e: (SKey, nat)| #[trigger] r.contains(e) <==> (s.contains(e) && e.0
                    != k) by {
                    if s.contains(e) {
                        let i = choose|i: int| 0 <= i < s.len() && s[i] == e;
                        if i > 0 {
                            lemma_key_trans(k, s0.0, s[i].0);
                        }
                        lemma_key_refl(k);
                    }
                }
            },
            Ordering::Equal => {
                assert forall|e: (SKey, nat)| #[trigger] r.contains(e) <==> (s.contains(e) && e.0
                    != k) by {
                    if r.contains(e) {
                        let i = choose|i: int| 0 <= i < r.len() && r[i] == e;
                        assert(s[i + 1] == e);
                    }
                    if s.contains(e) && e.0 != k {
                        let i = choose|i: int| 0 <= i < s.len() && s[i] == e;
                        assert(r[i - 1] == e);
                    }
                }
            },
            Ordering::Greater => {
                lemma_map_remove(rest, k);
                let r2 = map_remove(rest, k);
                assert(r == seq![s0] + r2);
                assert forall|j: int| 0 <= j < r2.len() implies cmp_key_spec(s0.0, #[trigger] r2[j].0)
                    == Ordering::Less by {
                    assert(r2.contains(r2[j]));
                    let i = choose|i: int| 0 <= i < rest.len() && rest[i] == r2[j];
                    assert(rest[i] == s[i + 1]);
                }
                lemma_sorted_prepend(s0, r2);
                assert forall|e: (SKey, nat)| #[trigger] r.contains(e) <==> (s.contains(e) && e.0
                    != k) by {
                    if r.contains(e) {
                        let i = choose|i: int| 0 <= i < r.len() && r[i] == e;
                        if i > 0 {
                            assert(r2[i - 1] == e);
                            assert(r2.contains(e));
                            let m = choose|m: int| 0 <= m < rest.len() && rest[m] == e;
                            assert(s[m + 1] == e);
                        } else {
                            assert(s[0] == e);
                        }
                    }
                    if s.contains(e) && e.0 != k {
                        let i = choose|i: int| 0 <= i < s.len() && s[i] == e;
                        if i == 0 {
                            assert(r[0] == e);
                        } else {
                            assert(rest[i - 1] == e);
                            assert(rest.contains(e));
                            assert(r2.contains(e));
                            let m = choose|m: int| 0 <= m < r2.len() && r2[m] == e;
                            assert(r[m + 1] == e);
                        }
                    }
                }
            },
        }
    }
}


// =====================================================================
// Section 9. The agenda
// =====================================================================

/// Models `Agenda` (crates/ferric-rules-core/src/agenda.rs:152-170). Ids are `nat`s (SlotMap keys);
/// `rule_ordering`, the lazily built per-rule index, is not modelled.
pub struct AgendaModel {
    /// `ordering: BTreeMap<AgendaKey, ActivationId>` as its in-order entries.
    pub ordering: Entries,
    /// `activations: SlotMap<ActivationId, Activation>`.
    pub activations: Map<nat, SAct>,
    /// `id_to_key: SecondaryMap<ActivationId, AgendaKey>`.
    pub id_to_key: Map<nat, SKey>,
    /// `token_to_activations: FxHashMap<TokenId, SmallVec<[ActivationId; 2]>>`.
    pub token_to_activations: Map<nat, Seq<nat>>,
    /// `next_seq: ActivationSeq`.
    pub next_seq: u64,
    /// `strategy: ConflictResolutionStrategy`.
    pub strategy: Strategy,
}

/// Ordering-index invariant: `validate_consistency` checks 1, 2 and the
/// first half of 4 (crates/ferric-rules-core/src/agenda.rs:542-578, 607-620) plus the BTreeMap's own
/// structural invariant (strictly increasing in-order keys).
pub open spec fn ordering_inv(ag: AgendaModel) -> bool {
    &&& strictly_sorted(ag.ordering)
    &&& forall|e: (SKey, nat)| #[trigger]
        ag.ordering.contains(e) <==> (ag.id_to_key.contains_key(e.1) && ag.id_to_key[e.1] == e.0)
    &&& forall|id: nat| #[trigger]
        ag.id_to_key.contains_key(id) <==> ag.activations.contains_key(id)
}

/// Activation invariant: the identity / sequence / key checks of
/// `validate_consistency` (crates/ferric-rules-core/src/agenda.rs:633-649).
pub open spec fn activation_inv(ag: AgendaModel) -> bool {
    &&& forall|id: nat| #[trigger]
        ag.activations.contains_key(id) ==> {
            &&& ag.activations[id].id == id
            &&& ag.activations[id].activation_seq < ag.next_seq
            &&& ag.id_to_key[id] == build_key_spec(ag.strategy, ag.activations[id])
        }
    &&& forall|id1: nat, id2: nat|
        #[trigger] ag.activations.contains_key(id1) && #[trigger] ag.activations.contains_key(id2)
            && ag.activations[id1].activation_seq == ag.activations[id2].activation_seq ==> id1
            == id2
}

/// Token-index invariant for the entries of every token, `validate_consistency`
/// check 3 (crates/ferric-rules-core/src/agenda.rs:580-605).
pub open spec fn token_entries_inv(ag: AgendaModel) -> bool {
    forall|t: nat| #[trigger]
        ag.token_to_activations.contains_key(t) ==> {
            &&& ag.token_to_activations[t].len() > 0
            &&& ag.token_to_activations[t].no_duplicates()
            &&& forall|j: int|
                0 <= j < ag.token_to_activations[t].len() ==> ag.activations.contains_key(
                    #[trigger] ag.token_to_activations[t][j],
                ) && ag.activations[ag.token_to_activations[t][j]].token == t
        }
}

/// Every live activation (whose token is not `except`) is indexed under its
/// token: the second half of check 4 (crates/ferric-rules-core/src/agenda.rs:622-631).
pub open spec fn token_cover_inv_except(ag: AgendaModel, except: Option<nat>) -> bool {
    forall|id: nat| #[trigger]
        ag.activations.contains_key(id) && except != Some(ag.activations[id].token) ==> {
            &&& ag.token_to_activations.contains_key(ag.activations[id].token)
            &&& ag.token_to_activations[ag.activations[id].token].contains(id)
        }
}

/// The full consistency invariant (`Agenda::validate_consistency`, crates/ferric-rules-core/src/agenda.rs:541).
pub open spec fn inv(ag: AgendaModel) -> bool {
    &&& ordering_inv(ag)
    &&& activation_inv(ag)
    &&& token_entries_inv(ag)
    &&& token_cover_inv_except(ag, None)
}

/// Equal keys identify activations (keys embed the unique sequence number).
pub proof fn lemma_keys_identify(ag: AgendaModel, id1: nat, id2: nat)
    requires
        activation_inv(ag),
        ag.activations.contains_key(id1),
        ag.activations.contains_key(id2),
        ag.id_to_key[id1] == ag.id_to_key[id2],
    ensures
        id1 == id2,
{
    assert(ag.id_to_key[id1].seq == ag.activations[id1].activation_seq);
    assert(ag.id_to_key[id2].seq == ag.activations[id2].activation_seq);
}

// ---------------------------------------------------------------------
// Token-index helpers
// ---------------------------------------------------------------------

pub open spec fn ne(aid: nat) -> spec_fn(nat) -> bool {
    |x: nat| x != aid
}

/// Models `remove_from_token_index` (crates/ferric-rules-core/src/agenda.rs:81-95): `retain` the other ids,
/// dropping the entry once it is empty.
pub open spec fn remove_from_token_index(idx: Map<nat, Seq<nat>>, token: nat, aid: nat) -> Map<
    nat,
    Seq<nat>,
> {
    if idx.contains_key(token) {
        let acts = idx[token].filter(ne(aid));
        if acts.len() == 0 {
            idx.remove(token)
        } else {
            idx.insert(token, acts)
        }
    } else {
        idx
    }
}

pub proof fn lemma_filter_ne(s: Seq<nat>, aid: nat)
    ensures
        forall|x: nat| #[trigger] s.filter(ne(aid)).contains(x) <==> (s.contains(x) && x != aid),
        s.no_duplicates() ==> s.filter(ne(aid)).no_duplicates(),
    decreases s.len(),
{
    broadcast use group_filter_ensures;

    let f = s.filter(ne(aid));
    assert forall|x: nat| #[trigger] f.contains(x) <==> (s.contains(x) && x != aid) by {
        if f.contains(x) {
            s.lemma_filter_contains_rev(ne(aid), x);
            let j = choose|j: int| 0 <= j < f.len() && f[j] == x;
            assert(ne(aid)(f[j]));
        }
        if s.contains(x) && x != aid {
            let i = choose|i: int| 0 <= i < s.len() && s[i] == x;
            assert(ne(aid)(s[i]));
            assert(f.contains(s[i]));
        }
    }
    if s.len() > 0 && s.no_duplicates() {
        let init = s.drop_last();
        let last = s.last();
        assert(s =~= init.push(last));
        assert(init.no_duplicates());
        lemma_filter_ne(init, aid);
        init.lemma_filter_push(last, ne(aid));
        if last != aid {
            assert(!init.contains(last));
            assert(!init.filter(ne(aid)).contains(last));
            let g = init.filter(ne(aid));
            assert(f == g.push(last));
            assert forall|i: int, j: int|
                0 <= i < f.len() && 0 <= j < f.len() && i != j implies f[i] != f[j] by {
                if i == f.len() - 1 {
                    assert(g.contains(f[j]));
                } else if j == f.len() - 1 {
                    assert(g.contains(f[i]));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// add (crates/ferric-rules-core/src/agenda.rs:245)
// ---------------------------------------------------------------------

/// The activation as stored by `add`: fresh SlotMap key, next sequence number.
pub open spec fn added_activation(ag: AgendaModel, activation: SAct, new_id: nat) -> SAct {
    SAct { id: new_id, activation_seq: ag.next_seq, ..activation }
}

/// Models `Agenda::add` (crates/ferric-rules-core/src/agenda.rs:245-273) on the path without
/// `rebase_sequences` (`next_seq < u64::MAX`). `new_id` is the key SlotMap
/// returns, which is never the key of a live entry.
pub open spec fn add_spec(ag: AgendaModel, activation: SAct, new_id: nat) -> AgendaModel {
    let act = added_activation(ag, activation, new_id);
    let key = build_key_spec(ag.strategy, act);
    let prior = if ag.token_to_activations.contains_key(act.token) {
        ag.token_to_activations[act.token]
    } else {
        Seq::empty()
    };
    AgendaModel {
        ordering: map_insert(ag.ordering, key, new_id),
        activations: ag.activations.insert(new_id, act),
        id_to_key: ag.id_to_key.insert(new_id, key),
        token_to_activations: ag.token_to_activations.insert(act.token, prior.push(new_id)),
        next_seq: (ag.next_seq + 1) as u64,
        strategy: ag.strategy,
    }
}

/// THEOREM (add). `add` preserves the agenda invariant, stores the
/// activation under a fresh key, and the new activation is strictly newer
/// than every live one.
pub proof fn theorem_add_preserves_inv(ag: AgendaModel, activation: SAct, new_id: nat)
    requires
        inv(ag),
        ag.next_seq < u64::MAX,
        !ag.activations.contains_key(new_id),
    ensures
        inv(add_spec(ag, activation, new_id)),
        add_spec(ag, activation, new_id).activations == ag.activations.insert(
            new_id,
            added_activation(ag, activation, new_id),
        ),
        forall|id: nat| #[trigger]
            ag.activations.contains_key(id) ==> ag.activations[id].activation_seq
                < added_activation(ag, activation, new_id).activation_seq,
{
    let act = added_activation(ag, activation, new_id);
    let key = build_key_spec(ag.strategy, act);
    let ag2 = add_spec(ag, activation, new_id);
    lemma_map_insert(ag.ordering, key, new_id);
    // The new key is absent: every existing key carries an older sequence.
    assert forall|e: (SKey, nat)| #[trigger] ag.ordering.contains(e) implies e.0 != key by {
        assert(ag.id_to_key.contains_key(e.1));
        assert(ag.activations.contains_key(e.1));
    }
    // ordering_inv
    assert forall|e: (SKey, nat)| #[trigger]
        ag2.ordering.contains(e) <==> (ag2.id_to_key.contains_key(e.1) && ag2.id_to_key[e.1]
            == e.0) by {
        if ag.ordering.contains(e) {
            assert(ag.id_to_key.contains_key(e.1));
            assert(e.1 != new_id);
        }
        if ag2.id_to_key.contains_key(e.1) && ag2.id_to_key[e.1] == e.0 && e.1 != new_id {
            assert(ag.ordering.contains(e));
        }
    }
    assert forall|id: nat| #[trigger]
        ag2.id_to_key.contains_key(id) <==> ag2.activations.contains_key(id) by {
        assert(ag.id_to_key.contains_key(id) <==> ag.activations.contains_key(id));
    }
    // activation_inv
    assert forall|id1: nat, id2: nat|
        #[trigger] ag2.activations.contains_key(id1) && #[trigger] ag2.activations.contains_key(id2)
            && ag2.activations[id1].activation_seq == ag2.activations[id2].activation_seq implies id1
        == id2 by {
        if id1 != new_id && id2 != new_id {
            assert(ag.activations.contains_key(id1) && ag.activations.contains_key(id2));
        }
    }
    // token_entries_inv
    let prior = if ag.token_to_activations.contains_key(act.token) {
        ag.token_to_activations[act.token]
    } else {
        Seq::<nat>::empty()
    };
    assert forall|j: int| 0 <= j < prior.len() implies prior[j] != new_id && ag.activations.contains_key(
        #[trigger] prior[j],
    ) && ag.activations[prior[j]].token == act.token by {
        assert(ag.token_to_activations.contains_key(act.token));
    }
    assert forall|t: nat| #[trigger] ag2.token_to_activations.contains_key(t) implies {
        &&& ag2.token_to_activations[t].len() > 0
        &&& ag2.token_to_activations[t].no_duplicates()
        &&& forall|j: int|
            0 <= j < ag2.token_to_activations[t].len() ==> ag2.activations.contains_key(
                #[trigger] ag2.token_to_activations[t][j],
            ) && ag2.activations[ag2.token_to_activations[t][j]].token == t
    } by {
        if t == act.token {
            let s = prior.push(new_id);
            assert(ag2.token_to_activations[t] == s);
            assert forall|i: int, j: int| 0 <= i < s.len() && 0 <= j < s.len() && i != j implies s[i]
                != s[j] by {
                if i < prior.len() && j < prior.len() {
                    assert(ag.token_to_activations.contains_key(act.token));
                }
            }
            assert forall|j: int| 0 <= j < s.len() implies ag2.activations.contains_key(
                #[trigger] s[j],
            ) && ag2.activations[s[j]].token == t by {
                if j < prior.len() {
                    assert(s[j] == prior[j]);
                }
            }
        } else {
            assert(ag.token_to_activations.contains_key(t));
            let s = ag.token_to_activations[t];
            assert forall|j: int| 0 <= j < s.len() implies ag2.activations.contains_key(
                #[trigger] s[j],
            ) && ag2.activations[s[j]].token == t by {
                assert(ag.activations.contains_key(s[j]));
            }
        }
    }
    // token_cover_inv
    assert forall|id: nat| #[trigger]
        ag2.activations.contains_key(id) && None::<nat> != Some(ag2.activations[id].token) implies {
        &&& ag2.token_to_activations.contains_key(ag2.activations[id].token)
        &&& ag2.token_to_activations[ag2.activations[id].token].contains(id)
    } by {
        if id == new_id {
            assert(ag2.token_to_activations[act.token] == prior.push(new_id));
            assert(prior.push(new_id)[prior.len() as int] == new_id);
        } else {
            assert(ag.activations.contains_key(id));
            let t = ag.activations[id].token;
            assert(ag.token_to_activations[t].contains(id));
            if t == act.token {
                let j = choose|j: int|
                    0 <= j < ag.token_to_activations[t].len() && ag.token_to_activations[t][j]
                        == id;
                assert(prior.push(new_id)[j] == id);
            }
        }
    }
}

// ---------------------------------------------------------------------
// Removing one activation (shared by pop, pop_matching and
// remove_activations_for_rule)
// ---------------------------------------------------------------------

/// The ordering/activation part of removing the entry `(key, id)`:
/// `ordering.remove(&key)` (or `pop_first`), `id_to_key.remove(id)`,
/// `activations.remove(id)`.
pub open spec fn remove_core(ag: AgendaModel, key: SKey, id: nat) -> AgendaModel {
    AgendaModel {
        ordering: map_remove(ag.ordering, key),
        id_to_key: ag.id_to_key.remove(id),
        activations: ag.activations.remove(id),
        ..ag
    }
}

/// `remove_core` followed by `remove_from_token_index` for the removed
/// activation's token.
pub open spec fn remove_entry_spec(ag: AgendaModel, key: SKey, id: nat) -> AgendaModel {
    AgendaModel {
        token_to_activations: remove_from_token_index(
            ag.token_to_activations,
            ag.activations[id].token,
            id,
        ),
        ..remove_core(ag, key, id)
    }
}

pub proof fn lemma_remove_core(ag: AgendaModel, id: nat)
    requires
        ordering_inv(ag),
        activation_inv(ag),
        ag.activations.contains_key(id),
    ensures
        ordering_inv(remove_core(ag, ag.id_to_key[id], id)),
        activation_inv(remove_core(ag, ag.id_to_key[id], id)),
{
    let key = ag.id_to_key[id];
    let ag2 = remove_core(ag, key, id);
    lemma_map_remove(ag.ordering, key);
    assert forall|e: (SKey, nat)| #[trigger]
        ag2.ordering.contains(e) <==> (ag2.id_to_key.contains_key(e.1) && ag2.id_to_key[e.1]
            == e.0) by {
        if ag.ordering.contains(e) && e.0 != key {
            assert(ag.id_to_key.contains_key(e.1));
        }
        if ag2.id_to_key.contains_key(e.1) && ag2.id_to_key[e.1] == e.0 {
            assert(ag.ordering.contains(e));
            if e.0 == key {
                assert(ag.activations.contains_key(e.1));
                lemma_keys_identify(ag, e.1, id);
            }
        }
    }
    assert forall|id1: nat, id2: nat|
        #[trigger] ag2.activations.contains_key(id1) && #[trigger] ag2.activations.contains_key(id2)
            && ag2.activations[id1].activation_seq == ag2.activations[id2].activation_seq implies id1
        == id2 by {
        assert(ag.activations.contains_key(id1) && ag.activations.contains_key(id2));
    }
}

/// THEOREM (removal). Removing one live activation the way `pop`,
/// `pop_matching` and `remove_activations_for_rule` do preserves the
/// invariant and removes exactly that activation.
pub proof fn lemma_remove_entry(ag: AgendaModel, id: nat)
    requires
        inv(ag),
        ag.activations.contains_key(id),
    ensures
        inv(remove_entry_spec(ag, ag.id_to_key[id], id)),
        remove_entry_spec(ag, ag.id_to_key[id], id).activations == ag.activations.remove(id),
{
    let key = ag.id_to_key[id];
    let ag2 = remove_entry_spec(ag, key, id);
    lemma_remove_core(ag, id);
    let tok = ag.activations[id].token;
    let idx = ag.token_to_activations;
    assert(idx.contains_key(tok));
    lemma_filter_ne(idx[tok], id);
    let kept = idx[tok].filter(ne(id));
    assert forall|t: nat| #[trigger] ag2.token_to_activations.contains_key(t) implies {
        &&& ag2.token_to_activations[t].len() > 0
        &&& ag2.token_to_activations[t].no_duplicates()
        &&& forall|j: int|
            0 <= j < ag2.token_to_activations[t].len() ==> ag2.activations.contains_key(
                #[trigger] ag2.token_to_activations[t][j],
            ) && ag2.activations[ag2.token_to_activations[t][j]].token == t
    } by {
        if t == tok {
            assert(ag2.token_to_activations[t] == kept);
            assert forall|j: int| 0 <= j < kept.len() implies ag2.activations.contains_key(
                #[trigger] kept[j],
            ) && ag2.activations[kept[j]].token == t by {
                assert(kept.contains(kept[j]));
                assert(idx[tok].contains(kept[j]));
                let m = choose|m: int| 0 <= m < idx[tok].len() && idx[tok][m] == kept[j];
                assert(ag.activations.contains_key(idx[tok][m]));
            }
        } else {
            let s = idx[t];
            assert(idx.contains_key(t));
            assert forall|j: int| 0 <= j < s.len() implies ag2.activations.contains_key(
                #[trigger] s[j],
            ) && ag2.activations[s[j]].token == t by {
                assert(ag.activations.contains_key(s[j]));
            }
        }
    }
    assert forall|other: nat| #[trigger]
        ag2.activations.contains_key(other) && None::<nat> != Some(
            ag2.activations[other].token,
        ) implies {
        &&& ag2.token_to_activations.contains_key(ag2.activations[other].token)
        &&& ag2.token_to_activations[ag2.activations[other].token].contains(other)
    } by {
        assert(ag.activations.contains_key(other));
        let t = ag.activations[other].token;
        assert(idx[t].contains(other));
        if t == tok {
            assert(kept.contains(other));
        }
    }
}

// ---------------------------------------------------------------------
// pop (crates/ferric-rules-core/src/agenda.rs:304)
// ---------------------------------------------------------------------

/// Models `Agenda::pop` (crates/ferric-rules-core/src/agenda.rs:304-317): `ordering.pop_first()`, then
/// the reverse indices.
pub open spec fn pop_spec(ag: AgendaModel) -> (AgendaModel, Option<SAct>) {
    if ag.ordering.len() == 0 {
        (ag, None)
    } else {
        let key = ag.ordering[0].0;
        let id = ag.ordering[0].1;
        let activation = ag.activations[id];
        (
            AgendaModel {
                ordering: ag.ordering.drop_first(),
                id_to_key: ag.id_to_key.remove(id),
                activations: ag.activations.remove(id),
                token_to_activations: remove_from_token_index(
                    ag.token_to_activations,
                    activation.token,
                    id,
                ),
                next_seq: ag.next_seq,
                strategy: ag.strategy,
            },
            Some(activation),
        )
    }
}

/// THEOREM (pop). `pop` preserves the invariant; it returns `None` exactly
/// when the agenda is empty; otherwise it returns a live activation that
/// strictly precedes every other live activation under the agenda's
/// conflict-resolution strategy (the maximum-priority activation) and
/// removes exactly that activation.
pub proof fn theorem_pop(ag: AgendaModel)
    requires
        inv(ag),
    ensures
        inv(pop_spec(ag).0),
        pop_spec(ag).1 is None <==> ag.activations.dom() =~= Set::<nat>::empty(),
        pop_spec(ag).1 is None ==> pop_spec(ag).0 == ag,
        pop_spec(ag).1 matches Some(a) ==> {
            &&& ag.activations.contains_key(a.id)
            &&& ag.activations[a.id] == a
            &&& pop_spec(ag).0.activations == ag.activations.remove(a.id)
            &&& forall|id: nat| #[trigger]
                ag.activations.contains_key(id) && id != a.id ==> cmp_act_spec(
                    ag.strategy,
                    a,
                    ag.activations[id],
                ) == Ordering::Less
        },
{
    if ag.ordering.len() == 0 {
        assert forall|id: nat| !ag.activations.contains_key(id) by {
            if ag.activations.contains_key(id) {
                assert(ag.ordering.contains((ag.id_to_key[id], id)));
            }
        }
    } else {
        let key = ag.ordering[0].0;
        let id = ag.ordering[0].1;
        assert(ag.ordering.contains(ag.ordering[0]));
        assert(ag.activations.contains_key(id));
        assert(ag.activations.dom().contains(id));
        lemma_key_refl(key);
        assert(map_remove(ag.ordering, key) == ag.ordering.drop_first());
        lemma_remove_entry(ag, id);
        assert(pop_spec(ag).0 == remove_entry_spec(ag, key, id));
        let a = ag.activations[id];
        lemma_sorted_first_is_min(ag.ordering);
        assert forall|other: nat| #[trigger]
            ag.activations.contains_key(other) && other != a.id implies cmp_act_spec(
            ag.strategy,
            a,
            ag.activations[other],
        ) == Ordering::Less by {
            let e = (ag.id_to_key[other], other);
            assert(ag.ordering.contains(e));
            let j = choose|j: int| 0 <= j < ag.ordering.len() && ag.ordering[j] == e;
            assert(j != 0);
        }
    }
}

// ---------------------------------------------------------------------
// pop_matching (crates/ferric-rules-core/src/agenda.rs:324)
// ---------------------------------------------------------------------

/// The scan `for (key, &id) in ordering.iter().skip(i)` of `pop_matching`
/// (crates/ferric-rules-core/src/agenda.rs:332-340): the first in-order position at or after `i` holding a
/// live activation that satisfies `pred`.
pub open spec fn find_matching(
    s: Entries,
    acts: Map<nat, SAct>,
    pred: spec_fn(SAct) -> bool,
    i: int,
) -> Option<int>
    decreases s.len() - i,
{
    if i < 0 || i >= s.len() {
        None
    } else if acts.contains_key(s[i].1) && pred(acts[s[i].1]) {
        Some(i)
    } else {
        find_matching(s, acts, pred, i + 1)
    }
}

pub proof fn lemma_find_matching(
    s: Entries,
    acts: Map<nat, SAct>,
    pred: spec_fn(SAct) -> bool,
    i: int,
)
    requires
        0 <= i,
    ensures
        find_matching(s, acts, pred, i) matches Some(j) ==> {
            &&& i <= j < s.len()
            &&& acts.contains_key(s[j].1)
            &&& pred(acts[s[j].1])
            &&& forall|m: int|
                i <= m < j ==> !(acts.contains_key(#[trigger] s[m].1) && pred(acts[s[m].1]))
        },
        find_matching(s, acts, pred, i) is None ==> forall|m: int|
            i <= m < s.len() ==> !(acts.contains_key(#[trigger] s[m].1) && pred(acts[s[m].1])),
    decreases s.len() - i,
{
    if i < s.len() && !(acts.contains_key(s[i].1) && pred(acts[s[i].1])) {
        lemma_find_matching(s, acts, pred, i + 1);
    }
}

/// Models `Agenda::pop_matching` (crates/ferric-rules-core/src/agenda.rs:324-354). `pred` is the caller's
/// predicate, modelled as a pure function of the activation.
pub open spec fn pop_matching_spec(ag: AgendaModel, pred: spec_fn(SAct) -> bool) -> (
    AgendaModel,
    Option<SAct>,
) {
    if ag.ordering.len() == 0 {
        (ag, None)
    } else if ag.activations.contains_key(ag.ordering[0].1) && pred(
        ag.activations[ag.ordering[0].1],
    ) {
        pop_spec(ag)
    } else {
        match find_matching(ag.ordering, ag.activations, pred, 1) {
            None => (ag, None),
            Some(j) => {
                let key = ag.ordering[j].0;
                let id = ag.ordering[j].1;
                (remove_entry_spec(ag, key, id), Some(ag.activations[id]))
            },
        }
    }
}

/// THEOREM (pop_matching). `pop_matching` preserves the invariant; it
/// returns `None` exactly when no live activation satisfies `pred` (leaving
/// the agenda unchanged); otherwise it returns and removes a live activation
/// satisfying `pred` that strictly precedes every other live activation
/// satisfying `pred`.
pub proof fn theorem_pop_matching(ag: AgendaModel, pred: spec_fn(SAct) -> bool)
    requires
        inv(ag),
    ensures
        inv(pop_matching_spec(ag, pred).0),
        pop_matching_spec(ag, pred).1 is None <==> (forall|id: nat| #[trigger]
            ag.activations.contains_key(id) ==> !pred(ag.activations[id])),
        pop_matching_spec(ag, pred).1 is None ==> pop_matching_spec(ag, pred).0 == ag,
        pop_matching_spec(ag, pred).1 matches Some(a) ==> {
            &&& ag.activations.contains_key(a.id)
            &&& ag.activations[a.id] == a
            &&& pred(a)
            &&& pop_matching_spec(ag, pred).0.activations == ag.activations.remove(a.id)
            &&& forall|id: nat| #[trigger]
                ag.activations.contains_key(id) && id != a.id && pred(ag.activations[id])
                    ==> cmp_act_spec(ag.strategy, a, ag.activations[id]) == Ordering::Less
        },
{
    theorem_pop(ag);
    lemma_sorted_first_is_min(ag.ordering);
    if ag.ordering.len() == 0 {
    } else if ag.activations.contains_key(ag.ordering[0].1) && pred(
        ag.activations[ag.ordering[0].1],
    ) {
        assert(pop_spec(ag).1 is Some);
    } else {
        lemma_find_matching(ag.ordering, ag.activations, pred, 1);
        match find_matching(ag.ordering, ag.activations, pred, 1) {
            None => {
                assert forall|id: nat| #[trigger]
                    ag.activations.contains_key(id) implies !pred(ag.activations[id]) by {
                    let e = (ag.id_to_key[id], id);
                    assert(ag.ordering.contains(e));
                    let m = choose|m: int| 0 <= m < ag.ordering.len() && ag.ordering[m] == e;
                    assert(ag.ordering[m].1 == id);
                }
            },
            Some(j) => {
                let key = ag.ordering[j].0;
                let id = ag.ordering[j].1;
                assert(ag.ordering.contains(ag.ordering[j]));
                assert(ag.id_to_key[id] == key);
                lemma_remove_entry(ag, id);
                let a = ag.activations[id];
                assert forall|other: nat| #[trigger]
                    ag.activations.contains_key(other) && other != a.id && pred(
                        ag.activations[other],
                    ) implies cmp_act_spec(ag.strategy, a, ag.activations[other])
                    == Ordering::Less by {
                    let e = (ag.id_to_key[other], other);
                    assert(ag.ordering.contains(e));
                    let m = choose|m: int| 0 <= m < ag.ordering.len() && ag.ordering[m] == e;
                    assert(ag.ordering[m].1 == other);
                    assert(m != 0);
                    assert(!(m < j));
                    assert(m != j);
                    assert(cmp_key_spec(ag.ordering[j].0, ag.ordering[m].0) == Ordering::Less);
                }
            },
        }
    }
}


// ---------------------------------------------------------------------
// with_strategy / clear (crates/ferric-rules-core/src/agenda.rs:192, 520)
// ---------------------------------------------------------------------

/// Models `Agenda::with_strategy` (crates/ferric-rules-core/src/agenda.rs:192-202).
pub open spec fn with_strategy_spec(strategy: Strategy) -> AgendaModel {
    AgendaModel {
        ordering: Seq::empty(),
        activations: Map::empty(),
        id_to_key: Map::empty(),
        token_to_activations: Map::empty(),
        next_seq: 0,
        strategy,
    }
}

/// Models `Agenda::clear` (crates/ferric-rules-core/src/agenda.rs:520-528): everything but the strategy
/// is reset.
pub open spec fn clear_spec(ag: AgendaModel) -> AgendaModel {
    with_strategy_spec(ag.strategy)
}

pub proof fn theorem_new_and_clear_satisfy_inv(strategy: Strategy, ag: AgendaModel)
    ensures
        inv(with_strategy_spec(strategy)),
        inv(clear_spec(ag)),
{
}

/// Non-vacuity: the invariant is satisfiable by non-empty agendas, and a
/// one-element agenda pops its element.
pub proof fn example_add_then_pop(strategy: Strategy, activation: SAct)
    ensures
        inv(add_spec(with_strategy_spec(strategy), activation, 7)),
        pop_spec(add_spec(with_strategy_spec(strategy), activation, 7)).1 == Some(
            added_activation(with_strategy_spec(strategy), activation, 7),
        ),
{
    let ag0 = with_strategy_spec(strategy);
    theorem_add_preserves_inv(ag0, activation, 7);
    let ag1 = add_spec(ag0, activation, 7);
    assert(ag1.ordering =~= seq![(build_key_spec(strategy, added_activation(ag0, activation, 7)), 7nat)]);
}

// ---------------------------------------------------------------------
// remove_activations_for_rule (crates/ferric-rules-core/src/agenda.rs:435)
// ---------------------------------------------------------------------

/// One iteration of the loop in `remove_activations_for_rule`
/// (crates/ferric-rules-core/src/agenda.rs:446-455).
pub open spec fn remove_for_rule_step(ag: AgendaModel, id: nat) -> AgendaModel {
    let ordering = if ag.id_to_key.contains_key(id) {
        map_remove(ag.ordering, ag.id_to_key[id])
    } else {
        ag.ordering
    };
    let token_to_activations = if ag.activations.contains_key(id) {
        remove_from_token_index(ag.token_to_activations, ag.activations[id].token, id)
    } else {
        ag.token_to_activations
    };
    AgendaModel {
        ordering,
        id_to_key: ag.id_to_key.remove(id),
        activations: ag.activations.remove(id),
        token_to_activations,
        ..ag
    }
}

pub open spec fn remove_for_rule_loop(ag: AgendaModel, ids: Seq<nat>) -> AgendaModel
    decreases ids.len(),
{
    if ids.len() == 0 {
        ag
    } else {
        remove_for_rule_loop(remove_for_rule_step(ag, ids[0]), ids.drop_first())
    }
}

/// `act_ids` of `remove_activations_for_rule` (crates/ferric-rules-core/src/agenda.rs:439-443): the live
/// activations of `rule`, in SlotMap iteration order (any duplicate-free
/// enumeration).
pub open spec fn enumerates_rule(ag: AgendaModel, rule: nat, ids: Seq<nat>) -> bool {
    &&& ids.no_duplicates()
    &&& forall|id: nat| #[trigger]
        ids.contains(id) <==> (ag.activations.contains_key(id) && ag.activations[id].rule == rule)
}

/// Models `Agenda::remove_activations_for_rule` (crates/ferric-rules-core/src/agenda.rs:435-458).
pub open spec fn remove_activations_for_rule_spec(ag: AgendaModel, ids: Seq<nat>) -> AgendaModel {
    remove_for_rule_loop(ag, ids)
}

pub proof fn lemma_remove_for_rule_loop(ag: AgendaModel, ids: Seq<nat>)
    requires
        inv(ag),
        ids.no_duplicates(),
        forall|j: int| 0 <= j < ids.len() ==> ag.activations.contains_key(#[trigger] ids[j]),
    ensures
        inv(remove_for_rule_loop(ag, ids)),
        remove_for_rule_loop(ag, ids).strategy == ag.strategy,
        forall|id: nat| #[trigger]
            remove_for_rule_loop(ag, ids).activations.contains_key(id) <==> (
            ag.activations.contains_key(id) && !ids.contains(id)),
        forall|id: nat| #[trigger]
            remove_for_rule_loop(ag, ids).activations.contains_key(id) ==> remove_for_rule_loop(
                ag,
                ids,
            ).activations[id] == ag.activations[id],
    decreases ids.len(),
{
    if ids.len() > 0 {
        let id = ids[0];
        assert(ag.activations.contains_key(id));
        lemma_remove_entry(ag, id);
        assert(remove_for_rule_step(ag, id) == remove_entry_spec(ag, ag.id_to_key[id], id));
        let ag1 = remove_for_rule_step(ag, id);
        let rest = ids.drop_first();
        assert forall|j: int| 0 <= j < rest.len() implies ag1.activations.contains_key(
            #[trigger] rest[j],
        ) by {
            assert(rest[j] == ids[j + 1]);
            assert(ids[j + 1] != ids[0]);
        }
        lemma_remove_for_rule_loop(ag1, rest);
        assert forall|x: nat| #[trigger] ids.contains(x) <==> (x == id || rest.contains(x)) by {
            if ids.contains(x) {
                let j = choose|j: int| 0 <= j < ids.len() && ids[j] == x;
                if j > 0 {
                    assert(rest[j - 1] == x);
                }
            }
            if rest.contains(x) {
                let j = choose|j: int| 0 <= j < rest.len() && rest[j] == x;
                assert(ids[j + 1] == x);
            }
        }
    }
}

/// THEOREM (remove_activations_for_rule). Preserves the invariant and
/// leaves exactly the live activations of other rules, unchanged.
pub proof fn theorem_remove_activations_for_rule(ag: AgendaModel, rule: nat, ids: Seq<nat>)
    requires
        inv(ag),
        enumerates_rule(ag, rule, ids),
    ensures
        inv(remove_activations_for_rule_spec(ag, ids)),
        forall|id: nat| #[trigger]
            remove_activations_for_rule_spec(ag, ids).activations.contains_key(id) <==> (
            ag.activations.contains_key(id) && ag.activations[id].rule != rule),
        forall|id: nat| #[trigger]
            remove_activations_for_rule_spec(ag, ids).activations.contains_key(id)
                ==> remove_activations_for_rule_spec(ag, ids).activations[id] == ag.activations[id],
{
    assert forall|j: int| 0 <= j < ids.len() implies ag.activations.contains_key(#[trigger] ids[j]) by {
        assert(ids.contains(ids[j]));
    }
    lemma_remove_for_rule_loop(ag, ids);
}

// ---------------------------------------------------------------------
// remove_activations_for_token (crates/ferric-rules-core/src/agenda.rs:407)
// ---------------------------------------------------------------------

/// One iteration of the loop in `remove_activations_for_token`
/// (crates/ferric-rules-core/src/agenda.rs:414-427); the token's index entry was already removed.
pub open spec fn remove_for_token_step(ag: AgendaModel, id: nat) -> AgendaModel {
    let ordering = if ag.id_to_key.contains_key(id) {
        map_remove(ag.ordering, ag.id_to_key[id])
    } else {
        ag.ordering
    };
    AgendaModel {
        ordering,
        id_to_key: ag.id_to_key.remove(id),
        activations: ag.activations.remove(id),
        ..ag
    }
}

pub open spec fn remove_for_token_loop(ag: AgendaModel, ids: Seq<nat>) -> AgendaModel
    decreases ids.len(),
{
    if ids.len() == 0 {
        ag
    } else {
        remove_for_token_loop(remove_for_token_step(ag, ids[0]), ids.drop_first())
    }
}

/// Models `Agenda::remove_activations_for_token` (crates/ferric-rules-core/src/agenda.rs:407-430).
pub open spec fn remove_activations_for_token_spec(ag: AgendaModel, token: nat) -> AgendaModel {
    if !ag.token_to_activations.contains_key(token) {
        ag
    } else {
        remove_for_token_loop(
            AgendaModel { token_to_activations: ag.token_to_activations.remove(token), ..ag },
            ag.token_to_activations[token],
        )
    }
}

/// Loop invariant of `remove_activations_for_token`: the full invariant,
/// except that live activations of `token` are no longer indexed.
pub open spec fn inv_without_token(ag: AgendaModel, token: nat) -> bool {
    &&& ordering_inv(ag)
    &&& activation_inv(ag)
    &&& token_entries_inv(ag)
    &&& token_cover_inv_except(ag, Some(token))
    &&& !ag.token_to_activations.contains_key(token)
}

pub proof fn lemma_remove_for_token_loop(ag: AgendaModel, token: nat, ids: Seq<nat>)
    requires
        inv_without_token(ag, token),
        ids.no_duplicates(),
        forall|j: int|
            0 <= j < ids.len() ==> ag.activations.contains_key(#[trigger] ids[j])
                && ag.activations[ids[j]].token == token,
    ensures
        inv_without_token(remove_for_token_loop(ag, ids), token),
        forall|id: nat| #[trigger]
            remove_for_token_loop(ag, ids).activations.contains_key(id) <==> (
            ag.activations.contains_key(id) && !ids.contains(id)),
        forall|id: nat| #[trigger]
            remove_for_token_loop(ag, ids).activations.contains_key(id) ==> remove_for_token_loop(
                ag,
                ids,
            ).activations[id] == ag.activations[id],
    decreases ids.len(),
{
    if ids.len() > 0 {
        let id = ids[0];
        lemma_remove_core(ag, id);
        let ag1 = remove_for_token_step(ag, id);
        assert(ag1 == remove_core(ag, ag.id_to_key[id], id));
        // token index entries: every indexed id has another token, so it survives.
        assert forall|t: nat| #[trigger] ag1.token_to_activations.contains_key(t) implies {
            &&& ag1.token_to_activations[t].len() > 0
            &&& ag1.token_to_activations[t].no_duplicates()
            &&& forall|j: int|
                0 <= j < ag1.token_to_activations[t].len() ==> ag1.activations.contains_key(
                    #[trigger] ag1.token_to_activations[t][j],
                ) && ag1.activations[ag1.token_to_activations[t][j]].token == t
        } by {
            let s = ag.token_to_activations[t];
            assert(ag.token_to_activations.contains_key(t));
            assert forall|j: int| 0 <= j < s.len() implies ag1.activations.contains_key(
                #[trigger] s[j],
            ) && ag1.activations[s[j]].token == t by {
                assert(ag.activations.contains_key(s[j]));
            }
        }
        let rest = ids.drop_first();
        assert forall|j: int| 0 <= j < rest.len() implies ag1.activations.contains_key(
            #[trigger] rest[j],
        ) && ag1.activations[rest[j]].token == token by {
            assert(rest[j] == ids[j + 1]);
            assert(ids[j + 1] != ids[0]);
        }
        lemma_remove_for_token_loop(ag1, token, rest);
        assert forall|x: nat| #[trigger] ids.contains(x) <==> (x == id || rest.contains(x)) by {
            if ids.contains(x) {
                let j = choose|j: int| 0 <= j < ids.len() && ids[j] == x;
                if j > 0 {
                    assert(rest[j - 1] == x);
                }
            }
            if rest.contains(x) {
                let j = choose|j: int| 0 <= j < rest.len() && rest[j] == x;
                assert(ids[j + 1] == x);
            }
        }
    }
}

/// THEOREM (remove_activations_for_token). Preserves the invariant and
/// leaves exactly the live activations of other tokens, unchanged.
pub proof fn theorem_remove_activations_for_token(ag: AgendaModel, token: nat)
    requires
        inv(ag),
    ensures
        inv(remove_activations_for_token_spec(ag, token)),
        forall|id: nat| #[trigger]
            remove_activations_for_token_spec(ag, token).activations.contains_key(id) <==> (
            ag.activations.contains_key(id) && ag.activations[id].token != token),
        forall|id: nat| #[trigger]
            remove_activations_for_token_spec(ag, token).activations.contains_key(id)
                ==> remove_activations_for_token_spec(ag, token).activations[id]
                == ag.activations[id],
{
    if ag.token_to_activations.contains_key(token) {
        let ids = ag.token_to_activations[token];
        let ag0 = AgendaModel {
            token_to_activations: ag.token_to_activations.remove(token),
            ..ag
        };
        assert(inv_without_token(ag0, token));
        lemma_remove_for_token_loop(ag0, token, ids);
        let ag2 = remove_for_token_loop(ag0, ids);
        assert forall|id: nat| #[trigger]
            ag.activations.contains_key(id) && ag.activations[id].token == token implies ids.contains(
            id,
        ) by {}
        assert forall|id: nat| #[trigger] ids.contains(id) implies ag.activations.contains_key(id)
            && ag.activations[id].token == token by {
            let j = choose|j: int| 0 <= j < ids.len() && ids[j] == id;
        }
        assert(token_cover_inv_except(ag2, None));
    } else {
        assert forall|id: nat| #[trigger] ag.activations.contains_key(id) implies ag.activations[id].token
            != token by {}
    }
}

// ---------------------------------------------------------------------
// set_strategy (crates/ferric-rules-core/src/agenda.rs:500)
// ---------------------------------------------------------------------

/// The rebuild loop of `set_strategy` (crates/ferric-rules-core/src/agenda.rs:510-514): insert every
/// activation's key under the (new) strategy.
pub open spec fn rebuild_loop(ag: AgendaModel, ids: Seq<nat>) -> AgendaModel
    decreases ids.len(),
{
    if ids.len() == 0 {
        ag
    } else {
        let id = ids[0];
        let key = build_key_spec(ag.strategy, ag.activations[id]);
        rebuild_loop(
            AgendaModel {
                ordering: map_insert(ag.ordering, key, id),
                id_to_key: ag.id_to_key.insert(id, key),
                ..ag
            },
            ids.drop_first(),
        )
    }
}

pub open spec fn same_strategy(a: Strategy, b: Strategy) -> bool {
    match (a, b) {
        (Strategy::Depth, Strategy::Depth) => true,
        (Strategy::Breadth, Strategy::Breadth) => true,
        (Strategy::Lex, Strategy::Lex) => true,
        (Strategy::Mea, Strategy::Mea) => true,
        _ => false,
    }
}

/// Models `Agenda::set_strategy` (crates/ferric-rules-core/src/agenda.rs:500-517); `ids` is the SlotMap
/// iteration order of the live activations (any duplicate-free enumeration).
/// Returns the new agenda and the previous strategy.
pub open spec fn set_strategy_spec(ag: AgendaModel, strategy: Strategy, ids: Seq<nat>) -> (
    AgendaModel,
    Strategy,
) {
    if same_strategy(ag.strategy, strategy) {
        (ag, ag.strategy)
    } else {
        (
            rebuild_loop(
                AgendaModel {
                    strategy,
                    ordering: Seq::empty(),
                    id_to_key: Map::empty(),
                    ..ag
                },
                ids,
            ),
            ag.strategy,
        )
    }
}

pub open spec fn enumerates_live(ag: AgendaModel, ids: Seq<nat>) -> bool {
    &&& ids.no_duplicates()
    &&& forall|id: nat| #[trigger] ids.contains(id) <==> ag.activations.contains_key(id)
}

/// Loop invariant of the rebuild: a partial ordering index over the ids
/// processed so far.
pub open spec fn partial_index(ag: AgendaModel) -> bool {
    &&& strictly_sorted(ag.ordering)
    &&& forall|e: (SKey, nat)| #[trigger]
        ag.ordering.contains(e) <==> (ag.id_to_key.contains_key(e.1) && ag.id_to_key[e.1] == e.0)
    &&& forall|id: nat| #[trigger]
        ag.id_to_key.contains_key(id) ==> ag.activations.contains_key(id) && ag.id_to_key[id]
            == build_key_spec(ag.strategy, ag.activations[id])
    &&& forall|id1: nat, id2: nat|
        #[trigger] ag.activations.contains_key(id1) && #[trigger] ag.activations.contains_key(id2)
            && ag.activations[id1].activation_seq == ag.activations[id2].activation_seq ==> id1
            == id2
}

pub proof fn lemma_rebuild_loop(ag: AgendaModel, ids: Seq<nat>)
    requires
        partial_index(ag),
        ids.no_duplicates(),
        forall|j: int|
            0 <= j < ids.len() ==> ag.activations.contains_key(#[trigger] ids[j])
                && !ag.id_to_key.contains_key(ids[j]),
    ensures
        partial_index(rebuild_loop(ag, ids)),
        forall|id: nat| #[trigger]
            rebuild_loop(ag, ids).id_to_key.contains_key(id) <==> (ag.id_to_key.contains_key(id)
                || ids.contains(id)),
        rebuild_loop(ag, ids).activations == ag.activations,
        rebuild_loop(ag, ids).token_to_activations == ag.token_to_activations,
        rebuild_loop(ag, ids).next_seq == ag.next_seq,
        rebuild_loop(ag, ids).strategy == ag.strategy,
    decreases ids.len(),
{
    if ids.len() > 0 {
        let id = ids[0];
        let key = build_key_spec(ag.strategy, ag.activations[id]);
        let ag1 = AgendaModel {
            ordering: map_insert(ag.ordering, key, id),
            id_to_key: ag.id_to_key.insert(id, key),
            ..ag
        };
        lemma_map_insert(ag.ordering, key, id);
        assert forall|e: (SKey, nat)| #[trigger] ag.ordering.contains(e) implies e.0 != key by {
            assert(ag.id_to_key.contains_key(e.1));
            if e.0 == key {
                assert(ag.activations[e.1].activation_seq == ag.activations[id].activation_seq);
            }
        }
        assert forall|e: (SKey, nat)| #[trigger]
            ag1.ordering.contains(e) <==> (ag1.id_to_key.contains_key(e.1) && ag1.id_to_key[e.1]
                == e.0) by {
            if ag.ordering.contains(e) {
                assert(ag.id_to_key.contains_key(e.1));
            }
            if ag1.id_to_key.contains_key(e.1) && ag1.id_to_key[e.1] == e.0 && e.1 != id {
                assert(ag.ordering.contains(e));
            }
        }
        let rest = ids.drop_first();
        assert forall|j: int| 0 <= j < rest.len() implies ag1.activations.contains_key(
            #[trigger] rest[j],
        ) && !ag1.id_to_key.contains_key(rest[j]) by {
            assert(rest[j] == ids[j + 1]);
            assert(ids[j + 1] != ids[0]);
        }
        lemma_rebuild_loop(ag1, rest);
        assert forall|x: nat| #[trigger] ids.contains(x) <==> (x == id || rest.contains(x)) by {
            if ids.contains(x) {
                let j = choose|j: int| 0 <= j < ids.len() && ids[j] == x;
                if j > 0 {
                    assert(rest[j - 1] == x);
                }
            }
            if rest.contains(x) {
                let j = choose|j: int| 0 <= j < rest.len() && rest[j] == x;
                assert(ids[j + 1] == x);
            }
        }
    }
}

/// THEOREM (set_strategy). Switching strategy rebuilds a consistent agenda
/// for the new strategy, keeping every activation (identity, sequence,
/// recency, complexity), the token index and `next_seq`, and returns the
/// previous strategy (crates/ferric-rules-core/src/agenda.rs:496-499).
pub proof fn theorem_set_strategy(ag: AgendaModel, strategy: Strategy, ids: Seq<nat>)
    requires
        inv(ag),
        enumerates_live(ag, ids),
    ensures
        inv(set_strategy_spec(ag, strategy, ids).0),
        set_strategy_spec(ag, strategy, ids).0.strategy == strategy,
        set_strategy_spec(ag, strategy, ids).1 == ag.strategy,
        set_strategy_spec(ag, strategy, ids).0.activations == ag.activations,
        set_strategy_spec(ag, strategy, ids).0.token_to_activations == ag.token_to_activations,
        set_strategy_spec(ag, strategy, ids).0.next_seq == ag.next_seq,
{
    if !same_strategy(ag.strategy, strategy) {
        let ag0 = AgendaModel { strategy, ordering: Seq::empty(), id_to_key: Map::empty(), ..ag };
        assert forall|j: int| 0 <= j < ids.len() implies ag0.activations.contains_key(
            #[trigger] ids[j],
        ) && !ag0.id_to_key.contains_key(ids[j]) by {
            assert(ids.contains(ids[j]));
        }
        lemma_rebuild_loop(ag0, ids);
        let ag2 = rebuild_loop(ag0, ids);
        assert forall|id: nat| #[trigger]
            ag2.id_to_key.contains_key(id) <==> ag2.activations.contains_key(id) by {
            assert(ids.contains(id) <==> ag.activations.contains_key(id));
        }
        assert(ordering_inv(ag2));
        assert forall|id: nat| #[trigger] ag2.activations.contains_key(id) implies {
            &&& ag2.activations[id].id == id
            &&& ag2.activations[id].activation_seq < ag2.next_seq
            &&& ag2.id_to_key[id] == build_key_spec(ag2.strategy, ag2.activations[id])
        } by {
            assert(ag.activations.contains_key(id));
            assert(ag2.id_to_key.contains_key(id));
        }
        assert(activation_inv(ag2));
        assert(token_entries_inv(ag2));
        assert(token_cover_inv_except(ag2, None));
    } else {
        assert(ag.strategy == strategy);
    }
}


// ---------------------------------------------------------------------
// rebase_sequences and the exhaustion path of add (crates/ferric-rules-core/src/agenda.rs:249-251, 276)
// ---------------------------------------------------------------------

/// `chronological` after `sort_unstable_by_key(|&(sequence, _)| sequence)`
/// (crates/ferric-rules-core/src/agenda.rs:278-283): the live ids, each once, in strictly increasing
/// sequence order (sequences are distinct under `inv`, so the sorted order is
/// unique). The sort itself is std's and is taken as given.
pub open spec fn chronological_order(ag: AgendaModel, chron: Seq<nat>) -> bool {
    &&& chron.no_duplicates()
    &&& forall|id: nat| #[trigger] chron.contains(id) <==> ag.activations.contains_key(id)
    &&& forall|i: int, j: int|
        0 <= i < j < chron.len() ==> #[trigger] ag.activations[chron[i]].activation_seq
            < #[trigger] ag.activations[chron[j]].activation_seq
}

pub open spec fn position(s: Seq<nat>, x: nat) -> int {
    choose|i: int| 0 <= i < s.len() && s[i] == x
}

/// The renumbering loop of `rebase_sequences` (crates/ferric-rules-core/src/agenda.rs:284-288): each live
/// activation's sequence becomes its position in `chronological`.
pub open spec fn renumbered(ag: AgendaModel, chron: Seq<nat>) -> Map<nat, SAct> {
    Map::new(
        ag.activations.dom(),
        |id: nat| SAct { activation_seq: position(chron, id) as u64, ..ag.activations[id] },
    )
}

/// Models `Agenda::rebase_sequences` (crates/ferric-rules-core/src/agenda.rs:276-299); `ids` is the SlotMap
/// iteration order used by the rebuild loop.
pub open spec fn rebase_spec(ag: AgendaModel, chron: Seq<nat>, ids: Seq<nat>) -> AgendaModel {
    rebuild_loop(
        AgendaModel {
            activations: renumbered(ag, chron),
            next_seq: chron.len() as u64,
            ordering: Seq::empty(),
            id_to_key: Map::empty(),
            ..ag
        },
        ids,
    )
}

/// Models `Agenda::add` (crates/ferric-rules-core/src/agenda.rs:245) including the exhaustion path.
pub open spec fn add_full_spec(
    ag: AgendaModel,
    activation: SAct,
    new_id: nat,
    chron: Seq<nat>,
    ids: Seq<nat>,
) -> AgendaModel {
    if ag.next_seq == u64::MAX {
        add_spec(rebase_spec(ag, chron, ids), activation, new_id)
    } else {
        add_spec(ag, activation, new_id)
    }
}

pub proof fn lemma_position(chron: Seq<nat>, x: nat)
    requires
        chron.no_duplicates(),
        chron.contains(x),
    ensures
        0 <= position(chron, x) < chron.len(),
        chron[position(chron, x)] == x,
        forall|i: int| 0 <= i < chron.len() && chron[i] == x ==> i == position(chron, x),
{
}

/// THEOREM (rebase). `rebase_sequences` keeps every live activation (all
/// fields but `activation_seq`), restores the invariant with `next_seq`
/// equal to the number of live activations, and preserves the
/// conflict-resolution order between every pair of live activations under
/// the current strategy (so no activation "appears old" after exhaustion).
pub proof fn theorem_rebase(ag: AgendaModel, chron: Seq<nat>, ids: Seq<nat>)
    requires
        inv(ag),
        chronological_order(ag, chron),
        enumerates_live(ag, ids),
        chron.len() <= u64::MAX,
    ensures
        inv(rebase_spec(ag, chron, ids)),
        rebase_spec(ag, chron, ids).next_seq == chron.len(),
        rebase_spec(ag, chron, ids).strategy == ag.strategy,
        forall|id: nat| #[trigger]
            rebase_spec(ag, chron, ids).activations.contains_key(id)
                <==> ag.activations.contains_key(id),
        forall|id: nat| #[trigger]
            ag.activations.contains_key(id) ==> rebase_spec(ag, chron, ids).activations[id]
                == (SAct {
                activation_seq: rebase_spec(ag, chron, ids).activations[id].activation_seq,
                ..ag.activations[id]
            }),
        forall|a: nat, b: nat|
            ag.activations.contains_key(a) && ag.activations.contains_key(b) ==> #[trigger]
            cmp_act_spec(
                ag.strategy,
                rebase_spec(ag, chron, ids).activations[a],
                rebase_spec(ag, chron, ids).activations[b],
            ) == #[trigger] cmp_act_spec(ag.strategy, ag.activations[a], ag.activations[b]),
{
    let renum = renumbered(ag, chron);
    let ag0 = AgendaModel {
        activations: renum,
        next_seq: chron.len() as u64,
        ordering: Seq::empty(),
        id_to_key: Map::empty(),
        ..ag
    };
    assert(renum.dom() == ag.activations.dom());
    // Positions are an order-embedding of the old sequence numbers.
    assert forall|a: nat, b: nat|
        #![trigger position(chron, a), position(chron, b)]
        ag.activations.contains_key(a) && ag.activations.contains_key(b) implies (position(chron, a)
        < position(chron, b) <==> ag.activations[a].activation_seq
        < ag.activations[b].activation_seq) && (position(chron, a) == position(chron, b) <==> a
        == b) by {
        lemma_position(chron, a);
        lemma_position(chron, b);
        let pa = position(chron, a);
        let pb = position(chron, b);
        if pa < pb {
            assert(ag.activations[chron[pa]].activation_seq < ag.activations[chron[pb]].activation_seq);
        }
        if pb < pa {
            assert(ag.activations[chron[pb]].activation_seq < ag.activations[chron[pa]].activation_seq);
        }
    }
    // partial_index for the cleared, renumbered agenda.
    assert forall|id1: nat, id2: nat|
        #[trigger] ag0.activations.contains_key(id1) && #[trigger] ag0.activations.contains_key(id2)
            && ag0.activations[id1].activation_seq == ag0.activations[id2].activation_seq implies id1
        == id2 by {
        lemma_position(chron, id1);
        lemma_position(chron, id2);
    }
    assert forall|e: (SKey, nat)| #[trigger]
        ag0.ordering.contains(e) <==> (ag0.id_to_key.contains_key(e.1) && ag0.id_to_key[e.1]
            == e.0) by {}
    assert(partial_index(ag0));
    assert forall|j: int| 0 <= j < ids.len() implies ag0.activations.contains_key(#[trigger] ids[j])
        && !ag0.id_to_key.contains_key(ids[j]) by {
        assert(ids.contains(ids[j]));
    }
    lemma_rebuild_loop(ag0, ids);
    let ag2 = rebuild_loop(ag0, ids);
    assert forall|id: nat| #[trigger]
        ag2.id_to_key.contains_key(id) <==> ag2.activations.contains_key(id) by {
        assert(ids.contains(id) <==> ag.activations.contains_key(id));
    }
    assert(ordering_inv(ag2));
    assert forall|id: nat| #[trigger] ag2.activations.contains_key(id) implies {
        &&& ag2.activations[id].id == id
        &&& ag2.activations[id].activation_seq < ag2.next_seq
        &&& ag2.id_to_key[id] == build_key_spec(ag2.strategy, ag2.activations[id])
    } by {
        assert(ag.activations.contains_key(id));
        lemma_position(chron, id);
        assert(ag2.id_to_key.contains_key(id));
    }
    assert(activation_inv(ag2));
    assert forall|t: nat| #[trigger] ag2.token_to_activations.contains_key(t) implies {
        &&& ag2.token_to_activations[t].len() > 0
        &&& ag2.token_to_activations[t].no_duplicates()
        &&& forall|j: int|
            0 <= j < ag2.token_to_activations[t].len() ==> ag2.activations.contains_key(
                #[trigger] ag2.token_to_activations[t][j],
            ) && ag2.activations[ag2.token_to_activations[t][j]].token == t
    } by {
        assert(ag.token_to_activations.contains_key(t));
        let s = ag.token_to_activations[t];
        assert forall|j: int| 0 <= j < s.len() implies ag2.activations.contains_key(
            #[trigger] s[j],
        ) && ag2.activations[s[j]].token == t by {
            assert(ag.activations.contains_key(s[j]));
        }
    }
    assert forall|id: nat| #[trigger]
        ag2.activations.contains_key(id) && None::<nat> != Some(ag2.activations[id].token) implies {
        &&& ag2.token_to_activations.contains_key(ag2.activations[id].token)
        &&& ag2.token_to_activations[ag2.activations[id].token].contains(id)
    } by {
        assert(ag.activations.contains_key(id));
    }
    assert forall|a: nat, b: nat|
        ag.activations.contains_key(a) && ag.activations.contains_key(b) implies #[trigger]
        cmp_act_spec(ag.strategy, ag2.activations[a], ag2.activations[b]) == #[trigger]
        cmp_act_spec(ag.strategy, ag.activations[a], ag.activations[b]) by {
        lemma_position(chron, a);
        lemma_position(chron, b);
        theorem_order_preserving_renumbering_preserves_order(
            ag.strategy,
            ag.activations[a],
            ag.activations[b],
            ag2.activations[a],
            ag2.activations[b],
        );
    }
}

/// THEOREM (add, complete). `add` preserves the invariant on both paths,
/// provided the live activations do not fill the whole sequence space
/// (`chron.len() < u64::MAX`). This is not checked by `add` or
/// `rebase_sequences` (whose `expect`s at
/// crates/ferric-rules-core/src/agenda.rs:286-291 only guard `usize -> u64`
/// conversions); it is enforced by the `activations` SlotMap, which panics
/// before holding `u32::MAX` entries (slotmap 1.1.1 src/basic.rs:413-415).
pub proof fn theorem_add_full_preserves_inv(
    ag: AgendaModel,
    activation: SAct,
    new_id: nat,
    chron: Seq<nat>,
    ids: Seq<nat>,
)
    requires
        inv(ag),
        !ag.activations.contains_key(new_id),
        ag.next_seq == u64::MAX ==> chronological_order(ag, chron) && enumerates_live(ag, ids)
            && chron.len() < u64::MAX,
    ensures
        inv(add_full_spec(ag, activation, new_id, chron, ids)),
{
    if ag.next_seq == u64::MAX {
        theorem_rebase(ag, chron, ids);
        let ag1 = rebase_spec(ag, chron, ids);
        assert(!ag1.activations.contains_key(new_id));
        theorem_add_preserves_inv(ag1, activation, new_id);
    } else {
        theorem_add_preserves_inv(ag, activation, new_id);
    }
}


// ---------------------------------------------------------------------
// iter_ordered (crates/ferric-rules-core/src/agenda.rs:484)
// ---------------------------------------------------------------------

/// Models `Agenda::iter_ordered` (crates/ferric-rules-core/src/agenda.rs:484-488):
/// `ordering.values().filter_map(|id| activations.get(*id))`, i.e. the
/// activations of the in-order entries, skipping ids that are not live.
pub open spec fn iter_ordered_spec(s: Entries, acts: Map<nat, SAct>) -> Seq<SAct>
    decreases s.len(),
{
    if s.len() == 0 {
        Seq::empty()
    } else if acts.contains_key(s[0].1) {
        seq![acts[s[0].1]] + iter_ordered_spec(s.drop_first(), acts)
    } else {
        iter_ordered_spec(s.drop_first(), acts)
    }
}

/// When every entry's id is live, `filter_map` drops nothing.
pub proof fn lemma_iter_ordered_all_live(s: Entries, acts: Map<nat, SAct>)
    requires
        forall|i: int| 0 <= i < s.len() ==> acts.contains_key(#[trigger] s[i].1),
    ensures
        iter_ordered_spec(s, acts).len() == s.len(),
        forall|i: int|
            0 <= i < s.len() ==> #[trigger] iter_ordered_spec(s, acts)[i] == acts[s[i].1],
    decreases s.len(),
{
    if s.len() > 0 {
        let t = s.drop_first();
        assert forall|i: int| 0 <= i < t.len() implies acts.contains_key(#[trigger] t[i].1) by {
            assert(t[i] == s[i + 1]);
        }
        lemma_iter_ordered_all_live(t, acts);
        assert(acts.contains_key(s[0].1));
        let r = iter_ordered_spec(s, acts);
        assert(r == seq![acts[s[0].1]] + iter_ordered_spec(t, acts));
        assert forall|i: int| 0 <= i < s.len() implies #[trigger] r[i] == acts[s[i].1] by {
            if i > 0 {
                assert(r[i] == iter_ordered_spec(t, acts)[i - 1]);
                assert(t[i - 1] == s[i]);
            }
        }
    }
}

/// THEOREM (iter_ordered). On a consistent agenda the `filter_map` in
/// `iter_ordered` drops nothing (every entry of `ordering` is live), and the
/// iterator yields every live activation exactly once (each is the stored
/// activation of its id), in strictly decreasing priority under the
/// agenda's strategy.
pub proof fn theorem_iter_ordered(ag: AgendaModel)
    requires
        inv(ag),
    ensures
        forall|i: int|
            0 <= i < ag.ordering.len() ==> ag.activations.contains_key(#[trigger] ag.ordering[i].1),
        iter_ordered_spec(ag.ordering, ag.activations).len() == ag.ordering.len(),
        forall|i: int|
            0 <= i < ag.ordering.len() ==> #[trigger] iter_ordered_spec(ag.ordering, ag.activations)[i]
                == ag.activations[ag.ordering[i].1] && ag.activations[ag.ordering[i].1].id
                == ag.ordering[i].1,
        forall|i: int, j: int|
            0 <= i < j < iter_ordered_spec(ag.ordering, ag.activations).len() ==> cmp_act_spec(
                ag.strategy,
                #[trigger] iter_ordered_spec(ag.ordering, ag.activations)[i],
                #[trigger] iter_ordered_spec(ag.ordering, ag.activations)[j],
            ) == Ordering::Less,
        forall|i: int|
            0 <= i < iter_ordered_spec(ag.ordering, ag.activations).len() ==> {
                let a = #[trigger] iter_ordered_spec(ag.ordering, ag.activations)[i];
                ag.activations.contains_key(a.id) && ag.activations[a.id] == a
            },
        forall|i: int, j: int|
            0 <= i < iter_ordered_spec(ag.ordering, ag.activations).len() && 0 <= j
                < iter_ordered_spec(ag.ordering, ag.activations).len() && i != j
                ==> #[trigger] iter_ordered_spec(ag.ordering, ag.activations)[i].id
                != #[trigger] iter_ordered_spec(ag.ordering, ag.activations)[j].id,
        forall|id: nat| #[trigger]
            ag.activations.contains_key(id) ==> exists|i: int|
                0 <= i < iter_ordered_spec(ag.ordering, ag.activations).len()
                    && #[trigger] iter_ordered_spec(ag.ordering, ag.activations)[i]
                    == ag.activations[id],
{
    let s = ag.ordering;
    let out = iter_ordered_spec(s, ag.activations);
    lemma_sorted_first_is_min(s);
    assert forall|i: int| 0 <= i < s.len() implies ag.activations.contains_key(#[trigger] s[i].1) by {
        assert(s.contains(s[i]));
    }
    lemma_iter_ordered_all_live(s, ag.activations);
    assert forall|i: int| 0 <= i < out.len() implies {
        let a = #[trigger] out[i];
        ag.activations.contains_key(a.id) && ag.activations[a.id] == a && a.id == s[i].1
    } by {
        assert(s.contains(s[i]));
        assert(ag.activations.contains_key(s[i].1));
    }
    assert forall|i: int, j: int|
        0 <= i < j < out.len() implies cmp_act_spec(ag.strategy, #[trigger] out[i], #[trigger] out[j])
        == Ordering::Less by {
        assert(s.contains(s[i]));
        assert(s.contains(s[j]));
        assert(cmp_key_spec(s[i].0, s[j].0) == Ordering::Less);
    }
    assert forall|i: int, j: int|
        0 <= i < out.len() && 0 <= j < out.len() && i != j implies #[trigger] out[i].id
        != #[trigger] out[j].id by {
        assert(s.contains(s[i]));
        assert(s.contains(s[j]));
        assert(out[i].id == s[i].1);
        assert(out[j].id == s[j].1);
    }
    assert forall|id: nat| #[trigger] ag.activations.contains_key(id) implies exists|i: int|
        0 <= i < out.len() && #[trigger] out[i] == ag.activations[id] by {
        let e = (ag.id_to_key[id], id);
        assert(s.contains(e));
        let i = choose|i: int| 0 <= i < s.len() && s[i] == e;
        assert(out[i] == ag.activations[id]);
    }
}

// ---------------------------------------------------------------------
// Firing order and where `add` places a new activation (agenda level)
// ---------------------------------------------------------------------

/// The strict maximum is unique: an activation that strictly precedes every
/// other live activation is the one `pop` returns.
pub proof fn lemma_pop_returns_strict_max(ag: AgendaModel, x: nat)
    requires
        inv(ag),
        ag.activations.contains_key(x),
        forall|id: nat| #[trigger]
            ag.activations.contains_key(id) && id != x ==> cmp_act_spec(
                ag.strategy,
                ag.activations[x],
                ag.activations[id],
            ) == Ordering::Less,
    ensures
        pop_spec(ag).1 == Some(ag.activations[x]),
{
    theorem_pop(ag);
    assert(ag.activations.dom().contains(x));
    assert(!(ag.activations.dom() =~= Set::<nat>::empty()));
    match pop_spec(ag).1 {
        Some(a) => {
            if a.id != x {
                lemma_key_flip(
                    build_key_spec(ag.strategy, a),
                    build_key_spec(ag.strategy, ag.activations[x]),
                );
            }
        },
        None => {},
    }
}

/// `n` successive calls of `pop` (the engine's firing loop), stopping at the
/// first `None`.
pub open spec fn pops_spec(ag: AgendaModel, n: nat) -> Seq<SAct>
    decreases n,
{
    if n == 0 {
        Seq::empty()
    } else {
        match pop_spec(ag).1 {
            None => Seq::empty(),
            Some(a) => seq![a] + pops_spec(pop_spec(ag).0, (n - 1) as nat),
        }
    }
}

/// THEOREM (firing order). Successive pops return live activations of the
/// original agenda (as stored), pairwise distinct and in strictly
/// decreasing priority under the agenda's strategy; fewer than `n` pops
/// succeed only if every live activation has been returned.
pub proof fn theorem_pops_in_priority_order(ag: AgendaModel, n: nat)
    requires
        inv(ag),
    ensures
        pops_spec(ag, n).len() <= n,
        forall|i: int|
            0 <= i < pops_spec(ag, n).len() ==> ag.activations.contains_key(
                (#[trigger] pops_spec(ag, n)[i]).id,
            ) && ag.activations[pops_spec(ag, n)[i].id] == pops_spec(ag, n)[i],
        forall|i: int, j: int|
            0 <= i < j < pops_spec(ag, n).len() ==> cmp_act_spec(
                ag.strategy,
                #[trigger] pops_spec(ag, n)[i],
                #[trigger] pops_spec(ag, n)[j],
            ) == Ordering::Less && pops_spec(ag, n)[i].id != pops_spec(ag, n)[j].id,
        pops_spec(ag, n).len() < n ==> forall|id: nat| #[trigger]
            ag.activations.contains_key(id) ==> exists|i: int|
                0 <= i < pops_spec(ag, n).len() && (#[trigger] pops_spec(ag, n)[i]).id == id,
    decreases n,
{
    theorem_pop(ag);
    if n > 0 {
        match pop_spec(ag).1 {
            None => {
                assert forall|id: nat| #[trigger] ag.activations.contains_key(id) implies false by {
                    assert(ag.activations.dom().contains(id));
                }
            },
            Some(a) => {
                let ag1 = pop_spec(ag).0;
                let m = (n - 1) as nat;
                theorem_pops_in_priority_order(ag1, m);
                let rest = pops_spec(ag1, m);
                let all = pops_spec(ag, n);
                assert(all == seq![a] + rest);
                assert(ag1.strategy == ag.strategy);
                assert forall|i: int| 0 <= i < all.len() implies ag.activations.contains_key(
                    (#[trigger] all[i]).id,
                ) && ag.activations[all[i].id] == all[i] && (i > 0 ==> all[i].id != a.id) by {
                    if i > 0 {
                        assert(all[i] == rest[i - 1]);
                    }
                }
                assert forall|i: int, j: int| 0 <= i < j < all.len() implies cmp_act_spec(
                    ag.strategy,
                    #[trigger] all[i],
                    #[trigger] all[j],
                ) == Ordering::Less && all[i].id != all[j].id by {
                    assert(all[j] == rest[j - 1]);
                    if i > 0 {
                        assert(all[i] == rest[i - 1]);
                    } else {
                        assert(ag.activations.contains_key(all[j].id));
                    }
                }
                if all.len() < n {
                    assert forall|id: nat| #[trigger] ag.activations.contains_key(id) implies exists|i: int|
                        0 <= i < all.len() && (#[trigger] all[i]).id == id by {
                        if id == a.id {
                            assert(all[0].id == id);
                        } else {
                            assert(ag1.activations.contains_key(id));
                            let k = choose|k: int| 0 <= k < rest.len() && (#[trigger] rest[k]).id == id;
                            assert(all[k + 1] == rest[k]);
                        }
                    }
                }
            },
        }
    }
}

/// Depth at the agenda level (CLIPS: newly activated rules are placed above
/// all rules of the same salience). After `add` under Depth the new
/// activation precedes every live activation whose salience is at most its
/// own and follows every live activation of higher salience; so if no live
/// activation has higher salience, the next `pop` returns it.
pub proof fn theorem_depth_add_places_new_first(ag: AgendaModel, activation: SAct, new_id: nat)
    requires
        inv(ag),
        ag.next_seq < u64::MAX,
        !ag.activations.contains_key(new_id),
        ag.strategy is Depth,
    ensures
        forall|id: nat| #[trigger]
            ag.activations.contains_key(id) ==> cmp_act_spec(
                ag.strategy,
                added_activation(ag, activation, new_id),
                ag.activations[id],
            ) == (if ag.activations[id].salience <= activation.salience {
                Ordering::Less
            } else {
                Ordering::Greater
            }),
        (forall|id: nat| #[trigger]
            ag.activations.contains_key(id) ==> ag.activations[id].salience <= activation.salience)
            ==> pop_spec(add_spec(ag, activation, new_id)).1 == Some(
            added_activation(ag, activation, new_id),
        ),
{
    let n = added_activation(ag, activation, new_id);
    theorem_add_preserves_inv(ag, activation, new_id);
    let ag2 = add_spec(ag, activation, new_id);
    assert forall|id: nat| #[trigger] ag.activations.contains_key(id) implies cmp_act_spec(
        ag.strategy,
        n,
        ag.activations[id],
    ) == (if ag.activations[id].salience <= activation.salience {
        Ordering::Less
    } else {
        Ordering::Greater
    }) by {
        assert(ag.activations[id].activation_seq < n.activation_seq);
    }
    if forall|id: nat| #[trigger]
        ag.activations.contains_key(id) ==> ag.activations[id].salience <= activation.salience {
        assert forall|id: nat| #[trigger]
            ag2.activations.contains_key(id) && id != new_id implies cmp_act_spec(
            ag2.strategy,
            ag2.activations[new_id],
            ag2.activations[id],
        ) == Ordering::Less by {
            assert(ag.activations.contains_key(id));
        }
        lemma_pop_returns_strict_max(ag2, new_id);
    }
}

/// Breadth at the agenda level (CLIPS: newly activated rules are placed
/// below all rules of the same salience). After `add` under Breadth the new
/// activation follows every live activation whose salience is at least its
/// own and precedes every live activation of lower salience; so if some live
/// activation has salience at least the new one's, the next `pop` returns
/// exactly what it would have returned before the `add`.
pub proof fn theorem_breadth_add_places_new_last(ag: AgendaModel, activation: SAct, new_id: nat)
    requires
        inv(ag),
        ag.next_seq < u64::MAX,
        !ag.activations.contains_key(new_id),
        ag.strategy is Breadth,
    ensures
        forall|id: nat| #[trigger]
            ag.activations.contains_key(id) ==> cmp_act_spec(
                ag.strategy,
                added_activation(ag, activation, new_id),
                ag.activations[id],
            ) == (if ag.activations[id].salience >= activation.salience {
                Ordering::Greater
            } else {
                Ordering::Less
            }),
        (exists|id: nat| #[trigger]
            ag.activations.contains_key(id) && ag.activations[id].salience >= activation.salience)
            ==> pop_spec(add_spec(ag, activation, new_id)).1 == pop_spec(ag).1,
{
    let n = added_activation(ag, activation, new_id);
    theorem_add_preserves_inv(ag, activation, new_id);
    let ag2 = add_spec(ag, activation, new_id);
    assert forall|id: nat| #[trigger] ag.activations.contains_key(id) implies cmp_act_spec(
        ag.strategy,
        n,
        ag.activations[id],
    ) == (if ag.activations[id].salience >= activation.salience {
        Ordering::Greater
    } else {
        Ordering::Less
    }) by {
        assert(ag.activations[id].activation_seq < n.activation_seq);
    }
    if exists|id: nat| #[trigger]
        ag.activations.contains_key(id) && ag.activations[id].salience >= activation.salience {
        let x = choose|id: nat| #[trigger]
            ag.activations.contains_key(id) && ag.activations[id].salience >= activation.salience;
        theorem_pop(ag);
        assert(ag.activations.dom().contains(x));
        assert(!(ag.activations.dom() =~= Set::<nat>::empty()));
        let top = pop_spec(ag).1->Some_0;
        let t = top.id;
        // The current top has salience at least that of x.
        assert(top.salience >= ag.activations[x].salience) by {
            if t != x {
                assert(cmp_act_spec(ag.strategy, top, ag.activations[x]) == Ordering::Less);
            }
        }
        assert forall|id: nat| #[trigger]
            ag2.activations.contains_key(id) && id != t implies cmp_act_spec(
            ag2.strategy,
            ag2.activations[t],
            ag2.activations[id],
        ) == Ordering::Less by {
            if id == new_id {
                lemma_key_flip(build_key_spec(ag.strategy, n), build_key_spec(ag.strategy, top));
            } else {
                assert(ag.activations.contains_key(id));
            }
        }
        lemma_pop_returns_strict_max(ag2, t);
    }
}

/// LEX at the agenda level: an activation added with salience at least that
/// of every live activation, and matching a fact more recent than every
/// fact of every live activation, is the next one popped (whatever the
/// complexities and chronology).
pub proof fn theorem_lex_add_most_recent_fact_fires_next(
    ag: AgendaModel,
    activation: SAct,
    new_id: nat,
    i: int,
)
    requires
        inv(ag),
        ag.next_seq < u64::MAX,
        !ag.activations.contains_key(new_id),
        ag.strategy is Lex,
        0 <= i < activation.recency.len(),
        forall|id: nat| #[trigger]
            ag.activations.contains_key(id) ==> ag.activations[id].salience <= activation.salience,
        forall|id: nat, j: int|
            ag.activations.contains_key(id) && 0 <= j < ag.activations[id].recency.len()
                ==> #[trigger] ag.activations[id].recency[j] < activation.recency[i],
    ensures
        pop_spec(add_spec(ag, activation, new_id)).1 == Some(
            added_activation(ag, activation, new_id),
        ),
{
    let n = added_activation(ag, activation, new_id);
    theorem_add_preserves_inv(ag, activation, new_id);
    let ag2 = add_spec(ag, activation, new_id);
    assert forall|id: nat| #[trigger]
        ag2.activations.contains_key(id) && id != new_id implies cmp_act_spec(
        ag2.strategy,
        ag2.activations[new_id],
        ag2.activations[id],
    ) == Ordering::Less by {
        assert(ag.activations.contains_key(id));
        let x = ag.activations[id];
        if x.salience < n.salience {
            theorem_salience_precedes_everything(Strategy::Lex, n, x);
        } else {
            assert forall|j: int| 0 <= j < x.recency.len() implies x.recency[j] < n.recency[i] by {
                assert(ag.activations[id].recency[j] < activation.recency[i]);
            }
            theorem_lex_most_recent_fact_wins(n, x, i);
        }
    }
    lemma_pop_returns_strict_max(ag2, new_id);
}

/// MEA at the agenda level: an activation added with salience at least that
/// of every live activation and a first-pattern fact more recent than every
/// live activation's first-pattern fact is the next one popped.
pub proof fn theorem_mea_add_most_recent_first_pattern_fires_next(
    ag: AgendaModel,
    activation: SAct,
    new_id: nat,
)
    requires
        inv(ag),
        ag.next_seq < u64::MAX,
        !ag.activations.contains_key(new_id),
        ag.strategy is Mea,
        forall|id: nat| #[trigger]
            ag.activations.contains_key(id) ==> ag.activations[id].salience <= activation.salience
                && first_or_absent(ag.activations[id].recency) < first_or_absent(
                activation.recency,
            ),
    ensures
        pop_spec(add_spec(ag, activation, new_id)).1 == Some(
            added_activation(ag, activation, new_id),
        ),
{
    let n = added_activation(ag, activation, new_id);
    theorem_add_preserves_inv(ag, activation, new_id);
    let ag2 = add_spec(ag, activation, new_id);
    assert forall|id: nat| #[trigger]
        ag2.activations.contains_key(id) && id != new_id implies cmp_act_spec(
        ag2.strategy,
        ag2.activations[new_id],
        ag2.activations[id],
    ) == Ordering::Less by {
        assert(ag.activations.contains_key(id));
        let x = ag.activations[id];
        if x.salience < n.salience {
            theorem_salience_precedes_everything(Strategy::Mea, n, x);
        } else {
            theorem_mea_first_pattern_dominates(n, x);
        }
    }
    lemma_pop_returns_strict_max(ag2, new_id);
}

// ---------------------------------------------------------------------
// Determinism: the agenda order does not depend on SlotMap iteration order
// ---------------------------------------------------------------------

/// Two strictly sorted entry sequences with the same entries are equal (a
/// BTreeMap's in-order traversal is determined by its contents).
pub proof fn lemma_sorted_entries_unique(s1: Entries, s2: Entries)
    requires
        strictly_sorted(s1),
        strictly_sorted(s2),
        forall|e: (SKey, nat)|
            #![trigger s1.contains(e)]
            #![trigger s2.contains(e)]
            s1.contains(e) <==> s2.contains(e),
    ensures
        s1 == s2,
    decreases s1.len(),
{
    if s1.len() == 0 {
        if s2.len() > 0 {
            assert(s2.contains(s2[0]));
        }
        assert(s1 =~= s2);
    } else {
        assert(s1.contains(s1[0]));
        assert(s2.len() > 0);
        assert(s2.contains(s2[0]));
        lemma_sorted_first_is_min(s1);
        lemma_sorted_first_is_min(s2);
        let i = choose|i: int| 0 <= i < s1.len() && s1[i] == s2[0];
        let j = choose|j: int| 0 <= j < s2.len() && s2[j] == s1[0];
        if i != 0 && j != 0 {
            // s1[0] < s1[i] = s2[0] < s2[j] = s1[0]: impossible.
            lemma_key_flip(s1[0].0, s2[0].0);
        }
        if i == 0 && j != 0 {
            lemma_key_refl(s1[0].0);
        }
        if i != 0 && j == 0 {
            lemma_key_refl(s1[0].0);
        }
        assert(s1[0] == s2[0]);
        let t1 = s1.drop_first();
        let t2 = s2.drop_first();
        lemma_sorted_drop_first(s1);
        lemma_sorted_drop_first(s2);
        assert forall|e: (SKey, nat)|
            #![trigger t1.contains(e)]
            #![trigger t2.contains(e)]
            t1.contains(e) <==> t2.contains(e) by {
            if t1.contains(e) {
                let m = choose|m: int| 0 <= m < t1.len() && t1[m] == e;
                assert(s1[m + 1] == e);
                assert(s1[m + 1].0 != s1[0].0);
                assert(s2.contains(e));
                let n = choose|n: int| 0 <= n < s2.len() && s2[n] == e;
                assert(n != 0);
                assert(t2[n - 1] == e);
            }
            if t2.contains(e) {
                let m = choose|m: int| 0 <= m < t2.len() && t2[m] == e;
                assert(s2[m + 1] == e);
                assert(s2[m + 1].0 != s2[0].0);
                assert(s1.contains(e));
                let n = choose|n: int| 0 <= n < s1.len() && s1[n] == e;
                assert(n != 0);
                assert(t1[n - 1] == e);
            }
        }
        lemma_sorted_entries_unique(t1, t2);
        assert(s1 =~= seq![s1[0]] + t1);
        assert(s2 =~= seq![s2[0]] + t2);
    }
}

/// THEOREM (determinism). The agenda produced by `set_strategy` does not
/// depend on the SlotMap iteration order used to rebuild it.
pub proof fn theorem_set_strategy_deterministic(
    ag: AgendaModel,
    strategy: Strategy,
    ids1: Seq<nat>,
    ids2: Seq<nat>,
)
    requires
        inv(ag),
        enumerates_live(ag, ids1),
        enumerates_live(ag, ids2),
    ensures
        set_strategy_spec(ag, strategy, ids1) == set_strategy_spec(ag, strategy, ids2),
{
    theorem_set_strategy(ag, strategy, ids1);
    theorem_set_strategy(ag, strategy, ids2);
    let r1 = set_strategy_spec(ag, strategy, ids1).0;
    let r2 = set_strategy_spec(ag, strategy, ids2).0;
    assert(r1.id_to_key =~= r2.id_to_key) by {
        assert forall|id: nat| #[trigger] r1.id_to_key.contains_key(id) <==> r2.id_to_key.contains_key(
            id,
        ) by {}
        assert forall|id: nat| #[trigger] r1.id_to_key.contains_key(id) implies r1.id_to_key[id]
            == r2.id_to_key[id] by {
            assert(r1.activations.contains_key(id));
        }
    }
    assert forall|e: (SKey, nat)|
        #![trigger r1.ordering.contains(e)]
        #![trigger r2.ordering.contains(e)]
        r1.ordering.contains(e) <==> r2.ordering.contains(
        e,
    ) by {}
    lemma_sorted_entries_unique(r1.ordering, r2.ordering);
}

}
