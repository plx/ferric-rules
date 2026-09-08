//! Bracket-prefixed source tokens follow CLIPS 6.30 `ScanSymbol`.
//! Names retain their value tag without creating COOL instances. The bounded
//! reference programs include question marks, backslashes, Unicode whitespace,
//! punctuation and delimiters; no runtime scanner builtin is used here.

use ferric_rules::core::{Fact, Value};
use ferric_rules::runtime::{Engine, RunLimit};

#[test]
fn source_bracket_names_preserve_full_continuation_spellings() {
    for (source, expected) in [
        ("[a?b]", "a?b"),
        (r"[a\b]", r"a\b"),
        (r"[a\]", "a\\"),
        ("[a\u{a0}b]", "a\u{a0}b"),
        ("[a\u{85}b]", "a\u{85}b"),
        ("[a\u{2003}b]", "a\u{2003}b"),
        ("[a🙂b]", "a🙂b"),
        ("[a,b:DATA::x]", "a,b:DATA::x"),
        ("[a][b]", "a][b"),
        ("[[]]", "[]"),
    ] {
        let mut engine = Engine::with_rules(&format!(
            "(deffacts input (spelling {source}))
             (defrule match (spelling {source}) => (printout t (instance-namep {source})))"
        ))
        .unwrap_or_else(|error| panic!("{source:?}: {error:?}"));
        let facts = engine.find_facts("spelling").unwrap();
        assert_eq!(facts.len(), 1);
        let Fact::Ordered(fact) = facts[0].1 else {
            panic!("ordered spelling")
        };
        let [Value::InstanceName(name)] = fact.fields.as_slice() else {
            panic!("typed name for {source:?}")
        };
        assert_eq!(
            engine.resolve_core_symbol_bytes(name.as_symbol()),
            Some(expected.as_bytes())
        );
        assert_eq!(
            engine.run(RunLimit::Unlimited).unwrap().rules_fired,
            1,
            "{source:?}"
        );
        assert!(engine.action_diagnostics().is_empty());
        assert_eq!(engine.get_output("t").unwrap(), Some("TRUE"));
    }
}

#[test]
fn less_than_and_unmatched_brackets_remain_separate_symbol_data() {
    let mut engine = Engine::with_rules(
        "(deffacts input (split [a<b]) (ordinary [] [ [open close] [x]tail))
         (defrule match (split [a <b]) => (printout t matched))",
    )
    .unwrap();
    for (relation, expected) in [
        ("split", vec!["[a", "<b]"]),
        ("ordinary", vec!["[]", "[", "[open", "close]", "[x]tail"]),
    ] {
        let facts = engine.find_facts(relation).unwrap();
        assert_eq!(facts.len(), 1);
        let Fact::Ordered(fact) = facts[0].1 else {
            panic!("ordered symbols")
        };
        assert_eq!(fact.fields.len(), expected.len());
        for (value, expected) in fact.fields.iter().zip(expected) {
            let Value::Symbol(symbol) = value else {
                panic!("SYMBOL {expected}")
            };
            assert_eq!(
                engine.resolve_core_symbol_bytes(*symbol),
                Some(expected.as_bytes())
            );
        }
    }
    assert_eq!(engine.run(RunLimit::Unlimited).unwrap().rules_fired, 1);
    assert!(engine.action_diagnostics().is_empty());
    assert_eq!(engine.get_output("t").unwrap(), Some("matched"));
}
