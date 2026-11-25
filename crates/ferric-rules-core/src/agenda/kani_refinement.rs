//! Kani refinement harnesses linking `agenda`'s conflict-resolution ordering
//! to its Verus model, `verification/verus/conflict_strategy.rs`.
//!
//! `model` below is a plain-Rust transcription of the Verus exec functions
//! (`cmp_u64`/`cmp_i32`/`cmp_u16`, `cmp_vecs`, `discriminant`,
//! `cmp_strategy_ord`, `cmp_key`, `sort_desc`, `build_key`,
//! `cmp_activations`) with the ghost/proof annotations removed. Verus proves
//! those functions equal to the spec functions about which the total-order
//! and strategy theorems are stated; the harnesses here prove, on bounded
//! inputs, that the transcription computes exactly what production computes:
//!
//! * `key_cmp_refines_model`: for two arbitrary `AgendaKey`s (any
//!   `StrategyOrd` variants, cross-variant pairs included, arbitrary unsorted
//!   recency vectors of every length 0..=3, all scalars unconstrained), the
//!   derived `Ord::cmp` equals `model::cmp_key`, and `partial_cmp` agrees
//!   with `cmp`.
//! * `build_key_refines_model_{depth,breadth,lex}` (basis lengths 0..=4),
//!   `build_key_refines_model_mea` (0..=3) and `..._mea_len4` (4): for an
//!   activation with unconstrained salience, complexity, sequence and tags,
//!   `Agenda::build_key` (clone plus `sort_unstable_by` for LEX/MEA) produces
//!   exactly `model::build_key` (insertion sort), compared field by field
//!   after the abstraction map `key_to_model` (which strips the newtypes and
//!   `Reverse` wrappers).
//! * `activation_cmp_refines_model_{depth,breadth,lex,mea}`: end to end, for
//!   two such activations with every pair of basis lengths in 0..=3, the
//!   production key comparison equals `model::cmp_activations`.
//!
//! Bounds: recency vectors as stated above, at most the `SmallVec` inline
//! capacity 4 (the Verus proofs are unbounded); every integer field ranges
//! over its whole type. Lengths are concrete in the build-key and activation
//! harnesses (see `any_recency_of_len`).
//! Known gaps: recency vectors longer than these bounds, in particular the
//! `SmallVec` heap representation beyond 4 tags, are not exercised; the
//! agenda operations (`add`, `pop`, ...) are modelled in Verus but are not
//! linked to production here; agreement of this transcription with the Verus
//! source is by inspection (it is line-for-line).

use super::*;
use std::cmp::Ordering;

/// Plain-Rust transcription of the exec code in
/// `verification/verus/conflict_strategy.rs` (ghost code removed). The
/// activation's `id`, `rule` and `token` fields are omitted because no
/// transcribed function reads them.
mod model {
    use std::cmp::Ordering;

    #[derive(Clone, Copy)]
    pub enum Strategy {
        Depth,
        Breadth,
        Lex,
        Mea,
    }

    pub struct Activation {
        pub salience: i32,
        pub complexity: u16,
        pub activation_seq: u64,
        pub recency: Vec<u64>,
    }

    pub enum StrategyOrd {
        Depth(u64),
        Breadth(u64),
        Lex(Vec<u64>),
        Mea {
            first_recency: u64,
            sorted_recency: Vec<u64>,
        },
    }

    pub struct AgendaKey {
        pub salience: i32,
        pub strategy_ord: StrategyOrd,
        pub complexity: u16,
        pub seq: u64,
    }

    pub fn cmp_u64(a: u64, b: u64) -> Ordering {
        if a < b {
            Ordering::Less
        } else if a == b {
            Ordering::Equal
        } else {
            Ordering::Greater
        }
    }

    pub fn cmp_i32(a: i32, b: i32) -> Ordering {
        if a < b {
            Ordering::Less
        } else if a == b {
            Ordering::Equal
        } else {
            Ordering::Greater
        }
    }

    pub fn cmp_u16(a: u16, b: u16) -> Ordering {
        if a < b {
            Ordering::Less
        } else if a == b {
            Ordering::Equal
        } else {
            Ordering::Greater
        }
    }

    pub fn cmp_vecs(left: &Vec<u64>, right: &Vec<u64>) -> Ordering {
        let l = if left.len() < right.len() {
            left.len()
        } else {
            right.len()
        };
        let mut i: usize = 0;
        while i < l {
            match cmp_u64(left[i], right[i]) {
                Ordering::Equal => {}
                non_eq => {
                    return non_eq;
                }
            }
            i += 1;
        }
        cmp_u64(left.len() as u64, right.len() as u64)
    }

    pub fn discriminant(o: &StrategyOrd) -> u64 {
        match o {
            StrategyOrd::Depth(_) => 0,
            StrategyOrd::Breadth(_) => 1,
            StrategyOrd::Lex(_) => 2,
            StrategyOrd::Mea { .. } => 3,
        }
    }

    pub fn cmp_strategy_ord(a: &StrategyOrd, b: &StrategyOrd) -> Ordering {
        match (a, b) {
            (StrategyOrd::Depth(x), StrategyOrd::Depth(y)) => cmp_u64(*y, *x),
            (StrategyOrd::Breadth(x), StrategyOrd::Breadth(y)) => cmp_u64(*x, *y),
            (StrategyOrd::Lex(x), StrategyOrd::Lex(y)) => cmp_vecs(y, x),
            (
                StrategyOrd::Mea {
                    first_recency: f1,
                    sorted_recency: s1,
                },
                StrategyOrd::Mea {
                    first_recency: f2,
                    sorted_recency: s2,
                },
            ) => {
                match cmp_u64(*f2, *f1) {
                    Ordering::Equal => {}
                    non_eq => {
                        return non_eq;
                    }
                }
                cmp_vecs(s2, s1)
            }
            _ => cmp_u64(discriminant(a), discriminant(b)),
        }
    }

    pub fn cmp_key(a: &AgendaKey, b: &AgendaKey) -> Ordering {
        match cmp_i32(b.salience, a.salience) {
            Ordering::Equal => {}
            non_eq => {
                return non_eq;
            }
        }
        match cmp_strategy_ord(&a.strategy_ord, &b.strategy_ord) {
            Ordering::Equal => {}
            non_eq => {
                return non_eq;
            }
        }
        match cmp_u16(b.complexity, a.complexity) {
            Ordering::Equal => {}
            non_eq => {
                return non_eq;
            }
        }
        cmp_u64(a.seq, b.seq)
    }

    pub fn sort_desc(v: &Vec<u64>) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        let mut i: usize = 0;
        while i < v.len() {
            let x = v[i];
            let mut j: usize = 0;
            while j < out.len() && out[j] >= x {
                j += 1;
            }
            out.insert(j, x);
            i += 1;
        }
        out
    }

    pub fn build_key(strategy: Strategy, activation: &Activation) -> AgendaKey {
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
            }
        };
        AgendaKey {
            salience: activation.salience,
            strategy_ord,
            complexity: activation.complexity,
            seq: activation.activation_seq,
        }
    }

    pub fn cmp_activations(strategy: Strategy, a: &Activation, b: &Activation) -> Ordering {
        let ka = build_key(strategy, a);
        let kb = build_key(strategy, b);
        cmp_key(&ka, &kb)
    }
}

// ---------------------------------------------------------------------
// Abstraction maps (production -> model)
// ---------------------------------------------------------------------

fn tags_to_model(tags: &[RecencyTag]) -> Vec<u64> {
    tags.iter().map(|tag| tag.0).collect()
}

fn key_to_model(key: &AgendaKey) -> model::AgendaKey {
    model::AgendaKey {
        salience: key.salience.0.get(),
        strategy_ord: match &key.strategy_ord {
            StrategyOrd::Depth(seq) => model::StrategyOrd::Depth(seq.0.get()),
            StrategyOrd::Breadth(seq) => model::StrategyOrd::Breadth(seq.get()),
            StrategyOrd::Lex(sorted) => model::StrategyOrd::Lex(tags_to_model(&sorted.0)),
            StrategyOrd::Mea {
                first_recency,
                sorted_recency,
            } => model::StrategyOrd::Mea {
                first_recency: first_recency.0 .0,
                sorted_recency: tags_to_model(&sorted_recency.0),
            },
        },
        complexity: key.complexity.0,
        seq: key.seq.get(),
    }
}

fn activation_to_model(activation: &Activation) -> model::Activation {
    model::Activation {
        salience: activation.salience.get(),
        complexity: activation.complexity,
        activation_seq: activation.activation_seq.get(),
        recency: tags_to_model(&activation.recency),
    }
}

/// Element-wise equality (an explicit loop: the derived slice equality lowers
/// to a byte-wise `memcmp` whose loop needs eight times the unwinding).
fn same_tags(a: &[u64], b: &[u64]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Structural equality of model keys.
fn same_key(a: &model::AgendaKey, b: &model::AgendaKey) -> bool {
    let same_ord = match (&a.strategy_ord, &b.strategy_ord) {
        (model::StrategyOrd::Depth(x), model::StrategyOrd::Depth(y))
        | (model::StrategyOrd::Breadth(x), model::StrategyOrd::Breadth(y)) => x == y,
        (model::StrategyOrd::Lex(x), model::StrategyOrd::Lex(y)) => same_tags(x, y),
        (
            model::StrategyOrd::Mea {
                first_recency: f1,
                sorted_recency: s1,
            },
            model::StrategyOrd::Mea {
                first_recency: f2,
                sorted_recency: s2,
            },
        ) => f1 == f2 && same_tags(s1, s2),
        _ => false,
    };
    a.salience == b.salience && same_ord && a.complexity == b.complexity && a.seq == b.seq
}

fn strategy_to_model(strategy: ConflictResolutionStrategy) -> model::Strategy {
    match strategy {
        ConflictResolutionStrategy::Depth => model::Strategy::Depth,
        ConflictResolutionStrategy::Breadth => model::Strategy::Breadth,
        ConflictResolutionStrategy::Lex => model::Strategy::Lex,
        ConflictResolutionStrategy::Mea => model::Strategy::Mea,
    }
}

// ---------------------------------------------------------------------
// Symbolic inputs
// ---------------------------------------------------------------------

/// A recency vector of symbolic length `<= max_len` (at most the inline
/// capacity 4) with unconstrained tags.
fn any_recency(max_len: usize) -> SmallVec<[RecencyTag; 4]> {
    let buf: [u64; 4] = kani::any();
    let len: usize = kani::any_where(|len: &usize| *len <= max_len);
    SmallVec::from_buf_and_len(buf.map(RecencyTag), len)
}

/// A recency vector of exactly `len` (a concrete number <= 4) unconstrained
/// tags. The length is kept concrete for anything that is sorted: with a
/// symbolic length CBMC also unrolls the `ipnsort` path that
/// `sort_unstable_by` takes for slices longer than 20, and symbolic execution
/// does not finish.
fn any_recency_of_len(len: usize) -> SmallVec<[RecencyTag; 4]> {
    let buf: [u64; 4] = kani::any();
    SmallVec::from_buf_and_len(buf.map(RecencyTag), len)
}

/// An activation with every field read by `build_key` unconstrained and a
/// basis of exactly `len` tags.
fn any_activation(len: usize) -> Activation {
    Activation {
        id: ActivationId::default(),
        rule: RuleId(0),
        token: TokenId::default(),
        salience: Salience::new(kani::any()),
        complexity: kani::any(),
        timestamp: Timestamp::ZERO,
        activation_seq: ActivationSeq::new(kani::any()),
        recency: any_recency_of_len(len),
    }
}

/// An arbitrary key of any `StrategyOrd` variant (recency vectors need not
/// be sorted: the derived `Ord` is checked on its whole domain).
fn any_key(max_len: usize) -> AgendaKey {
    let variant: u8 = kani::any_where(|variant: &u8| *variant < 4);
    let strategy_ord = match variant {
        0 => StrategyOrd::Depth(std::cmp::Reverse(ActivationSeq::new(kani::any()))),
        1 => StrategyOrd::Breadth(ActivationSeq::new(kani::any())),
        2 => StrategyOrd::Lex(std::cmp::Reverse(any_recency(max_len))),
        _ => StrategyOrd::Mea {
            first_recency: std::cmp::Reverse(RecencyTag(kani::any())),
            sorted_recency: std::cmp::Reverse(any_recency(max_len)),
        },
    };
    AgendaKey {
        salience: std::cmp::Reverse(Salience::new(kani::any())),
        strategy_ord,
        complexity: std::cmp::Reverse(kani::any()),
        seq: ActivationSeq::new(kani::any()),
    }
}

fn variant_of(key: &AgendaKey) -> u8 {
    match key.strategy_ord {
        StrategyOrd::Depth(_) => 0,
        StrategyOrd::Breadth(_) => 1,
        StrategyOrd::Lex(_) => 2,
        StrategyOrd::Mea { .. } => 3,
    }
}

// ---------------------------------------------------------------------
// Harnesses
// ---------------------------------------------------------------------

/// The derived `Ord` (and `PartialOrd`) on `AgendaKey` equals the model's
/// `cmp_key` on all keys with recency vectors of length <= 3.
#[kani::proof]
#[kani::unwind(5)]
fn key_cmp_refines_model() {
    let a = any_key(3);
    let b = any_key(3);
    let production = a.cmp(&b);
    assert_eq!(
        production,
        model::cmp_key(&key_to_model(&a), &key_to_model(&b))
    );
    assert_eq!(a.partial_cmp(&b), Some(production));

    kani::cover!(production == Ordering::Less, "less");
    kani::cover!(production == Ordering::Equal, "equal");
    kani::cover!(production == Ordering::Greater, "greater");
    kani::cover!(variant_of(&a) != variant_of(&b), "cross-variant pair");
    kani::cover!(
        variant_of(&a) == 3 && variant_of(&b) == 3 && a.salience == b.salience,
        "MEA pair at equal salience"
    );
    kani::cover!(
        matches!(
            (&a.strategy_ord, &b.strategy_ord),
            (StrategyOrd::Lex(x), StrategyOrd::Lex(y))
                if x.0.len() == 3 && y.0.len() == 2 && x.0[..2] == y.0[..]
        ) && a.salience == b.salience,
        "LEX proper-prefix pair at equal salience"
    );
}

fn check_build_key(strategy: ConflictResolutionStrategy, len: usize) {
    let activation = any_activation(len);
    let agenda = Agenda::with_strategy(strategy);
    let production = agenda.build_key(&activation);
    let expected = model::build_key(
        strategy_to_model(strategy),
        &activation_to_model(&activation),
    );
    assert!(same_key(&key_to_model(&production), &expected));

    let tags = &activation.recency;
    kani::cover!(
        tags.len() >= 2 && tags[0] < tags[tags.len() - 1],
        "basis not already in descending order"
    );
    kani::cover!(
        tags.len() >= 3 && tags[0] == tags[2] && tags[1] > tags[0],
        "basis with a repeated tag around a larger one"
    );
}

/// `Agenda::build_key` equals `model::build_key` under Depth, for every
/// basis length 0..=4.
#[kani::proof]
#[kani::unwind(6)]
fn build_key_refines_model_depth() {
    for len in 0..=4 {
        check_build_key(ConflictResolutionStrategy::Depth, len);
    }
}

/// `Agenda::build_key` equals `model::build_key` under Breadth, for every
/// basis length 0..=4.
#[kani::proof]
#[kani::unwind(6)]
fn build_key_refines_model_breadth() {
    for len in 0..=4 {
        check_build_key(ConflictResolutionStrategy::Breadth, len);
    }
}

/// `Agenda::build_key` (clone + `sort_unstable_by` descending) equals
/// `model::build_key` (insertion sort) under LEX, for every basis length
/// 0..=4.
#[kani::proof]
#[kani::unwind(6)]
fn build_key_refines_model_lex() {
    for len in 0..=4 {
        check_build_key(ConflictResolutionStrategy::Lex, len);
    }
}

/// `Agenda::build_key` equals `model::build_key` under MEA (original first
/// tag plus the sorted basis), for every basis length 0..=3. (Lengths 0..=4
/// in one harness exceed the 7 GB memory cap during propositional
/// reduction; length 4 is checked separately below.)
#[kani::proof]
#[kani::unwind(6)]
fn build_key_refines_model_mea() {
    for len in 0..=3 {
        check_build_key(ConflictResolutionStrategy::Mea, len);
    }
}

/// `Agenda::build_key` equals `model::build_key` under MEA for a full inline
/// basis of 4 tags.
#[kani::proof]
#[kani::unwind(6)]
fn build_key_refines_model_mea_len4() {
    check_build_key(ConflictResolutionStrategy::Mea, 4);
}

fn check_activation_cmp(strategy: ConflictResolutionStrategy, len_a: usize, len_b: usize) {
    let a = any_activation(len_a);
    let b = any_activation(len_b);
    let agenda = Agenda::with_strategy(strategy);
    let production = agenda.build_key(&a).cmp(&agenda.build_key(&b));
    let expected = model::cmp_activations(
        strategy_to_model(strategy),
        &activation_to_model(&a),
        &activation_to_model(&b),
    );
    assert_eq!(production, expected);

    kani::cover!(
        production == Ordering::Less && a.salience == b.salience,
        "a fires first at equal salience"
    );
    kani::cover!(
        production == Ordering::Greater && a.salience == b.salience,
        "b fires first at equal salience"
    );
}

/// End to end: production key comparison equals `model::cmp_activations`
/// under Depth.
#[kani::proof]
#[kani::unwind(5)]
fn activation_cmp_refines_model_depth() {
    for len_a in 0..=3 {
        for len_b in 0..=3 {
            check_activation_cmp(ConflictResolutionStrategy::Depth, len_a, len_b);
        }
    }
}

/// End to end under Breadth.
#[kani::proof]
#[kani::unwind(5)]
fn activation_cmp_refines_model_breadth() {
    for len_a in 0..=3 {
        for len_b in 0..=3 {
            check_activation_cmp(ConflictResolutionStrategy::Breadth, len_a, len_b);
        }
    }
}

/// LEX/MEA end-to-end check for one basis length of `a` against every basis
/// length 0..=3 of `b`. (All sixteen length pairs in one harness exceed the
/// 7 GB memory cap during propositional reduction, so the rows are split.)
fn check_activation_cmp_row(strategy: ConflictResolutionStrategy, len_a: usize) {
    for len_b in 0..=3 {
        check_activation_cmp(strategy, len_a, len_b);
    }
}

/// End to end under LEX, `a` with an empty basis.
#[kani::proof]
#[kani::unwind(5)]
fn activation_cmp_refines_model_lex_a0() {
    check_activation_cmp_row(ConflictResolutionStrategy::Lex, 0);
}

/// End to end under LEX, `a` with 1 tag.
#[kani::proof]
#[kani::unwind(5)]
fn activation_cmp_refines_model_lex_a1() {
    check_activation_cmp_row(ConflictResolutionStrategy::Lex, 1);
}

/// End to end under LEX, `a` with 2 tags.
#[kani::proof]
#[kani::unwind(5)]
fn activation_cmp_refines_model_lex_a2() {
    check_activation_cmp_row(ConflictResolutionStrategy::Lex, 2);
}

/// End to end under LEX, `a` with 3 tags.
#[kani::proof]
#[kani::unwind(5)]
fn activation_cmp_refines_model_lex_a3() {
    check_activation_cmp_row(ConflictResolutionStrategy::Lex, 3);
}

/// End to end under MEA, `a` with an empty basis.
#[kani::proof]
#[kani::unwind(5)]
fn activation_cmp_refines_model_mea_a0() {
    check_activation_cmp_row(ConflictResolutionStrategy::Mea, 0);
}

/// End to end under MEA, `a` with 1 tag.
#[kani::proof]
#[kani::unwind(5)]
fn activation_cmp_refines_model_mea_a1() {
    check_activation_cmp_row(ConflictResolutionStrategy::Mea, 1);
}

/// End to end under MEA, `a` with 2 tags.
#[kani::proof]
#[kani::unwind(5)]
fn activation_cmp_refines_model_mea_a2() {
    check_activation_cmp_row(ConflictResolutionStrategy::Mea, 2);
}

/// End to end under MEA, `a` with 3 tags.
#[kani::proof]
#[kani::unwind(5)]
fn activation_cmp_refines_model_mea_a3() {
    check_activation_cmp_row(ConflictResolutionStrategy::Mea, 3);
}
