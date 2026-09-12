//! Pure rendering controls; no wrapper evaluation or diagnostic severity inferred.
use super::*;

const ALLOWANCE: usize = 4096;

fn spec(width: usize, zero_pad: bool, left_align: bool, precision: Option<usize>) -> FormatSpec {
    FormatSpec {
        left_align,
        zero_pad,
        width,
        precision,
    }
}

fn integer(conversion: IntegerConversion, value: i64, spec: FormatSpec) -> Vec<u8> {
    let mut out = ByteBuffer::new();
    render_integer(&mut out, conversion, value, spec, ALLOWANCE).unwrap();
    out.as_bytes().to_vec()
}

fn string(bytes: &[u8], spec: FormatSpec) -> Vec<u8> {
    let mut out = ByteBuffer::new();
    render_lexeme(&mut out, bytes, spec, ALLOWANCE).unwrap();
    out.as_bytes().to_vec()
}

fn character(byte: u8, spec: FormatSpec) -> Vec<u8> {
    let mut out = ByteBuffer::new();
    render_character(&mut out, byte, spec, ALLOWANCE).unwrap();
    out.as_bytes().to_vec()
}

#[test]
fn original_decimal_zero_padding_and_width_sign_controls() {
    use IntegerConversion::Decimal;
    for (value, expected) in [
        (7, b"0007".as_slice()),
        (-7, b"-007".as_slice()),
        (0, b"0000".as_slice()),
        (12345, b"12345".as_slice()),
        (-12345, b"-12345".as_slice()),
    ] {
        assert_eq!(
            integer(Decimal, value, spec(4, true, false, None)),
            expected
        );
    }
    for width in [0, 1] {
        assert_eq!(integer(Decimal, 7, spec(width, true, false, None)), b"7");
    }
    assert_eq!(integer(Decimal, 7, spec(6, false, false, None)), b"     7");
    assert_eq!(integer(Decimal, 7, spec(6, false, true, None)), b"7     ");
}

#[test]
fn normalized_flag_order_keeps_left_alignment_and_explicit_precision_rules() {
    use IntegerConversion::Decimal;
    // The eventual parser normalizes both %-05d and %0-5d to these flags.
    for (value, expected) in [(7, b"7    ".as_slice()), (-7, b"-7   ".as_slice())] {
        assert_eq!(integer(Decimal, value, spec(5, true, true, None)), expected);
    }
    assert_eq!(integer(Decimal, 7, spec(5, true, false, None)), b"00007");
    assert_eq!(integer(Decimal, -7, spec(5, true, false, None)), b"-0007");
    for (value, digits, padded, left) in [
        (
            7,
            b"007".as_slice(),
            b"     007".as_slice(),
            b"007     ".as_slice(),
        ),
        (
            -7,
            b"-007".as_slice(),
            b"    -007".as_slice(),
            b"-007    ".as_slice(),
        ),
        (
            0,
            b"000".as_slice(),
            b"     000".as_slice(),
            b"000     ".as_slice(),
        ),
    ] {
        assert_eq!(
            integer(Decimal, value, spec(0, false, false, Some(3))),
            digits
        );
        for zero_pad in [false, true] {
            assert_eq!(
                integer(Decimal, value, spec(8, zero_pad, false, Some(3))),
                padded
            );
        }
        assert_eq!(integer(Decimal, value, spec(8, true, true, Some(3))), left);
    }
}

#[test]
fn explicit_zero_precision_suppresses_only_a_zero_value() {
    use IntegerConversion::Decimal;
    for (value, plain, padded) in [
        (7, b"7".as_slice(), b"    7".as_slice()),
        (-7, b"-7".as_slice(), b"   -7".as_slice()),
        (0, b"".as_slice(), b"     ".as_slice()),
    ] {
        // Both %.0d and %.d normalize to Some(0).
        assert_eq!(
            integer(Decimal, value, spec(0, false, false, Some(0))),
            plain
        );
        assert_eq!(
            integer(Decimal, value, spec(5, true, false, Some(0))),
            padded
        );
    }
}

#[test]
fn signed_extrema_do_not_overflow_and_unsigned_radices_reinterpret_bits() {
    use IntegerConversion::{Decimal, Hex, Octal, Unsigned};
    assert_eq!(
        integer(Decimal, i64::MAX, spec(22, true, false, None)),
        b"0009223372036854775807"
    );
    assert_eq!(
        integer(Decimal, i64::MIN, spec(22, true, false, None)),
        b"-009223372036854775808"
    );
    for (conversion, padded, precise, negative) in [
        (
            Octal,
            b"00000101".as_slice(),
            b"     101".as_slice(),
            b"1777777777777777777777".as_slice(),
        ),
        (
            Hex,
            b"00000041".as_slice(),
            b"     041".as_slice(),
            b"ffffffffffffffff".as_slice(),
        ),
        (
            Unsigned,
            b"00000065".as_slice(),
            b"     065".as_slice(),
            b"18446744073709551615".as_slice(),
        ),
    ] {
        assert_eq!(integer(conversion, 65, spec(8, true, false, None)), padded);
        assert_eq!(
            integer(conversion, 65, spec(8, true, false, Some(3))),
            precise
        );
        assert_eq!(integer(conversion, -1, FormatSpec::default()), negative);
    }
    // Source-defined 64-bit bit-pattern derivation, not additional oracle claims.
    assert_eq!(
        integer(Hex, i64::MIN, FormatSpec::default()),
        b"8000000000000000"
    );
    assert_eq!(
        integer(Unsigned, i64::MIN, FormatSpec::default()),
        b"9223372036854775808"
    );
}

#[test]
fn numeric_conversion_has_an_explicit_finite_signed_range_boundary() {
    assert_eq!(
        integer_argument(FormatArgument::Integer(i64::MIN)),
        Ok(i64::MIN)
    );
    assert_eq!(
        integer_argument(FormatArgument::Integer(i64::MAX)),
        Ok(i64::MAX)
    );
    assert_eq!(integer_argument(FormatArgument::Float(7.9)), Ok(7));
    assert_eq!(integer_argument(FormatArgument::Float(-7.9)), Ok(-7));
    assert_eq!(integer_argument(FormatArgument::Float(-0.0)), Ok(0));
    assert_eq!(
        integer_argument(FormatArgument::Float(-9_223_372_036_854_775_808.0)),
        Ok(i64::MIN)
    );
    // These classify unmeasured, nonportable C casts; no fatal/default policy.
    assert_eq!(
        integer_argument(FormatArgument::Float(9_223_372_036_854_775_808.0)),
        Err(IntegerArgumentError::OutsideSignedRange)
    );
    assert_eq!(
        integer_argument(FormatArgument::Float(-9_223_372_036_854_777_856.0)),
        Err(IntegerArgumentError::OutsideSignedRange)
    );
    for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        assert_eq!(
            integer_argument(FormatArgument::Float(value)),
            Err(IntegerArgumentError::NonFinite)
        );
    }
    assert_eq!(
        integer_argument(FormatArgument::Other("VOID")),
        Err(IntegerArgumentError::NotNumeric("VOID"))
    );
}

#[test]
fn string_width_precision_and_nul_use_bytes_without_unicode_repair() {
    assert_eq!(string(b"red", spec(6, false, false, None)), b"   red");
    assert_eq!(string(b"red", spec(6, false, true, None)), b"red   ");
    assert_eq!(string(b"red", spec(6, true, false, None)), b"   red");
    assert_eq!(string(b"abcdef", spec(0, false, false, Some(3))), b"abc");
    assert_eq!(
        string("é".as_bytes(), spec(4, false, false, None)),
        b"  \xc3\xa9"
    );
    assert_eq!(
        string("é".as_bytes(), spec(0, false, false, Some(2))),
        b"\xc3\xa9"
    );
    assert_eq!(
        string("é".as_bytes(), spec(0, false, false, Some(1))),
        b"\xc3"
    );
    // strlen in PrintFormatFlag plus printf %s, source-derived controls.
    assert_eq!(string(b"a\0ignored", spec(4, false, false, None)), b"   a");
    assert_eq!(
        string(b"a\0ignored", spec(4, false, true, Some(9))),
        b"a   "
    );
    assert_eq!(
        string(b"\xff\xc3", spec(3, true, false, Some(1))),
        b"  \xff"
    );
    assert_eq!(string(b"anything", spec(3, true, false, Some(0))), b"   ");
}

#[test]
fn resolved_lexeme_admission_includes_names_but_character_admission_does_not() {
    for value in [
        FormatArgument::String(b"red\xff"),
        FormatArgument::Symbol(b"red\xff"),
        FormatArgument::InstanceName(b"red\xff"),
    ] {
        assert_eq!(lexeme_argument(value), Ok(b"red\xff".as_slice()));
    }
    assert_eq!(
        lexeme_argument(FormatArgument::Integer(65)),
        Err(ByteArgumentError::ExpectedLexeme("INTEGER"))
    );
    assert_eq!(character_argument(FormatArgument::String(b"red")), Ok(b'r'));
    assert_eq!(character_argument(FormatArgument::Symbol(b"red")), Ok(b'r'));
    assert_eq!(
        character_argument(FormatArgument::InstanceName(b"red")),
        Err(ByteArgumentError::ExpectedCharacter("INSTANCE-NAME"))
    );
    assert_eq!(
        character_argument(FormatArgument::Float(65.9)),
        Err(ByteArgumentError::ExpectedCharacter("FLOAT"))
    );
    assert_eq!(character_argument(FormatArgument::Integer(65)), Ok(b'A'));
    assert_eq!(character_argument(FormatArgument::Integer(-1)), Ok(0xff));
    assert_eq!(character_argument(FormatArgument::Integer(256)), Ok(0));
    assert_eq!(character_argument(FormatArgument::String(b"")), Ok(0));
    assert_eq!(
        character_argument(FormatArgument::String("é".as_bytes())),
        Ok(0xc3)
    );
}

#[test]
fn characters_use_space_width_and_truncate_only_their_own_nul_suffix() {
    assert_eq!(character(b'A', FormatSpec::default()), b"A");
    assert_eq!(character(b'A', spec(4, false, false, None)), b"   A");
    assert_eq!(character(b'A', spec(4, false, true, None)), b"A   ");
    assert_eq!(character(b'A', spec(4, true, false, None)), b"   A");
    // Source/C-string derivations; no additional pinned invocations were made.
    assert_eq!(character(0, spec(4, false, false, None)), b"   ");
    assert_eq!(character(0, spec(4, false, true, None)), b"");
    let mut out = ByteBuffer::new();
    out.push_bytes(b"before:");
    render_character(&mut out, 0, spec(3, false, false, None), ALLOWANCE).unwrap();
    out.push_bytes(b":after");
    assert_eq!(out.as_bytes(), b"before:  :after");
    assert_eq!(character(0xc3, FormatSpec::default()), b"\xc3");
}

#[test]
fn sealed_character_precision_nul_and_first_byte_cases() {
    let mut out = Vec::new();
    for (label, columns, suffix) in [
        (
            b"precision".as_slice(),
            [
                (b'A', spec(0, false, false, Some(0))),
                (b'A', spec(0, false, false, Some(3))),
                (b'A', spec(4, false, false, Some(0))),
                (b'A', spec(4, false, true, Some(3))),
            ]
            .as_slice(),
            b"".as_slice(),
        ),
        (
            b"empty".as_slice(),
            [
                (0, FormatSpec::default()),
                (0, spec(4, false, false, None)),
                (0, spec(4, false, true, None)),
            ]
            .as_slice(),
            b":end".as_slice(),
        ),
        (
            b"zero".as_slice(),
            [
                (0, FormatSpec::default()),
                (0, spec(4, false, false, None)),
                (0, spec(4, false, true, None)),
            ]
            .as_slice(),
            b":end".as_slice(),
        ),
        (
            b"bytes".as_slice(),
            [
                (0xc3, FormatSpec::default()),
                (0xc3, spec(3, false, false, None)),
                (0xff, FormatSpec::default()),
                (0xff, FormatSpec::default()),
            ]
            .as_slice(),
            b"".as_slice(),
        ),
    ] {
        out.extend(label);
        out.extend(b":[");
        for (index, &(byte, format)) in columns.iter().enumerate() {
            if index != 0 {
                out.push(b'|');
            }
            out.extend(character(byte, format));
        }
        out.extend(suffix);
        out.extend(b"]\n");
    }
    assert_eq!(
        out,
        include_bytes!("fixtures/character-empty-nul-byte-width-precision.expected.out")
    );
}

#[test]
fn output_allowance_is_checked_before_appending_or_allocating_padding() {
    let mut out = ByteBuffer::new();
    out.push_bytes(b"prefix");
    assert_eq!(
        render_integer(
            &mut out,
            IntegerConversion::Decimal,
            7,
            spec(10, true, false, None),
            12
        ),
        Err(OutputLimit {
            required: Some(16),
            allowed: 12
        })
    );
    assert_eq!(out.as_bytes(), b"prefix");
    assert!(render_integer(
        &mut out,
        IntegerConversion::Decimal,
        -7,
        spec(0, false, false, Some(usize::MAX)),
        12
    )
    .is_err());
    assert_eq!(out.as_bytes(), b"prefix");
    assert!(render_lexeme(&mut out, b"red", spec(usize::MAX, false, false, None), 12).is_err());
    assert!(render_character(&mut out, b'A', spec(usize::MAX, false, false, None), 12).is_err());
    assert_eq!(out.as_bytes(), b"prefix");
}
