//! Error conversion from Ferric engine errors to napi errors.

use napi::{Error, Status};

use ferric_rules_runtime::engine::EngineError;
use ferric_rules_runtime::loader::LoadError;
use ferric_rules_runtime::InitError;

fn engine_error_class(err: &EngineError) -> &'static str {
    match err {
        EngineError::FactNotFound(_) => "FerricFactNotFoundError",
        EngineError::TemplateNotFound(_) => "FerricTemplateNotFoundError",
        EngineError::SlotNotFound { .. } => "FerricSlotNotFoundError",
        EngineError::ModuleNotFound(_) => "FerricModuleNotFoundError",
        EngineError::Encoding(_) => "FerricEncodingError",
        EngineError::WrongThread { .. } | EngineError::NotATemplateFact(_)
        | EngineError::SlotCountMismatch { .. }
        | EngineError::DuplicateSlot { .. }
        | EngineError::InvalidSlotValue { .. }
        | EngineError::ProtectedInitialFact => "FerricRuntimeError",
    }
}

/// Convert an [`EngineError`] to a napi [`Error`].
pub fn engine_error_to_napi(err: EngineError) -> Error {
    Error::new(
        Status::GenericFailure,
        format!("{}: {err}", engine_error_class(&err)),
    )
}

fn load_error_class(err: &LoadError) -> &'static str {
    match err {
        LoadError::Parse(_) | LoadError::Interpret(_) => "FerricParseError",
        LoadError::UnsupportedForm { .. }
        | LoadError::InvalidAssert(_)
        | LoadError::InvalidDefrule(_)
        | LoadError::Compile(_)
        | LoadError::Validation(_) | LoadError::ResourceLimit { .. } => "FerricCompileError",
        LoadError::Engine(error) => engine_error_class(error),
        LoadError::Io(_) => "FerricIOError",
    }
}

/// Convert a `Vec<LoadError>` to a napi [`Error`].
///
/// All error messages are joined so no diagnostics are lost. The error code
/// prefix indicates parse vs. compile errors.
pub fn load_errors_to_napi(errors: Vec<LoadError>) -> Error {
    let msg = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    let classes = errors.iter().map(load_error_class).collect::<Vec<_>>();
    // Preserve parse-before-compile precedence for mixed diagnostics. Every
    // known variant has a category; an empty error list remains generic.
    let prefix = if classes.contains(&"FerricParseError") {
        "FerricParseError"
    } else if classes.contains(&"FerricCompileError") {
        "FerricCompileError"
    } else {
        classes.first().copied().unwrap_or("FerricError")
    };

    Error::new(Status::GenericFailure, format!("{prefix}: {msg}"))
}

/// Convert an [`InitError`] to a napi [`Error`].
pub fn init_error_to_napi(err: InitError) -> Error {
    match err {
        InitError::Load(errors) => load_errors_to_napi(errors),
        InitError::Reset(engine_err) => engine_error_to_napi(engine_err),
    }
}

/// Preserve file failures from snapshot convenience methods as typed I/O errors.
#[cfg(feature = "serde")]
pub fn io_error_to_napi(err: std::io::Error) -> Error {
    Error::new(Status::GenericFailure, format!("FerricIOError: {err}"))
}

/// Convert a serialization error to a napi [`Error`].
#[cfg(feature = "serde")]
pub fn serde_error_to_napi(err: ferric_rules_runtime::SerializationError) -> Error {
    Error::new(
        Status::GenericFailure,
        format!("FerricSerializationError: {err}"),
    )
}
