//! Kani proof harnesses for `strategy` and the order relations that realize
//! it (see docs/verification.md).
//!
//! A `ConflictResolutionStrategy` takes effect through `Agenda::build_key`,
//! which maps an activation to an `AgendaKey`; the agenda fires activations in
//! ascending key order (`Less` fires first). This file proves the order
//! relations themselves; `agenda/kani_proofs.rs` proves that `build_key`
//! implements the strategy semantics documented in `strategy.rs`.
//!
//! Proved, for every input within the bounds stated on each harness:
//!
//! - `strategy_equality_is_variant_identity`: `==`/`!=` on
//!   `ConflictResolutionStrategy` is variant identity (an equivalence), copies
//!   and clones compare equal, and the default is `Depth`.
//! - Lawful total order consistent with `Eq` for `Salience`, `ActivationSeq`,
//!   `RecencyTag`, `Reverse` of each (the agenda's wrappers), `Reverse<u16>`
//!   (complexity), recency vectors (`SmallVec<[RecencyTag; 4]>`), each
//!   per-strategy key (`StrategyOrd::{Depth, Breadth, Lex, Mea}`), `StrategyOrd`
//!   across variants, and `AgendaKey`, checked over three symbolic values:
//!   reflexivity; antisymmetry (`b.cmp(a) == a.cmp(b).reverse()`);
//!   transitivity of `<=`, `<` and `==`; substitutivity of `==`; totality
//!   (exactly one of `<`, `==`, `>` holds); and agreement of `cmp`,
//!   `partial_cmp`, `==`, `!=`, `<`, `<=`, `>`, `>=`.
//! - Exact characterization: in each of those harnesses `a.cmp(&b)` equals an
//!   independent specification written from the module documentation rather
//!   than from the derives: integer order for the scalars (reversed under
//!   `Reverse`); `ABSENT` below every fact recency; lexicographic order with a
//!   proper prefix smaller for recency vectors; depth = newer sequence first,
//!   breadth = older first, LEX = larger recency vector first, MEA = larger
//!   first-pattern recency first and then LEX; between variants, declaration
//!   order; `AgendaKey` = higher salience, then the strategy component, then
//!   higher complexity, then older sequence.
//!
//! Bounds and domains:
//!
//! - Recency vectors have length at most 4, the `SmallVec` inline capacity.
//!   Longer (spilled) vectors compare through the same slice `Ord` but are not
//!   exercised here.
//! - Harnesses marked "byte domain" draw recency tags, first-pattern recencies
//!   and sequence numbers from `0..=255` widened to `u64`. The relations under
//!   test only compare these values (derived `Ord`, no arithmetic), so each
//!   result depends only on the relative order of the at most 18 values
//!   involved, and every relative order of at most 256 values occurs in the
//!   byte domain. Salience and complexity are always full width, as are the
//!   two-value exact-specification checks in
//!   `recency_vector_order_is_lexicographic` and in `agenda/kani_proofs.rs`
//!   (`build_key_*`).
//! - `strategy_ord_is_total_order_across_variants` and
//!   `agenda_key_is_total_order` use recency vectors of length at most 2; the
//!   per-variant harnesses cover length 4.
//!
//! Known gaps: unbounded vector lengths are not covered by Kani. No property
//! of `Hash` is checked (agenda keys are not hashed).

use super::*;
use crate::agenda::{ActivationSeq, AgendaKey, RecencyTag, StrategyOrd};
use crate::beta::Salience;
use crate::fact::Timestamp;
use smallvec::SmallVec;
use std::cmp::{Ordering, Reverse};

const ALL_STRATEGIES: [ConflictResolutionStrategy; 4] = [
    ConflictResolutionStrategy::Depth,
    ConflictResolutionStrategy::Breadth,
    ConflictResolutionStrategy::Lex,
    ConflictResolutionStrategy::Mea,
];

// ---------------------------------------------------------------------------
// Generic order-law checker
// ---------------------------------------------------------------------------

/// Checks that `cmp`, `partial_cmp`, `==`, `!=`, `<`, `<=`, `>`, `>=` agree on
/// `(a, b)` and that `b.cmp(a)` is the reverse of `a.cmp(b)` (antisymmetry).
/// Returns `a.cmp(b)`.
fn check_pair<T: Ord>(a: &T, b: &T) -> Ordering {
    let ab = a.cmp(b);
    let partial = a.partial_cmp(b);
    let (eq, ne) = (a == b, a != b);
    let (lt, le, gt, ge) = (a < b, a <= b, a > b, a >= b);
    let ba = b.cmp(a);
    assert_eq!(partial, Some(ab));
    assert_eq!(eq, ab == Ordering::Equal);
    assert_eq!(ne, !eq);
    assert_eq!(lt, ab == Ordering::Less);
    assert_eq!(le, ab != Ordering::Greater);
    assert_eq!(gt, ab == Ordering::Greater);
    assert_eq!(ge, ab != Ordering::Less);
    assert_eq!(ba, ab.reverse());
    ab
}

/// Total-order laws over three arbitrary values: reflexivity, antisymmetry,
/// transitivity of `<=`, `<` and `==`, substitutivity of `==`, totality
/// (exactly one of `<`, `==`, `>` holds, via [`check_pair`]), and agreement of
/// every comparison operator with `cmp`. Returns `(a.cmp(b), b.cmp(c))`.
fn check_total_order<T: Ord>(a: &T, b: &T, c: &T) -> (Ordering, Ordering) {
    assert_eq!(a.cmp(a), Ordering::Equal);
    assert_eq!(a.partial_cmp(a), Some(Ordering::Equal));
    assert!(a == a);
    let ab = check_pair(a, b);
    let bc = check_pair(b, c);
    let ac = check_pair(a, c);
    if ab != Ordering::Greater && bc != Ordering::Greater {
        assert!(ac != Ordering::Greater);
    }
    if ab == Ordering::Less && bc == Ordering::Less {
        assert_eq!(ac, Ordering::Less);
    }
    if ab == Ordering::Equal {
        assert_eq!(ac, bc);
    }
    if bc == Ordering::Equal {
        assert_eq!(ac, ab);
    }
    (ab, bc)
}

/// Independent three-way comparison of two integers.
fn spec_cmp<T: PartialOrd>(a: T, b: T) -> Ordering {
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

// ---------------------------------------------------------------------------
// Symbolic inputs
// ---------------------------------------------------------------------------

/// Production `RecencyTag` whose encoded value is `v` (0 is `ABSENT`).
fn tag(v: u64) -> RecencyTag {
    match v.checked_sub(1) {
        None => RecencyTag::ABSENT,
        Some(t) => RecencyTag::from_timestamp(Timestamp::new(t)).unwrap(),
    }
}

/// Element domain for recency vectors and sequence numbers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Width {
    /// Every `u64`.
    Full,
    /// `u8` values widened to `u64` (see the module documentation).
    Byte,
}

fn any_u64(width: Width) -> u64 {
    match width {
        Width::Full => kani::any(),
        Width::Byte => u64::from(kani::any::<u8>()),
    }
}

#[derive(Clone, Copy)]
struct RawRecency {
    tags: [u64; 4],
    len: usize,
}

impl RawRecency {
    fn any(max_len: usize, width: Width) -> Self {
        Self {
            tags: [
                any_u64(width),
                any_u64(width),
                any_u64(width),
                any_u64(width),
            ],
            len: kani::any_where(|l: &usize| *l <= max_len),
        }
    }

    fn slice(&self) -> &[u64] {
        &self.tags[..self.len]
    }

    fn to_prod(self) -> SmallVec<[RecencyTag; 4]> {
        SmallVec::from_buf_and_len(self.tags.map(tag), self.len)
    }
}

/// Independent lexicographic comparison in which a proper prefix is smaller.
fn spec_lex(a: &[u64], b: &[u64]) -> Ordering {
    let mut i = 0;
    loop {
        if i == a.len() && i == b.len() {
            return Ordering::Equal;
        }
        if i == a.len() {
            return Ordering::Less;
        }
        if i == b.len() {
            return Ordering::Greater;
        }
        if a[i] < b[i] {
            return Ordering::Less;
        }
        if a[i] > b[i] {
            return Ordering::Greater;
        }
        i += 1;
    }
}

const DEPTH: u8 = 0;
const BREADTH: u8 = 1;
const LEX: u8 = 2;
const MEA: u8 = 3;

/// Raw data from which a production `StrategyOrd` and its specification are
/// both built.
#[derive(Clone, Copy)]
struct RawStrategyOrd {
    variant: u8,
    seq: u64,
    first: u64,
    recency: RawRecency,
}

impl RawStrategyOrd {
    fn any(variant: Option<u8>, max_len: usize, width: Width) -> Self {
        let variant = match variant {
            Some(v) => v,
            None => kani::any_where(|v: &u8| *v <= MEA),
        };
        Self {
            variant,
            seq: any_u64(width),
            first: any_u64(width),
            recency: RawRecency::any(max_len, width),
        }
    }

    fn to_prod(self) -> StrategyOrd {
        match self.variant {
            DEPTH => StrategyOrd::Depth(Reverse(ActivationSeq::new(self.seq))),
            BREADTH => StrategyOrd::Breadth(ActivationSeq::new(self.seq)),
            LEX => StrategyOrd::Lex(Reverse(self.recency.to_prod())),
            _ => StrategyOrd::Mea {
                first_recency: Reverse(tag(self.first)),
                sorted_recency: Reverse(self.recency.to_prod()),
            },
        }
    }
}

/// Specification of the strategy component, read as "`Less` fires first".
///
/// Within one strategy: depth fires the newer (larger) activation sequence
/// first; breadth the older one; LEX the lexicographically larger recency
/// vector; MEA the larger first-pattern recency, then the LEX comparison.
/// Keys of different strategies never coexist in one agenda; between them the
/// order is the declaration order of the variants.
fn spec_strategy(a: &RawStrategyOrd, b: &RawStrategyOrd) -> Ordering {
    if a.variant != b.variant {
        return spec_cmp(a.variant, b.variant);
    }
    match a.variant {
        DEPTH => spec_cmp(b.seq, a.seq),
        BREADTH => spec_cmp(a.seq, b.seq),
        LEX => spec_lex(b.recency.slice(), a.recency.slice()),
        _ => match spec_cmp(b.first, a.first) {
            Ordering::Equal => spec_lex(b.recency.slice(), a.recency.slice()),
            decided => decided,
        },
    }
}

#[derive(Clone, Copy)]
struct RawAgendaKey {
    salience: i32,
    strategy: RawStrategyOrd,
    complexity: u16,
    seq: u64,
}

impl RawAgendaKey {
    fn any(variant: Option<u8>, max_len: usize, width: Width) -> Self {
        Self {
            salience: kani::any(),
            strategy: RawStrategyOrd::any(variant, max_len, width),
            complexity: kani::any(),
            seq: any_u64(width),
        }
    }

    fn to_prod(self) -> AgendaKey {
        AgendaKey {
            salience: Reverse(Salience::new(self.salience)),
            strategy_ord: self.strategy.to_prod(),
            complexity: Reverse(self.complexity),
            seq: ActivationSeq::new(self.seq),
        }
    }
}

/// Specification of the full agenda key ("`Less` fires first"): higher
/// salience, then the strategy component, then higher complexity, then the
/// older activation sequence.
fn spec_agenda_key(a: &RawAgendaKey, b: &RawAgendaKey) -> Ordering {
    match spec_cmp(b.salience, a.salience) {
        Ordering::Equal => {}
        decided => return decided,
    }
    match spec_strategy(&a.strategy, &b.strategy) {
        Ordering::Equal => {}
        decided => return decided,
    }
    match spec_cmp(b.complexity, a.complexity) {
        Ordering::Equal => {}
        decided => return decided,
    }
    spec_cmp(a.seq, b.seq)
}

// ---------------------------------------------------------------------------
// ConflictResolutionStrategy
// ---------------------------------------------------------------------------

#[kani::proof]
fn strategy_equality_is_variant_identity() {
    let i: usize = kani::any_where(|i: &usize| *i < 4);
    let j: usize = kani::any_where(|j: &usize| *j < 4);
    let k: usize = kani::any_where(|k: &usize| *k < 4);
    let (a, b, c) = (ALL_STRATEGIES[i], ALL_STRATEGIES[j], ALL_STRATEGIES[k]);
    assert_eq!(a == b, i == j);
    assert_eq!(a != b, i != j);
    assert!(a == a);
    assert_eq!(a == b, b == a);
    if a == b && b == c {
        assert!(a == c);
    }
    let copied = a;
    #[allow(clippy::clone_on_copy)]
    let cloned = a.clone();
    assert!(copied == a && cloned == a);
    assert_eq!(
        ConflictResolutionStrategy::default(),
        ConflictResolutionStrategy::Depth
    );
    kani::cover!(i == j);
    kani::cover!(i != j);
    kani::cover!(a == b && b == c);
}

// ---------------------------------------------------------------------------
// Scalar key components
// ---------------------------------------------------------------------------

#[kani::proof]
fn salience_is_total_order_by_value() {
    let (x, y, z): (i32, i32, i32) = kani::any();
    let (a, b, c) = (Salience::new(x), Salience::new(y), Salience::new(z));
    let (ab, _) = check_total_order(&a, &b, &c);
    assert_eq!(ab, spec_cmp(x, y));
    assert_eq!(a.get(), x);
    // Agenda keys wrap salience in `Reverse`: higher salience sorts first.
    let (ra, rb, rc) = (Reverse(a), Reverse(b), Reverse(c));
    let (rab, _) = check_total_order(&ra, &rb, &rc);
    assert_eq!(rab, spec_cmp(y, x));
}

#[kani::proof]
fn activation_seq_is_total_order_by_value() {
    let (x, y, z): (u64, u64, u64) = kani::any();
    let (a, b, c) = (
        ActivationSeq::new(x),
        ActivationSeq::new(y),
        ActivationSeq::new(z),
    );
    let (ab, _) = check_total_order(&a, &b, &c);
    assert_eq!(ab, spec_cmp(x, y));
    assert_eq!(a.get(), x);
    let (ra, rb, rc) = (Reverse(a), Reverse(b), Reverse(c));
    let (rab, _) = check_total_order(&ra, &rb, &rc);
    assert_eq!(rab, spec_cmp(y, x));
}

#[kani::proof]
fn recency_tag_is_total_order_by_value() {
    let (x, y, z): (u64, u64, u64) = kani::any();
    let (a, b, c) = (tag(x), tag(y), tag(z));
    let (ab, _) = check_total_order(&a, &b, &c);
    assert_eq!(ab, spec_cmp(x, y));
    // Absent matches sort below every fact.
    assert!(RecencyTag::ABSENT <= a);
    if x != 0 {
        assert!(RecencyTag::ABSENT < a);
    }
    kani::cover!(x == 0);
}

#[kani::proof]
fn complexity_reverse_is_total_order() {
    let (x, y, z): (u16, u16, u16) = kani::any();
    let (rab, _) = check_total_order(&Reverse(x), &Reverse(y), &Reverse(z));
    assert_eq!(rab, spec_cmp(y, x));
}

// ---------------------------------------------------------------------------
// Recency vectors (LEX / MEA payload)
// ---------------------------------------------------------------------------

/// Bounds: three vectors of length <= 4 (the inline capacity), byte domain.
#[kani::proof]
#[kani::unwind(6)]
fn recency_vector_is_total_order() {
    let ra = RawRecency::any(4, Width::Byte);
    let rb = RawRecency::any(4, Width::Byte);
    let rc = RawRecency::any(4, Width::Byte);
    let (a, b, c) = (ra.to_prod(), rb.to_prod(), rc.to_prod());
    let (ab, bc) = check_total_order(&a, &b, &c);
    assert_eq!(ab, spec_lex(ra.slice(), rb.slice()));
    kani::cover!(ab == Ordering::Less && bc == Ordering::Less);
    kani::cover!(ab == Ordering::Equal && bc == Ordering::Equal && ra.len == 4);
    kani::cover!(ra.len < rb.len && ab == Ordering::Less);
    kani::cover!(ra.len > rb.len && ab == Ordering::Less);
}

/// Bounds: two vectors of length <= 4, full `u64` domain.
#[kani::proof]
#[kani::unwind(6)]
fn recency_vector_order_is_lexicographic() {
    let ra = RawRecency::any(4, Width::Full);
    let rb = RawRecency::any(4, Width::Full);
    let (a, b) = (ra.to_prod(), rb.to_prod());
    assert_eq!(check_pair(&a, &b), spec_lex(ra.slice(), rb.slice()));
    let (rev_a, rev_b) = (Reverse(a), Reverse(b));
    assert_eq!(check_pair(&rev_a, &rev_b), spec_lex(rb.slice(), ra.slice()));
    kani::cover!(ra.len == 4 && rb.len == 4 && spec_lex(ra.slice(), rb.slice()) == Ordering::Equal);
    kani::cover!(ra.len < rb.len && spec_lex(ra.slice(), &rb.slice()[..ra.len]) == Ordering::Equal);
    kani::cover!(ra.len == 3 && rb.len == 3 && ra.tags[2] > rb.tags[2]);
}

// ---------------------------------------------------------------------------
// Per-strategy keys
// ---------------------------------------------------------------------------

fn check_strategy_variant(variant: u8, max_len: usize, width: Width) {
    let ra = RawStrategyOrd::any(Some(variant), max_len, width);
    let rb = RawStrategyOrd::any(Some(variant), max_len, width);
    let rc = RawStrategyOrd::any(Some(variant), max_len, width);
    let (a, b, c) = (ra.to_prod(), rb.to_prod(), rc.to_prod());
    let (ab, bc) = check_total_order(&a, &b, &c);
    assert_eq!(ab, spec_strategy(&ra, &rb));
    kani::cover!(ab == Ordering::Less && bc == Ordering::Less);
    kani::cover!(ab == Ordering::Equal);
    kani::cover!(ab == Ordering::Greater);
}

/// Bounds: full `u64` sequences.
#[kani::proof]
fn depth_key_is_total_order_newest_first() {
    check_strategy_variant(DEPTH, 0, Width::Full);
}

/// Bounds: full `u64` sequences.
#[kani::proof]
fn breadth_key_is_total_order_oldest_first() {
    check_strategy_variant(BREADTH, 0, Width::Full);
}

/// Bounds: recency vectors of length <= 4, byte domain.
#[kani::proof]
#[kani::unwind(6)]
fn lex_key_is_total_order_most_recent_first() {
    check_strategy_variant(LEX, 4, Width::Byte);
}

/// Bounds: recency vectors of length <= 4, byte domain.
#[kani::proof]
#[kani::unwind(6)]
fn mea_key_is_total_order_first_recency_first() {
    check_strategy_variant(MEA, 4, Width::Byte);
}

/// All four variants mixed. Bounds: recency vectors of length <= 2, byte domain.
#[kani::proof]
#[kani::unwind(6)]
fn strategy_ord_is_total_order_across_variants() {
    let ra = RawStrategyOrd::any(None, 2, Width::Byte);
    let rb = RawStrategyOrd::any(None, 2, Width::Byte);
    let rc = RawStrategyOrd::any(None, 2, Width::Byte);
    let (a, b, c) = (ra.to_prod(), rb.to_prod(), rc.to_prod());
    let (ab, _) = check_total_order(&a, &b, &c);
    assert_eq!(ab, spec_strategy(&ra, &rb));
    kani::cover!(ra.variant != rb.variant);
    kani::cover!(ra.variant == MEA && rb.variant == MEA && ab == Ordering::Equal);
}

// ---------------------------------------------------------------------------
// AgendaKey
// ---------------------------------------------------------------------------

/// Order laws and exact specification for three `AgendaKey`s. Byte domain for
/// sequences and recencies, full domain for salience and complexity.
fn check_agenda_key(variant: Option<u8>, max_len: usize) {
    let ra = RawAgendaKey::any(variant, max_len, Width::Byte);
    let rb = RawAgendaKey::any(variant, max_len, Width::Byte);
    let rc = RawAgendaKey::any(variant, max_len, Width::Byte);
    let (a, b, c) = (ra.to_prod(), rb.to_prod(), rc.to_prod());
    let (ab, _) = check_total_order(&a, &b, &c);
    assert_eq!(ab, spec_agenda_key(&ra, &rb));
    kani::cover!(ra.salience != rb.salience);
    kani::cover!(
        ra.salience == rb.salience && ra.complexity != rb.complexity && ab != Ordering::Equal
    );
    kani::cover!(ab == Ordering::Equal);
}

/// Bounds: every strategy variant mixed, recency vectors of length <= 2.
#[kani::proof]
#[kani::unwind(6)]
fn agenda_key_is_total_order() {
    check_agenda_key(None, 2);
}
