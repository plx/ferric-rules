//! Conflict resolution strategies for the agenda.
//!
//! Salience takes precedence for every strategy. Depth and breadth compare
//! activation chronology. LEX compares sorted fact recencies, rule specificity,
//! then older activations; MEA first compares the initial pattern recency.
//! Simplicity, complexity and random strategies are not implemented.

/// Conflict resolution strategies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ConflictResolutionStrategy {
    #[default]
    Depth,
    Breadth,
    /// CLIPS LEX: sorted fact recencies, specificity, then older activations.
    Lex,
    /// CLIPS MEA: first-pattern recency, then the LEX comparison.
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
