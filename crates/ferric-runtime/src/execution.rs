//! Engine execution loop: run, step, halt, and reset.
//!
//! The execution loop pops activations from the agenda and fires them,
//! executing RHS actions that may assert/retract/modify facts and trigger
//! further Rete propagation.

use ferric_core::beta::RuleId;
use ferric_core::token::TokenId;

/// Run limit for the execution loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunLimit {
    /// Run until the agenda is empty or halt is requested.
    Unlimited,
    /// Run at most N rule firings.
    Count(usize),
}

/// Reason execution stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HaltReason {
    /// The agenda was empty (no more rules to fire).
    AgendaEmpty,
    /// The run limit was reached.
    LimitReached,
    /// A halt was requested.
    HaltRequested,
}

/// Result of an execution run.
#[derive(Clone, Copy, Debug)]
pub struct RunResult {
    /// Number of rules fired during this run.
    pub rules_fired: usize,
    /// Why execution stopped.
    pub halt_reason: HaltReason,
}

/// Information about a fired rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiredRule {
    /// The rule that was fired.
    pub rule_id: RuleId,
    /// The token (fact combination) that triggered the rule.
    pub token_id: TokenId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_limit_variants() {
        let _ = RunLimit::Unlimited;
        let _ = RunLimit::Count(5);
    }

    #[test]
    fn halt_reason_variants() {
        let _ = HaltReason::AgendaEmpty;
        let _ = HaltReason::LimitReached;
        let _ = HaltReason::HaltRequested;
    }

    #[test]
    fn run_result_default() {
        let result = RunResult {
            rules_fired: 0,
            halt_reason: HaltReason::AgendaEmpty,
        };
        assert_eq!(result.rules_fired, 0);
    }

    #[test]
    fn fired_rule_equality() {
        let a = FiredRule {
            rule_id: RuleId(1),
            token_id: TokenId::default(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
