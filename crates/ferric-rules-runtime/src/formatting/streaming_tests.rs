use super::*;
use std::mem::{needs_drop, size_of};

#[test]
fn validation_state_is_borrowed_and_constant_size() {
    assert!(!needs_drop::<ValidatedFormat<'_>>());
    assert!(!needs_drop::<FormatPieces<'_>>());
    assert!(size_of::<ValidatedFormat<'_>>() <= 4 * size_of::<usize>());
    assert!(size_of::<FormatPieces<'_>>() <= 4 * size_of::<usize>());
    let source = [0xff, b'a', b'%', b'0', b'4', b'd', 0, b'%', b'q'];
    let plan = validate_format(&source).unwrap();
    assert_eq!(plan.source.as_ptr(), source.as_ptr());
    assert_eq!(plan.effective_len(), 6);
    let mut pieces = plan.pieces();
    let Some(FormatPiece::Literal { bytes, range, .. }) = pieces.next() else {
        panic!("expected borrowed literal")
    };
    assert_eq!(range, 0..2);
    assert_eq!(bytes.as_ptr(), source.as_ptr());
    let Some(FormatPiece::Directive(directive)) = pieces.next() else {
        panic!("expected borrowed modifiers")
    };
    assert_eq!(directive.raw_modifiers.as_ptr(), source[3..].as_ptr());
    assert_eq!(directive.range, 2..6);
    assert_eq!(pieces.next(), None);
    assert_eq!(pieces.next(), None);
}

#[test]
fn sixteen_mib_of_no_data_controls_never_requires_a_piece_vector() {
    // The real per-call prefix ceiling, with eight million tiny pieces. The
    // source allocation is intentional; neither pass collects their outputs.
    let source = b"%n".repeat(8 * 1024 * 1024);
    let plan = validate_format(&source).unwrap();
    assert_eq!(plan.effective_len(), 16 * 1024 * 1024);
    assert_eq!(plan.conversion_count, 0);
    assert_eq!(plan.check_operand_count(0), Ok(()));
    let mut count = 0;
    for piece in plan.pieces() {
        let FormatPiece::Control { range, byte } = piece else {
            panic!("expected newline control")
        };
        assert_eq!(range, count * 2..count * 2 + 2);
        assert_eq!(byte, b'\n');
        count += 1;
    }
    assert_eq!(count, source.len() / 2);
}

#[test]
fn many_admitted_directives_retain_count_and_modifiers_without_a_new_cap() {
    let repetitions = 262_144;
    let source = b"%..d".repeat(repetitions);
    let plan = validate_format(&source).unwrap();
    assert_eq!(plan.check_operand_count(repetitions), Ok(()));
    for (index, piece) in plan.pieces().enumerate() {
        let FormatPiece::Directive(directive) = piece else {
            panic!("expected admitted directive")
        };
        assert_eq!(directive.range, index * 4..index * 4 + 4);
        assert_eq!(directive.raw_modifiers, b"..");
        assert_eq!(directive.conversion, Conversion::Decimal);
        assert_eq!(
            directive.spec,
            SpecAnalysis::NonCanonical {
                offset: index * 4 + 2,
                byte: b'.',
            }
        );
    }
    assert_eq!(plan.pieces().count(), repetitions);
}

#[test]
fn full_validation_and_count_fence_visitation_even_after_a_long_prefix() {
    // This visitor stands for the wrapper's second-pass entry point; it does
    // not simulate Engine effects or establish a runtime error-value contract.
    fn visit<'a>(
        source: &'a [u8],
        actual: usize,
        visited: &mut usize,
    ) -> Result<(), Result<InvalidFlag<'a>, OperandCountMismatch>> {
        let plan = validate_format(source).map_err(Ok)?;
        plan.check_operand_count(actual).map_err(Err)?;
        for piece in plan.pieces() {
            if matches!(piece, FormatPiece::Directive(_)) {
                *visited += 1;
            }
        }
        Ok(())
    }
    let mut source = b"%n".repeat(65_536);
    source.extend_from_slice(b"%d%q");
    let mut visited = 0;
    let Err(Ok(invalid)) = visit(&source, 0, &mut visited) else {
        panic!("later invalid flag must precede count and visitation")
    };
    assert_eq!(invalid.percent_offset, source.len() - 2);
    assert_eq!(invalid.flag_offset, source.len() - 1);
    assert_eq!(invalid.fragment, b"%q");
    assert_eq!(visited, 0);
    source.truncate(source.len() - 2);
    assert_eq!(
        visit(&source, 0, &mut visited),
        Err(Err(OperandCountMismatch {
            expected: 1,
            actual: 0,
        }))
    );
    assert_eq!(visited, 0);
    assert_eq!(visit(&source, 1, &mut visited), Ok(()));
    assert_eq!(visited, 1);
}

#[test]
fn lazy_pieces_preserve_source_partition_boundaries_and_repeatability() {
    let mut source = vec![0xff, b':'];
    source.extend_from_slice(b"%n%12%%");
    source.extend_from_slice(format!("%{}d|%{}q:%..s", "0".repeat(73), "0".repeat(74)).as_bytes());
    let effective_len = source.len();
    source.extend_from_slice(b"\0%q");
    let plan = validate_format(&source).unwrap();
    assert_eq!(plan.conversion_count, 2);
    assert_eq!(plan.effective_len(), effective_len);
    let first: Vec<_> = plan.pieces().collect();
    assert_eq!(first, plan.pieces().collect::<Vec<_>>());
    let mut next = 0;
    let mut saw_bound = false;
    let mut saw_percent = false;
    for piece in first {
        let range = match piece {
            FormatPiece::Literal { range, bytes, kind } => {
                assert_eq!(bytes, &source[range.clone()]);
                saw_bound |= kind == LiteralKind::Incomplete(IncompleteReason::ModifierBound);
                saw_percent |= kind == LiteralKind::Incomplete(IncompleteReason::Percent);
                range
            }
            FormatPiece::Control { range, .. } => {
                assert_eq!(source[range.start], b'%');
                range
            }
            FormatPiece::Directive(directive) => {
                let range = directive.range;
                assert_eq!(
                    directive.raw_modifiers,
                    &source[range.start + 1..range.end - 1]
                );
                assert_eq!(source[range.end - 1], directive.conversion.as_byte());
                range
            }
        };
        assert_eq!(range.start, next);
        assert!(range.end > range.start);
        next = range.end;
    }
    assert_eq!(next, effective_len);
    assert!(saw_bound && saw_percent);
}

#[test]
fn scanner_error_is_fused_and_nul_suffix_is_never_a_second_pass_input() {
    let mut scanner = PieceScanner::new(b"%d%q%d");
    assert!(matches!(
        scanner.next(),
        Some(Ok(FormatPiece::Directive(_)))
    ));
    assert!(matches!(
        scanner.next(),
        Some(Err(InvalidFlag { flag: b'q', .. }))
    ));
    assert_eq!(scanner.next(), None);
    assert_eq!(scanner.next(), None);
    for source in [b"\0%q".as_slice(), b"%d\0%q", b"%12\0%q"] {
        let plan = validate_format(source).unwrap();
        let prefix = &source[..plan.effective_len()];
        assert_eq!(
            plan.pieces().collect::<Vec<_>>(),
            validate_format(prefix)
                .unwrap()
                .pieces()
                .collect::<Vec<_>>()
        );
    }
}
