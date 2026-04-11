//! Node.js native addon for the Ferric rules engine via napi-rs.

#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::new_without_default)]
#![allow(clippy::used_underscore_binding)]
#![allow(clippy::needless_pass_by_value)]

pub mod config;
pub mod engine;
pub mod error;
pub mod fact;
pub mod result;
pub mod value;
