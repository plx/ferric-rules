//! Apple autorelease-pool integration for the pinned worker.
//!
//! On Apple platforms (macOS / iOS / tvOS / watchOS / visionOS), [`wrap`]
//! invokes [`objc2::rc::autoreleasepool`]; on every other target it is a
//! straight-through call. The [`AutoreleasePolicy`] decides where the worker
//! installs pools (per item, per batch, or not at all).

#[cfg(test)]
use std::sync::atomic::AtomicUsize;
#[cfg(test)]
use std::sync::atomic::Ordering;

/// Apple autorelease-pool installation policy.
///
/// Maps to the FFI enum [`FerricPinnedAutoreleasePolicy`] one-for-one.
///
/// [`FerricPinnedAutoreleasePolicy`]: ../../../ferric_ffi/pinned/enum.FerricPinnedAutoreleasePolicy.html
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum AutoreleasePolicy {
    /// Do not install an autorelease pool. Recommended for non-Apple workloads
    /// or when the host language manages pools itself.
    #[default]
    None,
    /// Install one pool per drained request.
    PerItem,
    /// Install one pool per drained batch.
    PerBatch,
}

#[cfg(target_vendor = "apple")]
#[inline]
fn apple_wrap<R>(f: impl FnOnce() -> R) -> R {
    objc2::rc::autoreleasepool(|_pool| f())
}

#[cfg(not(target_vendor = "apple"))]
#[inline]
fn apple_wrap<R>(f: impl FnOnce() -> R) -> R {
    f()
}

/// Counter incremented every time [`wrap`] is invoked. Exists only in test
/// builds so unit tests can verify the worker installs pools at the right
/// granularity.
#[cfg(test)]
pub(crate) static WRAP_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn wrap_count() -> usize {
    WRAP_COUNT.load(Ordering::Relaxed)
}

/// Test-only mutex that all tests touching [`WRAP_COUNT`] must hold while
/// they observe the counter. Without this, parallel unit tests stomp on
/// each other's deltas.
#[cfg(test)]
pub(crate) static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Run `f` inside an Apple autorelease pool on Apple targets; run `f`
/// directly on every other target.
#[inline]
pub(crate) fn wrap<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(test)]
    WRAP_COUNT.fetch_add(1, Ordering::Relaxed);
    apple_wrap(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_returns_inner_value() {
        let _g = TEST_MUTEX.lock().unwrap();
        let result = wrap(|| 42_u32);
        assert_eq!(result, 42);
    }

    #[test]
    fn wrap_increments_counter_under_cfg_test() {
        let _g = TEST_MUTEX.lock().unwrap();
        let before = wrap_count();
        wrap(|| ());
        wrap(|| ());
        wrap(|| ());
        assert_eq!(wrap_count() - before, 3);
    }
}
