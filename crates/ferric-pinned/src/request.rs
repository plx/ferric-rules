//! The opaque request type shipped from public-handle threads to the worker.

use ferric_runtime::Engine;

/// A unit of work submitted to the pinned worker.
///
/// The closure captures its inputs and a per-request oneshot sender for the
/// typed reply. The worker simply invokes it with `&mut Engine` — it does not
/// need to know what typed operation lives inside.
pub(crate) type Request = Box<dyn FnOnce(&mut Engine) + Send + 'static>;
