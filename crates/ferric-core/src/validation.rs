//! Pattern restriction validation (compile-time).
//!
//! Validates pattern restrictions at compile time according to Section 7.7
//! of the CLIPS specification. Reports stable error codes `E0001`–`E0005`
//! with source spans for diagnostics.
//!
//! ## Phase 2 implementation plan
//!
//! - Pass 011: Pattern validation and source-located compile errors

use std::fmt;

// ============================================================================
// Source Location
// ============================================================================

/// Source location for validation errors.
///
/// Simplified source location representation that doesn't depend on the parser
/// crate. The runtime layer converts `ferric_parser::Span` to this type when
/// creating validation errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    /// Starting line (1-indexed)
    pub line: u32,
    /// Starting column (1-indexed)
    pub column: u32,
    /// Ending line (1-indexed)
    pub end_line: u32,
    /// Ending column (1-indexed)
    pub end_column: u32,
}

impl SourceLocation {
    /// Create a new source location.
    #[must_use]
    pub fn new(line: u32, column: u32, end_line: u32, end_column: u32) -> Self {
        Self {
            line,
            column,
            end_line,
            end_column,
        }
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == self.end_line {
            write!(f, "{}:{}-{}", self.line, self.column, self.end_column)
        } else {
            write!(
                f,
                "{}:{}-{}:{}",
                self.line, self.column, self.end_line, self.end_column
            )
        }
    }
}

// ============================================================================
// Validation Stage
// ============================================================================

/// Where in the pipeline a validation error was detected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationStage {
    /// Detected during Stage 2 AST interpretation.
    AstInterpretation,
    /// Detected during Rete compilation.
    ReteCompilation,
}

impl fmt::Display for ValidationStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AstInterpretation => write!(f, "AST interpretation"),
            Self::ReteCompilation => write!(f, "Rete compilation"),
        }
    }
}

// ============================================================================
// Pattern Violation
// ============================================================================

/// A pattern restriction violation.
///
/// Each variant corresponds to a stable error code (E0001–E0005) for
/// consistent diagnostics across releases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternViolation {
    /// E0001: Nesting depth exceeded (e.g., `(not (exists (not ...)))`)
    NestingTooDeep {
        /// Actual nesting depth encountered
        depth: usize,
        /// Maximum allowed depth
        max: usize,
    },
    /// E0002: Forall condition is not a single fact pattern
    ForallConditionNotSinglePattern,
    /// E0003: Nested forall
    NestedForall,
    /// E0004: Unbound variable in forall <then> clause
    ForallUnboundVariable {
        /// Name of the unbound variable
        var_name: String,
    },
    /// E0005: Unsupported nesting combination (e.g., `(exists (not ...))`)
    UnsupportedNestingCombination {
        /// Human-readable description of the violation
        description: String,
    },
}

impl PatternViolation {
    /// Returns the stable error code for this violation.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NestingTooDeep { .. } => "E0001",
            Self::ForallConditionNotSinglePattern => "E0002",
            Self::NestedForall => "E0003",
            Self::ForallUnboundVariable { .. } => "E0004",
            Self::UnsupportedNestingCombination { .. } => "E0005",
        }
    }

    /// Returns a suggested fix for this violation (if available).
    #[must_use]
    pub fn suggestion(&self) -> Option<String> {
        match self {
            Self::NestingTooDeep { max, .. } => Some(format!(
                "reduce nesting depth to {max} or fewer levels"
            )),
            Self::ForallConditionNotSinglePattern => {
                Some("forall condition must be a single fact pattern".to_string())
            }
            Self::NestedForall => Some("forall cannot be nested inside another forall".to_string()),
            Self::ForallUnboundVariable { var_name } => Some(format!(
                "bind variable {var_name} in the condition pattern before using it"
            )),
            Self::UnsupportedNestingCombination { .. } => {
                Some("try restructuring with supported nesting patterns".to_string())
            }
        }
    }
}

impl fmt::Display for PatternViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NestingTooDeep { depth, max } => {
                write!(
                    f,
                    "nesting depth {depth} exceeds maximum of {max}",
                )
            }
            Self::ForallConditionNotSinglePattern => {
                write!(f, "forall condition must be a single fact pattern")
            }
            Self::NestedForall => {
                write!(f, "forall cannot be nested inside another forall")
            }
            Self::ForallUnboundVariable { var_name } => {
                write!(f, "unbound variable {var_name} in forall action clause")
            }
            Self::UnsupportedNestingCombination { description } => {
                write!(f, "unsupported nesting: {description}")
            }
        }
    }
}

// ============================================================================
// Pattern Validation Error
// ============================================================================

/// A pattern validation error with source location and diagnostic information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternValidationError {
    /// Stable machine-readable error code (E0001-E0005)
    pub code: &'static str,
    /// What restriction was violated
    pub kind: PatternViolation,
    /// Source location (if available from parser)
    pub location: Option<SourceLocation>,
    /// Pipeline stage where detected
    pub stage: ValidationStage,
    /// Suggested fix (human-readable)
    pub suggestion: Option<String>,
}

impl PatternValidationError {
    /// Create a new validation error.
    #[must_use]
    pub fn new(
        kind: PatternViolation,
        location: Option<SourceLocation>,
        stage: ValidationStage,
    ) -> Self {
        let code = kind.code();
        let suggestion = kind.suggestion();
        Self {
            code,
            kind,
            location,
            stage,
            suggestion,
        }
    }
}

impl fmt::Display for PatternValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.kind)?;
        if let Some(loc) = self.location {
            write!(f, " at {loc}")?;
        }
        write!(f, " ({})", self.stage)?;
        if let Some(suggestion) = &self.suggestion {
            write!(f, " — suggestion: {suggestion}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PatternValidationError {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_location_display_single_line() {
        let loc = SourceLocation::new(10, 5, 10, 15);
        assert_eq!(format!("{}", loc), "10:5-15");
    }

    #[test]
    fn source_location_display_multi_line() {
        let loc = SourceLocation::new(10, 5, 12, 20);
        assert_eq!(format!("{}", loc), "10:5-12:20");
    }

    #[test]
    fn validation_stage_display() {
        assert_eq!(
            format!("{}", ValidationStage::AstInterpretation),
            "AST interpretation"
        );
        assert_eq!(
            format!("{}", ValidationStage::ReteCompilation),
            "Rete compilation"
        );
    }

    #[test]
    fn pattern_violation_nesting_too_deep() {
        let violation = PatternViolation::NestingTooDeep { depth: 3, max: 2 };
        assert_eq!(violation.code(), "E0001");
        assert_eq!(
            format!("{}", violation),
            "nesting depth 3 exceeds maximum of 2"
        );
        assert!(violation.suggestion().is_some());
    }

    #[test]
    fn pattern_violation_forall_condition_not_single_pattern() {
        let violation = PatternViolation::ForallConditionNotSinglePattern;
        assert_eq!(violation.code(), "E0002");
        assert_eq!(
            format!("{}", violation),
            "forall condition must be a single fact pattern"
        );
        assert!(violation.suggestion().is_some());
    }

    #[test]
    fn pattern_violation_nested_forall() {
        let violation = PatternViolation::NestedForall;
        assert_eq!(violation.code(), "E0003");
        assert_eq!(
            format!("{}", violation),
            "forall cannot be nested inside another forall"
        );
        assert!(violation.suggestion().is_some());
    }

    #[test]
    fn pattern_violation_forall_unbound_variable() {
        let violation = PatternViolation::ForallUnboundVariable {
            var_name: "x".to_string(),
        };
        assert_eq!(violation.code(), "E0004");
        assert_eq!(
            format!("{}", violation),
            "unbound variable x in forall action clause"
        );
        assert!(violation.suggestion().is_some());
    }

    #[test]
    fn pattern_violation_unsupported_nesting_combination() {
        let violation = PatternViolation::UnsupportedNestingCombination {
            description: "exists containing not".to_string(),
        };
        assert_eq!(violation.code(), "E0005");
        assert_eq!(
            format!("{}", violation),
            "unsupported nesting: exists containing not"
        );
        assert!(violation.suggestion().is_some());
    }

    #[test]
    fn pattern_validation_error_display_with_location() {
        let kind = PatternViolation::NestingTooDeep { depth: 3, max: 2 };
        let location = Some(SourceLocation::new(5, 10, 5, 30));
        let error = PatternValidationError::new(kind, location, ValidationStage::ReteCompilation);

        let display = format!("{}", error);
        assert!(display.contains("[E0001]"));
        assert!(display.contains("nesting depth 3 exceeds maximum of 2"));
        assert!(display.contains("5:10-30"));
        assert!(display.contains("Rete compilation"));
        assert!(display.contains("suggestion"));
    }

    #[test]
    fn pattern_validation_error_display_without_location() {
        let kind = PatternViolation::NestedForall;
        let error = PatternValidationError::new(kind, None, ValidationStage::AstInterpretation);

        let display = format!("{}", error);
        assert!(display.contains("[E0003]"));
        assert!(display.contains("forall cannot be nested"));
        assert!(display.contains("AST interpretation"));
    }

    #[test]
    fn pattern_validation_error_construction() {
        let kind = PatternViolation::UnsupportedNestingCombination {
            description: "test description".to_string(),
        };
        let loc = SourceLocation::new(1, 1, 1, 10);
        let error = PatternValidationError::new(
            kind.clone(),
            Some(loc),
            ValidationStage::ReteCompilation,
        );

        assert_eq!(error.code, "E0005");
        assert_eq!(error.kind, kind);
        assert_eq!(error.location, Some(loc));
        assert_eq!(error.stage, ValidationStage::ReteCompilation);
        assert!(error.suggestion.is_some());
    }
}
