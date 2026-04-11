//! Error conversion from Ferric engine errors to napi errors.

use napi::{Error, Status};

use ferric_runtime::engine::EngineError;
use ferric_runtime::loader::LoadError;
use ferric_runtime::InitError;

/// Convert an [`EngineError`] to a napi [`Error`].
pub fn engine_error_to_napi(err: EngineError) -> Error {
    let msg = match &err {
        EngineError::FactNotFound(_) => {
            format!("FerricFactNotFoundError: {err}")
        }
        EngineError::TemplateNotFound(_) => {
            format!("FerricTemplateNotFoundError: {err}")
        }
        EngineError::SlotNotFound { .. } => {
            format!("FerricSlotNotFoundError: {err}")
        }
        EngineError::ModuleNotFound(_) => {
            format!("FerricModuleNotFoundError: {err}")
        }
        EngineError::Encoding(_) => {
            format!("FerricEncodingError: {err}")
        }
        EngineError::WrongThread { .. } | EngineError::NotATemplateFact(_) => {
            format!("FerricRuntimeError: {err}")
        }
    };
    Error::new(Status::GenericFailure, msg)
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

    let has_parse = errors.iter().any(|e| matches!(e, LoadError::Parse(_)));
    let has_compile = errors.iter().any(|e| matches!(e, LoadError::Compile(_)));

    let prefix = if has_parse {
        "FerricParseError"
    } else if has_compile {
        "FerricCompileError"
    } else {
        "FerricError"
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

/// Convert a serialization error to a napi [`Error`].
#[cfg(feature = "serde")]
pub fn serde_error_to_napi(err: ferric_runtime::SerializationError) -> Error {
    Error::new(
        Status::GenericFailure,
        format!("FerricSerializationError: {err}"),
    )
}
