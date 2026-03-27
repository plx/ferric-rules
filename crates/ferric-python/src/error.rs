//! Exception hierarchy for ferric Python bindings.

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

use ferric_runtime::engine::EngineError;
use ferric_runtime::loader::LoadError;
use ferric_runtime::InitError;

// Exception hierarchy: FerricError (base) with specific subtypes.
create_exception!(
    ferric,
    FerricError,
    PyException,
    "Base exception for all ferric errors."
);
create_exception!(
    ferric,
    FerricParseError,
    FerricError,
    "Error parsing CLIPS source."
);
create_exception!(
    ferric,
    FerricCompileError,
    FerricError,
    "Error compiling a rule."
);
create_exception!(
    ferric,
    FerricRuntimeError,
    FerricError,
    "Runtime engine error."
);
create_exception!(
    ferric,
    FerricFactNotFoundError,
    FerricError,
    "Requested fact does not exist."
);
create_exception!(
    ferric,
    FerricModuleNotFoundError,
    FerricError,
    "Requested module does not exist."
);
create_exception!(
    ferric,
    FerricEncodingError,
    FerricError,
    "String encoding constraint violated."
);
create_exception!(
    ferric,
    FerricTemplateNotFoundError,
    FerricError,
    "Requested template does not exist."
);
create_exception!(
    ferric,
    FerricSlotNotFoundError,
    FerricError,
    "Requested slot does not exist in template."
);

/// Convert an `EngineError` into a Python exception.
pub fn engine_error_to_pyerr(err: EngineError) -> PyErr {
    match err {
        EngineError::WrongThread { .. } | EngineError::NotATemplateFact(_) => {
            FerricRuntimeError::new_err(err.to_string())
        }
        EngineError::FactNotFound(_) => FerricFactNotFoundError::new_err(err.to_string()),
        EngineError::Encoding(_) => FerricEncodingError::new_err(err.to_string()),
        EngineError::ModuleNotFound(_) => FerricModuleNotFoundError::new_err(err.to_string()),
        EngineError::TemplateNotFound(_) => FerricTemplateNotFoundError::new_err(err.to_string()),
        EngineError::SlotNotFound { .. } => FerricSlotNotFoundError::new_err(err.to_string()),
    }
}

/// Convert a `Vec<LoadError>` into a single Python exception.
///
/// All error messages are joined into the exception message so no
/// diagnostics are lost.  The exception *type* is determined by the
/// first parse or compile error encountered (parse takes precedence).
pub fn load_errors_to_pyerr(errors: Vec<LoadError>) -> PyErr {
    let msg = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    // Classify by scanning for parse errors first, then compile errors.
    let has_parse = errors.iter().any(|e| matches!(e, LoadError::Parse(_)));
    let has_compile = errors.iter().any(|e| matches!(e, LoadError::Compile(_)));

    if has_parse {
        FerricParseError::new_err(msg)
    } else if has_compile {
        FerricCompileError::new_err(msg)
    } else {
        FerricError::new_err(msg)
    }
}

/// Convert an `InitError` into a Python exception.
pub fn init_error_to_pyerr(err: InitError) -> PyErr {
    match err {
        InitError::Load(errors) => load_errors_to_pyerr(errors),
        InitError::Reset(engine_err) => engine_error_to_pyerr(engine_err),
    }
}

/// Register exception types on the module.
pub fn register_exceptions(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("FerricError", m.py().get_type::<FerricError>())?;
    m.add("FerricParseError", m.py().get_type::<FerricParseError>())?;
    m.add(
        "FerricCompileError",
        m.py().get_type::<FerricCompileError>(),
    )?;
    m.add(
        "FerricRuntimeError",
        m.py().get_type::<FerricRuntimeError>(),
    )?;
    m.add(
        "FerricFactNotFoundError",
        m.py().get_type::<FerricFactNotFoundError>(),
    )?;
    m.add(
        "FerricModuleNotFoundError",
        m.py().get_type::<FerricModuleNotFoundError>(),
    )?;
    m.add(
        "FerricEncodingError",
        m.py().get_type::<FerricEncodingError>(),
    )?;
    m.add(
        "FerricTemplateNotFoundError",
        m.py().get_type::<FerricTemplateNotFoundError>(),
    )?;
    m.add(
        "FerricSlotNotFoundError",
        m.py().get_type::<FerricSlotNotFoundError>(),
    )?;
    Ok(())
}
