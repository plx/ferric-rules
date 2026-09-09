use super::*;
use crate::formatting::{
    append_bytes, append_directive, scan_format, ByteArgumentError, DirectiveError, FormatArgument,
    FormatPiece, IntegerArgumentError, NumericParameter,
};

const ALLOWED: usize = 4096;

fn directive(source: &[u8]) -> Directive<'_> {
    let plan = scan_format(source).unwrap();
    assert_eq!(plan.conversion_count, 1);
    assert_eq!(plan.pieces.len(), 1);
    let FormatPiece::Directive(directive) = plan.pieces.into_iter().next().unwrap() else {
        panic!("one directive expected")
    };
    directive
}

fn fragment(source: &[u8]) -> Vec<u8> {
    let mut output = ByteBuffer::new();
    append_noncanonical(&mut output, &directive(source), ALLOWED).unwrap();
    output.as_bytes().to_vec()
}

// This test adapter is the proposed opt-in integration policy: only an already
// admitted NonCanonical result reaches fallback. Other failures propagate.
fn admitted_append(
    output: &mut ByteBuffer,
    directive: &Directive<'_>,
    argument: FormatArgument<'_>,
    allowed: usize,
) -> Result<(), DirectiveError> {
    match append_directive(output, directive, argument, allowed) {
        Err(DirectiveError::NonCanonical { .. }) => append_noncanonical(output, directive, allowed)
            .map_err(|error| match error {
                NonCanonicalError::OutputLimit(limit) => DirectiveError::OutputLimit(limit),
                other => panic!("scanner-generated noncanonical descriptor: {other:?}"),
            }),
        result => result,
    }
}

#[test]
fn five_observed_echoes_are_derived_and_whole_sealed_program_matches() {
    let mut output = ByteBuffer::new();
    for (label, source) in [
        (b'a', b"%--5d".as_slice()),
        (b'b', b"%0-05d"),
        (b'c', b"%5-3d"),
        (b'd', b"%.2.3d"),
        (b'e', b"%..d"),
        (b'f', b"%.-3d"),
        (b'g', b"%1.2-3d"),
    ] {
        append_bytes(&mut output, &[label, b':', b'['], ALLOWED).unwrap();
        admitted_append(
            &mut output,
            &directive(source),
            FormatArgument::Integer(7),
            ALLOWED,
        )
        .unwrap();
        append_bytes(&mut output, b"]\n", ALLOWED).unwrap();
    }
    assert_eq!(
        output.as_bytes(),
        include_bytes!("fixtures/small-repeated-misordered-modifiers.expected.out")
    );
    // The standalone policy code has no cases keyed by the five control strings.
    assert_eq!(fragment(b"%..d"), b"%.0.lld");
}

#[test]
fn unmeasured_prefix_folding_is_explicit_engine_policy() {
    // Deterministic Ferric policy, not new CLIPS/glibc expectations.
    for (source, expected) in [
        (b"%0005.002.003d".as_slice(), b"%05.2.003lld".as_slice()),
        (b"%--00.000-03f", b"%-.0-03f"),
        (b"%---0..s", b"%-.0.s"),
        (b"%4-000.05x", b"%4-000.05llx"),
        (b"%.000000.0003g", b"%.0.0003g"),
        (b"%000000-..c", b"%-.0.c"),
    ] {
        assert_eq!(fragment(source), expected);
    }
}

#[test]
fn integer_ll_and_every_other_conversion_preserve_the_unparsed_tail() {
    // Scope extension policy: identical spelling rules for all admitted kinds.
    for conversion in b"doxucs efg".iter().copied().filter(|&byte| byte != b' ') {
        let source = [b'%', b'.', b'.', conversion];
        let mut expected = b"%.0.".to_vec();
        if b"doxu".contains(&conversion) {
            expected.extend_from_slice(b"ll");
        }
        expected.push(conversion);
        assert_eq!(fragment(&source), expected);
    }
}

#[test]
fn huge_decimal_prefix_is_lexical_and_canonical_overflow_stays_separate() {
    let digits = "9".repeat(65);
    let source = format!("%{digits}.000-02d");
    let expected = format!("%{digits}.0-02lld");
    assert_eq!(fragment(source.as_bytes()), expected.as_bytes());
    let source = format!("%{}9d", usize::MAX);
    let directive = directive(source.as_bytes());
    assert!(matches!(
        directive.spec,
        SpecAnalysis::ParameterOverflow {
            parameter: NumericParameter::Width,
            ..
        }
    ));
    let mut output = ByteBuffer::new();
    output.push_bytes(b"prefix");
    assert_eq!(
        append_noncanonical(&mut output, &directive, ALLOWED),
        Err(NonCanonicalError::NotNonCanonical)
    );
    assert_eq!(output.as_bytes(), b"prefix");
}

#[test]
fn admission_failures_do_not_become_echoes_or_skip_conversion_type_rules() {
    let mut output = ByteBuffer::new();
    output.push_bytes(b"prefix");
    assert_eq!(
        admitted_append(
            &mut output,
            &directive(b"%..d"),
            FormatArgument::String(b"bad"),
            ALLOWED
        ),
        Err(DirectiveError::IntegerArgument(
            IntegerArgumentError::NotNumeric("STRING")
        ))
    );
    assert_eq!(
        admitted_append(
            &mut output,
            &directive(b"%..c"),
            FormatArgument::InstanceName(b"name"),
            ALLOWED
        ),
        Err(DirectiveError::ByteArgument(
            ByteArgumentError::ExpectedCharacter("INSTANCE-NAME")
        ))
    );
    assert_eq!(
        admitted_append(
            &mut output,
            &directive(b"%..d"),
            FormatArgument::Float(f64::INFINITY),
            ALLOWED
        ),
        Err(DirectiveError::IntegerArgument(
            IntegerArgumentError::NonFinite
        ))
    );
    assert_eq!(output.as_bytes(), b"prefix");
    admitted_append(
        &mut output,
        &directive(b"%..s"),
        FormatArgument::InstanceName(b"name"),
        ALLOWED,
    )
    .unwrap();
    assert_eq!(output.as_bytes(), b"prefix%.0.s");
}

#[test]
fn scanner_count_and_invalid_flag_fences_precede_fallback() {
    assert!(scan_format(b"%..d:%q").is_err());
    let plan = scan_format(b"%..d:%s").unwrap();
    assert_eq!(plan.conversion_count, 2);
    assert!(plan.check_operand_count(1).is_err());
    assert!(plan.check_operand_count(3).is_err());
    let bounded = format!("%{}d", "0".repeat(74));
    assert_eq!(scan_format(bounded.as_bytes()).unwrap().conversion_count, 0);
    assert_eq!(scan_format(b"%..d\0%q").unwrap().conversion_count, 1);
}

#[test]
fn output_limit_is_aggregate_atomic_and_does_not_expand_width() {
    let wide_directive = directive(b"%99999999999999999999-3d");
    let expected = b"%99999999999999999999-3lld";
    let mut output = ByteBuffer::new();
    output.push_bytes(b"prefix");
    let allowed = 6 + expected.len() - 1;
    assert_eq!(
        append_noncanonical(&mut output, &wide_directive, allowed),
        Err(NonCanonicalError::OutputLimit(OutputLimit {
            required: Some(6 + expected.len()),
            allowed
        }))
    );
    assert_eq!(output.as_bytes(), b"prefix");
    append_noncanonical(&mut output, &wide_directive, allowed + 1).unwrap();
    assert_eq!(&output.as_bytes()[6..], expected);
    // Empty precision adds one byte, which must be included in the size check.
    let mut output = ByteBuffer::new();
    assert_eq!(
        append_noncanonical(&mut output, &directive(b"%..d"), 6),
        Err(NonCanonicalError::OutputLimit(OutputLimit {
            required: Some(7),
            allowed: 6
        }))
    );
    assert!(output.as_bytes().is_empty());
}

#[test]
fn descriptor_guards_and_raw_adjacent_bytes_preserve_integrity() {
    let mut output = ByteBuffer::new();
    output.push_bytes(b"\xff:");
    let mut invalid = directive(b"%..d");
    invalid.range.start = usize::MAX;
    assert_eq!(
        append_noncanonical(&mut output, &invalid, ALLOWED),
        Err(NonCanonicalError::InvalidDescriptor)
    );
    assert_eq!(output.as_bytes(), b"\xff:");
    assert_eq!(
        append_noncanonical(&mut output, &directive(b"%04d"), ALLOWED),
        Err(NonCanonicalError::NotNonCanonical)
    );
    append_noncanonical(&mut output, &directive(b"%..d"), ALLOWED).unwrap();
    append_bytes(&mut output, b":\xc3", ALLOWED).unwrap();
    assert_eq!(output.as_bytes(), b"\xff:%.0.lld:\xc3");
}
