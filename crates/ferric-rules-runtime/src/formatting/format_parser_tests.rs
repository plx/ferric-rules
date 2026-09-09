use super::*;

fn only_directive(source: &[u8]) -> Directive<'_> {
    let plan = scan_format(source).unwrap();
    assert_eq!(plan.conversion_count, 1);
    assert_eq!(plan.pieces.len(), 1);
    match plan.pieces.into_iter().next().unwrap() {
        FormatPiece::Directive(directive) => directive,
        piece => panic!("expected directive, got {piece:?}"),
    }
}

fn literal_output(plan: &FormatPlan<'_>) -> Vec<u8> {
    let mut output = Vec::new();
    for piece in &plan.pieces {
        match piece {
            FormatPiece::Literal { bytes, .. } => output.extend_from_slice(bytes),
            FormatPiece::Control { byte, .. } => output.push(*byte),
            FormatPiece::Directive(_) => panic!("test needs explicit operand rendering"),
        }
    }
    output
}

#[test]
fn empty_and_first_nul_stop_do_not_inspect_the_suffix() {
    // First-NUL behavior is source-derived (ControlStringCheck), not claimed as
    // a successful literal-NUL batch probe. Raw input comes from the host API.
    for source in [b"".as_slice(), b"\0%q"] {
        let plan = scan_format(source).unwrap();
        assert_eq!(plan.effective_len, 0);
        assert!(plan.pieces.is_empty());
        assert_eq!(plan.check_operand_count(0), Ok(()));
    }
    let plan = scan_format(b"abc\0%q%d").unwrap();
    assert_eq!(plan.effective_len, 3);
    assert_eq!(literal_output(&plan), b"abc");
    assert_eq!(plan.conversion_count, 0);
    let plan = scan_format(b"%04d\0%q").unwrap();
    assert_eq!(plan.effective_len, 4);
    assert_eq!(plan.check_operand_count(1), Ok(()));
    let plan = scan_format(b"%12\0d").unwrap();
    assert_eq!(literal_output(&plan), b"%12");
}

#[test]
fn literal_bytes_are_borrowed_without_utf8_decoding() {
    let source = [0xff, 0xc3, b' ', b'a', b'%', b'0', b'4', b'd', 0x0c];
    let plan = scan_format(&source).unwrap();
    assert_eq!(plan.effective_len, source.len());
    assert_eq!(plan.pieces.len(), 3);
    match &plan.pieces[0] {
        FormatPiece::Literal { range, bytes, kind } => {
            assert_eq!(*range, 0..4);
            assert_eq!(*bytes, [0xff, 0xc3, b' ', b'a']);
            assert_eq!(bytes.as_ptr(), source.as_ptr());
            assert_eq!(*kind, LiteralKind::Plain);
        }
        other => panic!("expected literal, got {other:?}"),
    }
    match &plan.pieces[1] {
        FormatPiece::Directive(directive) => {
            assert_eq!(directive.range, 4..8);
            assert_eq!(directive.raw_modifiers, b"04");
            assert_eq!(directive.raw_modifiers.as_ptr(), source[5..].as_ptr());
        }
        other => panic!("expected directive, got {other:?}"),
    }
    assert_eq!(
        plan.pieces[2],
        FormatPiece::Literal {
            range: 8..9,
            bytes: &[0x0c],
            kind: LiteralKind::Plain,
        }
    );
}

#[test]
fn immediate_no_data_controls_have_exact_bytes_and_ranges() {
    // ordinary-directives: %r is CR (0x0D), not LF; %v is VT (0x0B).
    let plan = scan_format(b"%%:%n:%r:%t:%v").unwrap();
    assert_eq!(literal_output(&plan), b"%:\n:\r:\t:\x0b");
    assert_eq!(plan.conversion_count, 0);
    let controls: Vec<_> = plan
        .pieces
        .iter()
        .filter_map(|piece| match piece {
            FormatPiece::Control { range, byte } => Some((range.clone(), *byte)),
            _ => None,
        })
        .collect();
    assert_eq!(
        controls,
        [
            (0..2, b'%'),
            (3..5, b'\n'),
            (6..8, b'\r'),
            (9..11, b'\t'),
            (12..14, 0x0b)
        ]
    );
}

#[test]
fn all_nine_data_conversions_count_and_retain_clips_fragment_spelling() {
    let plan = scan_format(b"%d%o%x%u%c%s%e%f%g").unwrap();
    assert_eq!(plan.conversion_count, 9);
    let expected = [
        (Conversion::Decimal, b"%lld".as_slice()),
        (Conversion::Octal, b"%llo"),
        (Conversion::Hex, b"%llx"),
        (Conversion::Unsigned, b"%llu"),
        (Conversion::Character, b"%c"),
        (Conversion::Lexeme, b"%s"),
        (Conversion::Scientific, b"%e"),
        (Conversion::Fixed, b"%f"),
        (Conversion::General, b"%g"),
    ];
    for (index, (piece, (conversion, fragment))) in plan.pieces.iter().zip(expected).enumerate() {
        let FormatPiece::Directive(directive) = piece else {
            panic!("expected directive");
        };
        assert_eq!(directive.range, index * 2..index * 2 + 2);
        assert_eq!(directive.conversion, conversion);
        assert_eq!(
            directive.spec,
            SpecAnalysis::Canonical(FormatSpec::default())
        );
        assert_eq!(directive.clips_printf_fragment(), fragment);
    }
    assert_eq!(plan.check_operand_count(9), Ok(()));
}

#[test]
fn canonical_flags_width_and_precision_preserve_distinctions() {
    // Originals integer-flags / integer-precision plus sealed repeated flags.
    let cases = [
        (b"%04d".as_slice(), false, true, 4, None),
        (b"%-05d", true, true, 5, None),
        (b"%0-5d", true, true, 5, None),
        (b"%0005d", false, true, 5, None),
        (b"%--5d", true, false, 5, None),
        (b"%0-05d", true, true, 5, None),
        (b"%00d", false, true, 0, None),
        (b"%-.d", true, false, 0, Some(0)),
        (b"%.d", false, false, 0, Some(0)),
        (b"%.0d", false, false, 0, Some(0)),
        (b"%8.03d", false, false, 8, Some(3)),
        (b"%-08.3d", true, true, 8, Some(3)),
        (b"%012.2e", false, true, 12, Some(2)),
        (b"%010.3g", false, true, 10, Some(3)),
        (b"%.1s", false, false, 0, Some(1)),
        (b"%4.2c", false, false, 4, Some(2)),
    ];
    for (source, left_align, zero_pad, width, precision) in cases {
        let directive = only_directive(source);
        assert_eq!(
            directive.spec,
            SpecAnalysis::Canonical(FormatSpec {
                left_align,
                zero_pad,
                width,
                precision
            }),
            "{source:?}"
        );
        assert_eq!(directive.raw_modifiers, &source[1..source.len() - 1]);
    }
}

#[test]
fn incomplete_fragments_are_literal_and_percent_is_reprocessed() {
    // All five formats are sealed in unfinished-percent-fragments.
    for (source, expected) in [
        (b"%".as_slice(), b"%".as_slice()),
        (b"%12", b"%12"),
        (b"%-.3", b"%-.3"),
        (b"%12%%", b"%12%"),
    ] {
        let plan = scan_format(source).unwrap();
        assert_eq!(literal_output(&plan), expected);
        assert_eq!(plan.conversion_count, 0);
    }
    let plan = scan_format(b"%12%04d").unwrap();
    assert_eq!(plan.conversion_count, 1);
    assert_eq!(
        plan.pieces[0],
        FormatPiece::Literal {
            range: 0..3,
            bytes: b"%12",
            kind: LiteralKind::Incomplete(IncompleteReason::Percent),
        }
    );
    let FormatPiece::Directive(directive) = &plan.pieces[1] else {
        panic!("expected the second percent to begin a directive");
    };
    assert_eq!(directive.range, 3..7);
    assert_eq!(directive.raw_modifiers, b"04");
    // The pure parser does not render 7, but the renderer receives this exact
    // spec; the pinned composed output is %120007.
    assert_eq!(
        directive.spec,
        SpecAnalysis::Canonical(FormatSpec {
            zero_pad: true,
            width: 4,
            ..FormatSpec::default()
        })
    );
}

#[test]
fn seventy_three_modifiers_still_allow_a_conversion() {
    let source = format!("%{}d", "0".repeat(73));
    let directive = only_directive(source.as_bytes());
    assert_eq!(directive.range, 0..75);
    assert_eq!(directive.raw_modifiers.len(), 73);
    assert_eq!(
        directive.spec,
        SpecAnalysis::Canonical(FormatSpec {
            zero_pad: true,
            ..FormatSpec::default()
        })
    );
    assert_eq!(directive.clips_printf_fragment().len(), 77);
}

#[test]
fn seventy_four_modifiers_become_literal_without_consuming_data() {
    // modifier-buffer-73-versus-74 and modifier-buffer-literal-extra-operand.
    let source = format!("%{}d", "0".repeat(74));
    let plan = scan_format(source.as_bytes()).unwrap();
    assert_eq!(plan.conversion_count, 0);
    assert_eq!(literal_output(&plan), source.as_bytes());
    assert_eq!(plan.pieces.len(), 2);
    assert!(matches!(
        plan.pieces[0],
        FormatPiece::Literal {
            range: Range { start: 0, end: 75 },
            kind: LiteralKind::Incomplete(IncompleteReason::ModifierBound),
            ..
        }
    ));
    assert_eq!(
        plan.check_operand_count(1),
        Err(OperandCountMismatch {
            expected: 0,
            actual: 1,
        })
    );
}

#[test]
fn bytes_after_modifier_bound_return_to_outer_literal_scanning() {
    // Source-derived extensions of the measured 73/74 boundary.
    let source = format!("%{}q:%d", "0".repeat(74));
    let plan = scan_format(source.as_bytes()).unwrap();
    assert_eq!(plan.conversion_count, 1);
    assert!(matches!(
        plan.pieces[1],
        FormatPiece::Literal {
            bytes: b"q:",
            kind: LiteralKind::Plain,
            ..
        }
    ));
    let invalid = format!("%{}q", "0".repeat(73));
    let error = scan_format(invalid.as_bytes()).unwrap_err();
    assert_eq!(error.flag_offset, 74);
    assert_eq!(error.flag, b'q');
}

#[test]
fn invalid_flags_report_the_first_offending_byte_and_absolute_offset() {
    // Original invalid-flag controls plus source-derived unrecognized flags.
    for flag in [
        b'i', b'l', b'X', b'F', b'E', b'G', b'*', b'+', b' ', b'#', b'$', 0xff,
    ] {
        let source = [b'a', b'b', b'%', b'1', b'.', b'2', flag, b'd'];
        let error = scan_format(&source).unwrap_err();
        assert_eq!(error.percent_offset, 2);
        assert_eq!(error.flag_offset, 6);
        assert_eq!(error.flag, flag);
        assert_eq!(error.fragment, &source[2..7]);
        assert_eq!(error.fragment.as_ptr(), source[2..].as_ptr());
    }
    for source in [b"%5n", b"%5r", b"%5t", b"%5v"] {
        let error = scan_format(source).unwrap_err();
        assert_eq!(error.flag_offset, 2);
    }
}

#[test]
fn whole_format_validation_precedes_count_mismatch() {
    // The invalid-later-conversion oracle skips every data expression.
    let source = b"%d:%q";
    let error = scan_format(source).unwrap_err();
    assert_eq!(error.percent_offset, 3);
    assert_eq!(error.flag_offset, 4);
    assert_eq!(error.fragment, b"%q");
    // No plan is returned to begin evaluation or to report a count error.
    let plan = scan_format(b"%d:%d").unwrap();
    assert_eq!(
        plan.check_operand_count(1),
        Err(OperandCountMismatch {
            expected: 2,
            actual: 1
        })
    );
    assert_eq!(
        plan.check_operand_count(3),
        Err(OperandCountMismatch {
            expected: 2,
            actual: 3
        })
    );
    assert_eq!(plan.check_operand_count(2), Ok(()));
}

#[test]
fn measured_noncanonical_modifiers_keep_conversion_count_and_raw_spelling() {
    // small-repeated-misordered-modifiers: exact libc echoes live separately in
    // malformed-echo-catalog.json. They are NOT a scanner/rendering fallback.
    let cases = [
        (b"%5-3d".as_slice(), 2, b'-', b"%5-3lld".as_slice()),
        (b"%.2.3d", 3, b'.', b"%.2.3lld"),
        (b"%..d", 2, b'.', b"%..lld"),
        (b"%.-3d", 2, b'-', b"%.-3lld"),
        (b"%1.2-3d", 4, b'-', b"%1.2-3lld"),
    ];
    for (source, offset, byte, intermediate) in cases {
        let plan = scan_format(source).unwrap();
        assert_eq!(plan.check_operand_count(1), Ok(()));
        let FormatPiece::Directive(directive) = &plan.pieces[0] else {
            panic!("expected directive")
        };
        assert_eq!(directive.conversion, Conversion::Decimal);
        assert_eq!(directive.spec, SpecAnalysis::NonCanonical { offset, byte });
        assert_eq!(directive.raw_modifiers, &source[1..source.len() - 1]);
        assert_eq!(directive.clips_printf_fragment(), intermediate);
    }
    let plan = scan_format(b"x:%5-3d:%.2.3f:%..s").unwrap();
    assert_eq!(plan.check_operand_count(3), Ok(()));
    let FormatPiece::Directive(directive) = &plan.pieces[1] else {
        panic!("expected directive")
    };
    assert_eq!(
        directive.spec,
        SpecAnalysis::NonCanonical {
            offset: 4,
            byte: b'-'
        }
    );
}

#[test]
fn canonical_parameter_overflow_is_not_an_invalid_flag_or_lost_operand() {
    // Resource classification is host-size dependent; this tests checked
    // parsing without attempting giant allocation or invoking the reference.
    let huge = format!("{}9", usize::MAX);
    let width = format!("%{huge}d");
    let directive = only_directive(width.as_bytes());
    assert_eq!(
        directive.spec,
        SpecAnalysis::ParameterOverflow {
            parameter: NumericParameter::Width,
            range: 1..huge.len() + 1,
        }
    );
    let precision = format!("%.{huge}f");
    let directive = only_directive(precision.as_bytes());
    assert_eq!(
        directive.spec,
        SpecAnalysis::ParameterOverflow {
            parameter: NumericParameter::Precision,
            range: 2..huge.len() + 2,
        }
    );
    // A noncanonical spelling is classified syntactically before conversion,
    // even when an earlier numeric prefix is too large for usize.
    let noncanonical = format!("%{huge}-3d");
    assert_eq!(
        only_directive(noncanonical.as_bytes()).spec,
        SpecAnalysis::NonCanonical {
            offset: huge.len() + 1,
            byte: b'-',
        }
    );
}

#[test]
fn accepted_modifier_runs_cannot_hide_a_later_true_invalid_flag() {
    let error = scan_format(b"%5-3d|%..d|%+d").unwrap_err();
    assert_eq!(error.percent_offset, 11);
    assert_eq!(error.flag_offset, 12);
    assert_eq!(error.fragment, b"%+");
}
