use super::*;

const ALLOWED: usize = 8192;

fn spec(width: usize, zero_pad: bool, left_align: bool, precision: Option<usize>) -> FormatSpec {
    FormatSpec {
        left_align,
        zero_pad,
        width,
        precision,
    }
}

fn render(value: f64, conversion: FloatConversion, spec: FormatSpec) -> Vec<u8> {
    let mut out = ByteBuffer::new();
    render_float(&mut out, value, conversion, spec, ALLOWED).unwrap();
    out.as_bytes().to_vec()
}

#[test]
fn sealed_float_width_and_precision_cases_match_exact_bytes() {
    use FloatConversion::{Fixed, General, Scientific};
    let mut observed = Vec::new();
    for (value, conversion, format) in [
        (2.5, Fixed, spec(8, true, false, Some(2))),
        (-2.5, Fixed, spec(8, true, false, Some(2))),
        (2.5, Fixed, spec(8, true, true, Some(2))),
        (-2.5, Fixed, spec(8, true, true, Some(2))),
        (2.5, Scientific, spec(12, true, false, Some(2))),
        (-2.5, Scientific, spec(12, true, false, Some(2))),
        (2.5, General, spec(10, true, false, Some(3))),
        (-2.5, General, spec(10, true, false, Some(3))),
    ] {
        observed.push(b'[');
        observed.extend(render(value, conversion, format));
        observed.extend(b"]\n");
    }
    assert_eq!(
        observed,
        include_bytes!("fixtures/float-zero-and-precision.expected.out")
    );
}

#[test]
fn general_fifteen_digits_reuses_sealed_345_round_once_values_without_type_suffix() {
    use FloatConversion::General;
    // #345's source FloatToString uses %.15g, then appends .0 separately.
    // These expectations preserve the underlying %.15g spelling only.
    for (value, expected) in [
        (1.0, "1"),
        (-0.0, "-0"),
        (0.1, "0.1"),
        (0.0001, "0.0001"),
        (1e-5, "1e-05"),
        (1e14, "100000000000000"),
        (1e15, "1e+15"),
        (1e20, "1e+20"),
        (1.234_567_890_123_456_7, "1.23456789012346"),
        (5e-324, "4.94065645841247e-324"),
        (0.000_099_999_999_999_999_94, "9.99999999999999e-05"),
        (0.000_099_999_999_999_999_95, "0.0001"),
        (999_999_999_999_999.4, "999999999999999"),
        (999_999_999_999_999.5, "1e+15"),
        (1.234_567_890_123_445, "1.23456789012344"),
        (1.234_567_890_123_455, "1.23456789012345"),
    ] {
        assert_eq!(
            render(value, General, spec(0, false, false, Some(15))),
            expected.as_bytes()
        );
    }
}

#[test]
fn resource_rejection_is_atomic_and_precedes_output_sized_precision_work() {
    use FloatConversion::{Fixed, General, Scientific};
    for (value, conversion, format) in [
        (2.5, Fixed, spec(usize::MAX, true, false, Some(2))),
        (2.5, Scientific, spec(0, false, false, Some(usize::MAX))),
        (2.5, Fixed, spec(0, false, false, Some(usize::MAX))),
        (f64::MAX, Fixed, spec(0, false, false, Some(0))),
        (12345.0, General, FormatSpec::default()),
    ] {
        let mut out = ByteBuffer::new();
        out.push_bytes(b"prefix");
        assert!(render_float(&mut out, value, conversion, format, 10).is_err());
        assert_eq!(out.as_bytes(), b"prefix");
    }
    // Arbitrary g precision adds no meaningful digits after exact f64 decimal
    // expansion. Bounded scratch preserves a short result even with usize::MAX.
    let mut out = ByteBuffer::new();
    render_float(
        &mut out,
        1.0,
        General,
        spec(0, false, false, Some(usize::MAX)),
        1,
    )
    .unwrap();
    assert_eq!(out.as_bytes(), b"1");
}

#[test]
fn exact_binary_decimal_extremes_are_defensive_checks_without_unsafe_c_oracles() {
    use FloatConversion::{Fixed, General};
    // Closed-form integer 2^1024 - 2^971, independently generated using Python
    // integer arithmetic. The old CLIPS printf buffer is too small for %f here.
    assert_eq!(
        render(f64::MAX, Fixed, spec(0, false, false, Some(0))),
        include_bytes!("fixtures/max-finite-fixed.expected.out")
    );
    // Exact decimal fraction 2^-1074 = 5^1074 / 10^1074, not decimal re-rounding.
    assert_eq!(
        render(f64::from_bits(1), Fixed, spec(0, false, false, Some(1074))),
        include_bytes!("fixtures/min-subnormal-fixed.expected.out")
    );
    assert_eq!(
        render(0.1, General, spec(0, false, false, Some(usize::MAX))),
        b"0.1000000000000000055511151231257827021181583404541015625"
    );
}

fn line(out: &mut Vec<u8>, label: &[u8], columns: &[(f64, FloatConversion, FormatSpec)]) {
    out.extend(label);
    out.extend(b":[");
    for (index, &(value, conversion, spec)) in columns.iter().enumerate() {
        if index != 0 {
            out.push(b'|');
        }
        out.extend(render(value, conversion, spec));
    }
    out.extend(b"]\n");
}

#[test]
fn sealed_general_defaults_zero_precision_and_rounded_cutovers() {
    use FloatConversion::General as G;
    let mut out = Vec::new();
    line(
        &mut out,
        b"g0",
        &[
            (1.0, G, spec(0, false, false, Some(0))),
            (9.9, G, spec(0, false, false, Some(0))),
            (-0.0, G, spec(0, false, false, Some(0))),
        ],
    );
    line(
        &mut out,
        b"g3",
        &[
            (999.4, G, spec(0, false, false, Some(3))),
            (999.5, G, spec(0, false, false, Some(3))),
            (0.000_099_94, G, spec(0, false, false, Some(3))),
            (0.000_099_95, G, spec(0, false, false, Some(3))),
        ],
    );
    line(
        &mut out,
        b"default",
        &[
            (999_999.4, G, FormatSpec::default()),
            (999_999.5, G, FormatSpec::default()),
            (0.0001, G, FormatSpec::default()),
            (0.00001, G, FormatSpec::default()),
        ],
    );
    line(
        &mut out,
        b"subnormal",
        &[
            (5e-324, G, spec(0, false, false, Some(6))),
            (f64::MAX, G, FormatSpec::default()),
        ],
    );
    assert_eq!(
        out,
        include_bytes!("fixtures/general-precision-and-cutovers.expected.out")
    );
}

#[test]
fn sealed_scientific_exponents_signed_zero_and_fixed_ties() {
    use FloatConversion::{Fixed as F, Scientific as E};
    let mut out = Vec::new();
    line(
        &mut out,
        b"e",
        &[
            (12345.0, E, FormatSpec::default()),
            (9.9, E, spec(0, false, false, Some(0))),
            (1e-5, E, spec(0, false, false, Some(2))),
            (1e99, E, spec(0, false, false, Some(2))),
            (1e100, E, spec(0, false, false, Some(2))),
        ],
    );
    line(
        &mut out,
        b"zero",
        &[
            (-0.0, E, spec(10, true, false, Some(2))),
            (-0.0, F, spec(8, true, false, Some(2))),
        ],
    );
    line(
        &mut out,
        b"ties",
        &[
            (2.5, F, spec(0, false, false, Some(0))),
            (3.5, F, spec(0, false, false, Some(0))),
            (-2.5, F, spec(0, false, false, Some(0))),
            (-3.5, F, spec(0, false, false, Some(0))),
        ],
    );
    line(
        &mut out,
        b"subnormal",
        &[(5e-324, E, spec(0, false, false, Some(6)))],
    );
    assert_eq!(
        out,
        include_bytes!("fixtures/scientific-exponent-and-fixed-ties.expected.out")
    );
}

#[test]
fn sealed_nonfinite_spellings_pad_with_spaces_even_with_zero_flag() {
    use FloatConversion::{Fixed as F, General as G, Scientific as E};
    let mut out = Vec::new();
    for (label, value) in [
        (b"inf".as_slice(), f64::INFINITY),
        (b"nan".as_slice(), f64::NAN),
    ] {
        let columns = [
            (value, F, FormatSpec::default()),
            (value, F, spec(8, true, false, None)),
            (value, F, spec(8, true, true, None)),
            (value, E, spec(8, true, false, None)),
            (value, G, spec(8, true, false, None)),
        ];
        line(&mut out, label, &columns);
        if label == b"inf" {
            line(
                &mut out,
                b"negative",
                &[
                    (f64::NEG_INFINITY, F, spec(8, true, false, None)),
                    (f64::NEG_INFINITY, E, spec(8, true, false, None)),
                    (f64::NEG_INFINITY, G, spec(8, true, false, None)),
                ],
            );
        }
    }
    assert_eq!(
        out,
        include_bytes!("fixtures/nonfinite-format-padding.expected.out")
    );
}
