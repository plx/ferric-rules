//! Kani refinement harnesses linking the Verus model
//! verification/verus/sequence_match.rs to the production code of
//! `sequence` (see docs/verification.md).
//!
//! `model` below is a plain-Rust copy of every exec function of the Verus
//! model except the `example_admits` non-vacuity check, kept textually
//! aligned with it: specifications, ghost variables and proof blocks are
//! removed (so are the ghost `SplitSearch::work` counter and the `Ghost`
//! second component of `search`'s result, which carry the Verus work
//! bound), `slice_from_ref`/`slice_subrange` are the std
//! operations their Verus specifications describe, and the three model enums
//! that own heap data (`Val`, `Fact`, `TestType`) carry `#[repr(u8)]`, a
//! layout-only change (see "Harness technique"). The harnesses run model
//! functions and their production counterparts on the same inputs and
//! require equal results. They link to production the helpers the Verus
//! proof builds on (`segment_extra`, `test_extent`, and production
//! `evaluate_field_test` for `Equal` over the values `SplitView::get` is
//! documented to return, supplied by an oracle closure); they do NOT link
//! `search`/`place` or production `SplitView::get` themselves (see "Not
//! covered").
//!
//! # What is proved (all inputs within the stated bounds)
//!
//! * `segment_extra_refines_production`: for every list of up to five
//!   fields and every source of up to six values, model `segment_extra` ==
//!   production `segment_extra`. Unwind 7.
//! * `test_extent_equal_refines_production`,
//!   `test_extent_equal_slot_refines_production`: for `Equal` and
//!   `EqualSlot` tests over selectors of either kind and any `usize` index
//!   (saturation at `usize::MAX` included), model `test_extent` ==
//!   production `test_extent`; hence the Verus premise
//!   `needs[i] == sat(extent(tests[i]))` holds of production for these
//!   variants. Unbounded indices.
//! * `equal_semantics_refine_production`: for an `Equal(1)` test on a
//!   selector of either kind with index 0..=4 over four logical fields (a
//!   scalar integer, a scalar multifield, two captures of up to two values
//!   from {0, 1}), the model's `evaluate_field_test` over borrowed fields ==
//!   production `evaluate_field_test` over the values `SplitView::get` is
//!   documented to return (sequence.rs:542-555: a single field reads as its
//!   value, a capture as the multifield of its values, a selector of the
//!   other kind or past the fields as `None`). Unwind 3.
//! * `equal_never_matches_a_multifield`: for ANY integer key and a capture
//!   or scalar multifield of up to two ANY integers, the model and
//!   production `Equal` evaluation agree and are false, even when the
//!   multifield is exactly `[key]`. `equal_never_matches_a_one_value_multifield`
//!   is its one-value instance, kept separately because it still returns a
//!   counterexample within the memory budget for a production change that
//!   reads a one-value multifield as its atom (the symbolic-length harness
//!   runs out of memory on that change). Unwind 3.
//!
//! # Not covered, and why (Kani 0.68, `ulimit -v 7000000`, shared 4-CPU host)
//!
//! * The split search itself (`search`, `search_with_prefix`, `place`,
//!   `ready_tests_pass`, `admits`): no instance compares production with the
//!   model. Measured, with harnesses that compare event traces under a
//!   symbolic visitor: `(row ?x $?)` against one value, no tests: unwinding
//!   assertion at unwind 2 (two-element loops), solver out of memory after
//!   1m23s at unwind 3 (852k program steps, even with a CBMC recursion bound
//!   of 1 on `drop_glue::<Value>`); `(row $? $?)` against one value: out of
//!   memory after 7m57s; `(row $? ?x $?)` with an `Equal` test and up to
//!   three values: out of memory after 5-8 minutes in six variants (also
//!   with recursion bounds on `Value::clone`, `test_extent` and
//!   `evaluate_field_test_inner`); symbolic pattern shapes of two or three
//!   fields: out of memory after 4m39s-11m02s. A fully concrete
//!   `(row $?)`-over-one-value probe without a visitor runs out of memory
//!   after 3m19s at unwind 3 (verifies in 1m03s there with the drop-glue
//!   bound) and verifies in 1m18s at unwind 2. Causes seen
//!   in the CBMC logs: `place` recursion inlined inside two unrolled loops,
//!   and the production split view keeps `(FieldRef, OnceCell<Value>)` in a
//!   `SmallVec` union whose variants symbolic execution cannot resolve on
//!   read, so every `SplitView::get`, `pop` and `truncate` explores capture
//!   copying (`Value::clone`) and `Value` drop glue. The search model
//!   (`model::search`, `place`, `ready_tests_pass`, `Visitor`) is therefore
//!   linked to production only by textual fidelity of the transcription.
//! * `EqualSlot` evaluation against production: out of memory in
//!   propositional reduction after about 3 minutes (1.3-1.4M program steps)
//!   even with fixed selectors, dominated by production `Multifield`
//!   equality over `SmallVec`. `Any` tests are not compared (their
//!   recursive heap branches); their locality is proved only in Verus.
//! * `segment_values`, template sources and multi-segment plans are not
//!   compared here.
//!
//! # Harness technique
//!
//! * Inputs are built in lockstep from plain data (arrays, `vec![..]`
//!   literals, preallocated vectors). Symbolic execution loses the variant
//!   of an enum whose tag shares bytes with a payload (niche layout) once
//!   the value is moved through an `Option` or iterator plumbing, and then
//!   explores recursive `Any`/`Multifield` arms; growing a `Vec` at a
//!   symbolic length multiplies the formula.
//! * Values with recursive drop glue are leaked with `mem::forget`.
//! * All harnesses run with the plain command
//!   `cargo kani -p ferric-rules-core --harness <name>`.

use super::*;
use crate::value::{AtomKey, Multifield};

/// Plain-Rust transcription of the exec code of
/// verification/verus/sequence_match.rs (specifications, ghost code and
/// proofs removed). Items not reached by a harness (the search itself) are
/// kept so the transcription can be compared with the Verus file line by
/// line.
#[allow(dead_code, clippy::all, clippy::pedantic)]
mod model {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Field {
        Single,
        Multi,
    }

    pub enum Source {
        Ordered,
        TemplateSlot(usize),
        TemplateScalar(usize),
    }

    pub struct Segment {
        pub source: Source,
        pub fields: Vec<Field>,
    }

    pub enum Slot {
        Ordered(usize),
        Template(usize),
    }

    #[repr(u8)]
    pub enum Val {
        Atom(i64),
        Mf(Vec<Val>),
    }

    #[repr(u8)]
    pub enum Fact {
        Ordered(Vec<Val>),
        Template(Vec<Val>),
    }

    #[repr(u8)]
    pub enum TestType {
        Equal(i64),
        EqualSlot(Slot),
        Any(Vec<Vec<Test>>),
        WholeFact,
    }

    pub struct Test {
        pub slot: Slot,
        pub test_type: TestType,
    }

    pub struct Pattern {
        pub segments: Vec<Segment>,
        pub tests: Vec<Test>,
    }

    #[derive(Clone, Copy)]
    pub enum FieldRef<'a> {
        Single(&'a Val),
        Multi(&'a [Val]),
    }

    pub struct SplitView<'a> {
        pub fact: &'a Fact,
        pub fields: Vec<FieldRef<'a>>,
        pub lengths: Vec<usize>,
    }

    impl<'a> SplitView<'a> {
        pub fn get(&self, slot: &Slot) -> Option<FieldRef<'a>> {
            let index = match (slot, self.fact) {
                (Slot::Ordered(index), Fact::Ordered(_)) => *index,
                (Slot::Template(index), Fact::Template(_)) => *index,
                _ => return None,
            };
            if index < self.fields.len() {
                Some(self.fields[index])
            } else {
                None
            }
        }
    }

    pub fn value_eq(a: &Val, b: &Val) -> bool {
        match a {
            Val::Atom(x) => match b {
                Val::Atom(y) => *x == *y,
                _ => false,
            },
            Val::Mf(xs) => match b {
                Val::Mf(ys) => {
                    if xs.len() != ys.len() {
                        return false;
                    }
                    let mut i = 0;
                    while i < xs.len() {
                        if !value_eq(&xs[i], &ys[i]) {
                            return false;
                        }
                        i += 1;
                    }
                    true
                }
                _ => false,
            },
        }
    }

    pub fn values_eq(xs: &[Val], ys: &[Val]) -> bool {
        if xs.len() != ys.len() {
            return false;
        }
        let mut i = 0;
        while i < xs.len() {
            if !value_eq(&xs[i], &ys[i]) {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn field_eq(a: FieldRef, b: FieldRef) -> bool {
        match (a, b) {
            (FieldRef::Single(x), FieldRef::Single(y)) => value_eq(x, y),
            (FieldRef::Multi(xs), FieldRef::Multi(ys)) => values_eq(xs, ys),
            (FieldRef::Single(Val::Mf(xs)), FieldRef::Multi(ys)) => values_eq(xs.as_slice(), ys),
            (FieldRef::Multi(xs), FieldRef::Single(Val::Mf(ys))) => values_eq(xs, ys.as_slice()),
            _ => false,
        }
    }

    pub fn evaluate_field_test(test: &Test, split: &SplitView) -> bool {
        let value = match split.get(&test.slot) {
            Some(value) => value,
            None => return false,
        };
        match &test.test_type {
            TestType::WholeFact => false,
            TestType::Any(branches) => {
                let mut i = 0;
                while i < branches.len() {
                    let branch = &branches[i];
                    let mut all = true;
                    let mut j = 0;
                    while j < branch.len() {
                        if !evaluate_field_test(&branch[j], split) {
                            all = false;
                            break;
                        }
                        j += 1;
                    }
                    if all {
                        return true;
                    }
                    i += 1;
                }
                false
            }
            TestType::Equal(key) => match value {
                FieldRef::Single(Val::Atom(x)) => *x == *key,
                _ => false,
            },
            TestType::EqualSlot(other) => match split.get(other) {
                Some(other_value) => field_eq(value, other_value),
                None => false,
            },
        }
    }

    pub fn index(slot: &Slot) -> usize {
        match slot {
            Slot::Ordered(index) => *index,
            Slot::Template(index) => *index,
        }
    }

    pub fn test_extent(test: &Test) -> usize {
        let other = match &test.test_type {
            TestType::EqualSlot(slot) => index(slot).saturating_add(1),
            TestType::Any(branches) => {
                let mut best: usize = 0;
                let mut i = 0;
                while i < branches.len() {
                    let branch = &branches[i];
                    let mut j = 0;
                    while j < branch.len() {
                        let extent = test_extent(&branch[j]);
                        if extent > best {
                            best = extent;
                        }
                        j += 1;
                    }
                    i += 1;
                }
                best
            }
            _ => 0,
        };
        let own = index(&test.slot).saturating_add(1);
        if own >= other {
            own
        } else {
            other
        }
    }

    fn slice_from_ref<'a>(value: &'a Val) -> &'a [Val] {
        std::slice::from_ref(value)
    }

    fn slice_subrange<'a, T>(slice: &'a [T], i: usize, j: usize) -> &'a [T] {
        &slice[i..j]
    }

    pub fn segment_values<'a>(source: &Source, fact: &'a Fact) -> Option<&'a [Val]> {
        match (source, fact) {
            (Source::Ordered, Fact::Ordered(fields)) => Some(fields.as_slice()),
            (Source::TemplateSlot(index), Fact::Template(slots)) => {
                if *index >= slots.len() {
                    return None;
                }
                let value = &slots[*index];
                Some(match value {
                    Val::Mf(values) => values.as_slice(),
                    _ => slice_from_ref(value),
                })
            }
            (Source::TemplateScalar(index), Fact::Template(slots)) => {
                if *index >= slots.len() {
                    return None;
                }
                Some(slice_from_ref(&slots[*index]))
            }
            _ => None,
        }
    }

    pub fn segment_extra(fields: &Vec<Field>, values: &[Val]) -> Option<usize> {
        let mut singles: usize = 0;
        let mut k: usize = 0;
        while k < fields.len() {
            if fields[k] == Field::Single {
                singles += 1;
            }
            k += 1;
        }
        let extra = match values.len().checked_sub(singles) {
            Some(extra) => extra,
            None => return None,
        };
        if extra == 0 || singles < fields.len() {
            Some(extra)
        } else {
            None
        }
    }

    fn only_singles_after(fields: &Vec<Field>, start: usize) -> bool {
        let mut k = start;
        while k < fields.len() {
            if fields[k] != Field::Single {
                return false;
            }
            k += 1;
        }
        true
    }

    fn copy_lengths(lengths: &Vec<usize>) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut k = 0;
        while k < lengths.len() {
            out.push(lengths[k]);
            k += 1;
        }
        out
    }

    pub struct Visitor {
        pub matches: Vec<Vec<usize>>,
        pub steps: Vec<usize>,
        pub stop_on_match: bool,
        pub step_budget: Option<usize>,
    }

    impl Visitor {
        pub fn on_match(&mut self, lengths: &Vec<usize>) -> bool {
            let copy = copy_lengths(lengths);
            self.matches.push(copy);
            self.stop_on_match
        }

        pub fn on_step(&mut self) -> bool {
            if let Some(budget) = self.step_budget {
                if self.steps.len() >= budget {
                    return true;
                }
            }
            let marker = self.matches.len();
            self.steps.push(marker);
            false
        }
    }

    pub struct SplitSearch<'a> {
        pub plan: &'a Pattern,
        pub sources: Vec<(&'a [Val], usize)>,
        pub needs: Vec<usize>,
        pub split: SplitView<'a>,
    }

    impl<'a> SplitSearch<'a> {
        fn ready_tests_pass(&self, checked: usize, complete: bool) -> bool {
            let placed = self.split.fields.len();
            let mut i = 0;
            while i < self.plan.tests.len() {
                let need = self.needs[i];
                if !(need <= checked
                    || (need > placed && !complete)
                    || evaluate_field_test(&self.plan.tests[i], &self.split))
                {
                    return false;
                }
                i += 1;
            }
            true
        }

        fn place(
            &mut self,
            segment0: usize,
            field0: usize,
            offset0: usize,
            extra0: usize,
            checked: usize,
            visitor: &mut Visitor,
        ) -> bool {
            let mark = self.split.fields.len();
            let plan = self.plan;
            let mut segment = segment0;
            let mut field = field0;
            let mut offset = offset0;
            let mut extra = extra0;
            loop {
                let fields = &plan.segments[segment].fields;
                let values = self.sources[segment].0;
                if field == fields.len() {
                    segment += 1;
                    if segment < plan.segments.len() {
                        field = 0;
                        offset = 0;
                        extra = self.sources[segment].1;
                        continue;
                    }
                    let ok = self.ready_tests_pass(checked, true);
                    if ok {
                        let stop = visitor.on_match(&self.split.lengths);
                        if stop {
                            return true;
                        }
                    }
                    break;
                }
                if fields[field] == Field::Single {
                    self.split.fields.push(FieldRef::Single(&values[offset]));
                    field += 1;
                    offset += 1;
                    continue;
                }
                let ok = self.ready_tests_pass(checked, false);
                if !ok {
                    break;
                }
                let checked = self.split.fields.len();
                // The segment's last capture takes whatever the others leave.
                let last = only_singles_after(fields, field + 1);
                let shortest = if last { extra } else { 0 };
                let mut length = extra;
                loop {
                    if !last {
                        if visitor.on_step() {
                            return true;
                        }
                    }
                    self.split.fields.push(FieldRef::Multi(slice_subrange(
                        values,
                        offset,
                        offset + length,
                    )));
                    self.split.lengths.push(length);
                    let broke = self.place(
                        segment,
                        field + 1,
                        offset + length,
                        extra - length,
                        checked,
                        visitor,
                    );
                    if broke {
                        return true;
                    }
                    self.split.fields.pop();
                    self.split.lengths.pop();
                    if length == shortest {
                        break;
                    }
                    length -= 1;
                }
                break;
            }
            self.split.fields.truncate(mark);
            false
        }
    }

    pub fn search(plan: &Pattern, fact: &Fact, visitor: &mut Visitor) -> bool {
        let mut sources: Vec<(&[Val], usize)> = Vec::new();
        let mut k: usize = 0;
        while k < plan.segments.len() {
            let segment = &plan.segments[k];
            let values = match segment_values(&segment.source, fact) {
                Some(values) => values,
                None => {
                    return false;
                }
            };
            let extra = match segment_extra(&segment.fields, values) {
                Some(extra) => extra,
                None => {
                    return false;
                }
            };
            sources.push((values, extra));
            k += 1;
        }
        if sources.len() == 0 {
            return false;
        }
        let extra = sources[0].1;
        let mut needs: Vec<usize> = Vec::new();
        let mut i: usize = 0;
        while i < plan.tests.len() {
            needs.push(test_extent(&plan.tests[i]));
            i += 1;
        }
        let mut search = SplitSearch {
            plan,
            sources,
            needs,
            split: SplitView {
                fact,
                fields: Vec::new(),
                lengths: Vec::new(),
            },
        };
        search.place(0, 0, 0, extra, 0, visitor)
    }

    pub fn admits(plan: &Pattern, fact: &Fact) -> bool {
        let mut visitor = Visitor {
            matches: Vec::new(),
            steps: Vec::new(),
            stop_on_match: true,
            step_budget: None,
        };
        search(plan, fact, &mut visitor)
    }
}

// ---------------------------------------------------------------------------
// Harness support. Model inputs and their production images are built in
// lockstep from plain data (see "Harness technique").
// ---------------------------------------------------------------------------

/// A selector, of either kind, as `(model, production)`.
fn slot_pair(ordered: bool, index: usize) -> (model::Slot, SlotIndex) {
    if ordered {
        (model::Slot::Ordered(index), SlotIndex::Ordered(index))
    } else {
        (model::Slot::Template(index), SlotIndex::Template(index))
    }
}

/// `segment_extra`: for every field list of up to five fields and every
/// source of up to six values, the model returns exactly what production
/// returns (the shared capture budget, or `None`).
#[kani::proof]
#[kani::unwind(7)]
fn segment_extra_refines_production() {
    let singles: [bool; 5] = kani::any();
    let len: usize = kani::any_where(|len| *len <= 5);
    let count: usize = kani::any_where(|count| *count <= 6);
    let all_fields = singles.map(|single| {
        if single {
            model::Field::Single
        } else {
            model::Field::Multi
        }
    });
    let all_production_fields = singles.map(|single| {
        if single {
            SequenceField::Single
        } else {
            SequenceField::Multi
        }
    });
    let fields = all_fields[..len].to_vec();
    let values = [0; 6].map(model::Val::Atom);
    let production_values = [0; 6].map(Value::Integer);

    let expected = model::segment_extra(&fields, &values[..count]);
    let actual = segment_extra(&all_production_fields[..len], &production_values[..count]);
    assert_eq!(actual, expected);

    kani::cover!(
        expected == Some(0) && len > 0,
        "fields fill the source exactly"
    );
    kani::cover!(
        matches!(expected, Some(extra) if extra > 0),
        "captures share leftover values"
    );
    kani::cover!(
        expected.is_none() && count < len,
        "too few values for the singles"
    );
    kani::cover!(
        expected.is_none() && count > len && singles[..len].iter().all(|single| *single),
        "no capture can absorb leftover values"
    );
    std::mem::forget(values);
    std::mem::forget(production_values);
}

/// `test_extent` of an `Equal` test: the model and production agree for a
/// selector of either kind and any index, including `usize::MAX`
/// (saturation).
#[kani::proof]
#[kani::unwind(2)]
fn test_extent_equal_refines_production() {
    let index: usize = kani::any();
    let (slot, production_slot) = slot_pair(kani::any(), index);
    let test = model::Test {
        slot,
        test_type: model::TestType::Equal(1),
    };
    let production_test = ConstantTest {
        slot: production_slot,
        test_type: ConstantTestType::Equal(AtomKey::Integer(1)),
    };
    let expected = model::test_extent(&test);
    let actual = test_extent(&production_test);
    assert_eq!(actual, expected);

    kani::cover!(expected == usize::MAX, "saturated extent");
    kani::cover!(expected < usize::MAX, "exact extent");
    std::mem::forget(test);
    std::mem::forget(production_test);
}

/// `test_extent` of an `EqualSlot` test: the model and production agree for
/// any two selectors (either kind, any index), so the extent covers the
/// later of the two fields read.
#[kani::proof]
#[kani::unwind(2)]
fn test_extent_equal_slot_refines_production() {
    let left: usize = kani::any();
    let right: usize = kani::any();
    let (slot, production_slot) = slot_pair(kani::any(), left);
    let (other, production_other) = slot_pair(kani::any(), right);
    let test = model::Test {
        slot,
        test_type: model::TestType::EqualSlot(other),
    };
    let production_test = ConstantTest {
        slot: production_slot,
        test_type: ConstantTestType::EqualSlot(production_other),
    };
    let expected = model::test_extent(&test);
    let actual = test_extent(&production_test);
    assert_eq!(actual, expected);

    kani::cover!(
        left < right && right.checked_add(1) == Some(expected),
        "the other selector decides"
    );
    kani::cover!(
        right < left && left.checked_add(1) == Some(expected),
        "the own selector decides"
    );
    std::mem::forget(test);
    std::mem::forget(production_test);
}

/// Up to two integers from {0, 1}, built without growing a `Vec` at a
/// symbolic length (which dominates the CBMC formula otherwise).
#[derive(Clone, Copy)]
struct Run {
    atoms: [i64; 2],
    len: usize,
}

impl Run {
    fn any() -> Self {
        Run {
            atoms: [
                i64::from(kani::any::<bool>()),
                i64::from(kani::any::<bool>()),
            ],
            len: kani::any_where(|len| *len <= 2),
        }
    }

    fn model(self) -> Vec<model::Val> {
        let mut values = Vec::with_capacity(2);
        if self.len > 0 {
            values.push(model::Val::Atom(self.atoms[0]));
        }
        if self.len > 1 {
            values.push(model::Val::Atom(self.atoms[1]));
        }
        values
    }

    /// The multifield value a capture of these values reads as.
    fn production(self) -> Value {
        let mut values = Multifield::new();
        if self.len > 0 {
            values.push(Value::Integer(self.atoms[0]));
        }
        if self.len > 1 {
            values.push(Value::Integer(self.atoms[1]));
        }
        Value::Multifield(Box::new(values))
    }
}

/// Compare one field test on the four logical fields of a template split —
/// 0 a scalar integer, 1 a scalar multifield (a `TemplateScalar` source over
/// a slot holding a multifield), 2 and 3 captures of up to two values —
/// between the model (`evaluate_field_test` over borrowed fields) and
/// production `evaluate_field_test` over the values `SplitView::get` is
/// documented to return (sequence.rs:542-555: a single field reads as its
/// value, a capture as the multifield of its values, a selector of the
/// other fact kind or past the placed fields as `None`).
fn check_field_test(
    atom: i64,
    multi: Run,
    first: Run,
    second: Run,
    test: &model::Test,
    production_test: &ConstantTest,
) -> bool {
    let model_atom = model::Val::Atom(atom);
    let model_multi = model::Val::Mf(multi.model());
    let model_first = first.model();
    let model_second = second.model();
    let fact = model::Fact::Template(Vec::new());
    let split = model::SplitView {
        fact: &fact,
        fields: vec![
            model::FieldRef::Single(&model_atom),
            model::FieldRef::Single(&model_multi),
            model::FieldRef::Multi(&model_first),
            model::FieldRef::Multi(&model_second),
        ],
        lengths: Vec::new(),
    };
    let values = [
        Value::Integer(atom),
        multi.production(),
        first.production(),
        second.production(),
    ];
    let get = |slot: SlotIndex| match slot {
        SlotIndex::Template(index) => values.get(index),
        SlotIndex::Ordered(_) => None,
    };
    let expected = model::evaluate_field_test(test, &split);
    let actual = evaluate_field_test(production_test, get);
    assert_eq!(actual, expected);
    std::mem::forget(split);
    std::mem::forget(fact);
    std::mem::forget(model_atom);
    std::mem::forget(model_multi);
    std::mem::forget(model_first);
    std::mem::forget(model_second);
    std::mem::forget(values);
    expected
}

/// `Equal(1)` on a selector of either kind naming any of the four fields or
/// none: only a scalar integer can equal an integer key; multifields and
/// captures never do. The key is fixed (the field values range over {0, 1},
/// so both outcomes occur): a symbolic key ran out of memory in
/// propositional reduction.
#[kani::proof]
#[kani::unwind(3)]
fn equal_semantics_refine_production() {
    let atom = i64::from(kani::any::<bool>());
    let (multi, first, second) = (Run::any(), Run::any(), Run::any());
    let index: usize = kani::any_where(|index| *index <= 4);
    let ordered: bool = kani::any();
    let key = 1;
    let (slot, production_slot) = slot_pair(ordered, index);
    let test = model::Test {
        slot,
        test_type: model::TestType::Equal(key),
    };
    let production_test = ConstantTest {
        slot: production_slot,
        test_type: ConstantTestType::Equal(AtomKey::Integer(key)),
    };
    let equal = check_field_test(atom, multi, first, second, &test, &production_test);

    kani::cover!(equal && index == 0, "a scalar integer equals the key");
    kani::cover!(
        !equal && !ordered && index == 0,
        "a scalar integer differs from the key"
    );
    kani::cover!(
        !equal && !ordered && index == 2 && first.len > 0 && first.atoms[0] == key,
        "a capture holding the key still differs from it"
    );
    std::mem::forget(test);
    std::mem::forget(production_test);
}

/// `Equal(key)` on one multifield field — a capture when `scalar` is false,
/// a scalar slot holding a multifield when it is true — of the values in
/// `run`: the model (`evaluate_field_test` over the borrowed field) and
/// production `evaluate_field_test` (over the multifield value a capture
/// reads as, sequence.rs:551-554 and :602) agree, and neither matches.
/// Returns whether the multifield holds exactly `[key]`.
fn check_equal_on_multifield(key: i64, run: Run, scalar: bool) -> bool {
    let model_values = run.model();
    let model_multi = model::Val::Mf(run.model());
    let fact = model::Fact::Template(Vec::new());
    let field = if scalar {
        model::FieldRef::Single(&model_multi)
    } else {
        model::FieldRef::Multi(&model_values)
    };
    let split = model::SplitView {
        fact: &fact,
        fields: vec![field],
        lengths: Vec::new(),
    };
    let test = model::Test {
        slot: model::Slot::Template(0),
        test_type: model::TestType::Equal(key),
    };
    let production_test = ConstantTest {
        slot: SlotIndex::Template(0),
        test_type: ConstantTestType::Equal(AtomKey::Integer(key)),
    };
    let value = run.production();
    let get = |slot: SlotIndex| match slot {
        SlotIndex::Template(0) => Some(&value),
        _ => None,
    };
    let expected = model::evaluate_field_test(&test, &split);
    let actual = evaluate_field_test(&production_test, get);
    assert_eq!(actual, expected);
    assert!(!actual, "Equal never matches a multifield");
    std::mem::forget(split);
    std::mem::forget(fact);
    std::mem::forget(model_values);
    std::mem::forget(model_multi);
    std::mem::forget(value);
    std::mem::forget(test);
    std::mem::forget(production_test);
    run.len == 1 && run.atoms[0] == key
}

/// `Equal(key)` on a capture or a scalar multifield of up to two values, for
/// ANY key and ANY integer values: the model and production agree and
/// neither matches (the Verus `eval` compares the key only with a single
/// atom). Generalizes `equal_semantics_refine_production`, whose key and
/// values are fixed to {0, 1}.
#[kani::proof]
#[kani::unwind(3)]
fn equal_never_matches_a_multifield() {
    let key: i64 = kani::any();
    let run = Run {
        atoms: kani::any(),
        len: kani::any_where(|len| *len <= 2),
    };
    let scalar: bool = kani::any();
    let holds_key = check_equal_on_multifield(key, run, scalar);

    kani::cover!(holds_key && !scalar, "a capture holding exactly the key");
    kani::cover!(
        holds_key && scalar,
        "a scalar multifield holding exactly the key"
    );
    kani::cover!(run.len == 0, "an empty multifield");
    kani::cover!(run.len == 2, "a two-value multifield");
}

/// The one-value case of `equal_never_matches_a_multifield` on its own. A
/// production change that read a one-value multifield as its atom (e.g. in
/// `AtomKey::try_from`) runs the symbolic-length harness out of CBMC memory;
/// this fixed-length instance stays small enough to return a counterexample
/// for it (measured: about 6 s).
#[kani::proof]
#[kani::unwind(3)]
fn equal_never_matches_a_one_value_multifield() {
    let key: i64 = kani::any();
    let run = Run {
        atoms: kani::any(),
        len: 1,
    };
    let scalar: bool = kani::any();
    let holds_key = check_equal_on_multifield(key, run, scalar);

    kani::cover!(holds_key && !scalar, "a capture holding exactly the key");
    kani::cover!(
        holds_key && scalar,
        "a scalar multifield holding exactly the key"
    );
    kani::cover!(!holds_key, "a one-value multifield without the key");
}
