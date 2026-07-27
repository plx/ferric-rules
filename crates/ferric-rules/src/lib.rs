//! # Ferric
//!
//! A CLIPS-inspired forward-chaining rules engine implemented in Rust.
//!
//! This is the public facade crate that re-exports types from the internal
//! crates (`ferric-rules-core`, `ferric-rules-parser`, `ferric-rules-runtime`).

pub use ferric_rules_core as core;
pub use ferric_rules_parser as parser;
pub use ferric_rules_runtime as runtime;

#[cfg(test)]
mod tests {
    #[test]
    fn smoke_test_facade_reexports() {
        // Verify that the re-exported crate modules are accessible.
        let _ = std::any::type_name::<super::core::Value>();
        let _ = std::any::type_name::<super::parser::FileId>();
        let _ = std::any::type_name::<super::runtime::Engine>();
    }
}
