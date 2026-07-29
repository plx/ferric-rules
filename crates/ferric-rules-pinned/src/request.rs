//! Type-erased request envelopes shipped to the worker.

use std::panic::{catch_unwind, AssertUnwindSafe};

use ferric_rules_runtime::Engine;

/// A unit of work submitted to the pinned worker.
///
/// Completion-aware requests keep the fallible operation separate from their
/// terminal callback. That separation lets the envelope convert an operation
/// panic into a typed error before consuming the callback exactly once.
pub(crate) struct Request {
    dispatch: Box<dyn FnOnce(&mut Engine) + Send + 'static>,
}

impl Request {
    pub(crate) fn new<F>(operation: F) -> Self
    where
        F: FnOnce(&mut Engine) + Send + 'static,
    {
        Self {
            dispatch: Box::new(operation),
        }
    }

    pub(crate) fn with_completion<T, F, C>(operation: F, completion: C) -> Self
    where
        T: Send + 'static,
        F: FnOnce(&mut Engine) -> Result<T, crate::PinnedError> + Send + 'static,
        C: FnOnce(Result<T, crate::PinnedError>) + Send + 'static,
    {
        Self::new(move |engine| {
            let result = match catch_unwind(AssertUnwindSafe(|| operation(engine))) {
                Ok(result) => result,
                Err(payload) => {
                    discard_panic_payload(payload);
                    Err(crate::PinnedError::Internal)
                }
            };
            completion(result);
        })
    }

    pub(crate) fn dispatch(self, engine: &mut Engine) {
        (self.dispatch)(engine);
    }
}

/// Dispose of a caught payload without allowing an untrusted destructor to
/// terminate the worker with a secondary panic.
pub(crate) fn discard_panic_payload(payload: Box<dyn std::any::Any + Send>) {
    if let Err(secondary_payload) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
        std::mem::forget(secondary_payload);
    }
}
