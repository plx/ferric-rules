//! FFI contract test suite.
//!
//! Test modules organized by FFI surface area:
//! - `error_model` — Error code mapping, channel isolation, message lifetime
//! - `thread_affinity` — Thread-check-before-mutation enforcement
//! - `lifecycle` — Engine create/configure/free
//! - `execution` — run/step/assert/retract
//! - `copy_to_buffer` — Truncation, size query, edge cases
//! - `diagnostic_parity` — Phase 4 diagnostics through FFI unchanged
//! - `build_matrix` — Artifact build verification across profiles (Pass 009)

#[cfg(test)]
mod test_helpers;

#[cfg(test)]
mod header;

#[cfg(test)]
mod error_model;

#[cfg(test)]
mod lifecycle;

#[cfg(test)]
mod execution;

#[cfg(test)]
mod action_diagnostics;

#[cfg(test)]
mod copy_error;

#[cfg(test)]
mod values;

#[cfg(test)]
mod build_matrix;

#[cfg(test)]
mod diagnostic_parity;

#[cfg(test)]
mod contract_lock;
