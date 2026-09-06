//! Result types for the Node.js binding.

use napi_derive::napi;

/// Why the engine stopped executing.
#[napi]
pub enum HaltReason {
    /// The agenda was empty.
    AgendaEmpty = 0,
    /// The run limit was reached.
    LimitReached = 1,
    /// A halt was explicitly requested.
    HaltRequested = 2,
    /// Evaluation of the current activation's actions failed.
    ActionError = 3,
}

impl From<ferric_rules_runtime::HaltReason> for HaltReason {
    fn from(hr: ferric_rules_runtime::HaltReason) -> Self {
        match hr {
            ferric_rules_runtime::HaltReason::AgendaEmpty => Self::AgendaEmpty,
            ferric_rules_runtime::HaltReason::LimitReached => Self::LimitReached,
            ferric_rules_runtime::HaltReason::HaltRequested => Self::HaltRequested,
            ferric_rules_runtime::HaltReason::ActionError => Self::ActionError,
        }
    }
}

/// Result of a `run()` call.
#[napi(object)]
pub struct RunResult {
    /// Number of rules fired during this run.
    pub rules_fired: f64,
    /// Why execution stopped.
    pub halt_reason: HaltReason,
}

/// JavaScript counts use exact safe-integer numbers. Reject an unrepresentable
/// count rather than narrowing or silently rounding native progress.
pub fn checked_count(count: usize, name: &str) -> napi::Result<f64> {
    const MAX_SAFE: u64 = (1_u64 << 53) - 1;
    if u64::try_from(count).map_or(true, |count| count > MAX_SAFE) {
        return Err(napi::Error::from_reason(format!(
            "FerricRuntimeError: {name} exceeds JavaScript safe integer range"
        )));
    }
    #[allow(clippy::cast_precision_loss)]
    Ok(count as f64)
}

impl TryFrom<ferric_rules_runtime::RunResult> for RunResult {
    type Error = napi::Error;

    fn try_from(rr: ferric_rules_runtime::RunResult) -> napi::Result<Self> {
        Ok(Self {
            rules_fired: checked_count(rr.rules_fired, "fired count")?,
            halt_reason: rr.halt_reason.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::checked_count;

    #[test]
    fn fired_counts_are_checked_without_narrowing() {
        for (count, expected) in [
            (u32::MAX as usize, 4_294_967_295.0),
            (u32::MAX as usize + 1, 4_294_967_296.0),
            ((1_usize << 53) - 1, 9_007_199_254_740_991.0),
        ] {
            assert_eq!(
                checked_count(count, "test count").unwrap().to_bits(),
                f64::to_bits(expected)
            );
        }
        assert!(checked_count(1_usize << 53, "test count").is_err());
        assert!(checked_count(usize::MAX, "test count").is_err());
    }
}

/// Information about a single rule that fired.
#[napi(object)]
pub struct FiredRule {
    /// The name of the rule that fired.
    pub rule_name: String,
}

/// Summary information about a registered rule.
#[napi(object)]
pub struct RuleInfo {
    /// Rule name.
    pub name: String,
    /// Rule salience (priority).
    pub salience: i32,
}
