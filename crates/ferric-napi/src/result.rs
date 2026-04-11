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
}

impl From<ferric_runtime::HaltReason> for HaltReason {
    fn from(hr: ferric_runtime::HaltReason) -> Self {
        match hr {
            ferric_runtime::HaltReason::AgendaEmpty => Self::AgendaEmpty,
            ferric_runtime::HaltReason::LimitReached => Self::LimitReached,
            ferric_runtime::HaltReason::HaltRequested => Self::HaltRequested,
        }
    }
}

/// Result of a `run()` call.
#[napi(object)]
pub struct RunResult {
    /// Number of rules fired during this run.
    pub rules_fired: u32,
    /// Why execution stopped.
    pub halt_reason: HaltReason,
}

impl From<ferric_runtime::RunResult> for RunResult {
    fn from(rr: ferric_runtime::RunResult) -> Self {
        Self {
            #[allow(clippy::cast_possible_truncation)]
            rules_fired: rr.rules_fired as u32,
            halt_reason: rr.halt_reason.into(),
        }
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
