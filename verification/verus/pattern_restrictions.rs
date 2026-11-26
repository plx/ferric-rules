//! Verus model of the source-level pattern-restriction validator that emits
//! the stable codes of `crates/ferric-rules-core/src/validation.rs`:
//! `validate_rule_patterns`, `validate_pattern_recursive` and
//! `validate_forall_operands` in `crates/ferric-rules-runtime/src/loader.rs`
//! (lines 7028-7171).
//!
//! Check with `verus --crate-type=lib verification/verus/pattern_restrictions.rs`.
//!
//! # What is proved (unbounded: every pattern tree, every depth limit)
//!
//! For a rule's conditional elements, compared against bottom-up properties
//! of the pattern trees defined below independently of the validator's
//! top-down flags (`theorem_validate_rule_patterns_exact`):
//!
//! * no E0001 `NestingTooDeep` is reported exactly when every pattern's
//!   quantifier height (the largest number of `not`/`exists`/`forall` nodes
//!   on one root-to-leaf path; `and`/`or`/`logical` and `?f <-` are
//!   transparent) is at most the limit; every reported E0001 carries `max`
//!   equal to the limit and a `depth` above it;
//! * no E0003 `NestedForall` is reported exactly when no `forall` occurs
//!   below another `forall`;
//! * no E0005 "forall inside not or exists" is reported exactly when no
//!   `forall` occurs below a `not` or an `exists`;
//! * no forall-operand diagnostic (E0005 arity, E0002 condition not a single
//!   fact pattern, E0005 then-clause) is reported exactly when every `forall`
//!   has exactly two operands, the first a fact pattern or `forall`, the
//!   second a fact pattern, a `forall`, or a test-only condition;
//! * hence the validator reports no error exactly when all four properties
//!   hold, and it never reports E0004 (`ForallUnboundVariable`): the model's
//!   error type, transcribed from every `push_*` call in the validator, has
//!   no such constructor (`lemma_codes`).
//!
//! # Abstractions
//!
//! * Pattern leaves (`Ordered`, `Template`, `Test`) keep only their kind;
//!   spans, variable names and payloads are dropped (they only feed the
//!   diagnostics' locations and texts).
//! * `is_pure_test_condition(&children[1])` (loader.rs:7176, defined by the
//!   engine's test lowering) is an input: each `Forall` node carries the
//!   boolean the production function would return for its second operand.
//! * Diagnostics are tags (`RestrictionM`) with the code of the
//!   `PatternViolation` the production pushes; texts and locations are
//!   dropped. `NestingTooDeep` keeps `depth` and `max`.
//! * `depth + usize::from(quantified)` is computed on `usize`; the model
//!   requires the starting depth plus the tree's quantifier height to fit,
//!   which holds for the top-level call (depth 0).
//!
//! # Trusted base
//!
//! Verus, Z3 and vstd. No `assume`, `admit`, `external_body`, `external` or
//! `assume_specification`.
//!
//! # Known gaps
//!
//! * Hand transcription of runtime code (no Kani harness on it: the runtime
//!   crate's proof modules belong to another work item, and recursive
//!   heap-allocated trees are intractable for Kani here, see
//!   core/src/compiler/kani_proofs.rs). The meaning of `is_pure_test_condition`
//!   is not modeled.
//! * Unbound variables in a forall then-clause are reported by a different
//!   runtime diagnostic ("unbound" test variables), not by this validator.
use vstd::prelude::*;

verus! {

/// Models `ferric_rules_parser::Pattern` (parser stage2.rs:25).
pub enum PatM {
    Ordered,
    Template,
    Test,
    And(Vec<PatM>),
    Not(Box<PatM>),
    Exists(Vec<PatM>),
    /// Children, and `is_pure_test_condition` of the second child.
    Forall(Vec<PatM>, bool),
    Logical(Vec<PatM>),
    Or(Vec<PatM>),
    Assigned(Box<PatM>),
}

/// The violations the validator pushes (loader.rs:7053-7166).
pub enum RestrictionM {
    /// E0001 (`push_nesting_depth_error`, loader.rs:7053).
    NestingTooDeep { depth: usize, max: usize },
    /// E0003 (loader.rs:7080).
    NestedForall,
    /// E0005 "forall inside not or exists" (loader.rs:7087).
    ForallInsideNotOrExists,
    /// E0005 "forall requires exactly one fact condition ..." (loader.rs:7138).
    ForallArity,
    /// E0002 (loader.rs:7152).
    ForallConditionNotSinglePattern,
    /// E0005 "forall then-clause must be ..." (loader.rs:7163).
    ForallThenClause,
}

/// The stable code of each violation (`PatternViolation::code`,
/// validation.rs:115).
pub open spec fn code(r: RestrictionM) -> nat {
    match r {
        RestrictionM::NestingTooDeep { .. } => 1,
        RestrictionM::ForallConditionNotSinglePattern => 2,
        RestrictionM::NestedForall => 3,
        _ => 5,
    }
}

// ---------------------------------------------------------------------------
// Size measure (termination)
// ---------------------------------------------------------------------------

pub open spec fn psize(p: PatM) -> nat
    decreases p, 1nat,
{
    match p {
        PatM::And(c) | PatM::Exists(c) | PatM::Logical(c) | PatM::Or(c) => 1 + psizes(c@),
        PatM::Forall(c, _) => 1 + psizes(c@),
        PatM::Not(inner) | PatM::Assigned(inner) => 1 + psize(*inner),
        _ => 1,
    }
}

pub open spec fn psizes(s: Seq<PatM>) -> nat
    decreases s, 0nat,
{
    if s.len() == 0 {
        0
    } else {
        psizes(s.drop_last()) + psize(s.last())
    }
}

proof fn lemma_psize_member(s: Seq<PatM>, i: int)
    requires
        0 <= i < s.len(),
    ensures
        psize(s[i]) <= psizes(s),
    decreases s.len(),
{
    if i < s.len() - 1 {
        lemma_psize_member(s.drop_last(), i);
        assert(s.drop_last()[i] == s[i]);
    }
}

// ---------------------------------------------------------------------------
// Bottom-up specification
// ---------------------------------------------------------------------------

pub open spec fn quantified(p: PatM) -> bool {
    p is Not || p is Exists || p is Forall
}

pub open spec fn max(a: nat, b: nat) -> nat {
    if a >= b {
        a
    } else {
        b
    }
}

/// Largest number of quantified nodes on one root-to-leaf path.
pub open spec fn qheight(p: PatM) -> nat
    decreases p, 1nat,
{
    match p {
        PatM::And(c) | PatM::Logical(c) | PatM::Or(c) => seq_qheight(c@),
        PatM::Exists(c) => 1 + seq_qheight(c@),
        PatM::Forall(c, _) => 1 + seq_qheight(c@),
        PatM::Not(inner) => 1 + qheight(*inner),
        PatM::Assigned(inner) => qheight(*inner),
        _ => 0,
    }
}

pub open spec fn seq_qheight(s: Seq<PatM>) -> nat
    decreases s, 0nat,
{
    if s.len() == 0 {
        0
    } else {
        max(seq_qheight(s.drop_last()), qheight(s.last()))
    }
}

/// Some `forall` node in the tree (including the root).
pub open spec fn contains_forall(p: PatM) -> bool
    decreases p, 1nat,
{
    match p {
        PatM::Forall(_, _) => true,
        PatM::And(c) | PatM::Exists(c) | PatM::Logical(c) | PatM::Or(c) => seq_contains_forall(c@),
        PatM::Not(inner) | PatM::Assigned(inner) => contains_forall(*inner),
        _ => false,
    }
}

pub open spec fn seq_contains_forall(s: Seq<PatM>) -> bool
    decreases s, 0nat,
{
    s.len() > 0 && (seq_contains_forall(s.drop_last()) || contains_forall(s.last()))
}

/// Some `forall` node strictly below another `forall` node.
pub open spec fn forall_in_forall(p: PatM) -> bool
    decreases p, 1nat,
{
    match p {
        PatM::Forall(c, _) => seq_contains_forall(c@) || seq_forall_in_forall(c@),
        PatM::And(c) | PatM::Exists(c) | PatM::Logical(c) | PatM::Or(c) => seq_forall_in_forall(c@),
        PatM::Not(inner) | PatM::Assigned(inner) => forall_in_forall(*inner),
        _ => false,
    }
}

pub open spec fn seq_forall_in_forall(s: Seq<PatM>) -> bool
    decreases s, 0nat,
{
    s.len() > 0 && (seq_forall_in_forall(s.drop_last()) || forall_in_forall(s.last()))
}

/// Some `forall` node strictly below a `not` or `exists` node.
pub open spec fn forall_in_negation(p: PatM) -> bool
    decreases p, 1nat,
{
    match p {
        PatM::Not(inner) => contains_forall(*inner) || forall_in_negation(*inner),
        PatM::Exists(c) => seq_contains_forall(c@) || seq_forall_in_negation(c@),
        PatM::And(c) | PatM::Logical(c) | PatM::Or(c) => seq_forall_in_negation(c@),
        PatM::Forall(c, _) => seq_forall_in_negation(c@),
        PatM::Assigned(inner) => forall_in_negation(*inner),
        _ => false,
    }
}

pub open spec fn seq_forall_in_negation(s: Seq<PatM>) -> bool
    decreases s, 0nat,
{
    s.len() > 0 && (seq_forall_in_negation(s.drop_last()) || forall_in_negation(s.last()))
}

pub open spec fn single_fact_or_forall(p: PatM) -> bool {
    p is Ordered || p is Template || p is Forall
}

/// The documented forall shape: `(forall <fact> <fact-or-test>)`.
pub open spec fn forall_operands_ok(c: Seq<PatM>, then_is_pure_test: bool) -> bool {
    c.len() == 2 && single_fact_or_forall(c[0]) && (single_fact_or_forall(c[1])
        || then_is_pure_test)
}

/// Some `forall` node whose operands violate the documented shape.
pub open spec fn bad_forall(p: PatM) -> bool
    decreases p, 1nat,
{
    match p {
        PatM::Forall(c, pure) => !forall_operands_ok(c@, pure) || seq_bad_forall(c@),
        PatM::And(c) | PatM::Exists(c) | PatM::Logical(c) | PatM::Or(c) => seq_bad_forall(c@),
        PatM::Not(inner) | PatM::Assigned(inner) => bad_forall(*inner),
        _ => false,
    }
}

pub open spec fn seq_bad_forall(s: Seq<PatM>) -> bool
    decreases s, 0nat,
{
    s.len() > 0 && (seq_bad_forall(s.drop_last()) || bad_forall(s.last()))
}

// ---------------------------------------------------------------------------
// The validator's output, transcribed
// ---------------------------------------------------------------------------

pub open spec fn operand_errors(c: Seq<PatM>, then_is_pure_test: bool) -> Seq<RestrictionM> {
    if c.len() != 2 {
        seq![RestrictionM::ForallArity]
    } else {
        (if !single_fact_or_forall(c[0]) {
            seq![RestrictionM::ForallConditionNotSinglePattern]
        } else {
            seq![]
        }) + (if !single_fact_or_forall(c[1]) && !then_is_pure_test {
            seq![RestrictionM::ForallThenClause]
        } else {
            seq![]
        })
    }
}

pub open spec fn rec_errors(p: PatM, depth: nat, max_depth: usize, f: bool, ne: bool) -> Seq<RestrictionM>
    decreases p, 1nat,
{
    let child_depth = depth + if quantified(p) {
        1nat
    } else {
        0nat
    };
    let head = if quantified(p) && child_depth > max_depth {
        seq![RestrictionM::NestingTooDeep { depth: child_depth as usize, max: max_depth }]
    } else {
        seq![]
    };
    head + match p {
        PatM::Not(inner) => rec_errors(*inner, child_depth, max_depth, f, true),
        PatM::Exists(c) => seq_errors(c@, child_depth, max_depth, f, true),
        PatM::Forall(c, pure) => (if f {
            seq![RestrictionM::NestedForall]
        } else {
            seq![]
        }) + (if ne {
            seq![RestrictionM::ForallInsideNotOrExists]
        } else {
            seq![]
        }) + operand_errors(c@, pure) + seq_errors(c@, child_depth, max_depth, true, ne),
        PatM::Assigned(inner) => rec_errors(*inner, depth, max_depth, f, ne),
        PatM::And(c) | PatM::Logical(c) | PatM::Or(c) => seq_errors(c@, depth, max_depth, f, ne),
        _ => seq![],
    }
}

pub open spec fn seq_errors(s: Seq<PatM>, depth: nat, max_depth: usize, f: bool, ne: bool) -> Seq<
    RestrictionM,
>
    decreases s, 0nat,
{
    if s.len() == 0 {
        seq![]
    } else {
        seq_errors(s.drop_last(), depth, max_depth, f, ne) + rec_errors(
            s.last(),
            depth,
            max_depth,
            f,
            ne,
        )
    }
}

// ---------------------------------------------------------------------------
// Exec model
// ---------------------------------------------------------------------------

fn is_single_fact_or_forall(p: &PatM) -> (r: bool)
    ensures
        r == single_fact_or_forall(*p),
{
    match p {
        PatM::Ordered | PatM::Template | PatM::Forall(_, _) => true,
        _ => false,
    }
}

/// Models `validate_forall_operands` (loader.rs:7132).
fn validate_forall_operands(children: &Vec<PatM>, then_is_pure_test: bool, errors: &mut Vec<RestrictionM>)
    ensures
        final(errors)@ == old(errors)@ + operand_errors(children@, then_is_pure_test),
{
    if children.len() != 2 {
        errors.push(RestrictionM::ForallArity);
        return;
    }
    let ghost base = errors@;
    if !is_single_fact_or_forall(&children[0]) {
        errors.push(RestrictionM::ForallConditionNotSinglePattern);
    }
    if !is_single_fact_or_forall(&children[1]) && !then_is_pure_test {
        errors.push(RestrictionM::ForallThenClause);
    }
    proof {
        assert(errors@ =~= base + operand_errors(children@, then_is_pure_test));
    }
}

/// The child loop shared by every list-carrying arm of
/// `validate_pattern_recursive` (`for child in children { .. }`).
fn validate_children(
    children: &Vec<PatM>,
    depth: usize,
    max_depth: usize,
    inside_forall: bool,
    inside_not_or_exists: bool,
    errors: &mut Vec<RestrictionM>,
)
    requires
        depth + seq_qheight(children@) <= usize::MAX,
    ensures
        final(errors)@ == old(errors)@ + seq_errors(
            children@,
            depth as nat,
            max_depth,
            inside_forall,
            inside_not_or_exists,
        ),
    decreases psizes(children@), 2nat,
{
    let ghost base = errors@;
    let ghost s = children@;
    let mut i: usize = 0;
    while i < children.len()
        invariant
            s == children@,
            i <= s.len(),
            depth + seq_qheight(s) <= usize::MAX,
            errors@ == base + seq_errors(
                s.take(i as int),
                depth as nat,
                max_depth,
                inside_forall,
                inside_not_or_exists,
            ),
        decreases s.len() - i,
    {
        proof {
            assert(s.take(i + 1).drop_last() =~= s.take(i as int));
            assert(s.take(i + 1).last() == s[i as int]);
            lemma_psize_member(s, i as int);
            lemma_seq_qheight_member(s, i as int);
        }
        let ghost before = errors@;
        validate_pattern_recursive(
            &children[i],
            depth,
            max_depth,
            inside_forall,
            inside_not_or_exists,
            errors,
        );
        i += 1;
        proof {
            assert(errors@ =~= base + seq_errors(
                s.take(i as int),
                depth as nat,
                max_depth,
                inside_forall,
                inside_not_or_exists,
            ));
        }
    }
    proof {
        assert(s.take(s.len() as int) =~= s);
    }
}

proof fn lemma_seq_qheight_member(s: Seq<PatM>, i: int)
    requires
        0 <= i < s.len(),
    ensures
        qheight(s[i]) <= seq_qheight(s),
    decreases s.len(),
{
    if i < s.len() - 1 {
        lemma_seq_qheight_member(s.drop_last(), i);
        assert(s.drop_last()[i] == s[i]);
    }
}

/// Models `validate_pattern_recursive` (loader.rs:7039). The raised solver
/// budget only affects proof search.
#[verifier::rlimit(200)]
fn validate_pattern_recursive(
    pattern: &PatM,
    depth: usize,
    max_depth: usize,
    inside_forall: bool,
    inside_not_or_exists: bool,
    errors: &mut Vec<RestrictionM>,
)
    requires
        depth + qheight(*pattern) <= usize::MAX,
    ensures
        final(errors)@ == old(errors)@ + rec_errors(
            *pattern,
            depth as nat,
            max_depth,
            inside_forall,
            inside_not_or_exists,
        ),
    decreases psize(*pattern), 1nat,
{
    let ghost base = errors@;
    let quantified = match pattern {
        PatM::Not(_) | PatM::Exists(_) | PatM::Forall(_, _) => true,
        _ => false,
    };
    let child_depth = depth + if quantified {
        1usize
    } else {
        0usize
    };
    if quantified && child_depth > max_depth {
        // push_nesting_depth_error(errors, span, child_depth, max_depth, ..)
        errors.push(RestrictionM::NestingTooDeep { depth: child_depth, max: max_depth });
    }
    let ghost after_head = errors@;
    let ghost cd = child_depth as nat;
    let ghost head = if quantified && child_depth > max_depth {
        seq![RestrictionM::NestingTooDeep { depth: child_depth, max: max_depth }]
    } else {
        Seq::<RestrictionM>::empty()
    };
    proof {
        assert(after_head =~= base + head);
        assert(quantified == crate::quantified(*pattern));
    }
    match pattern {
        PatM::Not(inner) => {
            validate_pattern_recursive(inner, child_depth, max_depth, inside_forall, true, errors);
            proof {
                assert(rec_errors(*pattern, depth as nat, max_depth, inside_forall, inside_not_or_exists)
                    == head + rec_errors(**inner, cd, max_depth, inside_forall, true));
                assert(errors@ =~= base + rec_errors(*pattern, depth as nat, max_depth, inside_forall, inside_not_or_exists));
            }
        },
        PatM::Exists(children) => {
            validate_children(children, child_depth, max_depth, inside_forall, true, errors);
            proof {
                assert(rec_errors(*pattern, depth as nat, max_depth, inside_forall, inside_not_or_exists)
                    == head + seq_errors(children@, cd, max_depth, inside_forall, true));
                assert(errors@ =~= base + rec_errors(*pattern, depth as nat, max_depth, inside_forall, inside_not_or_exists));
            }
        },
        PatM::Forall(children, then_is_pure_test) => {
            if inside_forall {
                errors.push(RestrictionM::NestedForall);
            }
            if inside_not_or_exists {
                errors.push(RestrictionM::ForallInsideNotOrExists);
            }
            let ghost flags = errors@;
            validate_forall_operands(children, *then_is_pure_test, errors);
            validate_children(children, child_depth, max_depth, true, inside_not_or_exists, errors);
            proof {
                let nf: Seq<RestrictionM> = if inside_forall {
                    seq![RestrictionM::NestedForall]
                } else {
                    seq![]
                };
                let ni: Seq<RestrictionM> = if inside_not_or_exists {
                    seq![RestrictionM::ForallInsideNotOrExists]
                } else {
                    seq![]
                };
                assert(flags =~= after_head + nf + ni);
                assert(rec_errors(*pattern, depth as nat, max_depth, inside_forall, inside_not_or_exists)
                    == head + (nf + ni + operand_errors(children@, *then_is_pure_test) + seq_errors(
                    children@,
                    cd,
                    max_depth,
                    true,
                    inside_not_or_exists,
                )));
                assert(errors@ =~= base + rec_errors(*pattern, depth as nat, max_depth, inside_forall, inside_not_or_exists));
            }
        },
        PatM::Assigned(inner) => {
            validate_pattern_recursive(
                inner,
                depth,
                max_depth,
                inside_forall,
                inside_not_or_exists,
                errors,
            );
            proof {
                assert(errors@ =~= base + rec_errors(*pattern, depth as nat, max_depth, inside_forall, inside_not_or_exists));
            }
        },
        PatM::And(children) | PatM::Logical(children) | PatM::Or(children) => {
            validate_children(
                children,
                depth,
                max_depth,
                inside_forall,
                inside_not_or_exists,
                errors,
            );
            proof {
                assert(rec_errors(*pattern, depth as nat, max_depth, inside_forall, inside_not_or_exists)
                    == head + seq_errors(children@, depth as nat, max_depth, inside_forall, inside_not_or_exists));
                assert(errors@ =~= base + rec_errors(*pattern, depth as nat, max_depth, inside_forall, inside_not_or_exists));
            }
        },
        PatM::Ordered | PatM::Template | PatM::Test => {
            proof {
                assert(errors@ =~= base + rec_errors(*pattern, depth as nat, max_depth, inside_forall, inside_not_or_exists));
            }
        },
    }
}

pub open spec fn rule_errors(patterns: Seq<PatM>, max_depth: usize) -> Seq<RestrictionM> {
    seq_errors(patterns, 0, max_depth, false, false)
}

/// Models `validate_rule_patterns` (loader.rs:7028); the loader calls it
/// with `max_nesting_depth = 4` (loader.rs:2456).
fn validate_rule_patterns(patterns: &Vec<PatM>, max_nesting_depth: usize) -> (errors: Vec<
    RestrictionM,
>)
    requires
        seq_qheight(patterns@) <= usize::MAX,
    ensures
        errors@ == rule_errors(patterns@, max_nesting_depth),
{
    let mut errors: Vec<RestrictionM> = Vec::new();
    validate_children(patterns, 0, max_nesting_depth, false, false, &mut errors);
    proof {
        assert(errors@ =~= rule_errors(patterns@, max_nesting_depth));
    }
    errors
}

// ---------------------------------------------------------------------------
// Characterization
// ---------------------------------------------------------------------------

pub open spec fn none_depth(s: Seq<RestrictionM>) -> bool {
    forall|i: int| 0 <= i < s.len() ==> !(#[trigger] s[i] is NestingTooDeep)
}

pub open spec fn none_nested(s: Seq<RestrictionM>) -> bool {
    forall|i: int| 0 <= i < s.len() ==> !(#[trigger] s[i] is NestedForall)
}

pub open spec fn none_inside(s: Seq<RestrictionM>) -> bool {
    forall|i: int| 0 <= i < s.len() ==> !(#[trigger] s[i] is ForallInsideNotOrExists)
}

pub open spec fn is_operand_error(r: RestrictionM) -> bool {
    r is ForallArity || r is ForallConditionNotSinglePattern || r is ForallThenClause
}

pub open spec fn none_operand(s: Seq<RestrictionM>) -> bool {
    forall|i: int| 0 <= i < s.len() ==> !is_operand_error(#[trigger] s[i])
}

/// Every E0001 carries the configured limit and a depth above it.
pub open spec fn depth_errors_sound(s: Seq<RestrictionM>, max_depth: usize) -> bool {
    forall|i: int|
        0 <= i < s.len() && (#[trigger] s[i]) is NestingTooDeep ==> s[i]->NestingTooDeep_max == max_depth
            && s[i]->NestingTooDeep_depth > max_depth
}

proof fn lemma_concat(a: Seq<RestrictionM>, b: Seq<RestrictionM>, max_depth: usize)
    ensures
        none_depth(a + b) == (none_depth(a) && none_depth(b)),
        none_nested(a + b) == (none_nested(a) && none_nested(b)),
        none_inside(a + b) == (none_inside(a) && none_inside(b)),
        none_operand(a + b) == (none_operand(a) && none_operand(b)),
        depth_errors_sound(a + b, max_depth) == (depth_errors_sound(a, max_depth)
            && depth_errors_sound(b, max_depth)),
{
    let c = a + b;
    // Every element of `c` is an element of `a` or of `b`, and conversely.
    assert forall|i: int| 0 <= i < a.len() implies #[trigger] a[i] == c[i] by {}
    assert forall|i: int| 0 <= i < b.len() implies #[trigger] b[i] == c[a.len() + i] by {}
    assert forall|i: int| 0 <= i < c.len() implies #[trigger] c[i] == if i < a.len() {
        a[i]
    } else {
        b[i - a.len()]
    } by {}
}

proof fn lemma_operand_errors(c: Seq<PatM>, pure: bool, max_depth: usize)
    ensures
        none_depth(operand_errors(c, pure)),
        none_nested(operand_errors(c, pure)),
        none_inside(operand_errors(c, pure)),
        depth_errors_sound(operand_errors(c, pure), max_depth),
        none_operand(operand_errors(c, pure)) == forall_operands_ok(c, pure),
{
    let e = operand_errors(c, pure);
    if c.len() != 2 {
        assert(is_operand_error(e[0]));
    } else {
        let a: Seq<RestrictionM> = if !single_fact_or_forall(c[0]) {
            seq![RestrictionM::ForallConditionNotSinglePattern]
        } else {
            seq![]
        };
        let b: Seq<RestrictionM> = if !single_fact_or_forall(c[1]) && !pure {
            seq![RestrictionM::ForallThenClause]
        } else {
            seq![]
        };
        lemma_concat(a, b, max_depth);
        if !single_fact_or_forall(c[0]) {
            assert(is_operand_error(a[0]));
        }
        if !single_fact_or_forall(c[1]) && !pure {
            assert(is_operand_error(b[0]));
        }
    }
}

pub open spec fn single(r: RestrictionM) -> Seq<RestrictionM> {
    seq![r]
}

/// Theorem body (tree case): the four "no diagnostic of this kind" facts
/// are exactly the bottom-up properties, generalized over the validator's
/// top-down state (`depth`, inside-forall, inside-not-or-exists).
proof fn lemma_tree(p: PatM, depth: nat, max_depth: usize, f: bool, ne: bool)
    requires
        depth + qheight(p) <= usize::MAX,
    ensures
        none_depth(rec_errors(p, depth, max_depth, f, ne)) == (qheight(p) == 0 || depth + qheight(p)
            <= max_depth),
        none_nested(rec_errors(p, depth, max_depth, f, ne)) == (!(f && contains_forall(p))
            && !forall_in_forall(p)),
        none_inside(rec_errors(p, depth, max_depth, f, ne)) == (!(ne && contains_forall(p))
            && !forall_in_negation(p)),
        none_operand(rec_errors(p, depth, max_depth, f, ne)) == !bad_forall(p),
        depth_errors_sound(rec_errors(p, depth, max_depth, f, ne), max_depth),
    decreases p, 1nat,
{
    let q = quantified(p);
    let cd = depth + if q {
        1nat
    } else {
        0nat
    };
    let head: Seq<RestrictionM> = if q && cd > max_depth {
        seq![RestrictionM::NestingTooDeep { depth: cd as usize, max: max_depth }]
    } else {
        seq![]
    };
    let e = rec_errors(p, depth, max_depth, f, ne);
    match p {
        PatM::Not(inner) => {
            let rest = rec_errors(*inner, cd, max_depth, f, true);
            lemma_tree(*inner, cd, max_depth, f, true);
            lemma_concat(head, rest, max_depth);
            assert(e == head + rest);
            if cd > max_depth {
                assert(head[0] is NestingTooDeep);
            }
            lemma_head_sound(cd, max_depth, head);
        },
        PatM::Exists(c) => {
            let rest = seq_errors(c@, cd, max_depth, f, true);
            lemma_seq(c@, cd, max_depth, f, true);
            lemma_concat(head, rest, max_depth);
            assert(e == head + rest);
            if cd > max_depth {
                assert(head[0] is NestingTooDeep);
            }
            lemma_head_sound(cd, max_depth, head);
        },
        PatM::Forall(c, pure) => {
            let nf: Seq<RestrictionM> = if f {
                seq![RestrictionM::NestedForall]
            } else {
                seq![]
            };
            let ni: Seq<RestrictionM> = if ne {
                seq![RestrictionM::ForallInsideNotOrExists]
            } else {
                seq![]
            };
            let ops = operand_errors(c@, pure);
            let rest = seq_errors(c@, cd, max_depth, true, ne);
            lemma_seq(c@, cd, max_depth, true, ne);
            lemma_operand_errors(c@, pure, max_depth);
            lemma_concat(nf, ni, max_depth);
            lemma_concat(nf + ni, ops, max_depth);
            lemma_concat(nf + ni + ops, rest, max_depth);
            lemma_concat(head, nf + ni + ops + rest, max_depth);
            assert(e == head + (nf + ni + ops + rest));
            if cd > max_depth {
                assert(head[0] is NestingTooDeep);
            }
            if f {
                assert(nf[0] is NestedForall);
            }
            if ne {
                assert(ni[0] is ForallInsideNotOrExists);
            }
            lemma_head_sound(cd, max_depth, head);
        },
        PatM::Assigned(inner) => {
            lemma_tree(*inner, depth, max_depth, f, ne);
            assert(e =~= rec_errors(*inner, depth, max_depth, f, ne));
        },
        PatM::And(c) | PatM::Logical(c) | PatM::Or(c) => {
            lemma_seq(c@, depth, max_depth, f, ne);
            assert(e =~= seq_errors(c@, depth, max_depth, f, ne));
        },
        _ => {
            assert(e =~= seq![]);
        },
    }
}

proof fn lemma_head_sound(cd: nat, max_depth: usize, head: Seq<RestrictionM>)
    requires
        cd <= usize::MAX,
        head.len() == 0 || (head.len() == 1 && head[0] == RestrictionM::NestingTooDeep {
            depth: cd as usize,
            max: max_depth,
        } && cd > max_depth),
    ensures
        depth_errors_sound(head, max_depth),
{
}

/// Theorem body (children case).
proof fn lemma_seq(s: Seq<PatM>, depth: nat, max_depth: usize, f: bool, ne: bool)
    requires
        depth + seq_qheight(s) <= usize::MAX,
    ensures
        none_depth(seq_errors(s, depth, max_depth, f, ne)) == (seq_qheight(s) == 0 || depth
            + seq_qheight(s) <= max_depth),
        none_nested(seq_errors(s, depth, max_depth, f, ne)) == (!(f && seq_contains_forall(s))
            && !seq_forall_in_forall(s)),
        none_inside(seq_errors(s, depth, max_depth, f, ne)) == (!(ne && seq_contains_forall(s))
            && !seq_forall_in_negation(s)),
        none_operand(seq_errors(s, depth, max_depth, f, ne)) == !seq_bad_forall(s),
        depth_errors_sound(seq_errors(s, depth, max_depth, f, ne), max_depth),
    decreases s, 0nat,
{
    if s.len() > 0 {
        lemma_seq_qheight_member(s, s.len() - 1);
        lemma_seq(s.drop_last(), depth, max_depth, f, ne);
        lemma_tree(s.last(), depth, max_depth, f, ne);
        lemma_concat(
            seq_errors(s.drop_last(), depth, max_depth, f, ne),
            rec_errors(s.last(), depth, max_depth, f, ne),
            max_depth,
        );
    }
}

/// Theorem (the validator reports exactly the documented violations): for
/// the loader's call (`depth` 0, outside any quantifier), each kind of
/// diagnostic is absent exactly when the corresponding bottom-up property
/// holds, so the error list is empty exactly when all of them hold; every
/// E0001 names the configured limit and a depth above it.
proof fn theorem_validate_rule_patterns_exact(patterns: Seq<PatM>, max_depth: usize)
    requires
        seq_qheight(patterns) <= usize::MAX,
    ensures
        none_depth(rule_errors(patterns, max_depth)) == (seq_qheight(patterns) <= max_depth),
        none_nested(rule_errors(patterns, max_depth)) == !seq_forall_in_forall(patterns),
        none_inside(rule_errors(patterns, max_depth)) == !seq_forall_in_negation(patterns),
        none_operand(rule_errors(patterns, max_depth)) == !seq_bad_forall(patterns),
        depth_errors_sound(rule_errors(patterns, max_depth), max_depth),
        (rule_errors(patterns, max_depth).len() == 0) == (seq_qheight(patterns) <= max_depth && !seq_forall_in_forall(patterns)
            && !seq_forall_in_negation(patterns) && !seq_bad_forall(patterns)),
{
    lemma_seq(patterns, 0, max_depth, false, false);
    let e = rule_errors(patterns, max_depth);
    if e.len() > 0 {
        let r = e[0];
        assert(r is NestingTooDeep || r is NestedForall || r is ForallInsideNotOrExists
            || is_operand_error(r));
    }
}

/// The codes the validator can report: E0001, E0002, E0003, E0005 — never
/// E0004.
proof fn lemma_codes(r: RestrictionM)
    ensures
        code(r) == 1 || code(r) == 2 || code(r) == 3 || code(r) == 5,
        code(r) != 4,
{
}

}
