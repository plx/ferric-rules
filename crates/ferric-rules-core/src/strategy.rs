//! Conflict resolution strategies for the agenda.
//!
//! Depth and breadth provide the supported CLIPS ordering contract, with
//! salience taking precedence over activation chronology. The retained Lex and
//! Mea variants are experimental Ferric orderings; they do not implement CLIPS
//! sorted-recency and specificity tie-breaking. Use depth/breadth for portable
//! rules. Simplicity, complexity and random strategies are not implemented.

/// Conflict resolution strategies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ConflictResolutionStrategy {
    #[default]
    Depth,
    Breadth,
    /// Experimental Ferric ordering, not CLIPS LEX compatibility.
    Lex,
    /// Experimental Ferric ordering, not CLIPS MEA compatibility.
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
