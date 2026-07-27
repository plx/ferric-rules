/// Conditionally enter a tracing span.
///
/// When the `tracing` feature is disabled, this compiles to nothing.
/// When enabled, creates and enters a span at the specified level.
macro_rules! ferric_span {
    ($level:ident, $name:expr) => {
        #[cfg(feature = "tracing")]
        let _ferric_span = tracing::$level!($name).entered();
    };
    ($level:ident, $name:expr, $($field:tt)*) => {
        #[cfg(feature = "tracing")]
        let _ferric_span = tracing::$level!($name, $($field)*).entered();
    };
}

/// Conditionally emit a tracing event.
#[allow(unused_macros)]
macro_rules! ferric_event {
    ($level:ident, $($arg:tt)*) => {
        #[cfg(feature = "tracing")]
        tracing::$level!($($arg)*);
    };
}

#[allow(unused_imports)]
pub(crate) use ferric_event;
pub(crate) use ferric_span;
