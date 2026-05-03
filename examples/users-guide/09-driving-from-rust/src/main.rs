//! Users-guide example 09: driving the engine from Rust.
//!
//! Source: docs/users-guide.md §10.
//!
//! The guide shows a Classifier sketch with placeholder Request/Decision
//! types. This example fills them in with concrete shapes so the whole
//! thing compiles and runs end-to-end.

use ferric::core::Value;
use ferric::runtime::{Engine, RunLimit};

#[derive(Debug)]
pub struct Request {
    pub tier: String,
    pub session_count: i64,
    pub has_crashed: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub enum Decision {
    Premium,
    Warn,
    Standard,
    #[default]
    Unknown,
}

pub struct Classifier {
    engine: Engine,
}

impl Classifier {
    pub fn new(rules: &str) -> anyhow::Result<Self> {
        Ok(Self {
            engine: Engine::with_rules(rules)?,
        })
    }

    pub fn classify(&mut self, req: &Request) -> anyhow::Result<Decision> {
        // Start from a clean slate: reset reapplies deffacts + initial-fact.
        self.engine.reset()?;

        // Project the request onto facts. Note that `symbol_value` and
        // `assert_ordered` both borrow the engine mutably, so bind first.
        let tier = self.engine.symbol_value(&req.tier)?;
        self.engine.assert_ordered("user-tier", tier)?;
        self.engine
            .assert_ordered("session-count", req.session_count)?;
        if req.has_crashed {
            self.engine.assert_ordered_symbol("has-crashed", "yes")?;
        }

        // Bounded run — production handlers should always cap iterations.
        let _result = self.engine.run(RunLimit::Count(1_000))?;

        // Pick up the decision either from a fact or a printout channel.
        let decision = self.read_decision()?;

        // Surface non-fatal rule warnings if you want to log them.
        for diag in self.engine.action_diagnostics() {
            eprintln!("rule warning: {diag:?}");
        }
        self.engine.clear_action_diagnostics();
        self.engine.clear_output_channel("t");

        Ok(decision)
    }

    fn read_decision(&self) -> anyhow::Result<Decision> {
        for (_, fact) in self.engine.find_facts("decision")? {
            if let ferric::core::Fact::Ordered(of) = fact {
                if let Some(Value::Symbol(sym)) = of.fields.first() {
                    if let Some(name) = self.engine.resolve_symbol(*sym) {
                        return Ok(match name {
                            "premium" => Decision::Premium,
                            "warn" => Decision::Warn,
                            "standard" => Decision::Standard,
                            _ => Decision::Unknown,
                        });
                    }
                }
            }
        }
        Ok(Decision::default())
    }
}

fn main() -> anyhow::Result<()> {
    let rules = include_str!("../rules/classifier.clp");
    let mut classifier = Classifier::new(rules)?;

    let cases = [
        Request {
            tier: "vip".into(),
            session_count: 200,
            has_crashed: false,
        },
        Request {
            tier: "regular".into(),
            session_count: 5,
            has_crashed: true,
        },
        Request {
            tier: "regular".into(),
            session_count: 5,
            has_crashed: false,
        },
    ];

    let expected = [Decision::Premium, Decision::Warn, Decision::Standard];

    for (req, want) in cases.iter().zip(expected.iter()) {
        let got = classifier.classify(req)?;
        println!("{req:?} → {got:?}");
        assert_eq!(&got, want);
    }
    Ok(())
}
