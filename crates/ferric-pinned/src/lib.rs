//! # Ferric Pinned
//!
//! Rust-owned pinned-execution layer for the Ferric rules engine.
//!
//! [`PinnedEngine`] owns one dedicated worker thread and one [`ferric_runtime::Engine`].
//! Callers from any thread submit work through a serialized FIFO request queue;
//! the worker drains the queue in batches, optionally wrapping each batch or
//! each item in an Apple `autoreleasepool` (on Apple platforms).
//!
//! ## Design summary
//!
//! - The handle is `Send + Sync`. The underlying `Engine` never leaves the
//!   worker thread, so its `!Send + !Sync` invariants are preserved.
//! - Requests are erased into `Box<dyn FnOnce(&mut Engine) + Send + 'static>`.
//!   Typed operations (`run`, `load_str`, …) are thin wrappers that construct
//!   the closure and ship the typed `Result` back through a per-request
//!   oneshot.
//! - [`PinnedEngine::halt`] flips the active run's cancellation token; the
//!   worker bounds [`Engine::run`] into 64-firing chunks and checks the token
//!   between chunks. Idle calls are non-latching no-ops.
//! - Autorelease policy (`None` / `PerItem` / `PerBatch`) is a no-op on
//!   non-Apple platforms.

pub mod autorelease;
pub mod engine;
pub mod error;
pub mod options;
pub(crate) mod request;
pub(crate) mod worker;

pub use autorelease::AutoreleasePolicy;
pub use engine::{PinnedEngine, PreDispatchCancelToken};
pub use error::PinnedError;
pub use options::PinnedEngineOptions;

pub use ferric_runtime::{EngineConfig, FiredRule, HaltReason, LoadResult, RunLimit, RunResult};
