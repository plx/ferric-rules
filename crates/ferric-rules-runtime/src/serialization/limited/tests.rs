use serde::de::{self, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

use super::super::{decode_with_limits, encode, SerializationError, SerializationFormat};

fn assert_limit<T: de::DeserializeOwned, const STEPS: usize, const DEPTH: usize>(
    bytes: &[u8],
    format: SerializationFormat,
) {
    assert!(matches!(
        decode_with_limits::<T, STEPS, DEPTH>(bytes, format),
        Err(SerializationError::LimitExceeded(_))
    ));
}

#[test]
fn exact_step_allowance_accepts_both_codecs() {
    for &format in SerializationFormat::ALL {
        let bytes = encode(&vec![(); 8], format).unwrap();
        assert_eq!(
            decode_with_limits::<Vec<()>, 9, 8>(&bytes, format).unwrap(),
            vec![(); 8]
        );
        assert_limit::<Vec<()>, 8, 8>(&bytes, format);
        // A failure cannot poison an independent subsequent decode.
        assert!(decode_with_limits::<Vec<()>, 9, 8>(&bytes, format).is_ok());
    }
}

#[test]
fn indefinite_arrays_and_maps_share_the_step_budget() {
    let array = [0x9f, 0xf6, 0xf6, 0xf6, 0xf6, 0xf6, 0xff];
    assert_limit::<Vec<()>, 4, 8>(&array, SerializationFormat::Cbor);
    let map = [0xbf, 0x00, 0xf4, 0x01, 0xf4, 0x02, 0xf4, 0xff];
    assert_limit::<std::collections::BTreeMap<u8, bool>, 4, 8>(&map, SerializationFormat::Cbor);
}

#[test]
fn keys_and_values_both_consume_steps() {
    let values = std::collections::BTreeMap::from([(0_u8, false), (1, false), (2, false)]);
    for &format in SerializationFormat::ALL {
        let bytes = encode(&values, format).unwrap();
        assert!(
            decode_with_limits::<std::collections::BTreeMap<u8, bool>, 7, 8>(&bytes, format)
                .is_ok()
        );
        assert_limit::<std::collections::BTreeMap<u8, bool>, 6, 8>(&bytes, format);
    }
}

#[test]
fn huge_collection_hints_fail_before_visiting_elements() {
    for header in [0x9b, 0xbb] {
        let mut bytes = vec![header];
        bytes.extend_from_slice(&u64::MAX.to_be_bytes());
        assert_limit::<IgnoredAny, 8, 8>(&bytes, SerializationFormat::Cbor);
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct KnownField {
    keep: u8,
}

#[test]
fn ignored_subtrees_cannot_bypass_depth_or_steps() {
    for &format in SerializationFormat::ALL {
        let deep = serde_json::json!({"keep": 7, "ignored": [[[[[[[null]]]]]]]});
        let bytes = encode(&deep, format).unwrap();
        assert_limit::<KnownField, 100, 4>(&bytes, format);
        assert_eq!(
            decode_with_limits::<KnownField, 100, 32>(&bytes, format).unwrap(),
            KnownField { keep: 7 }
        );
        let wide = serde_json::json!({"keep": 7, "ignored": [null, null, null, null, null]});
        let bytes = encode(&wide, format).unwrap();
        assert_limit::<KnownField, 8, 32>(&bytes, format);
    }
}

#[derive(serde::Serialize, Deserialize)]
enum Tree {
    End,
    One(Box<Tree>),
    Pair(Box<Tree>, Box<Tree>),
    Fields { next: Box<Tree> },
}

#[test]
fn enum_and_newtype_descent_preserves_depth_limits() {
    let mut tree = Tree::End;
    for _ in 0..4 {
        tree = Tree::One(Box::new(Tree::Pair(
            Box::new(Tree::Fields {
                next: Box::new(tree),
            }),
            Box::new(Tree::End),
        )));
    }
    for &format in SerializationFormat::ALL {
        let bytes = encode(&tree, format).unwrap();
        assert_limit::<Tree, 1_000, 8>(&bytes, format);
        assert!(decode_with_limits::<Tree, 1_000, 128>(&bytes, format).is_ok());
    }
}

#[test]
fn native_cbor_recursion_failure_is_typed() {
    // Native CBOR traverses an array before asking the guarded visitor to enter
    // the next value. Its own exhaustion must retain the same public family.
    let bytes = [0x81, 0x81, 0x81, 0xf6];
    assert_limit::<IgnoredAny, 100, 2>(&bytes, SerializationFormat::Cbor);
}

struct SwallowedError;

impl<'de> Deserialize<'de> for SwallowedError {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        let _ = Vec::<()>::deserialize(decoder);
        Ok(Self)
    }
}

#[test]
fn swallowed_budget_errors_still_fail() {
    for &format in SerializationFormat::ALL {
        let bytes = encode(&vec![(); 8], format).unwrap();
        assert_limit::<SwallowedError, 4, 8>(&bytes, format);
    }
}

struct MessageError;

impl<'de> Deserialize<'de> for MessageError {
    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(de::Error::custom("snapshot decoder step limit exceeded"))
    }
}

#[test]
fn arbitrary_limit_looking_error_messages_remain_decode_errors() {
    for &format in SerializationFormat::ALL {
        let bytes = encode(&(), format).unwrap();
        assert!(matches!(
            decode_with_limits::<MessageError, 8, 8>(&bytes, format),
            Err(SerializationError::Decode(_))
        ));
    }
}

#[test]
fn trailing_data_is_checked_only_after_a_successful_value() {
    for &format in SerializationFormat::ALL {
        let mut bytes = encode(&vec![(); 8], format).unwrap();
        bytes.push(b'!');
        assert_limit::<Vec<()>, 4, 8>(&bytes, format);
        assert!(matches!(
            decode_with_limits::<Vec<()>, 100, 8>(&bytes, format),
            Err(SerializationError::Decode(_))
        ));
    }
}

struct NoAllocationHint;

impl<'de> Deserialize<'de> for NoAllocationHint {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct Elements;
        impl<'de> Visitor<'de> for Elements {
            type Value = NoAllocationHint;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a sequence without an allocation hint")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                assert_eq!(sequence.size_hint(), None);
                while sequence.next_element::<IgnoredAny>()?.is_some() {}
                Ok(NoAllocationHint)
            }
        }
        decoder.deserialize_seq(Elements)
    }
}

#[test]
fn safe_wire_lengths_do_not_become_allocation_hints() {
    let bytes = encode(&vec![(); 8], SerializationFormat::Cbor).unwrap();
    assert!(
        decode_with_limits::<NoAllocationHint, 9, 8>(&bytes, SerializationFormat::Cbor).is_ok()
    );
}
