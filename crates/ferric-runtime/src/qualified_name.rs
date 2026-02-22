//! Module-qualified name parsing and validation.
//!
//! CLIPS uses `MODULE::name` syntax for cross-module references.
//! This module provides shared utilities for splitting, validating,
//! and resolving qualified names throughout the runtime.

use std::str::FromStr;

/// A parsed module-qualified name.
///
/// Names can be either unqualified (`name`) or qualified (`MODULE::name`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QualifiedName {
    /// An unqualified name (e.g., `reading`, `my-func`).
    Unqualified(String),
    /// A module-qualified name (e.g., `SENSOR::reading`).
    Qualified {
        /// The module name (e.g., `SENSOR`).
        module: String,
        /// The local name within the module (e.g., `reading`).
        name: String,
    },
}

impl QualifiedName {
    /// Get the local (unqualified) name.
    #[must_use]
    pub fn local_name(&self) -> &str {
        match self {
            Self::Unqualified(name) | Self::Qualified { name, .. } => name,
        }
    }

    /// Get the module name, if qualified.
    #[must_use]
    pub fn module_name(&self) -> Option<&str> {
        match self {
            Self::Unqualified(_) => None,
            Self::Qualified { module, .. } => Some(module),
        }
    }

    /// Check if this is a qualified name.
    #[must_use]
    pub fn is_qualified(&self) -> bool {
        matches!(self, Self::Qualified { .. })
    }
}

impl std::fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unqualified(name) => write!(f, "{name}"),
            Self::Qualified { module, name } => write!(f, "{module}::{name}"),
        }
    }
}

impl FromStr for QualifiedName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((module, name)) = s.split_once("::") else {
            return Ok(Self::Unqualified(s.to_string()));
        };

        if module.is_empty() {
            return Err(format!("malformed qualified name `{s}`: empty module name"));
        }

        if name.contains("::") {
            return Err(format!(
                "malformed qualified name `{s}`: multiple `::` separators"
            ));
        }

        if name.is_empty() {
            return Err(format!(
                "malformed qualified name `{s}`: empty name after `{module}::`"
            ));
        }

        Ok(Self::Qualified {
            module: module.to_string(),
            name: name.to_string(),
        })
    }
}

impl TryFrom<&str> for QualifiedName {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

/// Parse a name string into a [`QualifiedName`].
///
/// Recognizes `MODULE::name` syntax. The `::` must appear exactly once.
/// Empty module or name parts are invalid.
///
/// # Errors
///
/// Returns a descriptive error string for:
/// - Empty module name (`::name`)
/// - Empty local name (`MODULE::`)
/// - Multiple `::` separators (`A::B::C`)
pub fn parse_qualified_name(s: &str) -> Result<QualifiedName, String> {
    s.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_unqualified_name() {
        let result = parse_qualified_name("reading").unwrap();
        assert_eq!(result, QualifiedName::Unqualified("reading".to_string()));
        assert_eq!(result.local_name(), "reading");
        assert_eq!(result.module_name(), None);
        assert!(!result.is_qualified());
    }

    #[test]
    fn parse_qualified_name_basic() {
        let result = parse_qualified_name("SENSOR::reading").unwrap();
        assert_eq!(
            result,
            QualifiedName::Qualified {
                module: "SENSOR".to_string(),
                name: "reading".to_string(),
            }
        );
        assert_eq!(result.local_name(), "reading");
        assert_eq!(result.module_name(), Some("SENSOR"));
        assert!(result.is_qualified());
    }

    #[test]
    fn parse_qualified_name_main() {
        let result = parse_qualified_name("MAIN::person").unwrap();
        assert_eq!(
            result,
            QualifiedName::Qualified {
                module: "MAIN".to_string(),
                name: "person".to_string(),
            }
        );
    }

    #[test]
    fn parse_qualified_name_with_hyphens() {
        let result = parse_qualified_name("MY-MODULE::my-func").unwrap();
        assert_eq!(
            result,
            QualifiedName::Qualified {
                module: "MY-MODULE".to_string(),
                name: "my-func".to_string(),
            }
        );
    }

    #[test]
    fn parse_empty_module_name_is_error() {
        let result = parse_qualified_name("::name");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty module name"));
    }

    #[test]
    fn parse_empty_local_name_is_error() {
        let result = parse_qualified_name("MODULE::");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty name after"));
    }

    #[test]
    fn parse_multiple_separators_is_error() {
        let result = parse_qualified_name("A::B::C");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("multiple `::` separators"));
    }

    #[test]
    fn display_unqualified() {
        let name = QualifiedName::Unqualified("reading".to_string());
        assert_eq!(format!("{name}"), "reading");
    }

    #[test]
    fn display_qualified() {
        let name = QualifiedName::Qualified {
            module: "SENSOR".to_string(),
            name: "reading".to_string(),
        };
        assert_eq!(format!("{name}"), "SENSOR::reading");
    }

    #[test]
    fn parse_simple_names_without_colons() {
        // Various simple names that should parse as unqualified
        for name in &["foo", "my-func", "str-cat", "+", ">=", "person*"] {
            let result = parse_qualified_name(name).unwrap();
            assert!(!result.is_qualified(), "expected unqualified for {name}");
        }
    }

    #[test]
    fn parse_single_colon_not_qualified() {
        // A single colon (not `::`) should not trigger qualified parsing.
        // In practice this won't appear because the lexer separates `:` from symbols,
        // but the utility should handle it gracefully if it ever comes through.
        let result = parse_qualified_name("foo:bar").unwrap();
        // Treated as unqualified since there's no `::` separator
        assert!(!result.is_qualified());
    }

    #[test]
    fn from_str_parses_qualified_names() {
        let parsed: QualifiedName = "MAIN::person".parse().unwrap();
        assert_eq!(parsed.module_name(), Some("MAIN"));
        assert_eq!(parsed.local_name(), "person");
    }

    #[test]
    fn try_from_str_parses_unqualified_names() {
        let parsed = QualifiedName::try_from("person").unwrap();
        assert!(!parsed.is_qualified());
        assert_eq!(parsed.local_name(), "person");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// `parse_qualified_name` never panics on arbitrary input.
        #[test]
        fn parse_never_panics(s in "[A-Za-z0-9:_-]{0,50}") {
            let _ = parse_qualified_name(&s);
        }

        /// Valid `module::name` forms always parse successfully as qualified.
        #[test]
        fn valid_qualified_always_parses(
            module in "[A-Z][A-Z0-9_-]{0,10}",
            name in "[a-z][a-z0-9_-]{0,10}",
        ) {
            let input = format!("{module}::{name}");
            let result = parse_qualified_name(&input).unwrap();
            prop_assert!(result.is_qualified());
            prop_assert_eq!(result.module_name(), Some(module.as_str()));
            prop_assert_eq!(result.local_name(), name.as_str());
        }

        /// Names without `::` always parse as unqualified.
        #[test]
        fn no_colons_always_unqualified(name in "[a-zA-Z][a-zA-Z0-9_-]{0,15}") {
            let result = parse_qualified_name(&name).unwrap();
            prop_assert!(!result.is_qualified());
            prop_assert_eq!(result.local_name(), name.as_str());
        }

        /// Roundtrip: parse then display produces the original string.
        #[test]
        fn display_roundtrip(
            module in "[A-Z][A-Z0-9]{0,8}",
            name in "[a-z][a-z0-9]{0,8}",
        ) {
            let input = format!("{module}::{name}");
            let parsed = parse_qualified_name(&input).unwrap();
            prop_assert_eq!(format!("{parsed}"), input);
        }
    }
}
