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
