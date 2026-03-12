//! Conflict resolution strategies for the agenda.
//!
//! CLIPS supports four conflict resolution strategies that determine the order
//! in which activations are fired:
//!
//! - **Depth** (default): Higher salience first, then most recent first.
//! - **Breadth**: Higher salience first, then least recent first.
//! - **LEX**: Higher salience first, then lexicographic comparison of fact
//!   recency (most recent fact in token compared first).
//! - **MEA**: Higher salience first, then the recency of the first pattern's
//!   fact determines order.
//!
//! Phase 1 implements depth only. Phase 2 adds all four strategies with
//! stable total ordering.
//!
//! ## Phase 2 implementation plan
//!
//! - Pass 007: Agenda conflict strategies and ordering contract

/// Conflict resolution strategies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ConflictResolutionStrategy {
    #[default]
    Depth,
    Breadth,
    Lex,
    Mea,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_strategy_is_depth() {
        assert_eq!(
            ConflictResolutionStrategy::default(),
            ConflictResolutionStrategy::Depth
        );
    }

    #[test]
    fn strategy_variants_exist() {
        let _ = ConflictResolutionStrategy::Depth;
        let _ = ConflictResolutionStrategy::Breadth;
        let _ = ConflictResolutionStrategy::Lex;
        let _ = ConflictResolutionStrategy::Mea;
    }

    #[test]
    fn strategies_are_comparable() {
        assert_eq!(
            ConflictResolutionStrategy::Depth,
            ConflictResolutionStrategy::Depth
        );
        assert_ne!(
            ConflictResolutionStrategy::Depth,
            ConflictResolutionStrategy::Breadth
        );
    }

    #[test]
    fn strategies_are_cloneable() {
        let s1 = ConflictResolutionStrategy::Lex;
        let s2 = s1;
        assert_eq!(s1, s2);
    }
}
