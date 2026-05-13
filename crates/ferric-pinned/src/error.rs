//! Error type for the pinned-execution layer.

use std::error::Error;
use std::fmt;

use ferric_runtime::engine::EngineError;
use ferric_runtime::loader::LoadError;
use ferric_runtime::InitError;

#[cfg(feature = "serde")]
use ferric_runtime::SerializationError;

/// Error returned by [`PinnedEngine`](crate::PinnedEngine) operations.
///
/// The four `Pinned*`-style variants describe scheduling failures owned by
/// this crate; the rest wrap errors from the underlying engine.
#[derive(Debug)]
pub enum PinnedError {
    /// Handle has been closed and no longer accepts requests.
    Closed,
    /// Request was canceled before completion. (Reserved; v1 only emits this
    /// for pre-dispatch cancellation if a future revision adds it.)
    Canceled,
    /// Bounded request queue is at capacity.
    QueueFull,
    /// Worker thread is unreachable (panicked or terminated unexpectedly).
    DispatchFailed,
    /// Engine construction failed.
    Init(InitError),
    /// `load_str` produced one or more parse/compile errors.
    Load(Vec<LoadError>),
    /// A runtime engine operation returned an error.
    Engine(EngineError),
    /// Serialization or deserialization failed. Only available with the `serde` feature.
    #[cfg(feature = "serde")]
    Serialization(SerializationError),
}

impl fmt::Display for PinnedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("pinned engine handle is closed"),
            Self::Canceled => f.write_str("request canceled before completion"),
            Self::QueueFull => f.write_str("pinned engine request queue is full"),
            Self::DispatchFailed => f.write_str("pinned engine worker stopped unexpectedly"),
            Self::Init(e) => write!(f, "engine construction failed: {e}"),
            Self::Load(errors) => {
                write!(f, "load failed with {} error(s)", errors.len())?;
                if let Some(first) = errors.first() {
                    write!(f, ": {first}")?;
                }
                Ok(())
            }
            Self::Engine(e) => write!(f, "engine error: {e}"),
            #[cfg(feature = "serde")]
            Self::Serialization(e) => write!(f, "serialization error: {e}"),
        }
    }
}

impl Error for PinnedError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Init(e) => Some(e),
            Self::Engine(e) => Some(e),
            Self::Load(errors) => errors.first().map(|e| e as &(dyn Error + 'static)),
            #[cfg(feature = "serde")]
            Self::Serialization(e) => Some(e),
            Self::Closed | Self::Canceled | Self::QueueFull | Self::DispatchFailed => None,
        }
    }
}

impl From<EngineError> for PinnedError {
    fn from(e: EngineError) -> Self {
        Self::Engine(e)
    }
}

impl From<Vec<LoadError>> for PinnedError {
    fn from(e: Vec<LoadError>) -> Self {
        Self::Load(e)
    }
}

impl From<InitError> for PinnedError {
    fn from(e: InitError) -> Self {
        Self::Init(e)
    }
}

#[cfg(feature = "serde")]
impl From<SerializationError> for PinnedError {
    fn from(e: SerializationError) -> Self {
        Self::Serialization(e)
    }
}
