//! Module-qualified name parsing and validation.
//!
//! CLIPS uses `MODULE::name` syntax for cross-module references.

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
    use proptest::prelude::*;

    proptest! {
        /// Parse-display roundtrip: display then re-parse always recovers the
        /// original `QualifiedName`.
        #[test]
        fn display_parse_roundtrip(
            module in "[A-Z][A-Z0-9_]{0,10}",
            name in "[a-z][a-z0-9_]{0,10}",
        ) {
            let qualified = QualifiedName::Qualified {
                module: module.clone(),
                name: name.clone(),
            };
            let displayed = qualified.to_string();
            let reparsed: QualifiedName = displayed.parse().unwrap();
            prop_assert_eq!(&reparsed, &qualified);
        }

        /// Unqualified names roundtrip through display-parse.
        #[test]
        fn unqualified_display_roundtrip(name in "[a-zA-Z][a-zA-Z0-9_]{0,15}") {
            let original = QualifiedName::Unqualified(name.clone());
            let displayed = original.to_string();
            let reparsed: QualifiedName = displayed.parse().unwrap();
            prop_assert_eq!(&reparsed, &original);
        }

        /// `is_qualified` is consistent with `module_name`.
        #[test]
        fn is_qualified_consistent_with_module_name(
            module in "[A-Z][A-Z0-9]{0,5}",
            name in "[a-z][a-z0-9]{0,5}",
        ) {
            let q = QualifiedName::Qualified { module, name };
            prop_assert!(q.is_qualified());
            prop_assert!(q.module_name().is_some());

            let u = QualifiedName::Unqualified("foo".to_string());
            prop_assert!(!u.is_qualified());
            prop_assert!(u.module_name().is_none());
        }

        /// Empty module part always rejected.
        #[test]
        fn empty_module_rejected(name in "[a-z][a-z0-9]{0,10}") {
            let input = format!("::{name}");
            prop_assert!(parse_qualified_name(&input).is_err());
        }

        /// Empty name part always rejected.
        #[test]
        fn empty_name_rejected(module in "[A-Z][A-Z0-9]{0,10}") {
            let input = format!("{module}::");
            prop_assert!(parse_qualified_name(&input).is_err());
        }

        /// Multiple `::` separators always rejected.
        #[test]
        fn multiple_separators_rejected(
            a in "[A-Z]{1,5}",
            b in "[A-Z]{1,5}",
            c in "[a-z]{1,5}",
        ) {
            let input = format!("{a}::{b}::{c}");
            prop_assert!(parse_qualified_name(&input).is_err());
        }
    }
}
