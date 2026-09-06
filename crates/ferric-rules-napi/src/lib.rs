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

// napi_define_class installs a native receiver check on this function. Moving
// that exact function off the prototype retains the check; a module-level
// FromNapiRef/ ClassInstance argument would instead cast any native wrap to
// Engine without checking its Rust type. No public Engine exposes continuation.
// Rust unit tests have no Node environment to register; addon tests exercise
// this hook in real Node processes.
#[cfg(not(test))]
#[napi_derive::module_exports]
fn private_worker_bridge(mut exports: napi::JsObject) -> napi::Result<()> {
    let constructor: napi::JsFunction = exports.get_named_property("Engine")?;
    let mut prototype: napi::JsObject = constructor
        .coerce_to_object()?
        .get_named_property("prototype")?;
    let continuation: napi::JsFunction = prototype.get_named_property("__continueRun")?;
    if !prototype.delete_named_property("__continueRun")? {
        return Err(napi::Error::from_reason(
            "could not hide worker continuation method",
        ));
    }
    exports.set_named_property("__continueRun", continuation)
}
