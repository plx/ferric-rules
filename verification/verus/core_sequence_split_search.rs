//! Verus model of the sequence split search in
//! crates/ferric-rules-core/src/sequence.rs (`SequencePattern::search_with_prefix`,
//! `SplitSearch::place`, `SplitSearch::ready_tests_pass`, `segment_extra`).
//!
//! What is proved (unbounded: any number of segments, fields, captures,
//! source lengths, tests):
//!
//! * The executable model `SplitSearch::place` (same loop, branches, field
//!   stack and capture loop as production) emits exactly the events of the
//!   structural spec `spec_place`, and restores `fields` and `lengths`
//!   (push/pop/truncate discipline). `ready_tests_pass`, `last_capture`
//!   and `segment_extra` meet their specs; `search_with_prefix` emits
//!   exactly `search_log`.
//! * `theorem_search` (main result): the `Match` events of the search are
//!   exactly the accepted splits, each reported once, in strictly
//!   decreasing lexicographic order of capture lengths (`sound`,
//!   `complete`, `sorted`). A split is a vector of capture lengths; it is
//!   *accepted* (`accepted`) when the plan has a segment, it tiles every
//!   segment's source (single fields take one value, captures take their
//!   length, consuming exactly the source; a missing source admits no
//!   split), every constant test holds on its projection, and the prefix
//!   filter holds at every point where `place` consults it (`chain_from`:
//!   before each capture with the fields placed so far and the field count
//!   checked at the previous capture, and on the complete split). This is
//!   the declarative brute-force characterization: early test evaluation
//!   prunes nothing it should not and the depth-first enumeration with
//!   longest-first lengths misses nothing, duplicates nothing and yields
//!   the order CLIPS uses.
//! * `example_two_captures`: a concrete instance (`(row $? $?)` over one
//!   value) where both splits are accepted and reported in order, showing
//!   the theorem is not vacuous.
//!
//! Premises of the theorem (proved of production code with Kani where
//! noted, in crates/ferric-rules-core/src/sequence/kani_proofs.rs):
//!
//! * `local(needs, holds)`: a constant test decides the same on two
//!   projections that agree on their first `needs[t]` fields. Kani proves
//!   `test_extent` is exactly one past the largest selector every non-`Any`
//!   test names (`test_extent_is_exact`) and that evaluating the atom tests
//!   reads no field at or past it (`atom_tests_read_below_extent`);
//!   `SplitView::get` reads only `fields[index]`. For slot-pair, offset and
//!   `Any` tests the read bound is by inspection of `evaluate_field_test`.
//! * `needs[t] >= 1` (`test_extent` saturating-adds one, sequence.rs:401).
//!
//! Model abstractions (each fixed by the production code it stands for):
//!
//! * Values are not modelled. A logical field is `Placed` (segment, offset,
//!   length, single/capture), which is what `FieldRef` borrows; the
//!   `OnceCell` copy cache of `SplitView` is not modelled (it only memoizes
//!   `get`).
//! * A segment's source is its length (`source_lens[s]`, `None` when
//!   `segment_values` finds none); Kani proves `segment_values` returns
//!   the documented slices (`segment_values_is_exact`).
//! * `needs` is the plan's tests mapped through `test_extent`; test
//!   evaluation is the closure `eval` computing the spec predicate `holds`
//!   on the placed fields, and the prefix filter is the closure `prefix`
//!   computing `pf` (a pure function: production's `FnMut` filters are not
//!   modelled with state).
//! * The visitor is a ghost event log; it never breaks. `ControlFlow::Break`
//!   (e.g. `admits` stopping at the first match) is not modelled: a
//!   breaking visitor sees a prefix of this log.
//! * `place`'s `mut` parameters are copied into locals (`segment_in` etc.)
//!   so loop invariants can name the entry values; the reverse range loop
//!   `for length in (shortest..=extra).rev()` is a counting-down loop.
//!
//! Trusted base: vstd (including its `Vec` specifications). No `assume`,
//! `admit`, `external_body` or other trusted glue is used.
//!
//! Known gaps: `validate`, `project`, `physical_selector` and the
//! `SplitView` accessors are not modelled here (see the Kani harnesses);
//! the link between this model and production is by inspection (Kani
//! cannot execute `search` itself: see the Kani module documentation).
use vstd::prelude::*;

verus! {

// ===========================================================================
// Model types
// ===========================================================================

/// Models `SequenceField` (crates/ferric-rules-core/src/sequence.rs:19).
pub enum Field {
    Single,
    Multi,
}

pub open spec fn is_multi(f: Field) -> bool {
    match f {
        Field::Single => false,
        Field::Multi => true,
    }
}

/// Models one logical field of a split, `FieldRef`
/// (crates/ferric-rules-core/src/sequence.rs:520), by position: the values
/// `offset..offset + length` of segment `segment`'s source
/// (`FieldRef::Single(&values[offset])` has `length == 1`, `multi == false`).
pub struct Placed {
    pub segment: usize,
    pub offset: usize,
    pub length: usize,
    pub multi: bool,
}

/// Models `SplitEvent` (crates/ferric-rules-core/src/sequence.rs:372);
/// `Match` carries the split's capture lengths (`SplitView::lengths`).
pub ghost enum Ev {
    Step,
    Match(Seq<usize>),
}

/// Models `SplitSearch` (crates/ferric-rules-core/src/sequence.rs:415).
/// `segments[s]` is `plan.segments[s].fields`, `needs` is `needs`,
/// `sources[s]` is `(values.len(), extra)` of `sources[s]`, `fields` is the
/// `FieldRef` part of `split.fields` and `lengths` is `split.lengths`.
pub struct SplitSearch {
    pub segments: Vec<Vec<Field>>,
    pub needs: Vec<usize>,
    pub sources: Vec<(usize, usize)>,
    pub fields: Vec<Placed>,
    pub lengths: Vec<usize>,
}

impl SplitSearch {
    pub open spec fn segs(&self) -> Seq<Seq<Field>> {
        self.segments@.map_values(|v: Vec<Field>| v@)
    }
}

// ===========================================================================
// Oracles: constant tests and the prefix filter
// ===========================================================================

/// The test closure (standing for `evaluate_field_test(test, |slot|
/// split.get(slot))` on test `t`) computes the spec predicate `holds`.
pub open spec fn eval_computes<E: Fn(usize, &Vec<Placed>) -> bool>(
    eval: E,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
) -> bool {
    &&& forall|t: usize, s: &Vec<Placed>| call_requires(eval, (t, s))
    &&& forall|t: usize, s: &Vec<Placed>, r: bool|
        call_ensures(eval, (t, s), r) ==> r == holds(t as nat, s@)
}

/// The prefix closure (standing for `prefix(&split, checked, complete)`)
/// computes the spec predicate `pf`.
pub open spec fn pfx_computes<P: Fn(&Vec<Placed>, usize, bool) -> bool>(
    pfx: P,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
) -> bool {
    &&& forall|s: &Vec<Placed>, c: usize, d: bool| call_requires(pfx, (s, c, d))
    &&& forall|s: &Vec<Placed>, c: usize, d: bool, r: bool|
        call_ensures(pfx, (s, c, d), r) ==> r == pf(s@, c as nat, d)
}

/// Spec of `ready_tests_pass` (crates/ferric-rules-core/src/sequence.rs:432).
pub open spec fn ready(
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
    fields: Seq<Placed>,
    checked: nat,
    complete: bool,
) -> bool {
    &&& forall|t: int|
        0 <= t < needs.len() ==> (needs[t] <= checked || (needs[t] > fields.len() && !complete)
            || holds(t as nat, fields))
    &&& pf(fields, checked, complete)
}

// ===========================================================================
// Structural spec of `place`
// ===========================================================================

/// Number of fields still to visit from `(seg, field)`, plus one per
/// segment end: the termination measure of `place`.
pub open spec fn rest(segs: Seq<Seq<Field>>, seg: nat) -> nat
    decreases segs.len() - seg,
{
    if seg >= segs.len() {
        0
    } else {
        segs[seg as int].len() + 1 + rest(segs, seg + 1)
    }
}

pub open spec fn rest_from(segs: Seq<Seq<Field>>, seg: nat, field: nat) -> nat {
    if seg >= segs.len() {
        0
    } else if field >= segs[seg as int].len() {
        1 + rest(segs, seg + 1)
    } else {
        ((segs[seg as int].len() - field) + 1 + rest(segs, seg + 1)) as nat
    }
}

/// Whether every field of `fs` from `from` on is a single field.
pub open spec fn all_single_from(fs: Seq<Field>, from: int) -> bool {
    forall|j: int| from <= j < fs.len() ==> !is_multi(#[trigger] fs[j])
}

/// Events emitted by `place(seg, field, offset, extra, checked)` from a
/// split holding `fields` and `lengths`; follows the loop of
/// crates/ferric-rules-core/src/sequence.rs:450 `place`, one recursive step
/// per loop iteration.
pub open spec fn spec_place(
    segs: Seq<Seq<Field>>,
    srcs: Seq<(usize, usize)>,
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
    seg: nat,
    field: nat,
    offset: nat,
    extra: nat,
    checked: nat,
    fields: Seq<Placed>,
    lengths: Seq<usize>,
) -> Seq<Ev>
    decreases rest_from(segs, seg, field), 1nat, 0int,
{
    if seg >= segs.len() || field > segs[seg as int].len() {
        seq![]
    } else if field == segs[seg as int].len() {
        if seg + 1 < segs.len() {
            spec_place(
                segs,
                srcs,
                needs,
                holds,
                pf,
                seg + 1,
                0,
                0,
                srcs[seg + 1 as int].1 as nat,
                checked,
                fields,
                lengths,
            )
        } else if ready(needs, holds, pf, fields, checked, true) {
            seq![Ev::Match(lengths)]
        } else {
            seq![]
        }
    } else if !is_multi(segs[seg as int][field as int]) {
        spec_place(
            segs,
            srcs,
            needs,
            holds,
            pf,
            seg,
            field + 1,
            offset + 1,
            extra,
            checked,
            fields.push(
                Placed { segment: seg as usize, offset: offset as usize, length: 1, multi: false },
            ),
            lengths,
        )
    } else if !ready(needs, holds, pf, fields, checked, false) {
        seq![]
    } else {
        let last = all_single_from(segs[seg as int], field + 1 as int);
        let shortest: nat = if last {
            extra
        } else {
            0
        };
        spec_tries(
            segs,
            srcs,
            needs,
            holds,
            pf,
            seg,
            field,
            offset,
            extra,
            fields.len(),
            fields,
            lengths,
            extra as int,
            shortest,
            last,
        )
    }
}

/// Events of the capture loop `for length in (shortest..=extra).rev()`
/// (crates/ferric-rules-core/src/sequence.rs:491), from `length` down.
pub open spec fn spec_tries(
    segs: Seq<Seq<Field>>,
    srcs: Seq<(usize, usize)>,
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
    seg: nat,
    field: nat,
    offset: nat,
    extra: nat,
    checked: nat,
    fields: Seq<Placed>,
    lengths: Seq<usize>,
    length: int,
    shortest: nat,
    last: bool,
) -> Seq<Ev>
    decreases rest_from(segs, seg, field), 0nat, length + 1,
{
    if length < shortest || length < 0 || length > extra || seg >= segs.len() || field
        >= segs[seg as int].len() {
        seq![]
    } else {
        let step: Seq<Ev> = if last {
            seq![]
        } else {
            seq![Ev::Step]
        };
        step + spec_place(
            segs,
            srcs,
            needs,
            holds,
            pf,
            seg,
            field + 1,
            offset + length as nat,
            (extra - length) as nat,
            checked,
            fields.push(
                Placed {
                    segment: seg as usize,
                    offset: offset as usize,
                    length: length as usize,
                    multi: true,
                },
            ),
            lengths.push(length as usize),
        ) + spec_tries(
            segs,
            srcs,
            needs,
            holds,
            pf,
            seg,
            field,
            offset,
            extra,
            checked,
            fields,
            lengths,
            length - 1,
            shortest,
            last,
        )
    }
}

// ===========================================================================
// Executable model
// ===========================================================================

/// Number of single fields of `fs` from `from` on.
pub open spec fn singles_from(fs: Seq<Field>, from: int) -> nat
    decreases fs.len() - from,
{
    if from < 0 || from >= fs.len() {
        0
    } else {
        (if is_multi(fs[from]) {
            0nat
        } else {
            1nat
        }) + singles_from(fs, from + 1)
    }
}

impl SplitSearch {
    /// What `search_with_prefix` establishes before calling `place`: one
    /// source per segment, each with the `extra` that `segment_extra`
    /// (crates/ferric-rules-core/src/sequence.rs:406) reported for it.
    pub open spec fn wf(&self) -> bool {
        &&& self.segments@.len() == self.sources@.len()
        &&& forall|s: int|
            0 <= s < self.sources@.len() ==> {
                &&& #[trigger] self.sources@[s].1 + singles_from(self.segs()[s], 0)
                    == self.sources@[s].0
                &&& (self.sources@[s].1 == 0 || !all_single_from(self.segs()[s], 0))
            }
    }

    /// Models `SplitSearch::ready_tests_pass`
    /// (crates/ferric-rules-core/src/sequence.rs:432): the `all` over the
    /// zipped tests and needs is the loop, and the call of `prefix` follows
    /// a passing loop (`&&` short-circuit).
    pub fn ready_tests_pass<E: Fn(usize, &Vec<Placed>) -> bool, P: Fn(
        &Vec<Placed>,
        usize,
        bool,
    ) -> bool>(
        &self,
        checked: usize,
        complete: bool,
        eval: &E,
        prefix: &P,
        Ghost(holds): Ghost<spec_fn(nat, Seq<Placed>) -> bool>,
        Ghost(pf): Ghost<spec_fn(Seq<Placed>, nat, bool) -> bool>,
    ) -> (r: bool)
        requires
            eval_computes(*eval, holds),
            pfx_computes(*prefix, pf),
        ensures
            r == ready(self.needs@, holds, pf, self.fields@, checked as nat, complete),
    {
        let placed = self.fields.len();
        let mut i: usize = 0;
        let mut constants_pass = true;
        while i < self.needs.len()
            invariant_except_break
                constants_pass,
            invariant
                i <= self.needs.len(),
                placed == self.fields@.len(),
                eval_computes(*eval, holds),
                forall|t: int|
                    0 <= t < i ==> (self.needs@[t] <= checked || (self.needs@[t] > placed
                        && !complete) || holds(t as nat, self.fields@)),
            ensures
                constants_pass ==> i == self.needs.len(),
                !constants_pass ==> i < self.needs.len() && !(self.needs@[i as int] <= checked
                    || (self.needs@[i as int] > placed && !complete) || holds(
                    i as nat,
                    self.fields@,
                )),
            decreases self.needs.len() - i,
        {
            let need = self.needs[i];
            if !(need <= checked || (need > placed && !complete) || eval(i, &self.fields)) {
                constants_pass = false;
                break;
            }
            i += 1;
        }
        constants_pass && prefix(&self.fields, checked, complete)
    }

    /// Models `fields[field + 1..].iter().all(|field| *field == Single)`
    /// (crates/ferric-rules-core/src/sequence.rs:487).
    pub fn last_capture(fields: &Vec<Field>, from: usize) -> (r: bool)
        ensures
            r == all_single_from(fields@, from as int),
    {
        let mut j = from;
        while j < fields.len()
            invariant
                forall|k: int| from <= k < j ==> !is_multi(#[trigger] fields@[k]),
                from <= j,
            decreases fields.len() - j,
        {
            match fields[j] {
                Field::Multi => {
                    return false;
                },
                Field::Single => {},
            }
            j += 1;
        }
        true
    }

    /// Models `SplitSearch::place` (crates/ferric-rules-core/src/sequence.rs:450).
    /// The visitor is abstracted to appending events to the ghost `log`
    /// (it never breaks); see the module documentation.
    pub fn place<E: Fn(usize, &Vec<Placed>) -> bool, P: Fn(&Vec<Placed>, usize, bool) -> bool>(
        &mut self,
        segment_in: usize,
        field_in: usize,
        offset_in: usize,
        extra_in: usize,
        checked: usize,
        eval: &E,
        prefix: &P,
        Ghost(holds): Ghost<spec_fn(nat, Seq<Placed>) -> bool>,
        Ghost(pf): Ghost<spec_fn(Seq<Placed>, nat, bool) -> bool>,
        log: &mut Ghost<Seq<Ev>>,
    )
        requires
            eval_computes(*eval, holds),
            pfx_computes(*prefix, pf),
            old(self).wf(),
            segment_in < old(self).segments@.len(),
            field_in <= old(self).segs()[segment_in as int].len(),
            offset_in + extra_in + singles_from(old(self).segs()[segment_in as int], field_in as int)
                == old(self).sources@[segment_in as int].0,
        ensures
            final(self).segs() == old(self).segs(),
            final(self).sources@ == old(self).sources@,
            final(self).needs@ == old(self).needs@,
            final(self).fields@ == old(self).fields@,
            final(self).lengths@ == old(self).lengths@,
            final(log)@ == old(log)@ + spec_place(
                old(self).segs(),
                old(self).sources@,
                old(self).needs@,
                holds,
                pf,
                segment_in as nat,
                field_in as nat,
                offset_in as nat,
                extra_in as nat,
                checked as nat,
                old(self).fields@,
                old(self).lengths@,
            ),
        decreases usize::MAX - segment_in, usize::MAX - field_in,
    {
        let ghost segs = self.segs();
        let ghost srcs = self.sources@;
        let ghost needs = self.needs@;
        let ghost f0 = self.fields@;
        let ghost l0 = self.lengths@;
        let ghost log0 = log@;
        let ghost (segment0, field0) = (segment_in, field_in);
        let ghost target = spec_place(
            segs,
            srcs,
            needs,
            holds,
            pf,
            segment_in as nat,
            field_in as nat,
            offset_in as nat,
            extra_in as nat,
            checked as nat,
            f0,
            l0,
        );
        let mark = self.fields.len();
        // `place` takes these as `mut` parameters; the model copies them so
        // that the loop invariants can name the entry values.
        let mut segment = segment_in;
        let mut field = field_in;
        let mut offset = offset_in;
        let mut extra = extra_in;
        loop
            invariant_except_break
                segment < segs.len(),
                field <= segs[segment as int].len(),
                offset + extra + singles_from(segs[segment as int], field as int)
                    == srcs[segment as int].0,
                log@ == log0,
                spec_place(
                    segs,
                    srcs,
                    needs,
                    holds,
                    pf,
                    segment as nat,
                    field as nat,
                    offset as nat,
                    extra as nat,
                    checked as nat,
                    self.fields@,
                    l0,
                ) == target,
                segment0 <= segment,
                segment0 == segment_in,
                field0 == field_in,
                segment == segment0 ==> field0 <= field,
            invariant
                eval_computes(*eval, holds),
                pfx_computes(*prefix, pf),
                self.wf(),
                self.segs() == segs,
                segs.len() == self.segments@.len(),
                self.segments@.len() == old(self).segments@.len(),
                self.sources@ == srcs,
                self.needs@ == needs,
                self.lengths@ == l0,
                mark == f0.len(),
                self.fields@.len() >= mark,
                self.fields@.subrange(0, mark as int) == f0,
            ensures
                log@ == log0 + target,
            decreases rest_from(segs, segment as nat, field as nat),
        {
            let nsegments = self.segments.len();
            let nfields = self.segments[segment].len();
            if field == nfields {
                if segment + 1 < nsegments {
                    segment += 1;
                    field = 0;
                    offset = 0;
                    extra = self.sources[segment].1;
                    continue;
                }
                if self.ready_tests_pass(checked, true, eval, prefix, Ghost(holds), Ghost(pf)) {
                    proof {
                        *log = Ghost(log@.push(Ev::Match(self.lengths@)));
                        assert(log@ =~= log0 + target);
                    }
                } else {
                    proof {
                        assert(log@ =~= log0 + target);
                    }
                }
                break;
            }
            let is_single = match self.segments[segment][field] {
                Field::Single => true,
                Field::Multi => false,
            };
            if is_single {
                self.fields.push(Placed { segment, offset, length: 1, multi: false });
                field += 1;
                offset += 1;
                continue;
            }
            if !self.ready_tests_pass(checked, false, eval, prefix, Ghost(holds), Ghost(pf)) {
                proof {
                    assert(log@ =~= log0 + target);
                }
                break;
            }
            let checked = self.fields.len();
            // The segment's last capture takes whatever the others leave.
            let last = Self::last_capture(&self.segments[segment], field + 1);
            let shortest = if last {
                extra
            } else {
                0
            };
            let ghost fields_here = self.fields@;
            let ghost log_here = log@;
            let mut length = extra;
            loop
                invariant_except_break
                    shortest <= length,
                    log_here + spec_tries(
                        segs,
                        srcs,
                        needs,
                        holds,
                        pf,
                        segment as nat,
                        field as nat,
                        offset as nat,
                        extra as nat,
                        checked as nat,
                        fields_here,
                        l0,
                        extra as int,
                        shortest as nat,
                        last,
                    ) == log@ + spec_tries(
                        segs,
                        srcs,
                        needs,
                        holds,
                        pf,
                        segment as nat,
                        field as nat,
                        offset as nat,
                        extra as nat,
                        checked as nat,
                        fields_here,
                        l0,
                        length as int,
                        shortest as nat,
                        last,
                    ),
                invariant
                    eval_computes(*eval, holds),
                    pfx_computes(*prefix, pf),
                    self.wf(),
                    self.segs() == segs,
                    segs.len() == self.segments@.len(),
                    self.segments@.len() == old(self).segments@.len(),
                    self.sources@ == srcs,
                    self.needs@ == needs,
                    self.lengths@ == l0,
                    self.fields@ == fields_here,
                    length <= extra,
                    segment < segs.len(),
                    field < segs[segment as int].len(),
                    field < nfields,
                    nfields == segs[segment as int].len(),
                    is_multi(segs[segment as int][field as int]),
                    offset + extra + singles_from(segs[segment as int], field as int)
                        == srcs[segment as int].0,
                    segment0 <= segment,
                    segment0 == segment_in,
                    field0 == field_in,
                    segment == segment0 ==> field0 <= field,
                ensures
                    log@ == log_here + spec_tries(
                        segs,
                        srcs,
                        needs,
                        holds,
                        pf,
                        segment as nat,
                        field as nat,
                        offset as nat,
                        extra as nat,
                        checked as nat,
                        fields_here,
                        l0,
                        extra as int,
                        shortest as nat,
                        last,
                    ),
                decreases length,
            {
                let ghost log_before = log@;
                let ghost step: Seq<Ev> = if last {
                    seq![]
                } else {
                    seq![Ev::Step]
                };
                if !last {
                    proof {
                        *log = Ghost(log@.push(Ev::Step));
                    }
                }
                proof {
                    assert(log@ =~= log_before + step);
                }
                self.fields.push(Placed { segment, offset, length, multi: true });
                self.lengths.push(length);
                let ghost pushed_fields = self.fields@;
                let ghost pushed_lengths = self.lengths@;
                proof {
                    assert(self.segs() == segs);
                    assert(self.segments@.len() == old(self).segments@.len());
                    assert(segment0 <= segment);
                    assert(segment == segment0 ==> field0 < field + 1);
                    assert(rest_from(segs, segment as nat, (field + 1) as nat) < rest_from(
                        segs,
                        segment as nat,
                        field as nat,
                    ));

                    assert(singles_from(segs[segment as int], field as int) == singles_from(
                        segs[segment as int],
                        field + 1,
                    ));
                }
                self.place(
                    segment,
                    field + 1,
                    offset + length,
                    extra - length,
                    checked,
                    eval,
                    prefix,
                    Ghost(holds),
                    Ghost(pf),
                    log,
                );
                self.fields.pop();
                self.lengths.pop();
                proof {
                    assert(self.fields@ =~= fields_here);
                    assert(self.lengths@ =~= l0);
                    let p = spec_place(
                        segs,
                        srcs,
                        needs,
                        holds,
                        pf,
                        segment as nat,
                        (field + 1) as nat,
                        (offset + length) as nat,
                        (extra - length) as nat,
                        checked as nat,
                        pushed_fields,
                        pushed_lengths,
                    );
                    let rest_tries = spec_tries(
                        segs,
                        srcs,
                        needs,
                        holds,
                        pf,
                        segment as nat,
                        field as nat,
                        offset as nat,
                        extra as nat,
                        checked as nat,
                        fields_here,
                        l0,
                        length - 1,
                        shortest as nat,
                        last,
                    );
                    assert(pushed_fields == fields_here.push(
                        Placed { segment, offset, length, multi: true },
                    ));
                    assert(pushed_lengths == l0.push(length));
                    assert(spec_tries(
                        segs,
                        srcs,
                        needs,
                        holds,
                        pf,
                        segment as nat,
                        field as nat,
                        offset as nat,
                        extra as nat,
                        checked as nat,
                        fields_here,
                        l0,
                        length as int,
                        shortest as nat,
                        last,
                    ) == step + p + rest_tries);
                    assert(log@ == log_before + step + p);
                    assert(log_before + (step + p + rest_tries) =~= log@ + rest_tries);
                    if length == shortest {
                        assert(rest_tries == Seq::<Ev>::empty());
                        assert(log@ + rest_tries =~= log@);
                    }
                }
                if length == shortest {
                    break;
                }
                length -= 1;
            }
            proof {
                assert(log@ =~= log0 + target);
            }
            break;
        }
        self.fields.truncate(mark);
        proof {
            assert(self.fields@ =~= f0);
        }
    }
}

// ===========================================================================
// Declarative specification: splits, projections, acceptance
// ===========================================================================

/// Number of captures among `fs[..f]`.
pub open spec fn caps_before(fs: Seq<Field>, f: int) -> nat
    decreases f,
{
    if f <= 0 {
        0
    } else {
        caps_before(fs, f - 1) + (if is_multi(fs[f - 1]) {
            1nat
        } else {
            0nat
        })
    }
}

/// Number of captures in the segments before `s`.
pub open spec fn base(segs: Seq<Seq<Field>>, s: int) -> nat
    decreases s,
{
    if s <= 0 {
        0
    } else {
        base(segs, s - 1) + caps_before(segs[s - 1], segs[s - 1].len() as int)
    }
}

/// Index, in a split's capture lengths, of the capture at `(s, f)`.
pub open spec fn cap_index(segs: Seq<Seq<Field>>, s: int, f: int) -> nat {
    base(segs, s) + caps_before(segs[s], f)
}

/// Number of captures of the whole plan: the length of every split.
pub open spec fn total_caps(segs: Seq<Seq<Field>>) -> nat {
    base(segs, segs.len() as int)
}

/// Number of physical values field `(s, f)` takes under capture lengths `c`.
pub open spec fn width_of(segs: Seq<Seq<Field>>, c: Seq<usize>, s: int, f: int) -> nat {
    if is_multi(segs[s][f]) {
        c[cap_index(segs, s, f) as int] as nat
    } else {
        1
    }
}

/// Physical offset of field `(s, f)` in segment `s`'s source.
pub open spec fn off(segs: Seq<Seq<Field>>, c: Seq<usize>, s: int, f: int) -> nat
    decreases f,
{
    if f <= 0 {
        0
    } else {
        off(segs, c, s, f - 1) + width_of(segs, c, s, f - 1)
    }
}

/// The values taken by the fields of segment `s` from `f` on.
pub open spec fn off_from(segs: Seq<Seq<Field>>, c: Seq<usize>, s: int, f: int) -> nat
    decreases segs[s].len() - f,
{
    if f < 0 || f >= segs[s].len() {
        0
    } else {
        width_of(segs, c, s, f) + off_from(segs, c, s, f + 1)
    }
}

/// The logical field `(s, f)` of the projection under capture lengths `c`.
pub open spec fn desc(segs: Seq<Seq<Field>>, c: Seq<usize>, s: int, f: int) -> Placed {
    Placed {
        segment: s as usize,
        offset: off(segs, c, s, f) as usize,
        length: width_of(segs, c, s, f) as usize,
        multi: is_multi(segs[s][f]),
    }
}

/// The logical fields before position `(s, f)`, in order.
pub open spec fn descs_upto(segs: Seq<Seq<Field>>, c: Seq<usize>, s: int, f: int) -> Seq<Placed>
    decreases s, f,
{
    if f <= 0 {
        if s <= 0 {
            seq![]
        } else {
            descs_upto(segs, c, s - 1, segs[s - 1].len() as int)
        }
    } else {
        descs_upto(segs, c, s, f - 1).push(desc(segs, c, s, f - 1))
    }
}

/// The projected fact of split `c`: every logical field.
pub open spec fn all_descs(segs: Seq<Seq<Field>>, c: Seq<usize>) -> Seq<Placed> {
    descs_upto(segs, c, segs.len() as int, 0)
}

/// `c` is a split: one length per capture, and in every segment the fields,
/// in order, consume exactly the segment's source.
pub open spec fn is_split(segs: Seq<Seq<Field>>, srcs: Seq<(usize, usize)>, c: Seq<usize>) -> bool {
    &&& c.len() == total_caps(segs)
    &&& forall|s: int| 0 <= s < segs.len() ==> #[trigger] off(segs, c, s, segs[s].len() as int) == srcs[s].0
}

/// The prefix filter holds where `place` consults it on split `c` from
/// position `(s, f)` on: before each capture with the fields placed so far
/// and the fields checked at the previous capture, and on the complete
/// split.
pub open spec fn chain_from(
    segs: Seq<Seq<Field>>,
    c: Seq<usize>,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
    s: nat,
    f: nat,
    checked: nat,
) -> bool
    decreases rest_from(segs, s, f),
{
    if s >= segs.len() || f > segs[s as int].len() {
        true
    } else if f == segs[s as int].len() {
        if s + 1 < segs.len() {
            chain_from(segs, c, pf, s + 1, 0, checked)
        } else {
            pf(descs_upto(segs, c, s as int, f as int), checked, true)
        }
    } else if !is_multi(segs[s as int][f as int]) {
        chain_from(segs, c, pf, s, f + 1, checked)
    } else {
        pf(descs_upto(segs, c, s as int, f as int), checked, false) && chain_from(
            segs,
            c,
            pf,
            s,
            f + 1,
            descs_upto(segs, c, s as int, f as int).len(),
        )
    }
}

/// Constant tests read only the fields below their extent: a test decides
/// the same on two projections that agree on their first `need` fields.
/// (Kani proves this of the production evaluator for the atom tests; see
/// the module documentation.)
pub open spec fn local(needs: Seq<usize>, holds: spec_fn(nat, Seq<Placed>) -> bool) -> bool {
    forall|t: int, s1: Seq<Placed>, s2: Seq<Placed>|
        0 <= t < needs.len() && needs[t] <= s1.len() && needs[t] <= s2.len() && s1.subrange(
            0,
            needs[t] as int,
        ) == s2.subrange(0, needs[t] as int) ==> #[trigger] holds(t as nat, s1) == #[trigger] holds(
            t as nat,
            s2,
        )
}

/// Strict lexicographic order on capture-length vectors.
pub open spec fn lex_gt(a: Seq<usize>, b: Seq<usize>) -> bool {
    exists|k: int|
        0 <= k < a.len() && k < b.len() && a.subrange(0, k) == b.subrange(0, k) && a[k] > b[k]
}

pub open spec fn match_of(e: Ev) -> Option<Seq<usize>> {
    match e {
        Ev::Match(c) => Some(c),
        Ev::Step => None,
    }
}

/// Every match of `log` satisfies `p`.
pub open spec fn sound(log: Seq<Ev>, p: spec_fn(Seq<usize>) -> bool) -> bool {
    forall|i: int| 0 <= i < log.len() && match_of(#[trigger] log[i]) is Some ==> p(match_of(log[i])->0)
}

/// Every split satisfying `p` is a match of `log`.
pub open spec fn complete(log: Seq<Ev>, p: spec_fn(Seq<usize>) -> bool) -> bool {
    forall|c: Seq<usize>| #[trigger] p(c) ==> exists|i: int| 0 <= i < log.len() && log[i] == Ev::Match(c)
}

/// The matches of `log` strictly decrease lexicographically.
pub open spec fn sorted(log: Seq<Ev>) -> bool {
    forall|i: int, j: int|
        0 <= i < j < log.len() && match_of(#[trigger] log[i]) is Some && match_of(#[trigger] log[j]) is Some
            ==> lex_gt(match_of(log[i])->0, match_of(log[j])->0)
}
// ===========================================================================
// Lemmas about projections
// ===========================================================================

/// `(s, f)` names a position of the walk: a field boundary of a segment, or
/// the end of the plan.
pub open spec fn pos_ok(segs: Seq<Seq<Field>>, s: int, f: int) -> bool {
    (0 <= s < segs.len() && 0 <= f <= segs[s].len()) || (s == segs.len() && f == 0)
}

/// `c1` and `c2` agree on the first `n` capture lengths.
pub open spec fn agree(c1: Seq<usize>, c2: Seq<usize>, n: int) -> bool {
    forall|i: int| 0 <= i < n ==> c1[i] == c2[i]
}

pub proof fn lemma_caps_before_bound(fs: Seq<Field>, f: int)
    requires
        0 <= f <= fs.len(),
    ensures
        caps_before(fs, f) <= f,
    decreases f,
{
    if f > 0 {
        lemma_caps_before_bound(fs, f - 1);
    }
}

pub proof fn lemma_caps_before_mono(fs: Seq<Field>, f1: int, f2: int)
    requires
        0 <= f1 <= f2 <= fs.len(),
    ensures
        caps_before(fs, f1) <= caps_before(fs, f2),
        f1 < f2 && is_multi(fs[f1]) ==> caps_before(fs, f1) < caps_before(fs, f2),
    decreases f2 - f1,
{
    if f1 < f2 {
        lemma_caps_before_mono(fs, f1, f2 - 1);
        if f1 < f2 - 1 {
            lemma_caps_before_mono(fs, f1 + 1, f2 - 1);
        }
    }
}

pub proof fn lemma_base_mono(segs: Seq<Seq<Field>>, s1: int, s2: int)
    requires
        0 <= s1 <= s2 <= segs.len(),
    ensures
        base(segs, s1) <= base(segs, s2),
    decreases s2 - s1,
{
    if s1 < s2 {
        lemma_base_mono(segs, s1, s2 - 1);
    }
}

/// Capture indices grow along the walk: every capture before position
/// `(s, f)` has an index below `cap_index(segs, s, f)`.
pub proof fn lemma_cap_index_before(segs: Seq<Seq<Field>>, s1: int, f1: int, s2: int, f2: int)
    requires
        0 <= s1 < segs.len(),
        0 <= f1 < segs[s1].len(),
        is_multi(segs[s1][f1]),
        pos_ok(segs, s2, f2),
        s1 < s2 || (s1 == s2 && f1 < f2),
    ensures
        cap_index(segs, s1, f1) < (if s2 < segs.len() {
            cap_index(segs, s2, f2)
        } else {
            total_caps(segs)
        }),
{
    lemma_caps_before_mono(segs[s1], f1, segs[s1].len() as int);
    if s1 == s2 {
        lemma_caps_before_mono(segs[s1], f1, f2);
    } else {
        lemma_caps_before_mono(segs[s1], f1, segs[s1].len() as int);
        assert(base(segs, s1 + 1) == base(segs, s1) + caps_before(
            segs[s1],
            segs[s1].len() as int,
        ));
        lemma_base_mono(segs, s1 + 1, s2);
        if s2 < segs.len() {
            assert(cap_index(segs, s2, f2) >= base(segs, s2));
        }
    }
}

/// The offset of a field reads only the captures before it.
pub proof fn lemma_off_agree(segs: Seq<Seq<Field>>, c1: Seq<usize>, c2: Seq<usize>, s: int, f: int)
    requires
        0 <= s < segs.len(),
        0 <= f <= segs[s].len(),
        agree(c1, c2, cap_index(segs, s, f) as int),
    ensures
        off(segs, c1, s, f) == off(segs, c2, s, f),
    decreases f,
{
    if f > 0 {
        lemma_caps_before_mono(segs[s], f - 1, f);
        lemma_off_agree(segs, c1, c2, s, f - 1);
    }
}

/// The logical fields before a position read only the captures before it.
pub proof fn lemma_descs_agree(
    segs: Seq<Seq<Field>>,
    c1: Seq<usize>,
    c2: Seq<usize>,
    s: int,
    f: int,
)
    requires
        pos_ok(segs, s, f),
        agree(
            c1,
            c2,
            (if s < segs.len() {
                cap_index(segs, s, f)
            } else {
                total_caps(segs)
            }) as int,
        ),
    ensures
        descs_upto(segs, c1, s, f) == descs_upto(segs, c2, s, f),
    decreases s, f,
{
    if f <= 0 {
        if s > 0 {
            assert(base(segs, s) == base(segs, s - 1) + caps_before(
                segs[s - 1],
                segs[s - 1].len() as int,
            ));
            lemma_descs_agree(segs, c1, c2, s - 1, segs[s - 1].len() as int);
        }
    } else {
        lemma_caps_before_mono(segs[s], f - 1, f);
        lemma_descs_agree(segs, c1, c2, s, f - 1);
        lemma_off_agree(segs, c1, c2, s, f - 1);
    }
}

/// Logical fields only accumulate along the walk.
pub proof fn lemma_descs_prefix(
    segs: Seq<Seq<Field>>,
    c: Seq<usize>,
    s1: int,
    f1: int,
    s2: int,
    f2: int,
)
    requires
        pos_ok(segs, s1, f1),
        pos_ok(segs, s2, f2),
        s1 < s2 || (s1 == s2 && f1 <= f2),
    ensures
        descs_upto(segs, c, s1, f1).len() <= descs_upto(segs, c, s2, f2).len(),
        descs_upto(segs, c, s2, f2).subrange(0, descs_upto(segs, c, s1, f1).len() as int)
            == descs_upto(segs, c, s1, f1),
    decreases s2, f2,
{
    let d1 = descs_upto(segs, c, s1, f1);
    if s1 == s2 && f1 == f2 {
        assert(d1.subrange(0, d1.len() as int) =~= d1);
    } else if f2 <= 0 {
        // s2 > 0: step back over the end of the previous segment.
        lemma_descs_prefix(segs, c, s1, f1, s2 - 1, segs[s2 - 1].len() as int);
    } else {
        lemma_descs_prefix(segs, c, s1, f1, s2, f2 - 1);
        let d = descs_upto(segs, c, s2, f2 - 1);
        assert(descs_upto(segs, c, s2, f2) == d.push(desc(segs, c, s2, f2 - 1)));
        assert(d.push(desc(segs, c, s2, f2 - 1)).subrange(0, d1.len() as int) =~= d.subrange(
            0,
            d1.len() as int,
        ));
    }
}

/// A segment's fields from `f` on take `off_from` values after offset `off`.
pub proof fn lemma_off_split(segs: Seq<Seq<Field>>, c: Seq<usize>, s: int, f: int)
    requires
        0 <= s < segs.len(),
        0 <= f <= segs[s].len(),
    ensures
        off(segs, c, s, segs[s].len() as int) == off(segs, c, s, f) + off_from(segs, c, s, f),
    decreases segs[s].len() - f,
{
    if f < segs[s].len() {
        lemma_off_split(segs, c, s, f + 1);
    }
}

/// Single fields take one value each and captures any number.
pub proof fn lemma_off_from_singles(segs: Seq<Seq<Field>>, c: Seq<usize>, s: int, f: int)
    requires
        0 <= s < segs.len(),
        0 <= f <= segs[s].len(),
    ensures
        off_from(segs, c, s, f) >= singles_from(segs[s], f),
        all_single_from(segs[s], f) ==> off_from(segs, c, s, f) == singles_from(segs[s], f),
    decreases segs[s].len() - f,
{
    if f < segs[s].len() {
        lemma_off_from_singles(segs, c, s, f + 1);
        if all_single_from(segs[s], f) {
            assert(!is_multi(segs[s][f]));
            assert(all_single_from(segs[s], f + 1));
        }
    }
}

/// Every split has a capture at every capture position.
pub proof fn lemma_cap_in_split(segs: Seq<Seq<Field>>, s: int, f: int)
    requires
        0 <= s < segs.len(),
        0 <= f < segs[s].len(),
        is_multi(segs[s][f]),
    ensures
        cap_index(segs, s, f) < total_caps(segs),
{
    lemma_cap_index_before(segs, s, f, segs.len() as int, 0);
}
// ===========================================================================
// Event-log combinators
// ===========================================================================

pub proof fn lemma_sound_concat(a: Seq<Ev>, b: Seq<Ev>, p: spec_fn(Seq<usize>) -> bool)
    requires
        sound(a, p),
        sound(b, p),
    ensures
        sound(a + b, p),
{
    assert forall|i: int| 0 <= i < (a + b).len() && match_of(#[trigger] (a + b)[i]) is Some implies p(
        match_of((a + b)[i])->0,
    ) by {
        if i < a.len() {
            assert((a + b)[i] == a[i]);
        } else {
            assert((a + b)[i] == b[i - a.len()]);
        }
    }
}

pub proof fn lemma_sound_weaken(a: Seq<Ev>, p: spec_fn(Seq<usize>) -> bool, q: spec_fn(Seq<usize>) -> bool)
    requires
        sound(a, p),
        forall|c: Seq<usize>| #[trigger] p(c) ==> q(c),
    ensures
        sound(a, q),
{
}

pub proof fn lemma_complete_concat(
    a: Seq<Ev>,
    b: Seq<Ev>,
    pa: spec_fn(Seq<usize>) -> bool,
    pb: spec_fn(Seq<usize>) -> bool,
    p: spec_fn(Seq<usize>) -> bool,
)
    requires
        complete(a, pa),
        complete(b, pb),
        forall|c: Seq<usize>| #[trigger] p(c) ==> pa(c) || pb(c),
    ensures
        complete(a + b, p),
{
    assert forall|c: Seq<usize>| #[trigger] p(c) implies exists|i: int|
        0 <= i < (a + b).len() && (a + b)[i] == Ev::Match(c) by {
        if pa(c) {
            let i = choose|i: int| 0 <= i < a.len() && a[i] == Ev::Match(c);
            assert((a + b)[i] == a[i]);
        } else {
            let i = choose|i: int| 0 <= i < b.len() && b[i] == Ev::Match(c);
            assert((a + b)[i + a.len()] == b[i]);
        }
    }
}

pub proof fn lemma_sorted_concat(a: Seq<Ev>, b: Seq<Ev>)
    requires
        sorted(a),
        sorted(b),
        forall|i: int, j: int|
            0 <= i < a.len() && 0 <= j < b.len() && match_of(#[trigger] a[i]) is Some && match_of(
                #[trigger] b[j],
            ) is Some ==> lex_gt(match_of(a[i])->0, match_of(b[j])->0),
    ensures
        sorted(a + b),
{
    assert forall|i: int, j: int|
        0 <= i < j < (a + b).len() && match_of(#[trigger] (a + b)[i]) is Some && match_of(
            #[trigger] (a + b)[j],
        ) is Some implies lex_gt(match_of((a + b)[i])->0, match_of((a + b)[j])->0) by {
        if j < a.len() {
            assert((a + b)[i] == a[i] && (a + b)[j] == a[j]);
        } else if i >= a.len() {
            assert((a + b)[i] == b[i - a.len()] && (a + b)[j] == b[j - a.len()]);
        } else {
            assert((a + b)[i] == a[i] && (a + b)[j] == b[j - a.len()]);
        }
    }
}

pub proof fn lemma_step_log(last: bool)
    ensures
        ({
            let step: Seq<Ev> = if last {
                seq![]
            } else {
                seq![Ev::Step]
            };
            &&& forall|p: spec_fn(Seq<usize>) -> bool| sound(step, p)
            &&& sorted(step)
            &&& forall|i: int| 0 <= i < step.len() ==> match_of(#[trigger] step[i]) is None
        }),
{
}

// ===========================================================================
// The search enumerates exactly the accepted splits, in order
// ===========================================================================

/// The plan facts `search_with_prefix` establishes (see `SplitSearch::wf`).
pub open spec fn wf_plan(segs: Seq<Seq<Field>>, srcs: Seq<(usize, usize)>) -> bool {
    &&& segs.len() == srcs.len()
    &&& forall|s: int|
        0 <= s < srcs.len() ==> {
            &&& #[trigger] srcs[s].1 + singles_from(segs[s], 0) == srcs[s].0
            &&& (srcs[s].1 == 0 || !all_single_from(segs[s], 0))
        }
}

/// The invariant of a `place` state: `lengths` holds the captures before
/// `(seg, field)`, the finished segments are tiled, `offset` and `extra`
/// are the current physical offset and the values left for the segment's
/// remaining captures, and `fields` is the projection so far.
pub open spec fn state_ok(
    segs: Seq<Seq<Field>>,
    srcs: Seq<(usize, usize)>,
    seg: nat,
    field: nat,
    offset: nat,
    extra: nat,
    checked: nat,
    fields: Seq<Placed>,
    lengths: Seq<usize>,
) -> bool {
    &&& seg < segs.len()
    &&& field <= segs[seg as int].len()
    &&& lengths.len() == cap_index(segs, seg as int, field as int)
    &&& forall|s: int|
        0 <= s < seg ==> #[trigger] off(segs, lengths, s, segs[s].len() as int) == srcs[s].0
    &&& offset == off(segs, lengths, seg as int, field as int)
    &&& offset + extra + singles_from(segs[seg as int], field as int) == srcs[seg as int].0
    &&& (extra == 0 || !all_single_from(segs[seg as int], field as int))
    &&& fields == descs_upto(segs, lengths, seg as int, field as int)
    &&& checked <= fields.len()
}

pub open spec fn extends(c: Seq<usize>, lengths: Seq<usize>) -> bool {
    lengths.len() <= c.len() && c.subrange(0, lengths.len() as int) == lengths
}

/// Split `c` completes the state's prefix and passes everything the search
/// still checks from `(seg, field)`: the tests not yet decided at
/// `checked`, and the prefix filter from here on.
pub open spec fn accepts_from(
    segs: Seq<Seq<Field>>,
    srcs: Seq<(usize, usize)>,
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
    seg: nat,
    field: nat,
    checked: nat,
    lengths: Seq<usize>,
    c: Seq<usize>,
) -> bool {
    &&& extends(c, lengths)
    &&& is_split(segs, srcs, c)
    &&& forall|t: int|
        0 <= t < needs.len() && #[trigger] needs[t] > checked ==> holds(t as nat, all_descs(segs, c))
    &&& chain_from(segs, c, pf, seg, field, checked)
}

/// The splits the capture loop still reports when its next length is
/// `length`: the capture at `(seg, field)` takes `shortest..=length`.
pub open spec fn tries_acc(
    segs: Seq<Seq<Field>>,
    srcs: Seq<(usize, usize)>,
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
    seg: nat,
    field: nat,
    checked: nat,
    lengths: Seq<usize>,
    shortest: nat,
    length: int,
    c: Seq<usize>,
) -> bool {
    &&& lengths.len() < c.len()
    &&& shortest <= c[lengths.len() as int] <= length
    &&& accepts_from(
        segs,
        srcs,
        needs,
        holds,
        pf,
        seg,
        field + 1,
        checked,
        lengths.push(c[lengths.len() as int]),
        c,
    )
}

/// Tests decided on a prefix decide the same on every split extending it.
pub proof fn lemma_test_on_prefix(
    segs: Seq<Seq<Field>>,
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    seg: nat,
    field: nat,
    lengths: Seq<usize>,
    c: Seq<usize>,
    t: int,
)
    requires
        local(needs, holds),
        seg < segs.len(),
        field <= segs[seg as int].len(),
        lengths.len() == cap_index(segs, seg as int, field as int),
        extends(c, lengths),
        0 <= t < needs.len(),
        needs[t] <= descs_upto(segs, lengths, seg as int, field as int).len(),
    ensures
        descs_upto(segs, c, seg as int, field as int) == descs_upto(
            segs,
            lengths,
            seg as int,
            field as int,
        ),
        holds(t as nat, all_descs(segs, c)) == holds(
            t as nat,
            descs_upto(segs, lengths, seg as int, field as int),
        ),
{
    let fields = descs_upto(segs, lengths, seg as int, field as int);
    assert(agree(c, lengths, lengths.len() as int)) by {
        assert forall|i: int| 0 <= i < lengths.len() implies c[i] == lengths[i] by {
            assert(c.subrange(0, lengths.len() as int)[i] == c[i]);
        }
    }
    lemma_descs_agree(segs, c, lengths, seg as int, field as int);
    lemma_descs_prefix(segs, c, seg as int, field as int, segs.len() as int, 0);
    let all = all_descs(segs, c);
    let n = needs[t] as int;
    assert(all.subrange(0, n) =~= fields.subrange(0, n)) by {
        assert(all.subrange(0, fields.len() as int) == fields);
        assert forall|i: int| 0 <= i < n implies all.subrange(0, n)[i] == fields.subrange(0, n)[i] by {
            assert(all.subrange(0, fields.len() as int)[i] == all[i]);
        }
    }
}

proof fn lemma_agree_extends(c: Seq<usize>, lengths: Seq<usize>)
    requires
        extends(c, lengths),
    ensures
        agree(c, lengths, lengths.len() as int),
{
    assert forall|i: int| 0 <= i < lengths.len() implies c[i] == lengths[i] by {
        assert(c.subrange(0, lengths.len() as int)[i] == c[i]);
    }
}

/// Finished segments stay tiled when a capture is appended.
proof fn lemma_tiled_push(
    segs: Seq<Seq<Field>>,
    srcs: Seq<(usize, usize)>,
    seg: nat,
    lengths: Seq<usize>,
    length: usize,
)
    requires
        seg < segs.len(),
        lengths.len() >= base(segs, seg as int),
        forall|s: int|
            0 <= s < seg ==> #[trigger] off(segs, lengths, s, segs[s].len() as int) == srcs[s].0,
    ensures
        forall|s: int|
            0 <= s < seg ==> #[trigger] off(segs, lengths.push(length), s, segs[s].len() as int)
                == srcs[s].0,
{
    assert forall|s: int| 0 <= s < seg implies #[trigger] off(
        segs,
        lengths.push(length),
        s,
        segs[s].len() as int,
    ) == srcs[s].0 by {
        lemma_base_mono(segs, s + 1, seg as int);
        assert(base(segs, s + 1) == base(segs, s) + caps_before(segs[s], segs[s].len() as int));
        assert(agree(lengths.push(length), lengths, cap_index(segs, s, segs[s].len() as int) as int));
        lemma_off_agree(segs, lengths.push(length), lengths, s, segs[s].len() as int);
    }
}

pub proof fn lemma_place(
    segs: Seq<Seq<Field>>,
    srcs: Seq<(usize, usize)>,
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
    seg: nat,
    field: nat,
    offset: nat,
    extra: nat,
    checked: nat,
    fields: Seq<Placed>,
    lengths: Seq<usize>,
)
    requires
        wf_plan(segs, srcs),
        local(needs, holds),
        state_ok(segs, srcs, seg, field, offset, extra, checked, fields, lengths),
    ensures
        ({
            let out = spec_place(
                segs,
                srcs,
                needs,
                holds,
                pf,
                seg,
                field,
                offset,
                extra,
                checked,
                fields,
                lengths,
            );
            let p = |c: Seq<usize>|
                accepts_from(segs, srcs, needs, holds, pf, seg, field, checked, lengths, c);
            &&& sound(out, p)
            &&& complete(out, p)
            &&& sorted(out)
        }),
    decreases rest_from(segs, seg, field), 1nat, 0int,
{
    let out = spec_place(
        segs,
        srcs,
        needs,
        holds,
        pf,
        seg,
        field,
        offset,
        extra,
        checked,
        fields,
        lengths,
    );
    let p = |c: Seq<usize>|
        accepts_from(segs, srcs, needs, holds, pf, seg, field, checked, lengths, c);
    let len = segs[seg as int].len();
    if field == len {
        // The segment is tiled: its remaining share is zero.
        assert(all_single_from(segs[seg as int], field as int));
        assert(extra == 0);
        assert(base(segs, seg + 1 as int) == base(segs, seg as int) + caps_before(
            segs[seg as int],
            len as int,
        ));
        if seg + 1 < segs.len() {
            let s2 = (seg + 1) as nat;
            let extra2 = srcs[s2 as int].1 as nat;
            assert(state_ok(segs, srcs, s2, 0, 0, extra2, checked, fields, lengths)) by {
                assert forall|s: int| 0 <= s < s2 implies #[trigger] off(
                    segs,
                    lengths,
                    s,
                    segs[s].len() as int,
                ) == srcs[s].0 by {
                    if s == seg {
                    }
                }
            }
            lemma_place(segs, srcs, needs, holds, pf, s2, 0, 0, extra2, checked, fields, lengths);
            let p2 = |c: Seq<usize>|
                accepts_from(segs, srcs, needs, holds, pf, s2, 0, checked, lengths, c);
            assert(p =~= p2);
        } else {
            assert(total_caps(segs) == lengths.len());
            assert(all_descs(segs, lengths) == fields);
            let r = ready(needs, holds, pf, fields, checked, true);
            assert forall|c: Seq<usize>| #[trigger] p(c) implies c == lengths && r by {
                assert(c =~= lengths) by {
                    assert(c.subrange(0, lengths.len() as int) == lengths);
                }
                assert(all_descs(segs, c) == fields);
            }
            if r {
                assert(p(lengths)) by {
                    assert(lengths.subrange(0, lengths.len() as int) =~= lengths);
                    assert forall|s: int| 0 <= s < segs.len() implies #[trigger] off(
                        segs,
                        lengths,
                        s,
                        segs[s].len() as int,
                    ) == srcs[s].0 by {
                        if s == seg {
                        }
                    }
                }
                assert(out == seq![Ev::Match(lengths)]);
                assert(out[0] == Ev::Match(lengths));
            } else {
                assert(out == Seq::<Ev>::empty());
            }
        }
    } else if !is_multi(segs[seg as int][field as int]) {
        let f2 = (field + 1) as nat;
        let fields2 = fields.push(
            Placed { segment: seg as usize, offset: offset as usize, length: 1, multi: false },
        );
        assert(state_ok(segs, srcs, seg, f2, offset + 1, extra, checked, fields2, lengths)) by {
            assert(caps_before(segs[seg as int], f2 as int) == caps_before(
                segs[seg as int],
                field as int,
            ));
            assert(all_single_from(segs[seg as int], f2 as int) ==> all_single_from(
                segs[seg as int],
                field as int,
            ));
            assert(descs_upto(segs, lengths, seg as int, f2 as int) == descs_upto(
                segs,
                lengths,
                seg as int,
                field as int,
            ).push(desc(segs, lengths, seg as int, field as int)));
        }
        lemma_place(
            segs,
            srcs,
            needs,
            holds,
            pf,
            seg,
            f2,
            offset + 1,
            extra,
            checked,
            fields2,
            lengths,
        );
        let p2 = |c: Seq<usize>|
            accepts_from(segs, srcs, needs, holds, pf, seg, f2, checked, lengths, c);
        assert(p =~= p2);
    } else {
        let r = ready(needs, holds, pf, fields, checked, false);
        if !r {
            assert(out == Seq::<Ev>::empty());
            assert forall|c: Seq<usize>| #[trigger] p(c) implies false by {
                lemma_agree_extends(c, lengths);
                lemma_descs_agree(segs, c, lengths, seg as int, field as int);
                if !pf(fields, checked, false) {
                    assert(!chain_from(segs, c, pf, seg, field, checked));
                } else {
                    let t = choose|t: int|
                        0 <= t < needs.len() && !(needs[t] <= checked || (needs[t] > fields.len()
                            && !false) || holds(t as nat, fields));
                    lemma_test_on_prefix(segs, needs, holds, seg, field, lengths, c, t);
                }
            }
        } else {
            let last = all_single_from(segs[seg as int], field + 1 as int);
            let shortest: nat = if last {
                extra
            } else {
                0
            };
            let checked2 = fields.len();
            assert(state_ok(segs, srcs, seg, field, offset, extra, checked2, fields, lengths));
            lemma_tries(
                segs,
                srcs,
                needs,
                holds,
                pf,
                seg,
                field,
                offset,
                extra,
                checked2,
                fields,
                lengths,
                extra as int,
                shortest,
                last,
            );
            let pt = |c: Seq<usize>|
                tries_acc(
                    segs,
                    srcs,
                    needs,
                    holds,
                    pf,
                    seg,
                    field,
                    checked2,
                    lengths,
                    shortest,
                    extra as int,
                    c,
                );
            assert forall|c: Seq<usize>| #[trigger] p(c) == pt(c) by {
                if p(c) {
                    lemma_agree_extends(c, lengths);
                    lemma_descs_agree(segs, c, lengths, seg as int, field as int);
                    lemma_cap_in_split(segs, seg as int, field as int);
                    let l = c[lengths.len() as int];
                    // The capture's length is bounded by the segment's share.
                    lemma_off_split(segs, c, seg as int, field as int);
                    lemma_off_agree(segs, c, lengths, seg as int, field as int);
                    lemma_off_from_singles(segs, c, seg as int, field + 1 as int);
                    assert(off(segs, c, seg as int, len as int) == srcs[seg as int].0);
                    assert(singles_from(segs[seg as int], field as int) == singles_from(
                        segs[seg as int],
                        field + 1 as int,
                    ));
                    assert(l <= extra);
                    assert(last ==> l == extra);
                    assert(c.subrange(0, lengths.len() + 1 as int) =~= lengths.push(l));
                    assert forall|t: int|
                        0 <= t < needs.len() && #[trigger] needs[t] > checked2 implies holds(
                        t as nat,
                        all_descs(segs, c),
                    ) by {
                    }
                    assert(pt(c));
                }
                if pt(c) {
                    let l = c[lengths.len() as int];
                    let lengths2 = lengths.push(l);
                    assert(c.subrange(0, lengths.len() as int) =~= lengths2.subrange(
                        0,
                        lengths.len() as int,
                    ));
                    assert(lengths2.subrange(0, lengths.len() as int) =~= lengths);
                    assert(c.subrange(0, lengths.len() as int) =~= c.subrange(
                        0,
                        lengths2.len() as int,
                    ).subrange(0, lengths.len() as int));
                    assert(extends(c, lengths));
                    lemma_agree_extends(c, lengths);
                    lemma_descs_agree(segs, c, lengths, seg as int, field as int);
                    assert forall|t: int|
                        0 <= t < needs.len() && #[trigger] needs[t] > checked implies holds(
                        t as nat,
                        all_descs(segs, c),
                    ) by {
                        if needs[t] <= checked2 {
                            lemma_test_on_prefix(segs, needs, holds, seg, field, lengths, c, t);
                        }
                    }
                    assert(p(c));
                }
            }
            assert(p =~= pt);
        }
    }
}

pub proof fn lemma_tries(
    segs: Seq<Seq<Field>>,
    srcs: Seq<(usize, usize)>,
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
    seg: nat,
    field: nat,
    offset: nat,
    extra: nat,
    checked: nat,
    fields: Seq<Placed>,
    lengths: Seq<usize>,
    length: int,
    shortest: nat,
    last: bool,
)
    requires
        wf_plan(segs, srcs),
        local(needs, holds),
        state_ok(segs, srcs, seg, field, offset, extra, checked, fields, lengths),
        field < segs[seg as int].len(),
        is_multi(segs[seg as int][field as int]),
        checked == fields.len(),
        last == all_single_from(segs[seg as int], field + 1 as int),
        shortest == (if last {
            extra
        } else {
            0
        }),
        length <= extra,
    ensures
        ({
            let out = spec_tries(
                segs,
                srcs,
                needs,
                holds,
                pf,
                seg,
                field,
                offset,
                extra,
                checked,
                fields,
                lengths,
                length,
                shortest,
                last,
            );
            let p = |c: Seq<usize>|
                tries_acc(
                    segs,
                    srcs,
                    needs,
                    holds,
                    pf,
                    seg,
                    field,
                    checked,
                    lengths,
                    shortest,
                    length,
                    c,
                );
            &&& sound(out, p)
            &&& complete(out, p)
            &&& sorted(out)
        }),
    decreases rest_from(segs, seg, field), 0nat, length + 1,
{
    let out = spec_tries(
        segs,
        srcs,
        needs,
        holds,
        pf,
        seg,
        field,
        offset,
        extra,
        checked,
        fields,
        lengths,
        length,
        shortest,
        last,
    );
    let p = |c: Seq<usize>|
        tries_acc(segs, srcs, needs, holds, pf, seg, field, checked, lengths, shortest, length, c);
    if length < shortest || length < 0 {
        assert(out == Seq::<Ev>::empty());
        assert forall|c: Seq<usize>| #[trigger] p(c) implies false by {
        }
    } else {
        let l = length as usize;
        let f2 = (field + 1) as nat;
        let fields2 = fields.push(
            Placed { segment: seg as usize, offset: offset as usize, length: l, multi: true },
        );
        let lengths2 = lengths.push(l);
        assert(state_ok(
            segs,
            srcs,
            seg,
            f2,
            offset + length as nat,
            (extra - length) as nat,
            checked,
            fields2,
            lengths2,
        )) by {
            assert(caps_before(segs[seg as int], f2 as int) == caps_before(
                segs[seg as int],
                field as int,
            ) + 1);
            lemma_caps_before_mono(segs[seg as int], field as int, segs[seg as int].len() as int);
            lemma_tiled_push(segs, srcs, seg, lengths, l);
            assert(agree(lengths2, lengths, lengths.len() as int));
            lemma_off_agree(segs, lengths2, lengths, seg as int, field as int);
            lemma_descs_agree(segs, lengths2, lengths, seg as int, field as int);
            assert(singles_from(segs[seg as int], field as int) == singles_from(
                segs[seg as int],
                f2 as int,
            ));
            assert(descs_upto(segs, lengths2, seg as int, f2 as int) == descs_upto(
                segs,
                lengths2,
                seg as int,
                field as int,
            ).push(desc(segs, lengths2, seg as int, field as int)));
        }
        lemma_place(
            segs,
            srcs,
            needs,
            holds,
            pf,
            seg,
            f2,
            offset + length as nat,
            (extra - length) as nat,
            checked,
            fields2,
            lengths2,
        );
        lemma_tries(
            segs,
            srcs,
            needs,
            holds,
            pf,
            seg,
            field,
            offset,
            extra,
            checked,
            fields,
            lengths,
            length - 1,
            shortest,
            last,
        );
        let step: Seq<Ev> = if last {
            seq![]
        } else {
            seq![Ev::Step]
        };
        let placed = spec_place(
            segs,
            srcs,
            needs,
            holds,
            pf,
            seg,
            f2,
            offset + length as nat,
            (extra - length) as nat,
            checked,
            fields2,
            lengths2,
        );
        let rest = spec_tries(
            segs,
            srcs,
            needs,
            holds,
            pf,
            seg,
            field,
            offset,
            extra,
            checked,
            fields,
            lengths,
            length - 1,
            shortest,
            last,
        );
        assert(out == step + placed + rest);
        let pp = |c: Seq<usize>|
            accepts_from(segs, srcs, needs, holds, pf, seg, f2, checked, lengths2, c);
        let pr = |c: Seq<usize>|
            tries_acc(
                segs,
                srcs,
                needs,
                holds,
                pf,
                seg,
                field,
                checked,
                lengths,
                shortest,
                length - 1,
                c,
            );
        // Matches of the placed block take `length` for this capture.
        assert forall|c: Seq<usize>| #[trigger] pp(c) implies p(c) && c[lengths.len() as int] == l
            && c.subrange(0, lengths.len() as int) == lengths by {
            assert(c.subrange(0, lengths2.len() as int)[lengths.len() as int] == c[lengths.len() as int]);
            assert(c.subrange(0, lengths.len() as int) =~= c.subrange(
                0,
                lengths2.len() as int,
            ).subrange(0, lengths.len() as int));
        }
        assert forall|c: Seq<usize>| #[trigger] pr(c) implies p(c) && c[lengths.len() as int] < l
            && c.subrange(0, lengths.len() as int) == lengths by {
            let k = c[lengths.len() as int];
            let lk = lengths.push(k);
            assert(c.subrange(0, lengths.len() as int) =~= c.subrange(0, lk.len() as int).subrange(
                0,
                lengths.len() as int,
            ));
        }
        assert forall|c: Seq<usize>| #[trigger] p(c) implies pp(c) || pr(c) by {
            if c[lengths.len() as int] == l {
                assert(lengths.push(c[lengths.len() as int]) == lengths2);
            }
        }
        lemma_step_log(last);
        lemma_sound_weaken(placed, pp, p);
        lemma_sound_weaken(rest, pr, p);
        lemma_sound_concat(step, placed, p);
        lemma_sound_concat(step + placed, rest, p);
        let none = |c: Seq<usize>| false;
        assert(complete(step, none));
        lemma_complete_concat(step, placed, none, pp, pp);
        lemma_complete_concat(step + placed, rest, pp, pr, p);
        lemma_sorted_concat(step, placed);
        assert forall|i: int, j: int|
            0 <= i < (step + placed).len() && 0 <= j < rest.len() && match_of(
                #[trigger] (step + placed)[i],
            ) is Some && match_of(#[trigger] rest[j]) is Some implies lex_gt(
            match_of((step + placed)[i])->0,
            match_of(rest[j])->0,
        ) by {
            assert(i >= step.len());
            assert((step + placed)[i] == placed[i - step.len()]);
            let a = match_of(placed[i - step.len()])->0;
            let b = match_of(rest[j])->0;
            assert(pp(a));
            assert(pr(b));
            let k = lengths.len() as int;
            assert(a.subrange(0, k) =~= b.subrange(0, k));
        }
        lemma_sorted_concat(step + placed, rest);
    }
}
// ===========================================================================
// `segment_extra`, `search_with_prefix`, and the main theorem
// ===========================================================================

/// Spec of `segment_extra` (crates/ferric-rules-core/src/sequence.rs:406):
/// the values a segment's captures share, or `None` when its single fields
/// cannot fit (or, without captures, do not fill) the source.
pub open spec fn spec_segment_extra(fs: Seq<Field>, len: usize) -> Option<usize> {
    let singles = singles_from(fs, 0);
    if len < singles {
        None
    } else if len - singles == 0 || singles < fs.len() {
        Some((len - singles) as usize)
    } else {
        None
    }
}

pub proof fn lemma_singles_bound(fs: Seq<Field>, f: int)
    requires
        0 <= f <= fs.len(),
    ensures
        singles_from(fs, f) <= fs.len() - f,
        singles_from(fs, f) == fs.len() - f <==> all_single_from(fs, f),
    decreases fs.len() - f,
{
    if f < fs.len() {
        lemma_singles_bound(fs, f + 1);
        if !is_multi(fs[f]) && all_single_from(fs, f + 1) {
            assert(all_single_from(fs, f));
        }
    }
}

/// Models `segment_extra` (crates/ferric-rules-core/src/sequence.rs:406);
/// `len` is `values.len()` and the `filter(..).count()` is the loop.
pub fn segment_extra(fields: &Vec<Field>, len: usize) -> (r: Option<usize>)
    ensures
        r == spec_segment_extra(fields@, len),
{
    let mut singles: usize = 0;
    let mut j: usize = 0;
    while j < fields.len()
        invariant
            j <= fields.len(),
            singles <= j,
            singles + singles_from(fields@, j as int) == singles_from(fields@, 0),
        decreases fields.len() - j,
    {
        match fields[j] {
            Field::Single => {
                singles += 1;
            },
            Field::Multi => {},
        }
        j += 1;
    }
    // `values.len().checked_sub(singles)?`
    if len < singles {
        return None;
    }
    let extra = len - singles;
    if extra == 0 || singles < fields.len() {
        Some(extra)
    } else {
        None
    }
}

/// Every segment has a source (`segment_values` is `Some`) whose single
/// fields fit (`segment_extra` is `Some`).
pub open spec fn all_sources(segs: Seq<Seq<Field>>, lens: Seq<Option<usize>>) -> bool {
    forall|s: int|
        0 <= s < segs.len() ==> #[trigger] lens[s] is Some && spec_segment_extra(segs[s], lens[s]->0)
            is Some
}

pub open spec fn srcs_of(segs: Seq<Seq<Field>>, lens: Seq<Option<usize>>) -> Seq<(usize, usize)> {
    Seq::new(segs.len(), |s: int| (lens[s]->0, spec_segment_extra(segs[s], lens[s]->0)->0))
}

/// The events of `search_with_prefix`.
pub open spec fn search_log(
    segs: Seq<Seq<Field>>,
    lens: Seq<Option<usize>>,
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
) -> Seq<Ev> {
    if segs.len() == 0 || lens.len() != segs.len() || !all_sources(segs, lens) {
        seq![]
    } else {
        let srcs = srcs_of(segs, lens);
        spec_place(segs, srcs, needs, holds, pf, 0, 0, 0, srcs[0].1 as nat, 0, seq![], seq![])
    }
}

/// Split `c` tiles every segment's source (`lens[s]` is the length of the
/// values `segment_values` gives segment `s`; a missing source admits no
/// split).
pub open spec fn tiles(segs: Seq<Seq<Field>>, lens: Seq<Option<usize>>, c: Seq<usize>) -> bool {
    &&& c.len() == total_caps(segs)
    &&& lens.len() == segs.len()
    &&& forall|s: int|
        0 <= s < segs.len() ==> #[trigger] lens[s] is Some && off(segs, c, s, segs[s].len() as int)
            == lens[s]->0
}

/// The reference acceptance of split `c`: the plan has segments, `c` tiles
/// every source, every constant test holds on the projection, and the
/// prefix filter holds wherever `place` consults it (`chain_from`).
pub open spec fn accepted(
    segs: Seq<Seq<Field>>,
    lens: Seq<Option<usize>>,
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
    c: Seq<usize>,
) -> bool {
    &&& segs.len() > 0
    &&& tiles(segs, lens, c)
    &&& forall|t: int| 0 <= t < needs.len() ==> #[trigger] holds(t as nat, all_descs(segs, c))
    &&& chain_from(segs, c, pf, 0, 0, 0)
}

/// Main theorem: the search reports exactly the accepted splits, each once,
/// in strictly decreasing lexicographic order of capture lengths — for any
/// plan, any source lengths, any constant tests that read only the fields
/// below their extent, and any prefix filter.
pub proof fn theorem_search(
    segs: Seq<Seq<Field>>,
    lens: Seq<Option<usize>>,
    needs: Seq<usize>,
    holds: spec_fn(nat, Seq<Placed>) -> bool,
    pf: spec_fn(Seq<Placed>, nat, bool) -> bool,
)
    requires
        local(needs, holds),
        // `test_extent` is at least 1 (crates/ferric-rules-core/src/sequence.rs:401).
        forall|t: int| 0 <= t < needs.len() ==> #[trigger] needs[t] >= 1,
    ensures
        ({
            let out = search_log(segs, lens, needs, holds, pf);
            let p = |c: Seq<usize>| accepted(segs, lens, needs, holds, pf, c);
            &&& sound(out, p)
            &&& complete(out, p)
            &&& sorted(out)
        }),
{
    let out = search_log(segs, lens, needs, holds, pf);
    let p = |c: Seq<usize>| accepted(segs, lens, needs, holds, pf, c);
    if segs.len() == 0 || lens.len() != segs.len() || !all_sources(segs, lens) {
        assert(out == Seq::<Ev>::empty());
        assert forall|c: Seq<usize>| #[trigger] p(c) implies false by {
            if segs.len() > 0 && lens.len() == segs.len() {
                let s = choose|s: int|
                    0 <= s < segs.len() && !(#[trigger] lens[s] is Some && spec_segment_extra(
                        segs[s],
                        lens[s]->0,
                    ) is Some);
                assert(lens[s] is Some);
                let n = lens[s]->0;
                lemma_off_split(segs, c, s, 0);
                lemma_off_from_singles(segs, c, s, 0);
                lemma_singles_bound(segs[s], 0);
            }
        }
    } else {
        let srcs = srcs_of(segs, lens);
        assert(wf_plan(segs, srcs)) by {
            assert forall|s: int| 0 <= s < srcs.len() implies {
                &&& #[trigger] srcs[s].1 + singles_from(segs[s], 0) == srcs[s].0
                &&& (srcs[s].1 == 0 || !all_single_from(segs[s], 0))
            } by {
                assert(lens[s] is Some);
                lemma_singles_bound(segs[s], 0);
            }
        }
        let extra = srcs[0].1 as nat;
        assert(state_ok(segs, srcs, 0, 0, 0, extra, 0, seq![], seq![])) by {
            assert(wf_plan(segs, srcs));
            assert(srcs[0].1 + singles_from(segs[0], 0) == srcs[0].0);
        }
        lemma_place(segs, srcs, needs, holds, pf, 0, 0, 0, extra, 0, seq![], seq![]);
        let q = |c: Seq<usize>|
            accepts_from(segs, srcs, needs, holds, pf, 0, 0, 0, seq![], c);
        assert forall|c: Seq<usize>| #[trigger] p(c) == q(c) by {
            assert(c.subrange(0, 0) =~= Seq::<usize>::empty());
            if p(c) {
                assert forall|s: int| 0 <= s < segs.len() implies #[trigger] off(
                    segs,
                    c,
                    s,
                    segs[s].len() as int,
                ) == srcs[s].0 by {
                    assert(lens[s] is Some);
                }
                assert(is_split(segs, srcs, c));
            }
            if q(c) {
                assert forall|s: int| 0 <= s < segs.len() implies #[trigger] lens[s] is Some && off(
                    segs,
                    c,
                    s,
                    segs[s].len() as int,
                ) == lens[s]->0 by {
                    assert(off(segs, c, s, segs[s].len() as int) == srcs[s].0);
                }
                assert forall|t: int| 0 <= t < needs.len() implies #[trigger] holds(
                    t as nat,
                    all_descs(segs, c),
                ) by {
                    assert(needs[t] >= 1);
                }
            }
        }
        assert(p =~= q);
    }
}

/// Models `SequencePattern::search_with_prefix`
/// (crates/ferric-rules-core/src/sequence.rs:265). `source_lens[s]` stands
/// for `segment_values(segments[s].source, fact)` (its length, or `None`);
/// `needs` is the plan's tests mapped through `test_extent`.
pub fn search_with_prefix<E: Fn(usize, &Vec<Placed>) -> bool, P: Fn(&Vec<Placed>, usize, bool) -> bool>(
    segments: Vec<Vec<Field>>,
    needs: Vec<usize>,
    source_lens: Vec<Option<usize>>,
    eval: &E,
    prefix: &P,
    Ghost(holds): Ghost<spec_fn(nat, Seq<Placed>) -> bool>,
    Ghost(pf): Ghost<spec_fn(Seq<Placed>, nat, bool) -> bool>,
    log: &mut Ghost<Seq<Ev>>,
)
    requires
        eval_computes(*eval, holds),
        pfx_computes(*prefix, pf),
        source_lens@.len() == segments@.len(),
    ensures
        final(log)@ == old(log)@ + search_log(
            segments@.map_values(|v: Vec<Field>| v@),
            source_lens@,
            needs@,
            holds,
            pf,
        ),
{
    let ghost segs = segments@.map_values(|v: Vec<Field>| v@);
    let ghost lens = source_lens@;
    let mut sources: Vec<(usize, usize)> = Vec::new();
    let mut s: usize = 0;
    while s < segments.len()
        invariant
            s <= segments@.len(),
            segs == segments@.map_values(|v: Vec<Field>| v@),
            lens == source_lens@,
            lens.len() == segs.len(),
            sources@.len() == s,
            log@ == old(log)@,
            forall|i: int|
                0 <= i < s ==> #[trigger] lens[i] is Some && spec_segment_extra(segs[i], lens[i]->0)
                    is Some && sources@[i] == (lens[i]->0, spec_segment_extra(
                    segs[i],
                    lens[i]->0,
                )->0),
        decreases segments.len() - s,
    {
        let values = match source_lens[s] {
            Some(n) => n,
            None => {
                proof {
                    assert(!all_sources(segs, lens));
                    assert(log@ =~= old(log)@ + search_log(segs, lens, needs@, holds, pf));
                }
                return ;
            },
        };
        let extra = match segment_extra(&segments[s], values) {
            Some(e) => e,
            None => {
                proof {
                    assert(segs[s as int] == segments@[s as int]@);
                    assert(!all_sources(segs, lens));
                    assert(log@ =~= old(log)@ + search_log(segs, lens, needs@, holds, pf));
                }
                return ;
            },
        };
        proof {
            assert(segs[s as int] == segments@[s as int]@);
        }
        sources.push((values, extra));
        s += 1;
    }
    if sources.len() == 0 {
        proof {
            assert(log@ =~= old(log)@ + search_log(segs, lens, needs@, holds, pf));
        }
        return ;
    }
    let extra = sources[0].1;
    let ghost srcs = srcs_of(segs, lens);
    proof {
        assert(sources@ =~= srcs);
        assert(all_sources(segs, lens));
    }
    let mut search = SplitSearch {
        segments,
        needs,
        sources,
        fields: Vec::new(),
        lengths: Vec::new(),
    };
    proof {
        assert(search.segs() == segs);
        assert(search.wf()) by {
            assert forall|i: int| 0 <= i < search.sources@.len() implies {
                &&& #[trigger] search.sources@[i].1 + singles_from(search.segs()[i], 0)
                    == search.sources@[i].0
                &&& (search.sources@[i].1 == 0 || !all_single_from(search.segs()[i], 0))
            } by {
                lemma_singles_bound(segs[i], 0);
            }
        }
        assert(search.fields@ =~= Seq::<Placed>::empty());
        assert(search.lengths@ =~= Seq::<usize>::empty());
    }
    search.place(0, 0, 0, extra, 0, eval, prefix, Ghost(holds), Ghost(pf), log);
}
// ===========================================================================
// Non-vacuity check
// ===========================================================================

/// A worked instance of `theorem_search`: for one segment `(row $? $?)`
/// over a one-value source, with no tests and an accepting filter, both
/// splits `[1, 0]` and `[0, 1]` are accepted, so the search reports both,
/// `[1, 0]` first. (Shows the theorem's premises are satisfiable and its
/// conclusions are not vacuous.)
pub proof fn example_two_captures() {
    let segs = seq![seq![Field::Multi, Field::Multi]];
    let lens = seq![Some(1usize)];
    let needs = Seq::<usize>::empty();
    let holds = |t: nat, s: Seq<Placed>| true;
    let pf = |s: Seq<Placed>, c: nat, d: bool| true;
    theorem_search(segs, lens, needs, holds, pf);
    let out = search_log(segs, lens, needs, holds, pf);
    let a = seq![1usize, 0];
    let b = seq![0usize, 1];
    assert(caps_before(segs[0], 2) == 2) by {
        reveal_with_fuel(caps_before, 3);
    }
    assert(total_caps(segs) == 2) by {
        reveal_with_fuel(base, 2);
    }
    assert(off(segs, a, 0, 2) == 1) by {
        reveal_with_fuel(off, 3);
        reveal_with_fuel(caps_before, 3);
        reveal_with_fuel(base, 2);
    }
    assert(off(segs, b, 0, 2) == 1) by {
        reveal_with_fuel(off, 3);
        reveal_with_fuel(caps_before, 3);
        reveal_with_fuel(base, 2);
    }
    assert(chain_from(segs, a, pf, 0, 0, 0)) by {
        reveal_with_fuel(chain_from, 4);
    }
    assert(chain_from(segs, b, pf, 0, 0, 0)) by {
        reveal_with_fuel(chain_from, 4);
    }
    let p = |c: Seq<usize>| accepted(segs, lens, needs, holds, pf, c);
    assert(p(a));
    assert(p(b));
    let i = choose|i: int| 0 <= i < out.len() && out[i] == Ev::Match(a);
    let j = choose|j: int| 0 <= j < out.len() && out[j] == Ev::Match(b);
    assert(match_of(out[i]) is Some && match_of(out[j]) is Some);
    if j < i {
        assert(lex_gt(b, a));
        let k = choose|k: int|
            0 <= k < b.len() && k < a.len() && b.subrange(0, k) == a.subrange(0, k) && b[k] > a[k];
        if k == 1 {
            assert(b.subrange(0, 1)[0] == a.subrange(0, 1)[0]);
        }
        assert(false);
    }
    assert(i < j);
}
} // verus!
