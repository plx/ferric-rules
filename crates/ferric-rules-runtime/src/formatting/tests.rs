use super::*;
use crate::byte_buffer::ByteBuffer;

const ALLOWED: usize = 8192;

#[derive(Debug, PartialEq, Eq)]
enum AssemblyError<'a> {
    Invalid(InvalidFlag<'a>),
    Count(OperandCountMismatch),
    Directive(DirectiveError),
    Limit(OutputLimit),
}

// Pure test orchestration of the scan/admission sequence. `visits` records
// primitive argument consumption, not runtime expression evaluation/effects.
fn assemble<'a>(
    control: &'a [u8],
    values: &[FormatArgument<'_>],
    allowed: usize,
    visits: &mut Vec<usize>,
) -> Result<Vec<u8>, AssemblyError<'a>> {
    let plan: FormatPlan<'_> = scan_format(control).map_err(AssemblyError::Invalid)?;
    plan.check_operand_count(values.len())
        .map_err(AssemblyError::Count)?;
    let mut output = ByteBuffer::new();
    let mut index = 0;
    for piece in &plan.pieces {
        match piece {
            FormatPiece::Literal { bytes, .. } => {
                append_bytes(&mut output, bytes, allowed).map_err(AssemblyError::Limit)?;
            }
            FormatPiece::Control { byte, .. } => {
                append_bytes(&mut output, &[*byte], allowed).map_err(AssemblyError::Limit)?;
            }
            FormatPiece::Directive(directive) => {
                visits.push(index);
                append_directive(&mut output, directive, values[index], allowed)
                    .map_err(AssemblyError::Directive)?;
                index += 1;
            }
        }
    }
    Ok(output.as_bytes().to_vec())
}

fn rendered(control: &[u8], values: &[FormatArgument<'_>]) -> Vec<u8> {
    let mut visits = Vec::new();
    let bytes = assemble(control, values, ALLOWED, &mut visits).unwrap();
    assert_eq!(visits, (0..values.len()).collect::<Vec<_>>());
    bytes
}

fn line(output: &mut Vec<u8>, label: &[u8], control: &[u8], values: &[FormatArgument<'_>]) {
    output.extend_from_slice(label);
    output.push(b'[');
    output.extend(rendered(control, values));
    output.extend_from_slice(b"]\n");
}

#[test]
fn parser_to_renderer_matches_sealed_ordinary_and_raw_unicode_programs() {
    use FormatArgument::{Float as F, Integer as I, String as S, Symbol};
    let mut output = Vec::new();
    line(
        &mut output,
        b"",
        b"%6d|%-6d|%6.2f|%6s|%-6s|%.3s|%06s",
        &[
            I(7),
            I(7),
            F(2.5),
            S(b"red"),
            Symbol(b"red"),
            S(b"abcdef"),
            Symbol(b"red"),
        ],
    );
    line(&mut output, b"", b"%%:%n:%r:%t:%v", &[]);
    assert_eq!(
        output,
        include_bytes!("fixtures/ordinary-directives.expected.out")
    );
    output.clear();
    line(&mut output, b"", b"%4s|%.2s|%.1s", &[S("é".as_bytes()); 3]);
    assert_eq!(
        output,
        include_bytes!("fixtures/unicode-width-and-precision.expected.out")
    );
    // Existing raw-byte host contract, not a new CLIPS invocation.
    assert_eq!(
        rendered(
            b"\xff:%.1s:%s",
            &[S(b"\xc3\xa9"), FormatArgument::InstanceName(b"n\xff")]
        ),
        b"\xff:\xc3:n\xff"
    );
}

#[test]
fn parser_to_renderer_matches_sealed_integer_flags_precision_extrema_and_radices() {
    use FormatArgument::{Float as F, Integer as I};
    let mut output = Vec::new();
    for control in [b"%-05d".as_slice(), b"%0-5d", b"%0005d"] {
        for value in [7, -7] {
            line(&mut output, b"", control, &[I(value)]);
        }
    }
    assert_eq!(
        output,
        include_bytes!("fixtures/integer-flags.expected.out")
    );
    output.clear();
    for control in [
        b"%.3d".as_slice(),
        b"%08.3d",
        b"%8.3d",
        b"%-08.3d",
        b"%.0d",
        b"%05.0d",
        b"%.d",
    ] {
        for value in [7, -7, 0] {
            line(&mut output, b"", control, &[I(value)]);
        }
    }
    assert_eq!(
        output,
        include_bytes!("fixtures/integer-precision.expected.out")
    );
    output.clear();
    line(
        &mut output,
        b"",
        b"%022d|%022d|%04d|%04d",
        &[I(i64::MAX), I(i64::MIN), F(7.9), F(-7.9)],
    );
    assert_eq!(
        output,
        include_bytes!("fixtures/integer-boundaries.expected.out")
    );
    output.clear();
    for (control, value) in [
        (b"%08o|%08x|%08u".as_slice(), 65),
        (b"%08.3o|%08.3x|%08.3u", 65),
        (b"%o|%x|%u", -1),
    ] {
        line(&mut output, b"", control, &[I(value); 3]);
    }
    assert_eq!(
        output,
        include_bytes!("fixtures/radix-directives.expected.out")
    );
}

#[test]
fn parsed_character_precision_nul_and_high_bytes_match_sealed_program() {
    use FormatArgument::{Integer as I, String as S};
    let mut output = Vec::new();
    line(
        &mut output,
        b"precision:",
        b"%.0c|%.3c|%4.0c|%-4.3c",
        &[I(65); 4],
    );
    line(&mut output, b"empty:", b"%c|%4c|%-4c:end", &[S(b""); 3]);
    line(
        &mut output,
        b"zero:",
        b"%c|%4c|%-4c:end",
        &[I(0), I(256), I(0)],
    );
    line(
        &mut output,
        b"bytes:",
        b"%c|%3c|%c|%c",
        &[S("é".as_bytes()), S("é".as_bytes()), I(-1), I(255)],
    );
    assert_eq!(
        output,
        include_bytes!("fixtures/character-empty-nul-byte-width-precision.expected.out")
    );
}

#[test]
fn parsed_float_rounding_exponents_and_nonfinite_padding_match_sealed_programs() {
    use FormatArgument::Float as F;
    let mut output = Vec::new();
    line(
        &mut output,
        b"g0:",
        b"%.0g|%.0g|%.0g",
        &[F(1.0), F(9.9), F(-0.0)],
    );
    line(
        &mut output,
        b"g3:",
        b"%.3g|%.3g|%.3g|%.3g",
        &[F(999.4), F(999.5), F(0.000_099_94), F(0.000_099_95)],
    );
    line(
        &mut output,
        b"default:",
        b"%g|%g|%g|%g",
        &[F(999_999.4), F(999_999.5), F(0.0001), F(0.00001)],
    );
    line(
        &mut output,
        b"subnormal:",
        b"%.6g|%g",
        &[F(5e-324), F(f64::MAX)],
    );
    assert_eq!(
        output,
        include_bytes!("fixtures/general-precision-and-cutovers.expected.out")
    );
    output.clear();
    line(
        &mut output,
        b"e:",
        b"%e|%.0e|%.2e|%.2e|%.2e",
        &[F(12345.0), F(9.9), F(1e-5), F(1e99), F(1e100)],
    );
    line(&mut output, b"zero:", b"%010.2e|%08.2f", &[F(-0.0); 2]);
    line(
        &mut output,
        b"ties:",
        b"%.0f|%.0f|%.0f|%.0f",
        &[F(2.5), F(3.5), F(-2.5), F(-3.5)],
    );
    line(&mut output, b"subnormal:", b"%.6e", &[F(5e-324)]);
    assert_eq!(
        output,
        include_bytes!("fixtures/scientific-exponent-and-fixed-ties.expected.out")
    );
    output.clear();
    line(
        &mut output,
        b"inf:",
        b"%f|%08f|%-08f|%08e|%08g",
        &[F(f64::INFINITY); 5],
    );
    line(
        &mut output,
        b"negative:",
        b"%08f|%08e|%08g",
        &[F(f64::NEG_INFINITY); 3],
    );
    line(
        &mut output,
        b"nan:",
        b"%f|%08f|%-08f|%08e|%08g",
        &[F(f64::NAN); 5],
    );
    assert_eq!(
        output,
        include_bytes!("fixtures/nonfinite-format-padding.expected.out")
    );
    // INTEGER -> FLOAT conversion intentionally differs from exact %d.
    assert_eq!(
        rendered(
            b"%d|%.0f",
            &[FormatArgument::Integer(9_007_199_254_740_993); 2]
        ),
        b"9007199254740993|9007199254740992"
    );
}

#[test]
fn scan_and_count_fences_precede_primitive_visitation_and_bounds_preserve_counts() {
    let mut visits = Vec::new();
    assert!(matches!(
        assemble(b"%d:%q", &[], ALLOWED, &mut visits),
        Err(AssemblyError::Invalid(_))
    ));
    assert!(visits.is_empty());
    assert!(matches!(
        assemble(b"%d", &[], ALLOWED, &mut visits),
        Err(AssemblyError::Count(_))
    ));
    assert!(visits.is_empty());
    let control = format!("%{}d", "0".repeat(74));
    assert_eq!(rendered(control.as_bytes(), &[]), control.as_bytes());
    assert!(matches!(
        assemble(
            control.as_bytes(),
            &[FormatArgument::Integer(7)],
            ALLOWED,
            &mut visits
        ),
        Err(AssemblyError::Count(_))
    ));
    assert!(visits.is_empty());
    assert_eq!(
        rendered(b"%12%04d", &[FormatArgument::Integer(7)]),
        b"%120007"
    );
    assert_eq!(rendered(b"raw\0%q", &[]), b"raw");
}

#[test]
fn noncanonical_admission_precedes_the_algorithmic_fallback() {
    for (control, expected) in [
        (b"%5-3d".as_slice(), b"%5-3lld".as_slice()),
        (b"%.2.3d", b"%.2.3lld"),
        (b"%..d", b"%.0.lld"),
        (b"%.-3d", b"%.0-3lld"),
        (b"%1.2-3d", b"%1.2-3lld"),
    ] {
        let plan = scan_format(control).unwrap();
        let FormatPiece::Directive(directive) = &plan.pieces[0] else {
            panic!("directive")
        };
        let mut output = ByteBuffer::new();
        output.push_bytes(b"prefix");
        let error = append_directive(&mut output, directive, FormatArgument::Integer(7), ALLOWED)
            .unwrap_err();
        assert!(matches!(error, DirectiveError::NonCanonical { .. }));
        assert_eq!(output.as_bytes(), b"prefix");
        append_noncanonical(&mut output, directive, ALLOWED).unwrap();
        assert_eq!(&output.as_bytes()[b"prefix".len()..], expected);
        assert_eq!(
            append_directive(
                &mut output,
                directive,
                FormatArgument::String(b"bad"),
                ALLOWED
            ),
            Err(DirectiveError::IntegerArgument(
                IntegerArgumentError::NotNumeric("STRING")
            ))
        );
    }
    let mut visits = Vec::new();
    assert!(matches!(
        assemble(
            b"%2-4f:%d",
            &[FormatArgument::Float(7.0), FormatArgument::Integer(8)],
            ALLOWED,
            &mut visits
        ),
        Err(AssemblyError::Directive(
            DirectiveError::NonCanonical { .. }
        ))
    ));
    assert_eq!(visits, [0]);
}

#[test]
fn parameter_overflow_and_float_integer_domains_do_not_choose_default_or_severity() {
    let huge = format!("%{}9d", usize::MAX);
    let mut visits = Vec::new();
    assert!(matches!(
        assemble(
            huge.as_bytes(),
            &[FormatArgument::Integer(7)],
            ALLOWED,
            &mut visits
        ),
        Err(AssemblyError::Directive(
            DirectiveError::ParameterOverflow {
                parameter: NumericParameter::Width,
                ..
            }
        ))
    ));
    assert_eq!(visits, [0]);
    for (value, error) in [
        (f64::NAN, IntegerArgumentError::NonFinite),
        (f64::INFINITY, IntegerArgumentError::NonFinite),
        (
            9_223_372_036_854_775_808.0,
            IntegerArgumentError::OutsideSignedRange,
        ),
    ] {
        visits.clear();
        assert_eq!(
            assemble(b"%d", &[FormatArgument::Float(value)], ALLOWED, &mut visits),
            Err(AssemblyError::Directive(DirectiveError::IntegerArgument(
                error
            )))
        );
        assert_eq!(visits, [0]);
    }
    visits.clear();
    assert_eq!(
        assemble(
            b"%c:%d",
            &[
                FormatArgument::InstanceName(b"x"),
                FormatArgument::Integer(7)
            ],
            ALLOWED,
            &mut visits
        ),
        Err(AssemblyError::Directive(DirectiveError::ByteArgument(
            ByteArgumentError::ExpectedCharacter("INSTANCE-NAME")
        )))
    );
    assert_eq!(visits, [0]);
}

#[test]
fn aggregate_literal_and_directive_limits_preserve_existing_bytes_atomically() {
    let mut output = ByteBuffer::new();
    append_bytes(&mut output, b"abc", 4).unwrap();
    assert_eq!(
        append_bytes(&mut output, b"de", 4),
        Err(OutputLimit {
            required: Some(5),
            allowed: 4
        })
    );
    assert_eq!(output.as_bytes(), b"abc");
    for control in [b"%4d".as_slice(), b"%4f", b"%4s", b"%4c"] {
        let plan = scan_format(control).unwrap();
        let FormatPiece::Directive(d) = &plan.pieces[0] else {
            panic!("directive")
        };
        let value = if control == b"%4s" {
            FormatArgument::String(b"x")
        } else {
            FormatArgument::Integer(7)
        };
        assert!(matches!(
            append_directive(&mut output, d, value, 4),
            Err(DirectiveError::OutputLimit(_))
        ));
        assert_eq!(output.as_bytes(), b"abc");
    }
    // Multiple individually short fields cannot bypass the aggregate allowance.
    let mut visits = Vec::new();
    assert!(matches!(
        assemble(
            b"%02d%02d",
            &[FormatArgument::Integer(7); 2],
            3,
            &mut visits
        ),
        Err(AssemblyError::Directive(DirectiveError::OutputLimit(_)))
    ));
    assert_eq!(visits, [0, 1]);
    assert_eq!(
        rendered(b"before:%-4c:after", &[FormatArgument::Integer(0)]),
        b"before::after"
    );
}
