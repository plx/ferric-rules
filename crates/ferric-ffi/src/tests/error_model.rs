//! Tests for the FFI error model (Pass 003).

use crate::error::{
    clear_global_error, ferric_clear_error_global, ferric_last_error_global, map_engine_error,
    map_load_error, set_engine_error_global, set_global_error, with_global_error, EngineErrorState,
    FerricError,
};

#[test]
fn ferric_error_ok_is_zero() {
    assert_eq!(FerricError::Ok as i32, 0);
}

#[test]
fn ferric_error_codes_are_distinct() {
    let codes = [
        FerricError::Ok,
        FerricError::NullPointer,
        FerricError::ThreadViolation,
        FerricError::NotFound,
        FerricError::ParseError,
        FerricError::CompileError,
        FerricError::RuntimeError,
        FerricError::IoError,
        FerricError::BufferTooSmall,
        FerricError::InvalidArgument,
        FerricError::InternalError,
    ];
    let as_ints: Vec<i32> = codes.iter().map(|c| *c as i32).collect();
    let unique: std::collections::HashSet<i32> = as_ints.iter().copied().collect();
    assert_eq!(
        as_ints.len(),
        unique.len(),
        "all error codes must be distinct"
    );
}

#[test]
fn global_error_starts_empty() {
    clear_global_error();
    with_global_error(|msg| assert!(msg.is_none()));
}

#[test]
fn set_and_retrieve_global_error() {
    clear_global_error();
    set_global_error("test error message".to_string());
    with_global_error(|msg| {
        assert_eq!(msg, Some("test error message"));
    });
    clear_global_error();
}

#[test]
fn clear_global_error_removes_message() {
    set_global_error("an error".to_string());
    clear_global_error();
    with_global_error(|msg| assert!(msg.is_none()));
}

#[test]
fn global_error_overwrite() {
    set_global_error("first".to_string());
    set_global_error("second".to_string());
    with_global_error(|msg| assert_eq!(msg, Some("second")));
    clear_global_error();
}

#[test]
fn engine_error_state_starts_empty() {
    let state = EngineErrorState::new();
    assert!(state.message().is_none());
}

#[test]
fn engine_error_state_set_and_read() {
    let mut state = EngineErrorState::new();
    state.set("engine error".to_string());
    assert_eq!(state.message(), Some("engine error"));
}

#[test]
fn engine_error_state_clear() {
    let mut state = EngineErrorState::new();
    state.set("error".to_string());
    state.clear();
    assert!(state.message().is_none());
}

#[test]
fn map_engine_wrong_thread() {
    use ferric_runtime::engine::EngineError;
    let err = EngineError::WrongThread {
        creator: std::thread::current().id(),
        current: std::thread::current().id(),
    };
    assert_eq!(map_engine_error(&err), FerricError::ThreadViolation);
}

#[test]
fn map_engine_module_not_found() {
    use ferric_runtime::engine::EngineError;
    let err = EngineError::ModuleNotFound("MAIN".to_string());
    assert_eq!(map_engine_error(&err), FerricError::NotFound);
}

#[test]
fn map_load_parse_error() {
    use ferric_runtime::loader::LoadError;
    let err = LoadError::Parse("unexpected token".to_string());
    assert_eq!(map_load_error(&err), FerricError::ParseError);
}

#[test]
fn map_load_compile_error() {
    use ferric_runtime::loader::LoadError;
    let err = LoadError::Compile("invalid pattern".to_string());
    assert_eq!(map_load_error(&err), FerricError::CompileError);
}

#[test]
fn map_load_io_error() {
    use ferric_runtime::loader::LoadError;
    let err = LoadError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "file not found",
    ));
    assert_eq!(map_load_error(&err), FerricError::IoError);
}

#[test]
fn global_error_c_api_null_when_empty() {
    clear_global_error();
    unsafe {
        let ptr = ferric_last_error_global();
        assert!(ptr.is_null());
    }
}

#[test]
fn global_error_c_api_returns_message() {
    set_global_error("hello from FFI".to_string());
    unsafe {
        let ptr = ferric_last_error_global();
        assert!(!ptr.is_null());
        let cstr = std::ffi::CStr::from_ptr(ptr);
        assert_eq!(cstr.to_str().unwrap(), "hello from FFI");
    }
    clear_global_error();
}

#[test]
fn global_clear_c_api_works() {
    set_global_error("some error".to_string());
    ferric_clear_error_global();
    unsafe {
        let ptr = ferric_last_error_global();
        assert!(ptr.is_null());
    }
}

#[test]
fn error_channels_are_independent() {
    // Global and per-engine errors should not interfere with each other.
    clear_global_error();
    let mut state = EngineErrorState::new();

    set_global_error("global error".to_string());
    state.set("engine error".to_string());

    with_global_error(|msg| assert_eq!(msg, Some("global error")));
    assert_eq!(state.message(), Some("engine error"));

    clear_global_error();
    assert!(state.message().is_some()); // engine error unaffected

    state.clear();
    with_global_error(|msg| assert!(msg.is_none())); // global still clear
}

#[test]
fn set_engine_error_global_stores_and_maps() {
    use ferric_runtime::engine::EngineError;
    clear_global_error();
    let err = EngineError::ModuleNotFound("MISSING".to_string());
    let code = set_engine_error_global(&err);
    assert_eq!(code, FerricError::NotFound);
    with_global_error(|msg| {
        let msg = msg.unwrap();
        assert!(
            msg.contains("MISSING"),
            "error message should contain module name"
        );
    });
    clear_global_error();
}
