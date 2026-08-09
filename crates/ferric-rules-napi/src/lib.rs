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

use napi_derive::napi;

pub mod config;
pub mod engine;
pub mod error;
pub mod fact;
pub mod result;
pub mod value;

/// Return the version embedded in this native addon.
///
/// The npm loader compares this value with the JavaScript and platform-package
/// versions before exposing the binding. This makes it impossible to silently
/// combine release artifacts from different Ferric versions.
#[napi]
#[must_use]
pub fn native_package_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Continue one bounded chunk for the internal Node worker batching loop.
///
/// This is a module-level bridge rather than an `Engine` prototype method so
/// the public synchronous API cannot continue a completed or canceled logical
/// run without the documented fresh-run reset.
#[doc(hidden)]
#[napi(js_name = "__continueRun", skip_typescript)]
pub fn continue_run(
    mut engine: napi::bindgen_prelude::ClassInstance<engine::Engine>,
    limit: u32,
) -> napi::Result<result::RunResult> {
    engine.continue_run(limit)
}
